#![allow(clippy::unwrap_used)]

use super::{RUNNING, Running, drive, notice, stop, stopped_within, with_idle_account};
use crate::LockExt;
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
    let (result, text, sent, _) = turn_keeping(mode, thread).await;
    (result, text, sent)
}

async fn turn_keeping(
    mode: &str,
    thread: Option<&str>,
) -> (anyhow::Result<String>, String, Vec<Value>, Option<String>) {
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
        "Fixture instructions",
        "Oi",
        &mut cancel,
        |id| {
            kept = Some(id.to_owned());
            Ok(())
        },
        |delta| {
            text.push_str(delta);
            true
        },
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
        Running { hub: std::sync::Weak::new(), account: "fence-account".into(), cancel, done: done_rx },
    );
    let busy = with_idle_account("fence-account", || Ok(()));
    assert!(busy.unwrap_err().to_string().starts_with("account_busy"));
    assert!(with_idle_account("other-account", || Ok(())).is_ok());
    drop(done);
    assert!(with_idle_account("fence-account", || Ok(())).is_ok(), "a finished reply frees the account");
    RUNNING.locked().as_mut().unwrap().remove("fence-bot");
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
        "Fixture instructions",
        "Oi",
        &mut cancel,
        |_| Ok(()),
        |_| {
            pieces += 1;
            pieces < 2
        },
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
async fn text_is_not_delivered_once_the_check_before_it_refuses() {
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
        "Fixture instructions",
        "texto que não pode chegar",
        &mut cancel,
        |_| Err(anyhow::anyhow!("open_chat_cancelled")),
        |_| true,
    )
    .await;
    assert!(rpc.terminate().await);
    let sent: Vec<Value> =
        std::fs::read_to_string(&log).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(result.unwrap_err().to_string(), "open_chat_cancelled");
    assert!(find(&sent, "thread/start").is_some(), "a thread já existia quando a conferência recusou");
    assert!(find(&sent, "turn/start").is_none(), "o texto nunca chega ao Codex");
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

fn register(bot: &str) -> (watch::Receiver<bool>, watch::Sender<()>) {
    let (cancel, cancel_rx) = watch::channel(false);
    let (done, done_rx) = watch::channel(());
    RUNNING.locked().get_or_insert_with(Default::default).insert(
        bot.into(),
        Running { hub: std::sync::Weak::new(), account: "delete-account".into(), cancel, done: done_rx },
    );
    (cancel_rx, done)
}

#[tokio::test]
async fn deleting_waits_for_a_slow_codex_to_exit_and_then_frees_the_reservation() {
    let bot = "delete-slow-bot";
    let (mut cancel, done) = register(bot);
    let exit = tokio::spawn(async move {
        cancel.wait_for(|stop| *stop).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        RUNNING.locked().as_mut().unwrap().remove(bot);
        drop(done);
    });
    stop(bot);
    let started = std::time::Instant::now();
    assert!(stopped_within(bot, std::time::Duration::from_secs(5)).await);
    assert!(started.elapsed() >= std::time::Duration::from_millis(350), "a exclusão esperou o Codex sair");
    exit.await.unwrap();
    assert!(with_idle_account("delete-account", || Ok(())).is_ok(), "com o Codex fora, a conta fica livre");
}

#[tokio::test]
async fn deleting_reports_a_codex_that_does_not_exit_in_time_and_keeps_the_account_reserved() {
    let bot = "delete-stuck-bot";
    let (_cancel, done) = register(bot);
    stop(bot);
    assert!(!stopped_within(bot, std::time::Duration::from_millis(100)).await);
    let busy = with_idle_account("delete-account", || Ok(()));
    assert!(busy.unwrap_err().to_string().starts_with("account_busy"), "a reserva continua enquanto o processo vive");
    RUNNING.locked().as_mut().unwrap().remove(bot);
    drop(done);
    assert!(stopped_within(bot, std::time::Duration::from_millis(100)).await);
}
