//! Só linhas ligadas + auditoria durável. Triggers preservam limpeza e geração na Tx original.
use anyhow::Result;
use rusqlite::Connection;

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_screen_access(
           bot_id TEXT NOT NULL, target TEXT NOT NULL, PRIMARY KEY(bot_id,target));
         CREATE TABLE IF NOT EXISTS donna_screen_access_audit(
           id INTEGER PRIMARY KEY, bot_id TEXT NOT NULL, target TEXT NOT NULL,
           enabled INTEGER NOT NULL, at INTEGER NOT NULL);
         CREATE TRIGGER IF NOT EXISTS donna_screen_access_on AFTER INSERT ON donna_screen_access
         BEGIN
           INSERT INTO donna_screen_access_audit(bot_id,target,enabled,at)
             VALUES(NEW.bot_id,NEW.target,1,CAST(unixepoch('subsec')*1000 AS INTEGER));
           UPDATE kv SET v=CAST(CAST(v AS INTEGER)+1 AS TEXT)
             WHERE k='donna.bot.config-generation.' || NEW.bot_id;
         END;
         CREATE TRIGGER IF NOT EXISTS donna_screen_access_off AFTER DELETE ON donna_screen_access
         BEGIN
           INSERT INTO donna_screen_access_audit(bot_id,target,enabled,at)
             VALUES(OLD.bot_id,OLD.target,0,CAST(unixepoch('subsec')*1000 AS INTEGER));
           UPDATE kv SET v=CAST(CAST(v AS INTEGER)+1 AS TEXT)
             WHERE k='donna.bot.config-generation.' || OLD.bot_id;
         END;
         CREATE TRIGGER IF NOT EXISTS donna_screen_access_bot_deleted AFTER UPDATE OF deleted ON bots
         WHEN NEW.deleted<>0
         BEGIN DELETE FROM donna_screen_access WHERE bot_id=NEW.id; END;
         CREATE TRIGGER IF NOT EXISTS donna_screen_access_bot_removed AFTER DELETE ON bots
         BEGIN DELETE FROM donna_screen_access WHERE bot_id=OLD.id; END;
         CREATE TRIGGER IF NOT EXISTS donna_screen_access_device_removed BEFORE DELETE ON devices
         BEGIN DELETE FROM donna_screen_access WHERE target IN
           (SELECT id FROM donna_computers WHERE device_key=OLD.key); END;
         CREATE TRIGGER IF NOT EXISTS donna_screen_access_computer_removed AFTER UPDATE OF removed_at ON donna_computers
         WHEN NEW.removed_at IS NOT NULL
         BEGIN DELETE FROM donna_screen_access WHERE target=NEW.id; END;",
    )?;
    for (name, kind) in [("choice", "TEXT"), ("card_id", "TEXT"), ("effect", "INTEGER"), ("execution", "TEXT")] {
        let exists: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('donna_screen_access_audit') WHERE name=?1)",
            [name],
            |r| r.get(0),
        )?;
        if !exists {
            db.execute(&format!("ALTER TABLE donna_screen_access_audit ADD COLUMN {name} {kind}"), [])?;
        }
    }
    Ok(())
}

/// Repareamento do mesmo ID nunca herda autorização. Chamada na Tx de `enroll_computer`.
pub(crate) fn clear_target_on(db: &Connection, target: &str) -> Result<()> {
    db.execute("DELETE FROM donna_screen_access WHERE target=?1", [target])?;
    Ok(())
}
