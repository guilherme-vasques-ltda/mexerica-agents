#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::*;
use crate::{
    fork::engine_accounts::{
        Account, AuthorizedScope, Backend, Billing, LaunchReference, Registry, SLOT as ACCOUNTS_SLOT, State,
    },
    remote::identity::Identity,
    store::Scope,
};
use std::sync::atomic::Ordering;
use tokio::{io::AsyncWriteExt, net::TcpListener};

struct TestEnv {
    hub: Arc<Hub>,
    dir: tempfile::TempDir,
}

impl TestEnv {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("test.db")).unwrap();
        let identity = Identity::load_or_create(dir.path()).unwrap();
        let hub = Hub::new(store, "test-host".into(), identity, "test-token".into(), 0);
        Self { hub, dir }
    }
}

#[tokio::test]
async fn status_reflects_real_readiness_and_never_fabricates_ai() {
    let env = TestEnv::new();
    let initial = crate::api::dispatch(&env.hub, &Caller::Local, STATUS_METHOD, json!({})).await.unwrap();
    assert_eq!(initial["hubReady"], false);
    assert_eq!(initial["spaceContext"]["personalSpaceId"], crate::fork::spaces::PERSONAL);
    let remote = Caller::Device { key: "dev-1".into(), scopes: vec![Scope::Control], autoridade: None };
    assert!(crate::api::dispatch(&env.hub, &remote, STATUS_METHOD, json!({})).await.is_err());
    assert_eq!(initial["ai"]["hasReadyAi"], false);
    assert_eq!(initial["ai"]["connectedAccounts"], 0);
    assert_eq!(initial["readiness"]["readyCount"], 0);
    assert!(initial["persisted"].is_null());

    // After default Berga is created, hubReady becomes true, but AI remains false.
    env.hub.store.initialize_default_donna().unwrap();
    let with_berga = crate::api::dispatch(&env.hub, &Caller::Local, STATUS_METHOD, json!({})).await.unwrap();
    assert_eq!(with_berga["hubReady"], true);
    assert_eq!(with_berga["ai"]["hasReadyAi"], false);
    assert_eq!(with_berga["readiness"]["readyCount"], 1);

    let mut reg = Registry::load(&env.hub.store).unwrap();
    let id = reg.fingerprint(Backend::Codex, "test", "acct-codex-1");
    let executable = env.dir.path().join("connector");
    std::fs::write(&executable, "fixture").unwrap();
    reg.accounts.push(Account {
        managed: None,
        id: id.clone(),
        backend: Backend::Codex,
        billing: Billing::PaidApi,
        state: State::Connected,
        launch: LaunchReference::CliProfile { executable, directory: env.dir.path().to_path_buf() },
        scope: AuthorizedScope::Owner,
        quota_identity: None,
        verified_at: None,
        subscription_proof: None,
        models: Vec::new(),
        hidden_model_ids: Vec::new(),
    });
    env.hub.store.kv_set(ACCOUNTS_SLOT, &serde_json::to_string(&reg).unwrap()).unwrap();

    let unselected = crate::api::dispatch(&env.hub, &Caller::Local, STATUS_METHOD, json!({})).await.unwrap();
    assert_eq!(unselected["ai"]["connectedAccounts"], 1);
    assert_eq!(unselected["ai"]["hasReadyAi"], false);
    let berga = env.hub.store.team_settings().unwrap().primary_bot_id.unwrap();
    {
        use crate::fork::sessoes::{engine_choice, store};
        let mut db = env.hub.store.db.locked();
        let tx = db.transaction().unwrap();
        store::ensure_principal(&tx, &berga).unwrap();
        let revision = store::get(&tx, &berga).unwrap().unwrap().revision;
        let selection = serde_json::from_value(json!({
            "accountId":id,"backend":"codex","model":null,"modelOptions":{},"fastMode":false
        }))
        .unwrap();
        engine_choice::set(&tx, &berga, &berga, revision, &selection).unwrap();
        tx.commit().unwrap();
    }
    let with_ai = crate::api::dispatch(&env.hub, &Caller::Local, STATUS_METHOD, json!({})).await.unwrap();
    assert_eq!(with_ai["ai"]["hasReadyAi"], true);
    assert_eq!(with_ai["ai"]["connectedAccounts"], 1);
    assert_eq!(with_ai["readiness"]["readyCount"], 2);
    reg.accounts[0].state = State::Disconnected;
    env.hub.store.kv_set(ACCOUNTS_SLOT, &serde_json::to_string(&reg).unwrap()).unwrap();
    let disconnected = crate::api::dispatch(&env.hub, &Caller::Local, STATUS_METHOD, json!({})).await.unwrap();
    assert_eq!(disconnected["ai"]["hasReadyAi"], false);
}

