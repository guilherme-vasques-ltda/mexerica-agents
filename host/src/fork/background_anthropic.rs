use anyhow::{Result, anyhow, ensure};
use reqwest::header::HeaderValue;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;
use zeroize::Zeroizing;

const BASE: &str = "https://api.anthropic.com";
const MESSAGES: &str = "https://api.anthropic.com/v1/messages";
const MAX_TOKENS: u64 = 4096;
const MAX_RESPONSE: usize = 256_000;

use crate::{
    fork::engine_accounts::{Account, Backend, Billing, LaunchReference},
    store::Store,
};

pub(super) fn slot(account: &Account) -> Option<&str> {
    let LaunchReference::VaultProfile { slot } = &account.launch else { return None };
    (account.backend == Backend::Claude
        && account.billing == Billing::PaidApi
        && account.subscription_proof.is_none()
        && account.managed.as_ref().is_some_and(|meta| meta.enabled && !meta.paused)
        && *slot == format!("agent-account:{}", account.id))
    .then_some(slot)
}

pub(super) fn credential(store: &Store, slot: &str) -> Result<(String, HeaderValue)> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Credential {
        #[serde(rename = "ANTHROPIC_API_KEY")]
        key: String,
    }
    let sealed = store.kv_read(slot)?.ok_or_else(|| anyhow!("Chave Anthropic indisponível."))?;
    let plaintext = Zeroizing::new(crate::market::vault::open_ciphertext(store, slot, &sealed)?);
    let parsed: Credential = serde_json::from_str(&plaintext).map_err(|_| anyhow!("Credencial Anthropic inválida."))?;
    Ok((sealed, api_key(&Zeroizing::new(parsed.key))?))
}

pub(crate) fn api_key(key: &str) -> Result<HeaderValue> {
    ensure!(
        key.strip_prefix("sk-ant-api").is_some_and(|rest| (12..=4096).contains(&rest.len()))
            && key.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')),
        "A automação exige uma chave de API da Anthropic, sem login pessoal."
    );
    let mut header = HeaderValue::from_str(key).map_err(|_| anyhow!("Chave Anthropic inválida."))?;
    header.set_sensitive(true);
    Ok(header)
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(90))
        .build()?)
}

