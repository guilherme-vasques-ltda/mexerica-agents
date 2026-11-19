//! Geração privada do perfil: mudanças A → B → A não restauram uma captura antiga.
use anyhow::{Result, anyhow};
use rusqlite::{Connection, OptionalExtension};

const PREFIX: &str = "donna.bot.config-generation.";

pub(super) fn migrate(db: &Connection) -> Result<()> {
    // Triggers cobrem também os escritores transacionais de team/Undo. A geração
    // muda junto do perfil, e rollback desfaz ambos; read marks não a renovam.
    db.execute_batch(
        "INSERT INTO kv(k,v) SELECT 'donna.bot.config-generation.' || id, CAST(rev AS TEXT) FROM bots
         WHERE 1 ON CONFLICT(k) DO NOTHING;
         CREATE TRIGGER IF NOT EXISTS donna_bot_config_generation_insert AFTER INSERT ON bots
         BEGIN
           INSERT INTO kv(k,v) VALUES('donna.bot.config-generation.' || NEW.id, '1')
           ON CONFLICT(k) DO UPDATE SET v=CAST(CAST(v AS INTEGER)+1 AS TEXT);
         END;
         CREATE TRIGGER IF NOT EXISTS donna_bot_config_generation_update AFTER UPDATE OF config,deleted ON bots
         WHEN OLD.config IS NOT NEW.config OR OLD.deleted IS NOT NEW.deleted
         BEGIN
           INSERT INTO kv(k,v) VALUES('donna.bot.config-generation.' || NEW.id, '1')
           ON CONFLICT(k) DO UPDATE SET v=CAST(CAST(v AS INTEGER)+1 AS TEXT);
         END;",
    )?;
    Ok(())
}

pub(crate) fn current_on(db: &Connection, bot: &str) -> Result<i64> {
    let value: String = db
        .query_row("SELECT v FROM kv WHERE k=?1", [format!("{PREFIX}{bot}")], |r| r.get(0))
        .optional()?
        .ok_or_else(|| anyhow!("config_generation_required"))?;
    Ok(value.parse()?)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn generation(db: &Connection) -> i64 {
        let value: String =
            db.query_row("SELECT v FROM kv WHERE k=?1", [format!("{PREFIX}bot")], |r| r.get(0)).unwrap();
        value.parse().unwrap()
    }

    #[test]
    fn geracao_cobre_sql_direto_aba_rollback_e_reabertura_sem_confundir_read_marks() {
        let path = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("config-generation-{}.db", uuid::Uuid::new_v4()));
        let mut db = Connection::open(&path).unwrap();
        db.execute_batch(
            r#"CREATE TABLE kv(k TEXT PRIMARY KEY,v TEXT NOT NULL);
             CREATE TABLE bots(id TEXT PRIMARY KEY,rev INTEGER,config TEXT,deleted INTEGER DEFAULT 0);
             INSERT INTO bots(id,rev,config) VALUES('bot',42,'{"computer":false}');"#,
        )
        .unwrap();
        migrate(&db).unwrap();
        assert_eq!(generation(&db), 42, "migração sem promover configurações antigas");
        db.execute("UPDATE bots SET config=?1 WHERE id='bot'", [r#"{"computer":true}"#]).unwrap();
        db.execute("UPDATE bots SET config=?1 WHERE id='bot'", [r#"{"computer":false}"#]).unwrap();
        assert_eq!(generation(&db), 44, "SQL direto de outros escritores também invalida ABA");
        {
            let tx = db.transaction().unwrap();
            tx.execute("UPDATE bots SET config=?1 WHERE id='bot'", [r#"{"computer":true}"#]).unwrap();
            assert_eq!(generation(&tx), 45);
            tx.rollback().unwrap();
        }
        assert_eq!(generation(&db), 44, "perfil e geração fazem rollback juntos");
        db.execute("UPDATE bots SET rev=rev+1 WHERE id='bot'", []).unwrap();
        assert_eq!(generation(&db), 44, "read marks e touch não revogam ferramentas");
        drop(db);
        let db = Connection::open(&path).unwrap();
        migrate(&db).unwrap();
        assert_eq!(generation(&db), 44, "reabrir não renova capturas");
        db.execute("UPDATE bots SET deleted=1 WHERE id='bot'", []).unwrap();
        assert_eq!(generation(&db), 45);
        drop(db);
        std::fs::remove_file(path).unwrap();
    }
}
