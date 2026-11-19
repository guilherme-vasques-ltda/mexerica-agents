// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Compaction and rollover on a real host with disposable data: the host compacting a full conversation at the
//! end of a turn, its loop guard turning to a rollover, and the handoff card the user edits or turns down.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(dead_code)]
mod common;

use common::host::Host;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// Waits until the bot's chat has a notice with this field, and returns the newest one.
async fn notice(host: &Host, bot: &str, field: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let history = host.call("history", json!({"botId": bot})).await;
        if let Some(e) = history["entries"].as_array().unwrap().iter().rev().find(|e| e["data"][field].is_object()) {
            return e.clone();
        }
        assert!(Instant::now() < deadline, "no {field} notice: {history}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn a_full_conversation_is_compacted_with_its_keep_list() {
    let host = Host::start().await;
    let claude = host.fixture_bot("Claude-like", "instructions_agent.py", "claude").await;
    host.reply(&claude, "hello").await;
    host.call("saveMemory", json!({"botId": claude, "text": "Allergic to peanuts"})).await;
    host.reply(&claude, "USAGE 210000 SIZE 1000000").await;
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut line = notice(&host, &claude, "compaction").await;
    let entry_id = line["id"].as_str().unwrap().to_owned();
    // O notice nasce com summary Null; o produtor atualiza essa mesma entrada depois.
    while !line["data"]["compaction"]["summary"].is_string() {
        let history = host.call("history", json!({"botId": claude})).await;
        if let Some(updated) = history["entries"].as_array().unwrap().iter().find(|e| e["id"] == entry_id) {
            line = updated.clone();
        }
        assert!(
            line["data"]["compaction"]["summary"].is_string() || Instant::now() < deadline,
            "resumo da compactação não chegou: entryId={entry_id}, última entrada={line}, histórico={history}"
        );
        if !line["data"]["compaction"]["summary"].is_string() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    assert_eq!(line["data"]["compaction"]["byHost"], true);
    assert_eq!(line["data"]["compaction"]["summary"], "Summary 1: the user is planning a trip.");
    let prompts = host.prompts(&claude);
    let compact = prompts.iter().find(|p| p.starts_with("/compact")).unwrap();
    assert_eq!(
        compact,
        &format!("/compact {}", codync_mem::compaction::KEEP),
        "Claude keeps its native prompt plus KEEP"
    );
    // O manual nativo é estável. A memória atualizada já chegou pelo turno anterior à compactação.
    let updated = prompts.iter().find(|p| p.contains("USAGE 210000")).unwrap();
    assert!(updated.contains("Allergic to peanuts"), "{updated}");
    assert!(host.requests(&claude, "session/load").is_empty(), "não recarrega para trocar o manual estável");

    let codex = host.fixture_bot("Codex-like", "instructions_agent.py", "codex").await;
    host.reply(&codex, "USAGE 150000 SIZE 200000").await;
    notice(&host, &codex, "compaction").await;
    assert!(host.prompts(&codex).iter().any(|p| p == "/compact"), "Codex's keep-list is in its config");
}

#[tokio::test]
async fn compacting_too_often_turns_into_a_rollover_the_user_can_edit_or_turn_down() {
    let host = Host::start().await;
    let bot = host.fixture_bot("Busy", "instructions_agent.py", "codex").await;
    host.reply(&bot, "USAGE 210000 SIZE 1000000").await;
    notice(&host, &bot, "compaction").await;
    // Full again right after a compaction: compacting again wouldn't help.
    host.reply(&bot, "USAGE 210000 SIZE 1000000").await;
    let card = notice(&host, &bot, "handoff").await;
    assert_eq!(card["data"]["handoff"]["status"], "draft");
    let text = card["data"]["handoff"]["text"].as_str().unwrap();
    assert!(text.starts_with("<handoff>") && text.contains("USAGE 210000"), "{text}");

    host.call("editHandoff", json!({"entryId": card["id"], "text": "<handoff>We were booking a train.</handoff>"}))
        .await;
    host.reply(&bot, "Which seat did we pick?").await;
    let news = host.requests(&bot, "session/new");
    assert_eq!(news.len(), 2, "a new conversation");
    let last = host.prompts(&bot).last().unwrap().clone();
    assert!(last.contains("We were booking a train.") && last.contains("Which seat did we pick?"), "{last}");
    let card = host.call("history", json!({"botId": bot})).await["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == card["id"])
        .unwrap()
        .clone();
    assert_eq!(card["data"]["handoff"]["status"], "sent");
    assert!(host.refused("editHandoff", json!({"entryId": card["id"], "text": "late"})).await.contains("moved on"));

    // Turned down: the conversation carries on.
    host.reply(&bot, "USAGE 210000 SIZE 1000000").await;
    notice(&host, &bot, "compaction").await;
    host.reply(&bot, "USAGE 210000 SIZE 1000000").await;
    let card = host.call("history", json!({"botId": bot})).await["entries"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|e| e["data"]["handoff"]["status"] == "draft")
        .unwrap()
        .clone();
    host.call("keepConversation", json!({"entryId": card["id"]})).await;
    host.reply(&bot, "carry on").await;
    assert_eq!(host.requests(&bot, "session/new").len(), 2, "same conversation");
    assert!(!host.prompts(&bot).last().unwrap().contains("<handoff>"));
}

/// C2: a leitura é da sessão visível e a ação reutiliza o ACP real, o KEEP e o reenvio.
#[tokio::test]
async fn context_meter_manual_compaction_keeps_engine_numbers_and_memory() {
    let host = Host::start().await;
    let bot = host.fixture_bot("Contexto", "instructions_agent.py", "claude").await;
    host.reply(&bot, "USAGE 84000 SIZE 200000").await;
    let deadline = Instant::now() + Duration::from_secs(30);
    let before = loop {
        let report = host.call("donnaContext", json!({"botId":bot})).await;
        if report["context"]["busy"] == false {
            break report;
        }
        assert!(Instant::now() < deadline, "context is not ready: {report}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert!(before["hubId"].is_string());
    assert_eq!(before["sessionId"], bot);
    assert_eq!(before["epoch"], 0);
    assert_eq!(before["context"]["used"], 84_000);
    assert_eq!(before["context"]["size"], 200_000);
    assert_eq!(before["context"]["trigger"], 150_000);
    assert_eq!(before["context"]["canCompact"], true);
    // C3: dados descartáveis do fixture simulam duas falhas automáticas já gravadas.
    {
        let db = rusqlite::Connection::open(host.home.join("codync.db")).unwrap();
        assert_eq!(
            db.execute("UPDATE kv SET v='{\"consecutive\":2}' WHERE k LIKE 'donna.compaction.failures.%'", []).unwrap(),
            1
        );
    }
    let paused = host.call("donnaContext", json!({"botId":bot})).await;
    assert_eq!(paused["context"]["automaticPaused"], true);
    assert_eq!(paused["context"]["automaticFailures"], 2);
    host.call("saveMemory", json!({"botId":bot,"text":"Allergic to peanuts"})).await;
    let compact =
        host.call("donnaContextCompact", json!({"botId":bot,"contextId":before["context"]["contextId"]})).await;
    assert_eq!(compact["accepted"], true);
    let line = notice(&host, &bot, "compaction").await;
    assert_eq!(line["data"]["compaction"]["byHost"], true);
    let resume_deadline = Instant::now() + Duration::from_secs(30);
    let resumed = loop {
        let report = host.call("donnaContext", json!({"botId":bot})).await;
        if report["context"]["automaticPaused"] == false && report["context"]["busy"] == false {
            break report;
        }
        assert!(Instant::now() < resume_deadline, "manual compaction did not resume: {report}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(resumed["context"]["automaticPaused"], false, "Comprimir agora com êxito despausa");
    assert_eq!(resumed["context"]["automaticFailures"], 0);
    let prompts = host.prompts(&bot);
    let command = prompts.iter().find(|p| p.starts_with("/compact")).unwrap();
    assert_eq!(command, &format!("/compact {}", codync_mem::compaction::KEEP), "{command}");
    let requests = host.requests(&bot, "session/prompt");
    assert_eq!(requests[0]["params"]["sessionId"], requests[1]["params"]["sessionId"]);
    host.reply(&bot, "after compact").await;
    let last = host.prompts(&bot).last().unwrap().clone();
    assert!(last.contains("Allergic to peanuts"), "memory resent after manual compaction: {last}");
    let after = host.call("donnaContext", json!({"botId":bot})).await;
    assert_eq!(after["epoch"], 1);
    assert_eq!(after["context"]["lastCompaction"]["byHost"], true);
    assert!(after["context"]["lastCompaction"]["at"].as_i64().unwrap() > 0);
    assert!(
        host.refused("donnaContextCompact", json!({"botId":bot,"contextId":after["context"]["contextId"]}))
            .await
            .contains("context_repeat_guard")
    );
    // C3: RPC real do ajuste global e ponto efetivo na medição da mesma conversa.
    let defaults = host.call("donnaContextSettings", json!({})).await;
    assert_eq!(defaults, json!({"triggerTokens":null,"idle":"beforeExpiry"}));
    let chosen = json!({"triggerTokens":100_000,"idle":"never"});
    assert_eq!(host.call("donnaContextSettingsSave", chosen.clone()).await, chosen);
    assert_eq!(host.call("donnaContextSettings", json!({})).await, chosen);
    let configured = host.call("donnaContext", json!({"botId":bot})).await;
    assert_eq!(configured["context"]["trigger"], 100_000);
    host.reply(&bot, "USAGE 12345 SIZE 0").await;
    let missing = host.call("donnaContext", json!({"botId":bot})).await;
    assert!(missing.get("context").is_none(), "missing size is not a catalog window: {missing}");
}

/// Comprimir agora com o Codex: o codex-acp manda a medida resumida antes do `completed`. O medidor precisa mostrar a
/// medida nova na época nova, em vez de sumir até a próxima mensagem.
#[tokio::test]
async fn codex_manual_compaction_keeps_the_summarized_measure() {
    let host = Host::start().await;
    let bot = host.fixture_bot("Codex", "instructions_agent.py", "codex").await;
    host.reply(&bot, "USAGE 150000 SIZE 1000000").await;
    let before = host.call("donnaContext", json!({"botId":bot})).await;
    assert_eq!(before["epoch"], 0);
    assert_eq!(before["context"]["used"], 150_000);
    let compact =
        host.call("donnaContextCompact", json!({"botId":bot,"contextId":before["context"]["contextId"]})).await;
    assert_eq!(compact["accepted"], true);
    let deadline = Instant::now() + Duration::from_secs(30);
    let after = loop {
        let report = host.call("donnaContext", json!({"botId":bot})).await;
        if report["epoch"] == 1 {
            break report;
        }
        assert!(Instant::now() < deadline, "compaction did not finish: {report}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(after["context"]["used"], 20_000, "o medidor some depois de comprimir: {after}");
    assert_eq!(after["context"]["size"], 1_000_000);
    assert_eq!(after["context"]["lastCompaction"]["byHost"], true);
    assert_ne!(after["context"]["contextId"], before["context"]["contextId"]);
}

/// Espera a leitura do contexto chegar à época `epoch` (compactações concluídas nesta sessão).
async fn context_at_epoch(host: &Host, bot: &str, epoch: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let report = host.call("donnaContext", json!({"botId":bot})).await;
        if report["epoch"] == epoch && report["context"]["busy"] == false {
            return report;
        }
        assert!(Instant::now() < deadline, "context never reached epoch {epoch}: {report}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn compact_now(host: &Host, bot: &str) {
    let before = host.call("donnaContext", json!({"botId":bot})).await;
    let compact =
        host.call("donnaContextCompact", json!({"botId":bot,"contextId":before["context"]["contextId"]})).await;
    assert_eq!(compact["accepted"], true, "{before}");
}

/// Revisão MEXE-189: um desfecho atrasado da compactação anterior (o `completed` repetido de A) chega durante a
/// seguinte (B). A medida de B não pode se perder: a época 2 mostra a medida que B mediu.
#[tokio::test]
async fn codex_late_outcome_of_the_previous_compaction_keeps_the_new_measure() {
    let host = Host::start().await;
    let bot = host.fixture_bot("Codex", "instructions_agent.py", "codex").await;
    host.reply(&bot, "LATE_DUPLICATE USAGE 150000 SIZE 1000000").await;
    compact_now(&host, &bot).await;
    assert_eq!(context_at_epoch(&host, &bot, 1).await["context"]["used"], 20_000);
    host.reply(&bot, "COMPACT_NOW").await;
    let second = context_at_epoch(&host, &bot, 2).await;
    assert_eq!(second["context"]["used"], 18_000, "o anel some na segunda compactação: {second}");
}

/// Revisão MEXE-189: uma medida atrasada da época anterior (150 mil) chega depois da resumida (20 mil), antes do
/// `completed`. A época nova mostra a resumida, não a velha como se fosse nova.
#[tokio::test]
async fn codex_late_sample_of_the_previous_epoch_is_not_the_new_measure() {
    let host = Host::start().await;
    let bot = host.fixture_bot("Codex", "instructions_agent.py", "codex").await;
    host.reply(&bot, "LATE_STALE USAGE 150000 SIZE 1000000").await;
    compact_now(&host, &bot).await;
    let after = context_at_epoch(&host, &bot, 1).await;
    assert_eq!(after["context"]["used"], 20_000, "a medida velha passou por nova: {after}");
}

/// Compactação que falha depois de medir: a medida provisória não troca o anel; ele fica com a de antes.
#[tokio::test]
async fn codex_failed_compaction_keeps_the_measure_it_had() {
    let host = Host::start().await;
    let bot = host.fixture_bot("Codex", "instructions_agent.py", "codex").await;
    host.reply(&bot, "FAIL_COMPACT USAGE 150000 SIZE 1000000").await;
    compact_now(&host, &bot).await;
    let deadline = Instant::now() + Duration::from_secs(30);
    while !host.prompts(&bot).iter().any(|p| p == "/compact") {
        assert!(Instant::now() < deadline, "the compaction never reached the agent");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let after = context_at_epoch(&host, &bot, 0).await;
    assert_eq!(after["context"]["used"], 150_000, "falha trocou o anel pela medida provisória: {after}");
    assert_eq!(policy_used(&host, &bot), 150_000, "a medida provisória entrou na política de compactação");
}

/// O que a política de compactação automática guardou como contexto em uso da conversa do bot.
fn policy_used(host: &Host, bot: &str) -> u64 {
    let db = rusqlite::Connection::open(host.home.join("codync.db")).unwrap();
    let raw: String =
        db.query_row("SELECT v FROM kv WHERE k LIKE ?1", [format!("compaction.{bot}.%")], |row| row.get(0)).unwrap();
    serde_json::from_str::<Value>(&raw).unwrap()["used"].as_u64().unwrap()
}

/// Revisão MEXE-189 (rodada 2): a sequência capturada do codex-acp 2.7.0 real (3.010 repetido, 90.010 do pedido de
/// resumo, 7.147 resumido, `completed`). A época nova mostra 7.147, a resumida, e a política guarda a mesma.
#[tokio::test]
async fn codex_captured_2_7_0_sequence_shows_the_summary() {
    let host = Host::start().await;
    let bot = host.fixture_bot("Codex", "instructions_agent.py", "codex").await;
    host.reply(&bot, "CAPTURE_2_7_0 USAGE 3010 SIZE 258400").await;
    compact_now(&host, &bot).await;
    let after = context_at_epoch(&host, &bot, 1).await;
    assert_eq!(after["context"]["used"], 7_147, "o anel não mostra a medida resumida: {after}");
    assert_eq!(after["context"]["size"], 258_400);
    assert_eq!(policy_used(&host, &bot), 7_147);
}

/// Revisão MEXE-189 (rodada 2): com C em curso, o `completed` atrasado de A (depois de A e B concluídas) não fecha
/// uma época. C falha: a conversa fica na época 2, com a medida de B.
#[tokio::test]
async fn codex_late_outcome_of_an_older_compaction_does_not_close_the_running_one() {
    let host = Host::start().await;
    let bot = host.fixture_bot("Codex", "instructions_agent.py", "codex").await;
    host.reply(&bot, "LATE_OLDEST USAGE 150000 SIZE 1000000").await;
    compact_now(&host, &bot).await;
    assert_eq!(context_at_epoch(&host, &bot, 1).await["context"]["used"], 20_000);
    host.reply(&bot, "COMPACT_NOW").await;
    assert_eq!(context_at_epoch(&host, &bot, 2).await["context"]["used"], 18_000);
    host.reply(&bot, "COMPACT_NOW FAIL_COMPACT").await;
    let after = host.call("donnaContext", json!({"botId":bot})).await;
    assert_eq!(after["epoch"], 2, "o completed antigo fechou a época da compactação em curso: {after}");
    assert_eq!(after["context"]["used"], 18_000, "{after}");
}
