//! MX-18: fila de saída gravada do `WhatsApp`. Sobrevive a reinício, deduplica por id
//! determinístico e só envia resposta a mensagem que já chegou neste número.
//!
//! Estados: `pending`/`held` (espera: silêncio, limite, ritmo, conexão) → `sending`
//! (gravado ANTES do envio: lease vivo, nunca reenviável) → `sent`;
//! `uncertain` (o wacli pode ter enviado ou a gravação falhou: nunca reenvia sozinho,
//! o dono decide); `expired` (passou a validade de 24 h) e `cancelled` (revogação,
//! troca de número, plugin desligado). Telefone e texto nunca vão para log.
use super::{
    pace::{self, Slot},
    send::{self, Turn},
    store::{BotSettings, DAY_MS, ReplyMode, bot_row, chat_marked, enabled, group_auto, hub_id},
    wacli::Sent,
};
use crate::{
    LockExt,
    fork::plugins::whatsapp_grants::{self, Read},
    hub::Hub,
    store::now_ms,
};
use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, TimeZone, Timelike, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::Notify;

const VALIDITY_MS: i64 = DAY_MS;
const LEASE_MS: i64 = 120_000;
const NOT_CONNECTED_MS: i64 = 10_000;
const NOT_INSTALLED_MS: i64 = 60_000;
const RETRY_BASE_MS: i64 = 5_000;
const RETRY_MAX_MS: i64 = 300_000;
const SENT_KEEP_MS: i64 = 25 * 3_600_000;
const TERMINAL_KEEP_MS: i64 = DAY_MS;
const UNCERTAIN_KEEP_MS: i64 = 7 * DAY_MS;

/// Instrumentação de teste: valor de `PRAGMA synchronous` visto dentro da transação do claim.
#[cfg(test)]
pub(super) static CLAIM_SYNCHRONOUS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(-1);

/// De onde veio a permissão de envio. `owner_direct` fica sempre recusado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    OwnerCard,
    BotHuman,
    BotRoutine,
    OwnerDirect,
}

impl Origin {
    fn wire(self) -> &'static str {
        match self {
            Self::OwnerCard => "owner_card",
            Self::BotHuman => "bot_human",
            Self::BotRoutine => "bot_routine",
            Self::OwnerDirect => "owner_direct",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "owner_card" => Some(Self::OwnerCard),
            "bot_human" => Some(Self::BotHuman),
            "bot_routine" => Some(Self::BotRoutine),
            "owner_direct" => Some(Self::OwnerDirect),
            _ => None,
        }
    }
}

/// Pedido de entrada na fila. O texto já vem do dono (cartão) ou do rascunho do bot
/// que o dono mandou enviar; nunca de mensagem recebida (D125).
pub(crate) struct Enqueue<'a> {
    pub bot: &'a str,
    pub chat: &'a str,
    pub reply_to: &'a str,
    pub text: &'a str,
    pub origin: Origin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Enqueued {
    Inserted,
    Existing(&'static str),
}

struct Item {
    bot: String,
    account: String,
    chat: String,
    reply_to: String,
    body: String,
    origin: Origin,
    state: String,
    not_before: i64,
    attempts: i64,
    expires_at: i64,
}

/// O que a tentativa de envio já produziu neste processo. Se a gravação do
/// resultado falhar, ela fica na memória do Hub e o drain retenta gravar — o
/// item nunca volta a enviar (revisão P2: lease vencido não reenvia).
#[derive(Clone)]
pub(super) enum Attempted {
    Sent(String),
    Uncertain,
    NotStarted,
    Failed(String),
}

impl Attempted {
    fn of(outcome: &Result<Sent>) -> Self {
        match outcome {
            Ok(Sent::Yes { id }) => Self::Sent(id.clone()),
            Ok(Sent::Uncertain) => Self::Uncertain,
            Ok(Sent::NotStarted) => Self::NotStarted,
            Err(error) => Self::Failed(error.to_string()),
        }
    }
}

// MARK: entrada

fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().fold(String::with_capacity(64), |mut out, b| {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
        out
    })
}

