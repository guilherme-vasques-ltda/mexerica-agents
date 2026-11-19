//! D389 `suggest`: a mudança que o bot faria fica pendente até o dono aprovar no app.
//! O pendente mora só nesta tabela, nunca em `memory_entries` ou nas notas: não entra no contexto
//! de abertura, no `search_memory` nem nos tetos de tokens. Descartar guarda o item (D369) e o
//! mesmo fato não volta a ser sugerido para a mesma memória.
use super::{
    queue::{Context, Reservation},
    worker::Operation,
};
use crate::{
    LockExt,
    fork::autoridade::Autoridade,
    hub::Hub,
    store::{BotConfig, Store},
};
use anyhow::{Result, anyhow, ensure};
use codync_mem::store as mem;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_memory_suggestions(
          id TEXT PRIMARY KEY,job TEXT NOT NULL,bot TEXT NOT NULL,memory TEXT NOT NULL,
          shared INTEGER NOT NULL DEFAULT 0,operation TEXT NOT NULL,text TEXT,origin TEXT NOT NULL,
          nature TEXT NOT NULL,fact TEXT NOT NULL,status TEXT NOT NULL DEFAULT 'pending',
          receipt TEXT,created_at INTEGER NOT NULL,decided_at INTEGER);
         CREATE INDEX IF NOT EXISTS donna_memory_suggestions_bot ON donna_memory_suggestions(bot,status);
         CREATE INDEX IF NOT EXISTS donna_memory_suggestions_fact ON donna_memory_suggestions(memory,fact);",
    )?;
    Ok(())
}

/// Quantas sugestões esperam o dono, em todos os bots (contador do cartão de Ajustes).
pub(crate) fn pending(store: &Store) -> Result<i64> {
    Ok(store.db.locked().query_row(
        "SELECT COUNT(*) FROM donna_memory_suggestions WHERE status='pending'",
        [],
        |r| r.get(0),
    )?)
}

