#![allow(clippy::unwrap_used)]

use super::*;
use crate::{
    fork::{donna_fixa::pedido::Origem, engine_setup::process::Rpc, open_tool_actions::respond},
    store::EntryKind,
};
use std::process::Stdio;
use std::time::Duration;

const ACCOUNT: &str = "ac_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

struct Fixture {
    source: LocalTurn,
    source_ticket: Ticket,
    target: String,
    dir: tempfile::TempDir,
}

fn fixture() -> Fixture {
    use crate::fork::engine_accounts::{Account, AuthorizedScope, Backend, Billing, LaunchReference, Registry, State};
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::Builder::new().permissions(std::fs::Permissions::from_mode(0o700)).tempdir().unwrap();
    let store = Store::open(&dir.path().join("hub.db")).unwrap();
    let mut registry = Registry::load(&store).unwrap();
    registry.accounts.push(Account {
        id: ACCOUNT.into(),
        backend: Backend::Codex,
        billing: Billing::Subscription,
        state: State::Connected,
        launch: LaunchReference::CliProfile { executable: "/fixture/codex".into(), directory: dir.path().into() },
        scope: AuthorizedScope::Owner,
        quota_identity: None,
        verified_at: None,
        subscription_proof: None,
        models: Vec::new(),
        hidden_model_ids: Vec::new(),
        managed: None,
    });
    registry.persist(&store).unwrap();
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
    let source_ticket = open_chat::admit_for_test(&hub, &id, ACCOUNT).unwrap();
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
    Fixture { source, source_ticket, target, dir }
}

async fn approved(f: &Fixture, intent: &Intent<'_>) -> Entry {
    approved_for(&f.source, intent).await
}

