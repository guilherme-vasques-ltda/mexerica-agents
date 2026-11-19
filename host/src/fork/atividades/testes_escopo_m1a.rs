#![allow(clippy::unwrap_used)]
use super::{
    escopo_migracao,
    escopo_store::{self, PersonalBinding},
};
use crate::{
    api::devices::Caller,
    fork::{
        escopo::{
            Revision,
            fontes::{ScopeSourceError as E, SourceInvalidationReason as Reason},
        },
        spaces::PERSONAL,
    },
    hub::Hub,
    remote::identity::Identity,
    store::Store,
};
use serde_json::json;
use std::path::Path;

fn rev(n: i64) -> Revision {
    Revision::try_from(u64::try_from(n).unwrap()).unwrap()
}
fn fixture() -> (std::sync::Arc<Hub>, PersonalBinding, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("d171-m1a-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir.join("fixture.db")).unwrap();
    let hub_id = "00000000-0000-4000-8000-000000000011";
    let bot = "00000000-0000-4000-8000-000000000012";
    store.kv_set("host_id", hub_id).unwrap();
    let cfg = serde_json::from_value(json!({"id":bot,"name":"fixture","cwd":"/tmp","backend":"claude"})).unwrap();
    store.save_bot(&cfg).unwrap();
    store.kv_set("team.settings.v1", &json!({"primaryBotId":bot}).to_string()).unwrap();
    let sr = store.spaces().unwrap().spaces[0].revision;
    let cr = store.space_conversation(PERSONAL, bot).unwrap().revision;
    let br: i64 = store.db.lock().unwrap().query_row("SELECT rev FROM bots WHERE id=?1", [bot], |r| r.get(0)).unwrap();
    let hub = Hub::new(store, hub_id.into(), Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
    (
        hub,
        PersonalBinding {
            hub_id: hub_id.into(),
            space_id: PERSONAL.into(),
            bot_id: bot.into(),
            conversation_id: bot.into(),
            space_revision: rev(sr),
            bot_revision: rev(br),
            conversation_revision: rev(cr),
            privacy_domain_id: PERSONAL.into(),
            domain_revision: rev(sr),
            primary: true,
        },
        dir,
    )
}
fn denial(h: &Hub, b: &PersonalBinding) -> E {
    match escopo_store::personal_sources(h, &Caller::Local, b) {
        Ok(x) => x.denial(),
        Err(e) => e,
    }
}
fn barrier(h: &Hub) -> i64 {
    h.store.db.lock().unwrap().query_row("SELECT revision FROM donna_scope_authority_state", [], |r| r.get(0)).unwrap()
}
fn global(h: &Hub) -> i64 {
    h.store.db.lock().unwrap().query_row("SELECT CAST(v AS INTEGER) FROM kv WHERE k='rev'", [], |r| r.get(0)).unwrap()
}

#[tokio::test]
async fn m1a_migration_reopen_and_no_reseed() {
    let (h, b, dir) = fixture();
    assert_eq!(denial(&h, &b), E::SourcesUninitialized);
    let old = barrier(&h);
    h.store.kv_set("fixture.legacy", "preserved").unwrap();
    drop(h);
    let s = Store::open(&dir.join("fixture.db")).unwrap();
    assert_eq!(s.kv_get("fixture.legacy").as_deref(), Some("preserved"));
    assert_eq!(
        s.db.lock()
            .unwrap()
            .query_row("SELECT revision FROM donna_scope_authority_state", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        old
    );
    s.db.lock().unwrap().execute("DELETE FROM donna_scope_authority_state", []).unwrap();
    drop(s);
    let s = Store::open(&dir.join("fixture.db")).unwrap();
    assert_eq!(
        s.db.lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM donna_scope_authority_state", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(s);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn m1a_binding_and_ingress_fail_closed() {
    let (h, mut b, dir) = fixture();
    assert_eq!(denial(&h, &b), E::SourcesUninitialized);
    b.hub_id = "00000000-0000-4000-8000-000000000099".into();
    assert_eq!(denial(&h, &b), E::ContextChanged);
    assert!(matches!(
        escopo_store::personal_sources(&h, &Caller::Pairing { key: "fixture".into() }, &b),
        Err(E::IngressDenied)
    ));
    assert!(matches!(
        escopo_store::personal_sources(
            &h,
            &Caller::Device { key: "fixture".into(), scopes: vec![], autoridade: None },
            &b,
        ),
        Err(E::IngressDenied)
    ));
    b.hub_id = h.host_id.clone();
    b.bot_revision = rev(i64::try_from(u64::from(b.bot_revision)).unwrap() + 1);
    assert_eq!(denial(&h, &b), E::ContextChanged);
    h.store.kv_set("donna.spaces.v1", "{").unwrap();
    assert_eq!(denial(&h, &b), E::OutcomeUnknown);
    drop(h);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn m1a_cas_and_fault_rollback() {
    let (h, b, dir) = fixture();
    let old = barrier(&h);
    let g = global(&h);
    h.store.db.lock().unwrap().execute_batch("CREATE TRIGGER m1a_fault BEFORE UPDATE ON donna_scope_authority_state BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert_eq!(
        escopo_store::invalidate_personal_sources(&h, &Caller::Local, &b, rev(old), Reason::PolicyChanged),
        Err(E::OutcomeUnknown)
    );
    assert_eq!((barrier(&h), global(&h)), (old, g));
    h.store.db.lock().unwrap().execute_batch("DROP TRIGGER m1a_fault").unwrap();
    let next =
        escopo_store::invalidate_personal_sources(&h, &Caller::Local, &b, rev(old), Reason::PolicyChanged).unwrap();
    assert!(u64::from(next) > u64::try_from(old).unwrap());
    let g = global(&h);
    assert_eq!(
        escopo_store::invalidate_personal_sources(&h, &Caller::Local, &b, rev(old), Reason::PolicyChanged),
        Err(E::StateConflict)
    );
    assert_eq!(global(&h), g);
    assert_eq!(denial(&h, &b), E::SourcesInvalidated);
    drop(h);
    let s = Store::open(&dir.join("fixture.db")).unwrap();
    assert_eq!(
        s.db.lock()
            .unwrap()
            .query_row("SELECT state FROM donna_scope_authority_state", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "invalidated"
    );
    drop(s);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn m1a_migration_fault_rolls_back_ddl_and_revision() {
    let s = Store::open(Path::new(":memory:")).unwrap();
    let db = s.db.lock().unwrap();
    db.execute_batch("DROP TABLE donna_scope_authority_state; CREATE TRIGGER m1a_rev_fault BEFORE UPDATE ON kv WHEN NEW.k='rev' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    let before: String = db.query_row("SELECT v FROM kv WHERE k='rev'", [], |r| r.get(0)).unwrap();
    drop(db);
    assert!(escopo_migracao::migrate(&s).is_err());
    let db = s.db.lock().unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name='donna_scope_authority_state'", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(db.query_row("SELECT v FROM kv WHERE k='rev'", [], |r| r.get::<_, String>(0)).unwrap(), before);
}

#[tokio::test]
async fn m1a_devices_and_busy_are_closed() {
    use crate::store::Scope;
    let (h, b, dir) = fixture();
    let caller = Caller::Device { key: "synthetic".into(), scopes: vec![Scope::Control], autoridade: None };
    assert!(matches!(escopo_store::personal_sources(&h, &caller, &b), Err(E::IngressDenied)));
    {
        let db = h.store.db.lock().unwrap();
        db.execute("INSERT INTO devices(key,name,platform,source,scopes,lease_until,created_at) VALUES('synthetic','test','test','local','[\"control\"]',NULL,1)",[]).unwrap();
    }
    assert_eq!(escopo_store::personal_sources(&h, &caller, &b).unwrap().denial(), E::SourcesUninitialized);
    for (source, scopes, lease, expected) in [
        ("local", "[]", None, E::IngressDenied),
        ("unknown", "[\"control\"]", None, E::OutcomeUnknown),
        ("local", "{", None, E::OutcomeUnknown),
        ("account", "[\"control\"]", Some(1), E::IngressDenied),
    ] {
        h.store
            .db
            .lock()
            .unwrap()
            .execute(
                "UPDATE devices SET source=?1,scopes=?2,lease_until=?3 WHERE key='synthetic'",
                rusqlite::params![source, scopes, lease],
            )
            .unwrap();
        assert!(matches!(escopo_store::personal_sources(&h,&caller,&b),Err(e) if e==expected));
    }
    let guard = h.store.db.lock().unwrap();
    assert_eq!(denial(&h, &b), E::OutcomeUnknown);
    drop(guard);
    drop(h);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn m1a_conversation_binding_and_config_missing() {
    let (h, mut b, dir) = fixture();
    let additional = "00000000-0000-4000-8000-000000000013";
    h.store.db.lock().unwrap().execute("INSERT INTO donna_space_conversations(id,space_id,bot_id,title,revision,is_primary) VALUES(?1,?2,?3,'fixture',7,0)",rusqlite::params![additional,PERSONAL,b.bot_id]).unwrap();
    b.conversation_id = additional.into();
    b.primary = false;
    b.conversation_revision = rev(7);
    assert_eq!(denial(&h, &b), E::SourcesUninitialized);
    b.conversation_revision = rev(8);
    assert_eq!(denial(&h, &b), E::ContextChanged);
    b.conversation_revision = rev(7);
    h.store
        .db
        .lock()
        .unwrap()
        .execute("UPDATE donna_space_conversations SET closed=1 WHERE id=?1", [additional])
        .unwrap();
    assert_eq!(denial(&h, &b), E::ContextUnavailable);
    h.store
        .db
        .lock()
        .unwrap()
        .execute("UPDATE donna_space_conversations SET closed=0 WHERE id=?1", [additional])
        .unwrap();
    b.primary = true;
    assert_eq!(denial(&h, &b), E::ContextUnavailable);
    b.primary = false;
    h.store
        .db
        .lock()
        .unwrap()
        .execute("UPDATE bots SET config=json_remove(config,'$.kind') WHERE id=?1", [&b.bot_id])
        .unwrap();
    assert_eq!(denial(&h, &b), E::OutcomeUnknown);
    drop(h);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn m1a_resolver_audiences_and_hub_isolation() {
    use crate::fork::escopo::{
        ContextRef,
        fontes::{PersonalOrigin, PersonalScopeExpectation},
        resolver::resolve_personal,
    };
    let (h, b, dir) = fixture();
    let (h2, _, dir2) = fixture();
    let hub2 = "00000000-0000-4000-8000-000000000099";
    h2.store.kv_set("host_id", hub2).unwrap();
    let raw = json!({"version":1,"hubId":b.hub_id,"spaceId":b.space_id,"botId":b.bot_id,"conversationId":b.conversation_id,"audience":{"kind":"principal"},"teamId":null,"privacyDomainId":PERSONAL,"domainRevision":u64::from(b.space_revision),"conversationRevision":u64::from(b.conversation_revision)});
    let input = |v: serde_json::Value| PersonalScopeExpectation {
        context: serde_json::from_value::<ContextRef>(v).unwrap(),
        space_revision: b.space_revision,
        bot_revision: b.bot_revision,
    };
    let app = || PersonalOrigin::App { session_id: b.bot_id.clone() };
    assert!(matches!(resolve_personal(&h2, Caller::Local, input(raw.clone()), app()), Err(E::ContextChanged)));
    let mut team = raw.clone();
    team["teamId"] = json!("00000000-0000-4000-8000-000000000022");
    assert!(matches!(resolve_personal(&h, Caller::Local, input(team), app()), Err(E::ContextUnavailable)));
    let mut group = raw.clone();
    group["conversationId"] = json!("00000000-0000-4000-8000-000000000023");
    group["audience"] = json!({"kind":"group","groupId":"00000000-0000-4000-8000-000000000023"});
    assert!(matches!(resolve_personal(&h, Caller::Local, input(group), app()), Err(E::ContextUnavailable)));
    let mut activity = raw.clone();
    activity["conversationId"] = json!("00000000-0000-4000-8000-000000000023");
    activity["audience"] = json!({"kind":"activity","activityId":"00000000-0000-4000-8000-000000000024"});
    assert!(matches!(resolve_personal(&h, Caller::Local, input(activity), app()), Err(E::ContextUnavailable)));
    // O primeiro pedido válido faz o bootstrap da política pessoal (05a).
    assert!(resolve_personal(&h, Caller::Local, input(raw), app()).is_ok());
    drop(h);
    drop(h2);
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(dir2).unwrap();
}

#[tokio::test]
async fn m1a_binding_source_mutations_and_missing_barrier() {
    let (h, mut b, dir) = fixture();
    let original = h.store.kv_get("donna.spaces.v1").unwrap();
    b.space_revision = rev(i64::try_from(u64::from(b.space_revision)).unwrap() + 1);
    assert_eq!(denial(&h, &b), E::ContextChanged);
    b.space_revision = rev(i64::try_from(u64::from(b.space_revision)).unwrap() - 1);
    b.space_id = "00000000-0000-4000-8000-000000000099".into();
    assert_eq!(denial(&h, &b), E::ContextChanged);
    b.space_id = PERSONAL.into();
    h.store.kv_set("host_id", "00000000-0000-4000-8000-000000000099").unwrap();
    assert_eq!(denial(&h, &b), E::ContextChanged);
    h.store.kv_set("host_id", &b.hub_id).unwrap();
    h.store.sem_donna_fixa(); // D366: exercita a troca/exclusão legada, sem Donna fixa.
    h.store.kv_set("team.settings.v1", r#"{"primaryBotId":"00000000-0000-4000-8000-000000000099"}"#).unwrap();
    assert_eq!(denial(&h, &b), E::ContextChanged);
    h.store.kv_set("team.settings.v1", &json!({"primaryBotId":b.bot_id}).to_string()).unwrap();
    let bot_json: String =
        h.store.db.lock().unwrap().query_row("SELECT config FROM bots WHERE id=?1", [&b.bot_id], |r| r.get(0)).unwrap();
    for path in ["$.kind", "$.spaceId"] {
        h.store
            .db
            .lock()
            .unwrap()
            .execute(
                "UPDATE bots SET config=json_remove(?1,?2) WHERE id=?3",
                rusqlite::params![bot_json, path, b.bot_id],
            )
            .unwrap();
        assert_eq!(denial(&h, &b), E::OutcomeUnknown);
    }
    h.store
        .db
        .lock()
        .unwrap()
        .execute("UPDATE bots SET config=?1 WHERE id=?2", rusqlite::params![bot_json, b.bot_id])
        .unwrap();
    h.store.kv_set("donna.spaces.v1", &original).unwrap();
    h.store.db.lock().unwrap().execute("DELETE FROM donna_scope_authority_state", []).unwrap();
    assert_eq!(denial(&h, &b), E::OutcomeUnknown);
    drop(h);
    let s = Store::open(&dir.join("fixture.db")).unwrap();
    assert_eq!(
        s.db.lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM donna_scope_authority_state", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(s);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn m1a_regressed_revision_and_poison_are_unknown() {
    let (h, b, dir) = fixture();
    let old = barrier(&h);
    h.store.db.lock().unwrap().execute("UPDATE kv SET v='0' WHERE k='rev'", []).unwrap();
    assert_eq!(
        escopo_store::invalidate_personal_sources(&h, &Caller::Local, &b, rev(old), Reason::RecoveryRequired),
        Err(E::OutcomeUnknown)
    );
    assert_eq!(barrier(&h), old);
    assert_eq!(global(&h), 0);
    let other = h.clone();
    let _ = std::thread::spawn(move || {
        let _lock = other.store.db.lock().unwrap();
        panic!("synthetic poison")
    })
    .join();
    assert_eq!(denial(&h, &b), E::OutcomeUnknown);
    drop(h);
    std::fs::remove_dir_all(dir).unwrap();
}
