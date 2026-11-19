#![allow(clippy::unwrap_used)]
use super::*;
use crate::store::Store;
use serde_json::json;

#[tokio::test]
async fn exact_engine_numbers_missing_size_and_same_bot_session_hub_epoch_without_fallback() {
    let dir = std::env::temp_dir().join(format!("context-scope-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let hub = Hub::new(
        store,
        "hub-a".into(),
        crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
        "fixture".into(),
        0,
    );
    let cfg: BotConfig = serde_json::from_value(json!({"id":"b","name":"B","backend":"claude"})).unwrap();
    let lane = Lane::main("b");
    let target = Target {
        bot: "b".into(),
        hub_id: hub.host_id.clone(),
        space_id: crate::fork::spaces::PERSONAL.into(),
        epoch: 0,
        lane: lane.clone(),
        acp: Some("acp".into()),
        engine: engine(&cfg),
    };
    observe(&hub, &cfg, &lane, "acp", true, Some((84_321, 220_004)), None).unwrap();
    let answer = projection(&target, load(&hub, "b", "acp").unwrap(), false).unwrap();
    assert_eq!(answer["hubId"], "hub-a");
    assert_eq!(answer["botId"], "b");
    assert_eq!(answer["sessionId"], "b");
    assert_eq!(answer["spaceId"], crate::fork::spaces::PERSONAL);
    assert_eq!(answer["epoch"], 0);
    assert_eq!(answer["context"]["used"], 84_321);
    assert_eq!(answer["context"]["size"], 220_004);
    assert_eq!(
        answer["context"]["trigger"],
        codync_mem::compaction::Conversation { size: 220_004, ..Default::default() }.trigger()
    );
    assert!(answer["context"]["updatedAt"].as_i64().unwrap() > 0);
    // #2: Hub e conversa visível são cercas da publicação; o índice também separa bot e ACP.
    let other_hub = Target { hub_id: "hub-b".into(), ..target.clone() };
    assert!(projection(&other_hub, load(&hub, "b", "acp").unwrap(), false).unwrap().get("context").is_none());
    assert!(load(&hub, "other-bot", "acp").unwrap().is_none());
    assert!(load(&hub, "b", "other-acp").unwrap().is_none());
    // #3: extra sem medida não reaproveita a principal, nem mesmo se alguém fornecer a observação antiga.
    let extra = Target { lane: Lane { chat: "b".into(), session: Some("extra".into()) }, ..target.clone() };
    assert!(projection(&extra, None, false).unwrap().get("context").is_none());
    assert!(projection(&extra, load(&hub, "b", "acp").unwrap(), false).unwrap().get("context").is_none());
    // #2: uma compressão invalida a medida anterior; somente novo usage_update da época atual volta a publicar.
    crate::chat::context::bump_epoch(&hub.store, cfg.session_owner(), "acp", "compaction-1").unwrap();
    let current = Target { epoch: epoch(&hub, &lane.chat, "acp").unwrap(), ..target.clone() };
    assert_eq!(current.epoch, 1);
    assert_ne!(current.context_id().unwrap(), target.context_id().unwrap());
    assert_eq!(current.scope_id().unwrap(), target.scope_id().unwrap());
    observe(&hub, &cfg, &lane, "acp", true, None, Some(Compaction { at: 123, by_host: true })).unwrap();
    assert!(projection(&current, load(&hub, "b", "acp").unwrap(), false).unwrap().get("context").is_none());
    observe(&hub, &cfg, &lane, "acp", true, Some((20_001, 220_004)), None).unwrap();
    let after = projection(&current, load(&hub, "b", "acp").unwrap(), false).unwrap();
    assert_eq!(after["epoch"], 1);
    assert_eq!(after["context"]["used"], 20_001);
    assert_eq!(after["context"]["lastCompaction"]["byHost"], true);
    observe(&hub, &cfg, &lane, "acp", true, Some((20_001, 0)), None).unwrap();
    assert!(projection(&current, load(&hub, "b", "acp").unwrap(), false).unwrap().get("context").is_none());
    // R2-01: Nova sessão troca a ACP, sem mudar a conversa visível; a época recomeça.
    let fresh = Target { acp: Some("fresh-acp".into()), epoch: 0, ..current.clone() };
    assert_ne!(fresh.scope_id().unwrap(), current.scope_id().unwrap());
    let empty = projection(&fresh, load(&hub, "b", "fresh-acp").unwrap(), false).unwrap();
    assert_eq!(empty["scopeId"], fresh.scope_id().unwrap());
    assert_eq!(empty["epoch"], 0);
    assert!(empty.get("context").is_none());
    observe(&hub, &cfg, &lane, "fresh-acp", true, Some((101, 220_004)), None).unwrap();
    let measured = projection(&fresh, load(&hub, "b", "fresh-acp").unwrap(), false).unwrap();
    assert_eq!(measured["scopeId"], empty["scopeId"]);
    assert_eq!(measured["context"]["used"], 101);
    assert_eq!(measured["epoch"], 0);
    for caller in
        [Caller::Pairing { key: "p".into() }, Caller::Device { key: "d".into(), scopes: vec![], autoridade: None }]
    {
        assert!(devices::permit(&caller, "send").is_err());
        for method in METHODS {
            assert!(devices::permit(&caller, method).is_err());
        }
    }
    assert!(!crate::fork::ponte_credencial::PERMITIDOS.iter().any(|m| METHODS.contains(m)));
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}
