#![allow(clippy::unwrap_used)]

use super::{EVENT, READ, SAVE, call, call_with_commit_boundary, event, model::ArchiveState, store};
use crate::{
    LockExt,
    api::devices::Caller,
    fork::spaces::PERSONAL,
    hub::{BotStatus, Hub},
    remote::identity::Identity,
    store::{Device, DeviceSource, Scope, Store, now_ms},
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};
use uuid::Uuid;

struct Fixture {
    hub: Arc<Hub>,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let hub = Hub::new(
            Store::open(Path::new(":memory:")).unwrap(),
            "governanca".into(),
            Identity::load_or_create(dir.path()).unwrap(),
            "fixture".into(),
            0,
        );
        Self { hub, _dir: dir }
    }

    async fn read(&self) -> Value {
        call(&self.hub, &Caller::Local, READ, self.coords()).await.unwrap()
    }

    fn coords(&self) -> Value {
        json!({"hubId":self.hub.host_id,"spaceId":PERSONAL})
    }

    fn request(&self, revision: i64, policy: Value) -> Value {
        let mut request = self.coords();
        request["expectedRevision"] = json!(revision);
        request["clientRequestId"] = json!(Uuid::new_v4().to_string());
        request["policy"] = policy;
        request
    }

    async fn save(&self, request: Value) -> anyhow::Result<Value> {
        call(&self.hub, &Caller::Local, SAVE, request).await
    }

    fn control(&self, key: &str) -> Caller {
        self.hub
            .store
            .put_device(&Device {
                key: key.into(),
                name: "fixture".into(),
                platform: "fixture".into(),
                source: DeviceSource::Local,
                grant_id: None,
                scopes: vec![Scope::Control],
                lease_until: None,
                created_at: now_ms(),
                last_seen_at: None,
            })
            .unwrap();
        crate::fork::autoridade::autenticar(&self.hub, key).unwrap().1
    }
}

#[tokio::test]
async fn policy_defaults_validation_and_fail_closed_storage() {
    let fixture = Fixture::new();
    let current = fixture.read().await;
    assert_eq!(
        current,
        json!({
            "version":1,"hubId":fixture.hub.host_id,"spaceId":PERSONAL,"revision":0,
            "policy":{"titleInstruction":"","autoArchiveIdleDays":3,"preservePinnedAndOpenWork":true,
                "maxConcurrentSubagents":12,"maxSubagentDepth":3}
        })
    );
    for (key, value) in [
        ("titleInstruction", json!("á".repeat(501))),
        ("autoArchiveIdleDays", json!(-1)),
        ("autoArchiveIdleDays", json!(366)),
        ("preservePinnedAndOpenWork", json!(false)),
        ("preservePinnedAndOpenWork", json!("false")),
        ("maxConcurrentSubagents", json!(0)),
        ("maxConcurrentSubagents", json!(33)),
        ("maxSubagentDepth", json!(0)),
        ("maxSubagentDepth", json!(2.5)),
        ("maxSubagentDepth", json!(6)),
        ("inventado", json!(true)),
    ] {
        let mut request = fixture.request(0, current["policy"].clone());
        request["policy"][key] = value;
        assert!(fixture.save(request).await.unwrap_err().to_string().contains("invalid_field"), "{key}");
    }
    for key in ["policy", "expectedRevision", "clientRequestId"] {
        let mut request = fixture.request(0, current["policy"].clone());
        request.as_object_mut().unwrap().remove(key);
        assert!(fixture.save(request).await.unwrap_err().to_string().contains("invalid_field"), "{key}");
    }
    assert_eq!(fixture.read().await, current);
    let mut policy = current["policy"].clone();
    policy["titleInstruction"] = json!("á".repeat(500));
    policy["autoArchiveIdleDays"] = json!(365);
    policy["maxConcurrentSubagents"] = json!(32);
    policy["maxSubagentDepth"] = json!(5);
    assert_eq!(fixture.save(fixture.request(0, policy.clone())).await.unwrap()["snapshot"]["policy"], policy);

    let db = fixture.hub.store.db.locked();
    store::migrate(&db).unwrap();
    assert_eq!(store::read_policy(&db, &fixture.hub.host_id, PERSONAL).unwrap().max_subagent_depth, 5);
    assert_eq!(store::read_policy(&db, "outro-hub", PERSONAL).unwrap(), store::GovernancePolicy::default());
    assert!(store::read_policy(&db, &fixture.hub.host_id, "inexistente").is_err());
    db.execute("UPDATE donna_governance_policies SET policy=json_set(policy, '$.maxSubagentDepth', 0)", []).unwrap();
    assert!(store::read_policy(&db, &fixture.hub.host_id, PERSONAL).is_err());
    db.execute("DROP TABLE donna_governance_policies", []).unwrap();
    assert!(store::read_policy(&db, &fixture.hub.host_id, PERSONAL).is_err());
}

