//! Retomada de setup na origem; fim de turno não cria nova concessão.
use super::tool_context::{self, ToolContext};
use crate::{
    hub::Hub,
    store::{BotConfig, Entry, run_session_key},
};
use anyhow::{Result, anyhow, ensure};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Origin {
    session: String,
    turn: i64,
    message: String,
}
fn key(id: &str) -> String {
    format!("donna.tools.connection-card.{id}")
}

pub(crate) fn record(hub: &Hub, context: &ToolContext, entry: &Entry) -> Result<()> {
    context.check(hub)?;
    {
        use crate::LockExt;
        tool_context::record_card_on(&hub.store.db.locked(), &context.cfg, entry)?;
    }
    let current = hub
        .store
        .entry(context.turn.message.as_deref().ok_or_else(|| anyhow!("human_turn_required"))?)
        .ok_or_else(|| anyhow!("message_not_found"))?;
    let origin = Origin {
        session: context.session().into(),
        turn: context.number(),
        message: source(hub, &context.cfg, &current)?.id,
    };
    hub.store.kv_set(&key(&entry.id), &serde_json::to_string(&origin)?)
}

pub(crate) fn check(hub: &Hub, entry: &Entry, turn: i64) -> Result<BotConfig> {
    let bot = entry.data["connectionRequest"]["botId"].as_str().ok_or_else(|| anyhow!("bot_id_required"))?;
    let original = hub.store.bot(bot)?.filter(|r| !r.deleted).ok_or_else(|| anyhow!("bot_not_found"))?.config;
    let cfg = tool_context::card_config(hub, entry, original)?;
    if cfg.context.is_some() {
        let origin: Origin = serde_json::from_str(
            &hub.store.kv_read(&key(&entry.id))?.ok_or_else(|| anyhow!("connection_card_binding_required"))?,
        )?;
        let human = hub.store.entry(&origin.message).ok_or_else(|| anyhow!("message_not_found"))?;
        ensure!(human.is_user_authored() && human.bot_id == entry.bot_id, "human_turn_required");
        super::validate_entries(hub, &cfg, &[origin.message])?;
        let slot = super::session_slot(&cfg).ok_or_else(|| anyhow!("conversation_context_required"))?;
        ensure!(
            hub.store.kv_read(&run_session_key(bot, &slot))?.as_deref() == Some(origin.session.as_str()),
            "connection_card_session_changed"
        );
        ensure!(entry.turn == origin.turn && hub.store.max_turn(&entry.bot_id) == turn, "connection_card_turn_changed");
        let connector =
            entry.data["connectionRequest"]["connectorId"].as_str().ok_or_else(|| anyhow!("connector_id_required"))?;
        ensure!(cfg.connectors.iter().any(|c| c == connector), "connector_not_enabled");
    }
    Ok(cfg)
}

pub(crate) fn check_resume(hub: &Hub, cfg: &BotConfig, entry: &Entry) -> Result<()> {
    if let Some(id) = entry.id.strip_prefix("connection-ack-") {
        ensure!(
            entry.kind == crate::store::EntryKind::User.as_str() && entry.data["from"] == "host",
            "connection_resume_mismatch"
        );
        let card = hub.store.entry(id).ok_or_else(|| anyhow!("card_not_found"))?;
        ensure!(card.data["connectionRequest"]["status"] == "ready", "connection_card_not_ready");
        let original = check(hub, &card, entry.turn)?;
        ensure!(original.id == cfg.id && original.session_owner() == cfg.session_owner(), "card_context_mismatch");
    }
    Ok(())
}

/// Só o ack emitido pelo Hub pode continuar a intenção humana preservada no cartão.
pub(crate) fn source(hub: &Hub, cfg: &BotConfig, entry: &Entry) -> Result<Entry> {
    if entry.is_user_authored() {
        return Ok(entry.clone());
    }
    check_resume(hub, cfg, entry)?;
    let id = entry.id.strip_prefix("connection-ack-").ok_or_else(|| anyhow!("human_turn_required"))?;
    let origin: Origin = serde_json::from_str(
        &hub.store.kv_read(&key(id))?.ok_or_else(|| anyhow!("connection_card_binding_required"))?,
    )?;
    let human = hub.store.entry(&origin.message).ok_or_else(|| anyhow!("message_not_found"))?;
    ensure!(human.is_user_authored() && human.bot_id == cfg.session_owner(), "human_turn_required");
    Ok(human)
}

/// Variante só-Connection para a cerca da Sessões3 (`ToolFence`): nunca volta ao Store.
/// Aceita a mensagem humana ou o ack de ID canônico cujo cartão pronto guarda essa mensagem.
pub(crate) fn human_on(db: &rusqlite::Connection, entry: &Entry) -> Result<()> {
    use rusqlite::OptionalExtension;
    if entry.is_user_authored() {
        return Ok(());
    }
    let id = entry.id.strip_prefix("connection-ack-").ok_or_else(|| anyhow!("human_turn_required"))?;
    ensure!(
        entry.kind == crate::store::EntryKind::User.as_str() && entry.data["from"] == "host",
        "connection_resume_mismatch"
    );
    let card = crate::store::entry_on(db, id)?.ok_or_else(|| anyhow!("card_not_found"))?;
    ensure!(
        card.bot_id == entry.bot_id && card.data["connectionRequest"]["status"] == "ready",
        "connection_card_not_ready"
    );
    let raw: String = db
        .query_row("SELECT v FROM kv WHERE k=?1", [key(id)], |r| r.get(0))
        .optional()?
        .ok_or_else(|| anyhow!("connection_card_binding_required"))?;
    let origin: Origin = serde_json::from_str(&raw)?;
    let human = crate::store::entry_on(db, &origin.message)?.ok_or_else(|| anyhow!("message_not_found"))?;
    ensure!(human.is_user_authored() && human.bot_id == entry.bot_id, "human_turn_required");
    Ok(())
}
