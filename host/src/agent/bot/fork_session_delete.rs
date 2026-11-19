//! D545: só o ator decide se a extra está parada; nenhuma espera entre a decisão e a Tx.
use super::Actor;
use crate::{
    LockExt,
    fork::{autoridade::Autoridade, sessoes::excluir},
    store::Lane,
};
use anyhow::{Result, ensure};
use serde_json::Value;

impl Actor {
    pub(super) fn session_delete(&mut self, request: &excluir::Request, original: &Autoridade) -> Result<Value> {
        ensure!(request.bot_id == self.cfg.id && request.hub_id == self.hub.host_id, "session_context_mismatch");
        let lane = Lane { chat: request.bot_id.clone(), session: Some(request.session_id.clone()) };
        ensure!(!self.lane_busy(&lane), "session_busy");
        crate::fork::donna_fixa::pedido::antes_de_gravar(&self.hub.store);
        let event = {
            let _emit = self.hub.emit_lock.locked();
            let event = original.with_db(&self.hub, None, |c| excluir::delete_on(c, request))?;
            let _ = self.hub.events.send(event.clone());
            event
        };
        // A remoção durável já confirmou. Nenhum turno/espera desta lane pode estar rodando.
        let slot = format!("session:{}", request.session_id);
        self.hub.pontes.revogar_slot(&self.cfg.id, Some(&slot));
        self.run_sessions.remove(&slot);
        if self.handoff_chat.as_ref() == Some(&lane) {
            self.handoff = None;
            self.handoff_chat = None;
        }
        if self.has_session_card(&lane) {
            self.session_card = None;
        }
        self.hub.publish_bot(&self.cfg.id);
        Ok(event)
    }
}
