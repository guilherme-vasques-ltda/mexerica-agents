//! I-023 E3: grava a declaração na mesma transação IMMEDIATE de `routines.v1`, pela guarda de origem existente
//! (`ToolOrigin::check`, só com a `Connection` recebida). Ordem dos locks: estado de `Routines` → banco.
use super::{Routines, Status, tool_origin::ToolOrigin};
use crate::fork::rotina_resultado::{self as resultado, Declaration};
use crate::fork::sessoes::ExecutionRef;
use crate::hub::Hub;
use anyhow::{Result, anyhow, ensure};

impl Routines {
    /// Síncrono: o ator chama logo depois de E2, sem `await` entre os dois. Recusa sem gravar nada se o vínculo foi
    /// revogado, a execução não está em andamento ou a sessão é outra; conflito é marcado e devolvido como erro.
    pub(crate) fn record_declaration(
        &self,
        hub: &Hub,
        bot: &str,
        run: &str,
        execution: &ExecutionRef,
        declaration: Declaration,
    ) -> Result<()> {
        let origin = ToolOrigin::capture(hub, bot, execution, false)?;
        let recorded = self.change_in_origin(&hub.store, Some(bot), Some(&origin), |s| {
            let r = s
                .runs
                .iter_mut()
                .find(|r| r.id == run && r.bot_id == bot && matches!(r.status, Status::Running | Status::Recovering))
                .ok_or_else(|| anyhow!(resultado::STALE))?;
            Ok(resultado::record(&mut r.declaration, &mut r.declaration_conflict, declaration))
        })?;
        ensure!(recorded, resultado::CONFLICT);
        Ok(())
    }
}

/// Testes diretos de E3 (sem API nem ator).
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::super::{Run, Status, new_run, tests::routine};
    use crate::fork::rotina_resultado::{CONFLICT, Declared, parse_declaration};
    use crate::fork::sessoes::{bindings, call_tests::interleave};
    use crate::{LockExt, hub::Hub, store::Store};
    use serde_json::{Value, json};
    use std::sync::{Arc, atomic::Ordering};

    fn hub() -> Arc<Hub> {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.save_bot(&serde_json::from_value(json!({"id":"bot","name":"Bot"})).unwrap()).unwrap();
        let dir = std::env::temp_dir().join(format!("rotina-resultado-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Hub::new(
            store,
            "fixture".into(),
            crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
            "f".into(),
            0,
        )
    }

    fn running(hub: &Hub, id: &str, status: Status) {
        hub.routines
            .change(&hub.store, |s| {
                if s.routines.is_empty() {
                    s.routines.push(routine());
                }
                let mut run: Run = new_run(&s.routines[0], Value::Null, None);
                run.id = id.into();
                run.status = status;
                s.runs.push(run);
                Ok(())
            })
            .unwrap();
    }

    fn run(hub: &Hub, id: &str) -> Run {
        hub.routines.state.locked().runs.iter().find(|r| r.id == id).unwrap().clone()
    }

    fn declared(outcome: &str, summary: &str) -> crate::fork::rotina_resultado::Declaration {
        parse_declaration(&json!({"outcome": outcome, "summary": summary}), 1).unwrap()
    }

    #[test]
    fn grava_uma_vez_e_conflito_fica_na_mesma_transacao() {
        let hub = hub();
        let execution = bindings::fixture_execution(&hub, "bot");
        running(&hub, "r1", Status::Running);
        hub.routines.record_declaration(&hub, "bot", "r1", &execution, declared("reported", "Feito")).unwrap();
        hub.routines.record_declaration(&hub, "bot", "r1", &execution, declared("reported", "Feito")).unwrap();
        let error =
            hub.routines.record_declaration(&hub, "bot", "r1", &execution, declared("failed", "Outro")).unwrap_err();
        assert_eq!(error.to_string(), CONFLICT);
        let r1 = run(&hub, "r1");
        assert!(r1.declaration_conflict);
        assert!(
            matches!(&r1.declaration, Some(crate::fork::rotina_resultado::Lenient::Ok(d)) if d.outcome == Declared::Reported)
        );
        // Execução que não está em andamento: recusa sem gravar.
        running(&hub, "done", Status::Succeeded);
        assert!(hub.routines.record_declaration(&hub, "bot", "done", &execution, declared("reported", "x")).is_err());
        assert!(run(&hub, "done").declaration.is_none());
    }

    #[test]
    fn revogacao_antes_da_transacao_de_e3_recusa_sem_gravar_nem_marcar_conflito() {
        let hub = hub();
        let execution = bindings::fixture_execution(&hub, "bot");
        running(&hub, "r1", Status::Running);
        running(&hub, "r2", Status::Recovering);
        hub.routines.record_declaration(&hub, "bot", "r1", &execution, declared("reported", "Feito")).unwrap();
        let before = hub.store.kv_read("routines.v1").unwrap();
        // Alcance (D70): chama E3 (`record_declaration`) direto, sem a API (E1) nem o ator (E2). A revogação real do
        // PR 3 entra depois da captura de origem válida e antes de abrir a transação; E1/E2 têm prova em routines_e2e.
        let store_hub = hub.clone();
        let reached = interleave(&hub.store, "routine_commit", move || {
            bindings::revoke(&store_hub.store, "bot", None).unwrap();
        });
        assert!(hub.routines.record_declaration(&hub, "bot", "r2", &execution, declared("no_news", "")).is_err());
        assert!(reached.load(Ordering::SeqCst));
        assert_eq!(hub.store.kv_read("routines.v1").unwrap(), before, "routines.v1 inalterado");
        assert!(run(&hub, "r2").declaration.is_none());
        // Com o vínculo revogado, nem a repetição igual nem a diferente recebem ack ou marcam conflito.
        assert!(hub.routines.record_declaration(&hub, "bot", "r1", &execution, declared("reported", "Feito")).is_err());
        assert!(hub.routines.record_declaration(&hub, "bot", "r1", &execution, declared("failed", "Outro")).is_err());
        assert!(!run(&hub, "r1").declaration_conflict);
    }
}