#[tokio::test]
async fn receipts_survive_revisions_and_never_emit_again() {
    let fixture = Fixture::new();
    let mut events = fixture.hub.events.subscribe();
    let mut policy = fixture.read().await["policy"].clone();
    let unchanged = fixture.save(fixture.request(0, policy.clone())).await.unwrap();
    assert_eq!(unchanged["receipt"]["revision"], 0);
    assert!(events.try_recv().is_err());

    policy["titleInstruction"] = json!("Primeira política");
    let initial = fixture.request(0, policy.clone());
    let first = fixture.save(initial.clone()).await.unwrap();
    assert_eq!(first["snapshot"]["revision"], 1);
    assert_eq!(first["receipt"], json!({"clientRequestId":initial["clientRequestId"],"revision":1}));
    let hint = json!({"type":EVENT,"hubId":fixture.hub.host_id,"spaceId":PERSONAL});
    assert_eq!(events.try_recv().unwrap(), hint);

    policy["titleInstruction"] = json!("Segunda política");
    let stale = fixture.save(fixture.request(0, policy.clone())).await.unwrap_err();
    assert_eq!(stale.downcast_ref::<crate::remote::cloud::Conflict>().unwrap().0, "revision_conflict");
    assert!(events.try_recv().is_err());
    let changed = fixture.save(fixture.request(1, policy.clone())).await.unwrap();
    assert_eq!(changed["snapshot"]["revision"], 2);
    assert_eq!(events.try_recv().unwrap(), hint);

    let replay = fixture.save(initial.clone()).await.unwrap();
    assert_eq!(replay, first);
    assert_eq!(fixture.read().await, changed["snapshot"]);
    let mut reused = initial;
    reused["policy"] = policy;
    assert_eq!(
        fixture.save(reused).await.unwrap_err().downcast_ref::<crate::remote::cloud::Conflict>().unwrap().0,
        "idempotency_conflict"
    );
    assert!(events.try_recv().is_err());
}

#[tokio::test]
async fn authority_and_event_boundaries() {
    let fixture = Fixture::new();
    let hub = &fixture.hub;
    let mut wrong_hub = fixture.coords();
    wrong_hub["hubId"] = json!("outro-hub");
    assert!(call(hub, &Caller::Local, READ, wrong_hub).await.is_err());
    let policy = fixture.read().await["policy"].clone();
    let mut hint = json!({"type":EVENT,"hubId":hub.host_id,"spaceId":PERSONAL,"policy":policy});
    assert_eq!(
        event(hub, &Caller::Local, PERSONAL, &hint),
        Some(json!({
            "type":EVENT,"hubId":hub.host_id,"spaceId":PERSONAL
        }))
    );
    assert!(event(hub, &Caller::Local, "outro-space", &hint).is_none());
    hint["hubId"] = json!("outro-hub");
    assert!(event(hub, &Caller::Local, PERSONAL, &hint).is_none());
    hint["hubId"] = json!(hub.host_id);
    for scopes in [vec![], vec![Scope::Control]] {
        let device = Caller::Device { key: "sem-autorizacao".into(), scopes, autoridade: None };
        assert!(event(hub, &device, PERSONAL, &hint).is_none());
        assert!(call(hub, &device, READ, fixture.coords()).await.is_err());
        assert!(call(hub, &device, SAVE, fixture.request(0, policy.clone())).await.is_err());
    }
}

