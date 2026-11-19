use super::*;
use crate::fork::atividades::startup_store::{lookup_startup_conversation, open_startup_conversation};
use crate::store::Store;
use serde_json::json;

fn fixture() -> (Store, StartupOpenInput) {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let bot = serde_json::from_value(json!({"id":"donna","name":"Donna","cwd":"/tmp","backend":"claude"})).unwrap();
    store.save_bot(&bot).unwrap();
    store.kv_set("team.settings.v1", r#"{"primaryBotId":"donna","permission":"ask"}"#).unwrap();
    let space = store.spaces().unwrap().spaces[0].revision;
    let primary = store.space_conversation(PERSONAL, "donna").unwrap().revision;
    let context = StartupExpectedContext {
        hub_id: "fixture-hub".into(),
        space_id: Id::try_from(PERSONAL.to_string()).unwrap(),
        space_revision: Revision::try_from(u64::try_from(space).unwrap()).unwrap(),
        principal_bot_id: "donna".into(),
        primary_conversation_revision: Revision::try_from(u64::try_from(primary).unwrap()).unwrap(),
    };
    let input = StartupOpenInput {
        version: Version,
        space_id: context.space_id.clone(),
        launch_ref: StartupLaunchRef {
            id: Id::try_from(uuid::Uuid::new_v4().to_string()).unwrap(),
            request_digest: context.digest().unwrap(),
        },
        expected_context: context,
    };
    (store, input)
}
fn lookup(i: &StartupOpenInput) -> StartupLookupInput {
    StartupLookupInput {
        version: i.version,
        space_id: i.space_id.clone(),
        launch_ref: i.launch_ref.clone(),
        expected_context: i.expected_context.clone(),
    }
}
fn counters(s: &Store) -> (i64, i64) {
    let db = s.db.lock().unwrap();
    (
        db.query_row("SELECT COUNT(*) FROM donna_space_conversations", [], |r| r.get(0)).unwrap(),
        db.query_row("SELECT CAST(v AS INTEGER) FROM kv WHERE k='rev'", [], |r| r.get(0)).unwrap(),
    )
}
#[test]
fn startup_replay_lookup_rollback_and_corruption() {
    let (s, i) = fixture();
    assert!(matches!(
        lookup_startup_conversation(&s, "fixture-hub", &lookup(&i)).unwrap(),
        StartupConversationResult::NotFound { .. }
    ));
    let (first, created) = open_startup_conversation(&s, "fixture-hub", &i).unwrap();
    assert!(created.is_some());
    let before = counters(&s);
    let (again, created) = open_startup_conversation(&s, "fixture-hub", &i).unwrap();
    assert!(created.is_none());
    assert_eq!(serde_json::to_value(&first).unwrap(), serde_json::to_value(&again).unwrap());
    assert_eq!(counters(&s), before);
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(lookup_startup_conversation(&s, "fixture-hub", &lookup(&i)).unwrap()).unwrap()
    );
    assert_eq!(open_startup_conversation(&s, "wrong-hub", &i).unwrap_err(), StartupError::ContextChanged);
    let mut other = i.clone();
    other.launch_ref.id = Id::try_from(uuid::Uuid::new_v4().to_string()).unwrap();
    {
        let db = s.db.lock().unwrap();
        db.execute_batch("CREATE TRIGGER startup_fail BEFORE INSERT ON donna_space_conversations WHEN NEW.startup_launch_id IS NOT NULL BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    }
    assert_eq!(open_startup_conversation(&s, "fixture-hub", &other).unwrap_err(), StartupError::OutcomeUnknown);
    assert_eq!(counters(&s), before);
    s.sem_donna_fixa(); // D366: exercita a troca/exclusão legada, sem Donna fixa.
    s.kv_set("team.settings.v1", "{").unwrap();
    assert_eq!(lookup_startup_conversation(&s, "fixture-hub", &lookup(&i)).unwrap_err(), StartupError::OutcomeUnknown);
}
#[test]
fn startup_parse_is_closed_and_digest_is_bound() {
    let (_, i) = fixture();
    let mut value = serde_json::to_value(&i).unwrap();
    value["extra"] = json!(true);
    assert!(serde_json::from_value::<StartupOpenInput>(value).is_err());
    let mut invalid = i.clone();
    invalid.expected_context.principal_bot_id = "other".into();
    assert_eq!(
        validate("fixture-hub", &invalid.space_id, &invalid.launch_ref, &invalid.expected_context),
        Err(StartupError::IntentConflict)
    );
}

#[test]
fn startup_collision_in_another_binding_is_not_absence() {
    let (s, i) = fixture();
    {
        let db = s.db.lock().unwrap();
        db.execute(
            "INSERT INTO donna_space_conversations(id,space_id,bot_id,title,revision,is_primary,startup_hub_id,startup_launch_id,startup_request_digest) VALUES(?1,?2,'other','fixture',1,0,?3,?4,?5)",
            rusqlite::params![uuid::Uuid::new_v4().to_string(), "00000000-0000-4000-8000-000000000002", "fixture-hub", String::from(i.launch_ref.id.clone()), String::from(i.launch_ref.request_digest.clone())],
        ).unwrap();
    }
    assert!(
        lookup_startup_conversation(&s, "fixture-hub", &lookup(&i)).is_err(),
        "a collision in another binding cannot establish absence"
    );
}

#[test]
fn startup_collision_open_and_other_hub_are_distinct() {
    let (s, i) = fixture();
    {
        let db = s.db.lock().unwrap();
        db.execute("INSERT INTO donna_space_conversations(id,space_id,bot_id,title,revision,is_primary,startup_hub_id,startup_launch_id,startup_request_digest) VALUES(?1,?2,'other','fixture',1,0,?3,?4,?5)", rusqlite::params![uuid::Uuid::new_v4().to_string(), "00000000-0000-4000-8000-000000000002", "fixture-hub", String::from(i.launch_ref.id.clone()), String::from(i.launch_ref.request_digest.clone())]).unwrap();
    }
    let before = counters(&s);
    assert_eq!(open_startup_conversation(&s, "fixture-hub", &i).unwrap_err(), StartupError::IntentConflict);
    assert_eq!(counters(&s), before);
    let mut other = i.clone();
    other.expected_context.hub_id = "other-hub".into();
    other.launch_ref.request_digest = other.expected_context.digest().unwrap();
    assert!(matches!(
        lookup_startup_conversation(&s, "other-hub", &lookup(&other)).unwrap(),
        StartupConversationResult::NotFound { .. }
    ));
    assert_eq!(counters(&s), before);
    s.db.lock().unwrap().execute_batch("DROP TABLE donna_space_conversations").unwrap();
    assert_eq!(
        lookup_startup_conversation(&s, "other-hub", &lookup(&other)).unwrap_err(),
        StartupError::OutcomeUnknown
    );
}

#[test]
fn startup_closed_replay_precedes_limit_and_migration_is_additive() {
    let (s, i) = fixture();
    let legacy = {
        let db = s.db.lock().unwrap();
        db.query_row("SELECT title,revision,read_revision,startup_hub_id,startup_launch_id,startup_request_digest FROM donna_space_conversations WHERE id='donna'", [], |r| Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,Option<String>>(3)?,r.get::<_,Option<String>>(4)?,r.get::<_,Option<String>>(5)?))).unwrap()
    };
    {
        let db = s.db.lock().unwrap();
        db.execute_batch("DROP INDEX donna_space_conversations_startup; ALTER TABLE donna_space_conversations DROP COLUMN startup_hub_id; ALTER TABLE donna_space_conversations DROP COLUMN startup_launch_id; ALTER TABLE donna_space_conversations DROP COLUMN startup_request_digest;").unwrap();
        super::super::store::migrate(&db).unwrap();
        super::super::store::migrate(&db).unwrap();
        let after = db.query_row("SELECT title,revision,read_revision,startup_hub_id,startup_launch_id,startup_request_digest FROM donna_space_conversations WHERE id='donna'", [], |r| Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,Option<String>>(3)?,r.get::<_,Option<String>>(4)?,r.get::<_,Option<String>>(5)?))).unwrap();
        assert_eq!(legacy, after);
    }
    assert_eq!((legacy.3, legacy.4, legacy.5), (None, None, None));
    let (first, _) = open_startup_conversation(&s, "fixture-hub", &i).unwrap();
    let id = match first {
        StartupConversationResult::Found { conversation_id, .. } => String::from(conversation_id),
        StartupConversationResult::NotFound { .. } => panic!("expected found"),
    };
    {
        let db = s.db.lock().unwrap();
        db.execute("UPDATE donna_space_conversations SET closed=1 WHERE id=?1", [&id]).unwrap();
        for n in 0..1022 {
            db.execute("INSERT INTO donna_space_conversations(id,space_id,bot_id,title,revision,is_primary) VALUES(?1,?2,'donna','fixture',1,0)",rusqlite::params![format!("filler-{n}"),PERSONAL]).unwrap();
        }
    }
    let before = counters(&s);
    let (replay, created) = open_startup_conversation(&s, "fixture-hub", &i).unwrap();
    assert!(created.is_none());
    assert!(matches!(replay, StartupConversationResult::Found { closed: true, .. }));
    let mut new = i.clone();
    new.launch_ref.id = Id::try_from(uuid::Uuid::new_v4().to_string()).unwrap();
    assert_eq!(open_startup_conversation(&s, "fixture-hub", &new).unwrap_err(), StartupError::ConversationLimit);
    assert_eq!(counters(&s), before);
}

