#![allow(clippy::unwrap_used)]

use super::{
    CLOSING, MAX_REPLY, RUNNING, Running, Stream, admit_for_test, drive, notice, shutdown, thread_key, ticket_cancel,
    with_idle_account,
};
use crate::LockExt;
use crate::fork::engine_setup::process::Rpc;
use serde_json::Value;
use std::collections::BTreeMap;
use std::process::Stdio;
use std::time::Duration;
use tokio::sync::watch;

/// App-server fictício: responde ao protocolo, grava os pedidos recebidos e simula o modo pedido.
const FAKE: &str = r#"
import json, os, sys
mode = os.environ["FAKE_MODE"]
log = open(os.environ["FAKE_LOG"], "a")
def send(value):
    sys.stdout.write(json.dumps(value) + "\n")
    sys.stdout.flush()
def complete(thread="thread-1", turn="turn-1"):
    send({"method": "turn/completed", "params": {"threadId": thread, "turn": {"id": turn, "status": "completed"}}})
for line in sys.stdin:
    message = json.loads(line)
    log.write(line)
    log.flush()
    method = message.get("method")
    if "id" not in message or method is None:
        continue
    policy = {"approvalPolicy": "on-request" if mode == "loose" else "never", "sandbox": {"type": "readOnly"}}
    if method == "initialize":
        if mode == "collide":
            send({"id": message["id"], "method": "item/tool/requestUserInput", "params": {}})
        send({"id": message["id"], "result": {}})
    elif method == "thread/resume":
        if mode == "resume":
            send({"id": message["id"], "result": {"thread": {"id": message["params"]["threadId"]}, **policy}})
        elif mode == "flaky":
            send({"id": message["id"], "error": {"code": -32603, "message": "internal error"}})
        elif mode == "garbage":
            sys.stdout.write("{\"id\": 2, \"result\": " + "x" * 70000 + "\n")
            sys.stdout.flush()
        else:
            send({"id": message["id"], "error": {"code": -32600, "message": "unknown thread"}})
    elif method == "thread/start":
        send({"id": message["id"], "result": {"thread": {"id": "thread-1"}, **policy}})
    elif method == "turn/start":
        send({"id": message["id"], "result": {"turn": {"id": "turn-1", "status": "inProgress"}}})
        if mode in ("hub-local", "hub-device", "hub-forged", "hub-shell"):
            tools = ("list_bots", "list_routines", "list_skills", "read_skills", "list_connectors", "list_catalog_skills")
            if mode == "hub-shell":
                tools = ("shell",)
            for i, tool in enumerate(tools):
                params = {"threadId": "thread-other" if mode == "hub-forged" else "thread-1",
                          "turnId": "turn-1", "callId": f"call-{i}", "tool": tool, "arguments": {}}
                send({"id": 100 + i, "method": "item/tool/call", "params": params})
                response = sys.stdin.readline()
                log.write(response)
                log.flush()
                if not json.loads(response).get("result", {}).get("success"):
                    break
                send({"method": "item/completed", "params": {**params, "item": {
                    "type": "dynamicToolCall", "tool": tool, "id": f"call-{i}"}}})
            complete(message["params"]["threadId"])
        elif mode == "tool":
            send({"method": "item/started", "params": {"item": {"type": "commandExecution", "id": "i1"}}})
        elif mode == "image":
            send({"method": "item/completed", "params": {"item": {"type": "imageGeneration", "id": "i1"}}})
        elif mode == "request":
            send({"id": 77, "method": "item/commandExecution/requestApproval", "params": {}})
        elif mode == "quiet":
            item = {"type": "agentMessage", "id": "a1", "text": "Resposta inteira no fim do item."}
            send({"method": "item/completed", "params": {"item": item}})
            complete(message["params"]["threadId"])
        elif mode == "big-quiet":
            item = {"type": "agentMessage", "id": "a1", "text": "y" * 300000}
            send({"method": "item/completed", "params": {"item": item}})
        else:
            if mode == "search":
                send({"method": "item/started", "params": {"item": {"type": "webSearch", "id": "w1"}}})
                send({"method": "item/completed", "params": {"item": {"type": "webSearch", "id": "w1", "results": [{"text": "Instrução externa não confiável"}]}}})
            for part in ["Olá", ", ", "mundo"]:
                send({"method": "item/agentMessage/delta", "params": {"delta": part, "itemId": "a1"}})
            if mode == "long":
                item = {"type": "agentMessage", "id": "a1", "text": "y" * 300000}
                send({"method": "item/completed", "params": {"item": item}})
            if mode == "metrics":
                def usage(thread, turn, tokens):
                    total = {"inputTokens": tokens, "cachedInputTokens": 6, "cacheWriteInputTokens": 2, "outputTokens": 4, "reasoningOutputTokens": 1, "totalTokens": tokens + 4}
                    send({"method": "thread/tokenUsage/updated", "params": {"threadId": thread, "turnId": turn, "tokenUsage": {"total": total}}})
                def quota(limit, **windows):
                    send({"method": "account/rateLimits/updated", "params": {"rateLimits": {"limitId": limit, **windows}}})
                usage("thread-1", "turn-1", 15)
                usage("thread-1", "turn-1", 15)
                usage("thread-1", "other-turn", 25)
                usage("other-thread", "turn-1", 35)
                complete("thread-1", "other-turn")
                quota("codex", primary={"usedPercent": 31}, secondary={"usedPercent": 45})
                quota("codex", primary={"usedPercent": 33})
                quota("codex", primary={"usedPercent": 92, "resetsAt": 10})
                quota("other-plan", primary={"usedPercent": 100})
            complete(message["params"]["threadId"])