#[tokio::test]
async fn current_control_can_save_but_revocation_before_write_stops_effect_and_event() {
    let fixture = Fixture::new();
    let hub = &fixture.hub;
    let device = Device {
        key: "controle".into(),
        name: "fixture".into(),
        platform: "fixture".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: now_ms(),
        last_seen_at: None,
    };
    hub.store.put_device(&device).unwrap();
    let (_, caller) = crate::fork::autoridade::autenticar(hub, &device.key).unwrap();
    let mut events = hub.events.subscribe();
    let mut policy = fixture.read().await["policy"].clone();
    policy["maxSubagentDepth"] = json!(2);
    let accepted = call(hub, &caller, SAVE, fixture.request(0, policy.clone())).await.unwrap();
    assert_eq!(accepted["snapshot"]["revision"], 1);
    let hint = events.try_recv().unwrap();
    assert_eq!(event(hub, &caller, PERSONAL, &hint), Some(hint.clone()));

    let owned_hub = hub.clone();
    *hub.store.before_owner_write.locked() = Some(Box::new(move || {
        assert!(owned_hub.revoke_device(&device.key).unwrap());
    }));
    policy["maxSubagentDepth"] = json!(1);
    assert!(call(hub, &caller, SAVE, fixture.request(1, policy)).await.is_err());
    assert_eq!(fixture.read().await, accepted["snapshot"]);
    assert!(event(hub, &caller, PERSONAL, &hint).is_none());
    while let Ok(input) = events.try_recv() {
        assert_ne!(input["type"], EVENT);
    }
}

#[tokio::test]
async fn committed_change_reaches_current_control_even_when_writer_loses_reply() {
    let fixture = Fixture::new();
    let hub = &fixture.hub;
    let writer = fixture.control("solicitante");
    let reader = fixture.control("destinatario");
    let mut events = hub.events.subscribe();

    let mut policy = fixture.read().await["policy"].clone();
    policy["titleInstruction"] = json!("Título da equipe");
    let request = fixture.request(0, policy.clone());
    let owned_hub = hub.clone();
    let err = call_with_commit_boundary(hub, &writer, SAVE, request.clone(), move || {
        {
            let db = owned_hub.store.db.locked();
            assert!(db.is_autocommit());
            assert_eq!(store::snapshot(&db, &owned_hub.host_id, PERSONAL).unwrap().revision, 1);
        }
        assert!(owned_hub.revoke_device("solicitante").unwrap());
    })
    .await
    .unwrap_err();
    assert!(err.is::<crate::fork::autoridade::AutorizacaoEncerrada>(), "{err:#}");

    let current = fixture.read().await;
    assert_eq!(current["policy"], policy);
    assert_eq!(
        call(hub, &reader, SAVE, request.clone()).await.unwrap(),
        json!({
            "snapshot": current, "receipt": {"clientRequestId":request["clientRequestId"],"revision":1}
        })
    );
    let hint = events.try_recv().unwrap();
    assert_eq!(hint, json!({"type":EVENT,"hubId":hub.host_id,"spaceId":PERSONAL}));
    assert_eq!(event(hub, &reader, PERSONAL, &hint), Some(hint.clone()));
    assert!(event(hub, &writer, PERSONAL, &hint).is_none());
    assert!(event(hub, &reader, "outro-space", &hint).is_none());
    assert!(matches!(events.try_recv(), Err(tokio::sync::broadcast::error::TryRecvError::Empty)));
}

#[test]
fn archive_requires_known_idle_state_and_never_overrides_protections() {
    let state = || ArchiveState {
        idle_days: 3,
        status: BotStatus::Idle,
        pinned: false,
        has_open_work: false,
        in_review: false,
    };
    let mut policy = store::GovernancePolicy::default();
    assert!(!policy.can_auto_archive(None));
    assert!(policy.can_auto_archive(Some(state())));
    for blocked in [
        ArchiveState { idle_days: 2, ..state() },
        ArchiveState { pinned: true, ..state() },
        ArchiveState { status: BotStatus::Working, ..state() },
        ArchiveState { status: BotStatus::NeedsInput, ..state() },
        ArchiveState { status: BotStatus::Waiting, ..state() },
        ArchiveState { has_open_work: true, ..state() },
        ArchiveState { in_review: true, ..state() },
    ] {
        assert!(!policy.can_auto_archive(Some(blocked)));
    }
    policy.auto_archive_idle_days = 0;
    assert!(!policy.can_auto_archive(Some(state())));
}