fn digest(parts: &[&str]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for part in parts {
        h.update(part.as_bytes());
        h.update([0]);
    }
    hex(h.finalize())
}

fn item_id(bot: &str, account: &str, chat: &str, reply_to: &str, text: &str) -> String {
    let text_digest = digest(&[text]);
    digest(&[bot, account, chat, reply_to, &text_digest])
}

/// Grava o envio na fila (ou devolve o que já existe). Não envia nada.
pub(crate) fn enqueue(hub: &Hub, req: &Enqueue<'_>, now: i64) -> Result<Enqueued> {
    ensure!(req.origin != Origin::OwnerDirect, "not_allowed: envio direto continua recusado.");
    send::shape(req.chat, req.text)?;
    let mut c = hub.store.db.locked();
    let tx = c.transaction()?;
    let account: Option<String> = tx
        .query_row("SELECT account_id FROM donna_whatsapp_numbers WHERE bot_id=?1 AND state='linked'", [req.bot], |r| {
            r.get(0)
        })
        .optional()?;
    let Some(account) = account else { bail!("not_connected: o número deste bot não está conectado agora.") };
    if matches!(req.origin, Origin::BotHuman | Origin::BotRoutine) {
        let group: Option<bool> = tx.query_row(
            "SELECT is_group FROM donna_whatsapp_messages WHERE bot_id=?1 AND chat_id=?2 AND message_id=?3 AND account_id=?4 AND call_media IS NULL",
            params![req.bot, req.chat, req.reply_to, account], |r| r.get(0),
        ).optional()?;
        if req.chat.ends_with("@g.us") || group == Some(true) {
            ensure!(group == Some(true), "not_allowed: mensagem de grupo não encontrada nesta conta.");
            ensure!(
                group_auto(&tx, req.bot, req.chat, &account)?,
                "not_allowed: o envio autônomo deste grupo está desligado."
            );
        }
    }
    let id = item_id(req.bot, &account, req.chat, req.reply_to, req.text);
    let inserted = tx.execute(
        "INSERT INTO donna_whatsapp_outbox(id,bot_id,account_id,chat_id,is_group,reply_to,body,origin,state,
         reason,not_before,attempts,lease_until,last_attempt_at,sent_at,sent_id,created_at,updated_at,expires_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'pending',NULL,?9,0,NULL,NULL,NULL,NULL,?9,?9,?10)
         ON CONFLICT(id) DO NOTHING",
        params![
            id,
            req.bot,
            account,
            req.chat,
            req.chat.ends_with("@g.us"),
            req.reply_to,
            req.text,
            req.origin.wire(),
            now,
            now + VALIDITY_MS,
        ],
    )?;
    if inserted == 1 {
        tx.commit()?;
        hub.whatsapp.wake_outbox();
        return Ok(Enqueued::Inserted);
    }
    // Mesma mensagem: se o item anterior venceu ou foi cancelado, esta é uma intenção nova.
    let state: String = tx.query_row("SELECT state FROM donna_whatsapp_outbox WHERE id=?1", [&id], |r| r.get(0))?;
    let result = match state.as_str() {
        "expired" | "cancelled" => {
            tx.execute(
                "UPDATE donna_whatsapp_outbox SET state='pending',reason=NULL,not_before=?2,attempts=0,
                 lease_until=NULL,last_attempt_at=NULL,sent_at=NULL,sent_id=NULL,created_at=?2,updated_at=?2,
                 expires_at=?3 WHERE id=?1 AND state IN ('expired','cancelled')",
                params![id, now, now + VALIDITY_MS],
            )?;
            Enqueued::Inserted
        }
        "pending" | "held" | "sending" | "sent" | "uncertain" => Enqueued::Existing(match state.as_str() {
            "pending" => "pending",
            "held" => "held",
            "sent" => "sent",
            _ => "uncertain",
        }),
        _ => bail!("invalid_field: estado da fila desconhecido."),
    };
    tx.commit()?;
    hub.whatsapp.wake_outbox();
    Ok(result)
}