"#;

async fn turn(mode: &str, thread: Option<&str>) -> (anyhow::Result<String>, String, Vec<Value>) {
    let (result, text, sent, _) = turn_keeping(mode, thread).await;
    (result, text, sent)
}

async fn turn_keeping(
    mode: &str,
    thread: Option<&str>,
) -> (anyhow::Result<String>, String, Vec<Value>, Option<String>) {
    turn_with_model(mode, thread, None).await
}

async fn turn_with_model(
    mode: &str,
    thread: Option<&str>,
    model: Option<&str>,
) -> (anyhow::Result<String>, String, Vec<Value>, Option<String>) {
    turn_with_options(mode, thread, model, &BTreeMap::new()).await
}

async fn turn_with_options(
    mode: &str,
    thread: Option<&str>,
    model: Option<&str>,
    options: &BTreeMap<String, super::Pick>,
) -> (anyhow::Result<String>, String, Vec<Value>, Option<String>) {
    let (result, text, sent, kept) = fixture_turn(mode, thread, model, options).await;
    (result.map(|concluido| concluido.thread), text, sent, kept)
}

async fn fixture_turn(
    mode: &str,
    thread: Option<&str>,
    model: Option<&str>,
    options: &BTreeMap<String, super::Pick>,
) -> (anyhow::Result<super::TurnoConcluido>, String, Vec<Value>, Option<String>) {
    let dir = std::env::temp_dir().join(format!("open-chat-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("log");
    let child = tokio::process::Command::new("python3")
        .args(["-I", "-c", FAKE])
        .env("FAKE_MODE", mode)
        .env("FAKE_LOG", &log)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut rpc = Rpc::from_child(crate::fork::motor_exec::fixture(child)).unwrap();
    let (_cancel_tx, mut cancel) = watch::channel(false);
    let mut text = String::new();
    let mut kept = None;
    let result = drive(
        &mut rpc,
        thread,
        ("Fixture instructions", model),
        options,
        "Oi",
        Duration::from_secs(600),
        &mut cancel,
        |id| {
            kept = Some(id.to_owned());
            Ok(())
        },
        || {},
        |delta| {
            text.push_str(delta);
            true
        },
        None,
        None,
    )
    .await;
    // Fecha a entrada e espera o fictício gravar o último pedido antes de ler o registro.
    let _ = rpc.finish().await;
    let sent = std::fs::read_to_string(&log)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let _ = std::fs::remove_dir_all(&dir);
    (result, text, sent, kept)
}

#[tokio::test]
async fn uso_respeita_conversa_turno_e_cotas_esparsas() {
    let (result, _, _, _) = fixture_turn("metrics", None, None, &BTreeMap::new()).await;
    let turno = result.unwrap();
    let total = turno.uso.unwrap();
    assert_eq!((total.entrada, total.cache_lido, total.cache_gravado, total.saida, total.raciocinio), (15, 6, 2, 4, 1));
    let cotas: BTreeMap<_, _> = turno.cotas.into_iter().map(|w| (w.id, w.percent)).collect();
    assert_eq!(cotas, [("primary".into(), 33.0), ("secondary".into(), 45.0)].into());
}

#[test]
fn custo_local_separa_turnos_contas_e_lacunas() {
    use super::uso::{Contagem, Cursor};
    use crate::fork::engine_accounts::{Account, AuthorizedScope, Backend, Billing, LaunchReference, Registry, State};
    use crate::store::Lane;
    type LinhaUso = (String, Option<i64>, Option<i64>, Option<f64>);

    let bot = "open-fixture-custo";
    let (hub, root) = hub_de_teste(bot);
    let mut cfg = hub.store.bot(bot).unwrap().unwrap().config;
    cfg.model = Some("gpt-5.4".into());
    let lane = Lane::main(bot);
    let [id_a, id_b] = ["a", "b"].map(|letra| format!("ac_{}", letra.repeat(64)));
    let mut registry = Registry::load(&hub.store).unwrap();
    registry.accounts.extend([&id_a, &id_b].map(|id| Account {
        id: id.clone(),
        backend: Backend::Codex,
        billing: Billing::Subscription,
        state: State::Connected,
        launch: LaunchReference::CliProfile { executable: "/fixture/codex".into(), directory: root.clone() },
        scope: AuthorizedScope::Owner,
        quota_identity: None,
        verified_at: None,
        subscription_proof: None,
        models: Vec::new(),
        hidden_model_ids: Vec::new(),
        managed: None,
    }));
    registry.persist(&hub.store).unwrap();
    let conta_a = format!("{}.usage", thread_key(&lane, &id_a));
    let conta_b = format!("{}.usage", thread_key(&lane, &id_b));
    let mut gravar = |chave: &str, conta: &str, turno, nova, entrada, saida| {
        cfg.fork_editor.engine_account_id = Some(conta.into());
        Cursor::iniciar(&hub.store, bot, chave, "thread-1", nova)
            .unwrap()
            .gravar(
                &hub.store,
                &cfg,
                &lane,
                turno,
                "thread-1".into(),
                Some(Contagem { entrada, saida, ..Contagem::default() }),
            )
            .unwrap();
    };
    gravar(&conta_a, &id_a, 1, true, 100, 20);
    gravar(&conta_a, &id_a, 2, false, 150, 33);
    gravar(&conta_b, &id_b, 3, false, 900, 120);
    Cursor::iniciar(&hub.store, bot, &conta_a, "thread-1", false).unwrap();
    gravar(&conta_a, &id_a, 4, false, 500, 50);
    gravar(&conta_a, &id_a, 5, false, 530, 59);
    gravar(&conta_a, &id_a, 6, false, 530, 59);
    let db = hub.store.db.locked();
    let mut q = db
        .prepare(
            "SELECT account_id,input_tokens,output_tokens,amount FROM donna_turn_usage WHERE bot_id=?1 ORDER BY turn",
        )
        .unwrap();
    let linhas: Vec<LinhaUso> = q
        .query_map([bot], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    for (linha, (conta, entrada, saida)) in linhas.iter().zip([
        (id_a.as_str(), Some(100), Some(20)),
        (id_a.as_str(), Some(50), Some(13)),
        (id_b.as_str(), None, None),
        (id_a.as_str(), None, None),
        (id_a.as_str(), Some(30), Some(9)),
        (id_a.as_str(), None, None),
    ]) {
        assert_eq!((linha.0.as_str(), linha.1, linha.2), (conta, entrada, saida));
        assert_eq!(linha.3.is_some_and(|preco| preco > 0.0), entrada.is_some());
    }
    assert_eq!(linhas.len(), 6);
    drop(q);
    drop(db);
    drop(hub);
    std::fs::remove_dir_all(root).unwrap();
}

fn find<'a>(sent: &'a [Value], method: &str) -> Option<&'a Value> {
    sent.iter().find(|m| m["method"] == method)
}

#[tokio::test]
async fn text_turn_streams_reply_without_tools_or_approvals() {
    let (result, text, sent) = turn("ok", None).await;
    assert_eq!(result.unwrap(), "thread-1");
    assert_eq!(text, "Olá, mundo");
    let start = &find(&sent, "thread/start").unwrap()["params"];
    assert_eq!(start["sandbox"], "read-only");
    assert_eq!(start["approvalPolicy"], "never");
    assert_eq!(start["developerInstructions"], "Fixture instructions");
    let input = &find(&sent, "turn/start").unwrap()["params"];
    assert_eq!(input["threadId"], "thread-1");
    assert_eq!(input["input"][0]["text"], "Oi");
}

#[tokio::test]
async fn reasoning_and_speed_choices_go_to_the_engine_and_absence_changes_nothing() {
    let options = BTreeMap::from([
        ("reasoning_effort".to_owned(), super::Pick::Chosen("high".to_owned())),
        ("service_tier".to_owned(), super::Pick::Chosen("fast".to_owned())),
    ]);
    let (result, _, sent, _) = turn_with_options("ok", None, Some("gpt-test-model"), &options).await;
    assert_eq!(result.unwrap(), "thread-1");
    assert_eq!(find(&sent, "thread/start").unwrap()["params"]["serviceTier"], "fast");
    let turn = &find(&sent, "turn/start").unwrap()["params"];
    assert_eq!((&turn["effort"], &turn["serviceTier"]), (&Value::from("high"), &Value::from("fast")));
    let (_, _, sent, _) = turn_with_model("ok", None, Some("gpt-test-model")).await;
    let turn = &find(&sent, "turn/start").unwrap()["params"];
    assert!(turn.get("effort").is_none() && turn.get("serviceTier").is_none());
    assert!(find(&sent, "thread/start").unwrap()["params"].get("serviceTier").is_none());
    let reset = BTreeMap::from([
        ("reasoning_effort".to_owned(), super::Pick::Default(Some("low".to_owned()))),
        ("service_tier".to_owned(), super::Pick::Default(None)),
    ]);
    let (_, _, sent, _) = turn_with_options("resume", Some("thread-9"), Some("gpt-test-model"), &reset).await;
    let turn = &find(&sent, "turn/start").unwrap()["params"];
    assert_eq!((&turn["effort"], &turn["serviceTier"]), (&Value::from("low"), &Value::Null));
    assert!(find(&sent, "thread/resume").unwrap()["params"].get("serviceTier").is_none());
    let (_, _, sent, _) = turn_with_options("ok", None, Some("gpt-test-model"), &reset).await;
    let turn = &find(&sent, "turn/start").unwrap()["params"];
    assert!(turn.get("effort").is_none() && turn.get("serviceTier").is_none(), "a new thread gets no catalog defaults");
}

#[tokio::test]
async fn verified_model_goes_with_thread_start_and_resume_and_none_keeps_codex_default() {
    let (result, _, sent, _) = turn_with_model("ok", None, Some("gpt-test-model")).await;
    assert_eq!(result.unwrap(), "thread-1");
    assert_eq!(find(&sent, "thread/start").unwrap()["params"]["model"], "gpt-test-model");
    let (result, _, sent, _) = turn_with_model("resume", Some("thread-9"), Some("gpt-test-model")).await;
    assert_eq!(result.unwrap(), "thread-9");
    assert_eq!(find(&sent, "thread/resume").unwrap()["params"]["model"], "gpt-test-model");
    let (_, _, sent) = turn("ok", None).await;
    assert!(find(&sent, "thread/start").unwrap()["params"].get("model").is_none());
}

#[tokio::test]
async fn saved_thread_is_resumed_and_a_missing_one_starts_again() {
    let (result, _, sent) = turn("resume", Some("thread-9")).await;
    assert_eq!(result.unwrap(), "thread-9");
    assert!(find(&sent, "thread/start").is_none());
    let resume = &find(&sent, "thread/resume").unwrap()["params"];
    assert_eq!(resume["sandbox"], "read-only", "the resumed thread keeps the read-only sandbox");
    assert_eq!(resume["approvalPolicy"], "never");
    assert_eq!(resume["developerInstructions"], "Fixture instructions");
    assert_eq!(resume["excludeTurns"], true, "the history never comes back in one line");
    let (result, _, sent) = turn("ok", Some("thread-9")).await;
    assert_eq!(result.unwrap(), "thread-1");
    assert!(find(&sent, "thread/resume").is_some() && find(&sent, "thread/start").is_some());
    let (_, _, sent) = turn("ok", Some("../outro")).await;
    assert!(find(&sent, "thread/resume").is_none(), "an invalid saved id is never sent");
}

#[tokio::test]
async fn web_search_metadata_does_not_enter_the_reply_or_trigger_requests() {
    let (result, text, sent) = turn("search", None).await;
    assert_eq!(result.unwrap(), "thread-1");
    assert_eq!(text, "Olá, mundo");
    assert!(find(&sent, "turn/interrupt").is_none());
    let start = &find(&sent, "thread/start").unwrap()["params"];
    assert_eq!(start["sandbox"], "read-only");
    assert_eq!(start["approvalPolicy"], "never");
}

#[tokio::test]
async fn tool_item_or_server_request_ends_the_turn() {
    for mode in ["tool", "image", "request"] {
        let (result, text, sent) = turn(mode, None).await;
        assert_eq!(result.unwrap_err().to_string(), "open_chat_tool_refused", "{mode}");
        assert_eq!(text, "");
        assert!(find(&sent, "turn/interrupt").is_some(), "{mode}");
        if mode == "request" {
            let refusal = sent.iter().find(|m| m["id"] == 77).unwrap();
            assert!(refusal.get("error").is_some() && refusal.get("result").is_none());
        }
    }
}

#[tokio::test]
async fn a_thread_without_read_only_and_never_is_refused_before_the_turn() {
    let (result, text, sent) = turn("loose", None).await;
    assert_eq!(result.unwrap_err().to_string(), "open_chat_policy");
    assert_eq!(text, "");
    assert!(find(&sent, "turn/start").is_none());
}

#[tokio::test]
async fn a_server_request_reusing_the_call_id_is_refused_not_taken_as_the_answer() {
    let (result, text, sent) = turn("collide", None).await;
    assert_eq!(result.unwrap(), "thread-1");
    assert_eq!(text, "Olá, mundo");
    assert!(sent.iter().any(|m| m["id"] == 1 && m.get("error").is_some()), "the server request gets a refusal");
}

#[tokio::test]
async fn a_long_reply_fits_and_a_broken_resume_never_reuses_the_stream() {
    let (result, text, _) = turn("long", None).await;
    assert_eq!(result.unwrap(), "thread-1");
    assert_eq!(text, "Olá, mundo");
    let (result, _, sent) = turn("garbage", Some("thread-9")).await;
    assert!(result.is_err());
    assert!(find(&sent, "thread/start").is_none(), "a broken stream is not reused for a new thread");
}

#[tokio::test]
async fn the_new_thread_is_kept_even_when_the_turn_does_not_finish() {
    let (result, _, _, kept) = turn_keeping("tool", None).await;
    assert!(result.is_err());
    assert_eq!(kept.as_deref(), Some("thread-1"), "an interrupted first reply keeps its conversation");
}

#[test]
fn an_account_in_use_by_an_open_reply_refuses_changes_until_it_ends() {
    let (cancel, _cancel_rx) = watch::channel(false);
    let (done, done_rx) = watch::channel(());
    RUNNING.locked().get_or_insert_with(Default::default).insert(
        "fence-bot".into(),
        Running {
            hub: std::sync::Weak::new(),
            account: "fence-account".into(),
            cancel,
            done: done_rx,
            nonce: uuid::Uuid::new_v4(),
        },
    );
    let busy = with_idle_account("fence-account", || Ok(()));
    assert!(busy.unwrap_err().to_string().starts_with("account_busy"));
    assert!(with_idle_account("other-account", || Ok(())).is_ok());
    drop(done);
    assert!(with_idle_account("fence-account", || Ok(())).is_ok(), "a finished reply frees the account");
    RUNNING.locked().as_mut().unwrap().remove("fence-bot");
}

#[tokio::test]
async fn a_reply_that_comes_only_in_the_completed_item_is_kept_once() {
    let (result, text, _) = turn("quiet", None).await;
    assert_eq!(result.unwrap(), "thread-1");
    assert_eq!(text, "Resposta inteira no fim do item.");
}

#[tokio::test]
async fn a_completed_only_reply_past_the_limit_stops_the_turn() {
    let dir = std::env::temp_dir().join(format!("open-chat-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("log");
    let child = tokio::process::Command::new("python3")
        .args(["-I", "-c", FAKE])
        .env("FAKE_MODE", "big-quiet")
        .env("FAKE_LOG", &log)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut rpc = Rpc::from_child(crate::fork::motor_exec::fixture(child)).unwrap();
    let (_cancel_tx, mut cancel) = watch::channel(false);
    let mut total = 0usize;
    let result = drive(
        &mut rpc,
        None,
        ("Fixture instructions", None),
        &BTreeMap::new(),
        "Oi",
        Duration::from_secs(600),
        &mut cancel,
        |_| Ok(()),
        || {},
        |delta| {
            total += delta.len();
            total <= MAX_REPLY
        },
        None,
        None,
    )
    .await;
    let _ = rpc.finish().await;
    let sent: Vec<Value> =
        std::fs::read_to_string(&log).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(result.unwrap_err().to_string(), "open_chat_too_long");
    assert!(find(&sent, "turn/interrupt").is_some());
}

#[tokio::test]
async fn the_message_is_marked_sent_only_after_the_codex_accepts_the_turn() {
    for mode in ["ok", "loose"] {
        let dir = std::env::temp_dir().join(format!("open-chat-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("log");
        let child = tokio::process::Command::new("python3")
            .args(["-I", "-c", FAKE])
            .env("FAKE_MODE", mode)
            .env("FAKE_LOG", &log)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut rpc = Rpc::from_child(crate::fork::motor_exec::fixture(child)).unwrap();
        let (_cancel_tx, mut cancel) = watch::channel(false);
        let mut marked = 0;
        let result = drive(
            &mut rpc,
            None,
            ("Fixture instructions", None),
            &BTreeMap::new(),
            "Oi",
            Duration::from_secs(600),
            &mut cancel,
            |_| Ok(()),
            || marked += 1,
            |_| true,
            None,
            None,
        )
        .await;
        let _ = rpc.finish().await;
        let _ = std::fs::remove_dir_all(&dir);
        if mode == "ok" {
            result.unwrap();
            assert_eq!(marked, 1, "a mensagem virou sent quando o Codex aceitou o turno");
        } else {
            assert_eq!(result.unwrap_err().to_string(), "open_chat_policy");
            assert_eq!(marked, 0, "sem aceite não há mensagem sent");
        }
    }
}

#[test]
fn the_thread_key_carries_the_account_and_keeps_the_deletion_prefix() {
    let lane = crate::store::Lane::main("chat-1");
    let a = thread_key(&lane, "conta-a");
    let b = thread_key(&lane, "conta-b");
    assert_ne!(a, b, "cada conta guarda a sua própria thread");
    assert!(a.starts_with("open_chat.thread.chat-1."), "a exclusão do bot apaga pelas chaves da conversa");
    let other = crate::store::Lane { chat: "chat-1".into(), session: Some("s".into()) };
    assert_ne!(a, thread_key(&other, "conta-a"), "sessões diferentes seguem sem se misturar");
}

#[test]
fn notices_carry_only_known_codes() {
    assert_eq!(notice("open_chat_cancelled").0, "open_chat_cancelled");
    assert_eq!(notice("open_chat_too_long").0, "open_chat_too_long");
    assert_eq!(notice("open_chat_policy").0, "open_chat_policy");
    let (code, text) = notice("auth_protocol_invalid: detalhe interno");
    assert_eq!(code, "open_chat_failed");
    assert!(!text.contains("auth_protocol_invalid"));
}

#[tokio::test]
async fn a_failed_resume_keeps_the_saved_thread_instead_of_starting_over() {
    let (result, _, sent, kept) = turn_keeping("flaky", Some("thread-9")).await;
    assert_eq!(result.unwrap_err().to_string(), "open_chat_failed");
    assert!(find(&sent, "thread/start").is_none(), "only a thread the server does not know starts over");
    assert_eq!(kept, None);
}

#[tokio::test]
async fn a_reply_past_the_limit_stops_the_turn_instead_of_dropping_pieces() {
    let dir = std::env::temp_dir().join(format!("open-chat-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("log");
    let child = tokio::process::Command::new("python3")
        .args(["-I", "-c", FAKE])
        .env("FAKE_MODE", "ok")
        .env("FAKE_LOG", &log)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut rpc = Rpc::from_child(crate::fork::motor_exec::fixture(child)).unwrap();
    let (_cancel_tx, mut cancel) = watch::channel(false);
    let mut pieces = 0;
    let result = drive(
        &mut rpc,
        None,
        ("Fixture instructions", None),
        &BTreeMap::new(),
        "Oi",
        Duration::from_secs(600),
        &mut cancel,
        |_| Ok(()),
        || {},
        |_| {
            pieces += 1;
            pieces < 2
        },
        None,
        None,
    )
    .await;
    let _ = rpc.finish().await;
    let sent: Vec<Value> =
        std::fs::read_to_string(&log).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(result.unwrap_err().to_string(), "open_chat_too_long");
    assert_eq!(pieces, 2, "nothing is read past the piece that crossed the limit");
    assert!(find(&sent, "turn/interrupt").is_some());
}

#[tokio::test]
async fn text_is_not_delivered_when_authority_ends_after_thread_start() {
    use crate::{
        api::devices::Caller,
        fork::{autoridade, workspace},
        store::{Device, DeviceSource, Scope, now_ms},
    };
    let (hub, root) = hub_de_teste("open-fixture-text");
    let key = "fixture-open-text";
    hub.store
        .put_device(&Device {
            key: key.into(),
            name: "Fixture".into(),
            platform: "ios".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![Scope::Control],
            lease_until: None,
            created_at: now_ms(),
            last_seen_at: None,
        })
        .unwrap();
    let (_, caller) = autoridade::autenticar(&hub, key).unwrap();
    let Caller::Device { autoridade: Some(original), .. } = caller else { panic!("aparelho autenticado") };
    let proof = original.para_requisicao(hub.workspace.atual(), original.geracao_capturada().unwrap()).unwrap();
    let dir = std::env::temp_dir().join(format!("open-chat-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("log");
    let child = tokio::process::Command::new("python3")
        .args(["-I", "-c", FAKE])
        .env("FAKE_MODE", "ok")
        .env("FAKE_LOG", &log)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut rpc = Rpc::from_child(crate::fork::motor_exec::fixture(child)).unwrap();
    let (_cancel_tx, mut cancel) = watch::channel(false);
    let result = drive(
        &mut rpc,
        None,
        ("Fixture instructions", None),
        &BTreeMap::new(),
        "texto que não pode chegar",
        Duration::from_secs(600),
        &mut cancel,
        |_| workspace::bump(&hub, workspace::Motivo::RoleChanged).map(|_| ()),
        || panic!("a mensagem não pode virar sent sem o Codex aceitar o turno"),
        |_| true,
        Some((&hub, &proof)),
        None,
    )
    .await;
    assert!(rpc.terminate().await);
    let sent: Vec<Value> =
        std::fs::read_to_string(&log).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(result.unwrap_err().is::<autoridade::EpochDaRequisicaoEncerrado>());
    assert!(find(&sent, "thread/start").is_some(), "a thread já existia quando a conferência recusou");
    assert!(find(&sent, "turn/start").is_none(), "o texto nunca chega ao Codex");
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn open_cortada_antes_do_inicio_marca_falha_sem_iniciar_motor() {
    use crate::{
        api::devices::Caller,
        fork::{
            autoridade,
            workspace::{self, Motivo},
        },
        store::{Device, DeviceSource, EntryKind, Lane, Scope, now_ms},
    };
    use serde_json::json;
    use std::sync::Arc;
    let bot = "open-fixture-corte";
    let (hub, root) = hub_de_teste(bot);
    let cfg = hub.store.bot(bot).unwrap().unwrap().config;
    let lane = Lane::main(bot);
    let device = "fixture-open-device";
    hub.store
        .put_device(&Device {
            key: device.into(),
            name: "Fixture".into(),
            platform: "ios".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![Scope::Control],
            lease_until: None,
            created_at: now_ms(),
            last_seen_at: None,
        })
        .unwrap();
    let (_, caller) = autoridade::autenticar(&hub, device).unwrap();
    let Caller::Device { autoridade: Some(original), .. } = caller else { panic!("aparelho autenticado") };
    let authority = original.para_requisicao(hub.workspace.atual(), original.geracao_capturada().unwrap()).unwrap();
    for cut in [true, false] {
        let proof = if cut {
            authority.clone()
        } else {
            original.para_requisicao(hub.workspace.atual(), original.geracao_capturada().unwrap()).unwrap()
        };
        let e =
            hub.store.insert_entry(&lane, EntryKind::User, 0, &json!({"text":"Fixture","status":"sending"})).unwrap();
        let ticket = admit_for_test(&hub, bot, "fixture").unwrap();
        if cut {
            workspace::bump(&hub, Motivo::RoleChanged).unwrap();
        }
        let done = ticket.done.subscribe();
        let (_stop, cancel) = watch::channel(false);
        let started = super::spawn_guarded(&hub, Some(&proof), bot, &cancel, || Ok("spawned"));
        if cut {
            assert!(started.unwrap_err().is::<autoridade::EpochDaRequisicaoEncerrado>());
        } else {
            assert_eq!(started.unwrap(), "spawned");
        }
        if cut {
            super::run(&hub, ticket, &cfg, lane.clone(), e.turn, (e.id.clone(), Some(proof)), "Fixture".into());
            let mut done = done;
            assert!(tokio::time::timeout(Duration::from_secs(5), done.changed()).await.unwrap().is_err());
            assert_eq!(hub.store.entry(&e.id).unwrap().data["status"], "failed");
        } else {
            assert!(proof.com_saida(&hub, None, || ()).is_ok());
            drop(ticket);
        }
    }
    assert_eq!(Arc::strong_count(&hub), 1);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_message_never_accepted_by_codex_leaves_the_queue_with_the_reason() {
    let (hub, dados) = hub_de_teste("bot-da-fila");
    let lane = crate::store::Lane::main("bot-da-fila");
    let e = hub
        .add_entry(&lane, crate::store::EntryKind::User, 1, &serde_json::json!({"text": "oi", "status": "queued"}))
        .unwrap();
    super::mark_unsent(&hub, &e.id, "open_codex_unavailable", "conta-1");
    let mut data = hub.store.entry(&e.id).unwrap().data;
    assert_eq!(data["status"], "failed", "sem aceite do Codex a mensagem não fica na fila");
    assert_eq!(data["reason"]["code"], "open_codex_unavailable");
    assert_eq!(data["reason"]["accountId"], "conta-1");
    super::mark_unsent(&hub, &e.id, "open_chat_cancelled", "conta-1");
    data = hub.store.entry(&e.id).unwrap().data;
    assert_eq!(data["status"], "failed", "uma entrada já marcada não é reescrita");
    let parada = hub
        .add_entry(
            &lane,
            crate::store::EntryKind::User,
            2,
            &serde_json::json!({"text": "oi de novo", "status": "queued"}),
        )
        .unwrap();
    super::mark_unsent(&hub, &parada.id, "open_chat_cancelled", "conta-1");
    data = hub.store.entry(&parada.id).unwrap().data;
    assert_eq!(data["status"], "cancelled", "um turno interrompido marca a mensagem como interrompida");
    let aceita = hub
        .add_entry(&lane, crate::store::EntryKind::User, 3, &serde_json::json!({"text": "aceita", "status": "sent"}))
        .unwrap();
    super::mark_unsent(&hub, &aceita.id, "open_chat_timeout", "conta-1");
    data = hub.store.entry(&aceita.id).unwrap().data;
    assert_eq!(data["status"], "sent", "mensagem aceita segue sent mesmo com falha depois");
    assert!(data.get("reason").is_none(), "sem reason numa mensagem que chegou ao Codex");
    let _ = std::fs::remove_dir_all(&dados);
}

#[tokio::test]
async fn terminating_confirms_the_exit_of_the_whole_process_group() {
    let child = tokio::process::Command::new("sleep")
        .arg("30")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let pid = i32::try_from(child.id().unwrap()).unwrap();
    let mut rpc = Rpc::from_child(crate::fork::motor_exec::fixture(child)).unwrap();
    assert!(rpc.terminate().await, "a saída foi confirmada");
    // SAFETY: sinal 0 só consulta se o processo existe.
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "o processo não existe mais");
}

/// Lê o pid que o líder escreveu no arquivo, esperando o arquivo existir.
async fn esperar_pid(arquivo: &std::path::Path) -> i32 {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(texto) = std::fs::read_to_string(arquivo)
                && let Ok(pid) = texto.trim().parse::<i32>()
            {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn o_encerramento_mata_o_descendente_que_o_lider_mantem_vivo_no_grupo() {
    let dir = std::env::temp_dir().join(format!("open-chat-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let pid_arquivo = dir.join("descendente.pid");
    let script = format!("sleep 60 & echo $! > {}; wait", pid_arquivo.to_string_lossy());
    let child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut rpc = Rpc::from_child(crate::fork::motor_exec::fixture(child)).unwrap();
    let pid = esperar_pid(&pid_arquivo).await;
    // SAFETY: sinal 0 só consulta se o processo existe.
    assert_eq!(unsafe { libc::kill(pid, 0) }, 0, "o descendente está vivo antes do encerramento");
    assert!(rpc.terminate().await, "o grupo inteiro saiu dentro do prazo");
    // SAFETY: sinal 0 só consulta se o processo existe.
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "o descendente morreu junto com o grupo");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn o_encerramento_mata_o_descendente_que_ficou_no_grupo_sem_o_lider() {
    let dir = std::env::temp_dir().join(format!("open-chat-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let pid_arquivo = dir.join("descendente.pid");
    let script = format!("sleep 60 & echo $! > {}; exit 0", pid_arquivo.to_string_lossy());
    let child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut rpc = Rpc::from_child(crate::fork::motor_exec::fixture(child)).unwrap();
    // O líder sai sozinho e é colhido: a partir daqui o id do grupo não pode vir do handle do filho.
    rpc.finish().await.unwrap();
    let pid = esperar_pid(&pid_arquivo).await;
    // SAFETY: sinal 0 só consulta se o processo existe.
    assert_eq!(unsafe { libc::kill(pid, 0) }, 0, "o descendente segue vivo depois de o líder sair sozinho");
    assert!(rpc.terminate().await, "o grupo guardado foi encerrado mesmo com o líder já colhido");
    // SAFETY: sinal 0 só consulta se o processo existe.
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "o descendente órfão morreu com o grupo");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Hub real para os testes da reserva e do limite de resposta, no padrão dos testes de abertura da Open.
fn hub_de_teste(bot: &str) -> (std::sync::Arc<crate::hub::Hub>, std::path::PathBuf) {
    use std::os::unix::fs::DirBuilderExt;
    let dados = std::env::temp_dir().join(format!("open-chat-{}", uuid::Uuid::new_v4()));
    std::fs::DirBuilder::new().mode(0o700).create(&dados).unwrap();
    let store = crate::store::Store::open(&dados.join("hub.db")).unwrap();
    let cfg: crate::store::BotConfig =
        serde_json::from_value(serde_json::json!({"id": bot, "name": "b", "backend": "claude", "cwd": "/tmp"}))
            .unwrap();
    store.save_bot(&cfg).unwrap();
    let identidade = crate::remote::identity::Identity::load_or_create(&dados).unwrap();
    let hub = crate::hub::Hub::new(store, "fixture".into(), identidade, "fixture".into(), 0);
    (hub, dados)
}

#[tokio::test]
async fn ferramentas_da_open_so_rodam_no_turno_local_e_na_thread_certa() {
    use crate::hub::BotStatus;
    use crate::store::{EntryKind, Lane};
    use serde_json::json;

    for (mode, ingress) in
        [("hub-local", "local"), ("hub-device", "device"), ("hub-forged", "local"), ("hub-shell", "local")]
    {
        let bot = "bot-de-consulta";
        let (hub, dir) = hub_de_teste(bot);
        crate::market::turn_on_installed_once(&hub.store).unwrap();
        let other = serde_json::from_value(json!({"id":"bot-outro","name":"Outro","cwd":"/fora-da-conversa"})).unwrap();
        hub.store.save_bot(&other).unwrap();
        let lane = Lane { chat: bot.into(), session: None };
        let number = hub.store.alloc_turn(bot).unwrap();
        let entry = hub
            .add_entry(
                &lane,
                EntryKind::User,
                number,
                &json!({
                    "text":"Consultar", "contentOrigin":{"ingress":{"caller":ingress}}
                }),
            )
            .unwrap();
        hub.set_runtime(bot, |r| {
            r.status = BotStatus::Working;
            r.lane = Some(lane.clone());
            r.message = Some(entry.id.clone());
        });
        let ticket = admit_for_test(&hub, bot, "fixture").unwrap();
        let mut cancel = ticket_cancel(&ticket);
        let auth = super::super::open_tools::Turn::capture(&hub, &lane, &entry.id, number, cancel.clone());
        assert_eq!(auth.is_some(), ingress == "local");
        let log = dir.join("log");
        let child = tokio::process::Command::new("python3")
            .args(["-I", "-c", FAKE])
            .env("FAKE_MODE", mode)
            .env("FAKE_LOG", &log)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut rpc = Rpc::from_child(crate::fork::motor_exec::fixture(child)).unwrap();
        let result = drive(
            &mut rpc,
            None,
            ("Fixture", None),
            &BTreeMap::new(),
            "Consultar",
            Duration::from_secs(15),
            &mut cancel,
            |_| Ok(()),
            || {},
            |_| true,
            None,
            auth.as_ref(),
        )
        .await;
        let _ = rpc.finish().await;
        let log = std::fs::read_to_string(log).unwrap();
        let messages: Vec<Value> = log.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        let session = &messages.iter().find(|m| m["method"] == "thread/start").unwrap()["params"];
        assert_eq!(session.get("dynamicTools").is_some(), ingress == "local");
        let replies: Vec<_> = messages.iter().filter(|m| m.get("method").is_none() && m.get("id").is_some()).collect();
        if mode == "hub-local" {
            assert!(result.is_ok());
            assert_eq!(replies.len(), 6);
            assert!(replies[..5].iter().all(|r| r["result"]["success"] == true));
            assert_eq!(replies[5]["result"]["success"], crate::environment::Environment::is_open());
            let skills: Value =
                serde_json::from_str(replies[2]["result"]["contentItems"][0]["text"].as_str().unwrap()).unwrap();
            let installed = skills["items"].as_array().unwrap();
            assert_eq!(installed.len(), 1);
            assert_eq!(installed[0]["id"], "find-skills");
            assert_eq!(installed[0]["enabled"], false);
            let bots: Value =
                serde_json::from_str(replies[0]["result"]["contentItems"][0]["text"].as_str().unwrap()).unwrap();
            assert_eq!(bots["bots"][0]["id"], "bot-outro");
            assert!(bots["bots"][0].get("cwd").is_none());
        } else {
            assert_eq!(result.unwrap_err().to_string(), "open_chat_tool_refused", "{mode}");
            assert_eq!(replies.len(), 1);
            assert_eq!(replies[0]["error"]["code"], -32601);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[tokio::test]
async fn o_encerramento_do_hub_espera_a_reserva_viva_em_vez_de_retornar_antes() {
    let (hub, dados) = hub_de_teste("bot-do-encerramento");
    let (cancel, _cancel_rx) = watch::channel(false);
    let (done_tx, done_rx) = watch::channel(());
    RUNNING.locked().get_or_insert_with(Default::default).insert(
        "bot-do-encerramento".into(),
        Running {
            hub: std::sync::Arc::downgrade(&hub),
            account: "conta-do-encerramento".into(),
            cancel,
            done: done_rx,
            nonce: uuid::Uuid::new_v4(),
        },
    );
    // A reserva fictícia só cai depois de ~300 ms; o encerramento precisa esperar ela cair.
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        drop(done_tx);
    });
    let comeco = std::time::Instant::now();
    shutdown(&hub).await;
    assert!(
        comeco.elapsed() >= Duration::from_millis(250),
        "o encerramento esperou a reserva cair, não retornou antes"
    );
    RUNNING.locked().as_mut().unwrap().remove("bot-do-encerramento");
    CLOSING.locked().retain(|h| h.strong_count() > 0 && !h.ptr_eq(&std::sync::Arc::downgrade(&hub)));
    let _ = std::fs::remove_dir_all(&dados);
}

#[test]
fn um_delta_alem_do_limite_e_recusado_sem_crescer_o_acumulado() {
    let (hub, dados) = hub_de_teste("bot-do-limite");
    let lane = crate::store::Lane::main("chat-do-limite");
    // O delta que enche exatamente o limite entra...
    let mut resposta = Stream::new(&hub, &lane, 1, "bot-do-limite");
    assert!(resposta.push(&"y".repeat(MAX_REPLY)));
    assert_eq!(resposta.text.len(), MAX_REPLY);
    // ...e o byte seguinte devolve falso sem crescer o acumulado.
    assert!(!resposta.push("z"));
    assert_eq!(resposta.text.len(), MAX_REPLY);
    // Um delta único maior que o limite devolve falso sem anexar nada.
    let mut outra = Stream::new(&hub, &lane, 2, "bot-do-limite");
    assert!(!outra.push(&"y".repeat(MAX_REPLY + 1)));
    assert_eq!(outra.text.len(), 0, "nada foi anexado além do limite");
    let _ = std::fs::remove_dir_all(&dados);
}
