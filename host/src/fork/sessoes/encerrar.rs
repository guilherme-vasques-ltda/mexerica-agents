//! Sessões PR5: encerrar uma extra (`sessionClose`), continuar em nova sessão (`sessionContinue`), o recomeço só da
//! principal (usado pelo `newSession` legado) e o arquivamento automático (P4/C5), todos no Hub.
//!
//! Estado persistido só `open`/`closed`, com `closed_reason` `owner`/`idle`/`bot_deleted`; "arquivada" é apresentação.
//! Toda gravação usa só a `Connection` recebida; quem chama publica depois do commit, sob `emit_lock`, que é tomado
//! ANTES da guarda Cloud → DB (ordem da base: MB → `emit_lock` → Cloud → DB), nunca dentro do callback.
use super::store::{self, SessionRow};
use super::{origin::SessionOrigin, titulo};
use crate::fork::autoridade::Autoridade;
use crate::store::{Entry, Lane, next_rev, now_ms};
use crate::{LockExt, api::devices::Caller, hub::Hub};
use anyhow::{Context, Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

/// `sessionActions`: `close` = `sessionClose`; `continue` = `sessionContinue`.
pub(crate) const ACTIONS: [&str; 2] = ["close", "continue"];
/// `sessionAutoCloseConfig` (leitura); o despacho (`fork/api.rs`) usa os literais dos quatro métodos.
const CONFIG_METHOD: &str = "sessionAutoCloseConfig";
/// Evento publicado depois de salvar a configuração.
pub(crate) const CONFIG_EVENT: &str = "sessionAutoClose";
const CONFIG_KEY: &str = "session.autoclose.v1";
/// P4: ligado por padrão, três dias sem atividade.
pub(crate) const DEFAULT_DAYS: u8 = 3;
pub(crate) const MAX_DAYS: u8 = 90;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
const SWEEP_EVERY: Duration = Duration::from_secs(60 * 60);
const CONTROL_TIMEOUT: Duration = Duration::from_secs(10);
/// Aviso na extra cuja retomada não roda mais porque ela foi encerrada (plano, R7 #2).
pub(crate) const CLOSED_BEFORE_REPLY: &str = "Esta sessão foi encerrada antes de eu conseguir responder.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CloseReason {
    Owner,
    Idle,
}

impl CloseReason {
    fn column(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Idle => "idle",
        }
    }
}

/// O que o fechamento mudou na Tx, para publicar depois do commit.
pub(crate) struct Closed {
    pub session: SessionRow,
    /// Entradas do dono que estavam na fila e foram canceladas.
    pub cancelled: Vec<String>,
    /// Todas as entradas mudadas (fila, permissões, perguntas e escolhas), para os eventos `entry`.
    pub entries: Vec<Entry>,
}

/// Estado canônico de uma extra para quem decide descartar algo: só `Closed` autoriza descartar; erro técnico de
/// leitura propaga e nunca vira "encerrada".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExtraState {
    Open,
    /// Encerrada, ou fora do catálogo deste chat (não existe sessão aberta para retomar).
    Closed,
}

pub(crate) fn extra_state_on(c: &Connection, chat: &str, id: &str) -> Result<ExtraState> {
    let state: Option<String> = c
        .query_row("SELECT state FROM donna_bot_sessions WHERE id=?1 AND chat_id=?2", params![id, chat], |r| r.get(0))
        .optional()?;
    Ok(if state.as_deref() == Some("open") { ExtraState::Open } else { ExtraState::Closed })
}

/// Cerca do despacho (`claim_user_turn_on`): uma extra precisa estar aberta. `None` ou o id do chat é a principal.
pub(crate) fn assert_open_on(c: &Connection, chat: &str, session: Option<&str>) -> Result<()> {
    let Some(id) = session.filter(|id| *id != chat) else { return Ok(()) };
    let state: Option<String> = c
        .query_row("SELECT state FROM donna_bot_sessions WHERE id=?1 AND chat_id=?2", params![id, chat], |r| r.get(0))
        .optional()?;
    ensure!(state.is_some(), "unknown_session");
    ensure!(state.as_deref() == Some("open"), "session_closed");
    Ok(())
}

