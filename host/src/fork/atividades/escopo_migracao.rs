//! DDL de abertura, na conexão existente. Não é uma transação alternativa de runtime.
use crate::store::{Store, next_rev};
use anyhow::{Result, bail};
use rusqlite::TransactionBehavior;

pub(crate) fn migrate(store: &Store) -> Result<()> {
    let mut db = store.db.try_lock().map_err(|_| anyhow::anyhow!("scope_source_store_unavailable"))?;
    if !db.is_autocommit() {
        bail!("scope_source_nested_migration");
    }
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let existed: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='donna_scope_authority_state')",
        [],
        |row| row.get(0),
    )?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_scope_authority_state (
            singleton INTEGER PRIMARY KEY CHECK(singleton=1),
            version INTEGER NOT NULL CHECK(version=1),
            revision INTEGER NOT NULL CHECK(revision>0 AND revision<=9007199254740991),
            state TEXT NOT NULL CHECK(state IN ('uninitialized','invalidated')),
            reason TEXT,
            CHECK((state='uninitialized' AND reason IS NULL) OR
                  (state='invalidated' AND reason IS NOT NULL AND reason IN
                   ('principal_changed','policy_changed','binding_changed','recovery_required')))
        )",
    )?;
    // Fonte positiva 05a: começa vazia; só o bootstrap do primeiro pedido validado grava.
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_scope_personal_activation (
            singleton INTEGER PRIMARY KEY CHECK(singleton=1),
            version INTEGER NOT NULL CHECK(version=1),
            barrier_revision INTEGER NOT NULL CHECK(barrier_revision>0 AND barrier_revision<=9007199254740991),
            principal_bot_id TEXT NOT NULL,
            policy TEXT NOT NULL CHECK(policy='personal_private_read_v1'),
            policy_revision INTEGER NOT NULL CHECK(policy_revision>0 AND policy_revision<=9007199254740991),
            recovery_epoch TEXT NOT NULL
        )",
    )?;
    // Ausência posterior da linha nunca é genesis. Não ressuscita estado removido.
    if !existed {
        let rev = next_rev(&tx)?;
        if !(1..=9_007_199_254_740_991).contains(&rev) {
            bail!("scope_source_revision_invalid");
        }
        tx.execute(
            "INSERT INTO donna_scope_authority_state(singleton,version,revision,state,reason) VALUES(1,1,?1,'uninitialized',NULL)",
            [rev],
        )?;
    }
    tx.commit()?;
    Ok(())
}