async fn approved_for(source: &LocalTurn, intent: &Intent<'_>) -> Entry {
    let hub = &source.hub;
    let mut events = hub.events.subscribe();
    let (entry, ()) = tokio::join!(
        approve(hub, &source.lane, source.turn, approval_title(source.background), &intent.detail),
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
    let tool = if s.background { MESSAGE } else { ASK };
    let chain = equipe::Entrada::nova(
        &s.lane.chat,
        intent.target,
        id,
        tool,
        intent.message,
        intent.contract.as_ref(),
        s.parent.as_ref().map(|(receipt, _)| receipt.clone()),
    )?;
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
import json, os, sys
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
        assert "threadId" not in params
        if os.environ.get("FIXTURE_FORWARD"):
            assert [tool["name"] for tool in params["dynamicTools"]] == ["ask_bot"]
        else:
            assert "dynamicTools" not in params
        assert params["sandbox"] == "read-only" and params["approvalPolicy"] == "never"
        result = {"thread":{"id":"target-thread","ephemeral":True},"sandbox":{"type":"readOnly"},"approvalPolicy":"never"}
    elif method == "turn/start":
        assert params["threadId"] == "target-thread"
        assert params["input"] == [{"type":"text","text":"Pedido fixo","text_elements":[]}]
        result = {"turn":{"id":"clean-turn"}}
    else:
        raise AssertionError(method)
    send({"id":request["id"],"result":result})
    if method == "turn/start":
        send({"method":"item/agentMessage/delta","params":{
            "threadId":"target-thread","turnId":"clean-turn","itemId":"a1","delta":os.environ.get("FIXTURE_REPLY", "Resposta isolada")}})
        send({"method":"thread/tokenUsage/updated","params":{
            "threadId":"target-thread","turnId":"clean-turn","tokenUsage":{"total":{
            "totalTokens":15,"inputTokens":12,"cachedInputTokens":2,"outputTokens":3,"reasoningOutputTokens":1}}}})
        send({"method":"turn/completed","params":{
            "threadId":"target-thread","turn":{"id":"clean-turn","status":"completed"}}})
"#;

#[tokio::test]
async fn async_turn_groups_two_requests_after_seal_and_never_replays() {
    if !crate::environment::Environment::is_open() {
        return;
    }
    for restart_between in [false, true] {
        let mut f = fixture();
        f.source.background = true;
        let hub = &f.source.hub;
        let intent = intent(&f);
        let first = ApprovedAsk { source: &f.source, intent: &intent, card: approved(&f, &intent).await };
        let (entries, receipt) = admit(&first, false).unwrap();
        let ticket = open_chat::admit_for_test(hub, &f.target, ACCOUNT).unwrap();
        let dispatch = Dispatch {
            source: &f.source,
            target: ticket.cancel(),
            receipt: &receipt,
            deadline: Instant::now() + Duration::from_secs(10),
            fence: AtomicI64::new(0),
            usage: std::sync::OnceLock::new(),
        };
        let text_expected =
            if restart_between { "PERGUNTA_AO_SOLICITANTE: Qual é o período?" } else { "Resposta isolada" };
        let child = tokio::process::Command::new("python3")
            .args(["-I", "-c", APP_SERVER])
            .env("FIXTURE_FORWARD", "1")
            .env("FIXTURE_REPLY", text_expected)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut rpc = Rpc::from_child(crate::fork::motor_exec::fixture(child)).unwrap();
        let text = open_chat::drive_delegated(&mut rpc, &ticket, "Área", "Pedido fixo", &dispatch).await.unwrap();
        rpc.finish().await.unwrap();
        assert_eq!(settle(&dispatch, &entries, Some(text), || false).unwrap().as_deref(), Some(text_expected));
        assert!(hub.store.history(&f.source.lane.chat, i64::MAX, 100).unwrap().iter().all(|e| e.kind != "agent"));
        let second = ApprovedAsk { source: &f.source, intent: &intent, card: approved(&f, &intent).await };
        let (entries, receipt) = admit(&second, false).unwrap();
        retornos::entrega::seal(hub, &f.source.lane, f.source.turn);
        assert!(hub.store.history(&f.source.lane.chat, i64::MAX, 100).unwrap().iter().all(|e| e.kind != "agent"));
        if !restart_between {
            let dispatch = Dispatch {
                source: &f.source,
                target: ticket.cancel(),
                receipt: &receipt,
                deadline: Instant::now() + Duration::from_secs(10),
                fence: AtomicI64::new(0),
                usage: std::sync::OnceLock::new(),
            };
            assert!(matches!(dispatch.write(&mut || Poll::Ready(Ok(()))), Poll::Ready(Ok(()))));
            assert!(settle(&dispatch, &entries, None, || false).unwrap().is_none());
        }
        ticket.release(hub);
        f.source_ticket.release(hub);
        let reopened = Store::open(&f.dir.path().join("hub.db")).unwrap();
        for _ in 0..2 {
            retornos::recuperar(&reopened).unwrap();
            let result: Vec<_> = reopened
                .history(&f.source.lane.chat, i64::MAX, 100)
                .unwrap()
                .into_iter()
                .filter(|e| e.kind == "agent")
                .collect();
            assert_eq!(result.len(), 1);
            let text = result[0].data["text"].as_str().unwrap();
            assert!(text.contains(if restart_between {
                "Pergunta de Destino:\nQual é o período?"
            } else {
                text_expected
            }));
            assert!(text.contains(if restart_between { "Interrompido antes do envio" } else { "Não concluiu" }));
            assert_eq!(result[0].turn, f.source.turn);
        }
    }
}

#[tokio::test]
async fn forwarded_local_request_cannot_revisit_ancestor_and_requires_its_own_card() {
    if !crate::environment::Environment::is_open() {
        return;
    }
    let mut f = fixture();
    f.source.background = true;
    let hub = &f.source.hub;
    let intent = intent(&f);
    let approval = ApprovedAsk { source: &f.source, intent: &intent, card: approved(&f, &intent).await };
    let (_, receipt) = admit(&approval, false).unwrap();
    let target = open_chat::admit_for_test(hub, &f.target, ACCOUNT).unwrap();
    let dispatch = Dispatch {
        source: &f.source,
        target: target.cancel(),
        receipt: &receipt,
        deadline: Instant::now() + Duration::from_secs(10),
        fence: AtomicI64::new(0),
        usage: std::sync::OnceLock::new(),
    };
    assert!(matches!(dispatch.write(&mut || Poll::Ready(Ok(()))), Poll::Ready(Ok(()))));
    {
        let db = hub.store.db.locked();
        let back =
            equipe::Entrada::nova(&f.target, &f.source.lane.chat, "back", ASK, "Volte", None, Some(receipt.clone()))
                .unwrap();
        let session = main_session(&db, &f.target).unwrap();
        assert_eq!(
            equipe::admitir_open_na(
                &db,
                &back,
                &f.source.space,
                &session,
                "hub-open:delegated:fixture",
                Some(&receipt)
            )
            .unwrap_err()
            .to_string(),
            "team_delegation_cycle"
        );
    }
    assert!(dispatch.forward(&json!({"botId": f.target, "message":"Sem autorização"})).await.is_err());
    let forwarded = dispatch.forwarded_turn().unwrap();
    let specialist = uuid::Uuid::new_v4().to_string();
    hub.store
        .save_bot(&serde_json::from_value(json!({"id":specialist,"name":"Especialista","backend":"codex"})).unwrap())
        .unwrap();
    let next = Intent {
        target: &specialist,
        target_name: "Especialista",
        message: "Confira apenas o total.",
        contract: None,
        detail: serde_json::to_string_pretty(
            &json!({"bot":"Especialista","botId":specialist,"message":"Confira apenas o total.","contract":null}),
        )
        .unwrap(),
    };
    let wrong = ApprovedAsk { source: &forwarded, intent: &next, card: approval.card };
    assert!(admit(&wrong, false).is_err());
    let fresh = ApprovedAsk { source: &forwarded, intent: &next, card: approved_for(&forwarded, &next).await };
    let (notices, child) = admit(&fresh, false).unwrap();
    assert_eq!(child.chain_id, receipt.chain_id);
    assert_eq!(child.level, receipt.level + 1);
    let worker = open_chat::admit_for_test(hub, &specialist, ACCOUNT).unwrap();
    let onward = Dispatch {
        source: &forwarded,
        target: worker.cancel(),
        receipt: &child,
        deadline: Instant::now() + Duration::from_secs(10),
        fence: AtomicI64::new(0),
        usage: std::sync::OnceLock::new(),
    };
    assert!(matches!(onward.write(&mut || Poll::Ready(Ok(()))), Poll::Ready(Ok(()))));
    assert_eq!(
        settle(&onward, &notices, Some("Total conferido.".into()), || false).unwrap().as_deref(),
        Some("Total conferido.")
    );
    worker.release(hub);
    target.release(hub);
    f.source_ticket.release(hub);
}

#[tokio::test]
async fn isolated_reply_is_held_on_commit_failure_and_not_replayed_after_restart() {
    if !crate::environment::Environment::is_open() {
        return;
    }
    let f = fixture();
    let intent = intent(&f);
    let ticket = open_chat::admit_for_test(&f.source.hub, &f.target, ACCOUNT).unwrap();
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
        (ACCOUNT.into(), 10, 2, 3),
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
        let ticket = open_chat::admit_for_test(&f.source.hub, &f.target, ACCOUNT).unwrap();
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