/// Pendentes desta memória, para a seção Sugestões. Só leitura; não chega ao bot.
pub(crate) fn list(store: &Store, cfg: &BotConfig) -> Result<Vec<Value>> {
    let db = store.db.locked();
    let rows = db
        .prepare(
            "SELECT id,operation,text,origin,nature,shared,created_at FROM donna_memory_suggestions
            WHERE bot=?1 AND memory=?2 AND status='pending' ORDER BY created_at,rowid LIMIT 100",
        )?
        .query_map(params![cfg.id, cfg.memory_id()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, bool>(5)?,
                r.get::<_, i64>(6)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(id, operation, text, origin, nature, shared, at)| {
            // Só o texto conferido quando a sugestão nasceu, nunca uma leitura nova da memória pelo ID.
            let op: Value = serde_json::from_str(&operation)?;
            Ok(json!({"id":id,"action":op["action"],"text":text,"file":op.get("file"),"shared":shared,
                "origin":serde_json::from_str::<Value>(&origin)?,"nature":nature,"createdAt":at}))
        })
        .collect()
}

/// Gesto do dono: o item fica guardado como descartado, nunca apagado. A autoridade ORIGINAL do pedido é
/// conferida na mesma Tx da gravação (SEG-491-01).
pub(crate) fn discard(hub: &Hub, cfg: &BotConfig, id: &str, autoridade: &Autoridade) -> Result<Value> {
    autoridade.with_db(hub, None, |db| discard_in(db, cfg, id))
}

fn discard_in(db: &Connection, cfg: &BotConfig, id: &str) -> Result<Value> {
    db.execute(
        "UPDATE donna_memory_suggestions SET status='discarded',decided_at=?4
        WHERE id=?1 AND bot=?2 AND memory=?3 AND status='pending'",
        params![id, cfg.id, cfg.memory_id(), crate::store::now_ms()],
    )?;
    let status: String = db
        .query_row(
            "SELECT status FROM donna_memory_suggestions WHERE id=?1 AND bot=?2 AND memory=?3",
            params![id, cfg.id, cfg.memory_id()],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| anyhow!("memory_suggestion_not_found"))?;
    Ok(json!({"status":status}))
}

/// Chave do fato para "não sugerir de novo": mesma memória, mesma ação e mesmo texto normalizado.
fn fact(op: &Operation) -> String {
    let body = match op {
        Operation::Remember { text, shared, .. } => format!("remember/{shared}/{}", mem::fingerprint(text)),
        Operation::Revise { entry, text, .. } => format!("revise/{entry}/{}", mem::fingerprint(text)),
        Operation::Archive { entry, .. } => format!("archive/{entry}"),
        Operation::Note { file, content, shared, .. } => {
            format!("note/{shared}/{file}/{}", content.as_deref().map(mem::fingerprint).unwrap_or_default())
        }
        Operation::NotDurable { .. } => "notDurable".into(),
        Operation::Confirm { entry, .. } => format!("confirm/{entry}"),
        Operation::Reclassify { entry, .. } => format!("reclassify/{entry}"),
    };
    super::queue::hash(&body)
}

/// O id determinístico da sugestão desta operação neste trabalho.
pub(crate) fn id(job: &str, op: &Operation) -> Result<String> {
    Ok(format!("memory-suggestion-{job}-{}", super::queue::hash(&serde_json::to_string(op)?)))
}

/// Dentro da transação do commit (modo `suggest`), depois de o `apply_one` da mesma operação passar num SAVEPOINT
/// desfeito (alvo, camada, revisão e compartilhamento já conferidos): guarda o que seria gravado, com a origem, a
/// natureza e o texto que teria. Repetição, pendente igual ou fato já descartado não criam outro item.
pub(crate) fn park(db: &Connection, job: &str, c: &Context, op: &Operation, now: i64) -> Result<bool> {
    let fact = fact(op);
    let known: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_memory_suggestions WHERE memory=?1 AND fact=?2
        AND status IN ('pending','discarded'))",
        params![c.memory_id, fact],
        |r| r.get(0),
    )?;
    if known {
        return Ok(false);
    }
    let (shared, text) = match op {
        Operation::Remember { shared, text, .. } => (*shared, Some(text.clone())),
        Operation::Note { shared, content, .. } => (*shared, content.clone()),
        Operation::Revise { entry, text, .. } => {
            (mem::get(db, *entry)?.ok_or(mem::Error::NotFound)?.bot.is_none(), Some(text.clone()))
        }
        // Arquivar não traz texto: guarda o da lembrança que o `apply_one` acabou de aceitar como visível.
        Operation::Archive { entry, .. } | Operation::Confirm { entry, .. } | Operation::Reclassify { entry, .. } => {
            let old = mem::get(db, *entry)?.ok_or(mem::Error::NotFound)?;
            (old.bot.is_none(), Some(old.text))
        }
        Operation::NotDurable { .. } => anyhow::bail!("memory_not_durable_requires_extractor"),
    };
    let origin = super::origin::derive(db, c, op.evidence(), op.text())?;
    let literal = !op.text().is_empty() && super::origin::literal_owner(db, c, op.evidence(), op.text())?;
    let nature = match op {
        Operation::Reclassify { .. } => serde_json::to_value(codync_mem::nature::KindNature::Confirmed)?,
        _ => serde_json::to_value(codync_mem::nature::proposed(literal, op.nature()))?,
    };
    let raw = serde_json::to_string(op)?;
    let id = id(job, op)?;
    Ok(db.execute(
        "INSERT OR IGNORE INTO donna_memory_suggestions(id,job,bot,memory,shared,operation,origin,nature,fact,created_at,text)
        VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            id,
            job,
            c.bot_id,
            c.memory_id,
            shared,
            raw,
            serde_json::to_string(&origin)?,
            nature.as_str().unwrap_or("unknown"),
            fact,
            now,
            text
        ],
    )? == 1)
}

