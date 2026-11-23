//! Ajustes do `WhatsApp` no Hub. Só o caminho do dono (api.rs) chama estas funções.
use super::number::State;
use crate::{
    LockExt,
    fork::{
        escopo::{ContextRef, Id},
        plugins::{
            self, Builtin,
            lifecycle::LifecycleChange,
            model::InstallationState,
            whatsapp_grants::{self, Access, Read},
        },
        spaces::PERSONAL,
    },
    store::{BotConfig, Store, now_ms},
    voice::Provider,
};
use anyhow::{Context, Result, bail, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;

pub(crate) fn migrate(c: &Connection) -> Result<()> {
    let tx = c.unchecked_transaction()?;
    tx.execute_batch(include_str!("schema.sql"))?;
    widen_r1_ranges(&tx)?;
    // Colunas novas em bancos antigos: R3 (número confirmado), R5c (contato, chamada, cursor)
    // e R8 (prazo de guarda).
    for (table, column, ddl) in [
        ("donna_whatsapp_numbers", "phone", "phone TEXT"),
        ("donna_whatsapp_marked", "auto", "auto INTEGER NOT NULL DEFAULT 0 CHECK(auto IN (0,1))"),
        (
            "donna_whatsapp_bots",
            "retention_days",
            "retention_days INTEGER NOT NULL DEFAULT 30 CHECK(retention_days IN (7,30,90))",
        ),
        ("donna_whatsapp_bots", "retention_rev", "retention_rev INTEGER NOT NULL DEFAULT 0 CHECK(retention_rev>=0)"),
        (
            "donna_whatsapp_messages",
            "sender_contact",
            "sender_contact INTEGER NOT NULL DEFAULT 0 CHECK(sender_contact IN (0,1))",
        ),
        ("donna_whatsapp_messages", "call_media", "call_media TEXT CHECK(call_media IN ('audio','video'))"),
        ("donna_whatsapp_messages", "call_secs", "call_secs INTEGER"),
        ("donna_whatsapp_messages", "mime", "mime TEXT"),
        ("donna_whatsapp_messages", "file_length", "file_length INTEGER"),
        ("donna_whatsapp_messages", "transcript", "transcript TEXT"),
        (
            "donna_whatsapp_messages",
            "transcript_state",
            "transcript_state TEXT CHECK(transcript_state IN ('pending','done','too_large','no_key','unsupported','failed'))",
        ),
        (
            "donna_whatsapp_messages",
            "transcript_provider",
            "transcript_provider TEXT CHECK(transcript_provider IN ('openai','gemini'))",
        ),
        ("donna_whatsapp_messages", "transcript_tries", "transcript_tries INTEGER NOT NULL DEFAULT 0"),
        (
            "donna_whatsapp_bots",
            "daily_limit",
            "daily_limit INTEGER NOT NULL DEFAULT 100 CHECK(daily_limit BETWEEN 1 AND 500)",
        ),
        (
            "donna_whatsapp_bots",
            "quiet_enabled",
            "quiet_enabled INTEGER NOT NULL DEFAULT 1 CHECK(quiet_enabled IN (0,1))",
        ),
        (
            "donna_whatsapp_bots",
            "quiet_start",
            "quiet_start INTEGER NOT NULL DEFAULT 1320 CHECK(quiet_start BETWEEN 0 AND 1439)",
        ),
        (
            "donna_whatsapp_bots",
            "quiet_end",
            "quiet_end INTEGER NOT NULL DEFAULT 480 CHECK(quiet_end BETWEEN 0 AND 1439)",
        ),
        ("donna_whatsapp_bots", "quiet_time_zone", "quiet_time_zone TEXT"),
        (
            "donna_whatsapp_cursors",
            "last_call_rowid",
            "last_call_rowid INTEGER NOT NULL DEFAULT 0 CHECK(last_call_rowid>=0)",
        ),
    ] {
        let exists: bool =
            tx.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name=?2)", [table, column], |r| {
                r.get(0)
            })?;
        if !exists {
            tx.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {ddl}"))?;
        }
    }
    widen_reply_mode(&tx)?;
    widen_outbox_states(&tx)?;
    tx.commit()?;
    Ok(())
}

/// P2/TEC: `state` da fila de saída passa a aceitar `sending` (gravado antes do
/// envio); o SQLite não troca CHECK, então a tabela é refeita quando ainda não o conhece.
fn widen_outbox_states(tx: &Connection) -> Result<()> {
    let sql: String =
        tx.query_row("SELECT sql FROM sqlite_master WHERE type='table' AND name='donna_whatsapp_outbox'", [], |r| {
            r.get(0)
        })?;
    if sql.contains("'sending'") {
        return Ok(());
    }
    // Os índices seguem a tabela no RENAME: derruba antes para o schema.sql recriá-los.
    tx.execute_batch(
        "DROP INDEX IF EXISTS donna_whatsapp_outbox_due;
         DROP INDEX IF EXISTS donna_whatsapp_outbox_bot;
         ALTER TABLE donna_whatsapp_outbox RENAME TO donna_whatsapp_outbox_ob;",
    )?;
    tx.execute_batch(&format!(
        "{};
         INSERT INTO donna_whatsapp_outbox(id,bot_id,account_id,chat_id,is_group,reply_to,body,origin,state,
             reason,not_before,attempts,lease_until,last_attempt_at,sent_at,sent_id,created_at,updated_at,
             expires_at)
         SELECT id,bot_id,account_id,chat_id,is_group,reply_to,body,origin,state,reason,not_before,attempts,
             lease_until,last_attempt_at,sent_at,sent_id,created_at,updated_at,expires_at
         FROM donna_whatsapp_outbox_ob;
         DROP TABLE donna_whatsapp_outbox_ob;",
        include_str!("schema.sql")
    ))?;
    Ok(())
}