/// Muda `status` das entradas `kind` da sessão da `lane` que estão em `from`; devolve as entradas mudadas.
fn flip_on(c: &Connection, lane: &Lane, kind: &str, from: &str, to: &str) -> Result<Vec<Entry>> {
    let ids: Vec<String> = c
        .prepare(
            "SELECT id FROM entries WHERE bot_id=?1 AND COALESCE(session,bot_id)=?2 AND kind=?3
             AND json_extract(data,'$.status')=?4 ORDER BY seq",
        )?
        .query_map(params![lane.chat, lane.session_id(), kind, from], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut changed = Vec::new();
    for id in ids {
        c.execute(
            "UPDATE entries SET data=json_set(data,'$.status',?2), rev=?3, updated_at=?4 WHERE id=?1",
            params![id, to, next_rev(c)?, now_ms()],
        )?;
        changed.push(crate::store::entry_on(c, &id)?.ok_or_else(|| anyhow!("unknown_entry"))?);
    }
    Ok(changed)
}

/// O checkpoint de espera (`turn.waiting.<bot>`) desta lane vira tombstone (`phase: finished`) na Tx do chamador, a
/// mesma forma que `clear_waiting` grava: depois do commit, nenhuma retomada dele sobrevive a uma queda. Checkpoint de
/// outra sessão fica intacto. Conclusão durável (`completing`: resposta já produzida, ainda não publicada) também
/// fica: publicá-la é dado, não prompt novo, e a recuperação canônica dela não roda IA.
fn finish_waiting_on(c: &Connection, lane: &Lane) -> Result<()> {
    let raw: Option<String> =
        c.query_row("SELECT v FROM kv WHERE k=?1", [format!("turn.waiting.{}", lane.chat)], |r| r.get(0)).optional()?;
    let Some(raw) = raw else { return Ok(()) };
    let Ok(mut value) = serde_json::from_str::<Value>(&raw) else {
        // Ilegível: não se sabe de qual sessão é; a retomada canônica já o recusa. Não é apagado aqui.
        return Ok(());
    };
    let visible = match &value["input"]["visibleSession"] {
        Value::Null => None,
        Value::String(id) => Some(id.as_str()),
        _ => return Ok(()),
    };
    if visible != lane.session.as_deref() || value["phase"] == "finished" || value["phase"] == "completing" {
        return Ok(());
    }
    value["phase"] = json!("finished");
    c.execute("UPDATE kv SET v=?2 WHERE k=?1", params![format!("turn.waiting.{}", lane.chat), value.to_string()])?;
    Ok(())
}

/// Fechamento pedido pelo dono, na Tx da guarda: CAS, não principal, aberta; cancela a fila, os cartões de permissão,
/// as perguntas e as escolhas DESTA sessão, encerra o checkpoint de espera dela e apaga a chave do cartão de
/// continuação dela. Nenhuma outra sessão muda. O turno em memória e o slot ACP ficam com o ator, depois do commit; a
/// cerca do claim e o vínculo (`state='open'`) impedem o que chegar atrasado.
pub(crate) fn close_on(c: &Connection, chat: &str, id: &str, expected: i64) -> Result<Closed> {
    let session = store::get(c, id)?.filter(|s| s.chat_id == chat).ok_or_else(|| anyhow!("unknown_session"))?;
    ensure!(!session.principal, "principal_not_closable");
    ensure!(session.state == "open", "session_closed");
    if session.revision != expected {
        return Err(crate::remote::cloud::Conflict("session_revision_conflict").into());
    }
    let lane = Lane { chat: chat.to_owned(), session: Some(id.to_owned()) };
    let mut entries = flip_on(c, &lane, "user", "queued", "cancelled")?;
    let cancelled = entries.iter().map(|e| e.id.clone()).collect();
    entries.extend(flip_on(c, &lane, "permission", "pending", "cancelled")?);
    entries.extend(crate::fork::owner_question::withdraw_chat_on(c, chat, Some(id))?);
    entries.extend(crate::fork::genui::flow::cancel_on(c, chat, Some(id))?);
    finish_waiting_on(c, &lane)?;
    c.execute("DELETE FROM kv WHERE k=?1", [crate::chat::handoff::key_for(&lane)])?;
    let session = store::mark_closed_on(c, id, CloseReason::Owner.column())?;
    Ok(Closed { session, cancelled, entries })
}

/// O que o recomeço mudou: as entradas canceladas/retiradas e, se houver, o marcador e o cartão de passagem.
pub(crate) type Restarted = (Vec<Entry>, Option<(Option<Entry>, Entry)>);

/// Recomeço da principal (`newSession` legado), na Tx da guarda do ator: cancela só a fila, as perguntas e as
/// escolhas DA PRINCIPAL (`entries.session IS NULL`), encerra só o checkpoint de espera dela (nunca uma conclusão
/// durável) e deixa o cartão de passagem na chave legada: o rascunho existente, re-marcado como recomeço, ou o novo
/// com `card`. Extras, suas filas e esperas não mudam. Devolve as entradas mudadas e o
/// marcador/cartão novos, para publicar depois do commit.
pub(crate) fn restart_on(c: &Connection, chat: &str, card: Option<&str>, person: Option<&str>) -> Result<Restarted> {
    let main = Lane::main(chat);
    let mut entries = flip_on(c, &main, "user", "queued", "cancelled")?;
    entries.extend(crate::fork::owner_question::withdraw_chat_on(c, chat, Some(chat))?);
    entries.extend(crate::fork::genui::flow::cancel_on(c, chat, Some(chat))?);
    finish_waiting_on(c, &main)?;
    // Um rascunho que já estava na principal vira o do recomeço (motivo e linha), com o texto que o dono editou; sem
    // ele, o cartão novo com o resumo lido antes.
    let reason = crate::chat::handoff::Reason::Restart;
    let card = match crate::chat::handoff::retag_draft_on(c, &main, reason)? {
        Some(card) => Some((None, card)),
        None => card.map(|text| crate::chat::handoff::draft_on(c, &main, chat, reason, text, person)).transpose()?,
    };
    Ok((entries, card))
}

/// Atividade da sessão (L1): a criação (`last_used_at`, que a base não renova) e as entradas DELA
/// (`created_at`/`updated_at`). Leitura, renomear, trocar o motor e a configuração não gravam entrada e não contam.
fn activity_on(c: &Connection, chat: &str, id: &str) -> Result<i64> {
    Ok(c.query_row(
        "SELECT MAX(s.created_at, s.last_used_at,
            COALESCE((SELECT MAX(e.updated_at) FROM entries e WHERE e.bot_id=?2 AND e.session=?1), 0))
         FROM donna_bot_sessions s WHERE s.id=?1 AND s.chat_id=?2",
        params![id, chat],
        |r| r.get(0),
    )?)
}

