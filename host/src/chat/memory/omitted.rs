//! Why each memory was left out of the bot's opening (D263).
//!
//! `opening::load_ranked` reports every active entry the caps cut, with a closed reason. `sync` mirrors
//! that set into `donna_memory_opening_omitted`, scoped to the viewing bot, the Space and its memory layer;
//! `list` reads it back for the memory view, adding entries the layer rules keep out of this bot's opening
//! ("scope"). Rows carry ids and closed labels only — never entry text, and never an id the caller could
//! not already see.

use crate::chat::memory::shared;
use crate::fork::spaces::PERSONAL;
use crate::store::{BotConfig, Store};
use anyhow::Result;
use codync_mem::opening::{Omitted, OmittedLayer, OmittedReason};
use rusqlite::params;
use serde::Serialize;
use std::collections::BTreeSet;

/// One memory the opening left out, as the app sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Row {
    /// The entry id inside its own layer; pair with `layer` to find it in the view.
    pub entry: i64,
    /// `yours` or `bot`: the layer the entry lives in.
    pub layer: &'static str,
    /// `overBudget`, `pinnedLimit` or `scope`.
    pub reason: &'static str,
    /// Unix milliseconds of the last opening that recorded this reason; `0` for a scope row.
    pub at: i64,
}

fn layer_name(layer: OmittedLayer) -> &'static str {
    match layer {
        OmittedLayer::Yours => "yours",
        OmittedLayer::Bot => "bot",
    }
}

fn reason_name(reason: OmittedReason) -> &'static str {
    match reason {
        OmittedReason::OverBudget => "overBudget",
        OmittedReason::PinnedLimit => "pinnedLimit",
    }
}

fn layer_of(text: &str) -> Option<&'static str> {
    match text {
        "yours" => Some("yours"),
        "bot" => Some("bot"),
        _ => None,
    }
}

fn reason_of(text: &str) -> Option<&'static str> {
    match text {
        "overBudget" => Some("overBudget"),
        "pinnedLimit" => Some("pinnedLimit"),
        _ => None,
    }
}

pub(crate) fn migrate(c: &rusqlite::Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_memory_opening_omitted(
            bot TEXT NOT NULL,
            space TEXT NOT NULL,
            memory_id TEXT NOT NULL,
            audience TEXT NOT NULL CHECK(audience='owner'),
            entry INTEGER NOT NULL,
            layer TEXT NOT NULL CHECK(layer IN ('yours','bot')),
            reason TEXT NOT NULL CHECK(reason IN ('overBudget','pinnedLimit')),
            at INTEGER NOT NULL,
            PRIMARY KEY(bot, space, memory_id, audience, entry, layer)
        );",
    )?;
    Ok(())
}

/// Mirrors the omissions of the opening just rendered for `cfg`. Writes only when the set changed.
pub(crate) fn sync(store: &Store, cfg: &BotConfig, omitted: &[Omitted]) -> Result<()> {
    let (bot, space, memory) = (cfg.id.as_str(), cfg.space_id.as_str(), cfg.memory_id().to_owned());
    let desired: BTreeSet<(i64, &'static str, &'static str)> =
        omitted.iter().map(|o| (o.id, layer_name(o.layer), reason_name(o.reason))).collect();
    let current = store.memory_read(|c| {
        let mut stmt = c.prepare(
            "SELECT entry, layer, reason FROM donna_memory_opening_omitted
             WHERE bot = ?1 AND space = ?2 AND memory_id = ?3 AND audience = 'owner'",
        )?;
        let rows = stmt.query_map(params![bot, space, memory], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })?;
        let mut set = BTreeSet::new();
        for row in rows {
            let (entry, layer, reason) = row?;
            if let (Some(layer), Some(reason)) = (layer_of(&layer), reason_of(&reason)) {
                set.insert((entry, layer, reason));
            }
        }
        Ok(set)
    })?;
    if current == desired {
        return Ok(());
    }
    store.memory_write(|c, now, _rev| {
        c.execute(
            "DELETE FROM donna_memory_opening_omitted WHERE bot = ?1 AND space = ?2 AND memory_id = ?3 AND audience = 'owner'",
            params![bot, space, memory],
        )?;
        for o in omitted {
            c.execute(
                "INSERT INTO donna_memory_opening_omitted(bot, space, memory_id, audience, entry, layer, reason, at)
                 VALUES(?1, ?2, ?3, 'owner', ?4, ?5, ?6, ?7)",
                params![bot, space, memory, o.id, layer_name(o.layer), reason_name(o.reason), now],
            )?;
        }
        Ok(())
    })?;
    Ok(())
}

