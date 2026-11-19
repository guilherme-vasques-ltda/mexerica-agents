//! D70 compartilhado pelos consumidores pessoal e contextual; sem motor, sleep ou compensação.
use crate::LockExt;
use crate::hub::Hub;
use anyhow::Result;
use rusqlite::OptionalExtension;

pub(crate) fn snapshot(hub: &Hub, target: &str) -> Result<(i64, Option<String>, (usize, usize))> {
    let db = hub.store.db.locked();
    let notices =
        db.query_row("SELECT COUNT(*) FROM entries WHERE json_extract(data,'$.delegationId') IS NOT NULL", [], |r| {
            r.get(0)
        })?;
    let turn = db.query_row("SELECT v FROM kv WHERE k=?1", [format!("turn.seq.{target}")], |r| r.get(0)).optional()?;
    drop(db);
    Ok((notices, turn, hub.team.test_pending_counts()))
}

pub(crate) fn fail_recipient(hub: &Hub, target: &str) -> Result<()> {
    hub.store.kv_set("test.team.fail-recipient", target)?;
    hub.store.db.locked().execute_batch(
        "CREATE TRIGGER team_second_insert BEFORE INSERT ON entries
             WHEN NEW.bot_id=(SELECT v FROM kv WHERE k='test.team.fail-recipient')
             AND json_extract(NEW.data,'$.delegationId') IS NOT NULL
             BEGIN SELECT RAISE(ABORT,'team second insert'); END;",
    )?;
    Ok(())
}

pub(crate) fn no_notices(events: &mut tokio::sync::broadcast::Receiver<serde_json::Value>) {
    while let Ok(event) = events.try_recv() {
        assert!(event["type"] != "entry" || event["entry"]["data"]["delegationId"].is_null(), "{event}");
    }
}
