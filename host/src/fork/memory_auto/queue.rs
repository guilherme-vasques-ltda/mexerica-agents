use crate::{
    LockExt,
    hub::Hub,
    store::{BotConfig, Lane, Store},
};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

pub(crate) const LIFE_MS: i64 = 30 * 24 * 60 * 60 * 1000;
pub(crate) const RESERVATION_MS: i64 = 120_000;
const ACTIVE: &str = "('waiting','reserved')";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Reference {
    pub id: String,
    pub revision: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResponseReference {
    pub id: String,
    pub hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Context {
    pub bot_id: String,
    pub chat_id: String,
    pub space_id: String,
    pub memory_id: String,
    #[serde(default)]
    pub backend: String,
    /// Revisão durável de configuração/escopo, não a revisão de publicação do roster.
    pub bot_revision: i64,
    pub conversation_revision: Option<i64>,
    pub session: String,
    /// Sessão visível do catálogo, distinta da sessão do motor acima.
    #[serde(default)]
    pub visible_session: Option<String>,
    /// Vínculo administrativo D406; o cartão nunca é evidência nem contexto do extrator.
    #[serde(default)]
    pub owner_question: Option<String>,
    pub token: String,
    pub messages: Vec<Reference>,
    #[serde(default)]
    pub responses: Vec<ResponseReference>,
    /// Turno do fechamento emitido pelo Hub, independente dos turnos das mensagens folded.
    #[serde(default)]
    pub closing_turn: Option<i64>,
    #[serde(default)]
    pub started_at: i64,
    #[serde(default = "marked_default")]
    pub initial_marked: bool,
}
fn marked_default() -> bool {
    true
}
impl Context {
    pub(crate) fn contains(&self, id: &str) -> bool {
        self.messages.iter().any(|m| m.id == id) || self.responses.iter().any(|m| m.id == id)
    }
}

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_memory_auto_jobs(
          id TEXT PRIMARY KEY,bot TEXT NOT NULL,chat TEXT NOT NULL,space TEXT NOT NULL,
          context TEXT NOT NULL,outcome TEXT NOT NULL DEFAULT 'running',scope TEXT NOT NULL DEFAULT 'allowed',
          scope_revision INTEGER NOT NULL DEFAULT 1,status TEXT NOT NULL DEFAULT 'running',
          reason TEXT,admitted_at INTEGER,admitted_order INTEGER NOT NULL DEFAULT 0,
          reserved_at INTEGER,reservation TEXT,attempts INTEGER NOT NULL DEFAULT 0);
         CREATE INDEX IF NOT EXISTS donna_memory_auto_queue ON donna_memory_auto_jobs(status,admitted_at);
         CREATE TABLE IF NOT EXISTS donna_memory_auto_proposals(
          job TEXT NOT NULL,operation TEXT NOT NULL,PRIMARY KEY(job,operation));
         CREATE TABLE IF NOT EXISTS donna_memory_auto_receipts(
          id TEXT PRIMARY KEY,job TEXT NOT NULL,operation TEXT NOT NULL,entry INTEGER,file_version INTEGER,
          rev INTEGER NOT NULL,previous TEXT,origin TEXT NOT NULL,status TEXT NOT NULL DEFAULT 'done',
          nature TEXT NOT NULL DEFAULT 'unknown',nature_bot TEXT,conflict_text TEXT,
          UNIQUE(job,operation));
         CREATE TABLE IF NOT EXISTS donna_memory_auto_engines(
          bot TEXT PRIMARY KEY,token TEXT NOT NULL,chat TEXT NOT NULL,backend TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS donna_memory_auto_legacy(
          notice TEXT PRIMARY KEY,job TEXT,disposition TEXT NOT NULL,admitted_at INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS donna_memory_auto_bot_bindings(
          bot TEXT PRIMARY KEY,revision INTEGER NOT NULL);
         INSERT OR IGNORE INTO donna_memory_auto_bot_bindings(bot,revision) SELECT id,rev FROM bots;
         CREATE TRIGGER IF NOT EXISTS donna_memory_auto_bot_insert AFTER INSERT ON bots BEGIN
          INSERT INTO donna_memory_auto_bot_bindings(bot,revision) VALUES(NEW.id,NEW.rev)
          ON CONFLICT(bot) DO UPDATE SET revision=MAX(revision+1,NEW.rev);
         END;
         CREATE TRIGGER IF NOT EXISTS donna_memory_auto_bot_update AFTER UPDATE OF config,deleted ON bots
          WHEN OLD.config IS NOT NEW.config OR OLD.deleted IS NOT NEW.deleted
         BEGIN
          UPDATE donna_memory_auto_bot_bindings SET revision=MAX(revision+1,NEW.rev) WHERE bot=NEW.id;
         END;
         CREATE TRIGGER IF NOT EXISTS donna_memory_auto_bot_delete AFTER DELETE ON bots BEGIN
          UPDATE donna_memory_auto_bot_bindings SET revision=revision+1 WHERE bot=OLD.id;
         END;",
    )?;
    let has_order: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('donna_memory_auto_jobs') WHERE name='admitted_order')",
        [],
        |r| r.get(0),
    )?;
    if !has_order {
        db.execute_batch(
            "ALTER TABLE donna_memory_auto_jobs ADD COLUMN admitted_order INTEGER NOT NULL DEFAULT 0;
            UPDATE donna_memory_auto_jobs SET admitted_order=rowid;",
        )?;
    }
    let has_nature: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('donna_memory_auto_receipts') WHERE name='nature')",
        [],
        |r| r.get(0),
    )?;
    if !has_nature {
        db.execute_batch(
            "ALTER TABLE donna_memory_auto_receipts ADD COLUMN nature TEXT NOT NULL DEFAULT 'unknown';
        ALTER TABLE donna_memory_auto_receipts ADD COLUMN nature_bot TEXT;
        ALTER TABLE donna_memory_auto_receipts ADD COLUMN conflict_text TEXT;",
        )?;
    }
    // D246: vínculo da execução que preparou a proposta (`apply::stage_in`). Coluna interna e aditiva; linhas antigas
    // ficam NULL e não são preenchidas: sem prova do vínculo original, a aplicação recusa (preparado não salvo).
    let has_binding: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('donna_memory_auto_proposals') WHERE name='binding')",
        [],
        |r| r.get(0),
    )?;
    if !has_binding {
        db.execute("ALTER TABLE donna_memory_auto_proposals ADD COLUMN binding TEXT", [])?;
    }
    // Sono 2b1b (R1-01 = SEG-499-01): prova explícita, gravada na Tx do commit, de que um `noChange` terminou com o
    // aprendizado ligado. Linhas antigas ficam NULL e não contam como lidas.
    let has_proof: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('donna_memory_auto_jobs') WHERE name='proof')",
        [],
        |r| r.get(0),
    )?;
    if !has_proof {
        db.execute("ALTER TABLE donna_memory_auto_jobs ADD COLUMN proof TEXT", [])?;
    }
    super::audit::migrate(db)?;
    super::recibo_injecao::migrate(db)?;
    // Abertura do Store ocorre antes dos atores. Falha e auditoria têm o mesmo commit, sem presumir sucesso.
    super::audit::atomic(db, |db| {
        let interrupted = db
            .prepare("SELECT id,context FROM donna_memory_auto_jobs WHERE outcome='running'")?
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        db.execute_batch("UPDATE donna_memory_auto_jobs SET outcome='failed',status='ineligible',reason='interrupted'
            WHERE outcome='running';
            DELETE FROM donna_memory_auto_proposals WHERE job IN (SELECT id FROM donna_memory_auto_jobs WHERE outcome IN ('failed','cancelled'));")?;
        for (id, raw) in interrupted {
            let Ok(context) = serde_json::from_str::<Context>(&raw) else { continue };
            match super::worker::validate_context(db, &context) {
                Ok(()) => {
                    super::audit::record_job(db, &id, super::audit::Reason::Ineligible, None, crate::store::now_ms())?;
                }
                Err(error) if super::worker::is_transient(&error) => return Err(error),
                Err(_) => {}
            }
        }
        Ok(())
    })?;
    super::transport::migrate(db)?;
    super::suggestions::migrate(db)?;
    super::sleep::migrate(db)?;
    migrate_legacy(db)
}

