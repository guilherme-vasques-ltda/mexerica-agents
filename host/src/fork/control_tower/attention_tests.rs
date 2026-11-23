#![allow(clippy::unwrap_used)]

use super::{ATTENTION_EVENT, ATTENTION_METHOD, METHOD, delegated_permissions_tests::Fixture};
use crate::{
    LockExt,
    api::{devices::Caller, dispatch, scoped_events_stream},
    fork::spaces::PERSONAL,
    store::{BotConfig, Device, DeviceSource, EntryKind, Lane, Scope, now_ms},
};
use futures::StreamExt;
use rusqlite::params;
use serde_json::{Value, json};

async fn read(f: &Fixture, include_resolved: bool) -> Value {
    dispatch(
        &f.hub,
        &Caller::Local,
        ATTENTION_METHOD,
        json!({"spaceId":PERSONAL,"includeResolvedToday":include_resolved}),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn canonical_permissions_deduplicate_beyond_sidebar_limit_and_wait_for_commit() {
    let f = Fixture::new();
    f.request("request", "a", "b");
    let mut delegated = Vec::new();
    for _ in 0..21 {
        delegated.push(f.card("request", "b", "pending"));
    }
    let direct = f
        .hub
        .store
        .insert_entry(
            &Lane::main("b"),
            EntryKind::Permission,
            2,
            &json!({"status":"pending","author":"b","summary":"reserved.example.invalid"}),
        )
        .unwrap();
    let queue = read(&f, true).await;
    let items = queue["items"].as_array().unwrap();
    assert_eq!(items.len(), 22);
    for id in &delegated {
        let rows: Vec<_> = items.iter().filter(|row| row["dedupeKey"] == format!("permission:{id}")).collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["kind"], "delegated_permission");
        assert_eq!(rows[0]["target"]["requesterBotId"], "a");
        assert_eq!(rows[0]["target"]["botId"], "b");
    }
    assert!(!queue.to_string().contains("reserved.example.invalid"));
    assert!(!queue.to_string().contains("SENTINELA"));
    let id = &delegated[0];
    let mut data = f.hub.store.entry(id).unwrap().data;
    data["status"] = "answered".into();
    f.hub.store.update_entry(id, &data).unwrap();
    let queue = read(&f, true).await;
    assert_eq!(queue["items"].as_array().unwrap().len(), 21);
    assert!(
        queue["resolvedToday"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["target"]["entryId"] == *id && row["resolutionKind"] == "answered")
    );
    assert_eq!(read(&f, false).await["resolvedToday"], json!([]));
    assert!(queue["items"].as_array().unwrap().iter().any(|row| row["target"]["entryId"] == direct.id));
    let snapshot =
        dispatch(&f.hub, &Caller::Local, METHOD, json!({"hubId":f.hub.host_id,"spaceId":PERSONAL})).await.unwrap();
    assert_eq!(snapshot["attention"]["value"]["items"], queue["items"]);
}

#[tokio::test]
async fn partial_sources_and_confirmed_proposal_and_review_resolutions_are_scoped() {
    let f = Fixture::new();
    let at = now_ms();
    let project = |id| json!({"id":id,"name":"Project","objective":"","coordinatorBotId":null,"state":"active","revision":1,"createdAt":at,"updatedAt":at});
    let task = |state| json!({"id":"review","parentTaskId":null,"assigneeBotId":null,"title":"Review","description":"","acceptance":"","evidence":"","state":state});
    {
        let db = f.hub.store.db.locked();
        for (hub, space, id, state) in [
            (f.hub.host_id.as_str(), PERSONAL, "project", "review"),
            (f.hub.host_id.as_str(), PERSONAL, "finished", "done"),
            (f.hub.host_id.as_str(), "other", "hidden-space", "review"),
            ("other", PERSONAL, "hidden-hub", "review"),
        ] {
            db.execute(
                "INSERT INTO donna_projects VALUES(?1,?2,?3,?4,?5)",
                params![hub, space, id, project(id).to_string(), json!([task(state)]).to_string()],
            )
            .unwrap();
        }
        crate::fork::projetos::store::history_as(
            &db,
            (&f.hub.host_id, PERSONAL, "finished"),
            at,
            "owner",
            "task_updated",
            Some("review"),
            "Alteração manual: estado. Estado: Em conferência → Concluída manualmente.",
        )
        .unwrap();
        for (space, id, state, decided) in [
            (PERSONAL, "pending", "pending", None),
            (PERSONAL, "approved", "approved", Some(at)),
            (PERSONAL, "yesterday", "rejected", Some(at - 86_400_000)),
            ("other", "hidden-proposal", "pending", None),
        ] {
            db.execute("INSERT INTO donna_task_proposals(hub_id,space_id,id,project_id,bot_id,request_id,title,items,state,version,created_at,decided_at)
                VALUES(?1,?2,?3,'project','a',?3,?3,'[]',?4,1,?5,?6)", params![f.hub.host_id,space,id,state,at,decided]).unwrap();
        }
        db.execute_batch("DROP TABLE donna_delegated_permission_checkpoints").unwrap();
    }
    let queue = read(&f, true).await;
    assert_eq!(queue["items"].as_array().unwrap().len(), 2);
    assert_eq!(queue["resolvedToday"].as_array().unwrap().len(), 2);
    let wire = queue.to_string();
    for hidden in ["hidden-space", "hidden-hub", "hidden-proposal", "yesterday"] {
        assert!(!wire.contains(hidden));
    }
    assert!(wire.contains("accepted"));
    assert!(wire.contains("approved"));
    assert!(
        queue["coverageGaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gap| gap == &json!({"source":"delegated_permissions","code":"source_unavailable"}))
    );
}

#[tokio::test]
async fn attention_rpc_validates_authority_and_stream_sends_only_scoped_hint() {
    let f = Fixture::new();
    for body in [
        json!({}),
        json!({"spaceId":"missing"}),
        json!({"spaceId":PERSONAL,"unexpected":true}),
        json!({"spaceId":PERSONAL,"includeResolvedToday":null}),
    ] {
        assert!(dispatch(&f.hub, &Caller::Local, ATTENTION_METHOD, body).await.is_err());
    }
    let body = json!({"spaceId":PERSONAL});
    let mut device = Device {
        key: "fixture-control".into(),
        name: "Fixture".into(),
        platform: "fixture".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: now_ms(),
        last_seen_at: None,
    };
    f.hub.store.put_device(&device).unwrap();
    let (_, owner) = crate::fork::autoridade::autenticar(&f.hub, &device.key).unwrap();
    assert!(dispatch(&f.hub, &owner, ATTENTION_METHOD, body.clone()).await.is_ok());
    device.scopes.clear();
    f.hub.store.put_device(&device).unwrap();
    assert!(dispatch(&f.hub, &owner, ATTENTION_METHOD, body.clone()).await.is_err());
    device.key = "fixture-executor".into();
    device.scopes = vec![Scope::Executor];
    f.hub.store.put_device(&device).unwrap();
    let (_, executor) = crate::fork::autoridade::autenticar(&f.hub, &device.key).unwrap();
    assert!(dispatch(&f.hub, &executor, ATTENTION_METHOD, body.clone()).await.is_err());
    let binding = crate::fork::sessoes::bindings::fixture_binding(&f.hub.store, "a");
    let mut servers = [json!({"env":[]})];
    crate::fork::ponte_credencial::provisionar(&f.hub, "a", None, &binding, &mut servers).unwrap();
    let token = servers[0]["env"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == crate::fork::ponte_credencial::ENV)
        .unwrap()["value"]
        .as_str()
        .unwrap();
    let bridge = f.hub.pontes.autenticar(&f.hub, token).unwrap();
    assert!(dispatch(&f.hub, &bridge, ATTENTION_METHOD, body).await.is_err());
    let stream = scoped_events_stream(&f.hub, f.hub.store.current_rev(), None, &Caller::Local, Some(PERSONAL)).unwrap();
    tokio::pin!(stream);
    assert_eq!(stream.next().await.unwrap()["type"], "hello");
    for space in ["missing", PERSONAL] {
        f.hub.events.send(json!({"type":crate::fork::projetos::EVENT,"spaceId":space,"hubId":f.hub.host_id})).unwrap();
    }
    f.hub.events.send(json!({"type":"resync","marker":"end"})).unwrap();
    assert_eq!(stream.next().await.unwrap()["type"], crate::fork::projetos::EVENT);
    assert_eq!(stream.next().await.unwrap(), json!({"type":super::EVENT,"hubId":f.hub.host_id,"spaceId":PERSONAL}));
    assert_eq!(stream.next().await.unwrap(), json!({"type":ATTENTION_EVENT,"hubId":f.hub.host_id,"spaceId":PERSONAL}));
    assert_eq!(stream.next().await.unwrap()["marker"], "end");
}

#[tokio::test]
async fn attention_items_resolutions_and_stream_are_isolated_between_real_spaces() {
    let f = Fixture::new();
    f.hub.store.set_team_settings(&json!({"primaryBotId":"a"})).unwrap();
    let created = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaSpaceCreate",
        json!({"hubId":f.hub.host_id,
        "expectedRevision":f.hub.store.spaces().unwrap().revision,"name":"Example",
        "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"EX"}}}),
    )
    .await
    .unwrap();
    let other = created["spaces"][1]["id"].as_str().unwrap();
    for id in ["other-source", "other-target"] {
        let cfg: BotConfig =
            serde_json::from_value(json!({"id":id,"name":id,"spaceId":other,"backend":"fixture"})).unwrap();
        f.hub.store.save_bot(&cfg).unwrap();
    }
    let mut ids = Vec::new();
    for (space, source, target) in [(PERSONAL, "a", "b"), (other, "other-source", "other-target")] {
        let lane = Lane::main(target);
        let direct = f
            .hub
            .store
            .insert_entry(&lane, EntryKind::Permission, 1, &json!({"author":target,"status":"pending"}))
            .unwrap();
        f.request(source, source, target);
        let delegated = f.card_in(space, source, target, "pending");
        let (_, _, question) =
            crate::fork::owner_question::ask_on(&f.hub.store.db.locked(), target, &lane, 1, "Question? ").unwrap();
        crate::fork::owner_question::refresh(&f.hub, [&target.to_owned()]);
        let routine = format!("routine-{target}");
        let entry = format!("routine-created:{routine}");
        {
            let db = f.hub.store.db.locked();
            crate::store::insert_visible_on(&db, &entry, &lane, EntryKind::Notice, 1, &json!({"routineCreation":{
                "version":1,"spaceId":space,"botId":target,"routineId":routine,"name":routine,"botName":target,"createdAt":now_ms(),"decision":"pending"
            }})).unwrap();
        }
        ids.push((space, target, direct, delegated, question, entry, routine));
    }
    f.hub.store.kv_set("routines.v1", &json!({"routines":ids.iter().map(|(_,target,_,_,_,_,routine)| json!({
        "id":routine,"botId":target,"name":routine,"instruction":"Example","enabled":true,"deleted":false,"createdAt":now_ms(),"updatedAt":now_ms(),"triggers":[],"nextRuns":[],"webhookKey":""
    })).collect::<Vec<_>>()}).to_string()).unwrap();
    for (space, _, direct, delegated, question, routine_entry, _) in &ids {
        let queue = dispatch(&f.hub, &Caller::Local, ATTENTION_METHOD, json!({"spaceId":space})).await.unwrap();
        let items = queue["items"].as_array().unwrap();
        assert_eq!(items.len(), 4, "{queue}");
        for (id, kind) in [
            (&direct.id, "permission"),
            (delegated, "delegated_permission"),
            (&question.id, "owner_question"),
            (routine_entry, "routine_review"),
        ] {
            assert!(items.iter().any(|row| row["target"]["entryId"] == *id && row["kind"] == kind));
        }
        assert_eq!(queue["resolvedToday"], json!([]));
    }
    let stream = scoped_events_stream(&f.hub, f.hub.store.current_rev(), None, &Caller::Local, Some(PERSONAL)).unwrap();
    tokio::pin!(stream);
    assert_eq!(stream.next().await.unwrap()["type"], "hello");
    let other_stream =
        scoped_events_stream(&f.hub, f.hub.store.current_rev(), None, &Caller::Local, Some(other)).unwrap();
    tokio::pin!(other_stream);
    assert_eq!(other_stream.next().await.unwrap()["type"], "hello");
    for (space, target, direct, delegated, _, routine_entry, _) in &ids {
        for id in [&direct.id, delegated] {
            let mut data = f.hub.store.entry(id).unwrap().data;
            data["status"] = "answered".into();
            let entry = f.hub.store.update_entry(id, &data).unwrap().unwrap();
            f.hub.events.send(json!({"type":"entry","rev":entry.rev,"entry":entry})).unwrap();
        }
        crate::fork::owner_question::add_human_message(&f.hub, &Lane::main(target), 2, &json!({"text":"Confirmed"}))
            .unwrap();
        let entry = {
            let db = f.hub.store.db.locked();
            let (entry, card) = crate::fork::routine_creation::card_on(&db, space, routine_entry).unwrap();
            crate::fork::routine_creation::settle_on(&db, &entry, card, "kept").unwrap()
        };
        crate::fork::routine_creation::publish(&f.hub, vec![entry]);
    }
    f.hub.events.send(json!({"type":"resync","marker":"end"})).unwrap();
    for (space, mut stream) in [(PERSONAL, stream), (other, other_stream)] {
        let mut hints = 0;
        loop {
            let event = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next()).await.unwrap().unwrap();
            if event["marker"] == "end" {
                break;
            }
            if event["type"] == ATTENTION_EVENT {
                assert_eq!(event, json!({"type":ATTENTION_EVENT,"hubId":f.hub.host_id,"spaceId":space}));
                hints += 1;
            }
        }
        assert_eq!(hints, 4);
    }
    for (space, _, direct, delegated, question, routine_entry, _) in &ids {
        let queue = dispatch(&f.hub, &Caller::Local, ATTENTION_METHOD, json!({"spaceId":space})).await.unwrap();
        assert_eq!(queue["items"], json!([]));
        let resolved = queue["resolvedToday"].as_array().unwrap();
        assert_eq!(resolved.len(), 4, "{queue}");
        for id in [&direct.id, delegated, &question.id, routine_entry] {
            assert!(resolved.iter().any(|row| row["target"]["entryId"] == *id));
        }
    }
}

#[tokio::test]
async fn question_and_routine_resolutions_follow_their_producers() {
    let f = Fixture::new();
    f.hub.store.set_team_settings(&json!({"primaryBotId":"a"})).unwrap();
    let lane = Lane::main("a");
    let bindings = &f.hub.store;
    let binding = crate::fork::sessoes::bindings::issue(bindings, "a", "a", "a", None).unwrap();
    crate::fork::sessoes::bindings::activate(bindings, &binding, "fixture-acp").unwrap();
    let request = bindings.insert_entry(&lane, EntryKind::User, 1, &json!({"text":"Pedido de exemplo"})).unwrap();
    f.hub
        .start_bot_turn("a", |runtime| {
            runtime.lane = Some(lane.clone());
            runtime.session = Some("fixture-acp".into());
            runtime.message = Some(request.id);
        })
        .unwrap();
    let execution = crate::fork::sessoes::bindings::capture(&f.hub, "a", binding.id.as_str()).unwrap();
    let question = crate::fork::owner_question::ask(&f.hub, "a", &lane, 1, "Confirmar horário?", &execution).unwrap();
    f.hub.routines.restore(&f.hub.store).unwrap();
    crate::routines::call_invoked_in_execution(&f.hub, "a", "save_routine", &json!({
        "name":"Revisão de exemplo", "instruction":"Notificar", "triggers":[{"type":"cron","expression":"0 9 * * 1","timeZone":"UTC"}]
    }), &execution, None).unwrap();
    let queue = read(&f, true).await;
    assert_eq!(queue["items"][0]["target"]["entryId"], question.id);
    assert_eq!(queue["items"][0]["summary"], "Confirmar horário?");
    assert_eq!(queue["items"][1]["kind"], "routine_review");
    let id = queue["items"][1]["target"]["entryId"].as_str().unwrap();
    {
        let db = f.hub.store.db.locked();
        let (entry, card) = crate::fork::routine_creation::card_on(&db, PERSONAL, id).unwrap();
        crate::fork::routine_creation::settle_on(&db, &entry, card, "kept").unwrap();
    }
    crate::fork::owner_question::add_human_message(&f.hub, &lane, 2, &json!({"text":"Confirmado"})).unwrap();
    let queue = read(&f, true).await;
    assert_eq!(queue["items"], json!([]));
    let resolved = queue["resolvedToday"].as_array().unwrap();
    assert!(resolved.iter().any(|row| row["kind"] == "owner_question" && row["resolutionKind"] == "answered"));
    assert!(resolved.iter().any(|row| row["kind"] == "routine_review" && row["resolutionKind"] == "kept"));
}