// MARK: cancelamentos

/// Cancela pendentes/seguradas de um bot (ou de todos) sem tocar nos terminais.
pub(super) fn cancel_pending(c: &Connection, bot: Option<&str>, reason: &str) -> Result<()> {
    let now = now_ms();
    match bot {
        Some(bot) => {
            c.execute(
                "UPDATE donna_whatsapp_outbox SET state='cancelled',reason=?2,lease_until=NULL,updated_at=?3
                 WHERE bot_id=?1 AND state IN ('pending','held')",
                params![bot, reason, now],
            )?;
        }
        None => {
            c.execute(
                "UPDATE donna_whatsapp_outbox SET state='cancelled',reason=?1,lease_until=NULL,updated_at=?2
                 WHERE state IN ('pending','held')",
                params![reason, now],
            )?;
        }
    }
    Ok(())
}

pub(super) fn cancel_all_pending(c: &Connection, reason: &str) -> Result<()> {
    cancel_pending(c, None, reason)
}

/// Depois de (re)aplicar concessões: cancela o que perdeu o "Responder" daquela conta.
pub(super) fn cancel_revoked(
    c: &Connection,
    host_id: &str,
    bot: &str,
    live: bool,
    settings: &BotSettings,
) -> Result<()> {
    let now = now_ms();
    if !live || (!settings.reply_chats && !settings.reply_groups) {
        c.execute(
            "UPDATE donna_whatsapp_outbox SET state='cancelled',reason='reply_revoked',lease_until=NULL,updated_at=?2
             WHERE bot_id=?1 AND state IN ('pending','held')",
            params![bot, now],
        )?;
        return Ok(());
    }
    let mut stmt = c.prepare(
        "SELECT id,is_group,account_id FROM donna_whatsapp_outbox WHERE bot_id=?1 AND state IN ('pending','held')",
    )?;
    let rows = stmt
        .query_map([bot], |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?, r.get::<_, String>(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (id, group, account) in rows {
        let want = if group { settings.reply_groups } else { settings.reply_chats };
        let read = if group { Read::Groups } else { Read::Chats };
        let ok = want && whatsapp_grants::can_reply(c, host_id, &account, bot, read).unwrap_or(false);
        if !ok {
            c.execute(
                "UPDATE donna_whatsapp_outbox SET state='cancelled',reason='reply_revoked',lease_until=NULL,updated_at=?2
                 WHERE id=?1",
                params![id, now],
            )?;
        }
    }
    Ok(())
}

// MARK: conferências na vez

fn current_account(c: &Connection, bot: &str) -> Result<Option<String>> {
    Ok(c.query_row("SELECT account_id FROM donna_whatsapp_numbers WHERE bot_id=?1 AND state='linked'", [bot], |r| {
        r.get(0)
    })
    .optional()?)
}

/// Todas as conferências de novo, na mesma transação que reserva o ritmo.
/// `connection` é a encarnação viva; `None` segura como `not_connected`.
fn authorize(c: &Connection, hub_id_str: &str, item: &Item, settings: &BotSettings) -> Result<()> {
    ensure!(enabled(c, &hub_id(hub_id_str)?)?, "plugin_disabled: ligue o plugin WhatsApp primeiro.");
    let current = current_account(c, &item.bot)?;
    let Some(account) = current else { bail!("not_connected: o número deste bot não está conectado agora.") };
    ensure!(account == item.account, "not_allowed: o número mudou desde que a mensagem entrou na fila.");
    let group: Option<bool> = c
        .query_row(
            "SELECT is_group FROM donna_whatsapp_messages
             WHERE bot_id=?1 AND chat_id=?2 AND message_id=?3 AND account_id=?4 AND call_media IS NULL",
            params![item.bot, item.chat, item.reply_to, &account],
            |r| r.get(0),
        )
        .optional()?;
    let Some(group) = group else {
        bail!("not_allowed: só dá para responder uma mensagem que esta conversa mandou para este número.");
    };
    match item.origin {
        // "Peço antes": o gesto do dono no cartão já é a permissão.
        Origin::OwnerCard => {}
        Origin::OwnerDirect => bail!("not_allowed: envio direto continua recusado."),
        // "Sozinho": só no modo autônomo e só em conversa que o dono marcou.
        Origin::BotHuman | Origin::BotRoutine => {
            ensure!(
                settings.reply_mode == ReplyMode::Autonomous,
                "not_allowed: o envio sozinho só vale no modo autônomo deste bot."
            );
            ensure!(
                !group || group_auto(c, &item.bot, &item.chat, &account)?,
                "not_allowed: o envio autônomo deste grupo está desligado."
            );
            ensure!(
                chat_marked(c, &item.bot, &item.chat, &account)?,
                "not_allowed: o dono não marcou esta conversa para resposta sozinha."
            );
        }
    }
    let read = if group { Read::Groups } else { Read::Chats };
    ensure!(
        whatsapp_grants::can_reply(c, hub_id_str, &account, &item.bot, read)?,
        "not_allowed: este bot não tem \"Responder\" liberado neste número."
    );
    Ok(())
}

// MARK: silêncio

fn quiet_in<T: TimeZone>(tz: &T, now: DateTime<Utc>, start: u32, end: u32) -> Option<i64> {
    let local = now.with_timezone(tz);
    let minute = i64::from(local.hour()) * 60 + i64::from(local.minute());
    let (start, end) = (i64::from(start), i64::from(end));
    let quiet = if start < end { minute >= start && minute < end } else { minute >= start || minute < end };
    if !quiet {
        return None;
    }
    let mut day = local.date_naive();
    if start > end && minute < end {
        // Já passou da meia-noite: a abertura é hoje.
    } else {
        day = day.succ_opt()?;
    }
    let end_min = u32::try_from(end).ok()?;
    let naive = day.and_hms_opt(end_min / 60, end_min % 60, 0)?;
    tz.from_local_datetime(&naive).earliest().map(|dt| dt.timestamp_millis())
}

fn quiet_until(settings: &BotSettings, now: i64) -> Option<i64> {
    if !settings.quiet_enabled || settings.quiet_start == settings.quiet_end {
        return None;
    }
    let now = DateTime::from_timestamp_millis(now)?;
    if let Some(tz) = settings.quiet_time_zone.as_deref().and_then(|s| s.parse::<chrono_tz::Tz>().ok()) {
        return quiet_in(&tz, now, settings.quiet_start, settings.quiet_end);
    }
    if let Some(tz) = crate::chat::context::owner_time_zone() {
        return quiet_in(&tz, now, settings.quiet_start, settings.quiet_end);
    }
    quiet_in(&chrono::Local, now, settings.quiet_start, settings.quiet_end)
}

// MARK: drenagem

fn load_item(c: &Connection, id: &str) -> Result<Option<Item>> {
    let row = c
        .query_row(
            "SELECT bot_id,account_id,chat_id,reply_to,body,origin,state,not_before,attempts,expires_at
             FROM donna_whatsapp_outbox WHERE id=?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, i64>(8)?,
                    r.get::<_, i64>(9)?,
                ))
            },
        )
        .optional()?;
    let Some((bot, account, chat, reply_to, body, origin, state, not_before, attempts, expires_at)) = row else {
        return Ok(None);
    };
    let Some(origin) = Origin::parse(&origin) else { bail!("invalid_field: origem da fila desconhecida.") };
    Ok(Some(Item { bot, account, chat, reply_to, body, origin, state, not_before, attempts, expires_at }))
}

