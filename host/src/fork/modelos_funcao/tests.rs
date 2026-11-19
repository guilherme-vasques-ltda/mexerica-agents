use super::{
    catalogo::FunctionId,
    config::{self, Binding, Selection, SelectionInput},
    resolver::*,
};
use crate::{
    api::devices::Caller,
    hub::Hub,
    remote::identity::Identity,
    store::{Scope, Store},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn account(id: char, billing: &str) -> Value {
    json!({"id":format!("ac_{}", id.to_string().repeat(64)), "backend":"codex", "billing":billing,
        "state":"connected", "launch":{"kind":"cli_profile","executable":"/private/TEST_SECRET/cli",
        "directory":"/private/TEST_SECRET/profile"}, "scope":"owner",
        "quotaIdentity":format!("ac_{}", id.to_string().repeat(64)), "verifiedAt":1,
        "subscriptionProof":{"method":"codex_account_read","plan":"pro"},
        "models":[{"id":"observed-model", "options":[{"id":"reasoning_effort","values":["low","high"],"default":"high"}]}]})
}
fn registry(store: &Store, values: Vec<Value>) {
    store
        .kv_set(
            "fork.engine-accounts.v1",
            &json!({"salt":uuid::Uuid::new_v4().to_string(), "accounts":Value::Array(values)}).to_string(),
        )
        .unwrap();
}
fn binding(id: char) -> Binding {
    Binding {
        account_id: format!("ac_{}", id.to_string().repeat(64)),
        model_id: "observed-model".into(),
        model_options: BTreeMap::from([("reasoning_effort".into(), "high".into())]),
    }
}
fn custom(store: &Store, binding: Binding) -> config::Document {
    config::save(
        store,
        config::load(store).unwrap().revision,
        Some(FunctionId::BotNaming),
        Some(SelectionInput::Custom { binding }),
    )
    .unwrap()
}
fn evidence(id: char) -> AccountEvidence {
    AccountEvidence {
        account_id: binding(id).account_id.clone(),
        quota_identity: binding(id).account_id,
        quota: Quota::Available,
        models: vec![(
            "observed-model".into(),
            Capabilities {
                input: vec![Modality::Text],
                output: vec![Modality::Text],
                tools_disabled: true,
                max_input_tokens: 8192,
                max_output_tokens: 1024,
            },
        )],
    }
}

#[test]
fn resolves_exact_account_in_owner_context_options_billing_quota_and_capabilities_without_retargeting() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    registry(&store, vec![account('a', "subscription"), account('b', "subscription")]);
    custom(&store, binding('b'));
    let caller = Caller::Local;
    let proof = [evidence('a'), evidence('b')];
    let mut context = TrustedContext::administration(&caller);
    context.evidence = &proof;
    let resolved = resolve_function(&store, "bot_naming", &context).unwrap();
    assert_eq!(resolved.effective.account_id, binding('b').account_id);
    assert_eq!(resolved.owner, "owner");
    assert!(resolved.declared_alternative.is_none());
    registry(&store, vec![account('b', "subscription"), account('a', "subscription")]);
    assert_eq!(resolve_function(&store, "bot_naming", &context).unwrap().effective, binding('b'));
    context.owner = "other-owner";
    assert_eq!(resolve_function(&store, "bot_naming", &context).unwrap_err().reason, Reason::ContextDenied);
    context.owner = "owner";
    // A opção foi válida ao salvar; removê-la do catálogo não troca a seleção.
    let mut changed = account('b', "subscription");
    changed["models"][0]["options"][0]["values"] = json!(["low"]);
    registry(&store, vec![account('a', "subscription"), changed]);
    assert_eq!(resolve_function(&store, "bot_naming", &context).unwrap_err().reason, Reason::OptionUnsupported);
    let mut unknown = account('b', "unverified");
    unknown["state"] = "unverified".into();
    unknown["verifiedAt"] = Value::Null;
    unknown["subscriptionProof"] = Value::Null;
    registry(&store, vec![account('a', "subscription"), unknown.clone()]);
    assert_eq!(resolve_function(&store, "bot_naming", &context).unwrap_err().reason, Reason::BillingUnknown);
    unknown["billing"] = "subscription".into();
    unknown["state"] = "connected".into(); // O DTO rebaixa assinatura sem prova a unverified.
    registry(&store, vec![account('a', "subscription"), unknown.clone()]);
    assert_eq!(resolve_function(&store, "bot_naming", &context).unwrap_err().reason, Reason::SubscriptionUnverified);
    unknown["billing"] = "paid_api".into();
    registry(&store, vec![account('a', "subscription"), unknown]);
    assert_eq!(resolve_function(&store, "bot_naming", &context).unwrap_err().reason, Reason::ApiNotEnabled);
    for state in ["disconnected", "unavailable"] {
        let mut unavailable = account('b', "subscription");
        unavailable["state"] = state.into();
        registry(&store, vec![account('a', "subscription"), unavailable]);
        assert_eq!(resolve_function(&store, "bot_naming", &context).unwrap_err().reason, Reason::AccountDisconnected);
    }
    registry(&store, vec![account('a', "subscription")]);
    assert_eq!(resolve_function(&store, "bot_naming", &context).unwrap_err().reason, Reason::AccountMissing);
    registry(&store, vec![account('a', "subscription"), account('b', "subscription")]);
    context.evidence = &proof[..1];
    assert_eq!(resolve_function(&store, "bot_naming", &context).unwrap_err().reason, Reason::QuotaUnknown);
    let mut exhausted = [evidence('a'), evidence('b')];
    exhausted[1].quota = Quota::Exhausted;
    context.evidence = &exhausted;
    assert_eq!(resolve_function(&store, "bot_naming", &context).unwrap_err().reason, Reason::QuotaExhausted);
    let mut incompatible = [evidence('a'), evidence('b')];
    incompatible[1].models[0].1.tools_disabled = false;
    context.evidence = &incompatible;
    assert_eq!(resolve_function(&store, "bot_naming", &context).unwrap_err().reason, Reason::ModelIncompatible);
    // Receita D109 conserva também a referência da alternativa ainda não comprovada.
    let mut claude = account('c', "subscription");
    claude["backend"] = "claude".into();
    claude["subscriptionProof"] = json!({"method":"claude_auth_status","plan":"max"});
    claude["models"] = json!([]);
    registry(&store, vec![account('a', "subscription"), account('b', "subscription"), claude]);
    let rev = config::load(&store).unwrap().revision;
    let doc = config::save(
        &store,
        rev,
        Some(FunctionId::BotDraft),
        Some(SelectionInput::Automatic {
            account_id: Some(binding('b').account_id.clone()),
            alternative_account_id: Some(binding('c').account_id.clone()),
        }),
    )
    .unwrap();
    match &doc.functions[&FunctionId::BotDraft] {
        Selection::Automatic { account_id, alternative_account_id, alternative_issue, .. } => {
            assert_eq!(account_id.as_deref(), Some(binding('b').account_id.as_str()));
            assert_eq!(alternative_account_id.as_deref(), Some(binding('c').account_id.as_str()));
            assert_eq!(*alternative_issue, Some(Reason::DefaultModelUnknown));
        }
        _ => panic!("receita automática esperada"),
    }
    registry(&store, vec![account('a', "subscription")]);
    config::initialize(&store).unwrap();
    assert_eq!(config::load(&store).unwrap(), doc);
    let doc = config::save(
        &store,
        doc.revision,
        Some(FunctionId::BotDraft),
        Some(SelectionInput::Custom { binding: binding('a') }),
    )
    .unwrap();
    assert!(matches!(doc.functions[&FunctionId::BotDraft], Selection::Custom { .. }));
}