/// MX-18: `reply_mode` passa a aceitar `autonomous`; o SQLite não troca CHECK, então a
/// tabela é refeita quando ainda só conhece `approval`. Roda depois das colunas novas.
fn widen_reply_mode(tx: &Connection) -> Result<()> {
    let sql: String =
        tx.query_row("SELECT sql FROM sqlite_master WHERE type='table' AND name='donna_whatsapp_bots'", [], |r| {
            r.get(0)
        })?;
    if !sql.contains("reply_mode='approval'") {
        return Ok(());
    }
    tx.execute_batch(&format!(
        "ALTER TABLE donna_whatsapp_bots RENAME TO donna_whatsapp_bots_rm;
         {}
         INSERT INTO donna_whatsapp_bots(bot_id,read_chats,reply_chats,read_groups,reply_groups,reply_mode,
             delay_seconds,hourly_limit,transcribe_audio,retention_days,retention_rev,daily_limit,
             quiet_enabled,quiet_start,quiet_end,quiet_time_zone)
         SELECT bot_id,read_chats,reply_chats,read_groups,reply_groups,reply_mode,delay_seconds,
             hourly_limit,transcribe_audio,retention_days,retention_rev,daily_limit,quiet_enabled,
             quiet_start,quiet_end,quiet_time_zone FROM donna_whatsapp_bots_rm;
         DROP TABLE donna_whatsapp_bots_rm;",
        include_str!("schema.sql")
    ))?;
    Ok(())
}

/// R4b (D357): as faixas do R1 (3–60 s, 1–60/h) viram 8–300 s e 1–200/h. O SQLite não troca
/// CHECK de tabela; ela é refeita. Espera de 3 a 7 s sobe para 8 s; o resto fica igual, nada desce.
fn widen_r1_ranges(tx: &Connection) -> Result<()> {
    let sql: String =
        tx.query_row("SELECT sql FROM sqlite_master WHERE type='table' AND name='donna_whatsapp_bots'", [], |r| {
            r.get(0)
        })?;
    if !sql.contains("BETWEEN 3 AND 60") {
        return Ok(());
    }
    tx.execute_batch(&format!(
        "ALTER TABLE donna_whatsapp_bots RENAME TO donna_whatsapp_bots_r1;
         {}
         INSERT INTO donna_whatsapp_bots(bot_id,read_chats,reply_chats,read_groups,reply_groups,reply_mode,
             delay_seconds,hourly_limit,transcribe_audio)
         SELECT bot_id,read_chats,reply_chats,read_groups,reply_groups,reply_mode,
             MAX(delay_seconds,{DELAY_MIN}),hourly_limit,transcribe_audio FROM donna_whatsapp_bots_r1;
         DROP TABLE donna_whatsapp_bots_r1;",
        include_str!("schema.sql")
    ))?;
    Ok(())
}

/// Regras da conexão (D357), valem para qualquer origem: tela, rotina, bot ou app antigo.
pub(crate) const DELAY_MIN: u32 = 8;
pub(crate) const DELAY_MAX: u32 = 300;
pub(crate) const HOURLY_MAX: u32 = 200;
/// MX-18: limite por dia, janela móvel de 24 h, por bot × número.
pub(crate) const DAILY_MAX: u32 = 500;

/// Dentro da transação de exclusão do bot.
pub(crate) fn on_bot_deleted(c: &Connection, id: &str) -> Result<()> {
    super::number::forget(c, id)?;
    super::messages::erase(c, id)?;
    c.execute("DELETE FROM donna_whatsapp_bots WHERE bot_id=?1", [id])?;
    c.execute("DELETE FROM donna_whatsapp_pace WHERE bot_id=?1", [id])?;
    c.execute("DELETE FROM donna_whatsapp_outbox WHERE bot_id=?1", [id])?;
    c.execute("DELETE FROM donna_whatsapp_marked WHERE bot_id=?1", [id])?;
    c.execute("DELETE FROM donna_whatsapp_numbers WHERE bot_id=?1", [id])?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Mode {
    /// Padrão até o dono escolher: o modo mais restrito.
    #[default]
    DonnaOnly,
    PerBot,
}

/// Fase 1 só tem aprovação por cartão; "responde sozinho" ainda não existe.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReplyMode {
    #[default]
    Approval,
    /// MX-18: "sozinho" — o bot responde dentro dos limites, só em conversa marcada.
    Autonomous,
}

fn reply_mode_wire(mode: ReplyMode) -> &'static str {
    match mode {
        ReplyMode::Approval => "approval",
        ReplyMode::Autonomous => "autonomous",
    }
}