fn hold(c: &Connection, id: &str, reason: &str, until: i64, now: i64) -> Result<()> {
    c.execute(
        "UPDATE donna_whatsapp_outbox SET state='held',reason=?2,not_before=?3,lease_until=NULL,updated_at=?4 WHERE id=?1",
        params![id, reason, until, now],
    )?;
    Ok(())
}

fn cancel_item(c: &Connection, id: &str, reason: &str, now: i64) -> Result<()> {
    c.execute(
        "UPDATE donna_whatsapp_outbox SET state='cancelled',reason=?2,lease_until=NULL,updated_at=?3 WHERE id=?1",
        params![id, reason, now],
    )?;
    Ok(())
}

fn expire_item(c: &Connection, id: &str, now: i64) -> Result<()> {
    c.execute(
        "UPDATE donna_whatsapp_outbox SET state='expired',reason='expired',lease_until=NULL,updated_at=?2 WHERE id=?1",
        params![id, now],
    )?;
    Ok(())
}

fn backoff_ms(attempts: i64) -> i64 {
    let shift = u32::try_from(attempts.saturating_sub(1).clamp(0, 6)).unwrap_or(0);
    (RETRY_BASE_MS << shift).min(RETRY_MAX_MS)
}

/// Um item por vez: reconfere tudo, reserva a vez na mesma transação e só então envia.
async fn process(hub: &Arc<Hub>, id: &str) -> Result<()> {
    let now = now_ms();
    let connection = {
        let c = hub.store.db.locked();
        let Some(item) = load_item(&c, id)? else { return Ok(()) };
        hub.whatsapp.connection(&item.bot)
    };
    let claimed = {
        let mut db = hub.store.db.locked();
        // SEG P1: o claim 'sending' precisa estar durável (fsync) ANTES do efeito
        // externo — queda de SO/energia não pode perder o estado e reenviar.
        // FULL só nesta transação e o valor anterior é restaurado depois do
        // commit: a conexão é compartilhada e pode já estar em FULL (bootstrap,
        // D430) — nunca rebaixar a durabilidade dos demais fluxos.
        // As gravações seguintes podem ficar como estavam: perder 'sent'/'uncertain'
        // deixa o item 'sending', que a varredura já resolve sem reenvio.
        let prior: i64 = db.pragma_query_value(None, "synchronous", |r| r.get(0))?;
        db.pragma_update(None, "synchronous", "FULL")?;
        let mut tx = db.transaction()?;
        tx.set_drop_behavior(rusqlite::DropBehavior::Rollback);
        let outcome = claim_tx(&tx, id, now, &hub.host_id, connection).and_then(|o| {
            tx.commit()?;
            Ok(o)
        });
        let _ = db.pragma_update(None, "synchronous", prior);
        outcome?
    };
    let Some(turn) = claimed else { return Ok(()) };
    let outcome = send::deliver(hub, turn).await;
    finish(hub, id, &Attempted::of(&outcome))
}