async fn response(mut response: reqwest::Response) -> Result<Vec<u8>> {
    ensure!(response.status().is_success(), "A API Anthropic recusou o pedido ({})", response.status().as_u16());
    let mut body = Vec::new();
    while let Some(chunk) =
        response.chunk().await.map_err(|_| anyhow!("A API Anthropic não respondeu completamente."))?
    {
        ensure!(body.len().saturating_add(chunk.len()) <= MAX_RESPONSE, super::background_notice::Problem::OutputLimit);
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

pub(crate) async fn models(key: HeaderValue) -> Result<Vec<super::engine_accounts::Model>> {
    let res = client()?
        .get(format!("{BASE}/v1/models?limit=100"))
        .header("x-api-key", key.clone())
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
        .map_err(|_| anyhow!("Não foi possível verificar os modelos da Anthropic."))?;
    let body = response(res).await?;
    let parsed: serde_json::Value =
        serde_json::from_slice(&body).map_err(|_| anyhow!("Catálogo Anthropic inválido."))?;
    let raw = parsed["data"].as_array().ok_or_else(|| anyhow!("Catálogo Anthropic inválido."))?;
    let mut models = raw
        .iter()
        .filter_map(|entry| {
            let id = entry["id"].as_str()?;
            (id.starts_with("claude-")
                && id.len() <= 128
                && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
                && !key.to_str().is_ok_and(|secret| id.contains(secret)))
            .then(|| super::engine_accounts::Model { id: id.to_owned(), options: Vec::new() })
        })
        .collect::<Vec<_>>();
    models.sort_unstable_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    ensure!(!models.is_empty(), "Nenhum modelo Anthropic foi confirmado.");
    Ok(models)
}

pub(super) fn request(key: HeaderValue, model: &str, system: &str, user: &str) -> Result<reqwest::RequestBuilder> {
    ensure!(system.len().saturating_add(user.len()) <= 128_000, "A tarefa de fundo é grande demais.");
    let (system, user) = if user.is_empty() { ("", system) } else { (system, user) };
    #[cfg(test)]
    let (builder, url) = test_transport::ENDPOINT
        .try_with(|(client, url)| Ok((client.clone(), url.clone())))
        .unwrap_or_else(|_| client().map(|client| (client, MESSAGES.to_owned())))?;
    #[cfg(not(test))]
    let (builder, url) = (client()?, MESSAGES);
    Ok(builder
        .post(url)
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .json(&json!({"model":model,"max_tokens":MAX_TOKENS,"system":system,
            "messages":[{"role":"user","content":user}]})))
}

#[cfg(test)]
pub(super) mod test_transport {
    tokio::task_local! {
        pub(in crate::fork) static ENDPOINT: (reqwest::Client, String);
    }
}

pub(super) async fn complete(
    response_result: reqwest::Result<reqwest::Response>,
    key: &HeaderValue,
    expected: &str,
    catalog: &[String],
    models: &mut Vec<String>,
    observed: &mut Option<super::background_history::Usage>,
) -> Result<String> {
    let res = response_result.map_err(|_| anyhow!("Não foi possível enviar a tarefa para a API Anthropic."))?;
    let parsed: serde_json::Value =
        serde_json::from_slice(&response(res).await?).map_err(|_| anyhow!("Resposta Anthropic inválida."))?;
    ensure!(parsed["type"] == "message" && parsed["role"] == "assistant", "Resposta Anthropic inválida.");
    *observed = usage(&parsed["usage"]).ok();
    let reported = parsed["model"].as_str().ok_or_else(|| anyhow!("Modelo Anthropic não confirmado."))?;
    ensure!(matches_model(expected, reported, catalog), "Modelo Anthropic não confirmado.");
    models.push(reported.to_owned());
    ensure!(observed.is_some(), "Uso Anthropic não confirmado.");
    parse_text(&parsed, key)
}

fn matches_model(expected: &str, reported: &str, catalog: &[String]) -> bool {
    expected == reported
        || (catalog.iter().any(|id| id == reported)
            && reported
                .strip_prefix(expected)
                .and_then(|suffix| suffix.strip_prefix('-'))
                .is_some_and(|date| date.len() == 8 && date.bytes().all(|b| b.is_ascii_digit())))
}

fn parse_text(parsed: &serde_json::Value, key: &HeaderValue) -> Result<String> {
    ensure!(parsed["stop_reason"] == "end_turn", "A tarefa Anthropic não terminou normalmente.");
    let content = parsed["content"].as_array().ok_or_else(|| anyhow!("Resposta Anthropic sem conteúdo."))?;
    let mut text = String::new();
    for block in content {
        ensure!(block["type"] == "text", "A tarefa Anthropic devolveu conteúdo que não é texto.");
        text.push_str(block["text"].as_str().ok_or_else(|| anyhow!("Resposta Anthropic sem texto."))?);
    }
    ensure!(text.len() <= super::background_executor::MAX_OUTPUT_BYTES, super::background_notice::Problem::OutputLimit);
    ensure!(key.to_str().is_ok_and(|secret| !text.contains(secret)), "A resposta Anthropic repetiu a credencial.");
    Ok(text)
}

fn usage(usage: &serde_json::Value) -> Result<super::background_history::Usage> {
    let count =
        |field: &str| -> Result<u64> { usage[field].as_u64().ok_or_else(|| anyhow!("Uso Anthropic não confirmado.")) };
    let cache = |field: &str| -> Result<u64> { if usage.get(field).is_none() { Ok(0) } else { count(field) } };
    let read = cache("cache_read_input_tokens")?;
    let write = cache("cache_creation_input_tokens")?;
    Ok(super::background_history::Usage {
        input: count("input_tokens")?,
        output: count("output_tokens")?,
        cached_input: Some(read),
        cached_write: Some(write),
        cache_invalid: false,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::{api_key, matches_model, parse_text, usage};
    use serde_json::{Value, json};

    const FIXTURE: &str = concat!("sk-ant-api", "-fixture-not-a-real-key");

    fn message() -> Value {
        json!({"type":"message", "role":"assistant", "stop_reason":"end_turn", "model":"claude-fixture",
            "content":[{"type":"text","text":"Resposta de teste"}],
            "usage":{"input_tokens":2,"output_tokens":3,"cache_read_input_tokens":4,"cache_creation_input_tokens":5}})
    }

    #[test]
    fn contabiliza_uso_sem_aceitar_ferramentas_conclusao_ausente_ou_credencial_no_texto() {
        let key = api_key(FIXTURE).unwrap();
        let valid = message();
        let text = parse_text(&valid, &key).unwrap();
        let usage = usage(&valid["usage"]).unwrap();
        assert_eq!(text, "Resposta de teste");
        assert_eq!((usage.input, usage.output, usage.cached_input, usage.cached_write), (2, 3, Some(4), Some(5)));
        for (field, invalid) in [
            ("stop_reason", Value::Null),
            ("content", json!([{"type":"tool_use"}])),
            ("content", json!([{"type":"text", "text":FIXTURE}])),
        ] {
            let mut candidate = valid.clone();
            candidate[field] = invalid;
            assert!(parse_text(&candidate, &key).is_err(), "{field}");
        }
        assert!(api_key("sk-ant-oat-fixture").is_err());
        assert!(api_key("LLM_0_fixture").is_err());
    }

    #[test]
    fn alias_so_aceita_revisao_datada_do_catalogo_da_conta() {
        let catalog = ["claude-fixture-20251001".into(), "claude-other-20251001".into()];
        assert!(matches_model("claude-fixture", "claude-fixture-20251001", &catalog));
        assert!(!matches_model("claude-fixture", "claude-other-20251001", &catalog));
        assert!(!matches_model("claude-fixture", "claude-fixture-20251002", &catalog));
    }
}