#[allow(clippy::struct_excessive_bools, reason = "Cada interruptor é uma opção independente da tela.")]
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BotSettings {
    pub read_chats: bool,
    pub reply_chats: bool,
    pub read_groups: bool,
    /// Fase 1: só quando respondem a uma mensagem do bot.
    pub reply_groups: bool,
    pub reply_mode: ReplyMode,
    pub delay_seconds: u32,
    pub hourly_limit: u32,
    /// MX-18: envios por dia nesta conexão (janela móvel de 24 h).
    pub daily_limit: u32,
    /// MX-18: silêncio por número; segurado até a abertura.
    pub quiet_enabled: bool,
    /// Minutos do dia, 0–1439, no fuso do dono (ou `quiet_time_zone`).
    pub quiet_start: u32,
    pub quiet_end: u32,
    /// Fuso IANA opcional; `None` usa o do dono (`harness-context`).
    pub quiet_time_zone: Option<String>,
    pub transcribe_audio: bool,
    /// R8: dias que a caixa guarda as mensagens ([`RETENTION_DAYS`]).
    pub retention_days: u32,
}

/// Prazos que o dono escolhe (R8). Padrão 30 dias (D302).
pub(super) const RETENTION_DAYS: [u32; 3] = [7, 30, 90];
const RETENTION_DEFAULT: u32 = 30;
pub(super) const DAY_MS: i64 = 24 * 3600 * 1000;

/// Prazo do bot em ms; sem opções gravadas, o padrão.
pub(super) fn retention_ms(c: &Connection, bot: &str) -> Result<i64> {
    let days: Option<u32> = c
        .query_row("SELECT retention_days FROM donna_whatsapp_bots WHERE bot_id=?1", [bot], |r| r.get(0))
        .optional()?;
    Ok(i64::from(days.unwrap_or(RETENTION_DEFAULT)) * DAY_MS)
}

/// MX-18: a conversa está marcada para o modo "sozinho" nesta conta?
pub(super) fn chat_marked(c: &Connection, bot: &str, chat: &str, account: &str) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_whatsapp_marked WHERE bot_id=?1 AND chat_id=?2 AND account_id=?3)",
        params![bot, chat, account],
        |r| r.get(0),
    )?)
}

/// Grupo precisa de autorização explícita separada da marca.
pub(super) fn group_auto(c: &Connection, bot: &str, chat: &str, account: &str) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_whatsapp_marked WHERE bot_id=?1 AND chat_id=?2 AND account_id=?3 AND auto=1)",
        params![bot, chat, account], |r| r.get(0),
    )?)
}

/// MX-18: o dono marca/desmarca a conversa para o modo "sozinho" responder sozinho.
/// Desmarcar cancela na hora o que ainda esperava sair por essa conversa.
pub(crate) fn mark_chat(
    store: &Store,
    host_id: &str,
    numbers: &State,
    bot: &str,
    chat: &str,
    marked: bool,
) -> Result<Value> {
    let hub = hub_id(host_id)?;
    crate::fork::donna_fixa::pedido::gravar(store, |tx| {
        ensure!(enabled(tx, &hub)?, "plugin_disabled: ligue o plugin WhatsApp primeiro.");
        ensure!(agents(tx)?.iter().any(|id| id == bot), "bot_not_found: bot não encontrado.");
        ensure!(!chat.is_empty() && chat.len() <= 128, "invalid_field: conversa inválida.");
        let account: String = tx
            .query_row("SELECT account_id FROM donna_whatsapp_numbers WHERE bot_id=?1 AND state='linked'", [bot], |r| {
                r.get(0)
            })
            .optional()?
            .context("not_connected: conecte o número deste bot antes de marcar conversas.")?;
        let now = now_ms();
        if marked {
            tx.execute(
                "INSERT INTO donna_whatsapp_marked(bot_id,chat_id,account_id,marked_at) VALUES(?1,?2,?3,?4)
                 ON CONFLICT(bot_id,chat_id) DO UPDATE SET auto=CASE WHEN account_id=excluded.account_id THEN auto ELSE 0 END,account_id=excluded.account_id,marked_at=excluded.marked_at",
                params![bot, chat, account, now],
            )?;
        } else {
            tx.execute("DELETE FROM donna_whatsapp_marked WHERE bot_id=?1 AND chat_id=?2", params![bot, chat])?;
            tx.execute(
                "UPDATE donna_whatsapp_outbox SET state='cancelled',reason='unmarked',lease_until=NULL,updated_at=?3
                 WHERE bot_id=?1 AND chat_id=?2 AND state IN ('pending','held')
                   AND origin IN ('bot_human','bot_routine')",
                params![bot, chat, now],
            )?;
        }
        Ok(())
    })?;
    numbers.wake_outbox();
    Ok(json!({"botId": bot, "whatsappChatId": chat, "marked": marked}))
}

