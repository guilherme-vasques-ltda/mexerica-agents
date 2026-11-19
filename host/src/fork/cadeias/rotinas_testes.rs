#![allow(clippy::unwrap_used)]
use super::*;
use crate::LockExt;
use std::path::Path;

fn inicializar(store: &Store) -> Routines {
    let routines = Routines::default();
    routines
        .change(store, |state| {
            state.routines.push(crate::routines::tests::routine());
            Ok(())
        })
        .unwrap();
    routines
}
fn uma_tentativa(store: &Store, routines: &Routines) -> Recibo {
    let value = routines.enqueue(store, "bot", "routine", json!({"source":"test"}), None, true).unwrap();
    let id = value["run"]["id"].as_str().unwrap();
    routines
        .change(store, |state| {
            state.runs.iter_mut().find(|r| r.id == id).unwrap().status = Status::Starting;
            Ok(())
        })
        .unwrap();
    routines.prepare_attempt(store, "bot", id).unwrap()
}

#[test]
fn queda_entre_commits_reusa_ocorrencia_e_run_id_mesmo_com_outro_gatilho_vencido() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let routines = inicializar(&store);
    routines
        .change(&store, |state| {
            let routine = &mut state.routines[0];
            routine.triggers.push(crate::routines::Trigger::Once { at: 120_000 });
            routine.next_runs.push(Some(120_000));
            Ok(())
        })
        .unwrap();
    store
        .db
        .locked()
        .execute_batch(
            "CREATE TRIGGER falhar_json BEFORE INSERT ON kv WHEN NEW.k='routines.v1'
        BEGIN SELECT RAISE(ABORT,'falha entre commits'); END;",
        )
        .unwrap();
    assert!(routines.schedule_due(&store, 61_000).is_err());
    let before: (String, String, String, i64) = store
        .db
        .locked()
        .query_row("SELECT id,run_id,occurrence,used FROM donna_chain_roots", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .unwrap();
    assert_eq!(before.3, 1);
    assert!(routines.state.locked().runs.is_empty());
    assert_eq!(admissao::orfas(&store).unwrap().len(), 1);
    assert!(routines.schedule_due(&store, 181_000).is_err());
    store.db.locked().execute_batch("DROP TRIGGER falhar_json").unwrap();
    let recovered = Routines::default();
    recovered.restore(&store).unwrap();
    recovered.schedule_due(&store, 181_000).unwrap();
    let state = recovered.state.locked();
    assert_eq!(state.runs.len(), 1);
    assert_eq!(state.runs[0].id, before.1);
    assert_eq!(state.runs[0].chain.as_ref().unwrap().chain_id, before.0);
    assert_eq!(state.routines[0].next_runs, vec![Some(240_000), None]);
    drop(state);
    let counts: (i64, i64) = store
        .db
        .locked()
        .query_row("SELECT COUNT(*),SUM(used) FROM donna_chain_roots", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!(counts, (1, 1));
    assert_eq!(admissao::orfas(&store).unwrap(), [] as [(String, String, String); 0]);
    assert!(recovered.list("bot")["runs"][0].get("_chain").is_none());
    assert!(recovered.list("bot")["routines"][0].get("scheduleGeneration").is_none());
}

#[test]
fn nova_retomada_gasta_tentativa_retry_sql_reusa_reserva_e_cas_nao_reenvia() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let routines = inicializar(&store);
    let first = uma_tentativa(&store, &routines);
    assert_eq!(routines.prepare_attempt(&store, "bot", &first.run_id).unwrap(), first);
    routines.mark_started(&store, "bot", &first.run_id, &first).unwrap();
    assert!(routines.mark_started(&store, "bot", &first.run_id, &first).is_err());
    let recovered = Routines::default();
    recovered.restore(&store).unwrap();
    let resumed = recovered.prepare_attempt(&store, "bot", &first.run_id).unwrap();
    assert_eq!(resumed.chain_id, first.chain_id);
    assert_eq!(resumed.ordinal, 2);
    assert_eq!(resumed.level, 1);
    assert_ne!(resumed.admission_id, first.admission_id);
    // Uma queda antes do CAS conserva a mesma reserva, inclusive depois de restore.
    let again = Routines::default();
    again.restore(&store).unwrap();
    assert_eq!(again.prepare_attempt(&store, "bot", &first.run_id).unwrap(), resumed);
    again.mark_started(&store, "bot", &first.run_id, &resumed).unwrap();
    assert_eq!(admissao::fase(&store, &resumed).unwrap(), Fase::EnvioPossivel);
}

