//! D259: ocorrências de não gravação, somente metadados e referências do escopo.
use super::queue::{Context, LIFE_MS};
use crate::{
    LockExt,
    hub::Hub,
    store::{BotConfig, Store},
};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Teto por bot + Space e por Hub, comum à auditoria `non_saved` e aos recibos do scanner.
pub(super) const SCOPE_CAP: i64 = 100;
pub(super) const HUB_CAP: i64 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Reason {
    Duplicate,
    WrongBot,
    NotDurable,
    TechnicalDeferred,
    Full,
    Expired,
    NoSelection,
    Ineligible,
}
impl Reason {
    fn name(self) -> &'static str {
        match self {
            Self::Duplicate => "duplicate",
            Self::WrongBot => "wrongBot",
            Self::NotDurable => "notDurable",
            Self::TechnicalDeferred => "technicalDeferred",
            Self::Full => "full",
            Self::Expired => "expired",
            Self::NoSelection => "noSelection",
            Self::Ineligible => "ineligible",
        }
    }
    fn disposition(self) -> (&'static str, Option<&'static str>) {
        match self {
            Self::Duplicate | Self::WrongBot | Self::NotDurable => ("notSaved", None),
            Self::TechnicalDeferred => ("deferred", Some("transient")),
            Self::NoSelection => ("deferred", Some("noSelection")),
            Self::Full => ("notSaved", Some("full")),
            Self::Expired => ("notSaved", Some("expired")),
            Self::Ineligible => ("notSaved", Some("ineligible")),
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Refs {
    message_ids: Vec<String>,
    response_ids: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NonSaved {
    id: String,
    job_id: String,
    reason: Reason,
    status: &'static str,
    at: i64,
    message_ids: Vec<String>,
    response_ids: Vec<String>,
    availability: Option<&'static str>,
}

pub(super) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_memory_auto_non_saved(
        id TEXT PRIMARY KEY,job TEXT NOT NULL,bot TEXT NOT NULL,chat TEXT NOT NULL,space TEXT NOT NULL,
        memory_id TEXT NOT NULL,audience TEXT NOT NULL CHECK(audience='owner'),
        reason TEXT NOT NULL CHECK(reason IN ('duplicate','wrongBot','notDurable','technicalDeferred',
            'full','expired','noSelection','ineligible')),
        at INTEGER NOT NULL,refs TEXT NOT NULL,UNIQUE(job,reason,refs));
        CREATE INDEX IF NOT EXISTS donna_memory_auto_non_saved_scope
        ON donna_memory_auto_non_saved(bot,chat,space,memory_id,audience,at);",
    )?;
    Ok(())
}

/// A manutenção também é atômica quando chamada fora de um checkpoint/complete.
pub(super) fn atomic<T>(db: &Connection, change: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    if !db.is_autocommit() {
        return change(db);
    }
    let tx = db.unchecked_transaction()?;
    let value = change(&tx)?;
    tx.commit()?;
    Ok(value)
}

/// Invalida a tela pela revisão persistida sem anunciar ao motor que um fato foi salvo.
pub(super) fn notify(hub: &Hub, context: &Context) {
    if crate::fork::spaces::memory_changed(hub, Some(&context.memory_id)).is_err() {
        tracing::warn!("invalidação do histórico de memória aguardando releitura");
    }
    hub.emit_bot(&context.bot_id);
}

const JOINS: &str = "JOIN donna_memory_auto_jobs j ON j.id=a.job
    JOIN bots b ON b.id=a.bot AND b.deleted=0
    JOIN donna_space_conversations sc ON sc.id=a.chat AND sc.bot_id=a.bot AND sc.space_id=a.space";
// Aplicado no WHERE antes de ORDER/LIMIT, inclusive para referências inválidas mais recentes.
// Memória de grupo e fontes bot→bot nunca ganham a audiência owner por ausência de classificação.
const VISIBLE: &str = "a.audience='owner' AND j.bot=a.bot AND j.chat=a.chat AND j.space=a.space
    AND json_extract(j.context,'$.botId')=a.bot AND json_extract(j.context,'$.chatId')=a.chat
    AND json_extract(j.context,'$.spaceId')=a.space
    AND json_extract(j.context,'$.memoryId')=a.memory_id
    AND COALESCE(json_extract(b.config,'$.kind'),'agent')='agent'
    AND ((COALESCE(json_extract(b.config,'$.spaceId'),?6)=a.space AND a.memory_id=a.bot)
        OR (EXISTS(SELECT 1 FROM kv WHERE k='donna.space.memory.'||a.space||'.'||a.bot AND v=a.memory_id)
            AND EXISTS(SELECT 1 FROM kv WHERE k='team.settings.v1' AND json_extract(v,'$.primaryBotId')=a.bot)))
    AND json_type(a.refs,'$.messageIds')='array' AND json_array_length(a.refs,'$.messageIds')>0
    AND json_type(a.refs,'$.responseIds')='array'
    AND NOT EXISTS(SELECT 1 FROM json_each(a.refs,'$.messageIds') ref WHERE ref.type<>'text' OR NOT EXISTS(
        SELECT 1 FROM entries e,json_each(j.context,'$.messages') admitted
        WHERE e.id=ref.value AND e.bot_id=a.chat AND e.kind='user'
            AND json_extract(admitted.value,'$.id')=e.id AND json_extract(admitted.value,'$.revision')=e.rev
            AND json_extract(e.data,'$.from') IS NULL AND json_extract(e.data,'$.sourceBotId') IS NULL
            AND json_extract(e.data,'$.runId') IS NULL AND json_extract(e.data,'$.author') IS NULL
            AND json_extract(e.data,'$.botMessage') IS NULL))
    AND NOT EXISTS(SELECT 1 FROM json_each(a.refs,'$.responseIds') ref WHERE ref.type<>'text' OR NOT EXISTS(
        SELECT 1 FROM entries e,json_each(j.context,'$.responses') admitted
        WHERE e.id=ref.value AND e.bot_id=a.chat AND e.kind='agent'
            AND json_extract(admitted.value,'$.id')=e.id AND json_extract(e.data,'$.author')=a.bot
            AND e.turn=json_extract(j.context,'$.closingTurn')))";

/// Nunca recebe candidato, motivo livre, segredo, digest de conteúdo ou ID de outro bot.
/// Deve participar da transação do efeito/disposição; `WrongBot` não tem produtor na Parte A.
pub(super) fn record_job(
    db: &Connection,
    job: &str,
    reason: Reason,
    evidence: Option<&[String]>,
    now: i64,
) -> Result<bool> {
    if reason == Reason::WrongBot {
        return Ok(false);
    }
    let raw: Option<String> =
        db.query_row("SELECT context FROM donna_memory_auto_jobs WHERE id=?1", [job], |r| r.get(0)).optional()?;
    let Some(context) = raw.and_then(|raw| serde_json::from_str::<Context>(&raw).ok()) else { return Ok(false) };
    let ids: Vec<String> = evidence.map_or_else(
        || {
            context
                .messages
                .iter()
                .map(|m| m.id.clone())
                .chain(context.responses.iter().map(|r| r.id.clone()))
                .collect()
        },
        <[String]>::to_vec,
    );
    ensure!(ids.iter().all(|id| context.contains(id)), "memory_audit_evidence_required");
    let mut refs = Refs::default();
    for id in ids {
        if context.messages.iter().any(|m| m.id == id) {
            refs.message_ids.push(id);
        } else {
            refs.response_ids.push(id);
        }
    }
    // Uma decisão baseada na resposta conserva também a referência humana que delimita a audiência.
    if refs.message_ids.is_empty() && !refs.response_ids.is_empty() {
        refs.message_ids.extend(context.messages.iter().map(|m| m.id.clone()));
    }
    refs.message_ids.sort();
    refs.message_ids.dedup();
    refs.response_ids.sort();
    refs.response_ids.dedup();
    let refs = serde_json::to_string(&refs)?;
    let visible: bool = db.query_row(
        &format!(
            "WITH a AS (SELECT ?1 AS job,?2 AS bot,?3 AS chat,?4 AS space,
        ?5 AS memory_id,'owner' AS audience,?7 AS refs) SELECT EXISTS(SELECT 1 FROM a {JOINS} WHERE {VISIBLE})"
        ),
        params![
            job,
            context.bot_id,
            context.chat_id,
            context.space_id,
            context.memory_id,
            crate::fork::spaces::PERSONAL,
            refs
        ],
        |r| r.get(0),
    )?;
    if !visible {
        return Ok(false);
    }
    // ID estável inclusive após retenção: só job/motivo/IDs canônicos, nunca hash do candidato recusado.
    let digest = Sha256::digest(serde_json::to_vec(&(job, reason.name(), &refs))?);
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let id = uuid::Uuid::from_bytes(bytes).to_string();
    let inserted = db.execute(
        "INSERT OR IGNORE INTO donna_memory_auto_non_saved(id,job,bot,chat,space,memory_id,audience,reason,at,refs)
        VALUES(?1,?2,?3,?4,?5,?6,'owner',?7,?8,?9)",
        params![
            id,
            job,
            context.bot_id,
            context.chat_id,
            context.space_id,
            context.memory_id,
            reason.name(),
            now,
            refs
        ],
    )? > 0;
    if inserted {
        db.execute("DELETE FROM donna_memory_auto_non_saved WHERE at<=?1", [now - LIFE_MS])?;
        db.execute(&format!("DELETE FROM donna_memory_auto_non_saved WHERE id IN(
            SELECT id FROM donna_memory_auto_non_saved WHERE bot=?1 AND space=?2 ORDER BY at DESC,rowid DESC LIMIT -1 OFFSET {SCOPE_CAP})"),
            params![context.bot_id,context.space_id])?;
        db.execute(
            &format!(
                "DELETE FROM donna_memory_auto_non_saved WHERE id IN(
            SELECT id FROM donna_memory_auto_non_saved ORDER BY at DESC,rowid DESC LIMIT -1 OFFSET {HUB_CAP})"
            ),
            [],
        )?;
        let rev = crate::store::next_rev(db)?;
        db.execute("INSERT INTO kv(k,v) VALUES('memory.rev',?1) ON CONFLICT(k) DO UPDATE SET v=excluded.v", [rev])?;
    }
    Ok(inserted)
}