/// Corpo do claim, dentro da transação já aberta: reconfere tudo e, se liberar,
/// marca `sending` + lease. `None` = nada a enviar nesta passada.
fn claim_tx(c: &Connection, id: &str, now: i64, host_id: &str, connection: Option<(u64, u64)>) -> Result<Option<Turn>> {
    #[cfg(test)]
    {
        let v: i64 = c.pragma_query_value(None, "synchronous", |r| r.get(0))?;
        CLAIM_SYNCHRONOUS.store(v, std::sync::atomic::Ordering::Relaxed);
    }
    let Some(item) = load_item(c, id)? else { return Ok(None) };
    if !matches!(item.state.as_str(), "pending" | "held") || item.not_before > now {
        return Ok(None);
    }
    if let Some(lease) = lease_of(c, id)?
        && lease > now
    {
        return Ok(None);
    }
    if item.expires_at < now {
        expire_item(c, id, now)?;
        return Ok(None);
    }
    let settings = bot_row(c, &item.bot)?.unwrap_or_default();
    match authorize(c, host_id, &item, &settings) {
        Ok(()) => {
            let Some(connection) = connection else {
                hold(c, id, "not_connected", now + NOT_CONNECTED_MS, now)?;
                return Ok(None);
            };
            if let Some(until) = quiet_until(&settings, now) {
                hold(c, id, "quiet", until, now)?;
                return Ok(None);
            }
            match pace::reserve(c, &item.bot, &item.account, now, pace::random_jitter_pct())? {
                Slot::WaitUntil(until) => {
                    hold(c, id, "limit", until, now)?;
                    return Ok(None);
                }
                Slot::Now => {}
            }
            let attempts = item.attempts + 1;
            // 'sending' é gravado ANTES do efeito externo: o dreno só pega
            // pending/held, então nenhuma falha posterior devolve o item à fila.
            c.execute(
                "UPDATE donna_whatsapp_outbox SET state='sending',reason=NULL,attempts=?2,lease_until=?3,
                 last_attempt_at=?4,updated_at=?4 WHERE id=?1",
                params![id, attempts, now + LEASE_MS, now],
            )?;
            Ok(Some(Turn { bot: item.bot, chat: item.chat, reply_to: item.reply_to, text: item.body, connection }))
        }

        Err(error) => {
            let msg = error.to_string();
            if msg.starts_with("plugin_disabled") || msg.starts_with("not_allowed") {
                cancel_item(
                    c,
                    id,
                    if msg.starts_with("plugin_disabled") { "plugin_disabled" } else { "not_allowed" },
                    now,
                )?;
            } else {
                hold(c, id, "check", now + NOT_CONNECTED_MS, now)?;
            }
            Ok(None)
        }
    }
}

