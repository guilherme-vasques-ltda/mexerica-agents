// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! A bot's changes to its memory files don't show in the chat: the memory settings keep every version. Lines and
//! cards that older hosts put in the chat ("Updated people.md" with an undo, a change to approve) still answer and
//! undo for the apps that show them; their `memoryFile` field holds the change.

use crate::LockExt;
use crate::fork::autoridade::Autoridade;
use crate::fork::memory_auto::worker::MemorySection;
use crate::hub::Hub;
use crate::store::BotConfig;
use anyhow::{Result, anyhow, bail};
use codync_mem::files::{self as mf, Author, Cause};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum Status {
    Done,
    Pending,
    Accepted,
    Declined,
    Undone,
    Restored,
    Failed,
}

/// What the `memoryFile` field of the chat line records.
// Each flag is one fact the apps show about the change, independent of the others.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Card {
    file: String,
    shared: bool,
    status: Status,
    /// `- ` removed and `+ ` added lines.
    diff: String,
    #[serde(default)]
    created: bool,
    #[serde(default)]
    deleted: bool,
    /// The change removes more than a quarter of the file.
    #[serde(default)]
    big: bool,
    /// The version written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    version: Option<i64>,
    /// The version before, which undo brings back; `None` with a version: undo deletes the new file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous: Option<i64>,
    /// Pending: the content it would write (`None` with `deleted`), over the content with this hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    base: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

/// Whether a chat line is a memory file's.
pub fn is_file_card(data: &Value) -> bool {
    data["memoryFile"].is_object()
}

/// The user approved or declined a bot's change.
pub fn answer(_hub: &Hub, _notice_id: &str, _accept: bool) -> Result<Value> {
    bail!("Confirmações foram substituídas pela memória automática. O pedido continua no histórico.")
}

/// The user took back a bot's change: the version before it comes back (or a new file goes).
pub fn undo(hub: &Hub, authority: &Autoridade, notice_id: &str) -> Result<Value> {
    let (notice, cfg, mut card) = open(hub, notice_id)?;
    let layer = layer(&cfg, &card)?;
    let section = MemorySection::enter(hub);
    let emission = hub.emit_lock.locked();
    let entry = super::api::write(&section, authority, |db, now, rev| {
        let current = crate::store::Store::entry_on(db, notice_id)?.ok_or_else(|| anyhow!("unknown card"))?;
        anyhow::ensure!(current.rev == notice.rev, "memory_card_changed");
        let version = card
            .version
            .filter(|_| matches!(card.status, Status::Done | Status::Accepted))
            .ok_or_else(|| anyhow!("There's nothing to undo."))?;
        let wrote = mf::version(db, version)?.ok_or_else(|| anyhow!("That version is gone."))?;
        let source = card.previous.map(|id| mf::version(db, id)).transpose()?.flatten();
        anyhow::ensure!(card.previous.is_none() || source.is_some(), "That version is gone.");
        let expected = wrote.content.as_deref().map(mf::hash);
        let content = source.as_ref().and_then(|v| v.content.as_deref());
        super::file_api::record(
            db,
            &mf::Change {
                bot: layer.as_deref(),
                name: &card.file,
                content,
                author: Author::User,
                author_bot: None,
                cause: Cause::Undo,
                mtime: now,
                size: content.map_or(0, |s| s.len() as u64),
            },
            expected.as_deref(),
            None,
            now,
            rev,
        )?;
        card.status = Status::Undone;
        let mut updated = notice.data.clone();
        let fresh = data(&cfg, &card);
        updated["text"] = fresh["text"].clone();
        updated["memoryFile"] = fresh["memoryFile"].clone();
        db.execute(
            "UPDATE entries SET data=?2,rev=?3,updated_at=?4 WHERE id=?1",
            rusqlite::params![notice_id, updated.to_string(), rev, now],
        )?;
        crate::store::Store::entry_on(db, notice_id)?.ok_or_else(|| anyhow!("unknown card"))
    })?;
    let _ = hub.events.send(json!({"type":"entry","rev":entry.rev,"entry":entry}));
    drop(emission);
    drop(section);
    super::file_api::project(hub, layer.as_deref(), &card.file);
    super::changed(hub, layer.as_deref());
    Ok(json!({"entry":entry}))
}

fn layer(cfg: &BotConfig, card: &Card) -> Result<Option<String>> {
    if card.shared && !super::shared(cfg) {
        bail!("This bot no longer uses the shared memory.");
    }
    Ok((!card.shared).then(|| cfg.id.clone()))
}

fn open(hub: &Hub, notice_id: &str) -> Result<(crate::store::Entry, BotConfig, Card)> {
    let notice = hub.store.entry(notice_id).ok_or_else(|| anyhow!("unknown card"))?;
    let card: Card =
        serde_json::from_value(notice.data["memoryFile"].clone()).map_err(|_| anyhow!("not a memory file card"))?;
    let bot = notice.data["author"].as_str().unwrap_or(&notice.bot_id);
    let cfg = hub.store.bot(bot)?.filter(|b| !b.deleted).ok_or_else(|| anyhow!("unknown bot"))?.config;
    let cfg = crate::fork::spaces::tool_context::card_config(hub, &notice, cfg)?;
    Ok((notice, cfg, card))
}

fn data(cfg: &BotConfig, card: &Card) -> Value {
    let memory = serde_json::to_value(card).expect("a card is plain data");
    json!({"text": summary(card), "style": "info", "author": cfg.id, "memoryFile": memory})
}

/// What an app that doesn't know memory file cards shows.
fn summary(card: &Card) -> String {
    let file = &card.file;
    let verb = if card.deleted {
        "Deleted"
    } else if card.created {
        "Created"
    } else {
        "Updated"
    };
    match card.status {
        Status::Done | Status::Accepted => format!("{verb} {file}"),
        Status::Pending => format!("Wants to {} {file}", verb.to_lowercase().trim_end_matches('d')),
        Status::Declined => format!("Declined: {file}"),
        Status::Undone => format!("Undone: {file}"),
        Status::Restored => format!("Put back {file}: {}", card.reason.as_deref().unwrap_or_default()),
        Status::Failed => format!("Couldn't change {file}: {}", card.reason.as_deref().unwrap_or_default()),
    }
}