#[test]
fn legado_normaliza_sem_orcamento_e_so_interrompe_ao_preparar_ia() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let routines = inicializar(&store);
    routines
        .change(&store, |state| {
            let mut run = crate::routines::new_run(&state.routines[0], Value::Null, None);
            run.status = Status::Running;
            run.started_at = Some(1);
            state.runs.push(run);
            Ok(())
        })
        .unwrap();
    for _ in 0..2 {
        let recovered = Routines::default();
        recovered.restore(&store).unwrap();
        assert_eq!(recovered.list("bot")["runs"][0]["status"], "recovering");
        let run = recovered.recovery("bot").unwrap();
        assert!(run.chain.is_none());
        assert!(recovered.guard_run_live("bot", &run.id));
        let roots: i64 =
            store.db.locked().query_row("SELECT COUNT(*) FROM donna_chain_roots", [], |r| r.get(0)).unwrap();
        assert_eq!(roots, 0);
    }
    let recovered = Routines::default();
    recovered.restore(&store).unwrap();
    let run = recovered.recovery("bot").unwrap();
    let before = recovered.list("bot");
    let raw = store.kv_read("routines.v1").unwrap();
    store
        .db
        .locked()
        .execute_batch(
            "CREATE TEMP TRIGGER recusar_interrupcao BEFORE INSERT ON kv WHEN NEW.k='routines.v1'
         BEGIN SELECT RAISE(ABORT,'falha ao persistir interrupcao'); END;",
        )
        .unwrap();
    let error = recovered.prepare_attempt(&store, "bot", &run.id).unwrap_err();
    assert!(error.downcast_ref::<PreparacaoNaoPersistida>().is_some());
    assert!(error.downcast_ref::<rusqlite::Error>().is_some());
    assert_eq!(recovered.list("bot"), before);
    assert_eq!(store.kv_read("routines.v1").unwrap(), raw);
    assert!(recovered.active.locked().contains("bot"));
    store.db.locked().execute_batch("DROP TRIGGER recusar_interrupcao").unwrap();
    assert!(recovered.prepare_attempt(&store, "bot", &run.id).is_err());
    assert_eq!(recovered.list("bot")["runs"][0]["status"], "interrupted");
    assert!(recovered.list("bot")["runs"][0]["detail"].as_str().unwrap().contains("tentativas anteriores"));
    assert!(recovered.recovery("bot").is_none());
    assert!(!recovered.guard_run_live("bot", &run.id));
    assert!(!recovered.active.locked().contains("bot"));
    let again = Routines::default();
    again.restore(&store).unwrap();
    assert_eq!(again.list("bot"), recovered.list("bot"));
    let error = again.prepare_attempt(&store, "bot", &run.id).unwrap_err();
    assert!(error.downcast_ref::<PreparacaoNaoPersistida>().is_none());
    let counts: (i64, i64) = store
        .db
        .locked()
        .query_row(
            "SELECT (SELECT COUNT(*) FROM donna_chain_roots),(SELECT COUNT(*) FROM donna_chain_admissions)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(counts, (0, 0));
}

