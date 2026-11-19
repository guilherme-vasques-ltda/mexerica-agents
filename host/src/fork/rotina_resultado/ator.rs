//! I-023 E2: no ator, depois do `await` da entrega. Sem `await` até E3 (`Routines::record_declaration`).
use super::Actor;
use crate::fork::rotina_resultado::{AMBIGUOUS, Declaration, STALE};
use crate::fork::sessoes::{ExecutionRef, bindings};
use crate::{LockExt, store::run_session_key};
use anyhow::{Context, Result, anyhow, ensure};
use rusqlite::params;

impl Actor {
    pub(super) fn routine_result(&mut self, execution: &ExecutionRef, declaration: Declaration) -> Result<()> {
        // O mesmo vínculo de E1, relido (revogação, vínculo ativo, turno do runtime): nenhuma concessão nova.
        bindings::recheck(&self.hub, execution)?;
        let run = self
            .active_routine
            .clone()
            .filter(|_| self.turn.is_some() && self.finishing.is_none())
            .ok_or_else(|| anyhow!(STALE))?;
        let slot = crate::routines::session_slot(&run);
        let acp = {
            let db = self.hub.store.db.locked();
            bindings::resolve(&db, execution.acp_binding().context(STALE)?, &self.cfg.id)?.acp_session
        };
        let acp = acp.ok_or_else(|| anyhow!(STALE))?;
        ensure!(
            self.turn_session.as_deref() == Some(acp.as_str())
                && self.session_for(Some(&slot)).as_deref() == Some(&acp),
            STALE
        );
        ensure!(!self.acp_in_other_slot(&slot, &acp)?, AMBIGUOUS);
        self.hub.routines.record_declaration(&self.hub, &self.cfg.id, &run, execution, declaration)
    }

    /// O ID da sessão ACP vem do motor: guardado também em outro slot do bot, a identidade é ambígua.
    fn acp_in_other_slot(&self, slot: &str, acp: &str) -> Result<bool> {
        if self.session_id.as_deref() == Some(acp) || self.run_sessions.iter().any(|(s, sid)| s != slot && sid == acp) {
            return Ok(true);
        }
        let prefix = run_session_key(&self.cfg.id, "");
        let mine = run_session_key(&self.cfg.id, slot);
        let other: bool = self.hub.store.db.locked().query_row(
            "SELECT EXISTS(SELECT 1 FROM kv WHERE substr(k,1,length(?1))=?1 AND k<>?2 AND v=?3)",
            params![prefix, mine, acp],
            |r| r.get(0),
        )?;
        Ok(other)
    }
}