fn kv_on(c: &Connection, key: &str) -> Result<Option<String>> {
    Ok(c.query_row("SELECT v FROM kv WHERE k=?1", [key], |r| r.get(0)).optional()?)
}

/// A espera persistida do bot é desta sessão e ainda vale? Ilegível conta como pendente (falha fechado: adia).
fn waiting_pending_on(c: &Connection, chat: &str, id: &str) -> Result<bool> {
    let Some(raw) = kv_on(c, &format!("turn.waiting.{chat}"))? else { return Ok(false) };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else { return Ok(true) };
    if value["phase"] == "finished" {
        return Ok(false);
    }
    match &value["input"]["visibleSession"] {
        Value::Null if crate::agent::bot::checkpoint_waiting_active(&value) => Ok(false),
        Value::String(session) if crate::agent::bot::checkpoint_waiting_active(&value) => Ok(session == id),
        // Forma que o leitor canônico não aceita: não se sabe de quem é.
        _ => Ok(true),
    }
}

/// O registro de admissão do bot (`turn.inflight.<bot>`) ocupa esta sessão? Pelo leitor canônico (`decode`): sem
/// entradas, com entrada dela, legado ou ilegível contam como pendente.
fn admission_pending_on(c: &Connection, chat: &str, id: &str) -> Result<bool> {
    let Some(raw) = kv_on(c, &format!("turn.inflight.{chat}"))? else { return Ok(false) };
    let Ok(Some(record)) = crate::fork::bot_admission::decode(&raw) else { return Ok(true) };
    if record.entry_ids.is_empty() {
        return Ok(true);
    }
    for entry in &record.entry_ids {
        let session: Option<Option<String>> =
            c.query_row("SELECT session FROM entries WHERE id=?1", [entry], |r| r.get(0)).optional()?;
        if session.flatten().as_deref() == Some(id) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Trabalho durável pendente da sessão (P4), na Tx: fila, cartão de permissão, pergunta ou escolha pendentes,
/// espera de retomada dela, admissão em andamento que a ocupa, e o cartão de continuação ainda em rascunho.
pub(super) fn pending_work_on(c: &Connection, chat: &str, id: &str) -> Result<bool> {
    let entries: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM entries WHERE bot_id=?1 AND session=?2 AND (
            (kind='user' AND json_extract(data,'$.status')='queued')
            OR (kind='permission' AND json_extract(data,'$.status')='pending')))",
        params![chat, id],
        |r| r.get(0),
    )?;
    let lane = Lane { chat: chat.to_owned(), session: Some(id.to_owned()) };
    Ok(entries
        || waiting_pending_on(c, chat, id)?
        || admission_pending_on(c, chat, id)?
        || crate::chat::handoff::draft_pending_on(c, &lane)?
        || crate::fork::owner_question::pending_in_on(c, chat, id)?
        || crate::fork::genui::flow::pending_on(c, chat, id)?)
}