#[test]
fn strict_persistence_cas_restart_migration_and_corruption_never_select_another_account() {
    let dir = std::env::temp_dir().join(format!("donnabot-functions-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("store.db");
    {
        let store = Store::open(&path).unwrap();
        registry(&store, vec![account('a', "subscription"), account('b', "subscription")]);
        let initial = config::load(&store).unwrap();
        let saved = custom(&store, binding('b'));
        assert!(
            config::save(&store, initial.revision, Some(FunctionId::BotNaming), None)
                .unwrap_err()
                .to_string()
                .starts_with("settings_conflict")
        );
        config::initialize(&store).unwrap();
        assert_eq!(config::load(&store).unwrap(), saved);
        let other = Store::open(&path).unwrap();
        assert_eq!(config::load(&other).unwrap(), saved);
        let before = store.kv_read(config::SLOT).unwrap().unwrap();
        custom(&other, binding('a'));
        assert!(!store.kv_compare_exchange(config::SLOT, Some(&before), &before).unwrap());
        // Retira a conta escolhida; reiniciar não restaura nem troca a seleção.
        registry(&store, vec![account('b', "subscription")]);
    }
    let store = Store::open(&path).unwrap();
    let saved = config::load(&store).unwrap();
    assert_eq!(saved.functions[&FunctionId::BotNaming], Selection::Custom { binding: binding('a') });
    let caller = Caller::Local;
    assert_eq!(
        resolve_function(&store, "bot_naming", &TrustedContext::administration(&caller)).unwrap_err().reason,
        Reason::AccountMissing
    );
    for corrupted in ["{broken".to_owned(), json!({"schemaVersion":999}).to_string()] {
        store.kv_set(config::SLOT, &corrupted).unwrap();
        assert!(config::load(&store).is_err());
        assert!(config::save(&store, saved.revision, Some(FunctionId::BotNaming), None).is_err());
        assert_eq!(
            resolve_function(&store, "bot_naming", &TrustedContext::administration(&caller)).unwrap_err().reason,
            Reason::ConfigurationUnavailable
        );
        let restarted = Store::open(&path).unwrap();
        assert_eq!(restarted.kv_read(config::SLOT).unwrap().unwrap(), corrupted);
    }
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn administrative_api_rejects_other_channels_and_forged_identity_and_emits_only_after_cas() {
    let hub = Hub::new(
        Store::open(std::path::Path::new(":memory:")).unwrap(),
        "test".into(),
        Identity {
            sign: ed25519_dalek::SigningKey::from_bytes(&[3; 32]),
            box_secret: x25519_dalek::StaticSecret::from([4; 32]),
        },
        "test".into(),
        0,
    );
    registry(&hub.store, vec![account('a', "subscription"), account('b', "subscription")]);
    let local = Caller::Local;
    let mut events = hub.events.subscribe();
    let before = hub.store.kv_read(config::SLOT).unwrap();
    for caller in [
        Caller::Device { key: "test".into(), scopes: vec![Scope::Control], autoridade: None },
        Caller::Pairing { key: "test".into() },
    ] {
        for method in [
            "functionModels",
            "saveFunctionModel",
            "restoreFunctionModel",
            "restoreFunctionModels",
            "checkFunctionModel",
        ] {
            assert!(
                crate::api::dispatch(&hub, &caller, method, json!({"human":true,"requesterId":"owner"}))
                    .await
                    .unwrap_err()
                    .is::<crate::api::devices::Forbidden>()
            );
        }
    }
    assert_eq!(hub.store.kv_read(config::SLOT).unwrap(), before);
    assert!(
        crate::api::dispatch(
            &hub,
            &local,
            "checkFunctionModel",
            json!({"functionId":"bot_naming","owner":"owner","evidence":[]})
        )
        .await
        .is_err()
    );
    let response = crate::api::dispatch(&hub, &local, "functionModels", json!({})).await.unwrap();
    assert_eq!(response["functions"].as_array().unwrap().len(), 13);
    for private in ["TEST_SECRET", "quotaIdentity", "subscriptionProof", "launch", "directory"] {
        assert!(!response.to_string().contains(private), "exposto: {private}");
    }
    let rev = response["document"]["revision"].as_u64().unwrap();
    let valid = json!({"mode":"custom", "accountId":binding('b').account_id,
        "modelId":"observed-model", "modelOptions":{"reasoning_effort":"high"}});
    let mut invalid = Vec::new();
    for (field, value) in [
        ("accountId", json!("op://Private/TEST_SECRET/token")),
        ("accountId", json!(binding('c').account_id)),
        ("modelId", json!("https://example.test/TEST_SECRET")),
        ("modelId", json!("/private/TEST_SECRET/profile")),
        ("modelId", json!("sk-TEST_SECRET")),
        ("modelId", json!("TEST_SECRET")),
        ("modelId", json!("x".repeat(129))),
        ("modelOptions", json!({"Authorization":"Bearer TEST_SECRET"})),
        ("modelOptions", json!({"reasoning_effort":"TEST_SECRET"})),
        ("modelOptions", json!({"reasoning_effort":"max"})),
    ] {
        let mut selection = valid.clone();
        selection[field] = value;
        invalid.push(selection);
    }
    for account_id in ["op://Private/TEST_SECRET/token".to_owned(), binding('c').account_id] {
        invalid.push(json!({"mode":"automatic", "accountId":account_id}));
        invalid.push(json!({"mode":"automatic", "alternativeAccountId":account_id}));
    }
    invalid.push(json!({"mode":"automatic", "alternativeAccountId":binding('a').account_id})); // Codex não é Claude.
    for selection in invalid {
        let error = crate::api::dispatch(
            &hub,
            &local,
            "saveFunctionModel",
            json!({"expectedRevision":rev,"functionId":"bot_draft","selection":selection}),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.starts_with("invalid_field"), "{error}");
        assert!(!error.contains("TEST_SECRET"));
        assert_eq!(hub.store.kv_read(config::SLOT).unwrap(), before);
        assert!(events.try_recv().is_err());
    }
    let save = json!({"expectedRevision":rev,"functionId":"bot_naming","selection":{"mode":"custom",
        "accountId":binding('b').account_id,"modelId":"observed-model","modelOptions":{"reasoning_effort":"high"}}});
    let saved = crate::api::dispatch(&hub, &local, "saveFunctionModel", save.clone()).await.unwrap();
    assert_eq!(saved["document"]["revision"], rev + 1);
    let checked =
        crate::api::dispatch(&hub, &local, "checkFunctionModel", json!({"functionId":"bot_naming"})).await.unwrap();
    assert_eq!(checked["unavailable"]["reason"], "codex_subscription_unavailable");
    assert_eq!(events.try_recv().unwrap(), json!({"type":"functionModelsChanged","revision":rev+1}));
    assert!(crate::api::dispatch(&hub, &local, "saveFunctionModel", save).await.is_err());
    assert!(events.try_recv().is_err());
    for id in ["voice_live", "speech_to_text", "text_to_speech"] {
        assert!(
            crate::api::dispatch(
                &hub,
                &local,
                "restoreFunctionModel",
                json!({"expectedRevision":rev+1,"functionId":id})
            )
            .await
            .is_err()
        );
    }
    let restored = crate::api::dispatch(
        &hub,
        &local,
        "restoreFunctionModel",
        json!({"expectedRevision":rev+1,"functionId":"bot_naming"}),
    )
    .await
    .unwrap();
    assert_eq!(restored["document"]["revision"], rev + 2);
    let restored =
        crate::api::dispatch(&hub, &local, "restoreFunctionModels", json!({"expectedRevision":rev+2})).await.unwrap();
    assert_eq!(restored["document"]["revision"], rev + 3);
    assert!(crate::api::dispatch(&hub, &local, "testFunctionModel", json!({})).await.is_err());
    // Documento antigo inseguro é recusado sem ecoar o valor no DTO/erro.
    let mut unsafe_doc = restored["document"].clone();
    let mut unsafe_selection = valid;
    unsafe_selection["accountId"] = "op://Private/TEST_SECRET/token".into();
    unsafe_doc["functions"]["bot_naming"] = unsafe_selection;
    hub.store.kv_set(config::SLOT, &unsafe_doc.to_string()).unwrap();
    let error = crate::api::dispatch(&hub, &local, "functionModels", json!({})).await.unwrap_err().to_string();
    assert!(error.starts_with("configuration_invalid"));
    assert!(!error.contains("TEST_SECRET"));
}

#[tokio::test]
async fn unavailable_accounts_never_block_boot_or_echo_private_data_and_initialization_retries() {
    let dir = std::env::temp_dir().join(format!("donnabot-functions-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    for (index, corrupted) in [
        "{broken-TEST_SECRET".to_owned(),
        json!({"salt":uuid::Uuid::new_v4().to_string(),"accounts":[],"unknown":"TEST_SECRET"}).to_string(),
        json!({"salt":uuid::Uuid::new_v4().to_string(),"accounts":[account('a',"subscription"),account('a',"subscription")]}).to_string(),
    ].into_iter().enumerate() {
        let path = dir.join(format!("{index}.db"));
        {
            let store = Store::open(&path).unwrap();
            store.kv_delete(config::SLOT).unwrap();
            store.kv_set("fork.engine-accounts.v1", &corrupted).unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert!(store.kv_read(config::SLOT).unwrap().is_none());
        let hub = Hub::new(store, "test".into(), Identity {
            sign: ed25519_dalek::SigningKey::from_bytes(&[3;32]),
            box_secret: x25519_dalek::StaticSecret::from([4;32]),
        }, "test".into(), 0);
        let response = crate::api::dispatch(&hub, &Caller::Local, "functionModels", json!({})).await.unwrap();
        assert!(response["document"].is_null());
        assert_eq!(response["accountsIssue"], "accounts_unavailable");
        assert_eq!(response["functions"][0]["resolution"]["unavailable"]["reason"], "accounts_unavailable");
        assert!(!response.to_string().contains("TEST_SECRET"));
        assert!(hub.store.kv_read(config::SLOT).unwrap().is_none());
        registry(&hub.store, vec![account('a',"subscription")]);
        let recovered = crate::api::dispatch(&hub, &Caller::Local, "functionModels", json!({})).await.unwrap();
        assert_eq!(recovered["document"]["revision"], 1);
        assert!(recovered["accountsIssue"].is_null());
        let saved = custom(&hub.store, binding('a'));
        hub.store.kv_set("fork.engine-accounts.v1", &corrupted).unwrap();
        let response = crate::api::dispatch(&hub, &Caller::Local, "functionModels", json!({})).await.unwrap();
        assert_eq!(response["document"], serde_json::to_value(&saved).unwrap());
        assert_eq!(response["accountsIssue"], "accounts_unavailable");
        let error = config::save(&hub.store, saved.revision, Some(FunctionId::BotNaming),
            Some(SelectionInput::Custom {binding:binding('a')})).unwrap_err().to_string();
        assert!(error.starts_with("accounts_unavailable"));
        assert!(!error.contains("TEST_SECRET"));
        assert_eq!(config::load(&hub.store).unwrap(), saved);
    }
    std::fs::remove_dir_all(dir).unwrap();
}

fn claude(id: char) -> Value {
    let mut value = account(id, "subscription");
    value["backend"] = "claude".into();
    value["subscriptionProof"] = json!({"method":"claude_auth_status","plan":"max"});
    value["models"] = json!([{"id":"haiku","options":[]}]);
    value
}

fn memory(store: &Store) -> Selection {
    config::load(store).unwrap().functions[&FunctionId::MemoryConsolidation].clone()
}

/// MEX-1355: conta escolhida desconectada é `account_disconnected` (com o texto
/// pronto para a tela), e o automático pula contas que não servem.
#[tokio::test]
async fn memoria_com_conta_desconectada_informa_o_motivo_e_o_automatico_pula_a_conta() {
    let hub = Hub::new(
        Store::open(std::path::Path::new(":memory:")).unwrap(),
        "test".into(),
        Identity {
            sign: ed25519_dalek::SigningKey::from_bytes(&[3; 32]),
            box_secret: x25519_dalek::StaticSecret::from([4; 32]),
        },
        "test".into(),
        0,
    );
    let store = &hub.store;
    let chosen = binding('c').account_id;
    registry(store, vec![claude('c')]);
    config::initialize(store).unwrap();
    assert!(matches!(memory(store), Selection::Automatic { binding: Some(b), .. } if b.account_id == chosen));

    // A conta escolhida desconecta e não há outra: motivo honesto, sem trocar a escolha.
    let mut off = claude('c');
    off["state"] = "disconnected".into();
    registry(store, vec![off.clone()]);
    let caller = Caller::Local;
    let context = TrustedContext::administration(&caller);
    assert_eq!(
        resolve_function(store, "memory_consolidation", &context).unwrap_err().reason,
        Reason::AccountDisconnected
    );
    config::initialize(store).unwrap();
    assert!(matches!(memory(store),
        Selection::Automatic { account_id: Some(id), binding: None, issue: Some(Reason::AccountDisconnected), .. }
        if id == chosen));
    let response = crate::api::dispatch(&hub, &caller, "functionModels", json!({})).await.unwrap();
    let function = response["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["definition"]["id"] == "memory_consolidation")
        .unwrap()
        .clone();
    assert_eq!(function["resolution"]["unavailable"]["reason"], "account_disconnected");
    assert_eq!(
        function["resolution"]["unavailable"]["message"],
        "Organizar memória: a conta Claude — login deste computador está desconectada; reconecte em Contas de IA ou escolha outra em Bot de fundo > Modelos por função"
    );
    assert_eq!(function["selection"]["accountId"], chosen.as_str());

    // Contas que não servem continuam puladas: API paga, não confirmada, sem modelo comprovado.
    let mut paid = claude('d');
    paid["billing"] = "paid_api".into();
    let mut unverified = claude('e');
    unverified["state"] = "unverified".into();
    let mut no_model = claude('f');
    no_model["models"] = json!([]);
    registry(store, vec![off.clone(), paid, unverified, no_model]);
    config::initialize(store).unwrap();
    assert!(matches!(
        memory(store),
        Selection::Automatic { binding: None, issue: Some(Reason::AccountDisconnected), .. }
    ));

    // Uma conta conectada com modelo comprovado aparece: o automático usa essa.
    registry(store, vec![off, claude('a')]);
    config::initialize(store).unwrap();
    assert!(matches!(memory(store),
        Selection::Automatic { account_id: Some(id), binding: Some(b), issue: None, .. }
        if id == binding('a').account_id && b.account_id == id && b.model_id == "haiku"));
}

#[test]
fn memoria_sem_modelo_continua_default_model_unknown_e_reconexao_retoma() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let mut no_model = claude('c');
    no_model["models"] = json!([]);
    registry(&store, vec![no_model]);
    config::initialize(&store).unwrap();
    assert!(matches!(
        memory(&store),
        Selection::Automatic { binding: None, issue: Some(Reason::DefaultModelUnknown), .. }
    ));
    let caller = Caller::Local;
    assert_eq!(
        resolve_function(&store, "memory_consolidation", &TrustedContext::administration(&caller)).unwrap_err().reason,
        Reason::DefaultModelUnknown
    );
    // A conta volta com o modelo anunciado: a seleção se completa sozinha.
    registry(&store, vec![claude('c')]);
    config::initialize(&store).unwrap();
    assert!(
        matches!(memory(&store), Selection::Automatic { binding: Some(b), .. } if b.account_id == binding('c').account_id)
    );
}