fn lease_of(c: &Connection, id: &str) -> Result<Option<i64>> {
    Ok(c.query_row("SELECT lease_until FROM donna_whatsapp_outbox WHERE id=?1", [id], |r| r.get(0))
        .optional()?
        .flatten())
}

/// Grava o resultado da tentativa. Envio confirmado (`Sent`) que não grava cai
/// para `uncertain` — decisão do dono — nunca volta a `pending` (revisão TEC).
/// Qualquer falha restante deixa o item em `sending`: o dreno não o pega e o
/// lease vencido vira `uncertain` na varredura — nunca reenvio automático.
fn finish(hub: &Arc<Hub>, id: &str, attempt: &Attempted) -> Result<()> {
    match record(hub, id, attempt) {
        Ok(()) => Ok(()),
        Err(error) => {
            if matches!(attempt, Attempted::Sent(_)) && record_uncertain(hub, id).is_ok() {
                return Ok(());
            }
            Err(error)
        }
    }
}

/// O `sent` não gravou: o efeito externo já aconteceu, então o item vira
/// `uncertain` (o dono decide "Enviou"/"Não saiu"), nunca `pending` de novo.
fn record_uncertain(hub: &Arc<Hub>, id: &str) -> Result<()> {
    #[cfg(test)]
    if hub.whatsapp.take_record_failure() {
        bail!("falha injetada: a gravação da fila falhou");
    }
    let now = now_ms();
    let mut db = hub.store.db.locked();
    let tx = db.transaction()?;
    tx.execute(
        "UPDATE donna_whatsapp_outbox SET state='uncertain',reason='uncertain',lease_until=NULL,updated_at=?2
         WHERE id=?1 AND state='sending'",
        params![id, now],
    )?;
    tx.commit()?;
    Ok(())
}

