//! Ledger negativo D430; admissão/consentimento ficam para a próxima fatia.
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};

pub(super) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(include_str!("personal_link.sql"))?;
    Ok(())
}

pub(super) fn origin_on(db: &Connection, key: &str) -> Result<Option<String>> {
    let raw: Option<Vec<u8>> =
        db.query_row("SELECT binding FROM personal_link_origins WHERE key=?", [key], |row| row.get(0)).optional()?;
    raw.map(|raw| crate::fork::personal_link::generation(&raw, key)).transpose()
}
