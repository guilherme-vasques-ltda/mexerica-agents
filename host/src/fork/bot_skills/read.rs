//! API candidata Hub, por ID exato, sem leitura de path ou anúncio de fs.readTextFile.
use super::Prepared;
use crate::{
    LockExt,
    agent::acp::Acp,
    fork::{bot_admission, sessoes::bindings},
    hub::Hub,
};
use anyhow::{Result, anyhow, ensure};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{Value, json};
use std::task::Poll;

pub(crate) const METHOD: &str = "_mexerica/skills/read";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Request {
    #[serde(rename = "sessionId")]
    pub(crate) session: String,
    #[serde(rename = "preparationId")]
    pub(crate) preparation: String,
    #[serde(rename = "skillId")]
    pub(crate) skill: String,
}

impl Prepared {
    /// Só na ativação canônica, antes de session/prompt; nunca captura no pedido/retorno.
    pub(crate) fn activate(&mut self, hub: &Hub) -> Result<()> {
        let _boundary = hub.store.memory_boundary.locked();
        let binding = self.binding.as_ref().ok_or_else(|| anyhow!("skills_read_unavailable"))?;
        let execution = bindings::capture(hub, &self.bot, binding.id.as_str())?;
        ensure!(execution.turn_token() == self.ticket.execution_token, "skills_read_stale");
        if let Some(original) = &self.execution {
            ensure!(*original == execution, "skills_read_stale");
        } else {
            self.execution = Some(execution);
        }
        Ok(())
    }

    /// Só metadado de protocolo, sem corpo/caminho/token de execução no prompt.
    pub(crate) fn descriptor(&self) -> Value {
        json!({"method":METHOD,"preparationId":self.ticket.attempt,"status":"candidate"})
    }

    fn check_read(&self, runtime: &crate::hub::Runtime, c: &Connection, request: &Request) -> Result<()> {
        ensure!(request.preparation == self.ticket.attempt, "skills_read_stale");
        let execution = self.execution.as_ref().ok_or_else(|| anyhow!("skills_read_unavailable"))?;
        let binding = self.binding.as_ref().ok_or_else(|| anyhow!("skills_read_unavailable"))?;
        ensure!(binding.acp_session.as_deref() == Some(request.session.as_str()), "skills_read_stale");
        ensure!(
            matches!(runtime.status, crate::hub::BotStatus::Working | crate::hub::BotStatus::NeedsInput)
                && runtime.memory_turn_token.as_deref() == Some(self.ticket.execution_token.as_str())
                && runtime.session == binding.acp_session
                && runtime
                    .lane
                    .as_ref()
                    .is_some_and(|lane| lane.chat == self.lane.chat && lane.session == self.lane.session),
            "skills_read_stale"
        );
        let live = bindings::assert_execution_live(c, execution)?;
        ensure!(live == binding.session_ref, "skills_read_stale");
        let record = bot_admission::current_on(c, &self.bot)?.ok_or_else(|| anyhow!("skills_read_stale"))?;
        ensure!(
            record.ticket() == self.ticket
                && record.phase == bot_admission::Phase::Permitted
                && !record.stop_requested
                && record.conversation_id == self.lane.chat
                && record.origin == self.origin
                && record.entry_ids == self.entries,
            "skills_read_stale"
        );
        self.check_sources(c)?;
        ensure!(self.selection.as_ref().is_some_and(|s| s.skills.contains(&request.skill)), "skills_read_unavailable");
        Ok(())
    }

    /// Revalida antes da cópia; entrega exige nova conferência, no mesmo escopo da escrita.
    pub(crate) fn read(&self, hub: &Hub, request: &Request) -> Result<String> {
        let _boundary = hub.store.memory_boundary.locked();
        let runtime = hub.runtime(&self.bot);
        let c = hub.store.db.locked();
        self.check_read(&runtime, &c, request)?;
        self.catalog.as_ref().ok_or_else(|| anyhow!("skills_read_unavailable"))?.document(&request.skill)
    }

    /// Não retém locks durante stdin/pipe Pending; commit DB e `poll_write` são ordenados.
    pub(crate) async fn deliver(&self, hub: &Hub, acp: &Acp, rpc_id: Value, request: &Request) -> Result<()> {
        let bytes = self.read(hub, request)?;
        acp.respond_scoped(rpc_id, json!({"content":bytes}), |effect| self.delivery_scope(hub, request, effect)).await
    }

    pub(crate) fn delivery_scope(
        &self,
        hub: &Hub,
        request: &Request,
        effect: &mut dyn FnMut() -> Poll<Result<()>>,
    ) -> Poll<Result<()>> {
        let _boundary = hub.store.memory_boundary.locked();
        let runtime = hub.runtime(&self.bot);
        let c = hub.store.db.locked();
        if let Err(error) = self.check_read(&runtime, &c, request) {
            return Poll::Ready(Err(error));
        }
        // Conteúdo de skill é externo mesmo sem tool_call do motor. As duas
        // marcas canônicas precedem qualquer prefixo e sobrevivem ao fim/revogação.
        let exposed = crate::store::Store::guard_mark_external_on(&c, &self.bot)
            .and_then(|()| bindings::mark(&c, &self.bot, &request.session, "leitura de skill externa"));
        if let Err(error) = exposed {
            return Poll::Ready(Err(error));
        }
        effect()
    }
}