fn record(hub: &Arc<Hub>, id: &str, attempt: &Attempted) -> Result<()> {
    #[cfg(test)]
    if hub.whatsapp.take_record_failure() {
        bail!("falha injetada: a gravação da fila falhou");
    }
    let now = now_ms();
    let mut db = hub.store.db.locked();
    let tx = db.transaction()?;
    match attempt {
        Attempted::Sent(sent_id) => {
            tx.execute(
                "UPDATE donna_whatsapp_outbox SET state='sent',reason=NULL,sent_at=?2,sent_id=?3,lease_until=NULL,updated_at=?2
                 WHERE id=?1 AND state='sending'",
                params![id, now, sent_id],
            )?;
        }
        Attempted::Uncertain => {
            // Pode ter saído: nunca reenvia sozinho. Fica visível para o dono decidir.
            tx.execute(
                "UPDATE donna_whatsapp_outbox SET state='uncertain',reason='uncertain',lease_until=NULL,updated_at=?2
                 WHERE id=?1 AND state='sending'",
                params![id, now],
            )?;
        }
        Attempted::NotStarted => {
            let attempts: i64 =
                tx.query_row("SELECT attempts FROM donna_whatsapp_outbox WHERE id=?1", [id], |r| r.get(0))?;
            tx.execute(
                "UPDATE donna_whatsapp_outbox SET state='pending',reason='retry',not_before=?2,lease_until=NULL,updated_at=?3
                 WHERE id=?1 AND state='sending'",
                params![id, now + backoff_ms(attempts), now],
            )?;
        }
        Attempted::Failed(msg) => {
            let (reason, until) = if msg.starts_with("not_connected") {
                ("not_connected", now + NOT_CONNECTED_MS)
            } else if msg.starts_with("not_installed") {
                ("not_installed", now + NOT_INSTALLED_MS)
            } else {
                ("send_error", now + RETRY_BASE_MS)
            };
            tx.execute(
                "UPDATE donna_whatsapp_outbox SET state='held',reason=?2,not_before=?3,lease_until=NULL,updated_at=?4
                 WHERE id=?1 AND state='sending'",
                params![id, reason, until, now],
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}

fn expire_due(c: &Connection, now: i64) -> Result<()> {
    c.execute(
        "UPDATE donna_whatsapp_outbox SET state='expired',reason='expired',lease_until=NULL,updated_at=?1
         WHERE state IN ('pending','held') AND expires_at<?1",
        [now],
    )?;
    Ok(())
}

fn cleanup(c: &Connection, now: i64) -> Result<()> {
    expire_due(c, now)?;
    // 'sending' com lease vencido: a tentativa pode ter saído → incerto, nunca reenvio.
    c.execute(
        "UPDATE donna_whatsapp_outbox SET state='uncertain',reason='uncertain',lease_until=NULL,updated_at=?1
         WHERE state='sending' AND (lease_until IS NULL OR lease_until<?1)",
        [now],
    )?;
    c.execute("DELETE FROM donna_whatsapp_outbox WHERE state='sent' AND sent_at<?1", [now - SENT_KEEP_MS])?;
    c.execute(
        "DELETE FROM donna_whatsapp_outbox WHERE state IN ('expired','cancelled') AND updated_at<?1",
        [now - TERMINAL_KEEP_MS],
    )?;
    c.execute(
        "DELETE FROM donna_whatsapp_outbox WHERE state='uncertain' AND updated_at<?1",
        [now - UNCERTAIN_KEEP_MS],
    )?;
    Ok(())
}

/// Reinício: o que estava em voo ('sending') ou com lease pode ter saído → incerto;
/// o resto volta a esperar.
pub(super) fn recover(store: &crate::store::Store) -> Result<()> {
    let now = now_ms();
    let mut db = store.db.locked();
    let tx = db.transaction()?;
    cleanup(&tx, now)?;
    tx.execute(
        "UPDATE donna_whatsapp_outbox SET state='uncertain',reason='restart',lease_until=NULL,updated_at=?1
         WHERE state='sending' OR (state IN ('pending','held') AND lease_until IS NOT NULL)",
        [now],
    )?;
    tx.commit()?;
    Ok(())
}

fn next_due(c: &Connection) -> Result<Option<i64>> {
    Ok(c.query_row("SELECT MIN(not_before) FROM donna_whatsapp_outbox WHERE state IN ('pending','held')", [], |r| {
        r.get(0)
    })
    .optional()?
    .flatten())
}

async fn drain(hub: Arc<Hub>, notify: Arc<Notify>) {
    loop {
        if hub.whatsapp.closing() {
            break;
        }
        let now = now_ms();
        let due = {
            let c = hub.store.db.locked();
            let _ = cleanup(&c, now);
            c.query_row(
                "SELECT id FROM donna_whatsapp_outbox
                 WHERE state IN ('pending','held') AND not_before<=?1
                 ORDER BY not_before,created_at LIMIT 1",
                [now],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .ok()
            .flatten()
        };
        if let Some(id) = due {
            if let Err(error) = process(&hub, &id).await {
                tracing::warn!(error = %error, "whatsapp: falha ao processar a fila de saída");
            }
            continue;
        }
        let wait = {
            let c = hub.store.db.locked();
            next_due(&c).ok().flatten().map(|t| (t - now).max(0).cast_unsigned())
        };
        let sleep = tokio::time::sleep(std::time::Duration::from_millis(wait.unwrap_or(60_000).min(60_000)));
        tokio::select! {
            () = notify.notified() => {}
            () = sleep => {}
        }
    }
}

pub(super) fn start(hub: &Arc<Hub>) {
    if let Err(error) = recover(&hub.store) {
        tracing::warn!(error = %error, "whatsapp: não deu para restaurar a fila de saída");
    }
    let notify = Arc::new(Notify::new());
    hub.whatsapp.set_outbox_notify(notify.clone());
    tokio::spawn(drain(hub.clone(), notify));
}

// MARK: leitura e decisão do dono

fn item_json(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": row.get::<_, String>(0)?,
        "botId": row.get::<_, String>(1)?,
        "chatId": row.get::<_, String>(2)?,
        "isGroup": row.get::<_, bool>(3)?,
        "replyTo": row.get::<_, String>(4)?,
        "text": row.get::<_, String>(5)?,
        "origin": row.get::<_, String>(6)?,
        "state": row.get::<_, String>(7)?,
        "reason": row.get::<_, Option<String>>(8)?,
        "notBefore": row.get::<_, i64>(9)?,
        "attempts": row.get::<_, i64>(10)?,
        "sentAt": row.get::<_, Option<i64>>(11)?,
        "createdAt": row.get::<_, i64>(12)?,
    }))
}

const ITEM_COLS: &str =
    "id,bot_id,chat_id,is_group,reply_to,body,origin,state,reason,not_before,attempts,sent_at,created_at";

/// Fila visível do dono: o que espera, o segurado e o incerto (nunca reenvia sozinho).
pub(crate) fn list(store: &crate::store::Store, bot: &str) -> Result<Value> {
    let now = now_ms();
    let c = store.db.locked();
    let mut stmt = c.prepare(&format!(
        "SELECT {ITEM_COLS} FROM donna_whatsapp_outbox
         WHERE bot_id=?1 AND (state IN ('pending','held','uncertain')
            OR (state='sent' AND sent_at>?2))
         ORDER BY created_at DESC LIMIT 100"
    ))?;
    let rows = stmt.query_map(params![bot, now - 3_600_000], item_json)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(json!({"items": rows}))
}

/// O dono decide o incerto: `sent` (confirma que saiu) ou `cancelled` (descarta).
/// Pendente/segurada também pode ser cancelada pelo dono.
pub(crate) fn decide(store: &crate::store::Store, bot: &str, id: &str, decision: &str) -> Result<Value> {
    let now = now_ms();
    // D368: a decisão do dono grava pela autoridade original do pedido.
    crate::fork::donna_fixa::pedido::gravar(store, |tx| {
        let (state,): (String,) = tx
            .query_row("SELECT state FROM donna_whatsapp_outbox WHERE id=?1 AND bot_id=?2", params![id, bot], |r| {
                Ok((r.get(0)?,))
            })
            .optional()?
            .context("not_found: item da fila não encontrado.")?;
        match (state.as_str(), decision) {
            ("uncertain", "sent") => {
                tx.execute(
                    "UPDATE donna_whatsapp_outbox SET state='sent',reason=NULL,sent_at=?3,sent_id='owner',
                     lease_until=NULL,updated_at=?3 WHERE id=?1 AND bot_id=?2",
                    params![id, bot, now],
                )?;
            }
            ("uncertain" | "pending" | "held", "cancelled") => {
                tx.execute(
                    "UPDATE donna_whatsapp_outbox SET state='cancelled',reason='owner',lease_until=NULL,updated_at=?3
                     WHERE id=?1 AND bot_id=?2",
                    params![id, bot, now],
                )?;
            }
            _ => bail!("invalid_field: decisão inválida para este item da fila."),
        }
        Ok(())
    })?;
    list(store, bot)
}