/// The most rows the memory view ever gets; the app treats a longer answer as malformed.
const MAX_ROWS: usize = 500;

/// The omissions the memory view shows for `cfg`, persisted ones plus the entries its layers keep out.
pub(crate) fn list(store: &Store, cfg: &BotConfig) -> Result<Vec<Row>> {
    if cfg.is_group() {
        return Ok(Vec::new());
    }
    let (bot, space, memory) = (cfg.id.clone(), cfg.space_id.clone(), cfg.memory_id().to_owned());
    let mut rows = store.memory_read(|c| {
        // The join proves the row still points at an active entry of the layer it claims; anything else is
        // stale or foreign and stays hidden.
        let mut stmt = c.prepare(
            "SELECT o.entry, o.layer, o.reason, o.at FROM donna_memory_opening_omitted o
             JOIN memory_entries e ON e.id = o.entry AND e.state = 'active'
               AND ((o.layer = 'yours' AND e.bot IS NULL) OR (o.layer = 'bot' AND e.bot = o.memory_id))
             WHERE o.bot = ?1 AND o.space = ?2 AND o.memory_id = ?3 AND o.audience = 'owner'
             ORDER BY o.entry, o.layer",
        )?;
        let mapped = stmt.query_map(params![bot, space, memory], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?))
        })?;
        let mut rows = Vec::new();
        for row in mapped {
            let (entry, layer, reason, at) = row?;
            if let (Some(layer), Some(reason)) = (layer_of(&layer), reason_of(&reason)) {
                rows.push(Row { entry, layer, reason, at });
            }
        }
        Ok(rows)
    })?;
    // A personal bot without shared memory never opens "yours"; its active entries are out by scope.
    if cfg.space_id == PERSONAL && !shared(cfg) {
        rows.retain(|row| row.layer != "yours");
        let yours = store.memory_read(|c| codync_mem::store::layer(c, None))?;
        rows.extend(yours.into_iter().filter(|e| e.state == codync_mem::store::State::Active).map(|e| Row {
            entry: e.id,
            layer: "yours",
            reason: "scope",
            at: 0,
        }));
    }
    rows.truncate(MAX_ROWS);
    Ok(rows)
}