/// Só o dono, neste computador, altera o envio autônomo de um grupo marcado.
pub(crate) fn set_group_auto(
    store: &Store,
    host_id: &str,
    numbers: &State,
    bot: &str,
    chat: &str,
    auto: bool,
) -> Result<Value> {
    let hub = hub_id(host_id)?;
    crate::fork::donna_fixa::pedido::gravar(store, |tx| {
        ensure!(enabled(tx, &hub)?, "plugin_disabled: ligue o plugin WhatsApp primeiro.");
        ensure!(agents(tx)?.iter().any(|id| id == bot), "bot_not_found: bot não encontrado.");
        let account: String = tx
            .query_row("SELECT account_id FROM donna_whatsapp_numbers WHERE bot_id=?1 AND state='linked'", [bot], |r| {
                r.get(0)
            })
            .optional()?
            .context("not_connected: conecte o número deste bot.")?;
        let group: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM donna_whatsapp_messages WHERE bot_id=?1 AND chat_id=?2 AND account_id=?3 AND is_group=1)",
            params![bot, chat, account], |r| r.get(0),
        )?;
        ensure!(group, "invalid_field: grupo não encontrado nesta conta.");
        let changed = tx.execute(
            "UPDATE donna_whatsapp_marked SET auto=?4 WHERE bot_id=?1 AND chat_id=?2 AND account_id=?3",
            params![bot, chat, account, auto],
        )?;
        ensure!(changed == 1, "not_allowed: marque o grupo antes de mudar o envio autônomo.");
        if !auto {
            tx.execute(
                "UPDATE donna_whatsapp_outbox SET state='cancelled',reason='not_allowed',lease_until=NULL,updated_at=?3
                 WHERE bot_id=?1 AND chat_id=?2 AND state IN ('pending','held') AND origin IN ('bot_human','bot_routine')",
                params![bot, chat, now_ms()],
            )?;
        }
        Ok(())
    })?;
    numbers.wake_outbox();
    Ok(json!({"botId": bot, "whatsappChatId": chat, "auto": auto}))
}

impl Default for BotSettings {
    fn default() -> Self {
        Self {
            read_chats: false,
            reply_chats: false,
            read_groups: false,
            reply_groups: false,
            reply_mode: ReplyMode::Approval,
            // R4b: 15 s para conexões novas (margem acima do piso de 8 s).
            delay_seconds: 15,
            hourly_limit: 30,
            daily_limit: 100,
            quiet_enabled: true,
            quiet_start: 22 * 60,
            quiet_end: 8 * 60,
            quiet_time_zone: None,
            transcribe_audio: true,
            retention_days: RETENTION_DEFAULT,
        }
    }
}

