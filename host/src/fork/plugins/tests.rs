#![allow(clippy::unwrap_used)]
//! Somente fixtures sintéticas e fronteiras descritivas; nenhuma conta ou agenda real.
use super::{
    lifecycle::{LifecycleChange, transition_installation},
    model::{InstallationState, PluginAccountRef, PluginError, PluginInstallationRef, PluginResourceRef},
};
use crate::fork::escopo::Revision;
use serde_json::{Value, json};

fn id(n: u32) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}

fn installation_json() -> Value {
    json!({"version":1,"hubId":id(1),"installationId":id(2),
        "generation":2,"revision":3,"state":"enabled"})
}

fn account_json() -> Value {
    json!({"version":1,"installationRef":installation_json(),"accountId":id(4),
        "generation":4,"revision":5})
}

fn native_json() -> Value {
    json!({"version":1,"installationRef":installation_json(),
        "owner":{"kind":"native","hubId":id(1),"spaceId":id(3)},
        "resourceId":id(5),"kind":"calendar","generation":6,"revision":7})
}

fn external_json() -> Value {
    json!({"version":1,"installationRef":installation_json(),
        "owner":{"kind":"external","accountRef":account_json()},
        "resourceId":id(6),"kind":"calendar","generation":8,"revision":9})
}

fn encoded(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

fn installation() -> PluginInstallationRef {
    PluginInstallationRef::parse_json(&encoded(&installation_json())).unwrap()
}

#[test]
fn descriptive_refs_roundtrip_without_content_or_positive_proof() {
    let raw = installation_json();
    let descriptor = PluginInstallationRef::parse_json(&encoded(&raw)).unwrap();
    assert_eq!(serde_json::to_value(descriptor).unwrap(), raw);
    let raw = account_json();
    let descriptor = PluginAccountRef::parse_json(&encoded(&raw)).unwrap();
    assert_eq!(serde_json::to_value(descriptor).unwrap(), raw);
    for raw in [native_json(), external_json()] {
        let descriptor = PluginResourceRef::parse_json(&encoded(&raw)).unwrap();
        assert_eq!(serde_json::to_value(descriptor).unwrap(), raw);
    }

    // Estado desligado também é descritível, sem ser admissão para uso.
    let mut raw = installation_json();
    raw["state"] = json!("disabled");
    assert_eq!(PluginInstallationRef::parse_json(&encoded(&raw)).unwrap().state, InstallationState::Disabled);
}

#[test]
fn malformed_versions_ids_counters_and_unknown_fields_fail_closed() {
    for (path, value) in [
        ("/version", json!(2)),
        ("/installationRef/version", json!(0)),
        ("/owner/accountRef/version", json!(2)),
        ("/resourceId", json!("resource-label")),
        ("/resourceId", json!("00000000-0000-0000-0000-000000000000")),
        ("/resourceId", json!(id(0x00ab_cdef).to_uppercase())),
        ("/owner/accountRef/accountId", json!("{00000000-0000-4000-8000-000000000004}")),
        ("/revision", json!(0)),
        ("/revision", json!(-1)),
        ("/revision", json!(1.5)),
        ("/generation", json!(9_007_199_254_740_992_u64)),
        ("/owner/accountRef/generation", json!(0)),
        ("/installationRef/revision", json!(0)),
        ("/kind", json!("arbitrary_tool")),
        ("/kind", json!("mailbox")),
        ("/installationRef/state", json!("authorized")),
    ] {
        let mut raw = external_json();
        *raw.pointer_mut(path).unwrap() = value;
        assert_eq!(PluginResourceRef::parse_json(&encoded(&raw)), Err(PluginError::InvalidDescriptor), "{path}");
    }

    // Não aceitar permissão/corpo/credencial nem nas estruturas aninhadas.
    for path in ["", "/installationRef", "/owner", "/owner/accountRef", "/owner/accountRef/installationRef"] {
        let mut raw = external_json();
        raw.pointer_mut(path).unwrap().as_object_mut().unwrap().insert("approved".into(), json!(true));
        assert_eq!(PluginResourceRef::parse_json(&encoded(&raw)), Err(PluginError::InvalidDescriptor));
    }
    let mut missing = installation_json();
    missing.as_object_mut().unwrap().remove("version");
    assert_eq!(PluginInstallationRef::parse_json(&encoded(&missing)), Err(PluginError::InvalidDescriptor));
}

#[test]
fn native_and_external_owners_cannot_mix_installations_or_hubs() {
    let mut raw = native_json();
    raw["owner"]["hubId"] = json!(id(90));
    assert_eq!(PluginResourceRef::parse_json(&encoded(&raw)), Err(PluginError::InvalidDescriptor));
    let mut raw = native_json();
    raw["owner"]["accountRef"] = account_json();
    assert!(PluginResourceRef::parse_json(&encoded(&raw)).is_err());

    for (key, value) in [("hubId", json!(id(90))), ("installationId", json!(id(91))), ("generation", json!(3))] {
        let mut raw = external_json();
        raw["owner"]["accountRef"]["installationRef"][key] = value;
        assert_eq!(PluginResourceRef::parse_json(&encoded(&raw)), Err(PluginError::InvalidDescriptor));
    }
    let mut raw = external_json();
    raw["owner"]["spaceId"] = json!(id(3));
    assert!(PluginResourceRef::parse_json(&encoded(&raw)).is_err());
}

#[test]
fn replacing_account_resource_or_space_is_a_different_description() {
    let raw = external_json();
    let original = PluginResourceRef::parse_json(&encoded(&raw)).unwrap();
    for (path, replacement) in [
        ("/owner/accountRef/accountId", json!(id(90))),
        ("/owner/accountRef/generation", json!(5)),
        ("/owner/accountRef/revision", json!(6)),
        ("/resourceId", json!(id(91))),
        ("/generation", json!(9)),
        ("/revision", json!(10)),
    ] {
        let mut changed = raw.clone();
        *changed.pointer_mut(path).unwrap() = replacement;
        assert_ne!(original, PluginResourceRef::parse_json(&encoded(&changed)).unwrap(), "{path}");
    }
    let mut raw = native_json();
    let original = PluginResourceRef::parse_json(&encoded(&raw)).unwrap();
    raw["owner"]["spaceId"] = json!(id(92));
    assert_ne!(original, PluginResourceRef::parse_json(&encoded(&raw)).unwrap());
}

#[test]
fn disable_and_reenable_fence_old_refs_without_restoring_permissions() {
    let enabled = installation();
    let account = PluginAccountRef::parse_json(&encoded(&account_json())).unwrap();
    let resource = PluginResourceRef::parse_json(&encoded(&external_json())).unwrap();
    let disabled = transition_installation(&enabled, &enabled, LifecycleChange::Disable).unwrap();
    assert_eq!(disabled.state, InstallationState::Disabled);
    assert_eq!(u64::from(disabled.revision), 4);
    assert_eq!(u64::from(disabled.generation), 3);
    assert_eq!(
        transition_installation(&disabled, &enabled, LifecycleChange::Enable),
        Err(PluginError::SnapshotConflict)
    );
    let reenabled = transition_installation(&disabled, &disabled, LifecycleChange::Enable).unwrap();
    assert_eq!(reenabled.state, InstallationState::Enabled);
    assert_eq!(u64::from(reenabled.revision), 5);
    assert_eq!(u64::from(reenabled.generation), 4);
    assert_ne!(account.installation_ref, reenabled);
    assert_ne!(resource.installation_ref(), &reenabled);
    assert_eq!(
        transition_installation(&reenabled, &enabled, LifecycleChange::Disable),
        Err(PluginError::SnapshotConflict)
    );
    // Nenhum campo pode carregar uma concessão/cartão/fila para a reativação.
    let mut forged = serde_json::to_value(&reenabled).unwrap();
    forged["grants"] = json!([{"approved":true}]);
    assert!(PluginInstallationRef::parse_json(&encoded(&forged)).is_err());
    assert_eq!(enabled, installation());
}

#[test]
fn cas_checks_full_snapshot_and_rejects_repeated_transition() {
    let snapshot = installation();
    for (key, value) in [
        ("hubId", json!(id(90))),
        ("installationId", json!(id(91))),
        ("revision", json!(4)),
        ("generation", json!(3)),
        ("state", json!("disabled")),
    ] {
        let mut raw = installation_json();
        raw[key] = value;
        let expected = PluginInstallationRef::parse_json(&encoded(&raw)).unwrap();
        assert_eq!(
            transition_installation(&snapshot, &expected, LifecycleChange::Disable),
            Err(PluginError::SnapshotConflict)
        );
    }
    assert_eq!(
        transition_installation(&snapshot, &snapshot, LifecycleChange::Enable),
        Err(PluginError::InvalidTransition)
    );
    let disabled = transition_installation(&snapshot, &snapshot, LifecycleChange::Disable).unwrap();
    assert_eq!(
        transition_installation(&disabled, &disabled, LifecycleChange::Disable),
        Err(PluginError::InvalidTransition)
    );
    assert_eq!(snapshot, installation());
}

#[test]
fn counter_exhaustion_never_wraps_or_returns_partial_snapshot() {
    let max = 9_007_199_254_740_991_u64;
    for exhausted in ["revision", "generation"] {
        let mut raw = installation_json();
        raw[exhausted] = json!(max);
        let snapshot = PluginInstallationRef::parse_json(&encoded(&raw)).unwrap();
        assert_eq!(
            transition_installation(&snapshot, &snapshot, LifecycleChange::Disable),
            Err(PluginError::CounterExhausted)
        );
        assert_eq!(serde_json::to_value(snapshot).unwrap(), raw);
    }
    let mut snapshot = installation();
    snapshot.revision = Revision::try_from(max - 1).unwrap();
    snapshot.generation = Revision::try_from(max - 1).unwrap();
    let last = transition_installation(&snapshot, &snapshot, LifecycleChange::Disable).unwrap();
    assert_eq!(u64::from(last.revision), max);
    assert_eq!(u64::from(last.generation), max);
    assert_eq!(transition_installation(&last, &last, LifecycleChange::Enable), Err(PluginError::CounterExhausted));
}

#[test]
fn explicit_parsers_expose_only_static_errors() {
    let private_input = br#"{"SENTINELA_PRIVADA":"conteudo que nao pode sair"}"#;
    let errors = [
        PluginInstallationRef::parse_json(private_input).unwrap_err(),
        PluginAccountRef::parse_json(private_input).unwrap_err(),
        PluginResourceRef::parse_json(private_input).unwrap_err(),
        PluginResourceRef::parse_json(&vec![b'x'; 8193]).unwrap_err(),
    ];
    for error in errors {
        assert_eq!(error, PluginError::InvalidDescriptor);
        assert_eq!(error.to_string(), "plugin_invalid_descriptor");
        assert_eq!(format!("{error:?}"), "InvalidDescriptor");
        assert!(std::error::Error::source(&error).is_none());
    }
}
