use super::{PERSONAL, filter_event};
use crate::{LockExt, hub::Hub, remote::identity::Identity, store::Store};
use serde_json::json;

#[test]
fn group_events_are_confined_to_their_space() {
    let dir = crate::store::private_fixture_dir("group-events");
    let hub = Hub::new(
        Store::open(std::path::Path::new(":memory:")).unwrap(),
        "fixture".into(),
        Identity::load_or_create(&dir).unwrap(),
        "fixture".into(),
        0,
    );
    let kind = "botGroups";
    let event = json!({"type":kind,"spaceId":"work","snapshot":{"rev":1}});
    assert!(filter_event(&hub, PERSONAL, event.clone()).is_none());
    assert_eq!(filter_event(&hub, "work", event.clone()), Some(event));
    assert!(filter_event(&hub, "other", json!({"type":kind})).is_none());
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn work_group_is_not_delivered_to_personal_stream() {
    use crate::api::{devices::Caller, dispatch, scoped_events_stream};
    use futures::StreamExt;
    let dir = crate::store::private_fixture_dir("group-event-streams");
    let hub = Hub::new(
        Store::open(std::path::Path::new(":memory:")).unwrap(),
        "fixture".into(),
        Identity::load_or_create(&dir).unwrap(),
        "fixture".into(),
        0,
    );
    let first = dispatch(&hub, &Caller::Local, "donnaSpaces", json!({"hubId":hub.host_id})).await.unwrap();
    let spaces = dispatch(&hub, &Caller::Local, "donnaSpaceCreate", json!({"hubId":hub.host_id,"expectedRevision":first["revision"],"name":"Trabalho","tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}}})).await.unwrap();
    let work = spaces["spaces"][1]["id"].as_str().unwrap();
    let personal_stream =
        scoped_events_stream(&hub, hub.store.current_rev(), None, &Caller::Local, Some(PERSONAL)).unwrap();
    let work_stream = scoped_events_stream(&hub, hub.store.current_rev(), None, &Caller::Local, Some(work)).unwrap();
    tokio::pin!(personal_stream, work_stream);
    assert_eq!(personal_stream.next().await.unwrap()["type"], "hello");
    assert_eq!(work_stream.next().await.unwrap()["type"], "hello");
    dispatch(&hub, &Caller::Local, "botGroupsEdit", json!({"spaceId":work,"op":"add","name":"Privado"})).await.unwrap();
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), work_stream.next()).await.unwrap().unwrap();
    assert_eq!(event["type"], "botGroups");
    assert_eq!(event["spaceId"], work);
    assert!(tokio::time::timeout(std::time::Duration::from_millis(100), personal_stream.next()).await.is_err());
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn space_deletion_survives_failed_group_copy() {
    use crate::api::{devices::Caller, dispatch};
    let dir = crate::store::private_fixture_dir("group-copy-space-delete");
    let hub = Hub::new(
        Store::open(std::path::Path::new(":memory:")).unwrap(),
        "fixture".into(),
        Identity::load_or_create(&dir).unwrap(),
        "fixture".into(),
        0,
    );
    let first = dispatch(&hub, &Caller::Local, "donnaSpaces", json!({"hubId":hub.host_id})).await.unwrap();
    let created=dispatch(&hub,&Caller::Local,"donnaSpaceCreate",json!({"hubId":hub.host_id,"expectedRevision":first["revision"],"name":"Trabalho","tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}}})).await.unwrap();
    let space = created["spaces"][1]["id"].as_str().unwrap();
    {
        let db = hub.store.db.locked();
        db.execute_batch("DELETE FROM kv WHERE k='bot_groups.migrated'; CREATE TABLE donna_bot_sections(id TEXT PRIMARY KEY,name TEXT NOT NULL,position INTEGER NOT NULL,space_id TEXT); CREATE TABLE donna_bot_section_assignments(bot_id TEXT PRIMARY KEY,section_id TEXT NOT NULL); CREATE TRIGGER fail_group BEFORE INSERT ON bot_groups BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
        db.execute("INSERT INTO donna_bot_sections VALUES('work-group','Grupo',0,?1)", [space]).unwrap();
        db.execute("INSERT INTO donna_bot_sections VALUES('personal-group','Pessoal',0,?1)", [PERSONAL]).unwrap();
        db.execute_batch("INSERT INTO donna_bot_section_assignments VALUES('work-bot','work-group'),('personal-bot','personal-group');").unwrap();
        db.execute("INSERT INTO kv(k,v) VALUES(?1,'44')", [format!("donna.botSections.rev.{space}")]).unwrap();
        crate::store::bot_groups::legado::migrate(&db).unwrap();
    }
    dispatch(
        &hub,
        &Caller::Local,
        "donnaSpaceDelete",
        json!({"hubId":hub.host_id,"spaceId":space,"expectedRevision":created["revision"],"confirmName":"Trabalho"}),
    )
    .await
    .unwrap();
    {
        // A exclusão do Space remove também a cópia legada do Space, na mesma
        // transação — antes de qualquer retentativa de migração.
        let db = hub.store.db.locked();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM donna_bot_sections WHERE id='work-group'", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM donna_bot_sections WHERE id='personal-group'", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM donna_bot_section_assignments WHERE bot_id='work-bot'", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM donna_bot_section_assignments WHERE bot_id='personal-bot'", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM kv WHERE k=?1", [format!("donna.botSections.rev.{space}")], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        db.execute_batch("DROP TRIGGER fail_group").unwrap();
        crate::store::bot_groups::legado::migrate(&db).unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM bot_groups WHERE space_id=?1", [space], |r| r.get::<_, i64>(0)).unwrap(),
            0
        );
        assert!(
            !db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='donna_bot_sections')", [], |r| r
                .get::<_, bool>(0))
                .unwrap()
        );
    }
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn deleting_member_emits_group_protocol() {
    use crate::api::{devices::Caller, dispatch};
    let dir = crate::store::private_fixture_dir("group-deletion-event");
    let hub = Hub::new(
        Store::open(std::path::Path::new(":memory:")).unwrap(),
        "fixture".into(),
        Identity::load_or_create(&dir).unwrap(),
        "fixture".into(),
        0,
    );
    let config = serde_json::from_value(json!({"id":"a","name":"a","backend":"fixture","cwd":"/tmp"})).unwrap();
    hub.store.save_bot(&config).unwrap();
    dispatch(&hub, &Caller::Local, "botGroupsEdit", json!({"op":"add","name":"Grupo","botId":"a"})).await.unwrap();
    let mut events = hub.events.subscribe();
    hub.delete_bot("a").unwrap();
    let mut modern = None;
    while let Ok(event) = events.try_recv() {
        if event["type"] == "botGroups" {
            modern = Some(event);
        }
    }
    let modern = modern.unwrap();
    assert_eq!(modern["snapshot"]["groups"][0]["bots"], json!([]));
    assert!(modern["snapshot"]["rev"].as_i64().unwrap() > hub.store.bot("a").unwrap().unwrap().rev);
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}
