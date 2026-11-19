// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! The memory crate's tables live in this database. Each change is one transaction stamped with the next `rev`,
//! like every other mutation; see `codync_mem` and `chat::memory`.

use super::{Store, next_rev, now_ms};
use crate::LockExt;
use anyhow::Result;
use rusqlite::Connection;

impl Store {
    /// A memory change, in one transaction with its `rev`. The closure gets the time and the `rev` to stamp.
    pub fn memory_write<T>(
        &self,
        change: impl FnOnce(&Connection, i64, i64) -> codync_mem::store::Result<T>,
    ) -> Result<T> {
        self.memory_write_in_execution(None, change)
    }
    pub(crate) fn memory_write_in_execution<T>(
        &self,
        execution: Option<&crate::fork::sessoes::ExecutionRef>,
        change: impl FnOnce(&Connection, i64, i64) -> codync_mem::store::Result<T>,
    ) -> Result<T> {
        let mut c = self.db.locked();
        let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        crate::fork::sessoes::bindings::check_optional(&tx, execution)?;
        let rev = next_rev(&tx)?;
        let out = change(&tx, now_ms(), rev)?;
        tx.execute("INSERT INTO kv(k, v) VALUES('memory.rev', ?1) ON CONFLICT(k) DO UPDATE SET v = ?1", [rev])?;
        tx.commit()?;
        Ok(out)
    }

    pub fn memory_read<T>(&self, read: impl FnOnce(&Connection) -> codync_mem::store::Result<T>) -> Result<T> {
        Ok(read(&self.db.locked())?)
    }

    /// The `rev` of the last memory change anywhere: clients fetch a bot's memory again when it moves.
    pub fn memory_rev(&self) -> i64 {
        self.kv_get("memory.rev").and_then(|v| v.parse().ok()).unwrap_or(0)
    }
}
