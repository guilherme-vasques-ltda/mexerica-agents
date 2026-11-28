use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

#[cfg(target_os = "macos")]
const EPOCH: &str = "macLocalScreen.epoch";

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Consent {
    bot: Uuid,
    global: Option<Uuid>,
}

#[cfg(target_os = "macos")]
impl Consent {
    pub(super) fn read(db: &Connection, bot: &str) -> Result<Option<Self>> {
        let global = db
            .query_row("SELECT v FROM kv WHERE k=?1", [super::KV_ENABLED], |row| row.get::<_, String>(0))
            .optional()?;
        if !matches!(global.as_deref(), None | Some("1")) {
            return Ok(None);
        }
        let epoch: Option<String> =
            db.query_row("SELECT v FROM kv WHERE k=?1", [EPOCH], |row| row.get(0)).optional()?;
        let global = epoch.map(|value| Uuid::parse_str(&value)).transpose()?;
        Ok(consent_on(db, bot)?.map(|bot| Self { bot, global }))
    }
}

#[cfg(target_os = "macos")]
pub(super) fn set_global_on(db: &Connection, enabled: bool) -> Result<()> {
    db.execute(
        "INSERT INTO kv(k,v) VALUES(?1,?2),(?3,?4) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
        params![super::KV_ENABLED, if enabled { "1" } else { "0" }, EPOCH, Uuid::new_v4().to_string()],
    )?;
    Ok(())
}

fn key(bot: &str) -> String {
    format!("macLocalScreen.consent.{bot}")
}

pub(crate) fn consent_on(db: &Connection, bot: &str) -> Result<Option<Uuid>> {
    let value: Option<String> = db.query_row("SELECT v FROM kv WHERE k=?1", [key(bot)], |r| r.get(0)).optional()?;
    value.map(|value| Uuid::parse_str(&value).map_err(Into::into)).transpose()
}

pub(crate) fn set_on(db: &Connection, bot: &str, enabled: bool) -> Result<()> {
    if enabled {
        db.execute(
            "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
            params![key(bot), Uuid::new_v4().to_string()],
        )?;
    } else {
        db.execute("DELETE FROM kv WHERE k=?1", [key(bot)])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consent_is_per_bot_and_reenable_cannot_reuse_old_consent() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE kv(k TEXT PRIMARY KEY,v TEXT);
            CREATE TABLE bots(config TEXT);
            INSERT INTO bots VALUES('{\"computer\":true}');",
        )
        .unwrap();
        assert_eq!(consent_on(&db, "a").unwrap(), None);
        {
            let tx = db.transaction().unwrap();
            set_on(&tx, "a", true).unwrap();
            assert!(consent_on(&tx, "a").unwrap().is_some());
        }
        assert_eq!(consent_on(&db, "a").unwrap(), None);
        set_on(&db, "a", true).unwrap();
        let first = consent_on(&db, "a").unwrap().unwrap();
        assert_eq!(consent_on(&db, "b").unwrap(), None);
        set_on(&db, "a", false).unwrap();
        assert_eq!(consent_on(&db, "a").unwrap(), None);
        set_on(&db, "a", true).unwrap();
        assert_ne!(consent_on(&db, "a").unwrap().unwrap(), first);
        db.execute("UPDATE kv SET v='true'", []).unwrap();
        assert!(consent_on(&db, "a").is_err());
    }
}