/// Drops every omission recorded for or against `id`, inside the bot's deletion transaction.
pub(crate) fn forget_bot(c: &rusqlite::Connection, id: &str) -> Result<()> {
    c.execute("DELETE FROM donna_memory_opening_omitted WHERE bot = ?1 OR memory_id = ?1", [id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LockExt;
    use crate::store::Store;

    fn fixture() -> (Store, BotConfig, std::path::PathBuf) {
        let dir = crate::store::private_fixture_dir("codync-opening-omitted");
        let store = Store::open(&dir.join("memory.db")).unwrap();
        let cfg: BotConfig = serde_json::from_value(serde_json::json!({
            "id": "bot-omitted", "name": "Fixer", "backend": "claude", "cwd": "/tmp",
            "sharedMemory": true,
        }))
        .unwrap();
        (store, cfg, dir)
    }

    fn remember(store: &Store, cfg: &BotConfig, text: &str) {
        store
            .memory_write(|c, now, rev| {
                codync_mem::store::add(
                    c,
                    codync_mem::store::Writer::User,
                    codync_mem::store::New { bot: Some(cfg.memory_id()), text, room: None, evidence: &[] },
                    now,
                    rev,
                )
            })
            .unwrap();
    }

    fn overflow(store: &Store, cfg: &BotConfig) {
        // BOT_CAP is 1,200 estimated tokens; ~11 per entry means the last ones fall out.
        for i in 0..130 {
            remember(store, cfg, &format!("unrelated note number {i} about the weather"));
        }
    }

    #[test]
    fn a_cut_opening_records_each_omission_with_its_closed_reason() {
        let (store, cfg, dir) = fixture();
        overflow(&store, &cfg);
        super::super::opening_frozen(&store, &cfg, Some("when do supplier invoices arrive?"));
        let rows = list(&store, &cfg).unwrap();
        assert_ne!(rows.len(), 0);
        assert!(rows.iter().all(|row| row.layer == "bot" && row.reason == "overBudget" && row.at > 0));
        // The join already proves each row names an active entry of this bot's own layer.
        assert!(rows.iter().all(|row| row.entry > 0));
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_new_request_rescores_and_the_record_follows_the_last_opening() {
        let (store, cfg, dir) = fixture();
        // An old fact only a matching request saves; the filler pushes everything else out.
        remember(&store, &cfg, "supplier invoices always arrive on the 10th");
        overflow(&store, &cfg);
        let fact_id = store
            .memory_read(|c| codync_mem::store::layer(c, Some(cfg.memory_id())))
            .unwrap()
            .into_iter()
            .find(|e| e.text.contains("supplier invoices"))
            .unwrap()
            .id;
        let first = super::super::opening_frozen(&store, &cfg, Some("supplier invoices"));
        assert!(first.contains("more did not fit"), "{first}");
        let before = list(&store, &cfg).unwrap();
        assert!(before.iter().all(|row| row.entry != fact_id), "sanity: the fact is not omitted");
        // The next owner opening asks something else: the record follows the last opening, not the first.
        let second = super::super::opening_frozen(&store, &cfg, Some("qual a senha do wifi?"));
        assert_ne!(first, second, "a different request can pick a different set");
        let after = list(&store, &cfg).unwrap();
        assert!(after.iter().any(|row| row.entry == fact_id), "the unmatched request lets the fact fall out");
        assert_ne!(before, after, "and the record follows it");
        // Rendering the same request again is stable: same opening, same record, no rewrite.
        let third = super::super::opening_frozen(&store, &cfg, Some("qual a senha do wifi?"));
        assert_eq!(second, third);
        assert_eq!(after, list(&store, &cfg).unwrap());
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_change_notice_never_records_omissions() {
        let (store, cfg, dir) = fixture();
        overflow(&store, &cfg);
        // A pending memory change makes `change_notice` render the whole opening — with a query —
        // but the notice is not the session's opening, so nothing may be recorded.
        store.kv_set(&format!("memory.revision.{}", cfg.memory_id()), "1").unwrap();
        let notice = super::super::change_notice(&store, &cfg, "s1", Some("supplier invoices"));
        assert!(notice.is_some_and(|(text, _)| text.contains("memory_update")), "the notice renders");
        assert_eq!(list(&store, &cfg).unwrap().len(), 0, "and records nothing");
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_non_owner_opening_never_overwrites_the_record() {
        let (store, cfg, dir) = fixture();
        overflow(&store, &cfg);
        super::super::opening_frozen(&store, &cfg, Some("supplier invoices"));
        let owner_rows = list(&store, &cfg).unwrap();
        assert_ne!(owner_rows.len(), 0);
        // A routine's, a group's or a hidden turn's opening renders with no request (`query` is `None`
        // exactly then): it must leave the owner's record alone.
        super::super::opening_frozen(&store, &cfg, None);
        assert_eq!(list(&store, &cfg).unwrap(), owner_rows);
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn forget_bot_drops_rows_where_the_bot_is_viewer_or_owner() {
        let (store, cfg, dir) = fixture();
        overflow(&store, &cfg);
        super::super::opening_frozen(&store, &cfg, Some("supplier invoices"));
        assert_ne!(list(&store, &cfg).unwrap().len(), 0);
        // Rows naming this bot either as viewer or as memory owner go together.
        store
            .db
            .locked()
            .execute(
                "INSERT INTO donna_memory_opening_omitted(bot, space, memory_id, audience, entry, layer, reason, at)
                 VALUES('outro', ?1, ?2, 'owner', 1, 'bot', 'overBudget', 1)",
                rusqlite::params![cfg.space_id, cfg.id],
            )
            .unwrap();
        {
            let db = store.db.locked();
            forget_bot(&db, &cfg.id).unwrap();
        }
        assert_eq!(list(&store, &cfg).unwrap().len(), 0, "the viewer's rows are gone");
        let left: i64 =
            store.db.locked().query_row("SELECT COUNT(*) FROM donna_memory_opening_omitted", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0, "and so are the rows where it owned the memory");
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_opening_with_room_clears_the_record() {
        let (store, cfg, dir) = fixture();
        overflow(&store, &cfg);
        super::super::opening_frozen(&store, &cfg, Some("supplier invoices"));
        assert_ne!(list(&store, &cfg).unwrap().len(), 0);
        // Forgetting enough entries lets everything fit: nothing is left out anymore.
        store
            .memory_write(|c, _now, _rev| {
                for entry in codync_mem::store::layer(c, Some(cfg.memory_id()))? {
                    if entry.state == codync_mem::store::State::Active && entry.text.contains("number 1") {
                        codync_mem::store::forget(c, entry.id)?;
                    }
                }
                Ok(())
            })
            .unwrap();
        super::super::opening_frozen(&store, &cfg, Some("does everything fit now?"));
        assert_eq!(list(&store, &cfg).unwrap().len(), 0);
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_personal_bot_without_shared_memory_reports_yours_as_scope() {
        let (store, mut cfg, dir) = fixture();
        cfg.shared_memory = Some(false);
        // One shared entry the bot never opens, written into "yours" (bot = NULL).
        store
            .memory_write(|c, now, rev| {
                codync_mem::store::add(
                    c,
                    codync_mem::store::Writer::User,
                    codync_mem::store::New { bot: None, text: "the owner likes espresso", room: None, evidence: &[] },
                    now,
                    rev,
                )
            })
            .unwrap();
        let yours_id = store
            .memory_read(|c| codync_mem::store::layer(c, None))
            .unwrap()
            .into_iter()
            .find(|e| e.state == codync_mem::store::State::Active)
            .unwrap()
            .id;
        super::super::opening_frozen(&store, &cfg, None);
        let rows = list(&store, &cfg).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].entry, rows[0].layer, rows[0].reason, rows[0].at), (yours_id, "yours", "scope", 0));
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rows_for_foreign_or_stale_entries_stay_hidden() {
        let (store, cfg, dir) = fixture();
        overflow(&store, &cfg);
        super::super::opening_frozen(&store, &cfg, Some("anything"));
        // A row claiming an entry of another bot's layer must not surface, even hand-planted.
        let foreign = "bot-intruso";
        store
            .memory_write(|c, now, rev| {
                codync_mem::store::add(
                    c,
                    codync_mem::store::Writer::User,
                    codync_mem::store::New {
                        bot: Some(foreign),
                        text: "secret of another bot",
                        room: None,
                        evidence: &[],
                    },
                    now,
                    rev,
                )
            })
            .unwrap();
        let foreign_id =
            store.memory_read(|c| codync_mem::store::layer(c, Some(foreign))).unwrap().into_iter().next().unwrap().id;
        let db = store.db.locked();
        db.execute(
            "INSERT INTO donna_memory_opening_omitted(bot, space, memory_id, audience, entry, layer, reason, at)
             VALUES(?1, ?2, ?3, 'owner', ?4, 'bot', 'overBudget', 1)",
            rusqlite::params![cfg.id, cfg.space_id, cfg.memory_id(), foreign_id],
        )
        .unwrap();
        drop(db);
        let rows = list(&store, &cfg).unwrap();
        assert!(rows.iter().all(|row| row.entry != foreign_id), "a foreign entry id never leaks");
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_group_bot_has_no_opening_omissions() {
        let (store, mut cfg, dir) = fixture();
        cfg.kind = crate::store::BotKind::Group;
        assert_eq!(list(&store, &cfg).unwrap().len(), 0);
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_database_from_before_the_table_migrates_without_losing_memory() {
        let (store, cfg, dir) = fixture();
        remember(&store, &cfg, "supplier invoices always arrive on the 10th");
        let entries = store.memory_read(|c| codync_mem::store::layer(c, Some(cfg.memory_id()))).unwrap().len();
        drop(store);
        // A database written before this feature has every table but this one.
        rusqlite::Connection::open(dir.join("memory.db"))
            .unwrap()
            .execute_batch("DROP TABLE donna_memory_opening_omitted")
            .unwrap();
        let store = Store::open(&dir.join("memory.db")).unwrap();
        let kept = store.memory_read(|c| codync_mem::store::layer(c, Some(cfg.memory_id()))).unwrap();
        assert_eq!(kept.len(), entries, "the migration keeps the memory it found");
        let exists = store
            .memory_read(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'donna_memory_opening_omitted'",
                    [],
                    |r| r.get::<_, i64>(0),
                )?)
            })
            .unwrap();
        assert_eq!(exists, 1, "and adds the omissions table back");
        assert_eq!(list(&store, &cfg).unwrap().len(), 0);
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