pub(crate) fn list(store: &Store, cfg: &BotConfig) -> Result<Vec<NonSaved>> {
    if cfg.is_group() {
        return Ok(Vec::new());
    }
    let db = store.db.locked();
    let mut stmt = db.prepare(&format!(
        "SELECT a.id,a.job,a.reason,a.at,a.refs FROM donna_memory_auto_non_saved a {JOINS}
        WHERE a.bot=?1 AND a.chat=?2 AND a.space=?3 AND a.memory_id=?4 AND a.at>?5 AND {VISIBLE}
        ORDER BY a.at DESC,a.rowid DESC LIMIT 50"
    ))?;
    let rows = stmt.query_map(
        params![
            cfg.id,
            cfg.session_owner(),
            cfg.space_id,
            cfg.memory_id(),
            crate::store::now_ms() - LIFE_MS,
            crate::fork::spaces::PERSONAL
        ],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
            ))
        },
    )?;
    let mut result = Vec::new();
    for row in rows {
        let (id, job_id, reason, at, refs) = row?;
        let Ok(reason) = serde_json::from_value::<Reason>(serde_json::Value::String(reason)) else { continue };
        let refs: Refs = serde_json::from_str(&refs)?;
        let (status, availability) = reason.disposition();
        result.push(NonSaved {
            id,
            job_id,
            reason,
            status,
            at,
            message_ids: refs.message_ids,
            response_ids: refs.response_ids,
            availability,
        });
    }
    Ok(result)
}
