use super::*;
use crate::{remote::identity::Identity, store::Scope};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

struct Fixture {
    hub: Arc<Hub>,
    directory: PathBuf,
    space: String,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn fixture() -> Fixture {
    let directory = std::env::temp_dir().join(format!("background-space-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let store = crate::store::Store::open(Path::new(":memory:")).unwrap();
    let account = format!("ac_{}", "a".repeat(64));
    store
        .kv_set(
            engine_accounts::SLOT,
            &json!({"salt":uuid::Uuid::new_v4().to_string(),"accounts":[{
                "id":account,"backend":"claude","billing":"subscription","state":"connected",
                "launch":{"kind":"cli_profile","executable":"/bin/false","directory":"/fixture/profile"},
                "scope":"owner","quotaIdentity":account,"verifiedAt":1,
                "subscriptionProof":{"method":"claude_auth_status","plan":"max"},
                "models":[{"id":"haiku","options":[]}]
            }]})
            .to_string(),
        )
        .unwrap();
    config::initialize(&store).unwrap();
    config::save(
        &store,
        config::load(&store).unwrap().revision,
        Some(FunctionId::MemoryConsolidation),
        Some(config::SelectionInput::Custom {
            binding: Binding { account_id: account, model_id: "haiku".into(), model_options: BTreeMap::new() },
        }),
    )
    .unwrap();
    let mut spaces = store.spaces().unwrap();
    let mut space = spaces.spaces[0].clone();
    space.id = uuid::Uuid::new_v4().to_string();
    space.kind = "space".into();
    space.name = "Trabalho".into();
    let id = space.id.clone();
    spaces.spaces.push(space);
    store.kv_set(SPACES, &serde_json::to_string(&spaces).unwrap()).unwrap();
    let hub = Hub::new(store, "test".into(), Identity::load_or_create(&directory).unwrap(), "test".into(), 0);
    Fixture { hub, directory, space: id }
}
fn request(f: &Fixture, enabled: bool, expected: i64) -> Value {
    let db = f.hub.store.db.locked();
    let (reference, _) = current_reference(&db, &f.space).unwrap();
    json!({"spaceId":f.space,"functionId":"memory_consolidation","expectedRevision":expected,
        "expectedSpaceRevision":reference.space_revision,"functionRevision":reference.function_revision,
        "accountId":reference.account_id,"accountRevision":reference.account_revision,"enabled":enabled})
}
fn context(space: &str) -> Context {
    serde_json::from_value(json!({"botId":"bot","chatId":"bot","spaceId":space,
        "memoryId":"bot","botRevision":1,"conversationRevision":null,"session":"session",
        "token":"token","messages":[],"responses":[],"startedAt":1}))
    .unwrap()
}

#[tokio::test]
async fn space_nao_herda_selecao_pessoal_e_mudancas_revogam_snapshot() {
    let f = fixture();
    let ctx = context(&f.space);
    assert!(resolve(&f.hub, &ctx).is_err());
    let response = call(&f.hub, &Caller::Local, "saveBackgroundSpaceSelection", request(&f, true, 0)).await.unwrap();
    assert!(
        response["selections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["spaceId"] == f.space && r["state"] == "configured")
    );
    let prepared = resolve(&f.hub, &ctx).unwrap();
    assert!(with_current(&f.hub, &prepared, |check| check(&f.hub.store.db.locked(), &ctx)).is_ok());
    let current = config::load(&f.hub.store).unwrap();
    config::save(&f.hub.store, current.revision, Some(FunctionId::BotNaming), None).unwrap();
    assert!(resolve(&f.hub, &ctx).is_err());
    assert!(with_current(&f.hub, &prepared, |check| check(&f.hub.store.db.locked(), &ctx)).is_err());
}

#[tokio::test]
async fn device_control_nao_e_capacidade_e_payload_nao_fabrica_autoridade() {
    let f = fixture();
    let caller =
        Caller::Device { key: "untrusted".into(), scopes: vec![Scope::Control, Scope::OwnerDevices], autoridade: None };
    assert!(call(&f.hub, &caller, "backgroundSpaceSelections", json!({})).await.is_err());
    assert!(call(&f.hub, &caller, "saveBackgroundSpaceSelection", request(&f, true, 0)).await.is_err());
    let mut fake = request(&f, true, 0);
    fake["isOwner"] = true.into();
    assert!(call(&f.hub, &Caller::Local, "saveBackgroundSpaceSelection", fake).await.is_err());
    assert!(row(&f.hub.store.db.locked(), &f.space).unwrap().is_none());
}

#[tokio::test]
async fn cas_preserva_autorizacao_e_remocao_funciona_sem_conta_disponivel() {
    let f = fixture();
    call(&f.hub, &Caller::Local, "saveBackgroundSpaceSelection", request(&f, true, 0)).await.unwrap();
    let before = resolve(&f.hub, &context(&f.space)).unwrap();
    assert!(call(&f.hub, &Caller::Local, "saveBackgroundSpaceSelection", request(&f, false, 0)).await.is_err());
    let (revision, _, _) = row(&f.hub.store.db.locked(), &f.space).unwrap().unwrap();
    f.hub.store.kv_delete(engine_accounts::SLOT).unwrap();
    let revision_space = space_revision(&f.hub.store.db.locked(), &f.space).unwrap();
    let response = call(
        &f.hub,
        &Caller::Local,
        "saveBackgroundSpaceSelection",
        json!({
            "spaceId":f.space,"functionId":"memory_consolidation","expectedRevision":revision,
            "expectedSpaceRevision":revision_space,"enabled":false,
        }),
    )
    .await
    .unwrap();
    assert!(
        response["selections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["spaceId"] == f.space && r["state"] == "not_authorized")
    );
    assert!(with_current(&f.hub, &before, |check| check(&f.hub.store.db.locked(), &context(&f.space))).is_err());
}

#[test]
fn callback_original_nao_revalida_com_nova_concessao() {
    let f = fixture();
    let db = f.hub.store.db.locked();
    let (reference, binding) = current_reference(&db, &f.space).unwrap();
    drop(db);
    let prepared = Prepared {
        space: f.space.clone(),
        reference,
        binding,
        revision: Some(1),
        issuer: Issuer::Owner { key: "old-device".into(), binding: "old-capability".into() },
    };
    assert!(with_current(&f.hub, &prepared, |_| Ok(())).is_err());
}

// Mesmo encadeamento de fixture ACS4 usado pelos testes existentes de engine_setup.
impl Fixture {
    /// Cadeia ACS4 completa, com assinaturas próprias e sem rede/credencial real.
    fn owner(&self) -> Caller {
        use crate::{
            fork::bootstrap::codec::{self, Challenge, Grant, Proof, Receipt},
            remote::{cloud, crypto},
            store::bootstrap::{CloudGrant, Publication, TrustedGeneration},
        };
        use ed25519_dalek::SigningKey;
        let issuer = SigningKey::from_bytes(&[21; 32]);
        let device = SigningKey::from_bytes(&[22; 32]);
        let host = &self.hub.identity.sign;
        let now = crate::store::now_ms();
        let cfg = TrustedGeneration {
            generation: crypto::b64(&[23; 32]),
            issuer_key_id: "issuer".into(),
            issuer_key: crypto::b64(issuer.verifying_key().as_bytes()),
            issuer: "donna".into(),
            deployment_id: "fixture".into(),
            env: "test".into(),
            manifest_hash: "a".repeat(64),
            resource_digest: "b".repeat(64),
            computer_id: "computer".into(),
            sign_key: crypto::b64(host.verifying_key().as_bytes()),
            box_key: crypto::b64(&[24; 32]),
        };
        let g = Grant(
            "donna/bootstrap-grant/v1".into(),
            "boot".into(),
            "intent".into(),
            cfg.resource_digest.clone(),
            cfg.generation.clone(),
            cfg.issuer_key_id.clone(),
            cfg.issuer.clone(),
            cfg.deployment_id.clone(),
            cfg.env.clone(),
            cfg.manifest_hash.clone(),
            "owner".into(),
            "device".into(),
            crypto::b64(device.verifying_key().as_bytes()),
            crypto::b64(&[25; 32]),
            cfg.computer_id.clone(),
            cfg.sign_key.clone(),
            cfg.box_key.clone(),
            crypto::b64(&[26; 32]),
            now - 1000,
            now + 200_000,
            vec!["control".into(), "owner.devices".into()],
        );
        let store = &self.hub.store;
        store.bootstrap_install_generation(&cfg).unwrap();
        store.bootstrap_install_grant(&codec::encode(&g, |b| crypto::sign(&issuer, b)).unwrap(), now).unwrap();
        let wire = store.bootstrap_challenge(host, "boot", &g.12, now).unwrap();
        let ch: Challenge = codec::decode(&wire, &host.verifying_key().to_bytes()).unwrap();
        let proof = Proof(
            "donna/bootstrap-proof/v1".into(),
            g.1.clone(),
            g.4.clone(),
            codec::hash(&g).unwrap(),
            codec::hash(&ch).unwrap(),
        );
        let wire = store
            .bootstrap_commit_proof(host, &codec::encode(&proof, |b| crypto::sign(&device, b)).unwrap(), now)
            .unwrap();
        let r: Receipt = codec::decode(&wire, &host.verifying_key().to_bytes()).unwrap();
        let publication = Publication {
            bootstrap_id: r.1.clone(),
            generation: r.2.clone(),
            grant_id: r.11.clone(),
            device_key: r.8.clone(),
            owner_user_id: r.6.clone(),
            receipt_hash: codec::hash(&r).unwrap(),
        };
        let grant = CloudGrant { grant_id: r.11, device_key: r.8.clone(), owner_user_id: r.6 };
        assert!(store.bootstrap_promote(&publication, &grant, now).unwrap());
        assert!(store.bootstrap_activation_envelope(host, now).unwrap().is_some());
        cloud::update_status(&self.hub, |s| s.owner = Some(cloud::Owner { user_id: "owner".into(), email: None }));
        self.hub.cloud_synced.store(true, Ordering::Release);
        crate::fork::autoridade::autenticar(&self.hub, &r.8).unwrap().1
    }
}

#[tokio::test]
async fn dono_remoto_autoriza_referencia_sem_liberar_documento_global_e_revogacao_cerca_efeito() {
    let f = fixture();
    let owner = f.owner();
    assert!(crate::api::dispatch(&f.hub, &owner, "functionModels", json!({})).await.is_err());
    let view = crate::api::dispatch(&f.hub, &owner, "backgroundSpaceSelections", json!({})).await.unwrap();
    let current = &view["selections"].as_array().unwrap().iter().find(|r| r["spaceId"] == f.space).unwrap()["current"];
    assert_eq!(current["modelId"], "haiku");
    assert!(current["accountLabel"].as_str().is_some_and(|s| !s.is_empty()));
    for private in ["launch", "quotaIdentity", "models", "salt", "directory"] {
        assert!(current.get(private).is_none());
    }
    let saved =
        crate::api::dispatch(&f.hub, &owner, "saveBackgroundSpaceSelection", request(&f, true, 0)).await.unwrap();
    let row = saved["selections"].as_array().unwrap().iter().find(|r| r["spaceId"] == f.space).unwrap();
    assert_eq!(row["state"], "configured");
    let ctx = context(&f.space);
    let prepared = resolve(&f.hub, &ctx).unwrap();
    assert!(with_current(&f.hub, &prepared, |guard| guard(&f.hub.store.db.locked(), &ctx)).is_ok());
    crate::remote::cloud::update_status(&f.hub, |s| s.owner.as_mut().unwrap().user_id = "outra-posse".into());
    assert!(with_current(&f.hub, &prepared, |guard| guard(&f.hub.store.db.locked(), &ctx)).is_err());
    assert!(crate::api::dispatch(&f.hub, &owner, "backgroundSpaceSelections", json!({})).await.is_err());
    assert_eq!(row["execution"], "awaiting_executor", "cadastro nunca libera bytes no Space");
}
