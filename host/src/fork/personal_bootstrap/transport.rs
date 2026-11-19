//! POSTs pessoais de H: tabela fechada, sem redirects/cache ou signer de parede.
use super::protocol;
use crate::{hub::Hub, remote::crypto};
use anyhow::{Context, Result, ensure};
use futures::StreamExt;
use serde_json::Value;
use std::time::Duration;
pub(super) struct Reply {
    pub value: Value,
    pub original: zeroize::Zeroizing<Vec<u8>>,
}
#[cfg(test)]
pub(super) type TestTransport = std::sync::Arc<
    dyn Fn(String, Vec<u8>, String) -> std::pin::Pin<Box<dyn Future<Output = Result<Vec<u8>>> + Send>> + Send + Sync,
>;
pub(super) async fn post(hub: &Hub, route: &str, body: Value) -> Result<Value> {
    Ok(post_bytes(hub, route, &serde_json::to_vec(&body)?).await?.value)
}
pub(super) async fn post_bytes(hub: &Hub, route: &str, bytes: &[u8]) -> Result<Reply> {
    let body = protocol::packet(bytes)?;
    let fields: &[&str] = match route {
        "time" => &["timeRequest"],
        "device" => &["begin", "ticket"],
        "candidate" => &["ticket", "token"],
        "receipt" => &["grant", "challenge", "proof", "receipt"],
        "host-state" => &["stateRequest"],
        "activation" => &["activation"],
        "fence-ack" => &["fenceAck"],
        "revocation" => &["revocation"],
        "local-consent" => &["localConsent"],
        "box-proof" => &["boxProof"],
        "local-confirm" => &["localConfirmation"],
        "consume" => &[
            "intent",
            "ticket",
            "appConsent",
            "localConsent",
            "boxChallenge",
            "boxProof",
            "approval",
            "localConfirmation",
            "consume",
        ],
        "read" => {
            // HOST-R1-01: `witness` só como booleano true (read de testemunho antes do primeiro K); o resto continua exato.
            let witness = body.get("witness").map(|w| *w == Value::Bool(true));
            ensure!(witness != Some(false), "invalid personal witness flag");
            match (body.get("deviceCode").is_some(), witness.is_some()) {
                (false, false) => &["read"],
                (true, false) => &["read", "deviceCode"],
                (false, true) => &["read", "witness"],
                (true, true) => &["read", "deviceCode", "witness"],
            }
        }
        _ => anyhow::bail!("personal route not admitted"),
    };
    let map = body.as_object().context("invalid personal body")?;
    ensure!(map.len() == fields.len() && fields.iter().all(|k| map.contains_key(*k)), "personal body fields mismatch");
    if route == "activation" {
        super::sync::guard(hub, None, false, |r| {
            ensure!(
                protocol::canonical(&r.activation)? == body["activation"],
                "activation original changed before send"
            );
            Ok(())
        })?;
    }
    let (origin, epoch) = hub.personal.with(|s| {
        s.config.matches(&hub.identity)?;
        let epoch = if route == "time" { 1 } else { s.clock.issued()? };
        Ok((s.config.context()[3].as_str().context("invalid personal origin")?.to_owned(), epoch))
    })?;
    let url = reqwest::Url::parse(&format!("{origin}/v1/personal-bootstrap/{route}"))?;
    ensure!(url.scheme() == "https" && url.query().is_none() && url.fragment().is_none(), "invalid personal URL");
    let nonce = crypto::b64(&crypto::random::<16>());
    let authority = url.host_str().context("personal origin missing")?;
    let authority = match url.port() {
        Some(p) => format!("{authority}:{p}"),
        None => authority.to_owned(),
    };
    let input = crypto::request_sig_input("POST", &authority, url.path(), i64::try_from(epoch)?, &nonce, bytes);
    let header = crypto::request_sig_header(&hub.identity.sign, i64::try_from(epoch)?, &nonce, &input);
    #[cfg(test)]
    let test_transport = {
        use crate::LockExt;
        hub.personal.test_transport.locked().clone()
    };
    #[cfg(test)]
    if let Some(test) = test_transport {
        let raw = test(route.into(), bytes.to_vec(), header).await?;
        hub.personal.with(|s| {
            s.config.matches(&hub.identity)?;
            if route != "time" {
                s.clock.validity()?;
            }
            Ok(())
        })?;
        return Ok(Reply { value: protocol::packet(&raw)?, original: zeroize::Zeroizing::new(raw) });
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()?;
    let response = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("Cache-Control", "no-store")
        .header("Codync-Sig", header)
        .body(bytes.to_vec())
        .send()
        .await?;
    ensure!(response.status().is_success(), "personal transport refused");
    let mut stream = response.bytes_stream();
    let mut raw = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        ensure!(raw.len() + chunk.len() <= 192 * 1024, "personal response too large");
        raw.extend_from_slice(&chunk);
    }
    hub.personal.with(|s| {
        s.config.matches(&hub.identity)?;
        if route != "time" {
            s.clock.validity()?;
        }
        Ok(())
    })?;
    Ok(Reply { value: protocol::packet(&raw)?, original: zeroize::Zeroizing::new(raw) })
}
pub(super) async fn time(hub: &Hub) -> Result<Value> {
    let raw = hub.personal.time_request(&hub.identity)?;
    let result = post(hub, "time", serde_json::json!({"timeRequest":protocol::canonical(&raw)?})).await;
    let checked = (|| {
        let body = result?;
        ensure!(body.as_object().is_some_and(|m| m.len() == 1 && m.contains_key("time")), "invalid time result");
        let raw = serde_json::to_vec(&body["time"])?;
        hub.personal.accept_time(&raw)?;
        Ok(body["time"].clone())
    })();
    if checked.is_err() {
        hub.personal.with(|s| {
            s.clock.abandon_time();
            Ok(())
        })?;
    }
    checked
}

pub(super) fn signed(hub: &Hub, payload: Value) -> Result<Value> {
    let bytes = serde_json::to_vec(&payload)?;
    let signature = crypto::b64(&hub.identity.sign(&bytes));
    let out = Value::Array(vec![payload, Value::String(signature)]);
    protocol::canonical(&serde_json::to_vec(&out)?)?;
    Ok(out)
}
pub(super) fn nonce() -> String {
    crypto::b64(&crypto::random::<32>())
}
