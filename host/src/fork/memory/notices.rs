//! A bot's memory writes. Each leaves a line in the bot's chat, "Lembrei: …" with an undo (Donnabot D60), and the
//! user reviews and changes memory in the bot's memory settings too. Cards that older hosts put in the chat (a card
//! to approve) still answer for the apps that show them. The line's `memory` field says what happened.

use crate::LockExt;
use crate::fork::autoridade::Autoridade;
use crate::fork::memory_auto::worker::{MemorySection, Operation};
use crate::hub::{Hub, Runtime};
use crate::store::BotConfig;
use anyhow::{Result, anyhow, bail};
use codync_mem::store::{self as mem, Error, Writer};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum Action {
    Remember {
        text: String,
        #[serde(default)]
        shared: bool,
    },
    Revise {
        entry: i64,
        text: String,
    },
    Archive {
        entry: i64,
    },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    /// Written; the user can undo it.
    Done,
    /// Waiting for the user's approval.
    Pending,
    Accepted,
    /// O fato já existia quando o dono respondeu; este cartão não criou uma entrada.
    AlreadyExists,
    Declined,
    Undone,
    /// Approved, but the write was refused (the layer is full, say).
    Failed,
    /// The entry changed after the line was posted, so the line can't undo it (Mexerica).
    Changed,
}

/// Decide e escreve sob a mesma cerca usada para marcar exposição.
pub fn apply(
    hub: &Hub,
    cfg: &BotConfig,
    turn: &Runtime,
    action: &Action,
    context: Option<&crate::fork::spaces::tool_context::ToolContext>,
) -> Result<Value> {
    let current = hub.runtime(&cfg.id);
    if current.memory_turn_token != turn.memory_turn_token
        || current.message != turn.message
        || current.session != turn.session
        || current.lane != turn.lane
        || current.started_at != turn.started_at
    {
        bail!("O turno mudou antes da proposta.");
    }
    let message = turn
        .message
        .clone()
        .ok_or_else(|| anyhow!("Memory proposals require a turn the user started in your own chat."))?;
    let evidence = vec![message];
    let op = match action {
        Action::Remember { text, shared } => {
            Operation::Remember { text: text.clone(), shared: *shared, evidence, nature: None }
        }
        Action::Revise { entry, text } => {
            let old = hub.store.memory_read(|c| mem::get(c, *entry))?.ok_or(Error::NotFound)?;
            Operation::Revise { entry: *entry, expected_rev: old.rev, text: text.clone(), evidence, nature: None }
        }
        Action::Archive { entry } => {
            let old = hub.store.memory_read(|c| mem::get(c, *entry))?.ok_or(Error::NotFound)?;
            Operation::Archive { entry: *entry, expected_rev: old.rev, evidence, nature: None }
        }
    };
    crate::fork::memory_auto::apply::stage_in(hub, cfg, turn, &op, context)
}

/// Clientes antigos não podem transformar cartões legados em sucesso presumido.
pub fn answer(_hub: &Hub, _notice_id: &str, _accept: bool) -> Result<Value> {
    bail!("Confirmações foram substituídas pela memória automática. O pedido continua no histórico.")
}
pub fn answer_confirmed(_hub: &Hub, _notice_id: &str, _chat: &str, _revision: i64, _accept: bool) -> Result<Value> {
    bail!("Confirmações foram substituídas pela memória automática. O pedido continua no histórico.")
}

/// The user took back what a bot wrote, if the entry is still as the line left it.
#[cfg(test)]
pub fn undo(hub: &Hub, notice_id: &str) -> Result<Value> {
    if notice_id.starts_with("memory-auto-") {
        return crate::fork::memory_auto::apply::undo(hub, notice_id, &Autoridade::local());
    }
    undo_authorized(hub, &Autoridade::local(), notice_id)
}