#[test]
fn startup_same_store_concurrent_open_creates_once() {
    let (s, i) = fixture();
    let s = std::sync::Arc::new(s);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let s = s.clone();
        let i = i.clone();
        let b = barrier.clone();
        workers.push(std::thread::spawn(move || {
            b.wait();
            open_startup_conversation(&s, "fixture-hub", &i)
        }));
    }
    barrier.wait();
    let outcomes: Vec<_> = workers.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(outcomes.iter().filter(|v| matches!(v, Ok((_, Some(_))))).count(), 1);
    assert!(outcomes.iter().all(|v| v.is_ok() || matches!(v, Err(StartupError::OutcomeUnknown))));
    assert_eq!(counters(&s).0, 2);
    assert!(matches!(
        lookup_startup_conversation(&s, "fixture-hub", &lookup(&i)).unwrap(),
        StartupConversationResult::Found { .. }
    ));
}

#[tokio::test]
async fn startup_api_ingress_and_event_after_commit() {
    use crate::{
        api::{devices::Caller, dispatch},
        hub::Hub,
        remote::identity::Identity,
        store::Scope,
    };
    let (s, i) = fixture();
    let dir =
        std::fs::canonicalize(std::env::temp_dir()).unwrap().join(format!("startup-fixture-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let hub = Hub::new(s, "fixture-hub".into(), Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
    let mut events = hub.events.subscribe();
    let mut body = serde_json::to_value(&i).unwrap();
    let before = counters(&hub.store);
    assert!(dispatch(&hub, &Caller::Local, "donnaConversationStartupOpen", body.clone()).await.is_err());
    body["hubId"] = json!("wrong");
    assert!(dispatch(&hub, &Caller::Local, "donnaConversationStartupOpen", body.clone()).await.is_err());
    body["hubId"] = json!("fixture-hub");
    let device = Caller::Device { key: "fixture".into(), scopes: vec![Scope::Control], autoridade: None };
    assert!(dispatch(&hub, &device, "donnaConversationStartupOpen", body.clone()).await.is_err());
    let mut wrong = body.clone();
    wrong["expectedContext"]["principalBotId"] = json!("other");
    assert!(dispatch(&hub, &Caller::Local, "donnaConversationStartupOpen", wrong).await.is_err());
    assert_eq!(counters(&hub.store), before);
    assert!(events.try_recv().is_err());
    let first = dispatch(&hub, &Caller::Local, "donnaConversationStartupOpen", body.clone()).await.unwrap();
    let event = events.try_recv().unwrap();
    assert_eq!(event["type"], "conversation");
    let persisted = hub.store.space_conversation(PERSONAL, first["conversationId"].as_str().unwrap()).unwrap();
    assert_eq!(persisted.id, first["conversationId"]);
    let after = counters(&hub.store);
    let replay = dispatch(&hub, &Caller::Local, "donnaConversationStartupOpen", body).await.unwrap();
    assert_eq!(replay["conversationId"], first["conversationId"]);
    assert_eq!(counters(&hub.store), after);
    assert!(events.try_recv().is_err());
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn startup_cria_a_principal_da_conversa_na_mesma_transacao() {
    let (s, i) = fixture();
    open_startup_conversation(&s, "fixture-hub", &i).unwrap();
    let id: String =
        s.db.lock()
            .unwrap()
            .query_row("SELECT id FROM donna_space_conversations WHERE startup_hub_id='fixture-hub'", [], |r| r.get(0))
            .unwrap();
    let entry = s
        .insert_entry(&crate::store::Lane::main(&id), crate::store::EntryKind::User, 1, &json!({"text":"oi"}))
        .unwrap();
    // Sem reabrir o banco: a entrada resolve para a principal da conversa.
    let db = s.db.lock().unwrap();
    let session = crate::fork::sessoes::store::session_ref_of_entry(&db, &id, &entry.id).unwrap();
    assert_eq!((session.id.as_str(), session.chat_id.as_str()), (id.as_str(), id.as_str()));
}
