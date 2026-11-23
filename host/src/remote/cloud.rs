// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! The Codync cloud (Cloudflare relay + accounts, spec §4.2, §4.3, §8.4, §9.6): which one
//! this host uses, its `/v1/host/*` API (signed with `Codync-Sig`), the state pull that may
//! only renew or remove account devices, never add one, and account access requests with
//! the commit-then-reveal SAS. The relay socket itself lives in `relay`.

use crate::LockExt;
use crate::api::devices::{self, Caller};
use crate::fork::enrollment::approval::{self, DecideResult, Expected, Tombstones};
use crate::fork::enrollment::{Fence, FenceAck, acknowledged, confirmable};
use crate::hub::Hub;
use crate::remote::crypto::{self, b64};
use crate::store::bootstrap::{CloudGrant, Publication};
#[cfg(not(test))]
use crate::store::now_ms;
use crate::store::{Device, DeviceSource, Scope, Store};
use anyhow::{Context, Result, anyhow, bail};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
#[cfg(test)]
use test_clock::now_ms;
use tokio::sync::watch;

#[cfg(test)]
#[path = "cloud_test_clock.rs"]
pub(super) mod test_clock;

/// Só a origem que o operador configurou no serviço (`MEXERICA_CLOUD_URL`, ou a antiga `DONNABOT_CLOUD_URL`, validada no módulo do
/// fork); o padrão por ambiente continua sem nuvem. Contrato `donna-acesso/v1` §4.1.
fn build_cloud_url() -> Option<String> {
    crate::fork::enrollment::cloud_url()
        .or_else(|| crate::environment::Environment::current().cloud_url().map(str::to_owned))
}

/// How long one successful state pull keeps an account device allowed.
pub const LEASE_MS: i64 = 15 * 60 * 1000;
const TIMEOUT: Duration = Duration::from_secs(15);
/// Grant revocations the cloud hasn't confirmed yet (kv, JSON array of grant ids).
const PENDING_REVOKES: &str = "grant_revokes";
/// kv: the saved `Approval`.
const APPROVAL: &str = "account_approval";

pub fn approval(store: &Store) -> Approval {
    if store.kv_get(APPROVAL).as_deref() == Some("auto") {
        tracing::warn!("Configuração antiga de aprovação automática ignorada");
    }
    Approval::Code
}

/// When this host sent its recent SAS nonces (kv, JSON array of ms).
const SAS_NONCES: &str = "sas_nonces";
/// Every host nonce is one guess at the phone's code for whoever controls the cloud, revealed
/// or not: at most this many per hour keeps a 6-digit SAS out of reach of a search.
const SAS_NONCES_PER_HOUR: usize = 5;
const HOUR_MS: i64 = 60 * 60 * 1000;
const REQUEST_MAX_MS: i64 = 10 * 60 * 1000;
const BUDGET_ERROR: &str = "Não foi possível registrar o limite de pedidos; novos pedidos aguardam.";

/// Sem nuvem/relay por padrão, independentemente do ambiente ou preferências antigas. Desligada
/// aqui (`setCloud`, só Local) vale sempre; a preferência nunca escolhe o endereço. Somente testes
/// unitários podem injetar uma nuvem falsa pela store isolada.
pub fn url(store: &Store) -> Option<String> {
    if store.kv_get("cloud_enabled").as_deref() == Some("false") {
        return None;
    }
    if cfg!(test) {
        return store.kv_get("test_cloud_url");
    }
    build_cloud_url()
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Owner {
    pub user_id: String,
    #[serde(default)]
    pub email: Option<String>,
}

/// `cloudStatus` and the `cloud` event.
#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CloudStatus {
    pub enabled: bool,
    pub url: Option<String>,
    pub registered: bool,
    pub connected: bool,
    pub owner: Option<Owner>,
    pub last_error: Option<String>,
    pub approval: Approval,
}

/// How account devices get in (`setApproval`, local only).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Approval {
    /// The user compares the 6-digit code and approves each device here.
    #[default]
    Code,
    /// Approved as soon as the SAS exchange completes, with no one comparing the code.
    Auto,
}

/// A pending account access request, with this host's own nonce (memory only).
#[derive(Clone)]
struct Request {
    id: String,
    device_key: [u8; 32],
    device_name: String,
    platform: String,
    email: Option<String>,
    commit: [u8; 32],
    host_nonce: [u8; 32],
    /// The SAS, once the device revealed a nonce that matches its commit.
    code: Option<String>,
    /// The confirmation code (MX-149), computed with the SAS; never listed, only returned once approved and saved.
    confirm_code: Option<String>,
    transcript: Option<String>,
    incarnation: [u8; 16],
    consuming: bool,
    created_at: i64,
    expires_at: i64,
}

impl Request {
    fn json(&self) -> Value {
        json!({
            "requestId": self.id,
            "deviceKey": b64(&self.device_key),
            "deviceName": self.device_name,
            "platform": self.platform,
            "email": self.email,
            "code": self.code,
            "codeReady": self.code.is_some(),
            "codeCheck": "server",
            "transcript": self.transcript,
            "createdAt": self.created_at,
            "expiresAt": self.expires_at,
            "deadline": self.expires_at,
        })
    }
}

pub struct Cloud {
    status: Mutex<CloudStatus>,
    /// Época RAM do dono, alterada sob status; detecta saída e retorno do mesmo dono.
    owner_generation: std::sync::atomic::AtomicU64,
    requests: Mutex<Vec<Request>>,
    tombstones: Mutex<Tombstones>,
    /// One state pull at a time.
    pulling: tokio::sync::Mutex<()>,
    /// Bumped when the cloud is turned on or off or pointed elsewhere: the relay restarts.
    pub config: watch::Sender<u64>,
}

impl Default for Cloud {
    fn default() -> Self {
        Self {
            status: Mutex::default(),
            owner_generation: std::sync::atomic::AtomicU64::new(0),
            requests: Mutex::default(),
            tombstones: Mutex::default(),
            pulling: tokio::sync::Mutex::new(()),
            config: watch::Sender::new(0),
        }
    }
}

impl Cloud {
    /// SAAS-04 lock order: owner status, then Store DB; callback must not await
    /// or call back into Cloud. Ownership changes cannot interleave a guarded write.
    pub(crate) fn with_engine_owner<T>(&self, f: impl FnOnce(Option<&str>) -> Result<T>) -> Result<T> {
        let status = self.status.locked();
        f(status.owner.as_ref().map(|owner| owner.user_id.as_str()))
    }

    /// PL1: callback síncrono na ordem Cloud → runtime → Connection.
    pub(crate) fn with_personal_status<T>(&self, f: impl FnOnce(&mut CloudStatus) -> Result<T>) -> Result<T> {
        f(&mut self.status.locked())
    }

    /// Mesmo lock do SAAS04; a época não é prova humana nem credencial externa.
    pub(crate) fn with_transport_owner<T>(&self, f: impl FnOnce(Option<&str>, u64) -> Result<T>) -> Result<T> {
        self.with_engine_owner(|owner| {
            let generation = self.owner_generation.load(std::sync::atomic::Ordering::Relaxed);
            anyhow::ensure!(generation < u64::MAX, "not_allowed: época do dono esgotada");
            f(owner, generation)
        })
    }

    pub fn status(&self) -> CloudStatus {
        self.status.locked().clone()
    }

    pub fn requests_json(&self) -> Vec<Value> {
        self.requests.locked().iter().filter(|r| !r.consuming && now_ms() <= r.expires_at).map(Request::json).collect()
    }

    /// A lista para quem não é este computador: sem o código que o aparelho mostra. Quem aprova digita o
    /// código lido no aparelho e o Hub compara em `decide_as`.
    pub fn requests_json_sem_codigo(&self) -> Vec<Value> {
        let mut list = self.requests_json();
        for r in &mut list {
            if let Some(o) = r.as_object_mut() {
                o.remove("code");
            }
        }
        list
    }
}

/// Changes the status and tells this computer's apps when it actually changed.
pub fn update_status(hub: &Hub, f: impl FnOnce(&mut CloudStatus)) {
    let changed = {
        let mut s = hub.cloud.status.locked();
        let before = s.clone();
        f(&mut s);
        if s.owner.as_ref().map(|o| &o.user_id) != before.owner.as_ref().map(|o| &o.user_id) {
            crate::fork::autoridade::epoca_dono::avancar(&hub.cloud.owner_generation);
        }
        (*s != before).then(|| s.clone())
    };
    crate::fork::workspace::observar_dono(hub);
    if let Some(s) = changed {
        let _ = hub.events.send(json!({"type": "cloud", "cloud": s}));
    }
}

fn emit_requests(hub: &Hub) {
    let _ = hub.events.send(json!({"type": "accessRequests", "requests": hub.cloud.requests_json()}));
}

/// An error answer from the cloud (`{"error":{"code","message"}}`).
#[derive(Debug)]
pub struct CloudError {
    pub status: u16,
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for CloudError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the Mexerica cloud refused ({} {}): {}", self.status, self.code, self.message)
    }
}

impl std::error::Error for CloudError {}

fn cloud_status_of(e: &anyhow::Error) -> Option<u16> {
    e.downcast_ref::<CloudError>().map(|c| c.status)
}

