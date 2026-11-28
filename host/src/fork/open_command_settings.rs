use crate::{
    LockExt,
    api::devices::Caller,
    hub::Hub,
    store::{BotConfig, Store},
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) const FIELD: &str = "localCommandsEnabled";

fn supported(cfg: &BotConfig) -> bool {
    cfg!(any(target_os = "linux", target_os = "macos"))
        && crate::environment::Environment::is_open()
        && cfg.backend == "codex"
        && !cfg.hidden
        && !cfg.is_group()
}

pub(super) fn revision(db: &Connection, bot: &str) -> Result<Option<i64>> {
    if !Store::bot_on(db, bot)?.is_some_and(|row| !row.deleted && supported(&row.config)) {
        return Ok(None);
    }
    let value: Option<String> =
        db.query_row("SELECT v FROM kv WHERE k = ?1", [format!("local_commands.{bot}")], |r| r.get(0)).optional()?;
    Ok(value.and_then(|rev| rev.parse::<i64>().ok()).filter(|rev| *rev > 0))
}

pub(super) fn require(db: &Connection, bot: &str, expected: i64) -> Result<()> {
    ensure!(revision(db, bot)? == Some(expected), "open_tools_command_disabled");
    Ok(())
}

pub(super) fn on_bot_deleted(db: &Connection, bot: &str) -> Result<()> {
    db.execute("DELETE FROM kv WHERE k=?1", [format!("local_commands.{bot}")])?;
    Ok(())
}

pub(crate) fn project(hub: &Hub, cfg: &BotConfig) -> Option<bool> {
    supported(cfg).then(|| revision(&hub.store.db.locked(), &cfg.id).is_ok_and(|r| r.is_some()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Change {
    id: String,
    local_commands_enabled: bool,
}

pub(crate) fn update(hub: &Hub, caller: &Caller, body: Value) -> Result<Value> {
    ensure!(matches!(caller, Caller::Local), "open_tools_local_app_required");
    let request: Change = serde_json::from_value(body).context("open_tools_invalid_command_setting")?;
    let owner = crate::fork::autoridade::Autoridade::capturar(hub, caller)?;
    let _emit = hub.emit_lock.locked();
    owner.with_db(hub, None, |db| {
        ensure!(
            Store::bot_on(db, &request.id)?.is_some_and(|row| !row.deleted && supported(&row.config)),
            "open_tools_command_unavailable"
        );
        let current = revision(db, &request.id)?;
        if request.local_commands_enabled != current.is_some() {
            let rev = crate::store::next_rev(db)?;
            if request.local_commands_enabled {
                db.execute(
                    "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
                    params![format!("local_commands.{}", request.id), rev.to_string()],
                )?;
            } else {
                on_bot_deleted(db, &request.id)?;
            }
            db.execute("UPDATE bots SET rev=?2 WHERE id=?1", params![request.id, rev])?;
        }
        Ok(())
    })?;
    let row = hub.store.bot(&request.id)?.filter(|row| !row.deleted).context("open_tools_command_unavailable")?;
    let bot = hub.bot_json(&row);
    let _ = hub.events.send(json!({"type":"bot","rev":bot["rev"],"bot":bot}));
    Ok(json!({"bot":bot}))
}