/// Fechamento automático (P4), na Tx IMMEDIATE do produtor do sistema: relê a configuração e o limite, e confere de
/// novo bot agente vivo do Pessoal, extra aberta, inativa e sem trabalho pendente. Qualquer condição falha → não
/// fecha e não muda nada (nem o cartão em rascunho).
pub(crate) fn close_idle_on(c: &Connection, chat: &str, id: &str, now: i64) -> Result<Option<SessionRow>> {
    let Some(cutoff) = config_on(c)?.cutoff(now) else { return Ok(None) };
    if !store::personal_on(c, chat)? {
        return Ok(None);
    }
    let open = store::get(c, id)?.is_some_and(|s| s.chat_id == chat && !s.principal && s.state == "open");
    if !open || activity_on(c, chat, id)? > cutoff || pending_work_on(c, chat, id)? {
        return Ok(None);
    }
    store::mark_closed_on(c, id, CloseReason::Idle.column()).map(Some)
}

/// Extras abertas de bots agentes vivos do Pessoal com atividade até o limite (só leitura; a Tx do fechamento
/// reconfere tudo).
fn idle_candidates_on(c: &Connection, now: i64) -> Result<Vec<(String, String)>> {
    let Some(cutoff) = config_on(c)?.cutoff(now) else { return Ok(Vec::new()) };
    let rows: Vec<(String, String)> = c
        .prepare(
            "SELECT s.chat_id, s.id FROM donna_bot_sessions s
             WHERE s.principal=0 AND s.state='open' AND MAX(s.created_at, s.last_used_at,
                COALESCE((SELECT MAX(e.updated_at) FROM entries e WHERE e.bot_id=s.chat_id AND e.session=s.id), 0))
                <= ?1
             ORDER BY s.chat_id, s.id",
        )?
        .query_map([cutoff], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut candidates = Vec::new();
    for (chat, id) in rows {
        if store::personal_on(c, &chat)? {
            candidates.push((chat, id));
        }
    }
    Ok(candidates)
}

/// A configuração do sistema: `days` 1–90, ou `null` = Nunca (desligado). Ausente = três dias.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AutoClose {
    pub days: Option<u8>,
    pub revision: i64,
}

impl AutoClose {
    fn cutoff(self, now: i64) -> Option<i64> {
        self.days.map(|days| now - i64::from(days) * DAY_MS)
    }
}

/// Ilegível ou fora da faixa falha fechado: a varredura não fecha nada.
pub(crate) fn config_on(c: &Connection) -> Result<AutoClose> {
    let raw: Option<String> = c.query_row("SELECT v FROM kv WHERE k=?1", [CONFIG_KEY], |r| r.get(0)).optional()?;
    let Some(raw) = raw else { return Ok(AutoClose { days: Some(DEFAULT_DAYS), revision: 0 }) };
    let config: AutoClose = serde_json::from_str(&raw).context("autoclose_config_invalid")?;
    ensure!(config.days.is_none_or(|d| (1..=MAX_DAYS).contains(&d)), "autoclose_config_invalid");
    Ok(config)
}

fn save_on(c: &Connection, days: Option<u8>, expected: i64) -> Result<AutoClose> {
    ensure!(days.is_none_or(|d| (1..=MAX_DAYS).contains(&d)), "invalid_field: days deve ser de 1 a 90, ou null.");
    if config_on(c)?.revision != expected {
        return Err(crate::remote::cloud::Conflict("autoclose_revision_conflict").into());
    }
    let config = AutoClose { days, revision: next_rev(c)? };
    c.execute(
        "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=?2",
        params![CONFIG_KEY, serde_json::to_string(&config)?],
    )?;
    Ok(config)
}