#[cfg(unix)]
#[test]
fn direct_key_does_not_configure_berga_without_its_selected_model() {
    let env = TestEnv::new();
    env.hub.store.initialize_default_donna().unwrap();
    let id = env.hub.store.team_settings().unwrap().primary_bot_id.unwrap();
    let mut bot = env.hub.store.bot(&id).unwrap().unwrap().config;
    let registry = Registry::load(&env.hub.store).unwrap();
    let providers = ["anthropic".to_string()];
    assert!(!berga_ai_configured(&env.hub.store, &bot, &registry, true, &providers));
    bot.backend = crate::fork::motor_mexerica::ENGINE_ID.into();
    bot.model = Some("anthropic/fixture-model".into());
    assert!(!berga_ai_configured(&env.hub.store, &bot, &registry, true, &providers));
    env.hub
        .store
        .kv_set("motor-mexerica.modelos/anthropic", r#"{"modelos":[{"id":"anthropic/fixture-model"}]}"#)
        .unwrap();
    assert!(!berga_ai_configured(&env.hub.store, &bot, &registry, true, &[]));
    assert!(berga_ai_configured(&env.hub.store, &bot, &registry, true, &providers));
}

#[tokio::test]
async fn local_transport_cannot_read_or_write_account_progress() {
    let env = TestEnv::new();
    for key in ["fork.onboarding.v1.fixture-a", "fork.onboarding.v1.fixture-b"] {
        env.hub.store.kv_set(key, r#"{"step":3}"#).unwrap();
    }
    for account in [json!(null), json!("account-a"), json!("account-b")] {
        let body = if account.is_null() { json!({}) } else { json!({"accountKey": account}) };
        let status = crate::api::dispatch(&env.hub, &Caller::Local, STATUS_METHOD, body.clone()).await;
        if account.is_null() {
            assert!(status.unwrap()["persisted"].is_null());
        } else {
            assert!(status.is_err());
        }
        assert!(crate::api::dispatch(&env.hub, &Caller::Local, SAVE_METHOD, body).await.is_err());
    }
    assert!(
        crate::api::dispatch(
            &env.hub,
            &Caller::Local,
            SAVE_METHOD,
            json!({"accountKey":"account-a", "deferredBeforeHub":true})
        )
        .await
        .is_err()
    );
    for key in ["fork.onboarding.v1.fixture-a", "fork.onboarding.v1.fixture-b"] {
        assert_eq!(env.hub.store.kv_read(key).unwrap().as_deref(), Some(r#"{"step":3}"#));
    }
}

#[tokio::test]
async fn detect_is_readonly_local_only_never_unlocks_vault_and_probes_ollama() {
    let env = TestEnv::new();
    let remote = Caller::Device { key: "dev-1".into(), scopes: vec![Scope::Control], autoridade: None };
    assert!(crate::api::dispatch(&env.hub, &remote, DETECT_METHOD, json!({})).await.is_err());

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let body = r#"{"models":[{"name":"llama3.2"},{"name":"qwen2.5"}]}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes()).await;
        }
    });

    {
        let mut guard = OLLAMA_PROBE_URL.lock().unwrap();
        *guard = Some(format!("http://{addr}"));
    }
    let result = crate::api::dispatch(&env.hub, &Caller::Local, DETECT_METHOD, json!({})).await.unwrap();
    {
        let mut guard = OLLAMA_PROBE_URL.lock().unwrap();
        *guard = None;
    }
    let _ = server.await;

    assert_eq!(result["readOnly"], true);
    assert_eq!(result["extractedCredentials"], false);
    assert_eq!(result["ollama"]["reachable"], true);
    assert_eq!(result["ollama"]["modelsCount"], 2);
    assert_eq!(
        env.hub.store.vault_unlock_attempts.load(Ordering::SeqCst),
        0,
        "readonly detect must never trigger vault unlock"
    );
    assert_eq!(Registry::load(&env.hub.store).unwrap().accounts.len(), 0);
}
