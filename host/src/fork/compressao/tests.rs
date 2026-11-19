#![allow(clippy::unwrap_used)]
use super::*;
use crate::{hub::Hub, remote::identity::Identity};
use serde_json::json;

#[tokio::test]
async fn pause_is_per_visible_conversation_persistent_and_resets_only_explicitly() {
    let dir = std::env::temp_dir().join(format!("compaction-failure-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("store.db");
    let a = Lane::main("a");
    let extra = Lane { chat: "a".into(), session: Some("extra".into()) };
    {
        let store = Store::open(&path).unwrap();
        failed(&store, "a", &a).unwrap();
        assert!(!failures(&store, "a", &a).unwrap().paused());
        failed(&store, "a", &a).unwrap();
        assert!(failures(&store, "a", &a).unwrap().paused());
        assert!(!failures(&store, "a", &extra).unwrap().paused());
        assert!(!failures(&store, "b", &Lane::main("b")).unwrap().paused());
    }
    let store = Store::open(&path).unwrap();
    assert!(failures(&store, "a", &a).unwrap().paused(), "reinício conserva a pausa");
    reset(&store, "a", &a).unwrap();
    assert_eq!(failures(&store, "a", &a).unwrap().consecutive, 0);
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn hub_settings_require_owner_validate_floor_and_keep_default_rule() {
    let dir = std::env::temp_dir().join(format!("compaction-settings-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let hub = Hub::new(store, "hub".into(), Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
    let denied = Caller::Device { key: "device".into(), scopes: vec![], autoridade: None };
    for method in METHODS {
        assert!(call(&hub, &denied, method, json!({})).is_err());
    }
    let control = Caller::Device { key: "owner".into(), scopes: vec![crate::store::Scope::Control], autoridade: None };
    assert!(call(&hub, &control, METHODS[0], json!({})).is_ok());
    assert!(!crate::fork::ponte_credencial::PERMITIDOS.iter().any(|m| METHODS.contains(m)));
    let defaults = call(&hub, &Caller::Local, METHODS[0], json!({})).unwrap();
    assert_eq!(defaults, json!({"triggerTokens":null,"idle":"beforeExpiry"}));
    for n in [0, 49_000, 50_001] {
        assert!(call(&hub, &Caller::Local, METHODS[1], json!({"triggerTokens":n,"idle":"never"})).is_err());
    }
    let chosen = json!({"triggerTokens":60_000,"idle":"never"});
    assert_eq!(call(&hub, &Caller::Local, METHODS[1], chosen.clone()).unwrap(), chosen);
    assert_eq!(settings(&hub.store).unwrap().trigger_tokens, Some(60_000));
    let c = codync_mem::compaction::Conversation { size: 200_000, trigger_tokens: Some(60_000), ..Default::default() };
    assert_eq!(c.trigger(), 60_000);
    assert_eq!(codync_mem::compaction::Conversation { trigger_tokens: None, ..c }.trigger(), 150_000);
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}
