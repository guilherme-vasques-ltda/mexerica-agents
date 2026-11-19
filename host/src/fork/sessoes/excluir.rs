//! D545: exclusão de extra pelo dono, serializada pelo ator. Tombstone sem conteúdo para repetição e catch-up.
use super::{encerrar, store};
use crate::api::devices::Caller;
use crate::fork::autoridade::Autoridade;
use crate::hub::Hub;
use crate::store::{next_rev, now_ms};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

pub(crate) const ACTION: &str = "delete";

pub(super) fn migrate(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_deleted_sessions(
            id TEXT PRIMARY KEY, chat TEXT NOT NULL, space TEXT NOT NULL,
            original_revision INTEGER NOT NULL, revision INTEGER NOT NULL);
         CREATE INDEX IF NOT EXISTS donna_deleted_sessions_space ON donna_deleted_sessions(space,revision);
         CREATE TRIGGER IF NOT EXISTS donna_deleted_session_insert BEFORE INSERT ON entries
         WHEN EXISTS(SELECT 1 FROM donna_deleted_sessions d WHERE d.chat=NEW.bot_id AND d.id=NEW.session)
         BEGIN SELECT RAISE(ABORT,'session_deleted'); END;
         CREATE TRIGGER IF NOT EXISTS donna_deleted_session_update BEFORE UPDATE ON entries
         WHEN EXISTS(SELECT 1 FROM donna_deleted_sessions d WHERE d.chat=NEW.bot_id AND d.id=NEW.session)
         BEGIN SELECT RAISE(ABORT,'session_deleted'); END;",
    )?;
    Ok(())
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Request {
    pub hub_id: String,
    pub space_id: String,
    pub bot_id: String,
    pub session_id: String,
    pub expected_revision: i64,
}

pub(crate) async fn delete(hub: &Arc<Hub>, caller: &Caller, body: Value) -> Result<Value> {
    crate::fork::bot_editor::authorization::require_owner(caller)?;
    let original = Autoridade::capturar(hub, caller)?;
    let request: Request = serde_json::from_value(body).map_err(|_| anyhow!("invalid_session_delete"))?;
    ensure!(request.hub_id == hub.host_id, "hub_context_mismatch");
    encerrar::live_bot(hub, &request.bot_id)?;
    let _guard = hub.store.team_lock.lock().await;
    let (reply, result) = tokio::sync::oneshot::channel();
    hub.send_cmd(
        &request.bot_id,
        crate::agent::bot::Cmd::SessionDelete {
            request: Box::new(request.clone()),
            autoridade: Box::new(original),
            reply,
        },
    )?;
    tokio::time::timeout(Duration::from_secs(10), result).await.map_err(|_| anyhow!("session_control_pending"))??
}

fn event(request: &Request, revision: i64) -> Value {
    json!({"type":"sessionDeleted", "spaceId":request.space_id, "chatId":request.bot_id,
        "sessionId":request.session_id, "rev":revision})
}

/// Só a Connection da transação IMMEDIATE da autoridade. Sem Store, await, memória ou rotinas.
pub(crate) fn delete_on(c: &Connection, request: &Request) -> Result<Value> {
    let space: Option<String> = c
        .query_row("SELECT space_id FROM donna_space_conversations WHERE id=?1 AND bot_id=?1", [&request.bot_id], |r| {
            r.get(0)
        })
        .optional()?;
    ensure!(space.as_deref() == Some(&request.space_id), "cross_space_reference");
    ensure!(store::personal_on(c, &request.bot_id)?, "session_chat_unsupported");
    let removed: Option<(String, String, i64, i64)> = c
        .query_row(
            "SELECT chat,space,original_revision,revision FROM donna_deleted_sessions WHERE id=?1",
            [&request.session_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    if let Some((chat, space, expected, revision)) = removed {
        ensure!(chat == request.bot_id && space == request.space_id, "unknown_session");
        ensure!(expected == request.expected_revision, "session_revision_conflict");
        return Ok(event(request, revision));
    }
    let session = store::get(c, &request.session_id)?
        .filter(|s| s.chat_id == request.bot_id)
        .ok_or_else(|| anyhow!("unknown_session"))?;
    ensure!(!session.principal, "principal_not_deletable");
    ensure!(session.revision == request.expected_revision, "session_revision_conflict");
    ensure!(!encerrar::pending_work_on(c, &request.bot_id, &request.session_id)?, "session_busy");
    let revision = next_rev(c)?;
    // O checkpoint já finalizado ainda pode carregar texto do turno. Só apagar o desta sessão.
    c.execute(
        "DELETE FROM kv WHERE k=?1 AND CASE WHEN json_valid(v) THEN
         json_extract(v,'$.phase')='finished' AND json_extract(v,'$.input.visibleSession')=?2 ELSE 0 END",
        params![format!("turn.waiting.{}", request.bot_id), request.session_id],
    )?;
    // Registros de escolha também guardam o texto da mensagem; não depender de foreign_keys=ON.
    c.execute(
        "DELETE FROM donna_genui_questions WHERE chat=?1 AND session=?2",
        params![request.bot_id, request.session_id],
    )?;
    c.execute("DELETE FROM entries WHERE bot_id=?1 AND session=?2", params![request.bot_id, request.session_id])?;
    c.execute(
        "UPDATE donna_session_bindings SET revoked_at=COALESCE(revoked_at,?3) WHERE chat=?1 AND session=?2",
        params![request.bot_id, request.session_id, now_ms()],
    )?;
    c.execute(
        "DELETE FROM donna_session_gateway_pending WHERE binding IN
        (SELECT id FROM donna_session_bindings WHERE chat=?1 AND session=?2)",
        params![request.bot_id, request.session_id],
    )?;
    let lane = crate::store::Lane { chat: request.bot_id.clone(), session: Some(request.session_id.clone()) };
    c.execute("DELETE FROM kv WHERE k=?1", [crate::chat::handoff::key_for(&lane)])?;
    c.execute(
        "DELETE FROM kv WHERE k=?1",
        [crate::store::run_session_key(&request.bot_id, &format!("session:{}", request.session_id))],
    )?;
    store::remove_extra_on(c, &request.bot_id, &request.session_id, session.revision)?;
    c.execute(
        "INSERT INTO donna_deleted_sessions(id,chat,space,original_revision,revision) VALUES(?1,?2,?3,?4,?5)",
        params![request.session_id, request.bot_id, request.space_id, session.revision, revision],
    )?;
    c.execute("UPDATE bots SET rev=?2 WHERE id=?1", params![request.bot_id, revision])?;
    Ok(event(request, revision))
}

pub(crate) fn catch_up(c: &Connection, space: &str, _since: i64) -> Result<Vec<Value>> {
    Ok(c.prepare("SELECT id,chat,revision FROM donna_deleted_sessions WHERE space=?1 ORDER BY revision")?
        .query_map(params![space], |r| {
            Ok(json!({"type":"sessionDeleted","spaceId":space,
            "sessionId":r.get::<_,String>(0)?,"chatId":r.get::<_,String>(1)?,"rev":r.get::<_,i64>(2)?}))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}