impl BotSettings {
    fn any_access(&self) -> bool {
        self.read_chats || self.reply_chats || self.read_groups || self.reply_groups
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BotPatch {
    pub read_chats: Option<bool>,
    pub reply_chats: Option<bool>,
    pub read_groups: Option<bool>,
    pub reply_groups: Option<bool>,
    pub reply_mode: Option<ReplyMode>,
    pub delay_seconds: Option<u32>,
    pub hourly_limit: Option<u32>,
    pub daily_limit: Option<u32>,
    pub quiet_enabled: Option<bool>,
    pub quiet_start: Option<u32>,
    pub quiet_end: Option<u32>,
    /// `null` volta ao fuso do dono; string fixa um fuso IANA (uso interno/teste).
    #[serde(default)]
    #[allow(clippy::option_option)]
    pub quiet_time_zone: Option<Option<String>>,
    pub transcribe_audio: Option<bool>,
    pub retention_days: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HubPatch {
    pub enabled: Option<bool>,
    pub mode: Option<Mode>,
    /// `null` volta ao padrão; objeto `{provider, model}` fixa a escolha deste computador.
    #[serde(default, deserialize_with = "present")]
    pub transcription: Option<Value>,
}

/// Sem isto o serde lê `null` como campo ausente, e "voltar ao padrão" se perde.
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Transcription {
    provider: Provider,
    model: String,
}

/// Nome gravado no banco, o mesmo do JSON (`snake_case`/`openai`).
fn wire(value: impl Serialize) -> String {
    serde_json::to_value(value).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

pub(super) fn hub_id(id: &str) -> Result<Id> {
    id.to_owned().try_into().map_err(|_| anyhow::anyhow!("whatsapp_hub_id_invalid"))
}

pub(super) fn enabled(c: &Connection, hub: &Id) -> Result<bool> {
    let installation = plugins::builtin_installation(c, hub, Builtin::Whatsapp)?;
    Ok(installation.is_some_and(|i| i.state == InstallationState::Enabled))
}

fn hub_row(c: &Connection) -> Result<(Mode, Option<(String, String)>)> {
    let row: Option<(String, Option<String>, Option<String>)> = c
        .query_row("SELECT mode,transcribe_provider,transcribe_model FROM donna_whatsapp_hub WHERE id=1", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?;
    let Some((mode, provider, model)) = row else { return Ok((Mode::default(), None)) };
    let mode = serde_json::from_value(json!(mode)).context("whatsapp_mode_invalid")?;
    Ok((mode, provider.zip(model)))
}

pub(super) fn bot_row(c: &Connection, bot: &str) -> Result<Option<BotSettings>> {
    Ok(c.query_row(
        "SELECT read_chats,reply_chats,read_groups,reply_groups,delay_seconds,hourly_limit,daily_limit,
           quiet_enabled,quiet_start,quiet_end,quiet_time_zone,transcribe_audio,retention_days,reply_mode
         FROM donna_whatsapp_bots WHERE bot_id=?1",
        [bot],
        |r| {
            Ok(BotSettings {
                read_chats: r.get(0)?,
                reply_chats: r.get(1)?,
                read_groups: r.get(2)?,
                reply_groups: r.get(3)?,
                reply_mode: match r.get::<_, String>(13)?.as_str() {
                    "autonomous" => ReplyMode::Autonomous,
                    _ => ReplyMode::Approval,
                },
                delay_seconds: r.get(4)?,
                hourly_limit: r.get(5)?,
                daily_limit: r.get(6)?,
                quiet_enabled: r.get(7)?,
                quiet_start: r.get(8)?,
                quiet_end: r.get(9)?,
                quiet_time_zone: r.get(10)?,
                transcribe_audio: r.get(11)?,
                retention_days: r.get(12)?,
            })
        },
    )
    .optional()?)
}

/// "Transcrever áudios recebidos" do bot (R1; padrão ligado).
/// O plugin do `WhatsApp` está ligado neste Hub.
pub(super) fn plugin_on(c: &Connection, host_id: &str) -> Result<bool> {
    enabled(c, &hub_id(host_id)?)
}

pub(super) fn transcribe_audio(c: &Connection, bot: &str) -> Result<bool> {
    Ok(bot_row(c, bot)?.unwrap_or_default().transcribe_audio)
}

/// Provedor e modelo de transcrição deste computador, só se houver chave no Hub: a escolha
/// do dono (R1) ou, sem escolha, o primeiro provedor com chave e o modelo padrão dele.
pub(super) fn transcriber(store: &Store) -> Result<Option<(Provider, String)>> {
    let voice = crate::voice::status(store)?;
    let (_, chosen) = hub_row(&store.db.locked())?;
    let view = transcription(&voice, chosen);
    if view["keyOnHub"] != true {
        return Ok(None);
    }
    let (Some(provider), Some(model)) = (view["provider"].as_str(), view["model"].as_str()) else { return Ok(None) };
    let provider: Provider = serde_json::from_value(json!(provider)).context("whatsapp_transcription_provider")?;
    Ok(Some((provider, model.to_owned())))
}

/// Audiência da caixa do `WhatsApp`: bot agente vivo do espaço Pessoal (o único onde o
/// número pode ter concessão). Quem chama já conferiu que é o dono.
pub(super) fn inbox_bot(store: &Store, bot: &str) -> Result<()> {
    let raw: Option<String> = store
        .db
        .locked()
        .query_row("SELECT config FROM bots WHERE id=?1 AND deleted=0", [bot], |r| r.get(0))
        .optional()?;
    let ok = raw
        .and_then(|raw| serde_json::from_str::<BotConfig>(&raw).ok())
        .is_some_and(|cfg| cfg.id == bot && !cfg.is_group() && cfg.space_id == PERSONAL);
    ensure!(ok, "bot_not_found: bot não encontrado.");
    Ok(())
}

/// Bots agentes vivos, na ordem do Hub. Grupos não têm número.
fn agents(c: &Connection) -> Result<Vec<String>> {
    let mut stmt = c.prepare("SELECT id,config FROM bots WHERE deleted=0 ORDER BY id")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut ids = Vec::new();
    for row in rows {
        let (id, raw) = row?;
        if serde_json::from_str::<BotConfig>(&raw).is_ok_and(|cfg| cfg.id == id && !cfg.is_group()) {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// Quem pode ter número agora: plugin ligado, bot agente vivo e não pausado.
pub(super) struct Policy {
    pub enabled: bool,
    pub agents: Vec<String>,
    pub allowed: Vec<String>,
}

pub(super) fn policy(store: &Store, host_id: &str) -> Result<Policy> {
    let hub = hub_id(host_id)?;
    let c = store.db.locked();
    let enabled = enabled(&c, &hub)?;
    let agents = agents(&c)?;
    let allowed = eligible_in_mode(&c, &agents)?.into_iter().filter(|_| enabled).cloned().collect();
    Ok(Policy { enabled, agents, allowed })
}

pub(crate) fn eligible_in_mode<'a>(c: &Connection, agents: &'a [String]) -> Result<Vec<&'a String>> {
    let (mode, _) = hub_row(c)?;
    let donna = crate::store::team_settings_on(c)?.primary_bot_id;
    Ok(agents.iter().filter(|id| mode == Mode::PerBot || donna.as_deref() == Some(id.as_str())).collect())
}

/// Padrão: o primeiro provedor com chave no Hub e o modelo de transcrição dele.
fn transcription(voice: &Value, chosen: Option<(String, String)>) -> Value {
    let providers = voice["providers"].as_array().cloned().unwrap_or_default();
    let has_key = |id: &str| providers.iter().any(|p| p["provider"] == id && p["configured"] == true);
    if let Some((provider, model)) = chosen {
        let key_on_hub = has_key(&provider);
        return json!({"provider": provider, "model": model, "source": "chosen", "keyOnHub": key_on_hub});
    }
    match providers.iter().find(|p| p["configured"] == true) {
        Some(p) => {
            json!({"provider": p["provider"], "model": p["defaults"]["transcribe"], "source": "default", "keyOnHub": true})
        }
        None => json!({"provider": null, "model": null, "source": "none", "keyOnHub": false}),
    }
}

/// Os 4 estados ficam separados: plugin ≠ número ≠ o que o bot pode ≠ envio.
pub(crate) fn read(store: &Store, host_id: &str, numbers: &State) -> Result<Value> {
    let hub = hub_id(host_id)?;
    let donna = store.team_settings()?.primary_bot_id;
    let voice = crate::voice::status(store)?;
    let c = store.db.locked();
    let enabled = enabled(&c, &hub)?;
    let (mode, chosen) = hub_row(&c)?;
    let rows = super::number::rows(&c)?;
    let accounts = accounts(&c)?;
    let installed = numbers.binary().is_some();
    let mut bots = Vec::new();
    for id in agents(&c)? {
        let settings = bot_row(&c, &id)?.unwrap_or_default();
        let is_donna = donna.as_deref() == Some(id.as_str());
        let paused = mode == Mode::DonnaOnly && !is_donna;
        let live = enabled && !paused;
        // Ver: só o registro de plugins responde (concessão ativa no número conectado).
        let can = |read| -> Result<bool> {
            Ok(match accounts.get(&id) {
                Some(account) => whatsapp_grants::can_read(&c, host_id, account, &id, read)?,
                None => false,
            })
        };
        let (read_chats, read_groups) = (can(Read::Chats)?, can(Read::Groups)?);
        bots.push(json!({
            "botId": id,
            "isDonna": is_donna,
            "paused": paused,
            "settings": settings,
            // Telefone só com a sessão conectada e confirmada (`number::State::view`).
            "number": numbers.view(&id, rows.get(&id).copied(), live, installed),
            "botCan": {
                "readChats": read_chats,
                "replyChats": live && settings.reply_chats,
                "readGroups": read_groups,
                "replyGroups": live && settings.reply_groups,
            },
            // MX-18: o envio existe; o app deriva o estado 4 do `reply_mode` das settings.
            "sending": {"state": "none"},
            // Caixa do WhatsApp do bot (aba própria no app, nunca o chat): não lidas.
            "inbox": {"unread": super::messages::unread(&c, &id)?},
        }));
    }
    Ok(json!({
        "plugin": {"enabled": enabled, "mode": mode, "wacli": {"installed": installed}},
        "transcription": transcription(&voice, chosen),
        "bots": bots,
    }))
}

fn checked_model(model: &str) -> Result<()> {
    let ok = !model.is_empty()
        && model.len() <= 100
        && model.chars().all(|c| c.is_ascii_alphanumeric() || "-._".contains(c));
    ensure!(ok, "invalid_field: modelo de transcrição inválido.");
    Ok(())
}

pub(crate) fn save_hub(store: &Store, host_id: &str, numbers: &State, patch: HubPatch) -> Result<Value> {
    let hub = hub_id(host_id)?;
    let donna = store.team_settings()?.primary_bot_id;
    let contexts = contexts(store, &hub)?;
    let transcription = match patch.transcription {
        None => None,
        Some(Value::Null) => Some(None),
        Some(raw) => {
            let t: Transcription =
                serde_json::from_value(raw).map_err(|_| anyhow::anyhow!("invalid_field: transcrição inválida."))?;
            checked_model(&t.model)?;
            Some(Some(t))
        }
    };
    // D368: a autoridade original do pedido do dono é reconferida nesta transação (`pedido::gravar`).
    crate::fork::donna_fixa::pedido::gravar(store, |tx| {
        let (mut mode, mut chosen) = hub_row(tx)?;
        if let Some(next) = patch.mode {
            // Trocar para "Só a Donna" só pausa os outros bots; nada é apagado.
            mode = next;
        }
        if let Some(next) = transcription {
            chosen = next.map(|t| (wire(t.provider), t.model));
        }
        let (provider, model) = chosen.unzip();
        tx.execute(
            "INSERT INTO donna_whatsapp_hub(id,mode,transcribe_provider,transcribe_model) VALUES(1,?1,?2,?3)
             ON CONFLICT(id) DO UPDATE SET mode=excluded.mode,transcribe_provider=excluded.transcribe_provider,
             transcribe_model=excluded.transcribe_model",
            params![wire(mode), provider, model],
        )?;
        if let Some(want) = patch.enabled
            && want != enabled(tx, &hub)?
        {
            let change = if want { LifecycleChange::Enable } else { LifecycleChange::Disable };
            plugins::set_builtin(tx, &hub, Builtin::Whatsapp, change)?;
            if !want {
                // Contrato §7: desligar tira todo acesso; religar começa sem nada.
                tx.execute(
                    "UPDATE donna_whatsapp_bots SET read_chats=0,reply_chats=0,read_groups=0,reply_groups=0",
                    [],
                )?;
                super::outbox::cancel_all_pending(tx, "plugin_disabled")?;
            }
        }
        apply_grants(tx, &hub, donna.as_deref(), &contexts)?;
        Ok(())
    })?;
    numbers.wake_outbox();
    read(store, host_id, numbers)
}

pub(crate) fn save_bot(store: &Store, host_id: &str, numbers: &State, bot: &str, patch: &BotPatch) -> Result<Value> {
    let hub = hub_id(host_id)?;
    let donna = store.team_settings()?.primary_bot_id;
    let contexts = contexts(store, &hub)?;
    // D368: a autoridade original do pedido do dono é reconferida nesta transação (`pedido::gravar`).
    crate::fork::donna_fixa::pedido::gravar(store, |tx| {
        ensure!(enabled(tx, &hub)?, "plugin_disabled: ligue o plugin WhatsApp primeiro.");
        if !agents(tx)?.iter().any(|id| id == bot) {
            bail!("bot_not_found: bot não encontrado.");
        }
        let mut s = bot_row(tx, bot)?.unwrap_or_default();
        if let Some(v) = patch.read_chats {
            s.read_chats = v;
            if !v && patch.reply_chats.is_none() {
                s.reply_chats = false;
            }
        }
        if let Some(v) = patch.read_groups {
            s.read_groups = v;
            if !v && patch.reply_groups.is_none() {
                s.reply_groups = false;
            }
        }
        s.reply_chats = patch.reply_chats.unwrap_or(s.reply_chats);
        s.reply_groups = patch.reply_groups.unwrap_or(s.reply_groups);
        s.reply_mode = patch.reply_mode.unwrap_or(s.reply_mode);
        s.delay_seconds = patch.delay_seconds.unwrap_or(s.delay_seconds);
        s.hourly_limit = patch.hourly_limit.unwrap_or(s.hourly_limit);
        s.daily_limit = patch.daily_limit.unwrap_or(s.daily_limit);
        s.quiet_enabled = patch.quiet_enabled.unwrap_or(s.quiet_enabled);
        s.quiet_start = patch.quiet_start.unwrap_or(s.quiet_start);
        s.quiet_end = patch.quiet_end.unwrap_or(s.quiet_end);
        if let Some(tz) = patch.quiet_time_zone.as_ref() {
            s.quiet_time_zone.clone_from(tz);
        }
        s.transcribe_audio = patch.transcribe_audio.unwrap_or(s.transcribe_audio);
        let previous = s.retention_days;
        s.retention_days = patch.retention_days.unwrap_or(s.retention_days);
        ensure!(!s.reply_chats || s.read_chats, "invalid_field: responder conversas exige ver conversas.");
        ensure!(!s.reply_groups || s.read_groups, "invalid_field: responder grupos exige ver grupos.");
        ensure!(
            (DELAY_MIN..=DELAY_MAX).contains(&s.delay_seconds),
            "invalid_field: o intervalo entre mensagens vai de 8 a 300 segundos. Nunca mais rápido que 1 mensagem a cada 8 s."
        );
        ensure!(
            (1..=HOURLY_MAX).contains(&s.hourly_limit),
            "invalid_field: o limite vai de 1 a 200 mensagens por hora. Você pode baixar, nunca passar de 200."
        );
        ensure!(
            (1..=DAILY_MAX).contains(&s.daily_limit),
            "invalid_field: o limite por dia vai de 1 a 500 mensagens. Você pode baixar, nunca passar de 500."
        );
        ensure!(
            s.quiet_start <= 1439 && s.quiet_end <= 1439,
            "invalid_field: o horário de silêncio usa minutos do dia, de 0 a 1439."
        );
        if let Some(tz) = s.quiet_time_zone.as_deref() {
            ensure!(tz.parse::<chrono_tz::Tz>().is_ok(), "invalid_field: fuso de silêncio inválido.");
        }
        ensure!(RETENTION_DAYS.contains(&s.retention_days), "invalid_field: o prazo é 7, 30 ou 90 dias.");
        let (mode, _) = hub_row(tx)?;
        if mode == Mode::DonnaOnly && donna.as_deref() != Some(bot) && s.any_access() {
            bail!("not_allowed: no modo Só o Berga, só o Berga usa o WhatsApp.");
        }
        tx.execute(
            "INSERT INTO donna_whatsapp_bots(bot_id,read_chats,reply_chats,read_groups,reply_groups,reply_mode,
             delay_seconds,hourly_limit,daily_limit,quiet_enabled,quiet_start,quiet_end,quiet_time_zone,
             transcribe_audio,retention_days)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)
             ON CONFLICT(bot_id) DO UPDATE SET read_chats=excluded.read_chats,reply_chats=excluded.reply_chats,
             read_groups=excluded.read_groups,reply_groups=excluded.reply_groups,reply_mode=excluded.reply_mode,
             delay_seconds=excluded.delay_seconds,
             hourly_limit=excluded.hourly_limit,daily_limit=excluded.daily_limit,quiet_enabled=excluded.quiet_enabled,
             quiet_start=excluded.quiet_start,quiet_end=excluded.quiet_end,quiet_time_zone=excluded.quiet_time_zone,
             transcribe_audio=excluded.transcribe_audio,retention_days=excluded.retention_days",
            params![
                bot,
                s.read_chats,
                s.reply_chats,
                s.read_groups,
                s.reply_groups,
                reply_mode_wire(s.reply_mode),
                s.delay_seconds,
                s.hourly_limit,
                s.daily_limit,
                s.quiet_enabled,
                s.quiet_start,
                s.quiet_end,
                s.quiet_time_zone,
                s.transcribe_audio,
                s.retention_days
            ],
        )?;
        if s.retention_days != previous {
            // O prazo novo vale para o que já está guardado: encurtar some das leituras na
            // hora e sai na próxima limpeza; alongar não traz de volta o que já saiu.
            super::messages::retime(tx, bot, i64::from(s.retention_days) * DAY_MS)?;
            tx.execute("UPDATE donna_whatsapp_bots SET retention_rev=retention_rev+1 WHERE bot_id=?1", [bot])?;
        }
        if s.reply_mode != ReplyMode::Autonomous {
            // Voltou ao "peço antes": o que o modo sozinho ainda esperava não sai mais.
            tx.execute(
                "UPDATE donna_whatsapp_outbox SET state='cancelled',reason='mode',lease_until=NULL,updated_at=?2
                 WHERE bot_id=?1 AND state IN ('pending','held') AND origin IN ('bot_human','bot_routine')",
                params![bot, now_ms()],
            )?;
        }
        let refused = apply_grants(tx, &hub, donna.as_deref(), &contexts)?;
        if refused.iter().any(|id| id == bot) {
            bail!("not_allowed: só bots do espaço Pessoal podem ver o WhatsApp.");
        }
        Ok(())
    })?;
    numbers.wake_outbox();
    read(store, host_id, numbers)
}

// MARK: concessões (R3, D247)

/// Conta (ID do número) de cada bot com número no módulo, conectado ou não.
fn accounts(c: &Connection) -> Result<HashMap<String, String>> {
    let mut stmt = c.prepare("SELECT bot_id,account_id FROM donna_whatsapp_numbers")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Contexto pessoal principal de cada bot agente (descrição da concessão, como na Agenda).
/// Lido antes da transação: as leituras da Store travam o banco.
fn contexts(store: &Store, hub: &Id) -> Result<HashMap<String, ContextRef>> {
    let Some(space) =
        store.spaces().ok().and_then(|s| s.spaces.into_iter().find(|s| s.id == PERSONAL)).map(|s| s.revision)
    else {
        return Ok(HashMap::new());
    };
    let ids = agents(&store.db.locked())?;
    let mut out = HashMap::new();
    for id in ids {
        let Ok(conversation) = store.space_conversation(PERSONAL, &id) else { continue };
        let raw = json!({"version":1,"hubId":hub,"spaceId":PERSONAL,"botId":id,"conversationId":id,
            "audience":{"kind":"principal"},"teamId":null,"privacyDomainId":PERSONAL,
            "domainRevision":space,"conversationRevision":conversation.revision});
        if let Ok(context) = serde_json::from_value(raw) {
            out.insert(id, context);
        }
    }
    Ok(out)
}

/// Deixa o registro igual às opções: para cada número, "ver conversas/grupos" concedido
/// se o plugin está ligado, o bot não está pausado e o dono ligou a opção; senão revogado.
/// Devolve os bots cuja concessão o registro recusou (ex.: fora do espaço Pessoal).
fn apply_grants(
    tx: &Connection,
    hub: &Id,
    donna: Option<&str>,
    contexts: &HashMap<String, ContextRef>,
) -> Result<Vec<String>> {
    let on = enabled(tx, hub)?;
    let (mode, _) = hub_row(tx)?;
    let mut refused = Vec::new();
    for (bot, account) in accounts(tx)? {
        if !whatsapp_grants::link(tx, hub, &account)? {
            continue;
        }
        let settings = bot_row(tx, &bot)?.unwrap_or_default();
        let live = on && (mode == Mode::PerBot || donna == Some(bot.as_str()));
        let context = contexts.get(&bot);
        for (read, want) in [(Read::Chats, settings.read_chats), (Read::Groups, settings.read_groups)] {
            let want = live && want;
            match whatsapp_grants::set(tx, hub, &account, &bot, context, (read, Access::See), want) {
                Ok(()) => {}
                Err(plugins::model::PluginError::PolicyDenied | plugins::model::PluginError::Constraint) if want => {
                    refused.push(bot.clone());
                }
                Err(error) => return Err(error.into()),
            }
        }
        // MX-18: "Responder" é concessão real do registro: ligar a opção concede, desligar revoga.
        for (read, want) in [(Read::Chats, settings.reply_chats), (Read::Groups, settings.reply_groups)] {
            let want = live && want;
            match whatsapp_grants::set(tx, hub, &account, &bot, context, (read, Access::Reply), want) {
                Ok(()) => {}
                Err(plugins::model::PluginError::PolicyDenied | plugins::model::PluginError::Constraint) if want => {
                    refused.push(bot.clone());
                }
                Err(error) => return Err(error.into()),
            }
        }
        super::outbox::cancel_revoked(tx, hub.as_str(), &bot, live, &settings)?;
    }
    Ok(refused)
}

/// Mesma coisa fora de um pedido do dono: número novo e início do Hub.
pub(super) fn refresh_grants(store: &Store, host_id: &str) -> Result<()> {
    let hub = hub_id(host_id)?;
    let donna = store.team_settings()?.primary_bot_id;
    let contexts = contexts(store, &hub)?;
    let mut db = store.db.locked();
    let tx = db.transaction()?;
    apply_grants(&tx, &hub, donna.as_deref(), &contexts)?;
    tx.commit()?;
    Ok(())
}
