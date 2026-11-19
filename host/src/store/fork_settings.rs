//! Primitiva transacional para documentos do fork; erro de leitura nunca vira ausência.
use super::Store;
use crate::LockExt;
use anyhow::Result;
use rusqlite::OptionalExtension;

impl Store {
    pub(crate) fn kv_compare_exchange(&self, key: &str, expected: Option<&str>, value: &str) -> Result<bool> {
        let mut db = self.db.locked();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let current: Option<String> =
            tx.query_row("SELECT v FROM kv WHERE k = ?1", [key], |row| row.get(0)).optional()?;
        if current.as_deref() != expected {
            return Ok(false);
        }
        tx.execute("INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v", [key, value])?;
        tx.commit()?;
        Ok(true)
    }
}
