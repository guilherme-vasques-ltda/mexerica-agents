#![allow(clippy::unwrap_used)]

use super::*;
use crate::{
    fork::{donna_fixa::pedido::Origem, engine_setup::process::Rpc, open_tool_actions::respond},
    store::EntryKind,
};
use std::process::Stdio;
use std::time::Duration;

struct Fixture {
    source: LocalTurn,
    source_ticket: Ticket,
    target: String,
    _dir: tempfile::TempDir,
}

fn fixture() -> Fixture {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::Builder::new().permissions(std::fs::Permissions::from_mode(0o700)).tempdir().unwrap();
    let store = Store::open(&dir.path().join("hub.db")).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let target = uuid::Uuid::new_v4().to_string();
    for (id, name) in [(&id, "Origem"), (&target, "Destino")] {
        let cfg = serde_json::from_value(json!({"id":id,"name":name,"backend":"codex"})).unwrap();
        store.save_bot(&cfg).unwrap();
    }
    let hub = Hub::new(
        store,
        "delegation-test".into(),
        crate::remote::identity::Identity::load_or_create(dir.path()).unwrap(),
        "test".into(),
        0,
    );
    let lane = Lane::main(&id);
    let source_ticket = open_chat::admit_for_test(&hub, &id, "fixture").unwrap();
    let entry = hub
        .add_entry(
            &lane,
            EntryKind::User,
            7,
            &json!({
                "text":"Pode pedir uma segunda opinião", "status":"sent",
                "contentOrigin":{"ingress":{"caller":"local"}}
            }),
        )
        .unwrap();
    hub.set_runtime(&id, |runtime| {
        runtime.status = BotStatus::Working;
        runtime.lane = Some(lane.clone());
        runtime.message = Some(entry.id.clone());
    });
    let source =
        LocalTurn::capture(&hub, &lane, &entry.id, 7, &source_ticket.cancel(), crate::fork::spaces::PERSONAL).unwrap();
    Fixture { source, source_ticket, target, _dir: dir }
}

async fn approved(f: &Fixture, intent: &Intent<'_>) -> Entry {
    let hub = &f.source.hub;
    let mut events = hub.events.subscribe();
    let (entry, ()) = tokio::join!(
        approve(hub, &f.source.lane, f.source.turn, "Pedir resposta a outro bot", &intent.detail),
        async {
            let event = tokio::time::timeout(Duration::from_secs(5), events.recv()).await.unwrap().unwrap();
            let card = hub.store.entry(event["entry"]["id"].as_str().unwrap()).unwrap();
            assert_eq!(card.data["detail"], intent.detail);
            assert!(respond(hub, &card, card.rev, &json!({"source":"app"}), Some("allow"), None).is_err());
            respond(hub, &card, card.rev, &json!({"source":"app"}), Some("allow"), Some(&Origem::Local)).unwrap();
        }
    );
    entry.unwrap()
}

fn intent(f: &Fixture) -> Intent<'_> {
    let message = "Confira estes dois números.";
    Intent {
        target: &f.target,
        target_name: "Destino",
        message,
        contract: None,
        detail: serde_json::to_string_pretty(
            &json!({"bot":"Destino","botId":f.target,"message":message,"contract":null}),
        )
        .unwrap(),
    }
}

fn admit(approval: &ApprovedAsk<'_>, swapped_text: bool) -> Result<([String; 2], Recibo)> {
    let s = approval.source;
    let id = &approval.card.id;
    let intent = approval.intent;
    let mut notices = [0, 1].map(|_| {
        json!({
            "text":"Pedido", "status":"accepted", "delegationId":id, "sourceBotId":s.lane.chat,
            "targetBotId":intent.target, "botMessage":intent.message_json(&s.lane),
        })
    });
    if swapped_text {
        notices[1]["botMessage"]["text"] = "Pedido trocado".into();
    }
    let chain =
        equipe::Entrada::nova(&s.lane.chat, intent.target, id, ASK, intent.message, intent.contract.as_ref(), None)?;
    let reservations = RUNNING.locked();
    s.check_live(&reservations)?;
    let (entries, receipt) =
        s.hub.add_team_notices_with(&s.lane, Some(s.turn), &notices, TeamOrigin::Open(approval), &chain)?;
    Ok((entries.map(|e| e.id), receipt.unwrap()))
}

fn request_state(f: &Fixture, id: &str) -> (String, Option<String>) {
    f.source
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT state,end_reason FROM donna_session_requests WHERE id=?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap()
}

