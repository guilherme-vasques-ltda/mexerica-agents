//! Captura da chamada MCP, sem substituir as autoridades próprias de app/webhook.
use crate::{
    LockExt,
    fork::sessoes::{ExecutionRef, bindings, store::SessionRef},
    hub::Hub,
    store::Lane,
};
use anyhow::{Context, Result, ensure};
use rusqlite::Connection;

pub(crate) struct ToolOrigin {
    pub(super) execution: ExecutionRef,
    session: SessionRef,
    message: Option<String>,
    creation: bool,
    invocation: Option<crate::fork::routine_creation::Invocation>,
}

impl ToolOrigin {
    pub(super) fn capture(hub: &Hub, bot: &str, execution: &ExecutionRef, changes: bool) -> Result<Self> {
        let session = bindings::recheck(hub, execution)?;
        let message = changes.then(|| crate::chat::manage::user_turn(hub, bot)).transpose()?;
        let origin = Self { execution: execution.clone(), session, message, creation: false, invocation: None };
        origin.check(&hub.store.db.locked(), bot)?;
        Ok(origin)
    }

    /// Capacidade estreita de criar uma tarefa própria. A origem persistida e a
    /// cota são conferidas na transação; não autoriza editar uma tarefa existente.
    pub(super) fn capture_creation(
        hub: &Hub,
        bot: &str,
        execution: &ExecutionRef,
        invocation: Option<&str>,
        args: &serde_json::Value,
    ) -> Result<Self> {
        ensure!(args.get("id").is_none(), "invalid_field: criação não aceita id");
        let session = bindings::recheck(hub, execution)?;
        let message = crate::chat::manage::user_turn(hub, bot).ok();
        let invocation = invocation
            .map(|id| crate::fork::routine_creation::Invocation::new(execution, bot, id, args))
            .transpose()?;
        let origin = Self { execution: execution.clone(), session, message, creation: true, invocation };
        origin.check(&hub.store.db.locked(), bot)?;
        Ok(origin)
    }

    pub(super) fn is_creation(&self) -> bool {
        self.creation
    }
    pub(super) fn human_message(&self) -> Option<&str> {
        self.message.as_deref()
    }
    pub(super) fn invocation(&self) -> Option<&crate::fork::routine_creation::Invocation> {
        self.invocation.as_ref()
    }

    /// Somente Connection; usada na mesma transação IMMEDIATE do efeito/fila.
    pub(crate) fn check(&self, db: &Connection, bot: &str) -> Result<()> {
        let session = bindings::assert_execution_live(db, &self.execution)?;
        let binding = bindings::resolve(db, self.execution.acp_binding().context("chamada vencida")?, bot)?;
        ensure!(session == self.session && binding.session_ref == session, "chamada vencida");
        if let Some(message) = &self.message {
            let entry = crate::store::entry_on(db, message)?;
            ensure!(
                entry.is_some_and(|e| e.bot_id == bot && e.session_id == session.id && e.is_user_authored()),
                "Esta ação precisa de um pedido do dono na sessão de origem."
            );
        }
        Ok(())
    }

    pub(super) fn recheck(&self, hub: &Hub, bot: &str) -> Result<()> {
        bindings::recheck(hub, &self.execution)?;
        self.check(&hub.store.db.locked(), bot)
    }

    pub(super) fn lane(&self) -> Lane {
        Lane {
            chat: self.session.chat_id.clone(),
            session: (self.session.id != self.session.chat_id).then(|| self.session.id.clone()),
        }
    }
}
