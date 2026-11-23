//! C8: fim do turno real → ledger → RPC privado, sem preço inventado.
#![allow(clippy::unwrap_used)]
#[allow(dead_code)]
mod common;
use common::host::Host;
use serde_json::json;

#[tokio::test]
async fn turn_usage_is_saved_with_motor_delta_and_unpriced_turn_tokens() {
    let host = Host::start().await;
    let bot = host.fixture_bot("Custo", "instructions_agent.py", "claude").await;
    host.reply(&bot, "COST 0.4").await;
    let first = host.call("donnaCostReport", json!({"botId":bot})).await;
    assert_eq!(first["conversation"]["sessionId"], bot);
    assert_eq!(first["conversation"]["totals"]["turns"], 1);
    assert_eq!(first["conversation"]["totals"]["costs"][0]["amount"], 0.4);
    assert_eq!(first["conversation"]["totals"]["costs"][0]["source"], "motor");
    host.reply(&bot, "COST 0.7").await;
    host.reply(&bot, "C8_TOKENS").await;
    let result = host.call("donnaCostReport", json!({"botId":bot})).await;
    let total = &result["conversation"]["totals"];
    assert_eq!(total["turns"], 3);
    assert_eq!(total["unpricedTurns"], 1);
    assert_eq!(total["inputTokens"], 30);
    assert_eq!(total["cachedReadTokens"], 60);
    assert_eq!(total["cachedWriteTokens"], 90);
    assert_eq!(total["outputTokens"], 120);
    assert!((total["costs"][0]["amount"].as_f64().unwrap() - 0.7).abs() < 1e-10);
    let accounts = result["accounts"].as_array().unwrap();
    let models = result["models"].as_array().unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0]["totals"]["turns"], 3);
    assert_eq!(models.len(), 1);
    assert_eq!(models[0]["totals"]["turns"], 3);
    let no_price = host.fixture_bot("Só tokens", "instructions_agent.py", "codex").await;
    host.reply(&no_price, "C8_TOKENS").await;
    let only = host.call("donnaCostReport", json!({"botId":no_price})).await;
    assert_eq!(only["conversation"]["totals"]["costs"], json!([]));
    assert_eq!(only["conversation"]["totals"]["inputTokens"], 10);
}

/// MX-145 R3-P2-4: o prompt de fundo SAIU (o extrator da memória já o recebeu e
/// está parado nele) e o trabalho é cortado antes da resposta — aqui pelo jeito
/// mais duro, o Hub caindo no meio do await. A linha do envio já está no ledger
/// (tokens NULL = não conferido): cancelamento nunca apaga o que foi enviado.
#[cfg(unix)]
#[tokio::test]
#[ignore = "A prova positiva requer um executor de fundo com credencial de API."]
async fn background_prompt_cut_mid_await_stays_in_ledger() {
    use std::time::{Duration, Instant};
    let mut host = Host::start_with_background("marker").await;
    let marker = host.home.join("dream-boundary");
    let bot = host.fixture_bot_as("Fundo cortado", "memory_agent.py", marker.to_str().unwrap(), "claude").await;
    host.reply(&bot, "Please always explain things to me concisely in Traditional Chinese.").await;
    let deadline = Instant::now() + Duration::from_secs(40);
    while !marker.with_extension("ready").exists() {
        assert!(Instant::now() < deadline, "o prompt de fundo não saiu");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let mut rows = Vec::new();
    host.restart(|home| {
        let db = rusqlite::Connection::open(home.join("codync.db")).unwrap();
        let mut stmt = db
            .prepare(
                "SELECT bot_id, input_tokens IS NULL AND output_tokens IS NULL FROM donna_turn_usage
                 WHERE source='fundo' AND session_id='background:memory'",
            )
            .unwrap();
        rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
    })
    .await;
    assert_eq!(rows, [(bot.clone(), true)], "uma linha do envio, sem tokens, mesmo sem resposta");
}
