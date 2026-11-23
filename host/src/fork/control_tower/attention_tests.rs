#![allow(clippy::unwrap_used)]

use super::{ATTENTION_EVENT, ATTENTION_METHOD, METHOD, delegated_permissions_tests::Fixture};
use crate::{
    LockExt,
    api::{devices::Caller, dispatch, scoped_events_stream},
    fork::spaces::PERSONAL,
    store::{EntryKind, Lane, Scope, now_ms},
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
    let no_control = Caller::Device { key: "unknown".into(), scopes: vec![Scope::Screen], autoridade: None };
    assert!(dispatch(&f.hub, &no_control, ATTENTION_METHOD, json!({"spaceId":PERSONAL})).await.is_err());
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