pub(crate) fn undo_authorized(hub: &Hub, authority: &Autoridade, notice_id: &str) -> Result<Value> {
    let (notice, cfg, action, mut card) = open(hub, notice_id)?;
    if matches!(card.status, Status::Undone | Status::Changed) {
        authority.with_db(hub, None, |_| Ok(()))?;
        return Ok(json!({"entry":notice}));
    }
    let section = MemorySection::enter(hub);
    let emission = hub.emit_lock.locked();
    let (entry, layer) = crate::chat::memory::authorized_write(&section, authority, |db, now, next| {
        let current = crate::store::Store::entry_on(db, notice_id)?.ok_or_else(|| anyhow!("unknown card"))?;
        anyhow::ensure!(current.rev == notice.rev, "memory_card_changed");
        let id = card
            .entry
            .filter(|_| matches!(card.status, Status::Done | Status::Accepted))
            .ok_or_else(|| anyhow!("There's nothing to undo."))?;
        let rev =
            card.rev.ok_or_else(|| anyhow!("This line is too old to undo; change it in the bot's memory settings."))?;
        let layer = if let Some(entry) = mem::get(db, id)?.filter(|e| e.rev == rev) {
            match &action {
                Action::Remember { .. } => {
                    mem::block_fact(db, entry.bot.as_deref(), &entry.text, &entry.evidence)?;
                    mem::withdraw(db, id)?;
                }
                Action::Revise { .. } => {
                    mem::revise(db, Writer::User, id, card.previous.as_deref().unwrap_or(&entry.text), now, next)?;
                }
                Action::Archive { .. } => {
                    mem::bring_back(db, id, now, next)?;
                }
            }
            card.status = Status::Undone;
            Some(entry.bot)
        } else {
            card.status = Status::Changed;
            None
        };
        let mut updated = notice.data.clone();
        let fresh = data(&cfg, &action, &card);
        updated["text"] = fresh["text"].clone();
        updated["memory"] = fresh["memory"].clone();
        db.execute(
            "UPDATE entries SET data=?2,rev=?3,updated_at=?4 WHERE id=?1",
            rusqlite::params![notice_id, updated.to_string(), next, now],
        )?;
        Ok((crate::store::Store::entry_on(db, notice_id)?.ok_or_else(|| anyhow!("unknown card"))?, layer))
    })?;
    let _ = hub.events.send(json!({"type":"entry","rev":entry.rev,"entry":entry}));
    drop(emission);
    drop(section);
    if let Some(layer) = layer {
        crate::chat::memory::changed(hub, layer.as_deref());
    }
    Ok(json!({"entry":entry}))
}

/// What a card records beside its action.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Card {
    status: Status,
    #[serde(default)]
    confirmation: bool,
    /// The memory entry written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    entry: Option<i64>,
    /// The entry's `rev` right after the write: undo acts only while it still is (Mexerica).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rev: Option<i64>,
    /// The text before a revision, or of the entry being archived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous: Option<String>,
    /// The user's message the turn answered: where the fact was learned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn data(cfg: &BotConfig, action: &Action, card: &Card) -> Value {
    let mut memory = serde_json::to_value(action).expect("an action is plain data");
    if let (Some(memory), Value::Object(card)) =
        (memory.as_object_mut(), serde_json::to_value(card).expect("a card is plain data"))
    {
        memory.extend(card);
    }
    json!({"text": summary(action, card), "style": "info", "author": cfg.id, "memory": memory})
}

/// What an app that doesn't know memory cards shows.
fn summary(action: &Action, card: &Card) -> String {
    let previous = card.previous.as_deref().unwrap_or_default();
    let what = match action {
        Action::Remember { text, .. } | Action::Revise { text, .. } => text.as_str(),
        Action::Archive { .. } => previous,
    };
    match (card.status, action) {
        (Status::Pending, Action::Remember { .. }) => format!("Salvar na memória? {what}"),
        (Status::Pending, Action::Revise { .. }) => format!("Quer atualizar a memória: {previous} → {what}"),
        (Status::Pending, Action::Archive { .. }) => format!("Quer arquivar da memória: {what}"),
        (Status::Done | Status::Accepted, Action::Remember { .. }) => format!("Salvo na memória: {what}"),
        (Status::Done | Status::Accepted, Action::Revise { .. }) => format!("Atualizei a memória: {what}"),
        (Status::Done | Status::Accepted, Action::Archive { .. }) => format!("Arquivei da memória: {what}"),
        (Status::AlreadyExists, _) => format!("Já existe na memória: {what} (confira também os itens arquivados)"),
        (Status::Declined, _) => format!("Recusado: {what}"),
        (Status::Undone, _) => format!("Desfeito: {what}"),
        (Status::Changed, _) => {
            format!(
                "{} (mudou depois; veja na memória do bot)",
                summary(action, &Card { status: Status::Done, ..card.clone() })
            )
        }
        (Status::Failed, _) => match &card.error {
            Some(error) => format!("Não consegui guardar na memória: {what}. {error}"),
            None => format!("Não consegui guardar na memória: {what}"),
        },
    }
}

fn open(hub: &Hub, notice_id: &str) -> Result<(crate::store::Entry, BotConfig, Action, Card)> {
    let notice = hub.store.entry(notice_id).ok_or_else(|| anyhow!("unknown card"))?;
    let memory = &notice.data["memory"];
    let action: Action = serde_json::from_value(memory.clone()).map_err(|_| anyhow!("not a memory card"))?;
    let card: Card = serde_json::from_value(memory.clone())?;
    let bot = notice.data["author"].as_str().unwrap_or(&notice.bot_id);
    let cfg = hub.store.bot(bot)?.filter(|b| !b.deleted).ok_or_else(|| anyhow!("unknown bot"))?.config;
    let cfg = crate::fork::spaces::tool_context::card_config(hub, &notice, cfg)?;
    Ok((notice, cfg, action, card))
}
