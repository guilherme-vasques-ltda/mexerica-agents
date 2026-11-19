//! Adapta o snapshot canônico da Parte A ao claim VG, na Connection do permit.
//! Não cria job, autoridade, Scope ou prova humana; só cerca a revisão que o claim muda.
use super::queue::Context;
use crate::store::{BotConfig, Entry};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};

pub(crate) fn before_claim(
    c: &Connection,
    cfg: &BotConfig,
    token: &str,
    chat: &str,
    ids: &[String],
) -> Result<Option<(String, Context)>> {
    let row: Option<(String, String)> = c
        .query_row(
            "SELECT id,context FROM donna_memory_auto_jobs WHERE bot=?1
        AND json_extract(context,'$.token')=?2 AND outcome='running'",
            params![cfg.id, token],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((id, raw)) = row else { return Ok(None) };
    let context: Context = serde_json::from_str(&raw)?;
    ensure!(
        context.chat_id == chat
            && context.space_id == cfg.space_id
            && context.memory_id == cfg.memory_id()
            && context.messages.iter().map(|m| &m.id).eq(ids.iter()),
        "memory_request_conflict"
    );
    super::worker::validate_context(c, &context)?;
    for reference in &context.messages {
        let unchanged: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM entries WHERE id=?1 AND bot_id=?2 AND rev=?3)",
            params![reference.id, context.chat_id, reference.revision],
            |row| row.get(0),
        )?;
        ensure!(unchanged, "memory_evidence_changed_before_claim");
    }
    Ok(Some((id, context)))
}

pub(crate) fn after_claim(c: &Connection, snapshot: Option<(String, Context)>, entries: &[Entry]) -> Result<()> {
    let Some((id, mut context)) = snapshot else { return Ok(()) };
    for reference in &mut context.messages {
        let entry = entries
            .iter()
            .find(|entry| entry.id == reference.id)
            .ok_or_else(|| anyhow::anyhow!("memory_claim_missing"))?;
        ensure!(
            entry.bot_id == context.chat_id
                && entry.is_user_authored()
                && entry.data["sourceBotId"].is_null()
                && entry.data["runId"].is_null(),
            "memory_owner_evidence_required"
        );
        reference.revision = entry.rev;
    }
    let changed = c.execute(
        "UPDATE donna_memory_auto_jobs SET context=?2 WHERE id=?1 AND outcome='running'",
        params![id, serde_json::to_string(&context)?],
    )?;
    ensure!(changed == 1, "memory_request_conflict");
    Ok(())
}