#[tokio::test]
async fn authorizes_exact_live_local_request_and_allocates_siblings_atomically() {
    if !crate::environment::Environment::is_open() {
        return;
    }
    let f = fixture();
    let intent = intent(&f);
    let (_cancel, imitation) = watch::channel(false);
    assert!(
        LocalTurn::capture(&f.source.hub, &f.source.lane, &f.source.entry, f.source.turn, &imitation, &f.source.space,)
            .is_err()
    );
    assert!(ask(f.source.clone(), &json!({"botId":f.source.lane.chat,"message":"Teste"})).await.is_err());
    let ticket = open_chat::admit_for_test(&f.source.hub, &f.target, "fixture").unwrap();
    assert!(ask(f.source.clone(), &json!({"botId":f.target,"message":"Teste"})).await.is_err());
    let a = ApprovedAsk { source: &f.source, intent: &intent, card: approved(&f, &intent).await };
    assert!(admit(&a, true).is_err());
    let (first, ar) = admit(&a, false).unwrap();
    assert!(admit(&a, false).is_err());
    let b = ApprovedAsk { source: &f.source, intent: &intent, card: approved(&f, &intent).await };
    let (_, br) = admit(&b, false).unwrap();
    assert_eq!(ar.chain_id, br.chain_id);
    assert_eq!(br.ordinal, ar.ordinal + 1);
    assert_eq!(request_state(&f, &ar.run_id).0, "accepted");
    {
        let db = f.source.hub.store.db.locked();
        let count = |sql: &str| db.query_row(sql, [], |row| row.get::<_, i64>(0)).unwrap();
        assert_eq!(count("SELECT COUNT(*) FROM entries WHERE kind='notice'"), 4);
        assert_eq!(count("SELECT used FROM donna_chain_roots"), 3);
        db.execute("UPDATE donna_chain_roots SET used=50", []).unwrap();
    }
    let c = ApprovedAsk { source: &f.source, intent: &intent, card: approved(&f, &intent).await };
    assert!(admit(&c, false).unwrap_err().is::<equipe::RecusaEquipe>());
    assert_eq!(
        f.source
            .hub
            .store
            .db
            .locked()
            .query_row("SELECT COUNT(*) FROM entries WHERE kind='notice'", [], |row| row.get::<_, i64>(0),)
            .unwrap(),
        4
    );
    open_chat::stop(&f.source.lane.chat);
    assert!(admit(&b, false).is_err());
    retornos::recuperar(&f.source.hub.store).unwrap();
    assert_eq!(request_state(&f, &ar.run_id), ("interrupted".into(), Some("restart_before_start".into())));
    assert_eq!(f.source.hub.store.entry(&first[0]).unwrap().data["status"], "failed");
    ticket.release(&f.source.hub);
    f.source_ticket.release(&f.source.hub);
}

const APP_SERVER: &str = r#"
import json, sys
def send(value):
    print(json.dumps(value), flush=True)
for line in sys.stdin:
    request = json.loads(line)
    method = request["method"]
    if method == "initialized":
        continue
    params = request["params"]
    if method == "initialize":
        result = {}
    elif method == "thread/start":
        assert params["ephemeral"] is True
        assert params["config"]["web_search"] == "disabled"
        assert "dynamicTools" not in params and "threadId" not in params
        assert params["sandbox"] == "read-only" and params["approvalPolicy"] == "never"
        result = {"thread":{"id":"target-thread","ephemeral":True},"sandbox":{"type":"readOnly"},"approvalPolicy":"never"}
    elif method == "turn/start":
        assert params["threadId"] == "target-thread"
        assert params["input"][0]["text"] == "Pedido fixo"
        result = {"turn":{"id":"clean-turn"}}
    else:
        raise AssertionError(method)
    send({"id":request["id"],"result":result})
    if method == "turn/start":
        send({"method":"item/agentMessage/delta","params":{
            "threadId":"target-thread","turnId":"clean-turn","itemId":"a1","delta":"Resposta isolada"}})
        send({"method":"thread/tokenUsage/updated","params":{
            "threadId":"target-thread","turnId":"clean-turn","tokenUsage":{"total":{
            "totalTokens":15,"inputTokens":12,"cachedInputTokens":2,"outputTokens":3,"reasoningOutputTokens":1}}}})
        send({"method":"turn/completed","params":{
            "threadId":"target-thread","turn":{"id":"clean-turn","status":"completed"}}})
"#;

