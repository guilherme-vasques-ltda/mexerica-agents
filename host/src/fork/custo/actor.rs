//! Ganchos do ator: captura a seleção efetiva antes do prompt e grava uma vez ao terminar.
use super::{Actor, Done};

impl Actor {
    pub(super) fn cost_begin(&mut self, session: &str) -> anyhow::Result<()> {
        self.cost_turn = Some(
            crate::fork::custo::Pending::begin(
                &self.hub.store,
                &self.cfg,
                &self.lane,
                session,
                self.session_fresh,
                self.conn.as_ref().and_then(|c| c.adapter_version.clone()),
            )?
            // A compactação oculta anexa ao turno corrente (mesmo número): o uso dela não entra na conta do turno.
            .with_turn(self.turn.filter(|_| self.compacting.is_none())),
        );
        Ok(())
    }

    pub(super) fn cost_dispatch(&mut self, session: &str) -> anyhow::Result<()> {
        // Também atende ao envio direto de um prompt já preparado pela admissão.
        if self.cost_turn.is_none() {
            self.cost_begin(session)?;
        }
        if let Some(pending) = &mut self.cost_turn {
            pending.dispatched(&self.hub.store)?;
        }
        Ok(())
    }

    pub(super) fn cost_observe(&mut self, update: &serde_json::Value) {
        if let Some(pending) = &mut self.cost_turn {
            pending.observe(update);
        }
    }

    pub(super) fn cost_finish(&mut self, done: &Done) {
        if let Some(pending) = self.cost_turn.take()
            && let Err(error) = pending.finish(&self.hub.store, done.as_ref().ok())
        {
            tracing::warn!(%error, "não foi possível gravar o uso do turno");
        }
    }
}
