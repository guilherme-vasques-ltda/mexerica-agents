//! Pausa global durável: reservas existentes e recuperação mantêm sua política.
use super::{LockExt, Result, Routines, Store, Trigger, Value, json, now_ms};

impl Routines {
    pub fn pause_state(&self) -> Value {
        json!({"paused": self.state.locked().paused})
    }

    pub fn set_paused(&self, store: &Store, paused: bool) -> Result<Value> {
        self.change(store, |state| {
            if state.paused && !paused {
                let now = now_ms();
                // Ocorrências perdidas durante a pausa são atrasadas (MX-110).
                state.available_at = now;
                for routine in &mut state.routines {
                    if !routine.enabled || routine.deleted {
                        continue;
                    }
                    if routine.triggers.iter().any(|t| matches!(t, Trigger::Interval { .. } | Trigger::Cron { .. })) {
                        routine.schedule_generation = routine
                            .schedule_generation
                            .checked_add(1)
                            .ok_or_else(|| anyhow::anyhow!("Não foi possível atualizar a agenda desta rotina"))?;
                    }
                    for (trigger, next) in routine.triggers.iter().zip(&mut routine.next_runs) {
                        if matches!(trigger, Trigger::Interval { .. } | Trigger::Cron { .. }) {
                            *next = trigger.next(now)?;
                        }
                    }
                }
            }
            if state.paused != paused {
                state.pausas = state.pausas.saturating_add(1);
            }
            state.paused = paused;
            Ok(json!({"paused": paused}))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routines::{Run, Status, new_run, tests::routine};
    use std::path::Path;

    #[test]
    fn estado_antigo_reinicio_e_retomada_idempotente() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        store.kv_set(super::super::STATE_KEY, r#"{"routines":[],"runs":[]}"#).unwrap();
        let routines = Routines::default();
        routines.restore(&store).unwrap();
        assert_eq!(routines.pause_state()["paused"], false);
        let mut once = routine();
        once.triggers = vec![Trigger::Once { at: 1 }];
        once.next_runs = vec![None];
        let mut disabled = routine();
        disabled.id = "disabled".into();
        disabled.enabled = false;
        routines
            .change(&store, |s| {
                s.routines.extend([once, disabled]);
                Ok(())
            })
            .unwrap();
        routines.set_paused(&store, true).unwrap();
        let restored = Routines::default();
        restored.restore(&store).unwrap();
        assert_eq!(restored.pause_state()["paused"], true);
        restored.set_paused(&store, false).unwrap();
        let next = restored.state.locked().routines[1].next_runs.clone();
        restored.set_paused(&store, false).unwrap();
        let state = restored.state.locked();
        assert_eq!(state.routines[0].next_runs, vec![None]);
        assert!(!state.routines[1].enabled);
        assert_eq!(state.routines[1].next_runs, next);
    }

    #[test]
    fn reserva_anterior_pode_iniciar_e_hooks_refletem_pausa() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let routines = Routines::default();
        let mut r = routine();
        r.triggers = vec![Trigger::Webhook];
        let mut run = new_run(&r, Value::Null, None);
        run.status = Status::Starting;
        let id = run.id.clone();
        routines
            .change(&store, |s| {
                s.routines.push(r);
                s.runs.push(run);
                Ok(())
            })
            .unwrap();
        assert!(routines.hooks()[0].enabled);
        routines.set_paused(&store, true).unwrap();
        assert!(!routines.hooks()[0].enabled);
        let mut state = routines.state.locked().clone();
        super::super::cadeias::restaurar(&store, &mut state).unwrap();
        *routines.state.locked() = state;
        let receipt = routines.prepare_attempt(&store, "bot", &id).unwrap();
        assert!(matches!(
            routines.mark_started(&store, "bot", &id, &receipt).unwrap(),
            crate::routines::Inicio::Iniciada
        ));
        assert!(routines.state.locked().runs[0].status == Status::Running);
        routines.set_paused(&store, false).unwrap();
        assert!(routines.hooks()[0].enabled);
    }

    #[test]
    fn metodos_globais_respeitam_local_e_control() {
        use crate::api::devices::{Caller, permit};
        use crate::store::Scope;
        let control = Caller::Device { key: "test".into(), scopes: vec![Scope::Control], autoridade: None };
        let denied = Caller::Device { key: "test".into(), scopes: vec![], autoridade: None };
        for method in ["routinesPause", "setRoutinesPaused"] {
            assert!(permit(&Caller::Local, method).is_ok());
            assert!(permit(&control, method).is_ok());
            assert!(permit(&denied, method).is_err());
        }
    }

    #[test]
    fn falha_no_commit_nao_confirma_pausa() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let routines = Routines::default();
        store.db.locked().execute_batch("CREATE TRIGGER reject_pause BEFORE INSERT ON kv WHEN NEW.k = 'routines.v1' BEGIN SELECT RAISE(ABORT, 'commit refused'); END;").unwrap();
        assert!(routines.set_paused(&store, true).is_err());
        assert_eq!(routines.pause_state()["paused"], false);
    }

    #[test]
    fn recusa_admissao_e_preserva_pendentes_e_recuperacao() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let routines = Routines::default();
        let r = routine();
        let pending = new_run(&r, Value::Null, None);
        let mut running: Run = new_run(&r, Value::Null, None);
        running.status = Status::Running;
        routines
            .change(&store, |s| {
                s.routines.push(r);
                s.runs.extend([pending, running]);
                Ok(())
            })
            .unwrap();
        routines.set_paused(&store, true).unwrap();
        assert!(routines.enqueue(&store, "bot", "routine", Value::Null, None, true).is_err());
        let restored = Routines::default();
        restored.restore(&store).unwrap();
        let state = restored.state.locked();
        assert_eq!(state.runs.len(), 2);
        assert!(state.runs[0].status == Status::Pending);
        assert!(state.runs[1].status == Status::Recovering);
        let id = state.runs[1].id.clone();
        drop(state);
        assert!(restored.prepare_attempt(&store, "bot", &id).is_err());
        let state = restored.state.locked();
        assert!(state.runs[1].status == Status::Interrupted);
    }
}

#[cfg(test)]
mod admission_tests {
    use super::*;
    use crate::{
        fork::bot_admission as admission,
        routines::{Status, new_run, tests::routine},
    };
    use std::path::Path;

    #[test]
    fn starting_com_permissao_incerta_nao_volta_a_pending_e_reserva_sem_envio_volta() {
        for permitted in [false, true] {
            let store = Store::open(Path::new(":memory:")).unwrap();
            let routines = Routines::default();
            let routine = routine();
            let mut run = new_run(&routine, Value::Null, None);
            run.status = Status::Starting;
            let id = run.id.clone();
            routines
                .change(&store, |s| {
                    s.routines.push(routine);
                    s.runs.push(run);
                    Ok(())
                })
                .unwrap();
            let cfg: crate::store::BotConfig = serde_json::from_str(r#"{"id":"bot","name":"Bot"}"#).unwrap();
            store.save_bot(&cfg).unwrap();
            let ticket =
                admission::reserve(&store, &cfg, "bot", "token-hub", &admission::Origin::Routine { id: id.clone() })
                    .unwrap();
            if permitted {
                admission::permit(&store, &cfg, &ticket, None, &[], None).unwrap();
            }
            let restored = Routines::default();
            restored.restore(&store).unwrap();
            let state = restored.state.locked();
            assert!(state.runs[0].status == if permitted { Status::Interrupted } else { Status::Pending });
            assert_eq!(state.runs[0].report_pending, permitted);
            assert_eq!(state.runs[0].started_at, None);
            assert!(!state.paused);
        }
    }
}
