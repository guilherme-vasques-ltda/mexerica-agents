//! Grava cartão e selo na mesma Connection; a publicação pertence ao Hub pós-commit.
use crate::LockExt;
use crate::fork::sessoes::ExecutionRef;
use crate::fork::spaces::tool_context;
use crate::store::{BotConfig, Entry, EntryKind, Lane, Store, check_execution_on, insert_visible_on};
use anyhow::Result;

impl Store {
    pub(crate) fn insert_permission_card(
        &self,
        cfg: &BotConfig,
        lane: &Lane,
        turn: i64,
        data: &serde_json::Value,
        execution: &ExecutionRef,
        pedido: Option<&crate::fork::delegacao::pausa::Pedido<'_>>,
    ) -> Result<(Option<Entry>, Entry)> {
        let mut db = self.db.locked();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let pair = match pedido {
            Some(pedido) => crate::fork::delegacao::pausa::inserir_na(&tx, cfg, lane, turn, data, execution, pedido)?,
            None => Self::insert_permission_card_on(&tx, cfg, lane, turn, data, execution)?,
        };
        tx.commit()?;
        Ok(pair)
    }
    pub(crate) fn insert_permission_card_on(
        db: &rusqlite::Connection,
        cfg: &BotConfig,
        lane: &Lane,
        turn: i64,
        data: &serde_json::Value,
        execution: &ExecutionRef,
    ) -> Result<(Option<Entry>, Entry)> {
        check_execution_on(db, lane, Some(execution))?;
        let pair = insert_visible_on(db, &uuid::Uuid::new_v4().to_string(), lane, EntryKind::Permission, turn, data)?;
        tool_context::record_card_on(db, cfg, &pair.1)?;
        check_execution_on(db, lane, Some(execution))?;
        Ok(pair)
    }
}
