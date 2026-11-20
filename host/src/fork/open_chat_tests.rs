#![allow(clippy::unwrap_used)]

use super::drive;
use crate::fork::engine_setup::process::Rpc;
use serde_json::Value;
use std::process::Stdio;
use tokio::sync::watch;

/// App-server fictício: responde ao protocolo, grava os pedidos recebidos e simula o modo pedido.
const FAKE: &str = r#"
import json, os, sys
mode = os.environ["FAKE_MODE"]
log = open(os.environ["FAKE_LOG"], "a")
def send(value):
    sys.stdout.write(json.dumps(value) + "\n")
    sys.stdout.flush()
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
        elif mode == "garbage":
            sys.stdout.write("{\"id\": 2, \"result\": " + "x" * 70000 + "\n")
            sys.stdout.flush()
        else:
            send({"id": message["id"], "error": {"code": -32600, "message": "unknown thread"}})
    elif method == "thread/start":
        send({"id": message["id"], "result": {"thread": {"id": "thread-1"}, **policy}})
    elif method == "turn/start":
        send({"id": message["id"], "result": {"turn": {"id": "turn-1", "status": "inProgress"}}})
        if mode == "tool":
            send({"method": "item/started", "params": {"item": {"type": "commandExecution", "id": "i1"}}})
        elif mode == "request":
            send({"id": 77, "method": "item/commandExecution/requestApproval", "params": {}})
        else:
            for part in ["Olá", ", ", "mundo"]:
                send({"method": "item/agentMessage/delta", "params": {"delta": part, "itemId": "a1"}})
            if mode == "long":
                item = {"type": "agentMessage", "id": "a1", "text": "y" * 300000}
                send({"method": "item/completed", "params": {"item": item}})
            send({"method": "turn/completed", "params": {"turn": {"id": "turn-1", "status": "completed"}}})
"#;

async fn turn(mode: &str, thread: Option<&str>) -> (anyhow::Result<String>, String, Vec<Value>) {
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
    let result = drive(&mut rpc, thread, "Fixture instructions", "Oi", &mut cancel, |delta| text.push_str(delta)).await;
    // Fecha a entrada e espera o fictício gravar o último pedido antes de ler o registro.
    let _ = rpc.finish().await;
    let sent = std::fs::read_to_string(&log)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let _ = std::fs::remove_dir_all(&dir);
    (result, text, sent)
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
async fn tool_item_or_server_request_ends_the_turn() {
    for mode in ["tool", "request"] {
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
