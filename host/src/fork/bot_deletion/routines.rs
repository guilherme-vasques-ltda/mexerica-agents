//! Preparação reversível: a trava impede novas admissões do scheduler, sem
//! alterar estado vivo/publicar hooks/cancelar trabalho antes do commit.
use super::{LockExt, Result, Routines, Store, cancel_run, guard_live_runs, hook_set};

impl Routines {
    pub(crate) fn delete_bot_committed(&self, store: &Store, bot: &str) -> Result<i64> {
        let mut state = self.state.locked();
        let mut next = state.clone();
        next.routines.retain(|r| r.bot_id != bot);
        for run in next.runs.iter_mut().filter(|r| r.bot_id == bot) {
            if run.status.active() {
                cancel_run(run);
            }
            // A outbox não publica resultado ou aviso para um bot excluído.
            run.report_pending = false;
            run.result = None;
            run.heartbeat_texto = None;
        }
        let rev = store.delete_bot_with_routines(bot, &serde_json::to_string(&next)?, guard_live_runs(&next))?;
        let hooks_changed = hook_set(&state) != hook_set(&next);
        *state = next;
        drop(state);
        // Não adquirir active sob state: restore usa active -> state.
        self.release(bot);
        if hooks_changed {
            self.hooks_rev.send_modify(|v| *v += 1);
        }
        Ok(rev)
    }

    /// D543: a mesma preparação reversível para vários bots (exclusão de Space).
    /// O `commit` roda a Tx inteira do chamador; `None` aborta sem trocar o estado.
    /// `live_bots` traz os bots com execução viva ANTES do cancelamento, para a
    /// recusa `space_busy` dentro da Tx.
    pub(crate) fn delete_bots_committed<T>(
        &self,
        bots: &[String],
        commit: impl FnOnce(&str, &[(String, String)], &[String]) -> Result<Option<T>>,
    ) -> Result<Option<T>> {
        let mut state = self.state.locked();
        let mut next = state.clone();
        next.routines.retain(|r| !bots.contains(&r.bot_id));
        // Exclusão definitiva: nenhuma ocorrência, event/detail ou resultado do
        // Space fica no histórico global de rotinas. Busy é lido do estado original.
        next.runs.retain(|r| !bots.contains(&r.bot_id));
        let live_bots: Vec<String> = state
            .runs
            .iter()
            .filter(|r| r.status.active() && bots.contains(&r.bot_id))
            .map(|r| r.bot_id.clone())
            .collect();
        let raw = serde_json::to_string(&next)?;
        let live: Vec<(String, String)> = guard_live_runs(&next).map(|(b, r)| (b.to_owned(), r.to_owned())).collect();
        let Some(out) = commit(raw.as_str(), &live, &live_bots)? else {
            return Ok(None);
        };
        let hooks_changed = hook_set(&state) != hook_set(&next);
        *state = next;
        drop(state);
        for bot in bots {
            self.release(bot);
        }
        if hooks_changed {
            self.hooks_rev.send_modify(|v| *v += 1);
        }
        Ok(Some(out))
    }
}