fn config_view(config: AutoClose) -> Value {
    json!({"days": config.days, "revision": config.revision, "defaultDays": DEFAULT_DAYS, "maxDays": MAX_DAYS})
}

/// Publica o que o fechamento mudou, chamado ainda sob `emit_lock` e depois do commit: entradas por revisão e, por
/// último, a sessão. Perguntas: o chamador atualiza o runtime depois de soltar as travas.
pub(crate) fn publish_locked(hub: &Hub, session: Option<&SessionRow>, entries: &mut [Entry]) {
    entries.sort_by_key(|e| e.rev);
    for entry in entries.iter() {
        let _ = hub.events.send(json!({"type":"entry","rev":entry.rev,"entry":entry}));
    }
    if let Some(session) = session {
        let _ = hub.events.send(json!({"type": "session", "session": session}));
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Target {
    bot_id: String,
    session_id: String,
    expected_revision: i64,
}

fn target(body: Value, what: &str) -> Result<Target> {
    serde_json::from_value(body).map_err(|_| anyhow!("invalid_field: pedido de {what} inválido."))
}

pub(super) fn live_bot(hub: &Hub, bot: &str) -> Result<()> {
    ensure!(hub.store.bot(bot)?.is_some_and(|b| !b.deleted && !b.config.is_group()), "unknown bot");
    Ok(())
}

/// `sessionClose {botId, sessionId, expectedRevision}`. Autoridade ORIGINAL capturada aqui, antes de qualquer
/// espera, e reconferida pelo ator na Tx do fechamento; o ator é quem inicia turnos, então decide e grava sem `await`
/// no meio. Sem resposta em 10 s, `session_control_pending`: resultado incerto, o app relê `sessions`.
pub(crate) async fn close(hub: &Arc<Hub>, caller: &Caller, body: Value) -> Result<Value> {
    let request = target(body, "encerramento")?;
    let autoridade = Autoridade::capturar(hub, caller)?;
    live_bot(hub, &request.bot_id)?;
    let _guard = hub.store.team_lock.lock().await;
    let (reply, result) = tokio::sync::oneshot::channel();
    hub.send_cmd(
        &request.bot_id,
        crate::agent::bot::Cmd::SessionClose {
            session: request.session_id,
            expected: request.expected_revision,
            autoridade: Box::new(autoridade),
            reply,
        },
    )?;
    tokio::time::timeout(CONTROL_TIMEOUT, result).await.map_err(|_| anyhow!("session_control_pending"))??
}

/// `sessionContinue {botId, sessionId, expectedRevision}`: outra extra aberta, `origin` continuation, ligada à origem
/// encerrada por `source_session`, com o cartão de passagem próprio (Draft, chave por sessão) montado só das linhas da
/// origem. D1: motor e conta são o retrato do padrão atual do bot (`store::create`), nada da origem além do resumo e
/// da exposição. Não envia mensagem nem reabre a origem; cada chamada cria uma sessão.
pub(crate) async fn continue_(hub: &Arc<Hub>, caller: &Caller, body: Value) -> Result<Value> {
    let request = target(body, "continuação")?;
    let autoridade = Autoridade::capturar(hub, caller)?;
    live_bot(hub, &request.bot_id)?;
    let guard = hub.store.team_lock.clone().lock_owned().await;
    let hub = hub.clone();
    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        let chat = request.bot_id;
        // O resumo lê pelo Store (sua própria trava): montado antes da guarda, da origem que a Tx confere encerrada.
        // A ACP canônica da origem é a última ligada a ela (`donna_session_acp`, gravada na ativação e mantida depois
        // de soltar o slot); o resumo do motor dela entra se existir. Nenhuma ACP de outra sessão.
        let acp = super::exposure::last_acp(&hub.store, &chat, &request.session_id)?;
        let acp = acp.as_deref().unwrap_or_default();
        let (text, person) = crate::chat::handoff::build_in_with_person(&hub.store, &chat, &request.session_id, acp)?;
        crate::fork::donna_fixa::pedido::antes_de_gravar(&hub.store);
        let _emit = hub.emit_lock.locked();
        let (source, session, marker, card) = autoridade.with_db(&hub, None, |c| {
            let source = store::get(c, &request.session_id)?
                .filter(|s| s.chat_id == chat)
                .ok_or_else(|| anyhow!("unknown_session"))?;
            ensure!(!source.principal, "principal_not_continuable");
            ensure!(source.state == "closed", "session_open");
            if source.revision != request.expected_revision {
                return Err(crate::remote::cloud::Conflict("session_revision_conflict").into());
            }
            let created = store::create(
                c,
                &store::NewSession {
                    chat: chat.clone(),
                    origin: SessionOrigin::Continuation,
                    source_session: Some(source.id.clone()),
                    parent_session: None,
                    title: titulo::continuation(&source.title),
                    runs_on: None,
                },
            )?;
            // A continuação herda a exposição da origem: o resumo pode carregar conteúdo externo (R4 #4).
            c.execute(
                "INSERT OR IGNORE INTO donna_session_exposure(bot,session,at,reason)
                 SELECT bot, ?2, at, reason FROM donna_session_exposure WHERE bot=?3 AND session=?1",
                params![source.id, created.id, chat],
            )?;
            let lane = Lane { chat: chat.clone(), session: Some(created.id.clone()) };
            let reason = crate::chat::handoff::Reason::Continuation;
            let (marker, card) = crate::chat::handoff::draft_on(c, &lane, &chat, reason, &text, person.as_deref())?;
            let session = store::get(c, &created.id)?.ok_or_else(|| anyhow!("unknown_session"))?;
            Ok((source, session, marker, card))
        })?;
        for entry in marker.iter().chain([&card]) {
            let _ = hub.events.send(json!({"type":"entry","rev":entry.rev,"entry":entry}));
        }
        let _ = hub.events.send(json!({"type": "session", "session": session}));
        Ok(json!({"session": session, "source": source, "handoff": card}))
    })
    .await?
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Save {
    days: Option<u8>,
    expected_revision: i64,
}

/// `sessionAutoCloseConfig {}` (leitura) e `sessionAutoCloseSave {days, expectedRevision}` (dono: autoridade ORIGINAL
/// reconferida na Tx da gravação).
pub(crate) async fn config(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    if method == CONFIG_METHOD {
        ensure!(body.as_object().is_none_or(serde_json::Map::is_empty), "invalid_field: pedido sem campos.");
        return Ok(json!({"config": config_view(config_on(&hub.store.db.locked())?)}));
    }
    ensure!(body.get("days").is_some(), "invalid_field: days é obrigatório (null = Nunca).");
    let request: Save = serde_json::from_value(body).map_err(|_| anyhow!("invalid_field: configuração inválida."))?;
    let autoridade = Autoridade::capturar(hub, caller)?;
    let guard = hub.store.team_lock.clone().lock_owned().await;
    let hub = hub.clone();
    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        crate::fork::donna_fixa::pedido::antes_de_gravar(&hub.store);
        let saved = autoridade.with_db(&hub, None, |c| save_on(c, request.days, request.expected_revision))?;
        let view = config_view(saved);
        let _ = hub.events.send(json!({"type": CONFIG_EVENT, "config": view}));
        Ok(json!({"config": view}))
    })
    .await?
}

/// A varredura horária do Hub. Cada candidata vai ao ator do bot, que confere o próprio trabalho em andamento e
/// fecha numa Tx que reconfere tudo; erro ou ator ausente só adiam para a próxima passada.
pub(crate) fn start(hub: &Arc<Hub>) {
    let hub = hub.clone();
    tokio::spawn(async move {
        let mut every = tokio::time::interval(SWEEP_EVERY);
        loop {
            every.tick().await;
            if hub.routines.stopping() {
                break;
            }
            let candidates = idle_candidates_on(&hub.store.db.locked(), now_ms());
            match candidates {
                Ok(candidates) => {
                    for (chat, session) in candidates {
                        let command = crate::agent::bot::Cmd::SessionIdleClose { session };
                        if let Err(error) = hub.send_cmd(&chat, command) {
                            tracing::debug!(%error, bot = %chat, "arquivamento automático adiado");
                        }
                    }
                }
                Err(error) => tracing::warn!(error = format!("{error:#}"), "arquivamento automático não rodou"),
            }
        }
    });
}

#[cfg(test)]
pub(crate) fn idle_candidates_for_test(c: &Connection, now: i64) -> Result<Vec<(String, String)>> {
    idle_candidates_on(c, now)
}
