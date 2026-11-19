use super::*;
use crate::{
    LockExt,
    api::devices::{self, Caller},
    fork::autoridade::{self, Autoridade},
    remote::identity::Identity,
    store::{Device, DeviceSource, Scope, Store},
};
use rusqlite::params;
use serde_json::json;
use std::{path::Path, sync::atomic::Ordering};

fn origin(key: &str) -> Origin {
    Origin {
        schema: "donna.personal-link/origin/v1".into(),
        hub: "fixture-hub".into(),
        context: "fixture-context".into(),
        generation: "fixture-generation".into(),
        key: key.into(),
        admission_digest: "a".repeat(64),
        approver: "fixture-qualified-T".into(),
        consent_digest: "b".repeat(64),
        policy: PolicyBinding {
            policy_id: "donna.personal-link/core".into(),
            policy_version: 1,
            policy_digest: crate::fork::bootstrap::codec::hash_bytes(POLICY),
            rights: vec!["link.connect".into()],
            delegation: false,
            rights_revision: 1,
        },
    }
}

fn device(key: &str, source: DeviceSource) -> Device {
    Device {
        key: key.into(),
        name: "fixture".into(),
        platform: "fixture".into(),
        source,
        grant_id: None,
        scopes: vec![Scope::Control, Scope::OwnerDevices],
        lease_until: None,
        created_at: 1,
        last_seen_at: None,
    }
}

fn insert(store: &Store, key: &str) -> Result<()> {
    store.db.locked().execute(
        "INSERT INTO personal_link_origins(key,binding) VALUES(?,?)",
        params![key, serde_json::to_vec(&origin(key))?],
    )?;
    Ok(())
}

#[test]
fn policy_binding_has_no_defaults_or_implicit_delegation() -> Result<()> {
    let base = origin("fixture-key");
    let value = serde_json::to_value(&base)?;
    let bytes = serde_json::to_vec(&base)?;
    assert_eq!(generation(&bytes, "fixture-key")?, "fixture-generation");
    assert!(generation(&bytes, "another-key").is_err());
    let mut noncanonical = vec![b' '];
    noncanonical.extend_from_slice(&bytes);
    assert!(generation(&noncanonical, "fixture-key").is_err());
    for rights in [
        vec!["link.connect"],
        vec!["devices.manage", "link.connect"],
        vec!["devices.delegate", "devices.manage", "link.connect"],
    ] {
        let mut binding = origin("fixture-key");
        binding.policy.delegation = rights.contains(&"devices.delegate");
        binding.policy.rights = rights.into_iter().map(str::to_owned).collect();
        assert!(generation(&serde_json::to_vec(&binding)?, "fixture-key").is_ok());
    }
    for rights in [
        vec![],
        vec!["devices.manage"],
        vec!["devices.delegate", "link.connect"],
        vec!["link.connect", "devices.manage"],
        vec!["link.connect", "link.connect"],
        vec!["Control", "link.connect"],
        vec!["engine.accounts", "link.connect"],
        vec!["owner.devices", "link.connect"],
    ] {
        let mut binding = origin("fixture-key");
        binding.policy.rights = rights.into_iter().map(str::to_owned).collect();
        assert!(generation(&serde_json::to_vec(&binding)?, "fixture-key").is_err());
    }
    for field in ["policyId", "policyVersion", "policyDigest", "rights", "delegation", "rightsRevision"] {
        let mut changed = value.clone();
        changed["policy"].as_object_mut().expect("object").remove(field);
        assert!(serde_json::from_value::<Origin>(changed).is_err(), "missing {field}");
    }
    for (field, wrong) in [
        ("policyId", json!("other")),
        ("policyVersion", json!(2)),
        ("policyDigest", json!("0".repeat(64))),
        ("delegation", json!(true)),
        ("rightsRevision", json!(0)),
        ("rightsRevision", json!(9_007_199_254_740_992_u64)),
    ] {
        let mut changed = value.clone();
        changed["policy"][field] = wrong;
        let parsed: Origin = serde_json::from_value(changed)?;
        assert!(generation(&serde_json::to_vec(&parsed)?, "fixture-key").is_err(), "{field}");
    }
    let mut unknown = value;
    unknown["policy"]["kit"] = json!(true);
    assert!(serde_json::from_value::<Origin>(unknown).is_err());
    let mut delegated = origin("fixture-key");
    delegated.policy.rights = vec!["devices.delegate".into(), "devices.manage".into(), "link.connect".into()];
    assert!(generation(&serde_json::to_vec(&delegated)?, "fixture-key").is_err());
    Ok(())
}

