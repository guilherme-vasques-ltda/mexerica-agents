//! Projeção efêmera dos checkpoints de permissão, sem autoridade de resposta ou retomada.
use super::model::{ConversationRef, Observation};
use anyhow::Result;
use rusqlite::{Connection, params};
use serde::Serialize;

const LIMIT: i64 = 20;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DelegatedPermissions {
    pub(super) items: Vec<Item>,
    omitted: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Item {
    key: String,
    pub(super) requester_bot_id: String,
    pub(super) target_bot_id: String,
    pub(super) entry_id: String,
    pub(super) conversation: ConversationRef,
    mode: String,
    pub(super) requested_at: i64,
}

/// Recebe a mesma transação e roster já autorizados da fotografia do Mission Board.
pub(super) fn project(c: &Connection, space: &str, roster: &[&str]) -> Result<Observation<DelegatedPermissions>> {
    let value = read(c, space, roster, LIMIT)?;
    Ok(Observation::Available {
        source: "retornos.delegatedPermissions",
        coverage: "pendingCardsInAuthorizedConversations",
        value,
    })
}

pub(super) fn read(c: &Connection, space: &str, roster: &[&str], limit: i64) -> Result<DelegatedPermissions> {
    let roster = serde_json::to_string(roster)?;
    let mut statement = c.prepare(
        "SELECT p.card_id,r.requester_bot_id,r.target_bot_id,c.id,c.revision,r.mode,p.created_at,
                COUNT(*) OVER() AS total
         FROM donna_delegated_permission_checkpoints p
         JOIN donna_session_requests r ON r.id=p.request_id
         JOIN entries e ON e.id=p.card_id
         JOIN donna_space_conversations c ON c.id=e.bot_id AND c.bot_id=r.target_bot_id
         JOIN donna_bot_sessions s ON s.id=COALESCE(e.session,e.bot_id) AND s.chat_id=c.id
         JOIN bots source ON source.id=r.requester_bot_id AND source.deleted=0
         JOIN bots target ON target.id=r.target_bot_id AND target.deleted=0
         WHERE p.space_id=?1 AND c.space_id=?1 AND c.closed=0 AND s.state='open' AND s.principal=1 AND s.id=c.id
           AND NOT EXISTS(SELECT 1 FROM donna_session_privacy privacy WHERE privacy.request_id=r.id)
           AND r.schema_version=1 AND r.kind='inter_bot' AND r.state='running' AND r.end_reason IS NULL
           AND NOT EXISTS(SELECT 1 FROM donna_session_results result WHERE result.request_id=r.id)
           AND r.requester_bot_id<>r.target_bot_id AND r.mode IN ('wait','async') AND r.attempt=1 AND p.attempt=r.attempt AND p.fence=r.fence
           AND r.target_chat_id=c.id AND r.target_session_id=s.id AND e.kind='permission'
           AND e.rev>=p.card_revision AND p.card_revision>0 AND p.created_at>=0
           AND CASE WHEN json_valid(e.data) THEN json_extract(e.data,'$.status')='pending'
                AND json_extract(e.data,'$.author')=r.target_bot_id ELSE 0 END
           AND CASE WHEN json_valid(source.config) THEN (CASE WHEN json_type(source.config,'$.spaceId') IS NULL THEN ?3 WHEN json_type(source.config,'$.spaceId')='text' THEN json_extract(source.config,'$.spaceId') END)=?1
                AND (CASE WHEN json_type(source.config,'$.kind') IS NULL THEN 'agent' WHEN json_type(source.config,'$.kind')='text' THEN json_extract(source.config,'$.kind') END)='agent' ELSE 0 END
           AND CASE WHEN json_valid(target.config) THEN (CASE WHEN json_type(target.config,'$.spaceId') IS NULL THEN ?3 WHEN json_type(target.config,'$.spaceId')='text' THEN json_extract(target.config,'$.spaceId') END)=?1
                AND (CASE WHEN json_type(target.config,'$.kind') IS NULL THEN 'agent' WHEN json_type(target.config,'$.kind')='text' THEN json_extract(target.config,'$.kind') END)='agent' ELSE 0 END
           AND EXISTS(SELECT 1 FROM json_each(?2) WHERE value=r.requester_bot_id)
           AND EXISTS(SELECT 1 FROM json_each(?2) WHERE value=r.target_bot_id)
         ORDER BY p.created_at DESC,p.card_id DESC LIMIT ?4",
    )?;
    let mut rows = statement.query(params![space, roster, crate::fork::spaces::PERSONAL, limit])?;
    let mut items = Vec::new();
    let mut total = 0_usize;
    while let Some(row) = rows.next()? {
        let entry_id: String = row.get(0)?;
        total = usize::try_from(row.get::<_, i64>(7)?)?;
        items.push(Item {
            key: entry_id.clone(),
            requester_bot_id: row.get(1)?,
            target_bot_id: row.get(2)?,
            entry_id,
            conversation: ConversationRef { conversation_id: row.get(3)?, revision: row.get(4)? },
            mode: row.get(5)?,
            requested_at: row.get(6)?,
        });
    }
    Ok(DelegatedPermissions { omitted: total.saturating_sub(items.len()), items })
}
