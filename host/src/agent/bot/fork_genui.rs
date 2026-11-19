//! G1b: `mostrar_componente` no ator. Só o turno vivo do próprio bot na conversa capturada; pedido de
//! outro bot/Times, rotina e canal não têm dono na conversa e recusam.
use super::Actor;
use crate::LockExt;
use crate::fork::spaces::tool_context::ToolContext;
use serde_json::Value;

impl Actor {
    pub(super) fn show_choice(
        &mut self,
        context: &ToolContext,
        invocation: &str,
        payload: &crate::fork::genui::contract::ValidatedChoice,
    ) -> anyhow::Result<Value> {
        anyhow::ensure!(self.turn.is_some() && !self.waiting_for_reset(), "no turn is running");
        anyhow::ensure!(
            self.active_request.is_none() && self.active_routine.is_none() && self.active_group.is_none(),
            "mostrar_componente não está disponível em pedido de outro bot, Times, canal ou rotina"
        );
        // Fecha o trecho em curso antes da barreira (flush grava entradas por conta própria).
        self.close_seg();
        let hub = self.hub.clone();
        // Mesma barreira de updateBot/exclusão/troca de turno: nada muda entre a conferência e o commit.
        let _boundary = hub.store.memory_boundary.locked();
        context.check_actor(&hub, &self.cfg, &self.lane, self.turn_session.as_deref())?;
        crate::fork::genui::flow::show(&hub, context, invocation, payload)
    }
}