#[tokio::test]
async fn effective_ingresses_refuse_before_vault_even_with_legacy_scopes() -> Result<()> {
    let store = Store::open(Path::new(":memory:"))?;
    *store.secret_key.locked() = None;
    let directory = store.engine_setup_root.clone();
    std::fs::create_dir_all(&directory)?;
    let identity = Identity::load_or_create(&directory)?;
    let hub = crate::hub::Hub::new(store, "fixture-hub".into(), identity, "fixture-token".into(), 0);
    let key = "fixture-key";
    hub.store.put_device(&device(key, DeviceSource::Local))?;
    // Captura real antes de a origem existir: a guarda original deve recusá-la depois.
    let (_, old_caller) = autoridade::autenticar(&hub, key).map_err(|_| anyhow::anyhow!("fixture auth"))?;
    let original = Autoridade::capturar(&hub, &old_caller)?;
    insert(&hub.store, key)?;
    assert!(hub.store.personal_configured()?);
    assert!(hub.store.bootstrap_claim_a1(None, "fixture-claim", |_| panic!("D430 must not sign legacy A1")).is_err());
    assert!(original.with_db::<()>(&hub, None, |_| panic!("origin must refuse before consumer")).is_err());
    assert!(autoridade::reconferir(&hub, &old_caller).is_err());
    let manager_key = "fixture-management-key";
    let mut manager_origin = origin(manager_key);
    manager_origin.policy.rights = vec!["devices.delegate".into(), "devices.manage".into(), "link.connect".into()];
    manager_origin.policy.delegation = true;
    hub.store.db.locked().execute(
        "INSERT INTO personal_link_origins(key,binding) VALUES(?,?)",
        params![manager_key, serde_json::to_vec(&manager_origin)?],
    )?;
    for source in [DeviceSource::Local, DeviceSource::Account] {
        hub.store.put_device(&device(key, source))?;
        hub.store.put_device(&device(manager_key, source))?;
        assert!(devices::authorize(&hub, key).is_err());
        assert!(autoridade::autenticar(&hub, key).is_err());
        for caller in [
            old_caller.clone(),
            Caller::Device { key: key.into(), scopes: vec![Scope::Control, Scope::OwnerDevices], autoridade: None },
            Caller::Device {
                key: manager_key.into(),
                scopes: vec![Scope::Control, Scope::OwnerDevices],
                autoridade: None,
            },
        ] {
            for method in [
                "credentialStatus",
                "credentialLogins",
                "credentialSaveLogin",
                "credentialLabelLogin",
                "credentialRemoveLogin",
                "credentialSetOnePassword",
                "credentialUpdateConnector",
                "credentialUnknownAlias",
                "setAgentEnv",
                "agentSetup",
                "agentAuth",
                "agentAuthenticate",
                "devices",
                "revokeDevice",
                "cloudStatus",
                "teamSettings",
                "donnaEngineAccountCreate",
                "donnaEngineAccountUsage",
                "donnaEngineAccountModelVisibility",
                "donnaEngineSetupStart",
                "donnaEngineSetupAction",
                "chatCall",
                "manageCall",
                "computerCall",
                "connectorCall",
                "memoryCall",
                "session.open",
                "invitation.create",
                "management.grant",
                "kit.generate",
                "kit.recover",
                "unknown",
            ] {
                let result = crate::api::dispatch(
                    &hub,
                    &caller,
                    method,
                    json!({
                        "name":"type_login", "backend":"claude", "method":"credentialSaveLogin",
                        "arguments":{"method":"credentialStatus"}, "rights":["devices.manage"],
                        "scopes":["control","owner.devices"], "origin":"legacy"
                    }),
                )
                .await;
                assert!(result.is_err_and(|e| e.is::<devices::Forbidden>()), "{method}");
            }
            assert!(
                crate::api::scoped_events_stream(&hub, 0, None, &caller, Some("missing-space"))
                    .is_err_and(|e| e.is::<devices::Forbidden>())
            );
            assert!(
                crate::api::term_events(&hub, "missing-terminal", &caller).is_err_and(|e| e.is::<devices::Forbidden>())
            );
        }
    }
    assert_eq!(hub.store.vault_unlock_attempts.load(Ordering::SeqCst), 0);
    assert!(hub.store.secret_key.locked().is_none());
    assert!(hub.store.kv_read("vault.id")?.is_none());
    hub.store.db.locked().execute("UPDATE personal_link_origins SET tombstone=1 WHERE key=?", [key])?;
    hub.store.remove_device(key)?;
    hub.store.put_device(&device(key, DeviceSource::Local))?;
    assert!(autoridade::autenticar(&hub, key).is_err());
    assert!(crate::api::dispatch(&hub, &old_caller, "credentialStatus", json!({})).await.is_err());
    // Falha no registro não libera o fallback de Control nem o consumidor.
    hub.store.db.locked().execute_batch("DROP TABLE personal_link_origins")?;
    assert!(devices::authorize(&hub, key).is_err());
    assert!(autoridade::autenticar(&hub, key).is_err());
    assert!(crate::api::dispatch(&hub, &old_caller, "credentialStatus", json!({})).await.is_err());
    assert_eq!(hub.store.vault_unlock_attempts.load(Ordering::SeqCst), 0);
    drop(hub);
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn ledger_retains_origin_after_delete_tombstone_and_reopen() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("d430-origin-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir)?;
    let path = dir.join("db");
    let store = Store::open(&path)?;
    store.put_device(&device("key", DeviceSource::Local))?;
    insert(&store, "key")?;
    {
        let db = store.db.locked();
        assert!(db.execute("DELETE FROM personal_link_origins", []).is_err());
        assert!(db.execute("UPDATE personal_link_origins SET binding=x'00'", []).is_err());
        assert!(db.execute("UPDATE personal_link_origins SET key='another'", []).is_err());
        assert!(
            db.execute("INSERT OR REPLACE INTO personal_link_origins(key,binding) VALUES('key',x'00')", []).is_err()
        );
    }
    store.remove_device("key")?;
    {
        let db = store.db.locked();
        assert_eq!(db.query_row("SELECT tombstone FROM personal_link_origins", [], |r| r.get::<_, i64>(0))?, 1);
        assert!(db.execute("UPDATE personal_link_origins SET tombstone=0", []).is_err());
    }
    drop(store);
    let store = Store::open(&path)?;
    assert!(store.device("key").is_none());
    assert_eq!(store.personal_origin("key")?.as_deref(), Some("fixture-generation"));
    let evidence = crate::store::personal_bootstrap::preflight(&path)?;
    assert!(evidence.personal && evidence.inconsistent, "D430 must not become PL1 or virgin");
    // Corrupção não precisa passar pelo parser de política para conservar a recusa.
    store.db.locked().execute("INSERT INTO personal_link_origins(key,binding) VALUES('corrupt',x'00')", [])?;
    assert!(store.personal_origin("corrupt").is_err());
    // Registros/wire antigos não são convertidos pela migração.
    assert!(store.personal_origin("legacy-unrelated")?.is_none());
    drop(store);
    std::fs::remove_dir_all(dir)?;
    Ok(())
}