#[tokio::test]
async fn isolated_reply_is_held_on_commit_failure_and_not_replayed_after_restart() {
    if !crate::environment::Environment::is_open() {
        return;
    }
    let f = fixture();
    let intent = intent(&f);
    let ticket = open_chat::admit_for_test(&f.source.hub, &f.target, "fixture").unwrap();
    let a = ApprovedAsk { source: &f.source, intent: &intent, card: approved(&f, &intent).await };
    let (entries, receipt) = admit(&a, false).unwrap();
    let dispatch = Dispatch {
        source: &f.source,
        target: ticket.cancel(),
        receipt: &receipt,
        deadline: Instant::now() + Duration::from_secs(10),
        fence: AtomicI64::new(0),
        usage: std::sync::OnceLock::new(),
    };
    let mut cmd = tokio::process::Command::new("python3");
    let child = cmd
        .args(["-I", "-c", APP_SERVER])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut rpc = Rpc::from_child(crate::fork::motor_exec::fixture(child)).unwrap();
    let response =
        open_chat::drive_delegated(&mut rpc, &ticket, "Perfil do destino", "Pedido fixo", &dispatch).await.unwrap();
    rpc.finish().await.unwrap();
    assert_eq!(response, "Resposta isolada");
    assert_eq!(request_state(&f, &receipt.run_id).0, "running");
    let db = &f.source.hub.store.db;
    assert_eq!(
        db.locked()
            .query_row(
                "SELECT account_id, input_tokens, cached_read_tokens, output_tokens FROM donna_turn_usage
             WHERE id=?1 AND session_id='background:delegacao' AND turn IS NULL",
                [dispatch.usage.get().unwrap()],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?)),
            )
            .unwrap(),
        ("fixture".into(), 10, 2, 3),
    );
    db.locked()
        .execute_batch(
            "CREATE TRIGGER deny_open_test BEFORE INSERT ON donna_session_results
        BEGIN SELECT RAISE(ABORT,'injected'); END;",
        )
        .unwrap();
    assert!(settle(&dispatch, &entries, Some(response), || false).is_err());
    assert_eq!(request_state(&f, &receipt.run_id).0, "running");
    assert_eq!(f.source.hub.store.entry(&entries[0]).unwrap().data["status"], "accepted");
    retornos::recuperar(&f.source.hub.store).unwrap();
    assert_eq!(request_state(&f, &receipt.run_id), ("interrupted".into(), Some("restart_uncertain".into())));
    assert!(matches!(dispatch.write(&mut || panic!("no replay")), Poll::Ready(Err(_))));
    assert_eq!(
        db.locked().query_row("SELECT COUNT(*) FROM entries WHERE kind='agent'", [], |r| r.get::<_, i64>(0)).unwrap(),
        0
    );
    ticket.release(&f.source.hub);
    f.source_ticket.release(&f.source.hub);
}

#[tokio::test]
async fn returns_only_committed_reply_and_discards_late_output_after_stop_or_timeout() {
    if !crate::environment::Environment::is_open() {
        return;
    }
    for case in ["complete", "source-stop", "target-stop", "timeout"] {
        let f = fixture();
        let intent = intent(&f);
        let ticket = open_chat::admit_for_test(&f.source.hub, &f.target, "fixture").unwrap();
        let a = ApprovedAsk { source: &f.source, intent: &intent, card: approved(&f, &intent).await };
        let (entries, receipt) = admit(&a, false).unwrap();
        let mut dispatch = Dispatch {
            source: &f.source,
            target: ticket.cancel(),
            receipt: &receipt,
            deadline: Instant::now() + Duration::from_secs(10),
            fence: AtomicI64::new(0),
            usage: std::sync::OnceLock::new(),
        };
        assert!(matches!(dispatch.write(&mut || Poll::Ready(Ok(()))), Poll::Ready(Ok(()))));
        match case {
            "source-stop" => open_chat::stop(&f.source.lane.chat),
            "target-stop" => open_chat::stop(&f.target),
            "timeout" => dispatch.deadline = Instant::now(),
            _ => {}
        }
        let reply = settle(&dispatch, &entries, Some("ephemeral".into()), || false).unwrap();
        let (status, why) = request_state(&f, &receipt.run_id);
        if case == "complete" {
            assert_eq!(reply.as_deref(), Some("ephemeral"));
            assert_eq!(status, "finished");
            assert!(
                retornos::privacidade::resultado_so_existe_em_memoria(
                    &f.source.hub.store.db.locked(),
                    &receipt.run_id,
                )
                .unwrap()
            );
        } else {
            assert_eq!(reply, None);
            assert_eq!(status, "cancelled");
            assert_eq!(
                why.as_deref(),
                Some(match case {
                    "source-stop" => "cancelled",
                    "target-stop" => "recipient_stopped",
                    _ => "timeout",
                })
            );
        }
        ticket.release(&f.source.hub);
        f.source_ticket.release(&f.source.hub);
    }
}
