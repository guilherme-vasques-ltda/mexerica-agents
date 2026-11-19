//! D70: tentativas inteiras, guardas livres e plaintext fora da espera.
#![allow(clippy::unwrap_used)]
use super::*;
use crate::fork::{
    atividades::modelo::CoreError,
    conectores::whatsapp_human::{self, WhatsappHumanRequest, retry_probe},
    sessoes::ExecutionRef,
    whatsapp::{draft_tests::ready, number_tests::Fixture},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{TryLockError, mpsc},
    thread::JoinHandle,
};

// Só esta thread de fixture segura DB. Drop sempre manda soltar e faz join.
struct HeldDb {
    release: Option<mpsc::Sender<()>>,
    worker: Option<JoinHandle<()>>,
}
impl HeldDb {
    fn new(hub: Arc<Hub>) -> Self {
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _db = hub.store.db.locked();
            let _ = ready_tx.send(());
            let _ = release_rx.recv();
        });
        ready_rx.recv().unwrap();
        Self { release: Some(release_tx), worker: Some(worker) }
    }
}
impl Drop for HeldDb {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn draft(f: &Fixture, execution: &ExecutionRef) -> String {
    let receipt = whatsapp_human::prepare_whatsapp_human(
        &f.hub,
        execution,
        WhatsappHumanRequest {
            chat_id: "5511988887777@s.whatsapp.net".into(),
            message_id: "M1".into(),
            text: "Resposta de fixture".into(),
        },
    )
    .unwrap();
    serde_json::to_value(receipt).unwrap()["draftId"].as_str().unwrap().into()
}

// O tempo da fixture de conexão já terminou. Pausa antes de criar o timer;
// espera a inspeção inicial REAL, sem usar yield como barreira de escalonamento.
async fn draft_after_initial_inspection(
    f: &Fixture,
    execution: &ExecutionRef,
) -> (String, retry_probe::TimerWatchGuard) {
    tokio::time::pause();
    let id = draft(f, execution);
    let (watch, inspected, _stopped) = retry_probe::watch_timer(&id);
    inspected.await.expect("o primeiro tick deve inspecionar este rascunho com sucesso");
    (id, watch)
}

fn assert_send<T: Send>(value: T) -> T {
    value
}

fn free_between_attempts(hub: &Hub) {
    retry_probe::assert_auxiliary_unlocked(hub);
    assert!(hub.store.db.try_lock().is_ok());
    assert!(hub.whatsapp.0.bots.try_lock().is_ok());
    assert!(!retry_probe::plain_live());
}

#[tokio::test]
async fn busy_na_primeira_livre_na_segunda_fecha_com_guardas_livres() {
    let (f, execution) = ready().await;
    let (id, _timer_watch) = draft_after_initial_inspection(&f, &execution).await;
    let held = Rc::new(RefCell::new(Some(HeldDb::new(f.hub.clone()))));
    let waits = Rc::new(Cell::new(0));
    let hook_held = held.clone();
    let hook_waits = waits.clone();
    let hub = f.hub.clone();
    let probe = retry_probe::install(move || {
        drop(hook_held.borrow_mut().take());
        free_between_attempts(&hub);
        hook_waits.set(hook_waits.get() + 1);
    });
    assert_send(whatsapp_human::close(&f.hub, &id)).await.unwrap();
    tokio::time::resume();
    drop(probe);
    assert_eq!(waits.get(), 1);
    assert_eq!(f.hub.store.kv_read(&format!("donna.whatsapp.draft.private.{id}")).unwrap(), None);
    assert_send(whatsapp_human::read(&f.hub, &id)).await.unwrap();
    f.done().await;
}

#[tokio::test]
async fn busy_persistente_termina_em_tres_sem_persistir_fechamento() {
    let (f, execution) = ready().await;
    let (id, _timer_watch) = draft_after_initial_inspection(&f, &execution).await;
    let private = format!("donna.whatsapp.draft.private.{id}");
    let receipt = format!("donna.whatsapp.draft.receipt.{id}");
    let before_private = f.hub.store.kv_read(&private).unwrap();
    let before_receipt = f.hub.store.kv_read(&receipt).unwrap();
    let held = HeldDb::new(f.hub.clone());
    let waits = Rc::new(Cell::new(0));
    let hook_waits = waits.clone();
    let hub = f.hub.clone();
    let probe = retry_probe::install(move || {
        retry_probe::assert_auxiliary_unlocked(&hub);
        assert!(hub.whatsapp.0.bots.try_lock().is_ok());
        // Só o escritor de fixture segura DB; esta tentativa já retornou.
        assert!(matches!(hub.store.db.try_lock(), Err(TryLockError::WouldBlock)));
        assert!(!retry_probe::plain_live());
        hook_waits.set(hook_waits.get() + 1);
    });
    let error = assert_send(whatsapp_human::close(&f.hub, &id)).await.unwrap_err();
    tokio::time::resume();
    drop(probe);
    drop(held);
    assert_eq!(error.downcast_ref::<CoreError>(), Some(&CoreError::StoreBusy));
    assert_eq!(waits.get(), 2); // três tentativas totais, só duas esperas.
    assert_eq!(f.hub.store.kv_read(&private).unwrap(), before_private);
    assert_eq!(f.hub.store.kv_read(&receipt).unwrap(), before_receipt);
    f.done().await;
}

#[tokio::test]
async fn segunda_conferencia_busy_preserva_rascunho_sem_close() {
    let (f, execution) = ready().await;
    let (id, _timer_watch) = draft_after_initial_inspection(&f, &execution).await;
    let private = format!("donna.whatsapp.draft.private.{id}");
    let receipt = format!("donna.whatsapp.draft.receipt.{id}");
    let before_private = f.hub.store.kv_read(&private).unwrap();
    let before_receipt = f.hub.store.kv_read(&receipt).unwrap();
    let before_entry = serde_json::to_value(f.hub.store.entry(&id).unwrap()).unwrap();
    let held = Rc::new(RefCell::new(None));
    let hook_held = held.clone();
    let hub = f.hub.clone();
    let opened = retry_probe::install_after_open(move || {
        assert!(retry_probe::plain_live());
        assert!(hub.store.secret_key.try_lock().is_ok());
        *hook_held.borrow_mut() = Some(HeldDb::new(hub.clone()));
    });
    let closes = Rc::new(Cell::new(0));
    let hook_closes = closes.clone();
    let before_close = retry_probe::install_before_close(move || {
        assert!(!retry_probe::plain_live());
        hook_closes.set(hook_closes.get() + 1);
    });
    let waits = Rc::new(Cell::new(0));
    let hook_waits = waits.clone();
    let probe = retry_probe::install(move || {
        assert!(!retry_probe::plain_live());
        hook_waits.set(hook_waits.get() + 1);
    });
    // DB continua ocupado até read retornar: read_result bloquearia este teste.
    let error = assert_send(whatsapp_human::read(&f.hub, &id)).await.unwrap_err();
    assert_eq!(error.downcast_ref::<CoreError>(), Some(&CoreError::StoreBusy));
    assert!(!retry_probe::plain_live());
    assert_eq!(closes.get(), 0);
    assert_eq!(waits.get(), 0);
    assert!(retry_probe::has_draft(&f.hub, &id));
    drop(held.borrow_mut().take());
    free_between_attempts(&f.hub);
    assert_eq!(f.hub.store.kv_read(&private).unwrap(), before_private);
    assert_eq!(f.hub.store.kv_read(&receipt).unwrap(), before_receipt);
    assert_eq!(serde_json::to_value(f.hub.store.entry(&id).unwrap()).unwrap(), before_entry);
    let current: serde_json::Value = serde_json::from_str(&before_receipt.unwrap()).unwrap();
    assert_eq!(current["state"], "draft");
    drop(probe);
    drop(before_close);
    drop(opened);
    let value = assert_send(whatsapp_human::read(&f.hub, &id)).await.unwrap();
    assert!(value["presentation"].is_object());
    assert_eq!(value["receipt"], current);
    assert!(retry_probe::has_draft(&f.hub, &id));
    assert_eq!(f.hub.store.kv_read(&private).unwrap(), before_private);
    tokio::time::resume();
    f.done().await;
}

#[tokio::test]
async fn segunda_conferencia_expirada_descarta_plain_antes_do_close_com_retry() {
    let (f, execution) = ready().await;
    let (id, _timer_watch) = draft_after_initial_inspection(&f, &execution).await;
    let private = format!("donna.whatsapp.draft.private.{id}");
    let receipt = format!("donna.whatsapp.draft.receipt.{id}");
    let receipt_after_open = receipt.clone();
    let hub = f.hub.clone();
    let opened = retry_probe::install_after_open(move || {
        assert!(retry_probe::plain_live());
        assert!(hub.store.secret_key.try_lock().is_ok());
        // Invalida o prazo REAL do recibo na fixture, sem ocupar DB no recheck.
        hub.store
            .db
            .locked()
            .execute("UPDATE kv SET v=json_set(v,'$.deadline',0) WHERE k=?1", [&receipt_after_open])
            .unwrap();
    });
    let held = Rc::new(RefCell::new(None));
    let hook_held = held.clone();
    let closes = Rc::new(Cell::new(0));
    let hook_closes = closes.clone();
    let hub = f.hub.clone();
    let before_close = retry_probe::install_before_close(move || {
        // Este gancho é APÓS o frame de open_current: Zeroizing já caiu.
        assert!(!retry_probe::plain_live());
        free_between_attempts(&hub);
        *hook_held.borrow_mut() = Some(HeldDb::new(hub.clone()));
        hook_closes.set(hook_closes.get() + 1);
    });
    let waits = Rc::new(Cell::new(0));
    let hook_waits = waits.clone();
    let hub = f.hub.clone();
    let probe = retry_probe::install(move || {
        assert!(!retry_probe::plain_live());
        // Só agora solta o escritor externo e confere todas as guardas livres.
        drop(held.borrow_mut().take());
        free_between_attempts(&hub);
        hook_waits.set(hook_waits.get() + 1);
    });
    let value = assert_send(whatsapp_human::read(&f.hub, &id)).await.unwrap();
    assert_eq!(closes.get(), 1);
    assert_eq!(waits.get(), 1);
    assert!(!retry_probe::plain_live());
    assert!(value["presentation"].is_null());
    assert_eq!(value["receipt"]["state"], "closed");
    assert_eq!(value["receipt"]["deadline"], 0);
    assert_eq!(f.hub.store.kv_read(&private).unwrap(), None);
    assert!(!retry_probe::has_draft(&f.hub, &id));
    free_between_attempts(&f.hub);
    drop(probe);
    drop(before_close);
    drop(opened);
    tokio::time::resume();
    f.done().await;
}

#[tokio::test]
async fn cancelar_entre_tentativas_nao_fecha_parcial_e_proxima_tentativa_converge() {
    let (f, execution) = ready().await;
    let (id, _timer_watch) = draft_after_initial_inspection(&f, &execution).await;
    let private = format!("donna.whatsapp.draft.private.{id}");
    let receipt = format!("donna.whatsapp.draft.receipt.{id}");
    let before_private = f.hub.store.kv_read(&private).unwrap();
    let before_receipt = f.hub.store.kv_read(&receipt).unwrap();
    let held = HeldDb::new(f.hub.clone());
    {
        let operation = assert_send(whatsapp_human::close(&f.hub, &id));
        tokio::pin!(operation);
        assert!(futures::poll!(operation.as_mut()).is_pending());
        // Drop da future pendente: não existe Tx ou guarda da tentativa nela.
    }
    drop(held);
    tokio::time::resume();
    assert_eq!(f.hub.store.kv_read(&private).unwrap(), before_private);
    assert_eq!(f.hub.store.kv_read(&receipt).unwrap(), before_receipt);
    free_between_attempts(&f.hub);
    whatsapp_human::close(&f.hub, &id).await.unwrap();
    assert_eq!(f.hub.store.kv_read(&private).unwrap(), None);
    f.done().await;
}

#[tokio::test]
async fn erro_sql_ou_commit_nao_dispara_retry() {
    let (f, execution) = ready().await;
    let (id, _timer_watch) = draft_after_initial_inspection(&f, &execution).await;
    let private = format!("donna.whatsapp.draft.private.{id}");
    let before = f.hub.store.kv_read(&private).unwrap();
    let probe = retry_probe::install(|| panic!("erro SQL/commit não é StoreBusy"));
    f.hub
        .store
        .db
        .locked()
        .execute_batch(
            "CREATE TEMP TRIGGER retry_sql_fault BEFORE UPDATE ON kv
             WHEN OLD.k LIKE 'donna.whatsapp.draft.receipt.%'
             BEGIN SELECT RAISE(ABORT,'fixture'); END;",
        )
        .unwrap();
    let error = assert_send(whatsapp_human::close(&f.hub, &id)).await.unwrap_err();
    assert_eq!(error.downcast_ref::<CoreError>(), Some(&CoreError::Storage));
    assert_eq!(f.hub.store.kv_read(&private).unwrap(), before);
    f.hub
        .store
        .db
        .locked()
        .execute_batch(
            "PRAGMA foreign_keys=ON;
             DROP TRIGGER retry_sql_fault;
             CREATE TEMP TABLE retry_parent(id INTEGER PRIMARY KEY);
             CREATE TEMP TABLE retry_child(id INTEGER REFERENCES retry_parent(id) DEFERRABLE INITIALLY DEFERRED);
             CREATE TEMP TRIGGER retry_commit_fault AFTER UPDATE ON kv
             WHEN OLD.k LIKE 'donna.whatsapp.draft.receipt.%'
             BEGIN INSERT INTO retry_child(id) VALUES(1); END;",
        )
        .unwrap();
    let error = assert_send(whatsapp_human::close(&f.hub, &id)).await.unwrap_err();
    assert_eq!(error.downcast_ref::<CoreError>(), Some(&CoreError::Storage));
    assert_eq!(f.hub.store.kv_read(&private).unwrap(), before);
    drop(probe);
    f.hub.store.db.locked().execute_batch("DROP TRIGGER retry_commit_fault").unwrap();
    free_between_attempts(&f.hub);
    tokio::time::resume();
    f.done().await;
}

#[tokio::test]
async fn timer_busy_final_preserva_mapa_para_leitura_e_nao_repete_a_cada_segundo() {
    let (f, execution) = ready().await;
    tokio::time::pause();
    let id = draft(&f, &execution);
    let (_timer_watch, _inspected, stopped) = retry_probe::watch_timer(&id);
    let private = format!("donna.whatsapp.draft.private.{id}");
    let receipt = format!("donna.whatsapp.draft.receipt.{id}");
    // Primeira inspect do timer recusa o prazo; DB só fica ocupado depois dela.
    f.hub.store.db.locked().execute("UPDATE kv SET v=json_set(v,'$.deadline',0) WHERE k=?1", [&receipt]).unwrap();
    let before_private = f.hub.store.kv_read(&private).unwrap();
    let before_receipt = f.hub.store.kv_read(&receipt).unwrap();
    let held = Rc::new(RefCell::new(None));
    let hook_held = held.clone();
    let hub = f.hub.clone();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let before_close = retry_probe::install_before_close(move || {
        *hook_held.borrow_mut() = Some(HeldDb::new(hub.clone()));
        let _ = started_tx.send(());
    });
    let waits = Rc::new(Cell::new(0));
    let hook_waits = waits.clone();
    let hub = f.hub.clone();
    let probe = retry_probe::install(move || {
        retry_probe::assert_auxiliary_unlocked(&hub);
        assert!(hub.whatsapp.0.bots.try_lock().is_ok());
        assert!(matches!(hub.store.db.try_lock(), Err(TryLockError::WouldBlock)));
        assert!(!retry_probe::plain_live());
        hook_waits.set(hook_waits.get() + 1);
    });
    started_rx.await.unwrap();
    // DB continua preso até o timer concluir a TERCEIRA tentativa e sair do loop.
    // O runtime dirige os dois sleeps reais da sequência usando tempo virtual.
    stopped.await.expect("o timer deve encerrar após a sequência StoreBusy completa");
    assert_eq!(waits.get(), 2);
    assert!(retry_probe::has_draft(&f.hub, &id));
    drop(held.borrow_mut().take());
    assert_eq!(f.hub.store.kv_read(&private).unwrap(), before_private);
    assert_eq!(f.hub.store.kv_read(&receipt).unwrap(), before_receipt);
    // A saída do loop foi confirmada, não inferida pela passagem de tempo.
    // Avançar mais um período conserva as assertivas sem depender de yield.
    tokio::time::advance(std::time::Duration::from_secs(1)).await;
    assert_eq!(waits.get(), 2);
    assert!(retry_probe::has_draft(&f.hub, &id));
    assert_eq!(f.hub.store.kv_read(&private).unwrap(), before_private);
    tokio::time::resume();
    drop(probe);
    drop(before_close);
    // Nova leitura, sem renovar o prazo nem construir outra fonte, converge a limpeza.
    let value = assert_send(whatsapp_human::read(&f.hub, &id)).await.unwrap();
    assert!(value["presentation"].is_null());
    assert_eq!(value["receipt"]["state"], "closed");
    assert!(!retry_probe::has_draft(&f.hub, &id));
    assert_eq!(f.hub.store.kv_read(&private).unwrap(), None);
    f.done().await;
}
