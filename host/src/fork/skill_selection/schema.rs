use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension};

// Um Host antigo também aciona estes triggers, mas eles não autorizam nem bloqueiam
// esse binário: a proteção de escrita exige o gate no produtor do Host atual.
const TRIGGERS: [(&str, &str); 2] = [
    (
        "donna_skills_version_insert",
        "CREATE TRIGGER donna_skills_version_insert AFTER INSERT ON bots BEGIN UPDATE bots SET donna_skills_version=lower(hex(randomblob(16))) WHERE id=NEW.id; END",
    ),
    (
        "donna_skills_version_update",
        "CREATE TRIGGER donna_skills_version_update AFTER UPDATE OF config, deleted ON bots WHEN OLD.deleted IS NOT NEW.deleted OR CASE WHEN OLD.config IS NEW.config THEN 0 WHEN NOT json_valid(OLD.config) OR NOT json_valid(NEW.config) THEN 1 WHEN COALESCE(json_type(OLD.config,'$.skills'),'array') != 'array' OR COALESCE(json_type(NEW.config,'$.skills'),'array') != 'array' THEN 1 ELSE EXISTS(SELECT key, type, atom FROM json_each(OLD.config,'$.skills') EXCEPT SELECT key, type, atom FROM json_each(NEW.config,'$.skills')) OR EXISTS(SELECT key, type, atom FROM json_each(NEW.config,'$.skills') EXCEPT SELECT key, type, atom FROM json_each(OLD.config,'$.skills')) END BEGIN UPDATE bots SET donna_skills_version=lower(hex(randomblob(16))) WHERE id=NEW.id; END",
    ),
];

pub(crate) fn migrate(c: &Connection) -> Result<()> {
    let tx = c.unchecked_transaction()?;
    let columns: Vec<String> =
        tx.prepare("PRAGMA table_info(bots)")?.query_map([], |r| r.get(1))?.collect::<rusqlite::Result<_>>()?;
    let new_column = !columns.iter().any(|column| column == "donna_skills_version");
    if new_column {
        tx.execute_batch("ALTER TABLE bots ADD COLUMN donna_skills_version TEXT")?;
    }
    let mut trigger_gap = false;
    for (name, sql) in TRIGGERS {
        let existing: Option<String> = tx
            .query_row("SELECT sql FROM sqlite_master WHERE type='trigger' AND name=?1", [name], |r| r.get(0))
            .optional()?;
        if let Some(existing) = existing {
            ensure!(existing == sql, "definição divergente do trigger {name}");
        } else {
            tx.execute_batch(sql)?;
            trigger_gap = true;
        }
    }
    // Sem algum trigger pode ter havido um ABA invisível, inclusive em tombstones.
    if new_column || trigger_gap {
        tx.execute("UPDATE bots SET donna_skills_version=lower(hex(randomblob(16)))", [])?;
    }
    tx.commit()?;
    Ok(())
}