#[test]
fn exclusao_com_cadeia_admitida_reverte_falha_e_cancela_sem_devolver_orcamento() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let cfg: crate::store::BotConfig = serde_json::from_value(json!({"id":"bot","name":"Bot de teste"})).unwrap();
    store.save_bot(&cfg).unwrap();
    let routines = inicializar(&store);
    let receipt = uma_tentativa(&store, &routines);
    routines.mark_started(&store, "bot", &receipt.run_id, &receipt).unwrap();
    routines.active.locked().insert("bot".into());
    store
        .db
        .locked()
        .execute(
            "INSERT INTO donna_guard_contexts(id,bot,connector,lane,run,data)
         VALUES('context-cadeia','bot','fixture','bot',?1,'{}')",
            [&receipt.run_id],
        )
        .unwrap();
    let before = routines.list("bot");
    let raw = store.kv_read("routines.v1").unwrap();
    let bot = store.bot("bot").unwrap().unwrap();
    store
        .db
        .locked()
        .execute_batch(
            "CREATE TEMP TRIGGER recusar_exclusao BEFORE DELETE ON donna_guard_contexts
         BEGIN SELECT RAISE(ABORT,'falha na limpeza transacional'); END;",
        )
        .unwrap();
    assert!(routines.delete_bot_committed(&store, "bot").is_err());
    assert_eq!(routines.list("bot"), before);
    assert_eq!(store.kv_read("routines.v1").unwrap(), raw);
    assert_eq!(store.bot("bot").unwrap().unwrap().rev, bot.rev);
    assert!(!store.bot("bot").unwrap().unwrap().deleted);
    assert!(routines.guard_run_live("bot", &receipt.run_id));
    assert!(routines.active.locked().contains("bot"));
    let contexts: i64 = store
        .db
        .locked()
        .query_row("SELECT COUNT(*) FROM donna_guard_contexts WHERE id='context-cadeia'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(contexts, 1);
    assert_eq!(admissao::fase(&store, &receipt).unwrap(), Fase::EnvioPossivel);
    store.db.locked().execute_batch("DROP TRIGGER recusar_exclusao").unwrap();
    routines.delete_bot_committed(&store, "bot").unwrap();
    assert!(store.bot("bot").unwrap().unwrap().deleted);
    assert_eq!(routines.list("bot")["runs"][0]["status"], "cancelled");
    assert!(!routines.guard_run_live("bot", &receipt.run_id));
    assert!(!routines.active.locked().contains("bot"));
    assert!(routines.prepare_attempt(&store, "bot", &receipt.run_id).is_err());
    assert!(routines.mark_started(&store, "bot", &receipt.run_id, &receipt).is_err());
    let durable: (i64, i64, i64, i64) = store
        .db
        .locked()
        .query_row(
            "SELECT used,linked,(SELECT COUNT(*) FROM donna_chain_admissions),
         (SELECT COUNT(*) FROM donna_guard_contexts) FROM donna_chain_roots WHERE id=?1",
            [&receipt.chain_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(durable, (1, 1, 1, 0));
    let restored = Routines::default();
    restored.restore(&store).unwrap();
    assert_eq!(restored.list("bot"), routines.list("bot"));
    assert!(restored.recovery("bot").is_none());
}

#[tokio::test]
async fn orfa_gera_um_aviso_visivel_sem_iniciar_run() {
    let dir = std::env::temp_dir().join(format!("donna-orfa-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir.join("test.db")).unwrap();
    let cfg: crate::store::BotConfig =
        serde_json::from_value(json!({"id":"bot","name":"Bot de teste","backend":"custom","cwd":dir})).unwrap();
    store.save_bot(&cfg).unwrap();
    let hub = Hub::new(
        store,
        "teste".into(),
        crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
        "token-ficticio".into(),
        19222,
    );
    hub.routines
        .change(&hub.store, |state| {
            state.routines.push(crate::routines::tests::routine());
            Ok(())
        })
        .unwrap();
    hub.store
        .db
        .locked()
        .execute_batch(
            "CREATE TRIGGER falhar_vinculo BEFORE INSERT ON kv WHEN NEW.k='routines.v1'
         BEGIN SELECT RAISE(ABORT,'falha real do segundo commit'); END;",
        )
        .unwrap();
    assert!(hub.routines.enqueue(&hub.store, "bot", "routine", json!({"source":"test"}), None, true).is_err());
    assert_eq!(admissao::orfas(&hub.store).unwrap().len(), 1);
    Routines::publish_chain_orphans(&hub).unwrap();
    Routines::publish_chain_orphans(&hub).unwrap();
    let history = hub.store.history("bot", i64::MAX, 100).unwrap();
    assert_eq!(history.iter().filter(|entry| entry.id.starts_with("routine-chain-orphan-")).count(), 1);
    assert_eq!(hub.routines.list("bot")["runs"].as_array().unwrap().as_slice(), &[] as &[Value]);
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn delivery_antiga_fora_do_json_nao_recria_rotina_cancelada() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let routines = inicializar(&store);
    routines.enqueue(&store, "bot", "routine", Value::Null, Some("evento-original".into()), true).unwrap();
    routines.cancel_queued(&store, "bot").unwrap();
    routines
        .change(&store, |state| {
            state.runs.clear();
            Ok(())
        })
        .unwrap();
    assert!(routines.enqueue(&store, "bot", "routine", Value::Null, Some("evento-original".into()), true).is_err());
    let used: i64 =
        store.db.locked().query_row("SELECT SUM(used) FROM donna_chain_roots", [], |row| row.get(0)).unwrap();
    assert_eq!(used, 1);
    assert!(routines.state.locked().runs.is_empty());
}

#[tokio::test]
async fn retomada_em_space_proprio_herda_contexto_persistido_da_rotina() {
    let dir = std::env::temp_dir().join(format!("donna-retomada-space-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let hub = Hub::new(
        Store::open(&dir.join("test.db")).unwrap(),
        "teste-space".into(),
        crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
        "token-ficticio".into(),
        0,
    );
    let caller = crate::api::devices::Caller::Local;
    let initial = crate::api::dispatch(&hub, &caller, "donnaSpaces", json!({"hubId":hub.host_id})).await.unwrap();
    let spaces = crate::api::dispatch(
        &hub,
        &caller,
        "donnaSpaceCreate",
        json!({"hubId":hub.host_id,"expectedRevision":initial["revision"],"name":"Rotina de teste",
            "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"RT"}}}),
    )
    .await
    .unwrap();
    let space = spaces["spaces"][1]["id"].as_str().unwrap();
    let cfg: crate::store::BotConfig =
        serde_json::from_value(json!({"id":"bot","name":"Bot de teste","backend":"custom","cwd":dir,"spaceId":space}))
            .unwrap();
    hub.store.save_bot(&cfg).unwrap();
    let routines = inicializar(&hub.store);
    let first = uma_tentativa(&hub.store, &routines);
    routines.mark_started(&hub.store, "bot", &first.run_id, &first).unwrap();
    let expected = Escopo::ConversaDoBot { bot_id: "bot".into(), space_id: Some(space.into()) };
    assert_eq!(admissao::contexto(&hub.store, &first).unwrap().escopo, expected);

    // Percurso real de restore/prepare_attempt; nenhum pedido de continuação montado pelo teste.
    let restored = Routines::default();
    restored.restore(&hub.store).unwrap();
    let resumed = restored.prepare_attempt(&hub.store, "bot", &first.run_id).unwrap();
    let context = admissao::contexto(&hub.store, &resumed).unwrap();
    assert_eq!(context.escopo, expected);
    assert_eq!(context.rotina.as_deref(), Some("routine"));
    assert_eq!(resumed.chain_id, first.chain_id);
    assert_eq!(resumed.ordinal, 2);
    let consistent: i64 = hub
        .store
        .db
        .locked()
        .query_row(
            "SELECT COUNT(*) FROM donna_chain_admissions a JOIN donna_chain_roots root ON root.id=a.chain_id
         WHERE a.chain_id=?1 AND a.scope=root.scope AND a.bot_id=root.bot_id
           AND a.run_id=root.run_id AND a.routine_id IS root.routine_id",
            [&first.chain_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(consistent, 2);
    assert_eq!(restored.prepare_attempt(&hub.store, "bot", &first.run_id).unwrap(), resumed);
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn publisher_adia_na_janela_manual_e_nao_publica_orfa_apos_vinculo() {
    let dir = std::env::temp_dir().join(format!("donna-janela-manual-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir.join("test.db")).unwrap();
    let cfg: crate::store::BotConfig =
        serde_json::from_value(json!({"id":"bot","name":"Bot de teste","backend":"custom","cwd":dir})).unwrap();
    store.save_bot(&cfg).unwrap();
    let hub = Hub::new(
        store,
        "teste-janela".into(),
        crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
        "token-ficticio".into(),
        0,
    );
    hub.routines
        .change(&hub.store, |state| {
            let routine = crate::routines::tests::routine();
            let run = manual(&hub.store, &routine, json!({"source":"test"}), None)?;
            let chain = &run.chain.as_ref().unwrap().chain_id;
            assert!(admissao::orfa_pendente(&hub.store, chain).unwrap());
            // Mesmo ponto de interleaving do tick: commit 1 salvo, commit 2 ainda não aconteceu.
            // try_lock deve adiar sem reentrar, publicar ou esperar outra thread.
            Routines::publish_chain_orphans(&hub)?;
            let history = hub.store.history("bot", i64::MAX, 100).unwrap();
            assert_eq!(history.iter().filter(|entry| entry.id.starts_with("routine-chain-orphan-")).count(), 0);
            state.routines.push(routine);
            state.runs.push(run);
            Ok(())
        })
        .unwrap();
    Routines::publish_chain_orphans(&hub).unwrap();
    let history = hub.store.history("bot", i64::MAX, 100).unwrap();
    assert_eq!(history.iter().filter(|entry| entry.id.starts_with("routine-chain-orphan-")).count(), 0);
    assert_eq!(admissao::orfas(&hub.store).unwrap(), [] as [(String, String, String); 0]);
    assert_eq!(hub.routines.list("bot")["runs"][0]["status"], "pending");

    let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _state = hub.routines.state.lock().unwrap();
        panic!("poison sintético do estado de Rotinas");
    }));
    assert!(poisoned.is_err());
    assert!(Routines::publish_chain_orphans(&hub).is_err());
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn negativa_legada_revalida_chain_dentro_do_produtor_sem_contar_apos_consulta_antiga() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let routines = inicializar(&store);
    let receipt = uma_tentativa(&store, &routines);
    routines
        .change(&store, |state| {
            let run = state.runs.iter_mut().find(|run| run.id == receipt.run_id).unwrap();
            run.status = Status::Recovering;
            run.chain = None;
            Ok(())
        })
        .unwrap();
    assert!(routines.recovery_without_receipt("bot", &receipt.run_id));
    // A consulta externa envelhece antes do callback: o recibo original voltou.
    routines
        .change(&store, |state| {
            state.runs.iter_mut().find(|run| run.id == receipt.run_id).unwrap().chain = Some(receipt.clone());
            Ok(())
        })
        .unwrap();
    let raw = store.kv_read("routines.v1").unwrap();
    let error = routines.reject_legacy_recovery(&store, "bot", &receipt.run_id).unwrap_err();
    assert!(error.downcast_ref::<Recusa>().is_some());
    assert!(error.downcast_ref::<PreparacaoNaoPersistida>().is_none());
    assert_eq!(store.kv_read("routines.v1").unwrap(), raw);
    assert_eq!(admissao::fase(&store, &receipt).unwrap(), Fase::Reservada);
    let counts: (i64, i64, i64) = store
        .db
        .locked()
        .query_row(
            "SELECT COUNT(*),SUM(used),(SELECT COUNT(*) FROM donna_chain_admissions) FROM donna_chain_roots",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(counts, (1, 1, 1), "a negativa não cria recibo nem consome tentativa");
}

/// Banco segurado por outra tarefa do Hub naquele instante (o `runRoutine: core_StoreBusy` do
/// `harness_e2e`). Sem espera, a chamada manual falha; com `retry_store_busy`, como a API faz, ela
/// passa assim que o banco é solto e cria uma única execução com uma única admissão.
#[tokio::test]
async fn run_manual_espera_banco_ocupado_e_admite_uma_vez() {
    use crate::fork::atividades::store::{is_store_busy, retry_store_busy};
    let store = std::sync::Arc::new(Store::open(Path::new(":memory:")).unwrap());
    let routines = inicializar(&store);
    let hold = |ms: u64| {
        let (store, (tx, rx)) = (store.clone(), std::sync::mpsc::channel());
        let holder = std::thread::spawn(move || {
            let guard = store.db.locked();
            tx.send(()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(ms));
            drop(guard);
        });
        rx.recv().unwrap();
        holder
    };
    // RED (comportamento sem espera): o try_lock recusa na hora.
    let holder = hold(150);
    let busy = routines.enqueue(&store, "bot", "routine", json!({"source":"test"}), None, true).unwrap_err();
    assert!(is_store_busy(&busy), "{busy:#}");
    holder.join().unwrap();
    routines
        .change(&store, |state| {
            assert!(state.runs.is_empty());
            Ok(())
        })
        .unwrap();
    // GREEN: com a espera da API, passa depois que o banco é solto.
    let holder = hold(150);
    let value = retry_store_busy(|| routines.enqueue(&store, "bot", "routine", json!({"source":"test"}), None, true))
        .await
        .unwrap();
    holder.join().unwrap();
    assert!(value["run"]["id"].is_string());
    routines
        .change(&store, |state| {
            assert_eq!(state.runs.len(), 1);
            Ok(())
        })
        .unwrap();
    let admissions: i64 =
        store.db.locked().query_row("SELECT COUNT(*) FROM donna_chain_admissions", [], |row| row.get(0)).unwrap();
    assert_eq!(admissions, 1, "a tentativa recusada não deixou outra admissão");
}

/// A espera tem prazo: um detentor que não solta continua recusado com o mesmo `StoreBusy`, e
/// um erro que não é de contenção volta na primeira tentativa.
#[tokio::test]
async fn espera_do_banco_ocupado_tem_prazo_e_nao_repete_outros_erros() {
    use crate::fork::atividades::store::{is_store_busy, retry_store_busy};
    let store = std::sync::Arc::new(Store::open(Path::new(":memory:")).unwrap());
    let routines = inicializar(&store);
    // Detentor em outra thread, por mais tempo que o prazo da espera (~2 s).
    let (held, (tx, rx)) = (store.clone(), std::sync::mpsc::channel());
    let holder = std::thread::spawn(move || {
        let guard = held.db.locked();
        tx.send(()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2_500));
        drop(guard);
    });
    rx.recv().unwrap();
    let started = std::time::Instant::now();
    let busy = retry_store_busy(|| routines.enqueue(&store, "bot", "routine", json!({"source":"test"}), None, true))
        .await
        .unwrap_err();
    assert!(is_store_busy(&busy), "{busy:#}");
    assert!(started.elapsed() >= std::time::Duration::from_secs(1), "esperou o prazo antes de recusar");
    holder.join().unwrap();
    let mut calls = 0;
    let other = retry_store_busy(|| {
        calls += 1;
        routines.enqueue(&store, "bot", "outra", json!({"source":"test"}), None, true)
    })
    .await
    .unwrap_err();
    assert_eq!(calls, 1);
    assert!(!is_store_busy(&other), "{other:#}");
}

/// Exercita o adaptador real (estado/JSON/reserva), sem processo de IA.
fn descendente(store: &Store) -> (Routines, Recibo) {
    store.save_bot(&serde_json::from_value(json!({"id":"bot","name":"Fixture"})).unwrap()).unwrap();
    let routines = inicializar(store);
    let parent = uma_tentativa(store, &routines);
    routines.mark_started(store, "bot", &parent.run_id, &parent).unwrap();
    routines
        .change(store, |state| {
            state.routines[0].enabled = false;
            let mut child = crate::routines::tests::routine();
            child.id = "child".into();
            child.creation_origin = Some(crate::fork::cadeias::descendencia_rotina::fixture_origin(
                parent.clone(),
                crate::fork::spaces::PERSONAL,
            ));
            state.routines.push(child);
            Ok(())
        })
        .unwrap();
    (routines, parent)
}
fn saldo(store: &Store, parent: &Recibo) -> i64 {
    store
        .db
        .locked()
        .query_row("SELECT used FROM donna_chain_roots WHERE id=?1", [&parent.chain_id], |r| r.get(0))
        .unwrap()
}

#[test]
fn descendente_agenda_reverte_reserva_avanco_run_e_link_se_json_falha() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let (routines, parent) = descendente(&store);
    let before = store.kv_read("routines.v1").unwrap();
    let state = routines.list("bot");
    store.db.locked().execute_batch("CREATE TRIGGER falhar_descendente BEFORE INSERT ON kv WHEN NEW.k='routines.v1' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    assert!(routines.schedule_due(&store, 61_000).is_err());
    assert_eq!(store.kv_read("routines.v1").unwrap(), before);
    assert_eq!(routines.list("bot"), state);
    assert_eq!(saldo(&store, &parent), 1);
    let admissions: i64 =
        store.db.locked().query_row("SELECT COUNT(*) FROM donna_chain_admissions", [], |r| r.get(0)).unwrap();
    assert_eq!(admissions, 1);
    store.db.locked().execute_batch("DROP TRIGGER falhar_descendente").unwrap();
    let recovered = Routines::default();
    recovered.restore(&store).unwrap();
    recovered.schedule_due(&store, 61_000).unwrap();
    let guard = recovered.state.locked();
    let run = guard.runs.iter().find(|r| r.routine_id == "child").unwrap();
    let receipt = run.chain.as_ref().unwrap();
    assert_eq!(receipt.chain_id, parent.chain_id);
    assert_eq!(receipt.level, 2);
    assert_eq!(guard.routines.iter().find(|r| r.id == "child").unwrap().next_runs, vec![Some(120_000)]);
    assert!(crate::fork::cadeias::store::vinculada(&store.db.locked(), receipt).unwrap());
    assert_eq!(saldo(&store, &parent), 2);
}

#[test]
fn descendente_manual_retomada_apos_reinicio_e_edicao_nao_renovam_cadeia() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let (routines, parent) = descendente(&store);
    let origin = routines.state.locked().routines.iter().find(|r| r.id == "child").unwrap().creation_origin.clone();
    routines.change(&store,|state|super::super::save_in(state,"bot",&json!({"id":"child","name":"Editada","instruction":"fixture","triggers":[{"type":"interval","seconds":120}]}))).unwrap();
    assert_eq!(routines.state.locked().routines.iter().find(|r| r.id == "child").unwrap().creation_origin, origin);
    let result =
        routines.enqueue(&store, "bot", "child", json!({"source":"test"}), Some("manual-1".into()), true).unwrap();
    let id = result["run"]["id"].as_str().unwrap();
    routines
        .change(&store, |state| {
            state.runs.iter_mut().find(|r| r.id == id).unwrap().status = Status::Starting;
            Ok(())
        })
        .unwrap();
    let receipt = routines.prepare_attempt(&store, "bot", id).unwrap();
    assert_eq!((receipt.level, receipt.ordinal), (2, 2));
    routines.mark_started(&store, "bot", id, &receipt).unwrap();
    let restored = Routines::default();
    restored.restore(&store).unwrap();
    let resumed = restored.prepare_attempt(&store, "bot", id).unwrap();
    assert_eq!((resumed.level, resumed.ordinal), (2, 3));
    assert_eq!(resumed.chain_id, parent.chain_id);
    assert_eq!(restored.prepare_attempt(&store, "bot", id).unwrap(), resumed);
    restored.mark_started(&store, "bot", id, &resumed).unwrap();
    assert!(restored.mark_started(&store, "bot", id, &resumed).is_err());
    assert_eq!(saldo(&store, &parent), 3);
}

#[test]
fn descendente_manual_replay_apos_retencao_nao_recria_run_nem_debito() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let (routines, parent) = descendente(&store);
    let first = routines.enqueue(&store, "bot", "child", json!({"source":"test"}), Some("old".into()), true).unwrap();
    let id = first["run"]["id"].as_str().unwrap();
    routines
        .change(&store, |state| {
            state.runs.retain(|r| r.id != id);
            Ok(())
        })
        .unwrap();
    assert!(routines.enqueue(&store, "bot", "child", json!({"source":"test"}), Some("old".into()), true).is_err());
    assert!(!routines.state.locked().runs.iter().any(|r| r.routine_id == "child"));
    assert_eq!(saldo(&store, &parent), 2);
}

#[test]
fn recusa_descendente_e_terminal_visivel_sem_tick_infinito_ou_nova_raiz() {
    for corrupt in [false, true] {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let (routines, parent) = descendente(&store);
        if corrupt {
            routines
                .change(&store, |state| {
                    let child = state.routines.iter_mut().find(|r| r.id == "child").unwrap();
                    let mut raw = serde_json::to_value(child.creation_origin.as_ref().unwrap()).unwrap();
                    raw["version"] = json!(2);
                    child.creation_origin = Some(serde_json::from_value(raw).unwrap());
                    Ok(())
                })
                .unwrap();
        } else {
            store.db.locked().execute("UPDATE donna_chain_roots SET used=50 WHERE id=?1", [&parent.chain_id]).unwrap();
        }
        let before = saldo(&store, &parent);
        routines.schedule_due(&store, 61_000).unwrap();
        routines.schedule_due(&store, 181_000).unwrap();
        let guard = routines.state.locked();
        let runs: Vec<_> = guard.runs.iter().filter(|r| r.routine_id == "child").collect();
        assert_eq!(runs.len(), 1);
        assert!(runs[0].status == Status::Interrupted);
        assert!(runs[0].report_pending);
        assert!(runs[0].detail.is_some());
        assert!(runs[0].chain.is_none());
        assert!(!guard.routines.iter().find(|r| r.id == "child").unwrap().enabled);
        assert_eq!(saldo(&store, &parent), before);
        let roots: i64 =
            store.db.locked().query_row("SELECT COUNT(*) FROM donna_chain_roots", [], |r| r.get(0)).unwrap();
        assert_eq!(roots, 1);
    }
}

#[test]
fn restauracao_de_descendente_sem_recibo_nao_fabrica_raiz_legada() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let (routines, parent) = descendente(&store);
    routines
        .change(&store, |state| {
            let child = state.routines.iter().find(|r| r.id == "child").unwrap();
            state.runs.push(super::super::new_run(child, Value::Null, None));
            Ok(())
        })
        .unwrap();
    let restored = Routines::default();
    restored.restore(&store).unwrap();
    let state = restored.state.locked();
    let run = state.runs.iter().find(|r| r.routine_id == "child").unwrap();
    assert!(run.status == Status::Interrupted && run.report_pending && run.chain.is_none());
    assert_eq!(saldo(&store, &parent), 1);
    let roots: i64 = store.db.locked().query_row("SELECT COUNT(*) FROM donna_chain_roots", [], |r| r.get(0)).unwrap();
    assert_eq!(roots, 1);
}

/// MEX-1434: o tick da agenda, os limites de uso e as conversas seguram a conexão por instantes. A lista
/// de rotinas órfãs (`orfas`) recusava na hora (`core_StoreBusy`, ~6 avisos por minuto em produção);
/// agora espera a disputa curta e continua recusando, sem espera ilimitada, se a conexão ficar presa.
/// As demais leituras da admissão seguem recusando na hora: quem precisa de espera repete a operação
/// inteira fora das travas (`retry_store_busy`).
#[test]
fn orfas_espera_disputa_curta_e_as_demais_leituras_recusam_na_hora() {
    use crate::fork::atividades::store::is_store_busy;
    let store = std::sync::Arc::new(Store::open(Path::new(":memory:")).unwrap());
    let routines = inicializar(&store);
    let receipt = uma_tentativa(&store, &routines);
    let hold = |ms: u64| {
        let (store, (tx, rx)) = (store.clone(), std::sync::mpsc::channel());
        let holder = std::thread::spawn(move || {
            let guard = store.db.locked();
            tx.send(()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(ms));
            drop(guard);
        });
        rx.recv().unwrap();
        holder
    };
    let holder = hold(60);
    admissao::orfas(&store).unwrap();
    holder.join().unwrap();

    let holder = hold(800);
    let imediatas = std::time::Instant::now();
    for resultado in [
        admissao::orfa_pendente(&store, &receipt.chain_id).map(drop),
        admissao::manual_pendente(&store, "routine").map(drop),
        admissao::vinculada(&store, &receipt).map(drop),
        admissao::fase(&store, &receipt).map(drop),
        admissao::contexto(&store, &receipt).map(drop),
    ] {
        assert!(is_store_busy(&resultado.unwrap_err()));
    }
    assert!(imediatas.elapsed() < std::time::Duration::from_millis(100), "só orfas espera");
    let espera = std::time::Instant::now();
    assert!(is_store_busy(&admissao::orfas(&store).unwrap_err()));
    let esperou = espera.elapsed();
    assert!(
        esperou >= std::time::Duration::from_millis(150) && esperou < std::time::Duration::from_millis(700),
        "espera limitada pelo prazo, não pelo detentor: {esperou:?}"
    );
    holder.join().unwrap();
}

/// A espera por `orfas` acontece antes de qualquer trava de Rotinas: com o banco preso, o estado das
/// rotinas segue livre para quem prepara envios e publica hooks.
#[test]
fn publicar_orfas_nao_segura_o_estado_das_rotinas_enquanto_espera_o_banco() {
    use crate::fork::atividades::store::is_store_busy;
    let dir = std::env::temp_dir().join(format!("donna-orfas-espera-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir.join("test.db")).unwrap();
    let hub = Hub::new(
        store,
        "teste-orfas".into(),
        crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
        "token-ficticio".into(),
        0,
    );
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        let holder = scope.spawn(|| {
            let guard = hub.store.db.locked();
            tx.send(()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(500));
            drop(guard);
        });
        rx.recv().unwrap();
        let publicar = scope.spawn(|| Routines::publish_chain_orphans(&hub));
        std::thread::sleep(std::time::Duration::from_millis(80));
        assert!(hub.routines.state.try_lock().is_ok(), "esperando o banco, o estado das rotinas fica livre");
        assert!(is_store_busy(&publicar.join().unwrap().unwrap_err()));
        holder.join().unwrap();
    });
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}

/// O prazo da API é um corte: depois dele nenhuma tentativa nova começa e volta o último `StoreBusy`.
#[tokio::test]
async fn retry_store_busy_nao_comeca_tentativa_depois_do_prazo() {
    use crate::fork::atividades::{
        modelo::CoreError,
        store::{is_store_busy, retry_store_busy},
    };
    let started = std::time::Instant::now();
    let mut starts = Vec::new();
    let busy = retry_store_busy(|| -> Result<()> {
        starts.push(started.elapsed());
        Err(CoreError::StoreBusy.into())
    })
    .await
    .unwrap_err();
    assert!(is_store_busy(&busy), "{busy:#}");
    assert!(starts.len() > 2, "repetiu durante o prazo");
    assert!(
        starts.iter().all(|at| *at < std::time::Duration::from_secs(2)),
        "começou tentativa após o prazo: {starts:?}"
    );
    assert!(started.elapsed() < std::time::Duration::from_millis(2_300));
}
