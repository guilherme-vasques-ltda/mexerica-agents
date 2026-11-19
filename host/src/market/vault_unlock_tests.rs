//! Fixtures opt-in por Store; nenhum builder global ou chaveiro real.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::*;
use crate::{hub::Hub, remote::identity::Identity};
use keyring::credential::CredentialApi;
use std::{
    collections::HashMap,
    future::{Future, poll_fn},
    pin::Pin,
    sync::{
        Condvar, LazyLock, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::Poll,
};

static FIXTURES: LazyLock<Mutex<HashMap<usize, Arc<MockState>>>> = LazyLock::new(Mutex::default);

#[derive(Default)]
struct ReadBarrier {
    entered: tokio::sync::Notify,
    released: Mutex<bool>,
    ready: Condvar,
}
impl ReadBarrier {
    fn wait(&self) {
        self.entered.notify_one();
        let mut released = self.released.lock().unwrap();
        while !*released {
            released = self.ready.wait(released).unwrap();
        }
    }
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.ready.notify_all();
    }
}

#[derive(Default)]
struct MockState {
    credential: keyring::mock::MockCredential,
    gets: AtomicUsize,
    sets: AtomicUsize,
    barrier: Option<ReadBarrier>,
}
struct CountedMock(Arc<MockState>);
impl CredentialApi for CountedMock {
    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        self.0.gets.fetch_add(1, Ordering::SeqCst);
        if let Some(barrier) = &self.0.barrier {
            barrier.wait();
        }
        self.0.credential.get_secret()
    }
    fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
        self.0.sets.fetch_add(1, Ordering::SeqCst);
        self.0.credential.set_secret(secret)
    }
    fn delete_credential(&self) -> keyring::Result<()> {
        self.0.credential.delete_credential()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn fixture_id(store: &Store) -> usize {
    Arc::as_ptr(&store.vault_unlock) as usize
}

/// Chamado somente por `read_key` cfg(test); ausência de opt-in mantém o caminho real.
pub(super) fn entry(store: &Store) -> Option<keyring::Entry> {
    FIXTURES
        .lock()
        .unwrap()
        .get(&fixture_id(store))
        .cloned()
        .map(|state| keyring::Entry::new_with_credential(Box::new(CountedMock(state))))
}

struct Fixture {
    hub: Arc<Hub>,
    state: Arc<MockState>,
}
impl Fixture {
    fn new(block_read: bool) -> Self {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        *store.secret_key.locked() = None;
        let identity = Identity {
            sign: ed25519_dalek::SigningKey::from_bytes(&[17; 32]),
            box_secret: x25519_dalek::StaticSecret::from([19; 32]),
        };
        let hub = Hub::new(store, "vault-fixture".into(), identity, "fixture-token".into(), 0);
        let state = Arc::new(MockState { barrier: block_read.then(ReadBarrier::default), ..MockState::default() });
        assert!(FIXTURES.lock().unwrap().insert(fixture_id(&hub.store), state.clone()).is_none());
        Self { hub, state }
    }
    fn gets(&self) -> usize {
        self.state.gets.load(Ordering::SeqCst)
    }
    fn sets(&self) -> usize {
        self.state.sets.load(Ordering::SeqCst)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(barrier) = &self.state.barrier {
            barrier.release();
        }
        FIXTURES.lock().unwrap().remove(&fixture_id(&self.hub.store));
    }
}

async fn pending<F: Future>(mut future: Pin<&mut F>) {
    poll_fn(|cx| {
        assert!(future.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn t1_trava_presa_expira_em_65s_sem_ler_chaveiro() {
    let fixture = Fixture::new(false);
    let _guard = fixture.hub.store.vault_unlock.lock().await;
    let started = tokio::time::Instant::now();
    let unlock = super::super::unlock(fixture.hub.clone());
    tokio::pin!(unlock);
    pending(unlock.as_mut()).await;
    tokio::time::advance(Duration::from_secs(64)).await;
    pending(unlock.as_mut()).await;
    assert!(fixture.hub.store.secret_key.locked().is_none());
    assert_eq!(fixture.gets(), 0);
    tokio::time::advance(Duration::from_secs(1)).await;
    let error = unlock.await.unwrap_err();
    assert!(error.to_string().contains("locked or unavailable"));
    assert_eq!(started.elapsed(), Duration::from_secs(65));
    assert!(fixture.hub.store.secret_key.locked().is_none());
    assert_eq!(fixture.gets(), 0);
    assert_eq!(fixture.sets(), 0);
    assert!(fixture.hub.store.kv_read("vault.id").unwrap().is_none());
}

#[tokio::test(start_paused = true)]
async fn t2_cache_retorna_imediato_com_trava_presa() {
    let fixture = Fixture::new(false);
    *fixture.hub.store.secret_key.locked() = Some(Zeroizing::new(vec![23; 32]));
    let _guard = fixture.hub.store.vault_unlock.lock().await;
    let started = tokio::time::Instant::now();
    tokio::time::timeout(Duration::ZERO, super::super::unlock(fixture.hub.clone())).await.unwrap().unwrap();
    assert_eq!(started.elapsed(), Duration::ZERO);
    assert_eq!(fixture.hub.store.secret_key.locked().as_deref().unwrap().as_slice(), &[23; 32]);
    assert_eq!(fixture.gets(), 0);
    assert_eq!(fixture.sets(), 0);
}

#[tokio::test]
async fn t3_dois_unlock_compartilham_uma_leitura_e_uma_chave() {
    let fixture = Fixture::new(true);
    let first = tokio::spawn(super::super::unlock(fixture.hub.clone()));
    let barrier = fixture.state.barrier.as_ref().unwrap();
    tokio::time::timeout(Duration::from_secs(65), barrier.entered.notified()).await.unwrap();
    assert!(fixture.hub.store.vault_unlock.try_lock().is_err());
    // A leitura bloqueada não prende a chave em memória nem SQLite (C2).
    assert!(fixture.hub.store.secret_key.try_lock().unwrap().is_none());
    assert!(fixture.hub.store.kv_read("vault.id").unwrap().is_some());
    let second = super::super::unlock(fixture.hub.clone());
    tokio::pin!(second);
    pending(second.as_mut()).await;
    assert_eq!(fixture.gets(), 1);
    assert_eq!(fixture.sets(), 0);
    barrier.release();
    let (first, second) = tokio::join!(first, second);
    assert!(first.unwrap().is_ok());
    assert!(second.is_ok());
    assert_eq!(fixture.gets(), 1);
    assert_eq!(fixture.sets(), 1);
    let cached = fixture.hub.store.secret_key.locked().as_ref().unwrap().to_vec();
    assert_eq!(cached.len(), 32);
    assert_eq!(fixture.state.credential.get_secret().unwrap(), cached);
    assert_eq!(fixture.hub.store.kv_read("vault.initialized").unwrap().as_deref(), Some("true"));
}

#[tokio::test]
async fn t4_initialized_sem_chave_nao_recria_e_preserva_ciphertext() {
    let fixture = Fixture::new(false);
    let store = &fixture.hub.store;
    *store.secret_key.locked() = Some(Zeroizing::new(vec![31; 32]));
    super::super::write(store, "fixture-record", "segredo sintético").unwrap();
    let ciphertext = store.kv_read("fixture-record").unwrap().unwrap();
    assert!(ciphertext.starts_with("vault:v1:"));
    *store.secret_key.locked() = None;
    store.kv_set("vault.initialized", "true").unwrap();
    let error = super::super::unlock(fixture.hub.clone()).await.unwrap_err();
    assert!(error.to_string().contains("credential key is missing"));
    assert_eq!(fixture.gets(), 1);
    assert_eq!(fixture.sets(), 0);
    assert!(store.secret_key.locked().is_none());
    assert_eq!(store.kv_read("fixture-record").unwrap().unwrap(), ciphertext);
    assert_eq!(store.kv_read("vault.initialized").unwrap().as_deref(), Some("true"));
}