/// Something that has to wait for the other side first (409).
#[derive(Debug)]
pub struct Conflict(pub &'static str);

impl std::fmt::Display for Conflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for Conflict {}

/// The lowercase `Host` header value for `base` (with a non-default port), as `Codync-Sig` signs it.
pub fn authority(base: &str) -> Result<String> {
    let u = reqwest::Url::parse(base).context("invalid cloud URL")?;
    let host = u.host_str().ok_or_else(|| anyhow!("cloud URL has no host"))?;
    Ok(match u.port() {
        Some(p) => format!("{host}:{p}"),
        None => host.to_owned(),
    }
    .to_ascii_lowercase())
}

fn base(hub: &Hub) -> Result<String> {
    url(&hub.store).ok_or_else(|| anyhow!("The Mexerica cloud is off on this computer."))
}

/// One signed `/v1/host/*` call.
async fn call(hub: &Hub, base: &str, method: Method, path: &str, body: Option<&Value>) -> Result<Value> {
    let bytes = body.map(serde_json::to_vec).transpose()?.unwrap_or_default();
    let sig = hub.identity.sign_request(method.as_str(), &authority(base)?, path, &bytes);
    let mut req =
        crate::http_cloud().request(method, format!("{base}{path}")).header("Codync-Sig", sig).timeout(TIMEOUT);
    if body.is_some() {
        req = req.header("content-type", "application/json").body(bytes);
    }
    let res = req.send().await.context("the Mexerica cloud is unreachable")?;
    let status = res.status();
    if status.is_redirection() {
        // A assinatura foi feita para a origem validada; um 3xx levaria a outro endereço. O erro não
        // repete a URL nem o `Location`, e a credencial nunca sai do pedido original.
        bail!("the Mexerica cloud answered with a redirect ({status}); call refused");
    }
    let v: Value = res.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        let e = &v["error"];
        return Err(CloudError {
            status: status.as_u16(),
            code: e["code"].as_str().unwrap_or("internal").to_owned(),
            message: e["message"].as_str().unwrap_or_default().to_owned(),
        }
        .into());
    }
    Ok(v)
}

/// `POST /v1/host/register`: makes this computer known to the cloud (no account needed).
pub async fn register(hub: &Hub, base: &str) -> Result<()> {
    let body = json!({
        "boxKey": hub.identity.box_pub_b64(),
        "name": crate::service::host_name(),
        "platform": std::env::consts::OS,
        "device": tokio::task::spawn_blocking(crate::service::device).await?,
        "version": env!("CARGO_PKG_VERSION"),
    });
    let v = call(hub, base, Method::POST, "/v1/host/register", Some(&body)).await?;
    if v["computerId"].as_str() != Some(hub.identity.computer_id().as_str()) {
        bail!("the cloud registered a different computer id");
    }
    tracing::info!(computer_id = hub.identity.computer_id(), "registered with the Donnabot cloud");
    update_status(hub, |s| s.registered = true);
    Ok(())
}

/// Short-lived media credentials, issued only after the host checks screen authorization.
pub async fn screen_ice(hub: &Hub, device_key: &str) -> Result<crate::screen::IceConfig> {
    let value =
        call(hub, &base(hub)?, Method::POST, "/v1/host/screen-ice", Some(&json!({"deviceKey": device_key}))).await?;
    let config: crate::screen::IceConfig =
        serde_json::from_value(value).context("invalid screen relay configuration")?;
    if config.ice_servers.is_empty() || config.expires_at <= now_ms() + 60_000 {
        bail!("the screen relay returned expired or empty credentials");
    }
    Ok(config)
}