pub(crate) fn hash(raw: &str) -> String {
    Sha256::digest(raw.as_bytes()).iter().fold(String::with_capacity(64), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

/// Captura apenas IDs/revisões persistidos antes de qualquer await de execução.
pub(crate) fn begin(hub: &Hub, cfg: &BotConfig, lane: &Lane, session: &str, ids: &[String]) -> Result<()> {
    // D389 `off`: sem pedido na fila não há extração nem chamada ao modelo.
    if ids.is_empty() || crate::fork::memory_learning::mode(&hub.store)? == crate::fork::memory_learning::Mode::Off {
        return Ok(());
    }
    let runtime = hub.runtime(&cfg.id);
    let token = runtime
        .memory_turn_token
        .as_deref()
        .or(runtime.preparing_turn_token.as_deref())
        .ok_or_else(|| anyhow::anyhow!("memory_turn_missing"))?;
    let owner = |e: &crate::store::Entry| {
        e.bot_id == lane.chat && e.is_user_authored() && e.data["sourceBotId"].is_null() && e.data["runId"].is_null()
    };
    // Evidência humana vem antes de qualquer reuso por token (#238): retomada não
    // transforma resposta não autoral em conflito nem em evidência.
    for id in ids {
        if let Some(e) = hub.store.entry(id) {
            ensure!(owner(&e), "memory_owner_evidence_required");
        }
    }
    // Identidade do pedido, não da tentativa. Um job terminal continua terminal;
    // D72 pode reenviar o pedido, mas não pode gerar outra extração com o mesmo token.
    if let Some(raw) = hub
        .store
        .db
        .locked()
        .query_row(
            "SELECT context FROM donna_memory_auto_jobs WHERE bot=?1 AND json_extract(context,'$.token')=?2",
            params![cfg.id, token],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        let original: Context = serde_json::from_str(&raw)?;
        ensure!(
            original.chat_id == lane.chat
                && original.space_id == cfg.space_id
                && original.memory_id == cfg.memory_id()
                && original.messages.iter().map(|m| &m.id).eq(ids.iter()),
            "memory_request_conflict"
        );
        return Ok(());
    }
    let bot = hub.store.bot(&cfg.id)?.filter(|b| !b.deleted).ok_or_else(|| anyhow::anyhow!("bot_not_found"))?;
    let mut messages = Vec::new();
    let mut initial_marked = hub.store.memory_session_marked(&cfg.id, session)?;
    for id in ids {
        let e = hub.store.entry(id).ok_or_else(|| anyhow::anyhow!("evidence_not_found"))?;
        ensure!(owner(&e), "memory_owner_evidence_required");
        initial_marked |= hub.store.memory_turn_marked(&cfg.id, &lane.chat, e.turn)?;
        messages.push(Reference { id: id.clone(), revision: e.rev });
    }
    let mut context = Context {
        bot_id: cfg.id.clone(),
        chat_id: lane.chat.clone(),
        space_id: cfg.space_id.clone(),
        memory_id: cfg.memory_id().into(),
        backend: prepared_engine(hub, &cfg.id)?.ok_or_else(|| anyhow::anyhow!("memory_engine_unknown"))?,
        bot_revision: bot.rev,
        conversation_revision: if cfg.context.is_some() {
            Some(hub.store.space_conversation(&cfg.space_id, &lane.chat)?.revision)
        } else {
            None
        },
        session: session.into(),
        visible_session: Some(lane.session_id().into()),
        owner_question: None,
        token: token.into(),
        messages,
        responses: Vec::new(),
        closing_turn: None,
        started_at: runtime.started_at.unwrap_or_else(crate::store::now_ms),
        initial_marked,
    };
    let _boundary = hub.store.memory_boundary.locked();
    let db = hub.store.db.locked();
    context.bot_revision = db.query_row(
        "SELECT m.revision FROM donna_memory_auto_bot_bindings m JOIN bots b ON b.id=m.bot
         WHERE m.bot=?1 AND b.deleted=0",
        [&cfg.id],
        |r| r.get(0),
    )?;
    let id = job_id(&context)?;
    db.execute(
        "INSERT OR IGNORE INTO donna_memory_auto_jobs(id,bot,chat,space,context) VALUES(?1,?2,?3,?4,?5)",
        params![id, cfg.id, lane.chat, cfg.space_id, serde_json::to_string(&context)?],
    )?;
    Ok(())
}

fn job_id(context: &Context) -> Result<String> {
    Ok(hash(&format!("memory-auto-v1:{}", serde_json::to_string(context)?)))
}

pub(crate) fn active_job(hub: &Hub, bot: &str) -> Result<Option<String>> {
    let runtime = hub.runtime(bot);
    let Some(token) = runtime.memory_turn_token.or(runtime.preparing_turn_token) else { return Ok(None) };
    Ok(hub.store.db.locked().query_row(
        "SELECT id FROM donna_memory_auto_jobs WHERE bot=?1 AND outcome='running' AND json_extract(context,'$.token')=?2",
        params![bot,token], |r| r.get(0)).optional()?)
}

/// Resultado e admissão são publicados junto do checkpoint Completing. Repetição não muda o resultado.
pub(crate) fn checkpoint(
    store: &Store,
    key: &str,
    raw: &str,
    job: Option<&str>,
    success: bool,
    stopped: bool,
) -> Result<Option<Context>> {
    let _boundary = store.memory_boundary.locked();
    let mut db = store.db.locked();
    let tx = db.transaction()?;
    let now = crate::store::now_ms();
    let mut audit_context = None;
    expire(&tx, now)?;
    let blocked = serde_json::from_str::<Value>(raw)?["completion"]["memory_blocked"] == true;
    if let Some(id) = job {
        let row: Option<(String, String, String)> = tx
            .query_row("SELECT bot,space,outcome FROM donna_memory_auto_jobs WHERE id=?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?;
        if let Some((bot, space, outcome)) = row.filter(|r| r.2 == "running") {
            let _ = outcome;
            let total: i64 = tx.query_row(
                &format!("SELECT COUNT(*) FROM donna_memory_auto_jobs WHERE status IN {ACTIVE}"),
                [],
                |r| r.get(0),
            )?;
            let local: i64 = tx.query_row(
                &format!(
                    "SELECT COUNT(*) FROM donna_memory_auto_jobs WHERE bot=?1 AND space=?2 AND status IN {ACTIVE}"
                ),
                params![bot, space],
                |r| r.get(0),
            )?;
            if success && !stopped && !blocked {
                capture_responses(&tx, id, raw, now)?;
            }
            let question = if success && !stopped && !blocked { super::owner::checkpoint_on(&tx, id)? } else { None };
            let (outcome, status, reason) = if stopped {
                ("cancelled", "ineligible", Some("ineligible"))
            } else if !success {
                ("failed", "ineligible", Some("ineligible"))
            } else if blocked {
                ("success", "ineligible", Some("ineligible"))
            } else if let Some(status) = question {
                ("success", status, Some(status))
            } else if total >= 1000 || local >= 100 {
                ("success", "notQueued", Some("full"))
            } else {
                ("success", "waiting", Some("noSelection"))
            };
            let admitted_order = crate::store::next_rev(&tx)?;
            tx.execute(
                "UPDATE donna_memory_auto_jobs SET outcome=?2,status=?3,reason=?4,admitted_at=?5,admitted_order=?6,
                scope=CASE WHEN ?7 THEN 'blocked' ELSE scope END,
                scope_revision=scope_revision+CASE WHEN ?7 THEN 1 ELSE 0 END WHERE id=?1",
                params![id, outcome, status, reason, now, admitted_order, question.is_some()],
            )?;
            if blocked {
                set_scope(&tx, id, 1, false)?;
            }
            if !success || stopped || blocked {
                tx.execute("DELETE FROM donna_memory_auto_proposals WHERE job=?1", [id])?;
            }
            let audit_reason = match reason {
                Some("full") => Some(super::audit::Reason::Full),
                Some("ineligible") => Some(super::audit::Reason::Ineligible),
                // noSelection inicial não é observação de uma tentativa do worker.
                _ => None,
            };
            if let Some(reason) = audit_reason
                && super::audit::record_job(&tx, id, reason, None, now)?
            {
                let raw: String =
                    tx.query_row("SELECT context FROM donna_memory_auto_jobs WHERE id=?1", [id], |r| r.get(0))?;
                audit_context = Some(serde_json::from_str(&raw)?);
            }
        }
    }
    tx.execute("INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v", params![key, raw])?;
    tx.commit()?;
    Ok(audit_context)
}

/// Somente invalidação da ocorrência já confirmada; não anuncia aprendizado nem cria Entry.
pub(crate) fn notify_checkpoint(hub: &Hub, context: &Context) {
    super::audit::notify(hub, context);
}

pub(crate) fn expire(db: &Connection, now: i64) -> Result<()> {
    super::audit::atomic(db, |db| {
        let jobs = db
            .prepare(
                "SELECT id FROM donna_memory_auto_jobs WHERE status IN ('waiting','reserved') AND admitted_at<=?1",
            )?
            .query_map([now - LIFE_MS], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        db.execute(
            "UPDATE donna_memory_auto_jobs SET status='expired',reason='expired',reservation=NULL
        WHERE status IN ('waiting','reserved') AND admitted_at<=?1",
            [now - LIFE_MS],
        )?;
        for job in jobs {
            super::audit::record_job(db, &job, super::audit::Reason::Expired, None, now)?;
        }
        Ok(())
    })
}

#[derive(Clone, Debug)]
pub(crate) struct Reservation {
    pub id: String,
    pub token: String,
    pub scope_revision: i64,
    pub context: Context,
}

/// Uma reserva abandonada é recuperável; o token impede o worker antigo de concluir.
pub(crate) fn reserve(store: &Store, bot: Option<&str>) -> Result<Option<Reservation>> {
    let mut db = store.db.locked();
    let tx = db.transaction()?;
    let now = crate::store::now_ms();
    expire(&tx, now)?;
    recover(&tx, now)?;
    let row: Option<(String, String, i64)> = tx
        .query_row(
            "SELECT j.id,j.context,j.scope_revision FROM donna_memory_auto_jobs j
         WHERE j.status='waiting' AND j.outcome='success' AND j.scope='allowed' AND (?1 IS NULL OR j.bot=?1)
         AND NOT EXISTS(SELECT 1 FROM donna_memory_auto_jobs older WHERE older.chat=j.chat
            AND older.status IN ('waiting','reserved') AND older.outcome='success' AND older.scope='allowed'
            AND (older.admitted_at<j.admitted_at OR (older.admitted_at=j.admitted_at
             AND (older.admitted_order<j.admitted_order OR (older.admitted_order=j.admitted_order AND older.rowid<j.rowid)))))
         ORDER BY COALESCE(j.reserved_at,0),j.admitted_at,j.admitted_order,j.rowid LIMIT 1",
            [bot],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let result = row.map(|(id,raw,scope_revision)| -> Result<Reservation> {
        let token = uuid::Uuid::new_v4().to_string();
        tx.execute("UPDATE donna_memory_auto_jobs SET status='reserved',reserved_at=?2,reservation=?3,attempts=attempts+1 WHERE id=?1",
            params![id,now,token])?;
        Ok(Reservation { id, token, scope_revision, context: serde_json::from_str(&raw)? })
    }).transpose()?;
    tx.commit()?;
    Ok(result)
}

/// Disposição persistente por pedido, pronta para o produtor canônico da Parte B.
/// Não altera resultado e nunca reabilita falha/cancelamento.
pub(crate) fn set_scope(db: &Connection, id: &str, expected: i64, allowed: bool) -> Result<bool> {
    super::audit::atomic(db, |db| {
        let row: Option<(String, String, Option<i64>, String, String)> = db
        .query_row(
            "SELECT outcome,status,admitted_at,bot,space FROM donna_memory_auto_jobs WHERE id=?1 AND scope_revision=?2
             AND (?3=0 OR json_extract(context,'$.ownerQuestion') IS NULL)",
            params![id, expected, allowed],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
        let Some((outcome, status, admitted, bot, space)) = row else { return Ok(false) };
        let mut next = status.clone();
        if !allowed && matches!(status.as_str(), "waiting" | "reserved") {
            next = "ineligible".into();
        }
        if allowed && outcome == "success" && status == "ineligible" {
            let total: i64 = db.query_row(
                "SELECT COUNT(*) FROM donna_memory_auto_jobs WHERE status IN ('waiting','reserved')",
                [],
                |r| r.get(0),
            )?;
            let local:i64=db.query_row("SELECT COUNT(*) FROM donna_memory_auto_jobs WHERE bot=?1 AND space=?2 AND status IN ('waiting','reserved')",params![bot,space],|r|r.get(0))?;
            next = if admitted.is_none_or(|at| at <= crate::store::now_ms() - LIFE_MS) {
                "expired"
            } else if total >= 1000 || local >= 100 {
                "notQueued"
            } else {
                "waiting"
            }
            .into();
        }
        let changed = db.execute("UPDATE donna_memory_auto_jobs SET scope=?3,scope_revision=scope_revision+1,status=?4,
        reason=CASE ?4 WHEN 'expired' THEN 'expired' WHEN 'notQueued' THEN 'full' WHEN 'waiting' THEN 'noSelection' ELSE 'ineligible' END,
        reservation=NULL WHERE id=?1 AND scope_revision=?2",
        params![id,expected,if allowed { "allowed" } else { "blocked" },next])? == 1;
        let reason = match next.as_str() {
            "expired" => Some(super::audit::Reason::Expired),
            "notQueued" => Some(super::audit::Reason::Full),
            "ineligible" => Some(super::audit::Reason::Ineligible),
            _ => None,
        };
        if changed && let Some(reason) = reason {
            super::audit::record_job(db, id, reason, None, crate::store::now_ms())?;
        }
        Ok(changed)
    })
}

pub(crate) fn eligible(db: &Connection, bot: &str, chat: &str, message: &str, revision: i64) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_memory_auto_jobs j,json_each(j.context,'$.messages') r
        WHERE j.bot=?1 AND j.chat=?2 AND j.outcome='success' AND j.scope='allowed'
        AND json_extract(r.value,'$.id')=?3 AND json_extract(r.value,'$.revision')=?4)",
        params![bot, chat, message, revision],
        |r| r.get(0),
    )?)
}

pub(crate) fn disposition(store: &Store, bot: &str, chat: &str) -> Result<Value> {
    let db = store.db.locked();
    expire(&db, crate::store::now_ms())?;
    let state: Option<(String, Option<String>, String)> = db
        .query_row(
            "SELECT status,reason,id FROM donna_memory_auto_jobs WHERE bot=?1 AND chat=?2 AND outcome<>'running'
        ORDER BY admitted_at DESC,admitted_order DESC,rowid DESC LIMIT 1",
            params![bot, chat],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    Ok(state.map_or(Value::Null, |(status, reason, id)| json!({"status":status,"availability":reason,"jobId":id})))
}

/// Migra somente referências, sem aceite retroativo. Sem prova original, não envia ao executor.
fn migrate_legacy(db: &Connection) -> Result<()> {
    let cards = db
        .prepare(
            "SELECT id,bot_id,data FROM entries WHERE kind='notice'
        AND (json_extract(data,'$.memory.status')='pending' OR json_extract(data,'$.memoryFile.status')='pending')",
        )?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let now = crate::store::now_ms();
    for (notice, chat, raw) in cards {
        let data: Value = serde_json::from_str(&raw)?;
        let author = data["author"].as_str().unwrap_or(&chat);
        let cfg: Option<String> =
            db.query_row("SELECT config FROM bots WHERE id=?1 AND deleted=0", [author], |r| r.get(0)).optional()?;
        let Some(cfg) = cfg else { continue };
        let cfg: BotConfig = serde_json::from_str(&cfg)?;
        let tx = db.unchecked_transaction()?;
        let previous: Option<String> = tx
            .query_row("SELECT disposition FROM donna_memory_auto_legacy WHERE notice=?1", [&notice], |r| r.get(0))
            .optional()?;
        if previous.as_deref().is_some_and(|s| s != "full") {
            continue;
        }
        expire(&tx, now)?;
        let total: i64 = tx.query_row(
            "SELECT COUNT(*) FROM donna_memory_auto_jobs WHERE status IN ('waiting','reserved')",
            [],
            |r| r.get(0),
        )?;
        let local:i64=tx.query_row("SELECT COUNT(*) FROM donna_memory_auto_jobs WHERE bot=?1 AND space=?2 AND status IN ('waiting','reserved')",params![author,cfg.space_id],|r|r.get(0))?;
        let full = total >= 1000 || local >= 100;
        let id = format!("legacy-{}", hash(&notice));
        if !full {
            // Origem desconhecida e ausência de sucesso não são promovidas pela migração.
            let message = data["memory"]["message"].as_str();
            let mut messages = Vec::new();
            if let Some(message) = message
                && let Some(revision) = tx
                    .query_row("SELECT rev FROM entries WHERE id=?1 AND bot_id=?2", params![message, chat], |r| {
                        r.get(0)
                    })
                    .optional()?
            {
                messages.push(Reference { id: message.into(), revision });
            }
            let context = Context {
                bot_id: author.into(),
                chat_id: chat.clone(),
                space_id: cfg.space_id.clone(),
                memory_id: cfg.memory_id().into(),
                backend: String::new(),
                bot_revision: 0,
                conversation_revision: None,
                session: String::new(),
                visible_session: None,
                owner_question: None,
                token: id.clone(),
                messages,
                responses: Vec::new(),
                closing_turn: None,
                started_at: now,
                initial_marked: true,
            };
            let admitted_order = crate::store::next_rev(&tx)?;
            tx.execute("INSERT OR IGNORE INTO donna_memory_auto_jobs(id,bot,chat,space,context,outcome,status,reason,admitted_at,admitted_order)
                VALUES(?1,?2,?3,?4,?5,'unknown','waiting','ineligible',?6,?7)",params![id,author,chat,cfg.space_id,serde_json::to_string(&context)?,now,admitted_order])?;
        }
        let state = if full { "full" } else { "ineligible" };
        tx.execute("INSERT INTO donna_memory_auto_legacy(notice,job,disposition,admitted_at) VALUES(?1,?2,?3,?4)
            ON CONFLICT(notice) DO UPDATE SET job=excluded.job,disposition=excluded.disposition,admitted_at=excluded.admitted_at",
            params![notice,(!full).then_some(&id),state,now])?;
        let field = if data["memory"].is_object() { "memory" } else { "memoryFile" };
        let mut updated = data.clone();
        updated[field]["confirmation"] = json!(false);
        updated[field]["availability"] = json!(state);
        updated[field]["reason"] = json!(if full {
            "Fila de memória cheia; o pedido continua no histórico."
        } else {
            "Sem prova durável do sucesso original; não aplicado."
        });
        if !full {
            updated[field]["status"] = json!("waiting");
            updated[field]["jobId"] = json!(id);
        }
        updated["text"] = json!("Memória automática não aplicada; pedido preservado no histórico.");
        let rev = crate::store::next_rev(&tx)?;
        tx.execute(
            "UPDATE entries SET data=?2,rev=?3,updated_at=?4 WHERE id=?1",
            params![notice, updated.to_string(), rev, now],
        )?;
        tx.commit()?;
    }
    Ok(())
}

/// Revisão diária reapresenta somente evidência que já tem sucesso e escopo duráveis.
/// Não há leitura bruta de histórico, nem promoção de memória legada a fala do dono.
pub(crate) fn schedule_review(store: &Store, bot: &str) -> Result<bool> {
    let mut db = store.db.locked();
    let tx = db.transaction()?;
    let now = crate::store::now_ms();
    expire(&tx, now)?;
    let day = chrono::Local::now().format("%Y-%m-%d").to_string();
    let review_key = format!("memory.auto.review.{bot}");
    let done: Option<String> = tx.query_row("SELECT v FROM kv WHERE k=?1", [&review_key], |r| r.get(0)).optional()?;
    if done.as_deref() == Some(day.as_str()) {
        return Ok(false);
    }
    let row: Option<(String, String, String)> = tx
        .query_row(
            "SELECT j.id,j.context,j.space FROM donna_memory_auto_jobs j
        WHERE j.bot=?1 AND j.outcome='success' AND j.scope='allowed' AND j.status='applied'
        AND EXISTS(SELECT 1 FROM memory_entries m,json_each(j.context,'$.messages') r
            WHERE m.bot=json_extract(j.context,'$.memoryId') AND m.state='active' AND m.author<>'user'
            AND m.evidence LIKE '%'||json_extract(r.value,'$.id')||'%'
            AND json_extract(m.origin,'$.kind') IN ('user','conversation','external'))
        ORDER BY j.admitted_at DESC,j.admitted_order DESC,j.rowid DESC LIMIT 1",
            [bot],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((source, raw, space)) = row else { return Ok(false) };
    let context: Context = serde_json::from_str(&raw)?;
    if super::worker::validate_context(&tx, &context).is_err() {
        return Ok(false);
    }
    let id = hash(&format!("review:{day}:{source}"));
    let local: i64 = tx.query_row(
        "SELECT COUNT(*) FROM donna_memory_auto_jobs WHERE bot=?1 AND space=?2 AND status IN ('waiting','reserved')",
        params![bot, space],
        |r| r.get(0),
    )?;
    let total: i64 =
        tx.query_row("SELECT COUNT(*) FROM donna_memory_auto_jobs WHERE status IN ('waiting','reserved')", [], |r| {
            r.get(0)
        })?;
    if local >= 100 || total >= 1000 {
        return Ok(false);
    }
    let admitted_order = crate::store::next_rev(&tx)?;
    let inserted = tx.execute(
        "INSERT OR IGNORE INTO donna_memory_auto_jobs(id,bot,chat,space,context,outcome,status,reason,admitted_at,admitted_order)
        VALUES(?1,?2,?3,?4,?5,'success','waiting','noSelection',?6,?7)",
        params![id, bot, context.chat_id, space, raw, now, admitted_order],
    )? == 1;
    if inserted {
        tx.execute(
            "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
            params![review_key, day],
        )?;
    }
    tx.commit()?;
    Ok(inserted)
}

/// Apenas referências: a resposta vem do checkpoint de conclusão bem-sucedido, nunca de inferência do histórico.
fn capture_responses(db: &Connection, job: &str, raw: &str, now: i64) -> Result<()> {
    let saved: String = db.query_row("SELECT context FROM donna_memory_auto_jobs WHERE id=?1", [job], |r| r.get(0))?;
    let mut c: Context = serde_json::from_str(&saved)?;
    let waiting: Value = serde_json::from_str(raw)?;
    let completion = &waiting["completion"];
    let closing_turn =
        completion["turn"].as_i64().filter(|turn| *turn > 0).ok_or(super::worker::InvalidSnapshot::Evidence)?;
    c.closing_turn = Some(closing_turn);
    let mut ids = Vec::new();
    if let Some(id) = completion["last_text"].as_str() {
        ids.push(id.to_owned());
    }
    if let Some(parts) = completion["stream"].as_array()
        && let (Some(id), Some(data)) = (parts.first().and_then(Value::as_str), parts.get(1))
    {
        let valid: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM entries WHERE id=?1 AND bot_id=?2 AND kind='agent' AND turn=?3)",
            params![id, c.chat_id, closing_turn],
            |r| r.get(0),
        )?;
        if valid {
            let data = crate::fork::spaces::stamp_entry(db, &c.chat_id, data)?;
            let rev = crate::store::next_rev(db)?;
            db.execute(
                "UPDATE entries SET data=?2,rev=?3,updated_at=?4 WHERE id=?1",
                params![id, data.to_string(), rev, now],
            )?;
            if !ids.iter().any(|old| old == id) {
                ids.push(id.into());
            }
        }
    }
    // send_message já persistiu suas entradas; só o texto listado no fechamento serve como prova.
    if let Some(sent) = completion["sent"].as_array() {
        for text in sent.iter().filter_map(Value::as_str) {
            let id: Option<String> = db
                .query_row(
                    "SELECT id FROM entries WHERE bot_id=?1 AND kind='agent'
                AND created_at>=?2 AND json_extract(data,'$.text')=?3 AND turn=?4 ORDER BY seq DESC LIMIT 1",
                    params![c.chat_id, c.started_at, text, closing_turn],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(id) = id
                && !ids.contains(&id)
            {
                ids.push(id);
            }
        }
    }
    for id in ids {
        let raw: Option<String> = db
            .query_row(
                "SELECT data FROM entries WHERE id=?1 AND bot_id=?2 AND kind='agent' AND turn=?3",
                params![id, c.chat_id, closing_turn],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(raw) = raw {
            let data: Value = serde_json::from_str(&raw)?;
            if data["author"].as_str() == Some(c.bot_id.as_str()) {
                c.responses.push(ResponseReference { id, hash: hash(data["text"].as_str().unwrap_or_default()) });
            }
        }
    }
    db.execute("UPDATE donna_memory_auto_jobs SET context=?2 WHERE id=?1", params![job, serde_json::to_string(&c)?])?;
    Ok(())
}

/// Snapshot do motor efetivamente aberto, antes de session/new ou qualquer await do motor.
/// Configuração editada não troca o motor em voo; o anúncio exige o vínculo original ainda válido.
pub(crate) fn capture_engine(hub: &Hub, cfg: &BotConfig, lane: &Lane, backend: &str) -> Result<()> {
    let runtime = hub.runtime(&cfg.id);
    let token = runtime
        .memory_turn_token
        .or(runtime.preparing_turn_token)
        .ok_or_else(|| anyhow::anyhow!("memory_turn_missing"))?;
    hub.store.db.locked().execute(
        "INSERT INTO donna_memory_auto_engines(bot,token,chat,backend) VALUES(?1,?2,?3,?4)
        ON CONFLICT(bot) DO UPDATE SET token=excluded.token,chat=excluded.chat,backend=excluded.backend",
        params![cfg.id, token, lane.chat, backend],
    )?;
    Ok(())
}

pub(crate) fn engine(hub: &Hub, bot: &str) -> Result<Option<String>> {
    let runtime = hub.runtime(bot);
    let Some(token) = runtime.memory_turn_token else { return Ok(None) };
    let Some(lane) = runtime.lane else { return Ok(None) };
    Ok(hub
        .store
        .db
        .locked()
        .query_row(
            "SELECT backend FROM donna_memory_auto_engines WHERE bot=?1 AND token=?2 AND chat=?3",
            params![bot, token, lane.chat],
            |r| r.get(0),
        )
        .optional()?)
}

/// Snapshot de preparação não constitui execução ativa nem autoridade de escrita.
pub(crate) fn prepared_engine(hub: &Hub, bot: &str) -> Result<Option<String>> {
    let runtime = hub.runtime(bot);
    let Some(token) = runtime.memory_turn_token.or(runtime.preparing_turn_token) else { return Ok(None) };
    let Some(lane) = runtime.lane else { return Ok(None) };
    Ok(hub
        .store
        .db
        .locked()
        .query_row(
            "SELECT backend FROM donna_memory_auto_engines WHERE bot=?1 AND token=?2 AND chat=?3",
            params![bot, token, lane.chat],
            |r| r.get(0),
        )
        .optional()?)
}

/// CAS local para aplicar propostas já autenticadas, sem abrir processo de fundo.
pub(crate) fn reserve_job(store: &Store, id: &str) -> Result<Option<Reservation>> {
    let mut db = store.db.locked();
    let tx = db.transaction()?;
    let now = crate::store::now_ms();
    expire(&tx, now)?;
    let row: Option<(String, i64)> = tx
        .query_row(
            "SELECT context,scope_revision FROM donna_memory_auto_jobs
        WHERE id=?1 AND status='waiting' AND outcome='success' AND scope='allowed'",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((raw, scope_revision)) = row else { return Ok(None) };
    let token = uuid::Uuid::new_v4().to_string();
    tx.execute(
        "UPDATE donna_memory_auto_jobs SET status='reserved',reserved_at=?2,reservation=?3 WHERE id=?1",
        params![id, now, token],
    )?;
    tx.commit()?;
    Ok(Some(Reservation { id: id.into(), token, scope_revision, context: serde_json::from_str(&raw)? }))
}

/// Uma tentativa interrompida por limite não leva propostas para a próxima execução.
pub(crate) fn abandon(hub: &Hub, bot: &str) -> Result<()> {
    let Some(id) = active_job(hub, bot)? else { return Ok(()) };
    let boundary = hub.store.memory_boundary.locked();
    let mut db = hub.store.db.locked();
    let tx = db.transaction()?;
    let context: Option<String> = tx
        .query_row("SELECT context FROM donna_memory_auto_jobs WHERE id=?1 AND outcome='running'", [&id], |r| r.get(0))
        .optional()?;
    let Some(context) = context else { return Ok(()) };
    let context: Context = serde_json::from_str(&context)?;
    tx.execute("UPDATE donna_memory_auto_jobs SET outcome='failed',status='ineligible',reason='interrupted' WHERE id=?1 AND outcome='running'",[&id])?;
    tx.execute("DELETE FROM donna_memory_auto_proposals WHERE job=?1", [&id])?;
    let changed = super::audit::record_job(&tx, &id, super::audit::Reason::Ineligible, None, crate::store::now_ms())?;
    tx.commit()?;
    drop(db);
    drop(boundary);
    if changed {
        super::audit::notify(hub, &context);
    }
    Ok(())
}

pub(crate) fn recover(db: &Connection, now: i64) -> Result<()> {
    super::audit::atomic(db, |db| {
        expire(db, now)?;
        let jobs = db
            .prepare("SELECT id FROM donna_memory_auto_jobs WHERE status='reserved' AND reserved_at<=?1")?
            .query_map([now - RESERVATION_MS], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        db.execute(
            "UPDATE donna_memory_auto_jobs SET status='waiting',reservation=NULL,reason='transient'
        WHERE status='reserved' AND reserved_at<=?1",
            [now - RESERVATION_MS],
        )?;
        for job in jobs {
            super::audit::record_job(db, &job, super::audit::Reason::TechnicalDeferred, None, now)?;
        }
        Ok(())
    })
}
