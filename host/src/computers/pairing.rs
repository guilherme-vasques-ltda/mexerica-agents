//! Executor enrollment is separate from QR; codes never enter the upstream pairing store.
use crate::LockExt;
use crate::hub::Hub;
use crate::remote::crypto;
use crate::store::now_ms;
use anyhow::{Result, anyhow, ensure};
use axum::{
    Json,
    extract::{ConnectInfo, State},
    http::StatusCode,
};
use hmac::{Hmac, KeyInit, Mac};
use serde_json::{Value, json};
use sha2::Sha256;
use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

struct Code {
    value: String,
    expires: i64,
}
#[derive(Default)]
pub struct Codes {
    codes: Vec<Code>,
    attempts: Vec<(String, Instant)>,
}
impl Codes {
    fn prune(&mut self) {
        self.codes.retain(|c| c.expires > now_ms());
    }
    fn attempt(&mut self, source: &str) -> Result<()> {
        self.attempts.retain(|(_, at)| at.elapsed() < Duration::from_secs(60));
        ensure!(
            self.attempts.len() < 10 && self.attempts.iter().filter(|(s, _)| s == source).count() < 3,
            "rate_limited: tente novamente em um minuto"
        );
        self.attempts.push((source.into(), Instant::now()));
        Ok(())
    }
    fn issue(&mut self) -> Value {
        self.prune();
        if self.codes.len() >= 5 {
            self.codes.remove(0);
        }
        let alphabet = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
        let code: String = crypto::random::<16>().iter().map(|b| char::from(alphabet[usize::from(b & 31)])).collect();
        let expires = now_ms() + 600_000;
        self.codes.push(Code { value: code.clone(), expires });
        json!({"code":code.as_bytes().chunks(4).map(|p|String::from_utf8_lossy(p)).collect::<Vec<_>>().join("-"),"expiresAt":expires})
    }
}
fn normalize(code: &str) -> String {
    code.chars().filter(|c| *c != '-').map(|c| c.to_ascii_uppercase()).collect()
}
pub fn open(hub: &Hub) -> bool {
    let mut codes = hub.computers.pairing.locked();
    codes.prune();
    !codes.codes.is_empty()
}
pub fn issue(hub: &Hub) -> Value {
    let mut v = hub.computers.pairing.locked().issue();
    v["hub"] = json!({"cid":hub.identity.computer_id(),"signPub":crypto::b64(&hub.identity.sign_pub())});
    v
}
pub fn hello(hub: &Hub, source: &str, a: &str) -> Result<Value> {
    let mut codes = hub.computers.pairing.locked();
    codes.attempt(source)?;
    codes.prune();
    let a = normalize(a);
    ensure!(a.len() == 4, "invalid_code: prefixo inválido");
    let c =
        codes.codes.iter().find(|c| c.value.starts_with(&a)).ok_or_else(|| anyhow!("invalid_code: código expirado"))?;
    let sign = hub.identity.sign_pub();
    let cid = hub.identity.computer_id();
    let mut mac = Hmac::<Sha256>::new_from_slice(&c.value.as_bytes()[4..])
        .map_err(|_| anyhow!("invalid_code: código inválido"))?;
    mac.update(&sign);
    mac.update(cid.as_bytes());
    Ok(json!({"cid":cid,"signPub":crypto::b64(&sign),"mac":crypto::b64(&mac.finalize().into_bytes())}))
}
pub async fn enroll_hello(
    State(hub): State<Arc<Hub>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(b): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    hello(&hub, &peer.ip().to_string(), b["a"].as_str().unwrap_or_default()).map(Json).map_err(|e| {
        let status = if e.to_string().starts_with("rate_limited:") {
            StatusCode::TOO_MANY_REQUESTS
        } else {
            StatusCode::FORBIDDEN
        };
        (status, Json(json!({"error":e.to_string()})))
    })
}
pub fn redeem(hub: &Hub, key: &str, b: &Value, source: &str) -> Result<Value> {
    let _gate = hub.computers.gate.locked();
    let name = b["name"]
        .as_str()
        .map(str::trim)
        .filter(|n| !n.is_empty() && n.chars().count() <= 100)
        .ok_or_else(|| anyhow!("invalid_arguments: nome inválido"))?;
    let platform = b["platform"]
        .as_str()
        .filter(|p| matches!(*p, "macos" | "windows"))
        .ok_or_else(|| anyhow!("invalid_arguments: plataforma inválida"))?;
    let raw = crypto::unb64_n::<32>(key)?;
    ensure!(hub.store.device(key).is_none(), "already_enrolled: revogue a credencial antes de cadastrar novamente");
    let mut codes = hub.computers.pairing.locked();
    codes.attempt(source)?;
    codes.prune();
    let given = normalize(b["code"].as_str().unwrap_or_default());
    let index = codes
        .codes
        .iter()
        .position(|c| crypto::ct_eq(c.value.as_bytes(), given.as_bytes()))
        .ok_or_else(|| anyhow!("invalid_code: código expirado ou já utilizado"))?;
    let c = super::registry::Computer {
        id: crypto::computer_id(&raw),
        device_key: key.into(),
        label: name.into(),
        platform: platform.into(),
        os_version: b["osVersion"].as_str().map(str::to_owned),
        arch: b["arch"].as_str().map(str::to_owned),
        app_version: None,
        capabilities: json!({}),
        presence: json!({}),
        enrolled_at: now_ms(),
        last_seen_at: None,
        last_used_at: None,
        state_since: Some(now_ms()),
        removed_at: None,
        last_boot_id: None,
    };
    hub.store.enroll_computer(&c)?;
    codes.codes.remove(index);
    hub.auth_changed();
    Ok(json!({"computerId":c.id,"label":c.label}))
}
