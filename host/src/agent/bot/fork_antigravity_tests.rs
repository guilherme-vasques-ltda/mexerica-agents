use super::*;
use crate::store::{Device, DeviceSource, EntryKind, Scope, Store, now_ms};

#[tokio::test]
async fn antigravity_encerra_processo_ao_revogar_dono_motor_ou_modo() {
    for cause in ["device", "owner", "engine", "mode"] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path();
        let cfg: BotConfig =
            serde_json::from_value(json!({"id":"bot","name":"Bot","backend":"antigravity","cwd":path})).unwrap();
        let store = Store::open(&path.join("db")).unwrap();
        store.save_bot(&cfg).unwrap();
        let hub = Hub::new(
            store,
            "fixture".into(),
            crate::remote::identity::Identity::load_or_create(path).unwrap(),
            "fixture".into(),
            0,
        );
        crate::remote::cloud::update_status(&hub, |status| {
            status.owner = Some(crate::remote::cloud::Owner { user_id: "owner".into(), email: None });
        });
        hub.cloud_synced.store(true, std::sync::atomic::Ordering::Release);
        let device = Device {
            key: "fixture".into(),
            name: "Fixture".into(),
            platform: "desktop".into(),
            source: DeviceSource::Account,
            grant_id: Some("grant".into()),
            scopes: vec![Scope::Control, Scope::OwnerDevices],
            lease_until: Some(now_ms() + 60_000),
            created_at: now_ms(),
            last_seen_at: None,
        };
        hub.store.put_device(&device).unwrap();
        let (_, caller) = crate::fork::autoridade::autenticar(&hub, &device.key).unwrap();
        let owner = crate::fork::antigravity::authenticated_owner(&hub, &caller).unwrap();
        let lane = Lane::main("bot");
        let entry = hub.add_entry(&lane, EntryKind::User, 1, &json!({"text":"fixture","status":"queued"})).unwrap();
        hub.antigravity_grants.insert(owner, lane, &entry);
        let mut actor = new_actor(hub.clone(), cfg.clone());
        let script = path.join("fixture.py");
        std::fs::write(&script, "import json,sys\nfor line in sys.stdin:\n m=json.loads(line)\n if 'id' in m: print(json.dumps(dict(jsonrpc='2.0', id=m['id'], result={})), flush=True)\n").unwrap();
        actor.conn = Some(
            launch::start_agent(&crate::shell::python(&script), path.to_str().unwrap(), &[], Duration::from_secs(5))
                .await
                .unwrap(),
        );
        let acp = actor.conn.as_ref().unwrap().acp.clone();
        actor.turn_session = Some("fixture".into());
        actor.antigravity_active = hub.antigravity_grants.take(&entry.id);
        assert!(actor.antigravity_active.as_ref().unwrap().current(&hub, &cfg));
        match cause {
            "device" => { hub.revoke_device(&device.key).unwrap(); }
            "owner" => crate::remote::cloud::update_status(&hub, |status| { status.owner = None; }),
            "engine" => {
                let mut updated = cfg.clone();
                updated.backend = "codex".into();
                hub.store.save_bot(&updated).unwrap();
                assert!(actor.antigravity_active.as_ref().unwrap().current(&hub, &cfg));
                let mut selection = crate::fork::sessoes::engine_choice::SessionEngine::snapshot(&updated);
                selection.source = crate::fork::sessoes::engine_choice::EngineSource::Own;
                let changed = hub.store.db.locked().execute(
                    "UPDATE donna_bot_sessions SET engine_next=?1 WHERE id='bot'",
                    [serde_json::to_string(&selection).unwrap()],
                ).unwrap();
                assert_eq!(changed, 1);
            }
            _ => actor.on_incoming(Some(Incoming::Notification { method: "session/update".into(),
                params: json!({"sessionId":"fixture","update":{"sessionUpdate":"current_mode_update","currentModeId":"auto"}}) })).await,
        }
        actor.check_antigravity_turn().await;
        assert!(actor.conn.is_none() && actor.antigravity_active.is_none(), "{cause}");
        assert!(acp.notify("fixture", json!({})).await.is_err(), "{cause}");
    }
}
