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
async fn save_and_status_isolate_by_account_and_space_and_reject_invalid() {
    let env = TestEnv::new();
    let personal = crate::fork::spaces::PERSONAL;
    let spaces_snap =
        crate::api::dispatch(&env.hub, &Caller::Local, "donnaSpaces", json!({ "hubId": env.hub.host_id.clone() }))
            .await
            .unwrap();
    let rev = spaces_snap["revision"].as_u64().unwrap();
    let created = crate::api::dispatch(
        &env.hub,
        &Caller::Local,
        "donnaSpaceCreate",
        json!({
            "hubId": env.hub.host_id.clone(),
            "expectedRevision": rev,
            "name": "Trabalho",
            "tile": { "color": "oceano", "style": "soft", "symbol": { "kind": "letters", "value": "TR" } },
        }),
    )
    .await
    .unwrap();
    let work_space = created["spaces"][1]["id"].as_str().unwrap().to_owned();

    let saved = crate::api::dispatch(
        &env.hub,
        &Caller::Local,
        SAVE_METHOD,
        json!({
            "accountKey": "user-alpha",
            "spaceId": personal,
            "step": 3,
            "where": "local",
            "aiSkipped": false,
            "completed": false,
        }),
    )
    .await
    .unwrap();
    assert_eq!(saved["persisted"]["step"], 3);
    assert_eq!(saved["persisted"]["whereChoice"], "local");
    assert_eq!(saved["persisted"]["aiSkipped"], false);
    assert_eq!(saved["persisted"]["completed"], false);

    // Different accountKey must not see user-alpha's progress.
    let other_account = crate::api::dispatch(
        &env.hub,
        &Caller::Local,
        STATUS_METHOD,
        json!({ "accountKey": "user-beta", "spaceId": personal }),
    )
    .await
    .unwrap();
    assert!(other_account["persisted"].is_null());

    // Different spaceId must not see personal's progress.
    let other_space = crate::api::dispatch(
        &env.hub,
        &Caller::Local,
        STATUS_METHOD,
        json!({ "accountKey": "user-alpha", "spaceId": work_space }),
    )
    .await
    .unwrap();
    assert!(other_space["persisted"].is_null());

    // Invalid step or control chars fail closed.
    assert!(crate::api::dispatch(&env.hub, &Caller::Local, SAVE_METHOD, json!({ "step": 0 })).await.is_err());
    assert!(crate::api::dispatch(&env.hub, &Caller::Local, SAVE_METHOD, json!({ "step": 5 })).await.is_err());
    assert!(
        crate::api::dispatch(&env.hub, &Caller::Local, STATUS_METHOD, json!({ "accountKey": "bad\nkey" }),)
            .await
            .is_err()
    );

    // Non-owner device cannot save onboarding state.
    let remote = Caller::Device { key: "dev-1".into(), scopes: vec![Scope::Control], autoridade: None };
    assert!(
        crate::api::dispatch(
            &env.hub,
            &remote,
            STATUS_METHOD,
            json!({ "accountKey": "user-alpha", "spaceId": personal }),
        )
        .await
        .is_err()
    );
    assert!(
        crate::api::dispatch(&env.hub, &remote, SAVE_METHOD, json!({ "step": 4, "completed": true }),).await.is_err()
    );
}

#[tokio::test]
async fn initial_deferral_is_local_personal_and_does_not_replace_saved_state() {
    let env = TestEnv::new();
    let request = json!({"accountKey":"account-a", "spaceId":crate::fork::spaces::PERSONAL, "deferredBeforeHub":true});
    let first = crate::api::dispatch(&env.hub, &Caller::Local, SAVE_METHOD, request.clone()).await.unwrap();
    assert_eq!(first["persisted"]["aiSkipped"], true);
    assert!(first["persisted"].get("step").is_none());
    assert_eq!(first["persisted"]["completed"], false);
    assert_eq!(first["hubReady"], false);
    assert_eq!(first["ai"]["hasReadyAi"], false);
    let repeated = crate::api::dispatch(&env.hub, &Caller::Local, SAVE_METHOD, request.clone()).await.unwrap();
    assert_eq!(first["persisted"], repeated["persisted"]);

    let saved = crate::api::dispatch(
        &env.hub,
        &Caller::Local,
        SAVE_METHOD,
        json!({"accountKey":"account-b", "spaceId":crate::fork::spaces::PERSONAL, "step":4, "completed":true, "aiSkipped":false}),
    ).await.unwrap();
    let mut other = request.clone();
    other["accountKey"] = json!("account-b");
    let preserved = crate::api::dispatch(&env.hub, &Caller::Local, SAVE_METHOD, other).await.unwrap();
    assert_eq!(saved["persisted"], preserved["persisted"]);

    let mut wrong_space = request.clone();
    wrong_space["spaceId"] = json!("another-space");
    assert!(crate::api::dispatch(&env.hub, &Caller::Local, SAVE_METHOD, wrong_space).await.is_err());
    let mut mixed = request;
    mixed["completed"] = json!(true);
    assert!(crate::api::dispatch(&env.hub, &Caller::Local, SAVE_METHOD, mixed).await.is_err());
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