/// Guarda Connection-only da aprovação: contexto atual válido e a sugestão ainda pendente.
pub(crate) fn check_pending(db: &Connection, id: &str, c: &Context) -> Result<()> {
    super::worker::validate_context(db, c)?;
    let pending: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_memory_suggestions WHERE id=?1 AND job IS NOT NULL AND bot=?2
        AND status='pending')",
        params![id, c.bot_id],
        |r| r.get(0),
    )?;
    ensure!(pending, "memory_suggestion_not_pending");
    Ok(())
}

/// Fecha a sugestão na mesma transação do recibo; sem recibo, o fato já estava na memória.
pub(crate) fn settle(db: &Connection, id: &str, receipt: Option<&String>, now: i64) -> Result<()> {
    db.execute(
        "UPDATE donna_memory_suggestions SET status=?2,receipt=?3,decided_at=?4 WHERE id=?1 AND status='pending'",
        params![id, if receipt.is_some() { "approved" } else { "noChange" }, receipt, now],
    )?;
    Ok(())
}

/// O contexto do pedido com as revisões atuais: aprovar é intenção atual do dono, como Desfazer.
fn current_context(db: &Connection, job: &str) -> Result<Context> {
    let raw: String = db.query_row("SELECT context FROM donna_memory_auto_jobs WHERE id=?1", [job], |r| r.get(0))?;
    let mut c: Context = serde_json::from_str(&raw)?;
    c.bot_revision = db
        .query_row(
            "SELECT m.revision FROM donna_memory_auto_bot_bindings m JOIN bots b ON b.id=m.bot
            WHERE m.bot=?1 AND b.deleted=0",
            [&c.bot_id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(super::worker::InvalidSnapshot::Context)?;
    if c.conversation_revision.is_some() {
        c.conversation_revision = Some(
            db.query_row(
                "SELECT revision FROM donna_space_conversations WHERE id=?1 AND bot_id=?2 AND space_id=?3",
                params![c.chat_id, c.bot_id, c.space_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(super::worker::InvalidSnapshot::Context)?,
        );
    }
    Ok(c)
}

fn stale(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        matches!(cause.downcast_ref::<mem::Error>(), Some(mem::Error::NotFound))
            || ["memory_revision_changed", "memory_note_changed"].iter().any(|code| cause.to_string().contains(code))
    })
}

/// Gesto do dono no app: aplica como o modo automático (recibo, Desfazer, versão). Se a memória
/// mudou desde a sugestão, recusa com aviso e não sobrescreve.
/// A autoridade ORIGINAL do pedido segue até a Tx do commit (SEG-491-01); revogada no meio, nada muda.
pub(crate) fn approve(hub: &Hub, cfg: &BotConfig, id: &str, autoridade: &Autoridade) -> Result<Value> {
    let (job, operation, status, context) = {
        let db = hub.store.db.locked();
        let row: (String, String, String) = db
            .query_row(
                "SELECT job,operation,status FROM donna_memory_suggestions WHERE id=?1 AND bot=?2 AND memory=?3",
                params![id, cfg.id, cfg.memory_id()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?
            .ok_or_else(|| anyhow!("memory_suggestion_not_found"))?;
        let context = if row.2 == "pending" { Some(current_context(&db, &row.0)?) } else { None };
        (row.0, row.1, row.2, context)
    };
    let Some(context) = context else { return Ok(json!({"status":status})) };
    let reservation = Reservation { id: job, token: String::new(), scope_revision: 0, context };
    match super::apply::approve(hub, &reservation, serde_json::from_str(&operation)?, id, autoridade) {
        Ok(ids) => Ok(json!({"status": if ids.is_empty() { "noChange" } else { "approved" }, "receipt": ids.first()})),
        Err(error) if stale(&error) => {
            autoridade.with_db(hub, None, |db| {
                db.execute(
                    "UPDATE donna_memory_suggestions SET status='stale',decided_at=?2 WHERE id=?1 AND status='pending'",
                    params![id, crate::store::now_ms()],
                )?;
                Ok(())
            })?;
            anyhow::bail!("memory_suggestion_stale: A memória mudou depois desta sugestão. Nada foi sobrescrito.")
        }
        Err(error) => Err(error),
    }
}
