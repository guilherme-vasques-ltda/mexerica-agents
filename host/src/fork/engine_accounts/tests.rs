use super::*;
use crate::LockExt;
use crate::{remote::identity::Identity, store::Scope};
use serde_json::json;

#[cfg(target_os = "linux")]
#[tokio::test]
async fn politica_linux_impede_processo_da_descoberta() {
    let dir = std::env::temp_dir().join(format!("fundo-politica-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(crate::service::data_dir()).unwrap();
    let executable = dir.join("claude");
    let marker = dir.join("executado");
    crate::fork::test_support::write_executable(
        &executable,
        b"#!/bin/sh\nprintf iniciado > \"$(dirname \"$0\")/executado\"\nprintf '{\"loggedIn\":false}'\n",
        0o700,
    );
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let hub = Hub::new(store, "teste".into(), Identity::load_or_create(&dir).unwrap(), "teste".into(), 0);
    let policy = dir.join("politica");
    std::fs::create_dir(&policy).unwrap();
    let blocked = discover_background_with(
        &hub,
        std::future::ready(super::super::background_policy::reject_policy_path(&policy)),
        || Some(executable.clone()),
        false,
    )
    .await;
    assert!(matches!(
        blocked.unwrap_err().downcast_ref::<super::super::background_notice::Problem>(),
        Some(super::super::background_notice::Problem::Policy)
    ));
    assert!(!marker.exists(), "a política deve impedir até o primeiro processo");
    std::fs::remove_dir(&policy).unwrap();
    discover_background_with(
        &hub,
        std::future::ready(super::super::background_policy::reject_policy_path(&policy)),
        || Some(executable),
        false,
    )
    .await
    .unwrap();
    assert!(marker.exists(), "o controle sem política deve iniciar o executável real da prova");
    std::fs::remove_dir_all(dir).unwrap();
}

/// MEX-1355 (P2 da rodada 2): parado, a reserva sonda o login local mesmo quando
/// ele já anuncia Haiku (a escolha pode ser outro modelo ou opção que sumiu).
#[cfg(target_os = "linux")]
#[tokio::test]
async fn parada_sonda_o_login_local_mesmo_com_haiku_anunciado() {
    let dir = std::env::temp_dir().join(format!("fundo-reserva-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(crate::service::data_dir()).unwrap();
    let executable = dir.join("claude");
    let marker = dir.join("executado");
    crate::fork::test_support::write_executable(
        &executable,
        b"#!/bin/sh\nprintf iniciado > \"$(dirname \"$0\")/executado\"\nprintf '{\"loggedIn\":false}'\n",
        0o700,
    );
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    store.kv_set(SLOT, &registro_local_e_gerenciado().to_string()).unwrap();
    let hub = Hub::new(store, "teste".into(), Identity::load_or_create(&dir).unwrap(), "teste".into(), 0);
    let local = || Some(executable.clone());
    discover_background_with(&hub, std::future::ready(Ok(())), local, false).await.unwrap();
    assert!(!marker.exists(), "fora da parada, o login com Haiku basta");
    discover_background_with(&hub, std::future::ready(Ok(())), local, true).await.unwrap();
    assert!(marker.exists(), "parada: confere o login e o catálogo de novo");
    std::fs::remove_dir_all(dir).unwrap();
}

/// Login local com Haiku e uma conta gerenciada no cofre, ambos de assinatura.
fn registro_local_e_gerenciado() -> Value {
    let (local, gerenciada) = (format!("ac_{}", "a".repeat(64)), format!("ac_{}", "b".repeat(64)));
    let conta = |id: &str| {
        json!({"id":id,"backend":"claude","billing":"subscription","state":"connected",
            "launch":{"kind":"cli_profile","executable":"/bin/false","directory":format!("/fixture/{id}")},
            "scope":"owner","quotaIdentity":id,"verifiedAt":1,
            "subscriptionProof":{"method":"claude_auth_status","plan":"max"},
            "models":[{"id":"haiku","options":[]},{"id":"sonnet","options":[]}]})
    };
    let mut managed = conta(&gerenciada);
    managed["managed"] = json!({"label":"Trabalho","revision":1,"generation":0,"enabled":true,
    "requestId":"pedido","principal":"fixture", "catalog": {
        "accountId":gerenciada, "principal":"fixture", "launch":managed["launch"],
        "revision":1, "subscription":managed["subscriptionProof"]
    }});
    json!({"salt":uuid::Uuid::new_v4().to_string(),"accounts":[conta(&local), managed]})
}

/// MEX-1355: a reserva mira a conta escolhida para a função — gerenciada quando
/// é ela, login local quando a escolha é local ou ainda não há conta.
#[test]
fn reserva_mira_a_conta_escolhida_para_a_funcao() {
    use crate::fork::modelos_funcao::{catalogo::FunctionId, config};
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    store.kv_set(SLOT, &registro_local_e_gerenciado().to_string()).unwrap();
    config::initialize(&store).unwrap();
    let function = FunctionId::MemoryConsolidation;
    let (local, gerenciada) = (format!("ac_{}", "a".repeat(64)), format!("ac_{}", "b".repeat(64)));
    let escolher = |input| {
        let revision = config::load(&store).unwrap().revision;
        config::save(&store, revision, Some(function), Some(input)).unwrap();
        probe_target(&store, &Registry::load(&store).unwrap(), function)
    };
    let custom = |account_id: &str| config::SelectionInput::Custom {
        binding: config::Binding {
            account_id: account_id.into(),
            model_id: "sonnet".into(),
            model_options: std::collections::BTreeMap::new(),
        },
    };
    assert_eq!(escolher(custom(&gerenciada)), ProbeTarget::Managed(gerenciada.clone()));
    assert_eq!(escolher(custom(&local)), ProbeTarget::Local);
    let automatica =
        config::SelectionInput::Automatic { account_id: Some(gerenciada.clone()), alternative_account_id: None };
    assert_eq!(escolher(automatica), ProbeTarget::Managed(gerenciada));
}

fn observation(backend: Backend) -> probe::Observation {
    let mut observation = probe::Observation::cli(backend, PathBuf::from("/private/account-profile/cli").as_path());
    observation.launch = LaunchReference::CliProfile {
        executable: "/private/account-profile/cli".into(),
        directory: "/private/account-profile/config".into(),
    };
    observation
}

fn subscription(email: &str) -> probe::Observation {
    let mut observation = observation(Backend::Codex);
    probe::codex_account(&mut observation, &json!({"account":{"type":"chatgpt","email":email,"planType":"pro"}}));
    observation.models = probe::codex_models(&json!({"data":[{
        "id":"gpt-6.1-sol","defaultReasoningEffort":"high",
        "supportedReasoningEfforts":[{"reasoningEffort":"high"},{"reasoningEffort":"xhigh"}],
        "apiKey":"TEST_SECRET_SENTINEL", "description":"/private/account-profile/secret"
    },{"id":"/private/account-profile/model"},{"id":"sk-TEST_SECRET_SENTINEL"}]}));
    observation
}

#[test]
fn dto_is_an_allowlist_and_unverified_never_becomes_a_subscription() {
    let mut registry = Registry { salt: "private-test-salt".into(), accounts: vec![] };
    let mut unknown = observation(Backend::Claude);
    probe::claude_status(
        &mut unknown,
        &json!({
            "loggedIn":true,"authMethod":"claude.ai","apiProvider":"firstParty",
            "subscriptionType":"future_plan","email":"owner@example.invalid",
            "orgId":"private-organization","token":"TEST_SECRET_SENTINEL",
            "configDirectory":"/private/account-profile/config"
        }),
        true,
    );
    let vault = probe::Observation {
        expected_account: None,
        backend: Backend::Claude,
        source: "agent-env:claude".into(),
        identity: None,
        billing: Billing::Unverified,
        state: State::Unverified,
        launch: LaunchReference::VaultProfile { slot: "agent-env:claude".into() },
        models: vec![],
        catalog_observed: false,
        subscription_proof: None,
    };
    registry.refresh(vec![subscription("owner@example.invalid"), unknown, vault]);
    let dto = registry.response();
    let text = dto.to_string();
    for forbidden in [
        "TEST_SECRET_SENTINEL",
        "/private",
        "owner@",
        "private-organization",
        "private-test-salt",
        "quotaIdentity",
        "verifiedAt",
        "launch",
        "directory",
        "slot",
        "scope",
        "token",
        "description",
    ] {
        assert!(!text.contains(forbidden), "o DTO expôs {forbidden}");
    }
    let account = &dto["accounts"][0];
    let mut keys: Vec<_> = account.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "backend",
            "billing",
            "botIds",
            "brand",
            "hiddenModelIds",
            "id",
            "label",
            "models",
            "origin",
            "paused",
            "revision",
            "state"
        ]
    );
    assert_eq!(account["models"].as_array().unwrap().len(), 1);
    assert_eq!(
        account["models"][0]["options"][0],
        json!({"id":"reasoning_effort","values":["high","xhigh"],"default":"high"})
    );
    assert!(registry.accounts[0].subscription_verified());
    assert_eq!(dto["accounts"][1]["billing"], "unverified");
    assert_eq!(dto["accounts"][1]["state"], "unverified");
    assert!(!registry.accounts[1].subscription_verified());
    assert_eq!(dto["accounts"][2]["billing"], "unverified");
    assert_eq!(dto["accounts"][2]["state"], "unverified");
    assert!(!registry.accounts[2].subscription_verified());

    for account in [
        json!({"type":"chatgpt","planType":"free","email":"owner@example.invalid"}),
        json!({"type":"chatgpt","email":"owner@example.invalid"}),
        json!({"type":"apiKey"}),
    ] {
        let mut candidate = observation(Backend::Codex);
        probe::codex_account(&mut candidate, &json!({"account":account}));
        registry.refresh(vec![candidate]);
        assert!(registry.accounts.iter().all(|account| !account.subscription_verified()));
    }
    let api = registry.accounts.iter().find(|account| account.billing == Billing::PaidApi).unwrap();
    assert!(!api.subscription_verified());
}

#[test]
fn stable_identity_survives_restart_and_deduplicates_quota_without_retargeting_old_ids() {
    let store = Store::open(PathBuf::from(":memory:").as_path()).unwrap();
    let mut registry = Registry::load(&store).unwrap();
    registry.refresh(vec![subscription("OWNER@example.invalid"), subscription("owner@example.invalid")]);
    assert_eq!(registry.accounts.len(), 1, "aliases da mesma cota não criam duas reservas");
    let id = registry.accounts[0].id.clone();
    store.kv_set(SLOT, &serde_json::to_string(&registry).unwrap()).unwrap();
    let mut reopened = Registry::load(&store).unwrap();
    reopened.refresh(vec![subscription("owner@example.invalid")]);
    assert_eq!(reopened.accounts[0].id, id);
    reopened.refresh(vec![subscription("another@example.invalid")]);
    assert_eq!(reopened.accounts.len(), 2);
    assert_eq!(reopened.accounts[0].id, id);
    assert_eq!(reopened.accounts[0].state, State::Disconnected);
    assert!(!reopened.accounts[0].subscription_verified());
    assert_ne!(reopened.accounts[0].quota_identity, reopened.accounts[1].quota_identity);
    assert!(reopened.accounts[1].subscription_verified());
    store.kv_set(SLOT, "invalid-/private/TEST_SECRET_SENTINEL").unwrap();
    let error = Registry::load(&store).err().unwrap().to_string();
    assert!(error.starts_with("accounts_unavailable:"));
    assert!(!error.contains("TEST_SECRET_SENTINEL"));
    assert_eq!(store.kv_read(SLOT).unwrap().as_deref(), Some("invalid-/private/TEST_SECRET_SENTINEL"));
}

#[test]
fn temporary_failure_preserves_the_account_id_and_recovers_without_a_phantom() {
    for backend in [Backend::Claude, Backend::Codex] {
        let connected = || match backend {
            Backend::Codex => subscription("owner@example.invalid"),
            Backend::Claude => {
                let mut candidate = observation(backend);
                probe::claude_status(
                    &mut candidate,
                    &json!({"loggedIn":true,"authMethod":"claude.ai","apiProvider":"firstParty",
                        "subscriptionType":"max","email":"owner@example.invalid","orgId":"test-org"}),
                    true,
                );
                candidate
            }
            Backend::Grok => unreachable!("o Grok tem teste próprio em engine_setup/grok"),
        };
        let mut registry = Registry { salt: "test".into(), accounts: vec![] };
        let mut failure = observation(backend);
        failure.state = State::Unavailable;
        registry.refresh(vec![failure]);
        assert!(registry.accounts.is_empty(), "uma falha sem conta conhecida não cria conta");

        registry.refresh(vec![connected()]);
        let id = registry.accounts[0].id.clone();
        let mut failure = observation(backend);
        failure.state = State::Unavailable;
        registry.refresh(vec![failure]);
        assert_eq!(registry.accounts.len(), 1);
        assert_eq!(registry.accounts[0].id, id);
        assert_eq!(registry.accounts[0].state, State::Unavailable);
        assert!(!registry.accounts[0].subscription_verified());
        assert_eq!(registry.accounts[0].models, [] as [Model; 0]);
        registry.refresh(vec![connected()]);
        assert_eq!(registry.accounts.len(), 1);
        assert_eq!(registry.accounts[0].id, id);
        assert_eq!(registry.accounts[0].state, State::Connected);
        let mut logged_out = observation(backend);
        match backend {
            Backend::Claude => probe::claude_status(&mut logged_out, &json!({"loggedIn":false}), false),
            Backend::Codex => probe::codex_account(&mut logged_out, &json!({"account":null})),
            Backend::Grok => unreachable!("o Grok tem teste próprio em engine_setup/grok"),
        }
        registry.refresh(vec![logged_out]);
        assert_eq!(registry.accounts.len(), 1);
        assert_eq!(registry.accounts[0].state, State::Disconnected);
        registry.refresh(vec![connected()]);
        registry.refresh(vec![]);
        assert_eq!(registry.accounts[0].state, State::Disconnected, "CLI ausente desconecta");
    }
    let mut candidate = observation(Backend::Claude);
    probe::claude_status(&mut candidate, &json!({"error":"consulta falhou"}), false);
    assert_eq!(candidate.state, State::Unavailable);
    probe::claude_status(&mut candidate, &json!({"loggedIn":false}), false);
    assert_eq!(candidate.state, State::Disconnected);
}

#[test]
fn codex_plan_change_does_not_retarget_the_same_email_quota() {
    let mut registry = Registry { salt: "test".into(), accounts: vec![] };
    registry.refresh(vec![subscription("owner@example.invalid")]);
    let id = registry.accounts[0].id.clone();
    let mut business = observation(Backend::Codex);
    probe::codex_account(
        &mut business,
        &json!({"account":{"type":"chatgpt","email":"owner@example.invalid","planType":"business"}}),
    );
    registry.refresh(vec![business]);
    assert_eq!(registry.accounts.len(), 2);
    assert_eq!(registry.accounts[0].id, id);
    assert_eq!(registry.accounts[0].state, State::Disconnected);
    assert_ne!(registry.accounts[0].quota_identity, registry.accounts[1].quota_identity);
    assert!(registry.accounts[1].subscription_verified());
}

#[test]
fn codex_workspace_and_plan_are_part_of_the_identity_from_the_full_response() {
    for (next_workspace, next_plan) in [("workspace-b", "pro"), ("workspace-a", "business")] {
        let candidate = |workspace: &str, plan: &str| {
            let mut candidate = observation(Backend::Codex);
            probe::codex_account(
                &mut candidate,
                &json!({"account":{"type":"chatgpt","email":"owner@example.invalid","planType":plan},
                    "workspaceRouting":{"chatgptAccountId":workspace}}),
            );
            candidate
        };
        let mut registry = Registry { salt: "test".into(), accounts: vec![] };
        registry.refresh(vec![candidate("workspace-a", "pro")]);
        let id = registry.accounts[0].id.clone();
        registry.refresh(vec![candidate(next_workspace, next_plan)]);
        assert_eq!(registry.accounts.len(), 2);
        assert_eq!(registry.accounts[0].id, id);
        assert_eq!(registry.accounts[0].state, State::Disconnected);
        assert_ne!(registry.accounts[0].quota_identity, registry.accounts[1].quota_identity);
        assert!(registry.accounts[1].subscription_verified());
        assert!(!registry.response().to_string().contains("workspace-"));
    }
    for response in [
        json!({"account":{"type":"chatgpt","email":"owner@example.invalid"}}),
        json!({"account":{"type":"chatgpt","email":"owner@example.invalid","planType":" "},
            "workspaceRouting":{"chatgptAccountId":""}}),
    ] {
        let mut candidate = observation(Backend::Codex);
        probe::codex_account(&mut candidate, &response);
        assert!(candidate.identity.is_none());
        assert_eq!(candidate.state, State::Unverified);
    }
    let mut candidate = observation(Backend::Codex);
    probe::codex_account(&mut candidate, &json!({}));
    assert_eq!(candidate.state, State::Unavailable, "campo ausente não equivale a logout");
}

#[test]
fn vault_slot_requires_nonempty_content_without_unlocking_or_migrating() {
    use crate::{LockExt, market::vault};
    let store = Store::open(PathBuf::from(":memory:").as_path()).unwrap();
    let slot = "agent-env:claude";
    assert!(!probe::vault_has_content(&store, slot));
    for empty in ["{}", r#"{"ANTHROPIC_API_KEY":" "}"#, "invalid"] {
        vault::write(&store, slot, empty).unwrap();
        assert!(!probe::vault_has_content(&store, slot));
    }
    vault::write(&store, slot, r#"{"ANTHROPIC_API_KEY":"TEST_SECRET_SENTINEL"}"#).unwrap();
    assert!(probe::vault_has_content(&store, slot));
    let encrypted = store.kv_read(slot).unwrap();
    *store.secret_key.locked() = None;
    assert!(!probe::vault_has_content(&store, slot));
    assert_eq!(store.kv_read(slot).unwrap(), encrypted);
    store.kv_set(slot, r#"{"ANTHROPIC_API_KEY":"TEST_SECRET_SENTINEL"}"#).unwrap();
    assert!(!probe::vault_has_content(&store, slot));
    assert_eq!(store.kv_read(slot).unwrap().unwrap(), r#"{"ANTHROPIC_API_KEY":"TEST_SECRET_SENTINEL"}"#);
}

fn test_hub(store: Store, directory: &std::path::Path) -> Arc<Hub> {
    Hub::new(store, "test".into(), Identity::load_or_create(directory).unwrap(), "test".into(), 0)
}

#[tokio::test]
async fn cache_expires_and_local_refresh_bypasses_it_without_rewriting_identical_accounts() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let directory = std::env::temp_dir().join(format!("donnabot-accounts-cache-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("store.db");
    let hub = test_hub(Store::open(&path).unwrap(), &directory);
    let cache = tokio::sync::Mutex::new(None);
    let probes = AtomicUsize::new(0);
    let discovery = || async {
        probes.fetch_add(1, Ordering::SeqCst);
        vec![subscription("owner@example.invalid")]
    };
    let first = read(&cache, &hub, false, discovery()).await.unwrap();
    let mut registry = Registry::load(&hub.store).unwrap();
    registry.accounts[0].verified_at = Some(1);
    hub.store.kv_set(SLOT, &serde_json::to_string(&registry).unwrap()).unwrap();
    let saved = hub.store.kv_read(SLOT).unwrap();
    let audit = rusqlite::Connection::open(&path).unwrap();
    audit
        .execute_batch(
            "CREATE TRIGGER reject_unchanged_write BEFORE UPDATE ON kv
        WHEN OLD.k = 'fork.engine-accounts.v1'
        BEGIN SELECT RAISE(ABORT, 'registro idêntico não deve ser regravado'); END;",
        )
        .unwrap();
    assert_eq!(read(&cache, &hub, false, discovery()).await.unwrap(), first);
    assert_eq!(probes.load(Ordering::SeqCst), 1);
    cache.lock().await.as_mut().unwrap().completed_at -= CACHE_TTL;
    assert_eq!(read(&cache, &hub, false, discovery()).await.unwrap(), first);
    assert_eq!(probes.load(Ordering::SeqCst), 2);
    assert_eq!(read(&cache, &hub, true, discovery()).await.unwrap(), first);
    assert_eq!(probes.load(Ordering::SeqCst), 3);
    assert_eq!(hub.store.kv_read(SLOT).unwrap(), saved);
    audit.execute_batch("DROP TRIGGER reject_unchanged_write;").unwrap();
    let changed = read(&cache, &hub, true, async { vec![subscription("another@example.invalid")] }).await.unwrap();
    assert_eq!(changed["accounts"].as_array().unwrap().len(), 2);
    assert_ne!(hub.store.kv_read(SLOT).unwrap(), saved);

    let another = test_hub(Store::open(PathBuf::from(":memory:").as_path()).unwrap(), &directory);
    let empty = read(&cache, &another, false, async { vec![] }).await.unwrap();
    assert_eq!(empty, json!({"accounts":[]}));
    drop((hub, another, audit));
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn concurrent_queries_share_the_probe_even_when_both_force_refresh() {
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        task::Poll,
    };
    let directory = std::env::temp_dir().join(format!("donnabot-accounts-flight-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let hub = test_hub(Store::open(PathBuf::from(":memory:").as_path()).unwrap(), &directory);
    let cache = tokio::sync::Mutex::new(None);
    let probes = AtomicUsize::new(0);
    let entered = tokio::sync::Notify::new();
    let release = tokio::sync::Notify::new();
    let first = read(&cache, &hub, true, async {
        probes.fetch_add(1, Ordering::SeqCst);
        entered.notify_one();
        release.notified().await;
        vec![subscription("owner@example.invalid")]
    });
    let waiting = async {
        entered.notified().await;
        let request = read(&cache, &hub, true, async {
            probes.fetch_add(1, Ordering::SeqCst);
            vec![]
        });
        tokio::pin!(request);
        std::future::poll_fn(|cx| {
            assert!(request.as_mut().poll(cx).is_pending(), "deve esperar a sondagem em andamento");
            Poll::Ready(())
        })
        .await;
        release.notify_one();
        request.await
    };
    let (first, waiting) = tokio::join!(first, waiting);
    assert_eq!(first.unwrap(), waiting.unwrap());
    assert_eq!(probes.load(Ordering::SeqCst), 1);
    drop(hub);
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn query_refuses_non_owner_and_configuration_before_running_a_cli() {
    let directory = std::env::temp_dir().join(format!("donnabot-accounts-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let hub = Hub::new(
        Store::open(PathBuf::from(":memory:").as_path()).unwrap(),
        "test".into(),
        Identity::load_or_create(&directory).unwrap(),
        "test".into(),
        0,
    );
    for caller in [
        Caller::Device { key: "executor".into(), scopes: vec![Scope::Executor], autoridade: None },
        Caller::Device { key: "screen".into(), scopes: vec![Scope::Screen], autoridade: None },
        Caller::Pairing { key: "pairing".into() },
    ] {
        let error = call(&hub, &caller, json!({})).await.unwrap_err().to_string();
        assert!(error.starts_with("remote_setup_unavailable:"));
    }
    let error = crate::api::dispatch(&hub, &Caller::Local, "donnaEngineAccounts", json!({"accountId":"spoof"}))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.starts_with("invalid_field:"), "a consulta deve passar pelo despacho do fork");
    for body in [json!({"refresh":null}), json!({"refresh":"yes"}), json!({"refresh":true,"path":"spoof"})] {
        let error = call(&hub, &Caller::Local, body).await.unwrap_err().to_string();
        assert!(error.starts_with("invalid_field:"));
    }
    let control = Caller::Device { key: "owner-device".into(), scopes: vec![Scope::Control], autoridade: None };
    let error = call(&hub, &control, json!({"refresh":true})).await.unwrap_err().to_string();
    assert!(error.starts_with("remote_setup_unavailable:"));
    assert!(serde_json::from_value::<Query>(json!({})).is_ok_and(|query| !query.refresh));
    assert!(serde_json::from_value::<Query>(json!({"refresh":false})).is_ok_and(|query| !query.refresh));
    assert!(serde_json::from_value::<Query>(json!({"refresh":true})).is_ok_and(|query| query.refresh));
    assert!(hub.store.kv_read(SLOT).unwrap().is_none());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn indisponibilidade_nao_ressuscita_conta_desconectada_e_cofre_fechado_nao_desloga() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let mut registry = Registry::load(&store).unwrap();
    registry.refresh(vec![subscription("antiga@example.invalid")]);
    registry.refresh(vec![subscription("atual@example.invalid")]);
    assert_eq!(registry.accounts[0].state, State::Disconnected);
    let mut unavailable = observation(Backend::Codex);
    unavailable.state = State::Unavailable;
    registry.refresh(vec![unavailable]);
    assert_eq!(registry.accounts[0].state, State::Disconnected);
    assert_eq!(registry.accounts[1].state, State::Unavailable);
    crate::market::vault::write(&store, "agent-env:claude", r#"{"ANTHROPIC_API_KEY":"FIXTURE"}"#).unwrap();
    let vault = probe::Observation {
        expected_account: None,
        backend: Backend::Claude,
        source: "agent-env:claude".into(),
        identity: None,
        billing: Billing::Unverified,
        state: State::Unverified,
        launch: LaunchReference::VaultProfile { slot: "agent-env:claude".into() },
        models: vec![],
        catalog_observed: false,
        subscription_proof: None,
    };
    registry.refresh(vec![vault]);
    let size = registry.accounts.len();
    *store.secret_key.locked() = None;
    managed::vault_states(&store, &mut registry).unwrap();
    let account = registry.accounts.iter().find(|a| matches!(a.launch, LaunchReference::VaultProfile { .. })).unwrap();
    assert_eq!(account.state, State::Unavailable);
    assert_eq!(registry.accounts.len(), size);
    store.kv_delete("agent-env:claude").unwrap();
    managed::vault_states(&store, &mut registry).unwrap();
    assert_eq!(
        registry.accounts.iter().find(|a| matches!(a.launch, LaunchReference::VaultProfile { .. })).unwrap().state,
        State::Disconnected
    );
}

#[cfg(unix)]
#[test]
fn bot_com_conta_grok_executa_so_o_executavel_gravado_na_conta() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("grok-cmd-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    // Espaço no caminho de propósito: o executável tem de entrar entre aspas na linha.
    let executable = dir.join("grok build");
    std::fs::write(&executable, b"#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let created = managed::create(&store, Backend::Grok, "Fixture", "cli", "cmd-proof").unwrap();
    let id = created["id"].as_str().unwrap();
    let mut registry = Registry::load(&store).unwrap();
    let revision = registry.account(id).unwrap().managed.as_ref().unwrap().revision;
    registry.account_mut(id, revision).unwrap().launch =
        LaunchReference::CliProfile { executable: executable.clone(), directory: dir.clone() };
    registry.persist(&store).unwrap();
    let bot: crate::store::BotConfig = serde_json::from_value(
        json!({"id":"fixture","name":"Fixture","backend":"grok","forkEditor":{"engineAccountId":id}}),
    )
    .unwrap();
    // O executável gravado (com espaço no caminho) entra entre aspas e os argumentos como elementos
    // separados: quem executa vê argv, nunca uma linha para o shell interpretar.
    let command = managed::recorded_command(&store, &bot).unwrap().pop().unwrap();
    assert_eq!(
        shlex::split(&command).unwrap(),
        vec![executable.to_string_lossy().into_owned(), "agent".into(), "stdio".into()]
    );
    // Fail-closed: sem o executável gravado, nenhum candidato do PATH ou do catálogo é usado.
    std::fs::remove_file(&executable).unwrap();
    let erro = managed::recorded_command(&store, &bot).unwrap_err().to_string();
    assert!(erro.starts_with("cli_unavailable"), "{erro}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn catalogo_claude_preserva_prova_exata_e_recusa_identidade_perfil_ou_revisao_trocados() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let created = managed::create(&store, Backend::Claude, "Fixture", "cli", "catalog-proof").unwrap();
    let mut registry = Registry::load(&store).unwrap();
    let id = created["id"].as_str().unwrap();
    let launch = registry.account(id).unwrap().launch.clone();
    let observe = |email: &str| {
        let mut observation = observation(Backend::Claude);
        observation.launch = launch.clone();
        probe::claude_status(
            &mut observation,
            &json!({"loggedIn":true,"authMethod":"claude.ai","apiProvider":"firstParty","subscriptionType":"max","email":email,"orgId":"fixture-org"}),
            true,
        );
        observation
    };
    registry.accounts[0].managed.as_mut().unwrap().enabled = true;
    let mut initial = observe("fixture@example.invalid");
    initial.catalog_observed = true;
    initial.models = vec![Model { id: "claude-fixture".into(), options: vec![] }];
    registry.apply_managed(0, initial);
    assert!(registry.accounts[0].catalog_valid());
    registry.persist(&store).unwrap();
    let stable = serde_json::to_string(&registry).unwrap();
    registry = Registry::load(&store).unwrap();
    registry.apply_managed(0, observe("fixture@example.invalid"));
    assert_eq!(registry.accounts[0].models[0].id, "claude-fixture");
    assert!(registry.accounts[0].catalog_valid());
    registry.accounts[0].rename("Renomeada".into()).unwrap();
    registry.apply_managed(0, observe("fixture@example.invalid"));
    assert!(registry.accounts[0].catalog_valid());
    let bot:crate::store::BotConfig = serde_json::from_value(json!({"id":"fixture","name":"Fixture","backend":"claude","model":"claude-fixture","forkEditor":{"engineAccountId":id}})).unwrap();
    registry.persist(&store).unwrap();
    managed::validate_selection(&store, &bot).unwrap();
    for kind in ["identity", "profile", "revision", "logout", "empty_catalog"] {
        let mut registry: Registry = serde_json::from_str(&stable).unwrap();
        let mut observation = observe("fixture@example.invalid");
        match kind {
            "identity" => observation = observe("other@example.invalid"),
            "profile" => {
                if let LaunchReference::CliProfile { directory, .. } = &mut observation.launch {
                    *directory = directory.join("other");
                }
            }
            "revision" => {
                registry.accounts[0].managed.as_mut().unwrap().revision += 1;
                assert_eq!(registry.accounts[0].dto().models, [] as [Model; 0]);
                registry.persist(&store).unwrap();
                assert!(managed::validate_selection(&store, &bot).is_err());
            }
            "empty_catalog" => observation.catalog_observed = true,
            "logout" => {
                observation.state = State::Disconnected;
                observation.identity = None;
            }
            _ => unreachable!(),
        }
        registry.apply_managed(0, observation);
        assert!(registry.accounts[0].models.is_empty(), "{kind}");
        assert!(!registry.accounts[0].catalog_valid(), "{kind}");
    }
    std::fs::remove_dir_all(&store.engine_setup_root).unwrap();
}

#[test]
fn perfil_legado_migrado_revalida_so_a_identidade_esperada() {
    let mut registry = Registry { salt: "revision-test".into(), accounts: vec![] };
    registry.refresh(vec![subscription("first@example.invalid"), subscription("second@example.invalid")]);
    let first = registry.accounts[0].clone();
    let second = registry.accounts[1].clone();
    let directory = crate::fork::motor_roots::accounts().join(&first.id).join("legacy/.codex");
    registry.accounts[0].launch =
        LaunchReference::CliProfile { executable: "/bin/true".into(), directory: directory.clone() };
    assert!(managed::migrated_legacy(&registry.accounts[0]));
    let mut valid = subscription("first@example.invalid");
    valid.expected_account = Some(first.id.clone());
    valid.launch = registry.accounts[0].launch.clone();
    let mut unavailable = observation(Backend::Codex);
    unavailable.expected_account = Some(second.id.clone());
    unavailable.state = State::Unavailable;
    registry.refresh(vec![valid, unavailable]);
    assert_eq!(registry.accounts[0].state, State::Connected);
    assert_eq!(registry.accounts[0].models, first.models);
    assert_eq!(registry.accounts[1].state, State::Unavailable);
    let mut changed = subscription("second@example.invalid");
    changed.expected_account = Some(first.id.clone());
    changed.launch = registry.accounts[0].launch.clone();
    registry.refresh(vec![changed]);
    assert_eq!(registry.accounts.len(), 2);
    assert_eq!(registry.accounts[0].state, State::Disconnected);
    assert_eq!(registry.accounts[1].launch, second.launch, "não adota o perfil de outra conta");
}

#[test]
fn mx6_modelos_ocultos_ida_e_volta_registro_antigo_idempotencia_e_limites() {
    let store = Store::open(PathBuf::from(":memory:").as_path()).unwrap();
    let created = managed::create(&store, Backend::Codex, "Fixture", "cli", "mx6-hidden").unwrap();
    let id = created["id"].as_str().unwrap().to_owned();
    let mut registry = Registry::load(&store).unwrap();
    let revision = registry.account(&id).unwrap().managed.as_ref().unwrap().revision;

    // Id inválido: vazio, só espaço, caractere de controle ou longo demais.
    for bad in ["", "   ", "a\nb", "x\u{7}", &"m".repeat(129)] {
        assert!(managed::model_id(bad).is_err(), "{bad:?}");
    }
    assert_eq!(managed::model_id("  gpt-6.1-sol  ").unwrap(), "gpt-6.1-sol");

    // Ida: ocultar, persistir e reler. Repetir não duplica; a revisão não muda,
    // porque o carimbo das sessões a inclui e uma sessão em uso não pode cair.
    {
        let account = registry.account_mut(&id, revision).unwrap();
        account.set_model_hidden("gpt-6.1-sol", true).unwrap();
        account.set_model_hidden("gpt-6.1-sol", true).unwrap();
        // Id fora do catálogo é aceito e guardado sem erro.
        account.set_model_hidden("modelo-fora-do-catalogo", true).unwrap();
        assert_eq!(account.hidden_model_ids, ["gpt-6.1-sol", "modelo-fora-do-catalogo"]);
        assert_eq!(account.managed.as_ref().unwrap().revision, revision);
    }
    registry.persist(&store).unwrap();
    let mut reloaded = Registry::load(&store).unwrap();
    assert_eq!(reloaded.account(&id).unwrap().hidden_model_ids, ["gpt-6.1-sol", "modelo-fora-do-catalogo"]);

    // Registro antigo, sem o campo, lê como lista vazia — sem migração destrutiva.
    let mut old = serde_json::to_value(&reloaded).unwrap();
    for item in old["accounts"].as_array_mut().unwrap() {
        item.as_object_mut().unwrap().remove("hiddenModelIds");
    }
    let old: Registry = serde_json::from_value(old).unwrap();
    assert_eq!(old.accounts[0].hidden_model_ids, [] as [String; 0]);

    // O DTO sempre expõe o campo; `models` segue intacto para o app filtrar.
    let item = &reloaded.response()["accounts"][0];
    assert_eq!(item["hiddenModelIds"], json!(["gpt-6.1-sol", "modelo-fora-do-catalogo"]));
    assert!(item["models"].is_array());

    // Volta: mostrar remove só o id pedido; mostrar ausente é no-op.
    {
        let account = reloaded.account_mut(&id, revision).unwrap();
        account.set_model_hidden("gpt-6.1-sol", false).unwrap();
        account.set_model_hidden("nunca-ocultado", false).unwrap();
        assert_eq!(account.hidden_model_ids, ["modelo-fora-do-catalogo"]);
        // Teto da lista por conta.
        while account.set_model_hidden(&format!("enchimento-{}", account.hidden_model_ids.len()), true).is_ok() {}
        assert_eq!(account.hidden_model_ids.len(), 128);
        assert!(account.set_model_hidden("estouro", true).is_err());
    }
    // Conta legada não é mutável, como nas demais alterações de conta.
    let mut legacy = Registry { salt: "test".into(), accounts: vec![] };
    legacy.refresh(vec![subscription("owner@example.invalid")]);
    let legacy_id = legacy.accounts[0].id.clone();
    let error = legacy.account_mut(&legacy_id, 1).err().unwrap().to_string();
    assert!(error.starts_with("legacy_preserved"), "{error}");
    std::fs::remove_dir_all(&store.engine_setup_root).unwrap();
}

#[test]
fn mx6_modelo_oculto_em_uso_continua_valendo_e_sumir_com_a_conta() {
    let store = Store::open(PathBuf::from(":memory:").as_path()).unwrap();
    let created = managed::create(&store, Backend::Codex, "Fixture", "cli", "mx6-in-use").unwrap();
    let id = created["id"].as_str().unwrap().to_owned();
    let mut registry = Registry::load(&store).unwrap();
    registry.accounts[0].managed.as_mut().unwrap().enabled = true;
    let launch = registry.accounts[0].launch.clone();
    let mut observation = subscription("fixture@example.invalid");
    observation.launch = launch;
    registry.apply_managed(0, observation);
    assert_eq!(registry.accounts[0].state, State::Connected);
    assert!(registry.accounts[0].models.iter().any(|m| m.id == "gpt-6.1-sol"));

    let bot: crate::store::BotConfig = serde_json::from_value(json!({
        "id":"bot-fixture","name":"Fixture","backend":"codex",
        "model":"gpt-6.1-sol",
        "forkEditor":{"engineAccountId":id}
    }))
    .unwrap();
    let revision = registry.account(&id).unwrap().managed.as_ref().unwrap().revision;
    registry.account_mut(&id, revision).unwrap().set_model_hidden("gpt-6.1-sol", true).unwrap();
    registry.persist(&store).unwrap();

    // Ocultar não tira o modelo do catálogo nem derruba a seleção que já o usa.
    let account = Registry::load(&store).unwrap();
    assert!(account.account(&id).unwrap().models.iter().any(|m| m.id == "gpt-6.1-sol"));
    assert_eq!(account.account(&id).unwrap().managed.as_ref().unwrap().revision, revision);
    managed::validate_selection(&store, &bot).unwrap();

    // Remover a conta apaga os ocultos dela junto com o registro.
    let mut registry = account;
    registry.accounts.retain(|a| a.id != id);
    assert!(registry.accounts.iter().all(|a| a.hidden_model_ids.is_empty()));
    registry.persist(&store).unwrap();
    assert_eq!(Registry::load(&store).unwrap().accounts, [] as [Account; 0]);
    std::fs::remove_dir_all(&store.engine_setup_root).unwrap();
}

/// MX-6: a consulta `donnaEngineAccounts` devolve `hiddenModelIds` por conta, inclusive
/// na resposta vinda do cache — sem sondar CLI (conta ainda não habilitada).
#[tokio::test]
async fn mx6_consulta_de_contas_devolve_hidden_model_ids() {
    let directory = std::env::temp_dir().join(format!("donnabot-mx6-read-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let hub = test_hub(Store::open(PathBuf::from(":memory:").as_path()).unwrap(), &directory);
    let created = managed::create(&hub.store, Backend::Codex, "Fixture", "cli", "mx6-read").unwrap();
    let id = created["id"].as_str().unwrap().to_owned();
    let mut registry = Registry::load(&hub.store).unwrap();
    let revision = registry.account(&id).unwrap().managed.as_ref().unwrap().revision;
    registry.account_mut(&id, revision).unwrap().set_model_hidden("gpt-6.1-sol", true).unwrap();
    registry.persist(&hub.store).unwrap();
    let cache = tokio::sync::Mutex::new(None);
    for _ in 0..2 {
        let response = read(&cache, &hub, false, async { vec![] }).await.unwrap();
        let item = response["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == id.as_str())
            .expect("conta na consulta");
        assert_eq!(item["hiddenModelIds"], json!(["gpt-6.1-sol"]));
        assert!(item["models"].is_array());
    }
    std::fs::remove_dir_all(&hub.store.engine_setup_root).unwrap();
    drop(hub);
    std::fs::remove_dir_all(directory).unwrap();
}

#[cfg(unix)]
pub(crate) async fn migrated_discovery_root(work: &std::path::Path, data: &std::path::Path) {
    let data = data.join("discovery-data");
    std::fs::create_dir(&data).unwrap();
    let old = data.join("global/.codex");
    std::fs::create_dir_all(&old).unwrap();
    let store = Store::open(&data.join("codync.db")).unwrap();
    let mut registry = Registry { salt: uuid::Uuid::new_v4().to_string(), accounts: vec![] };
    registry.refresh(vec![subscription("first@example.invalid")]);
    let id = registry.accounts[0].id.clone();
    let directory = crate::fork::motor_roots::accounts().join(&id).join("legacy/.codex");
    let status = std::process::Command::new(crate::fork::motor_exec::helper_path())
        .args(["--cwd", "/", "--", "/bin/mkdir", "-p", directory.to_str().unwrap()])
        .env("DONNABOT_MOTOR_PRIVATE", "1")
        .output()
        .unwrap();
    assert!(status.status.success());
    let executable = work.join("codex-account-fixture");
    let script = format!(
        r"#!/usr/bin/python3
import os,json,sys
assert os.environ['CODEX_HOME']=={directory:?}
assert os.environ['HOME']=={home:?}
for line in sys.stdin:
 r=json.loads(line)
 if 'id' not in r: continue
 method=r['method']
 result={{}}
 if method=='account/read': result={{'account':{{'type':'chatgpt','email':'first@example.invalid','planType':'pro'}}}}
 if method=='model/list': result={{'data':[{{'id':'gpt-6.1-sol'}}]}}
 print(json.dumps({{'id':r['id'],'result':result}}),flush=True)
",
        directory = directory.to_str().unwrap(),
        home = directory.parent().unwrap().to_str().unwrap()
    );
    crate::fork::test_support::write_executable(&executable, script.as_bytes(), 0o750);
    registry.accounts[0].launch = LaunchReference::CliProfile { executable: executable.clone(), directory: old };
    store.kv_set(SLOT, &serde_json::to_string(&registry).unwrap()).unwrap();
    crate::fork::motor_migration::run(&data, false, false).unwrap();
    registry = Registry::load(&store).unwrap();
    assert!(
        matches!(&registry.accounts[0].launch, LaunchReference::CliProfile { directory: actual, .. } if actual == &directory)
    );
    let observations = probe::discover_with(&store, |_| None).await;
    assert_eq!(observations.len(), 1);
    assert_eq!(observations[0].expected_account.as_deref(), Some(id.as_str()));
    assert_eq!(observations[0].state, State::Connected);
    registry.refresh(observations);
    assert_eq!(registry.accounts.len(), 1);
    assert_eq!(registry.accounts[0].id, id);
    assert_eq!(registry.accounts[0].state, State::Connected);
    assert_eq!(registry.accounts[0].models[0].id, "gpt-6.1-sol");
    managed::remove_profile_home(directory.parent().unwrap()).unwrap();
    std::fs::remove_file(executable).unwrap();
    println!(
        "PASS F3-REV2 descoberta real do CLI legado no perfil migrado 0700, HOME correto, identidade e catálogo preservados"
    );
}

#[test]
fn perfil_antigo_so_sai_da_lista_desconectado_e_sem_credencial_do_hub() {
    let mut registry = Registry { salt: "test".into(), accounts: vec![] };
    registry.refresh(vec![subscription("owner@example.invalid")]);
    let id = registry.accounts[0].id.clone();
    let revision = registry.accounts[0].dto().revision;
    // Conectado: a conta ainda está em uso pelo login do computador; nada sai.
    registry.accounts[0].state = State::Connected;
    let error = registry.legacy_removable(&id, revision).unwrap_err().to_string();
    assert!(error.starts_with("legacy_preserved"), "{error}");
    // Desconectado (login sumiu), perfil CLI comum: pode sair, na revisão mostrada.
    registry.accounts[0].state = State::Disconnected;
    registry.legacy_removable(&id, revision).unwrap();
    let error = registry.legacy_removable(&id, revision + 1).unwrap_err().to_string();
    assert!(error.starts_with("account_conflict"), "{error}");
    assert!(
        registry.legacy_removable("ac_inexistente", revision).unwrap_err().to_string().starts_with("account_not_found")
    );
    // Configuração do cofre fica preservada (remover apagaria o segredo guardado).
    registry.accounts[0].launch = LaunchReference::VaultProfile { slot: "slot-teste".into() };
    let error = registry.legacy_removable(&id, revision).unwrap_err().to_string();
    assert!(error.starts_with("legacy_preserved"), "{error}");
    // Conta gerenciada nunca passa por aqui (continua com `account_mut`).
    let store = Store::open(PathBuf::from(":memory:").as_path()).unwrap();
    managed::create(&store, Backend::Codex, "Fixture", "cli", "legacy-removivel").unwrap();
    let managed = Registry::load(&store).unwrap();
    let id = managed.accounts[0].id.clone();
    assert!(managed.legacy_removable(&id, 1).unwrap_err().to_string().starts_with("legacy_preserved"));
}