// MARK: state (§4.3)

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct State {
    owner: Option<Owner>,
    #[serde(default)]
    grants: Vec<Grant>,
    #[serde(default)]
    requests: Vec<StateRequest>,
    #[serde(default)]
    bootstrap_publication: Option<Publication>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Grant {
    pub grant_id: String,
    pub device_key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StateRequest {
    request_id: String,
    device_key: String,
    #[serde(default)]
    device_name: String,
    #[serde(default)]
    platform: String,
    #[serde(default)]
    email: Option<String>,
    commit: String,
    #[serde(default)]
    host_nonce: Option<String>,
    #[serde(default)]
    device_nonce: Option<String>,
    #[serde(default)]
    created_at: i64,
    #[serde(default)]
    expires_at: i64,
}

/// `GET /v1/host/state` and apply it: renew or remove account devices, answer access requests.
pub async fn pull(hub: &Hub, base: &str) -> Result<()> {
    let _one = hub.cloud.pulling.lock().await;
    if let Some(envelopes) = hub.store.bootstrap_receipt_envelopes()? {
        if serde_json::to_vec(&envelopes)?.len() > 65_536 {
            bail!("bootstrap receipt exceeds limit");
        }
        // Notification only: never re-executes the proof or reconstructs signatures.
        // Bound the request by the original grant; still GET state after expiry to
        // reconcile a publication whose response/notification may have been lost.
        let remaining = hub.store.bootstrap_pending_deadline()?.map(|deadline| deadline.saturating_sub(now_ms()));
        if remaining.is_none_or(|ms| ms > 0) {
            let budget = remaining.map_or(TIMEOUT, |ms| Duration::from_millis(ms.unsigned_abs()).min(TIMEOUT));
            if let Err(error) = tokio::time::timeout(
                budget,
                call(hub, base, Method::POST, "/v1/host/bootstrap-receipts", Some(&envelopes)),
            )
            .await
            .map_err(anyhow::Error::from)
            .and_then(std::convert::identity)
            {
                tracing::warn!(%error, "bootstrap receipt notification pending");
            }
        }
    }
    if let Some(envelope) = hub.store.bootstrap_revocation_envelope()?
        && let Err(error) =
            call(hub, base, Method::POST, "/v1/host/bootstrap-revocations", Some(&json!({"revocation":envelope}))).await
    {
        tracing::warn!(%error, "bootstrap revocation notification pending");
    }
    // Keep the original until an exact control confirmation exists.
    let raw = call(hub, base, Method::GET, "/v1/host/state", None).await?;
    // Only absence is legacy. Null or malformed fences cannot erase pending work.
    let raw_fences = raw.get("fences").cloned();
    let state: State = serde_json::from_value(raw).context("reading the cloud state")?;
    let ack = match raw_fences {
        None => Ok(FenceAck { fences: Vec::new() }),
        Some(value) => (|| -> Result<FenceAck> {
            let list: Vec<Fence> = serde_json::from_value(value)?;
            // Validate the entire fence batch before applying any bootstrap mutation.
            confirmable(&hub.store, &list)?;
            for f in &list {
                if hub.store.bootstrap_is_grant(&f.grant_id)? {
                    let now = now_ms();
                    crate::fork::workspace::revogar_cercado(hub, &f.grant_id, &f.device_key, now)?;
                    hub.auth_changed();
                }
            }
            confirmable(&hub.store, &list)
        })(),
    };
    // A malformed/unapplied fence batch must not promote a pending owner.
    if ack.is_ok()
        && hub.store.bootstrap_needs_publication()?
        && let (Some(publication), Some(owner)) = (&state.bootstrap_publication, &state.owner)
        && let Some(g) =
            state.grants.iter().find(|g| g.grant_id == publication.grant_id && g.device_key == publication.device_key)
    {
        let listed = CloudGrant {
            grant_id: g.grant_id.clone(),
            device_key: g.device_key.clone(),
            owner_user_id: owner.user_id.clone(),
        };
        // Authority is the durable proof plus this authenticated publication; now only sets the normal lease.
        if hub.store.bootstrap_promote(publication, &listed, now_ms())? {
            hub.auth_changed();
        }
    }
    apply_grants(hub, &state.grants)?;
    if ack.is_ok()
        && let Some(activation) = hub.store.bootstrap_activation_envelope(&hub.identity.sign, now_ms())?
        && let Err(error) =
            call(hub, base, Method::POST, "/v1/host/bootstrap-activations", Some(&json!({"activation":activation})))
                .await
    {
        tracing::warn!(%error, "bootstrap active snapshot notification pending");
    }
    let confirmation: Result<()> = async {
        let ack = ack?;
        if !ack.fences.is_empty() {
            let body = serde_json::to_value(&ack)?;
            let result = call(hub, base, Method::POST, "/v1/host/fences/ack", Some(&body)).await?;
            if !acknowledged(&result, &ack)? {
                bail!("confirmação parcial de cercas");
            }
        }
        Ok(())
    }
    .await;
    // Preserve the legacy value, but never honour automatic approval.
    let legacy_auto = hub.store.kv_read(APPROVAL)?.as_deref() == Some("auto");
    update_status(hub, |s| {
        s.owner = state.owner;
        s.approval = Approval::Code;
        s.last_error = legacy_auto.then(|| "Configuração antiga de aprovação automática ignorada".into());
    });
    sync_requests(hub, base, state.requests).await;
    retry_revokes(hub, base).await;
    if confirmation.is_err() {
        update_status(hub, |s| {
            let message = "A retirada local foi aplicada, mas sua confirmação na nuvem continua pendente.";
            s.last_error = Some(match &s.last_error {
                Some(previous) => format!("{previous} {message}"),
                None => message.into(),
            });
        });
    }
    if hub.store.bootstrap_needs_publication()? {
        bail!("A publicação do recibo de vínculo continua pendente; confirme o estado do pedido original.");
    }
    Ok(())
}

/// The cloud can only shorten access: a local account device listed with the same grant and
/// key is renewed, one missing is removed; grants this host never approved are ignored.
pub fn apply_grants(hub: &Hub, grants: &[Grant]) -> Result<()> {
    let until = now_ms() + LEASE_MS;
    let mut known = vec![false; grants.len()];
    for d in hub.store.devices()? {
        if d.source != DeviceSource::Account {
            continue;
        }
        let listed = grants.iter().position(|g| Some(&g.grant_id) == d.grant_id.as_ref() && g.device_key == d.key);
        if let Some(i) = listed {
            known[i] = true;
            hub.store.set_lease(&d.key, until)?;
        } else {
            tracing::info!(device = d.key, "account device's grant is gone from the cloud");
            hub.revoke_device(&d.key)?;
        }
    }
    for (g, _) in grants.iter().zip(known).filter(|(_, k)| !k) {
        tracing::warn!(grant_id = g.grant_id, "unknown grant from cloud ignored");
    }
    Ok(())
}

enum Next {
    /// Seen for the first time without a host nonce: send ours.
    Offer(Box<Request>),
    /// Refuse (lost our nonce in a restart, tampered with, or the commit didn't match).
    Deny(String),
}

/// Keeps the pending access requests in step with the cloud (§4.2 B steps 4 and 6).
async fn sync_requests(hub: &Hub, base: &str, list: Vec<StateRequest>) {
    let mut next = vec![];
    let mut changed = false;
    let mut throttled = false;
    let mut budget_error = false;
    {
        let mut mine = hub.cloud.requests.locked();
        let before = mine.len();
        let mut tombstones = hub.cloud.tombstones.locked();
        mine.retain(|r| {
            let kept = r.consuming || list.iter().any(|s| s.request_id == r.id);
            if !kept {
                tombstones.insert(r.id.clone(), None, DecideResult::Refused { reason: "notFound" }, now_ms());
            }
            if !kept && r.code.is_none() {
                // Still counted against the hourly nonce budget.
                tracing::warn!(request_id = r.id, "access request vanished without revealing its nonce");
            }
            kept
        });
        changed |= mine.len() != before;
        let mut budget = match nonce_budget(&hub.store) {
            Ok(sent) => sent,
            Err(error) => {
                tracing::warn!(error = %error, "access request budget unavailable");
                budget_error = true;
                Vec::new()
            }
        };
        for s in &list {
            if tombstones.contains(&s.request_id, now_ms()) {
                next.push(Next::Deny(s.request_id.clone()));
                continue;
            }
            let (Ok(dk), Ok(commit)) = (crypto::unb64_n::<32>(&s.device_key), crypto::unb64_n::<32>(&s.commit)) else {
                tombstones.insert(
                    s.request_id.clone(),
                    None,
                    DecideResult::Refused { reason: "transcriptMismatch" },
                    now_ms(),
                );
                next.push(Next::Deny(s.request_id.clone()));
                continue;
            };
            let now = now_ms();
            let Some(i) = mine.iter().position(|r| r.id == s.request_id) else {
                if next.iter().any(|n| matches!(n, Next::Offer(r) if r.id == s.request_id)) {
                    continue;
                }
                if s.expires_at <= now {
                    tombstones.insert(
                        s.request_id.clone(),
                        None,
                        DecideResult::Refused { reason: "requestExpired" },
                        now,
                    );
                    next.push(Next::Deny(s.request_id.clone()));
                    continue;
                }
                // Our nonce is only in memory: after a restart this request can't be finished.
                if s.host_nonce.is_some() {
                    tombstones.insert(s.request_id.clone(), None, DecideResult::Refused { reason: "notFound" }, now);
                    next.push(Next::Deny(s.request_id.clone()));
                } else if mine.iter().any(|r| r.device_key == dk && r.code.is_none())
                    || next.iter().any(|n| matches!(n, Next::Offer(r) if r.device_key == dk))
                {
                    // One unrevealed request per device at a time; this one waits its turn.
                } else if budget_error {
                    // No new offer may be created when the durable budget is unknown.
                } else if budget.len() >= SAS_NONCES_PER_HOUR {
                    throttled = true;
                } else {
                    budget.push(now_ms());
                    next.push(Next::Offer(Box::new(Request {
                        id: s.request_id.clone(),
                        device_key: dk,
                        device_name: s.device_name.clone(),
                        platform: s.platform.clone(),
                        email: s.email.clone(),
                        commit,
                        host_nonce: crypto::random(),
                        code: None,
                        confirm_code: None,
                        transcript: None,
                        incarnation: crypto::random(),
                        consuming: false,
                        created_at: s.created_at,
                        expires_at: s.expires_at.min(now.saturating_add(REQUEST_MAX_MS)),
                    })));
                }
                continue;
            };
            let r = &mut mine[i];
            // Later cloud values may shorten, but never extend, the first deadline.
            let previous_deadline = r.expires_at;
            r.expires_at = r.expires_at.min(s.expires_at);
            changed |= r.expires_at != previous_deadline;
            if r.expires_at < now {
                next.push(Next::Deny(r.id.clone()));
                tombstones.insert(r.id.clone(), None, DecideResult::Refused { reason: "requestExpired" }, now);
                mine.remove(i);
                changed = true;
                continue;
            }
            let ours = b64(&r.host_nonce);
            if r.device_key != dk || r.commit != commit || s.host_nonce.as_deref() != Some(ours.as_str()) {
                tracing::warn!(request_id = r.id, "access request changed in the cloud; denying it");
                next.push(Next::Deny(r.id.clone()));
                tombstones.insert(r.id.clone(), None, DecideResult::Refused { reason: "transcriptMismatch" }, now);
                mine.remove(i);
                changed = true;
                continue;
            }
            if r.consuming || r.code.is_some() {
                continue;
            }
            let Some(nd) = s.device_nonce.as_deref() else { continue };
            // The second code is computed first: without it (invalid requestId) the request is never approvable.
            let accepted =
                crypto::unb64_n::<32>(nd).ok().filter(|nd| crypto::sas_commit(&dk, nd) == r.commit).and_then(|nd| {
                    crypto::access_confirm_code(&hub.identity.sign_pub(), &dk, &nd, &r.host_nonce, &r.id)
                        .map(|confirm| (nd, confirm))
                });
            if let Some((nd, confirm_code)) = accepted {
                r.code = Some(crypto::sas_code(&hub.identity.sign_pub(), &dk, &nd, &r.host_nonce));
                r.confirm_code = Some(confirm_code);
                r.transcript = Some(approval::transcript(
                    &crypto::computer_id_raw(&hub.identity.sign_pub()),
                    &r.id,
                    &dk,
                    &r.commit,
                    &r.host_nonce,
                    &nd,
                    &r.incarnation,
                ));
                changed = true;
            } else {
                tracing::warn!(
                    request_id = r.id,
                    "device nonce doesn't match its commit, or the request id can't yield a confirmation code; denying the request"
                );
                next.push(Next::Deny(r.id.clone()));
                tombstones.insert(r.id.clone(), None, DecideResult::Refused { reason: "transcriptMismatch" }, now);
                mine.remove(i);
                changed = true;
            }
        }
        // Reserve the whole batch durably before any nonce or request is published.
        if next.iter().any(|n| matches!(n, Next::Offer(_)))
            && let Err(error) = set_nonce_budget(&hub.store, &budget)
        {
            tracing::warn!(error = %error, "access request budget could not be saved");
            next.retain(|n| !matches!(n, Next::Offer(_)));
            budget_error = true;
        }
    }
    if budget_error {
        update_status(hub, |s| s.last_error = Some(BUDGET_ERROR.into()));
    } else if throttled {
        tracing::warn!("accessRequests throttled: too many account access requests this hour");
        update_status(hub, |s| {
            s.last_error = Some("Too many requests to use this computer. New ones wait up to an hour.".into());
        });
    }
    for n in next {
        match n {
            Next::Offer(r) => {
                if now_ms() > r.expires_at {
                    hub.cloud.tombstones.locked().insert(
                        r.id.clone(),
                        None,
                        DecideResult::Refused { reason: "requestExpired" },
                        now_ms(),
                    );
                    deny(hub, base, &r.id).await;
                    continue;
                }
                let path = format!("/v1/host/access-requests/{}/nonce", r.id);
                match call(hub, base, Method::POST, &path, Some(&json!({"nonce": b64(&r.host_nonce)}))).await {
                    Ok(_) if now_ms() <= r.expires_at => {
                        hub.cloud.requests.locked().push(*r);
                        changed = true;
                    }
                    Ok(_) => {
                        hub.cloud.tombstones.locked().insert(
                            r.id.clone(),
                            None,
                            DecideResult::Refused { reason: "requestExpired" },
                            now_ms(),
                        );
                        deny(hub, base, &r.id).await;
                    }
                    // Someone else set a nonce: not one we can stand behind.
                    Err(e) if cloud_status_of(&e) == Some(409) => deny(hub, base, &r.id).await,
                    Err(e) => {
                        tracing::warn!(
                            error = format!("{e:#}"),
                            request_id = r.id,
                            "couldn't answer an access request"
                        );
                    }
                }
            }
            Next::Deny(id) => deny(hub, base, &id).await,
        }
    }
    if changed {
        emit_requests(hub);
    }
}

/// The host nonces sent in the last hour.
fn nonce_budget(store: &Store) -> Result<Vec<i64>> {
    let now = now_ms();
    let Some(raw) = store.kv_read(SAS_NONCES)? else { return Ok(Vec::new()) };
    let sent: Vec<i64> = serde_json::from_str(&raw).context("invalid access request budget")?;
    if sent.iter().any(|t| *t < 0 || *t > now) {
        bail!("invalid access request budget timestamp");
    }
    Ok(sent.into_iter().filter(|t| *t > now - HOUR_MS).collect())
}

fn set_nonce_budget(store: &Store, sent: &[i64]) -> Result<()> {
    store.kv_set(SAS_NONCES, &serde_json::to_string(sent)?)
}

fn grantable_scopes(scopes: &[Scope]) -> Vec<Scope> {
    scopes.iter().copied().filter(|s| *s != Scope::OwnerDevices && *s != Scope::Executor).collect()
}

#[derive(Clone)]
struct Decider {
    key: String,
    grant_id: String,
    scopes: Vec<Scope>,
}
fn decider(hub: &Hub, caller: &Caller) -> Result<Option<Decider>> {
    match caller {
        Caller::Local => Ok(None),
        Caller::Device { key, .. } => {
            let d = devices::authorize(hub, key).map_err(|_| anyhow!("device no longer authorized"))?;
            if d.source != DeviceSource::Account || d.grant_id.is_none() || !d.scopes.contains(&Scope::OwnerDevices) {
                bail!("live owner.devices grant required");
            }
            let scopes = grantable_scopes(&d.scopes);
            if scopes.is_empty() {
                bail!("no grantable scopes");
            }
            Ok(Some(Decider {
                key: key.clone(),
                grant_id: d.grant_id.ok_or_else(|| anyhow!("grant missing"))?,
                scopes,
            }))
        }
        _ => bail!("not an owner device"),
    }
}

async fn decision_with_decider(
    hub: &Hub,
    base: &str,
    id: &str,
    approve: bool,
    owner: Option<&Decider>,
) -> Result<Value> {
    let path = format!("/v1/host/access-requests/{id}/decision");
    let mut body = json!({"decision": if approve { "approve" } else { "deny" }});
    if let Some(owner) = owner {
        body["deciderDeviceKey"] = json!(owner.key);
        body["approvedScopes"] = serde_json::to_value(&owner.scopes)?;
    }
    call(hub, base, Method::POST, &path, Some(&body)).await
}

async fn decision(hub: &Hub, base: &str, id: &str, approve: bool) -> Result<Value> {
    let path = format!("/v1/host/access-requests/{id}/decision");
    let decision = if approve { "approve" } else { "deny" };
    call(hub, base, Method::POST, &path, Some(&json!({"decision": decision}))).await
}

async fn deny(hub: &Hub, base: &str, id: &str) {
    if let Err(e) = decision(hub, base, id, false).await {
        tracing::warn!(error = format!("{e:#}"), request_id = id, "couldn't deny an access request");
    }
}

/// `decideAccessRequest`: approving adds the device the user saw (its key, the SAS they
/// compared) with the grant the cloud returns; nothing else can add an account device.
/// `how`: only the local code comparison is accepted.
pub async fn decide(hub: &Hub, id: &str, approve: bool, expected: Option<Expected>) -> Result<DecideResult> {
    decide_as(hub, &Caller::Local, id, approve, expected).await
}

pub async fn decide_as(
    hub: &Hub,
    caller: &Caller,
    id: &str,
    approve: bool,
    expected: Option<Expected>,
) -> Result<DecideResult> {
    let authority = decider(hub, caller)?;
    if approve && expected.as_ref().is_none_or(|e| !e.valid()) {
        bail!("valid `expect` is required");
    }
    let request = {
        let mut mine = hub.cloud.requests.locked();
        let mut tombstones = hub.cloud.tombstones.locked();
        if let Some(result) = tombstones.result(id, expected.as_ref(), now_ms()) {
            return Ok(result);
        }
        let Some(i) = mine.iter().position(|r| r.id == id) else {
            return Ok(DecideResult::Refused { reason: "notFound" });
        };
        let r = &mine[i];
        if r.consuming {
            return Ok(DecideResult::Refused { reason: "consuming" });
        }
        let reason = if now_ms() > r.expires_at {
            Some("requestExpired")
        } else if approve
            && expected.as_ref().is_none_or(|e| {
                e.device_key != b64(&r.device_key)
                    || !r.code.as_ref().is_some_and(|code| crypto::ct_eq(code.as_bytes(), e.code.as_bytes()))
                    || r.transcript.as_ref() != Some(&e.transcript)
            })
        {
            Some("transcriptMismatch")
        } else {
            None
        };
        if let Some(reason) = reason {
            mine.remove(i);
            let result = DecideResult::Refused { reason };
            tombstones.insert(id.to_owned(), expected, result.clone(), now_ms());
            drop(tombstones);
            drop(mine);
            emit_requests(hub);
            return Ok(result);
        }
        mine[i].consuming = true;
        mine[i].clone()
    };
    emit_requests(hub);
    let answer = match base(hub) {
        Ok(base) => decision_with_decider(hub, &base, id, approve, authority.as_ref()).await,
        Err(error) => Err(error),
    };
    let mut mine = hub.cloud.requests.locked();
    let mut tombstones = hub.cloud.tombstones.locked();
    let current = mine.iter().position(|r| r.id == id && r.incarnation == request.incarnation && r.consuming);
    let authority_still_valid = match (&authority, decider(hub, caller)) {
        (None, Ok(None)) => true,
        (Some(before), Ok(Some(after))) => {
            before.key == after.key && before.grant_id == after.grant_id && before.scopes == after.scopes
        }
        _ => false,
    };
    let current_valid = authority_still_valid
        && current.is_some_and(|i| {
            now_ms() <= mine[i].expires_at
                && (!approve
                    || expected.as_ref().is_some_and(|e| {
                        e.device_key == b64(&mine[i].device_key)
                            && mine[i]
                                .code
                                .as_ref()
                                .is_some_and(|code| crypto::ct_eq(code.as_bytes(), e.code.as_bytes()))
                            && mine[i].transcript.as_ref() == Some(&e.transcript)
                    }))
        });
    if let Some(i) = current {
        mine.remove(i);
    }
    let result = match answer {
        Err(_) => DecideResult::Unknown { reason: "cloudUnreachable" },
        Ok(answer) if !approve && current_valid && answer["status"] == "denied" => DecideResult::Denied,
        Ok(_) if !approve => DecideResult::Unknown { reason: "superseded" },
        Ok(answer) => {
            let grant = answer["grantId"].as_str().filter(|g| g.starts_with("grt_") && g.len() > 4);
            if let Some(grant) = grant {
                let device = Device {
                    key: b64(&request.device_key),
                    name: request.device_name.chars().take(100).collect(),
                    platform: request.platform.clone(),
                    source: DeviceSource::Account,
                    grant_id: Some(grant.to_owned()),
                    scopes: authority
                        .as_ref()
                        .map_or_else(|| vec![Scope::Control, Scope::Screen], |d| d.scopes.clone()),
                    lease_until: Some(now_ms() + LEASE_MS),
                    created_at: now_ms(),
                    last_seen_at: None,
                };
                let saved = if current_valid && answer["status"] == "approved" {
                    if let Some(ref owner) = authority {
                        hub.cloud_synced.load(std::sync::atomic::Ordering::Acquire)
                            && hub
                                .store
                                .bootstrap_put_delegated(&owner.key, &owner.grant_id, &owner.scopes, &device, now_ms())
                                .is_ok()
                    } else {
                        hub.store.put_device(&device).is_ok()
                    }
                } else {
                    false
                };
                if saved {
                    crate::analytics::device_paired(hub, crate::analytics::Via::Account, &device.platform, false);
                    hub.auth_changed();
                    DecideResult::Approved {
                        device_key: device.key,
                        grant_id: grant.to_owned(),
                        confirm_code: request.confirm_code.clone(),
                    }
                } else {
                    let mut pending = pending_revokes(&hub.store);
                    if !pending.iter().any(|value| value == grant) {
                        pending.push(grant.to_owned());
                        set_pending_revokes(&hub.store, &pending);
                    }
                    DecideResult::Unknown { reason: "superseded" }
                }
            } else {
                DecideResult::Unknown { reason: "cloudUnreachable" }
            }
        }
    };
    tombstones.insert(id.to_owned(), expected, result.clone(), now_ms());
    drop(tombstones);
    drop(mine);
    emit_requests(hub);
    Ok(result)
}

// MARK: grants removed here

fn pending_revokes(store: &Store) -> Vec<String> {
    store.kv_get(PENDING_REVOKES).and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default()
}

fn set_pending_revokes(store: &Store, ids: &[String]) {
    let saved = serde_json::to_string(ids).map_err(anyhow::Error::from).and_then(|v| store.kv_set(PENDING_REVOKES, &v));
    if let Err(e) = saved {
        tracing::warn!(error = format!("{e:#}"), "couldn't remember a grant to revoke");
    }
}

/// Tells the cloud an account device was removed here. Local removal already took effect;
/// a failure is retried on later state pulls.
pub async fn revoke_grant(hub: Arc<Hub>, grant_id: String) {
    let mut pending = pending_revokes(&hub.store);
    if !pending.contains(&grant_id) {
        pending.push(grant_id);
        set_pending_revokes(&hub.store, &pending);
    }
    if let Ok(base) = base(&hub) {
        retry_revokes(&hub, &base).await;
    }
}

async fn retry_revokes(hub: &Hub, base: &str) {
    let pending = pending_revokes(&hub.store);
    if pending.is_empty() {
        return;
    }
    let mut left = vec![];
    for id in pending {
        match call(hub, base, Method::POST, &format!("/v1/host/grants/{id}/revoke"), Some(&json!({}))).await {
            Ok(_) => {}
            Err(e) if cloud_status_of(&e) == Some(404) && !hub.store.bootstrap_is_grant(&id).unwrap_or(true) => {}
            Err(e) => {
                tracing::warn!(error = format!("{e:#}"), grant_id = id, "couldn't revoke a grant in the cloud");
                left.push(id);
            }
        }
    }
    set_pending_revokes(&hub.store, &left);
}

// MARK: loopback methods

/// `unclaim`: takes this computer out of its account.
pub async fn unclaim(hub: &Hub) -> Result<()> {
    let base = base(hub)?;
    call(hub, &base, Method::POST, "/v1/host/unclaim", Some(&json!({}))).await?;
    tracing::info!("left the account");
    pull(hub, &base).await
}

/// `setCloud`: turns the cloud on or off; the relay restarts.
pub fn set_cloud(hub: &Hub, enabled: bool) -> Result<CloudStatus> {
    hub.store.kv_set("cloud_enabled", if enabled { "true" } else { "false" })?;
    let now = url(&hub.store);
    update_status(hub, |s| {
        s.enabled = now.is_some();
        s.url.clone_from(&now);
    });
    hub.cloud.config.send_modify(|g| *g += 1);
    Ok(hub.cloud.status())
}

/// `setApproval`: whether account devices need the 6-digit check. Local only.
pub fn set_approval(hub: &Hub, approval: Approval) -> Result<CloudStatus> {
    if approval == Approval::Auto {
        bail!("Aprovação automática não existe na Mexerica");
    }
    hub.store.kv_set(APPROVAL, "code")?;
    tracing::info!(?approval, "account approval changed");
    update_status(hub, |s| s.approval = approval);
    Ok(hub.cloud.status())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::remote::identity::Identity;

    fn temp_hub() -> Arc<Hub> {
        let dir = std::env::temp_dir().join(format!("codync-cloud-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("t.db")).unwrap();
        Hub::new(store, "host".into(), Identity::load_or_create(&dir).unwrap(), "token".into(), 0)
    }

    fn account_device(key: &str, grant: &str) -> Device {
        Device {
            key: key.into(),
            name: "Phone".into(),
            platform: "ios".into(),
            source: DeviceSource::Account,
            grant_id: Some(grant.into()),
            scopes: vec![Scope::Control, Scope::Screen],
            lease_until: Some(now_ms() + 1000),
            created_at: now_ms(),
            last_seen_at: None,
        }
    }

    #[test]
    fn state_only_renews_or_removes() {
        let hub = temp_hub();
        hub.store.put_device(&account_device("kept", "grt_a")).unwrap();
        hub.store.put_device(&account_device("gone", "grt_b")).unwrap();
        hub.store.put_device(&account_device("swapped", "grt_c")).unwrap();
        let mut local = account_device("qr", "x");
        (local.source, local.grant_id, local.lease_until) = (DeviceSource::Local, None, None);
        hub.store.put_device(&local).unwrap();
        let grant = |g: &str, k: &str| Grant { grant_id: g.into(), device_key: k.into() };
        apply_grants(&hub, &[grant("grt_a", "kept"), grant("grt_c", "another key"), grant("grt_injected", "attacker")])
            .unwrap();
        let keys: Vec<String> = hub.store.devices().unwrap().into_iter().map(|d| d.key).collect();
        assert_eq!(keys, ["kept", "qr"], "unknown grants add nothing; a grant naming another key doesn't count");
        assert!(hub.store.device("kept").unwrap().lease_until.unwrap() > now_ms() + LEASE_MS - 5000, "renewed");
        assert_eq!(hub.store.device("qr").unwrap().lease_until, None, "QR devices aren't touched");
    }

    #[test]
    fn cloud_is_unconfigured_by_default_in_every_build() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        assert!(build_cloud_url().is_none());
        assert!(crate::environment::Environment::Main.cloud_url().is_none());
        assert!(crate::environment::Environment::Dev.cloud_url().is_none());
        assert!(url(&store).is_none());
        store.kv_set("cloud_enabled", "true").unwrap();
        assert!(url(&store).is_none(), "preferência antiga não configura uma nuvem");
        store.kv_set("test_cloud_url", "http://127.0.0.1:9").unwrap();
        assert!(url(&store).is_some());
        store.kv_set("cloud_enabled", "false").unwrap();
        assert!(url(&store).is_none(), "desligada aqui vale mesmo com uma nuvem configurada");
    }

    #[test]
    fn cloud_authorities_are_normalized() {
        assert_eq!(authority("https://Codync.Example.dev").unwrap(), "codync.example.dev");
        assert_eq!(authority("http://127.0.0.1:8787").unwrap(), "127.0.0.1:8787");
        assert_eq!(authority("https://x.dev:443").unwrap(), "x.dev", "default port is left out");
    }

    #[derive(Default)]
    struct FakeCloud {
        state: Mutex<Value>,
        posts: Mutex<Vec<(String, Value)>>,
        decision_gate: Mutex<Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>>,
    }

    async fn fake_cloud(fake: Arc<FakeCloud>) -> String {
        use axum::extract::{OriginalUri, State as Ax};
        use axum::routing::{get, post};
        let app = axum::Router::new()
            .route("/v1/host/state", get(|Ax(f): Ax<Arc<FakeCloud>>| async move { axum::Json(f.state.lock().unwrap().clone()) }))
            .route(
                "/v1/host/access-requests/{id}/{action}",
                post(|Ax(f): Ax<Arc<FakeCloud>>, OriginalUri(uri): OriginalUri, axum::Json(b): axum::Json<Value>| async move {
                    let path = uri.path().to_owned();
                    f.posts.lock().unwrap().push((path.clone(), b.clone()));
                    let gate = if path.ends_with("/decision") { f.decision_gate.lock().unwrap().clone() } else { None };
                    if let Some((started, release)) = gate {
                        started.notify_one();
                        release.notified().await;
                    }
                    axum::Json(if path.ends_with("/decision") && b["decision"] == "approve" {
                        json!({"status": "approved", "grantId": "grt_new"})
                    } else if path.ends_with("/decision") && b["decision"] == "deny" {
                        json!({"status": "denied"})
                    } else {
                        json!({})
                    })
                }),
            )
            .with_state(fake);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        base
    }

    #[tokio::test]
    async fn access_request_needs_a_matching_reveal_before_approval() {
        let hub = temp_hub();
        let fake = Arc::new(FakeCloud::default());
        let base = fake_cloud(fake.clone()).await;
        hub.store.kv_set("test_cloud_url", &base).unwrap();
        let (dk, other_dk): ([u8; 32], [u8; 32]) = (crypto::random(), crypto::random());
        let (nd, wrong): ([u8; 32], [u8; 32]) = (crypto::random(), crypto::random());
        let request = |id: &str, host_nonce: Option<String>, device_nonce: Option<[u8; 32]>| {
            let dk = if id == "req_bad" { other_dk } else { dk };
            json!({"requestId": id, "deviceKey": b64(&dk), "deviceName": "Kevin's iPhone", "platform": "ios",
                "email": "k@example.com", "commit": b64(&crypto::sas_commit(&dk, &nd)),
                "hostNonce": host_nonce, "deviceNonce": device_nonce.map(|n| b64(&n)),
                "createdAt": now_ms(), "expiresAt": now_ms() + 60_000})
        };
        *fake.state.lock().unwrap() = json!({"owner": {"userId": "user_1", "email": "k@example.com"}, "grants": [],
                "requests": [request("req_ok", None, None), request("req_bad", None, None)]});
        pull(&hub, &base).await.unwrap();
        let nonces: Vec<(String, Value)> = fake.posts.lock().unwrap().clone();
        assert_eq!(nonces.len(), 2, "the host sends one nonce per request");
        let nh =
            |id: &str| nonces.iter().find(|(p, _)| p.contains(id)).unwrap().1["nonce"].as_str().unwrap().to_owned();
        assert_eq!(hub.cloud.requests_json()[0]["code"], Value::Null, "no code before the reveal");
        assert!(decide(&hub, "req_ok", true, None).await.is_err(), "approve requires an expected transcript");
        assert_eq!(hub.cloud.status().owner.unwrap().user_id, "user_1");

        *fake.state.lock().unwrap() = json!({"owner": null, "grants": [], "requests": [
            request("req_ok", Some(nh("req_ok")), Some(nd)),
            request("req_bad", Some(nh("req_bad")), Some(wrong)),
        ]});
        pull(&hub, &base).await.unwrap();
        let denied =
            fake.posts.lock().unwrap().iter().any(|(p, b)| p.ends_with("req_bad/decision") && b["decision"] == "deny");
        assert!(denied, "a reveal that doesn't match its commit is denied");
        let list = hub.cloud.requests_json();
        assert_eq!(list.len(), 1);
        let nh_ok: [u8; 32] = crypto::unb64_n(&nh("req_ok")).unwrap();
        assert_eq!(list[0]["code"], crypto::sas_code(&hub.identity.sign_pub(), &dk, &nd, &nh_ok));

        let expected = Expected {
            device_key: list[0]["deviceKey"].as_str().unwrap().into(),
            code: list[0]["code"].as_str().unwrap().into(),
            transcript: list[0]["transcript"].as_str().unwrap().into(),
        };
        let confirm = crypto::access_confirm_code(&hub.identity.sign_pub(), &dk, &nd, &nh_ok, "req_ok").unwrap();
        let listed = serde_json::to_string(&hub.cloud.requests_json()).unwrap();
        assert!(!listed.contains(&confirm) && !listed.contains("confirm"), "the list never carries the confirmation");
        let first = decide(&hub, "req_ok", true, Some(expected.clone())).await.unwrap();
        assert!(
            matches!(&first, DecideResult::Approved { confirm_code: Some(c), .. } if *c == confirm),
            "the confirmation comes only with the saved approval"
        );
        assert_eq!(serde_json::to_value(&first).unwrap()["confirmCode"], confirm.as_str());
        let again = decide(&hub, "req_ok", true, Some(expected.clone())).await.unwrap();
        assert_eq!(again, first, "asking again with the same capture gives the same result");
        let mut other = expected;
        other.code = if other.code == "000000" { "000001".into() } else { "000000".into() };
        assert!(
            !matches!(decide(&hub, "req_ok", true, Some(other)).await.unwrap(), DecideResult::Approved { .. }),
            "another capture never sees the confirmation"
        );
        let d = hub.store.device(&b64(&dk)).unwrap();
        assert_eq!((d.source, d.grant_id.as_deref()), (DeviceSource::Account, Some("grt_new")));
        assert_eq!(hub.cloud.requests_json().len(), 0);
    }

    /// Aprovar exige o código que a pessoa leu no aparelho: sem código ou com código errado não aprova, o
    /// erro queima o pedido (uma tentativa) e o código certo depois disso não reaproveita o pedido.
    #[tokio::test]
    async fn approval_needs_the_typed_code_once() {
        let hub = temp_hub();
        let fake = Arc::new(FakeCloud::default());
        let base = fake_cloud(fake.clone()).await;
        hub.store.kv_set("test_cloud_url", &base).unwrap();
        let (dk, nd): ([u8; 32], [u8; 32]) = (crypto::random(), crypto::random());
        let request = |host_nonce: Option<String>, device_nonce: Option<[u8; 32]>| {
            json!({"owner": null, "grants": [], "requests": [{"requestId": "req_typed", "deviceKey": b64(&dk),
                "deviceName": "Fixture phone", "platform": "ios", "email": "fixture@example.invalid",
                "commit": b64(&crypto::sas_commit(&dk, &nd)), "hostNonce": host_nonce,
                "deviceNonce": device_nonce.map(|n| b64(&n)), "createdAt": now_ms(), "expiresAt": now_ms() + 60_000}]})
        };
        *fake.state.lock().unwrap() = request(None, None);
        pull(&hub, &base).await.unwrap();
        assert_eq!(hub.cloud.requests_json()[0]["codeReady"], false);
        let nh = fake.posts.lock().unwrap()[0].1["nonce"].as_str().unwrap().to_owned();
        *fake.state.lock().unwrap() = request(Some(nh.clone()), Some(nd));
        pull(&hub, &base).await.unwrap();
        let nh: [u8; 32] = crypto::unb64_n(&nh).unwrap();
        let code = crypto::sas_code(&hub.identity.sign_pub(), &dk, &nd, &nh);

        let remote = hub.cloud.requests_json_sem_codigo();
        assert_eq!(remote.len(), 1);
        assert_eq!(remote[0]["codeReady"], true);
        assert_eq!(remote[0]["codeCheck"], "server", "o Hub declara que confere o código digitado");
        assert_eq!(hub.cloud.requests_json()[0]["codeCheck"], "server", "também para este computador");
        assert!(remote[0].get("code").is_none() && !serde_json::to_string(&remote).unwrap().contains(&code));

        let transcript = remote[0]["transcript"].as_str().unwrap().to_owned();
        assert!(decide(&hub, "req_typed", true, None).await.is_err(), "without a typed code nothing is decided");
        assert_eq!(hub.cloud.requests_json().len(), 1, "and the request is still there");

        let typed = |code: &str| Expected { device_key: b64(&dk), code: code.into(), transcript: transcript.clone() };
        let wrong = if code == "000000" { "000001" } else { "000000" };
        assert_eq!(
            decide(&hub, "req_typed", true, Some(typed(wrong))).await.unwrap(),
            DecideResult::Refused { reason: "transcriptMismatch" }
        );
        assert!(hub.cloud.requests_json().is_empty(), "a wrong code uses the only attempt");
        assert!(
            !matches!(
                decide(&hub, "req_typed", true, Some(typed(&code))).await.unwrap(),
                DecideResult::Approved { .. }
            ),
            "the right code can't reuse a burned request"
        );
        assert!(hub.store.device(&b64(&dk)).is_none());
        assert!(
            !fake.posts.lock().unwrap().iter().any(|(p, b)| p.ends_with("/decision") && b["decision"] == "approve")
        );
    }

    /// Um pedido revelado com código e transcrição conhecidos, sem depender do SAS real.
    async fn pedido_pronto(hub: &Arc<Hub>, fake: &Arc<FakeCloud>, base: &str, id: &str) -> Expected {
        let dk = crypto::random::<32>();
        let expires = now_ms() + 60_000;
        let row = |host_nonce: Option<String>| {
            json!({"requestId": id, "deviceKey": b64(&dk), "commit": b64(&[4u8; 32]), "hostNonce": host_nonce,
                "createdAt": now_ms(), "expiresAt": expires})
        };
        *fake.state.lock().unwrap() = json!({"owner": null, "grants": [], "requests": [row(None)]});
        pull(hub, base).await.unwrap();
        let nonce = fake.posts.lock().unwrap().last().unwrap().1["nonce"].as_str().unwrap().to_owned();
        *fake.state.lock().unwrap() = json!({"owner": null, "grants": [], "requests": [row(Some(nonce))]});
        pull(hub, base).await.unwrap();
        let mut mine = hub.cloud.requests.locked();
        let r = mine.iter_mut().find(|r| r.id == id).unwrap();
        r.code = Some("123456".into());
        r.transcript = Some(b64(&[9; 32]));
        Expected { device_key: b64(&dk), code: "123456".into(), transcript: b64(&[9; 32]) }
    }

    fn decisoes(fake: &FakeCloud) -> Vec<Value> {
        fake.posts.lock().unwrap().iter().filter(|(p, _)| p.ends_with("/decision")).map(|(_, b)| b.clone()).collect()
    }

    /// Código vazio ou fora do formato não decide nada, não queima o pedido e recebe a mesma mensagem fixa, que não
    /// ecoa o que foi enviado.
    #[tokio::test]
    async fn empty_or_malformed_code_decides_nothing_and_keeps_the_request() {
        let hub = temp_hub();
        let fake = Arc::new(FakeCloud::default());
        let base = fake_cloud(fake.clone()).await;
        hub.store.kv_set("test_cloud_url", &base).unwrap();
        let certo = pedido_pronto(&hub, &fake, &base, "req_formato").await;
        let fixa = "valid `expect` is required";
        for code in ["", "12345", "1234567", "12345a", " 23456", "１２３４５６"] {
            let typed = Expected { code: code.into(), ..certo.clone() };
            let e = decide(&hub, "req_formato", true, Some(typed)).await.unwrap_err();
            assert_eq!(format!("{e:#}"), fixa, "{code:?}");
            let expect = json!({"deviceKey": certo.device_key, "code": code, "transcript": certo.transcript});
            let body = json!({"requestId": "req_formato", "approve": true, "expect": expect});
            let e = crate::api::dispatch(&hub, &Caller::Local, "decideAccessRequest", body).await.unwrap_err();
            assert_eq!(format!("{e:#}"), fixa, "{code:?}");
        }
        for expect in [
            Value::Null,
            json!("654321"),
            json!({"deviceKey": certo.device_key, "code": 654_321, "transcript": certo.transcript}),
            json!({"deviceKey": certo.device_key, "code": "654321", "transcript": certo.transcript, "extra": "654321"}),
        ] {
            let body = json!({"requestId": "req_formato", "approve": true, "expect": expect});
            let e = crate::api::dispatch(&hub, &Caller::Local, "decideAccessRequest", body).await.unwrap_err();
            assert_eq!(format!("{e:#}"), fixa, "nada do corpo volta na mensagem");
        }
        assert_eq!(hub.cloud.requests_json().len(), 1, "nenhuma dessas tentativas queima o pedido");
        assert!(decisoes(&fake).is_empty(), "nada vai à nuvem");
        assert!(matches!(decide(&hub, "req_formato", true, Some(certo)).await.unwrap(), DecideResult::Approved { .. }));
    }

    /// A lista só leva o código a este computador; o aparelho com `owner.devices` recebe `codeReady` e `codeCheck`
    /// sem o código, e os demais chamadores não recebem a lista.
    #[tokio::test]
    async fn the_request_list_carries_the_code_only_to_this_computer() {
        let hub = temp_hub();
        let fake = Arc::new(FakeCloud::default());
        let base = fake_cloud(fake.clone()).await;
        hub.store.kv_set("test_cloud_url", &base).unwrap();
        pedido_pronto(&hub, &fake, &base, "req_escopo").await;
        let device = |scopes: Vec<Scope>| Caller::Device { key: "aparelho".into(), scopes, autoridade: None };
        let lista = |caller: Caller| {
            let hub = hub.clone();
            async move { crate::api::dispatch(&hub, &caller, "accessRequests", json!({})).await }
        };

        let local = lista(Caller::Local).await.unwrap();
        assert_eq!(local["requests"][0]["code"], "123456");
        assert_eq!(local["requests"][0]["codeCheck"], "server");

        let dono = lista(device(vec![Scope::OwnerDevices])).await.unwrap();
        let pedido = &dono["requests"][0];
        assert_eq!((pedido["codeReady"].as_bool(), pedido["codeCheck"].as_str()), (Some(true), Some("server")));
        assert!(pedido.get("code").is_none() && !dono.to_string().contains("123456"));

        for scopes in [vec![], vec![Scope::Control], vec![Scope::Screen], vec![Scope::Control, Scope::Screen]] {
            assert!(lista(device(scopes.clone())).await.is_err(), "{scopes:?}");
        }
        assert!(lista(device(vec![Scope::Executor])).await.is_err());
    }

    /// Aprovar e recusar o mesmo pedido ao mesmo tempo: só uma decisão vai à nuvem e vale; a outra encontra o pedido em
    /// consumo, nas duas ordens.
    #[tokio::test]
    async fn approve_and_deny_race_for_a_single_decision() {
        for aprova_primeiro in [true, false] {
            let hub = temp_hub();
            let fake = Arc::new(FakeCloud::default());
            let base = fake_cloud(fake.clone()).await;
            hub.store.kv_set("test_cloud_url", &base).unwrap();
            let certo = pedido_pronto(&hub, &fake, &base, "req_corrida").await;
            let started = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(tokio::sync::Notify::new());
            *fake.decision_gate.lock().unwrap() = Some((started.clone(), release.clone()));
            let primeira = {
                let hub = hub.clone();
                let expected = aprova_primeiro.then(|| certo.clone());
                tokio::spawn(async move { decide(&hub, "req_corrida", aprova_primeiro, expected).await })
            };
            started.notified().await;
            let segunda = decide(&hub, "req_corrida", !aprova_primeiro, (!aprova_primeiro).then(|| certo.clone()));
            assert_eq!(segunda.await.unwrap(), DecideResult::Refused { reason: "consuming" });
            release.notify_one();
            let vencedora = primeira.await.unwrap().unwrap();
            let gravado = hub.store.device(&certo.device_key).is_some();
            if aprova_primeiro {
                assert!(matches!(vencedora, DecideResult::Approved { .. }));
                assert!(gravado);
            } else {
                assert_eq!(vencedora, DecideResult::Denied);
                assert!(!gravado);
            }
            let enviadas = decisoes(&fake);
            assert_eq!(enviadas.len(), 1, "uma decisão só chega à nuvem");
            assert_eq!(enviadas[0]["decision"], if aprova_primeiro { "approve" } else { "deny" });
            let depois = decide(&hub, "req_corrida", !aprova_primeiro, (!aprova_primeiro).then(|| certo.clone()));
            assert!(!matches!(depois.await.unwrap(), DecideResult::Approved { .. } | DecideResult::Denied));
            assert_eq!(decisoes(&fake).len(), 1);
        }
    }

    #[tokio::test]
    async fn a_request_id_outside_the_alphabet_is_denied_and_never_gets_a_code() {
        let hub = temp_hub();
        let fake = Arc::new(FakeCloud::default());
        let base = fake_cloud(fake.clone()).await;
        hub.store.kv_set("test_cloud_url", &base).unwrap();
        let (dk, nd): ([u8; 32], [u8; 32]) = (crypto::random(), crypto::random());
        let request = |host_nonce: Option<String>, device_nonce: Option<[u8; 32]>| {
            json!({"owner": null, "grants": [], "requests": [{"requestId": "req.bad", "deviceKey": b64(&dk),
                "commit": b64(&crypto::sas_commit(&dk, &nd)), "hostNonce": host_nonce,
                "deviceNonce": device_nonce.map(|n| b64(&n)), "expiresAt": now_ms() + 60_000}]})
        };
        *fake.state.lock().unwrap() = request(None, None);
        pull(&hub, &base).await.unwrap();
        let nh = fake.posts.lock().unwrap()[0].1["nonce"].as_str().unwrap().to_owned();
        *fake.state.lock().unwrap() = request(Some(nh), Some(nd));
        pull(&hub, &base).await.unwrap();
        let denied =
            fake.posts.lock().unwrap().iter().any(|(p, b)| p.ends_with("req.bad/decision") && b["decision"] == "deny");
        assert!(denied, "without a second code the request is denied");
        assert!(hub.cloud.requests_json().is_empty(), "and it never becomes approvable");
    }

    #[tokio::test]
    async fn legacy_auto_is_ignored_and_rejected() {
        let hub = temp_hub();
        let fake = Arc::new(FakeCloud::default());
        let base = fake_cloud(fake.clone()).await;
        hub.store.kv_set("test_cloud_url", &base).unwrap();
        hub.store.kv_set(APPROVAL, "auto").unwrap();
        assert!(set_approval(&hub, Approval::Auto).is_err());
        assert_eq!(hub.store.kv_read(APPROVAL).unwrap().as_deref(), Some("auto"));
        let (dk, nd): ([u8; 32], [u8; 32]) = (crypto::random(), crypto::random());
        let request = |host_nonce: Option<String>, device_nonce: Option<[u8; 32]>| {
            json!({"owner": null, "grants": [], "requests": [{"requestId": "req_1", "deviceKey": b64(&dk),
                "commit": b64(&crypto::sas_commit(&dk, &nd)), "hostNonce": host_nonce,
                "deviceNonce": device_nonce.map(|n| b64(&n)), "expiresAt": now_ms() + 60_000}]})
        };
        *fake.state.lock().unwrap() = request(None, None);
        pull(&hub, &base).await.unwrap();
        assert!(hub.store.device(&b64(&dk)).is_none(), "nothing is approved before the reveal");
        let nh = fake.posts.lock().unwrap()[0].1["nonce"].as_str().unwrap().to_owned();
        *fake.state.lock().unwrap() = request(Some(nh), Some(nd));
        pull(&hub, &base).await.unwrap();
        assert!(hub.store.device(&b64(&dk)).is_none(), "legacy auto never approves");
        assert_eq!(hub.cloud.status().approval, Approval::Code);
        assert!(hub.cloud.status().last_error.as_deref().unwrap_or_default().contains("ignorada"));
    }

    #[tokio::test]
    async fn a_pull_that_works_clears_an_earlier_error() {
        let hub = temp_hub();
        let fake = Arc::new(FakeCloud::default());
        let base = fake_cloud(fake.clone()).await;
        *fake.state.lock().unwrap() = json!({"owner": null, "grants": [], "requests": []});
        update_status(&hub, |s| s.last_error = Some("couldn't get the cloud state".into()));
        pull(&hub, &base).await.unwrap();
        assert_eq!(hub.cloud.status().last_error, None, "Settings stops showing a relay error that's gone");
    }

    #[tokio::test]
    async fn invalid_fences_do_not_prevent_removal_or_disappear_from_status() -> Result<()> {
        let hub = temp_hub();
        let fake = Arc::new(FakeCloud::default());
        let base = fake_cloud(fake.clone()).await;
        let key = b64(&[1; 32]);
        hub.store.put_device(&account_device(&key, "grt_old"))?;
        *fake.state.locked() = json!({"owner": null, "grants": [], "requests": [], "fences": null});
        pull(&hub, &base).await?;
        assert!(hub.store.devices()?.is_empty());
        assert!(fake.posts.locked().is_empty());
        assert!(hub.cloud.status().last_error.is_some());
        // A failed ACK after the local removal is also retained after normal status cleanup.
        *fake.state.locked() = json!({"owner": null, "grants": [], "requests": [], "fences": [{"fenceId": "fnc_grt_old", "grantId": "grt_old", "deviceKey": key}]});
        pull(&hub, &base).await?;
        assert!(hub.cloud.status().last_error.is_some());
        assert!(hub.store.devices()?.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn host_nonces_are_rationed() {
        let hub = temp_hub();
        let fake = Arc::new(FakeCloud::default());
        let base = fake_cloud(fake.clone()).await;
        let request = |id: String, dk: [u8; 32]| json!({"requestId": id, "deviceKey": b64(&dk), "commit": b64(&[0u8; 32]), "expiresAt": now_ms() + 60_000});
        let one_key: [u8; 32] = crypto::random();
        let mut requests: Vec<Value> = (0..2).map(|i| request(format!("same_{i}"), one_key)).collect();
        requests.extend((0..8).map(|i| request(format!("req_{i}"), crypto::random())));
        *fake.state.lock().unwrap() = json!({"owner": null, "grants": [], "requests": requests});
        pull(&hub, &base).await.unwrap();
        let sent = fake.posts.lock().unwrap().len();
        assert_eq!(sent, SAS_NONCES_PER_HOUR, "a compromised cloud gets a few guesses an hour, not a million");
        assert!(!fake.posts.lock().unwrap().iter().any(|(p, _)| p.contains("same_1")), "one open request per key");

        // Requests that never reveal still used up the budget, across a restart too.
        *fake.state.lock().unwrap() =
            json!({"owner": null, "grants": [], "requests": [request("late".into(), crypto::random())]});
        hub.cloud.requests.locked().clear();
        pull(&hub, &base).await.unwrap();
        assert_eq!(fake.posts.lock().unwrap().len(), SAS_NONCES_PER_HOUR);
        assert!(hub.cloud.status().last_error.is_some(), "the Mac hears about it, though the pull worked");
    }

    #[tokio::test]
    async fn nonce_budget_corruption_and_failed_reservation_never_offer() {
        let hub = temp_hub();
        let fake = Arc::new(FakeCloud::default());
        let base = fake_cloud(fake.clone()).await;
        let request = || StateRequest {
            request_id: "fixture_request".into(),
            device_key: b64(&[1; 32]),
            device_name: String::new(),
            platform: String::new(),
            email: None,
            commit: b64(&[2; 32]),
            host_nonce: None,
            device_nonce: None,
            created_at: now_ms(),
            expires_at: now_ms() + REQUEST_MAX_MS,
        };
        hub.store.kv_set(SAS_NONCES, "not-json").unwrap();
        sync_requests(&hub, &base, vec![request()]).await;
        assert!(fake.posts.lock().unwrap().is_empty());
        assert_eq!(hub.cloud.requests_json(), [] as [Value; 0]);
        assert_eq!(hub.cloud.status().last_error.as_deref(), Some(BUDGET_ERROR));
        hub.store.kv_set(SAS_NONCES, "[]").unwrap();
        hub.store.db.locked().execute_batch("CREATE TRIGGER reject_nonce BEFORE UPDATE ON kv WHEN NEW.k='sas_nonces' BEGIN SELECT RAISE(FAIL, 'fixture'); END;").unwrap();
        sync_requests(&hub, &base, vec![request()]).await;
        assert!(fake.posts.lock().unwrap().is_empty());
        assert_eq!(hub.cloud.requests_json(), [] as [Value; 0]);
        assert_eq!(hub.store.kv_read(SAS_NONCES).unwrap().as_deref(), Some("[]"));
        hub.store.db.locked().execute_batch("DROP TABLE kv;").unwrap();
        assert!(nonce_budget(&hub.store).is_err(), "read errors never mean an empty budget");
    }

    #[tokio::test]
    async fn deadline_does_not_extend_and_is_checked_after_decision_await() {
        let hub = temp_hub();
        let fake = Arc::new(FakeCloud::default());
        let base = fake_cloud(fake.clone()).await;
        hub.store.kv_set("test_cloud_url", &base).unwrap();
        let dk = [3u8; 32];
        let deadline = now_ms() + 60_000;
        let row = |host_nonce: Option<String>, expires: i64| {
            json!({
                "requestId": "fixture_request", "deviceKey": b64(&dk), "commit": b64(&[4u8; 32]),
                "hostNonce": host_nonce, "createdAt": now_ms(), "expiresAt": expires
            })
        };
        *fake.state.lock().unwrap() = json!({"owner": null, "grants": [], "requests": [row(None, deadline)]});
        pull(&hub, &base).await.unwrap();
        let nonce = fake.posts.lock().unwrap()[0].1["nonce"].as_str().unwrap().to_owned();
        let first = hub.cloud.requests_json()[0]["expiresAt"].as_i64().unwrap();
        *fake.state.lock().unwrap() =
            json!({"owner": null, "grants": [], "requests": [row(Some(nonce), deadline + HOUR_MS)]});
        pull(&hub, &base).await.unwrap();
        assert_eq!(hub.cloud.requests_json()[0]["expiresAt"], first);
        hub.cloud.requests.locked()[0].code = Some("123456".into());
        hub.cloud.requests.locked()[0].transcript = Some(b64(&[9; 32]));
        let expected = Expected { device_key: b64(&dk), code: "123456".into(), transcript: b64(&[9; 32]) };
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        *fake.decision_gate.lock().unwrap() = Some((started.clone(), release.clone()));
        let task = {
            let hub = hub.clone();
            let expected = expected.clone();
            tokio::spawn(async move { decide(&hub, "fixture_request", true, Some(expected)).await })
        };
        started.notified().await;
        sync_requests(&hub, &base, vec![]).await;
        assert_eq!(hub.cloud.requests.locked().len(), 1, "pull omission preserves the consuming request");
        assert_eq!(
            decide(&hub, "fixture_request", true, Some(expected)).await.unwrap(),
            DecideResult::Refused { reason: "consuming" }
        );
        hub.cloud.requests.locked()[0].expires_at = now_ms() - 1;
        release.notify_one();
        assert_eq!(task.await.unwrap().unwrap(), DecideResult::Unknown { reason: "superseded" });
        assert!(hub.store.device(&b64(&dk)).is_none());
        assert!(pending_revokes(&hub.store).iter().any(|id| id == "grt_new"));
    }
}

#[cfg(test)]
mod redirect_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    //! 3xx da nuvem é recusado: o cliente não segue, a chamada falha com erro explícito e nenhum
    //! cabeçalho de credencial (`Codync-Sig`) chega ao destino do `Location`.
    use super::*;
    use crate::remote::identity::Identity;
    use axum::extract::Request;
    use axum::http::{HeaderMap, StatusCode, header};
    use axum::routing::{any, post};

    /// Registra os cabeçalhos de credencial de qualquer pedido que chegar.
    struct Spy {
        credentials: Mutex<Vec<String>>,
        hits: Mutex<usize>,
    }

    async fn spy_server(spy: Arc<Spy>) -> String {
        let app = axum::Router::new().fallback(any(move |req: Request| {
            let s = spy.clone();
            async move {
                let headers: &HeaderMap = req.headers();
                for name in ["codync-sig", "authorization"] {
                    if let Some(v) = headers.get(name).and_then(|v| v.to_str().ok()) {
                        s.credentials.lock().unwrap().push(format!("{name}: {v}"));
                    }
                }
                *s.hits.lock().unwrap() += 1;
                axum::Json(json!({}))
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        base
    }

    /// Devolve `code` com `Location` para `target` em qualquer caminho.
    async fn redirect_server(code: StatusCode, target: &str) -> String {
        let location = format!("{target}/v1/host/register");
        let app = axum::Router::new().route(
            "/v1/host/register",
            post(move || {
                let location = location.clone();
                async move { (code, [(header::LOCATION, location)]) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        base
    }

    fn temp_hub() -> Arc<Hub> {
        let dir = std::env::temp_dir().join(format!("codync-cloud-redir-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("t.db")).unwrap();
        Hub::new(store, "host".into(), Identity::load_or_create(&dir).unwrap(), "token".into(), 0)
    }

    #[tokio::test]
    async fn cloud_calls_refuse_every_redirect_and_send_no_credential_onward() {
        let hub = temp_hub();
        let spy = Arc::new(Spy { credentials: Mutex::new(vec![]), hits: Mutex::new(0) });
        let target = spy_server(spy.clone()).await;
        for code in [
            StatusCode::MOVED_PERMANENTLY,
            StatusCode::FOUND,
            StatusCode::TEMPORARY_REDIRECT,
            StatusCode::PERMANENT_REDIRECT,
        ] {
            let base = redirect_server(code, &target).await;
            let Err(err) = call(&hub, &base, Method::POST, "/v1/host/register", Some(&json!({"ping": true}))).await
            else {
                panic!("{code} deve ser recusado");
            };
            assert!(err.to_string().contains("redirect"), "{code}: erro inesperado: {err}");
            let res = crate::http_cloud()
                .post(format!("{base}/v1/host/register"))
                .send()
                .await
                .unwrap_or_else(|e| panic!("{code}: pedido falhou: {e}"));
            assert_eq!(res.status(), code, "{code}: o cliente não segue o redirect");
        }
        assert_eq!(*spy.hits.lock().unwrap(), 0, "o destino do redirect nunca foi alcançado");
        assert!(spy.credentials.lock().unwrap().is_empty(), "nenhum cabeçalho de credencial vazou");
    }

    #[tokio::test]
    async fn cloud_calls_accept_a_normal_success() {
        let hub = temp_hub();
        let computer_id = hub.identity.computer_id();
        let app = axum::Router::new().route(
            "/v1/host/register",
            post(move || {
                let computer_id = computer_id.clone();
                async move { axum::Json(json!({"computerId": computer_id})) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        register(&hub, &base).await.unwrap();
        assert!(hub.cloud.status().registered);
    }
}

#[cfg(test)]
mod acs4_scope_tests {
    use super::*;
    #[test]
    fn owner_devices_never_invents_screen_or_delegates_owner() {
        assert_eq!(grantable_scopes(&[Scope::Control, Scope::OwnerDevices]), vec![Scope::Control]);
        assert_eq!(
            grantable_scopes(&[Scope::Control, Scope::Screen, Scope::OwnerDevices]),
            vec![Scope::Control, Scope::Screen]
        );
        assert_eq!(grantable_scopes(&[Scope::OwnerDevices, Scope::Executor]), Vec::<Scope>::new());
    }
}
