// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! The outbound WebSocket to this computer's relay in the Codync cloud (spec §7, §9.6).
//!
//! Every connection runs the same order: register (once per URL) → connect with
//! `Codync-Sig` → pull the cloud state (≤ 10 s; account devices wait for it) → publish the
//! signed ACL → `acl.ok` → `ready`. Then device links are multiplexed onto `channel::run`
//! (the relay only ever sees ciphertext), queued mailbox sends are delivered in order and
//! acknowledged, and every change to the device table republishes the ACL. Once ready, the
//! routine webhooks are registered (and again whenever they change) and public webhook
//! deliveries arrive one at a time for an ack (§7.8). A reconnect drops all link state.

use crate::LockExt;
use crate::api;
use crate::api::devices;
use crate::fork::bootstrap::sync::{Pulls, pull_state};
use crate::hub::Hub;
use crate::remote::channel::{self, ChannelKind, Out, Transport};
use crate::remote::cloud;
use crate::remote::crypto::{self, b64};
use crate::store::DeviceSource;
use anyhow::{Context, Result, anyhow, bail};
use futures::{SinkExt as _, StreamExt as _};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep, timeout};
use tokio_stream::StreamMap;
use tokio_tungstenite::tungstenite::{self, Message, client::IntoClientRequest as _};

const PATH: &str = "/v1/relay/host?v=1";
/// Must be exactly this string: the relay answers it without waking up.
const PING: &str = r#"{"t":"ping"}"#;
const PING_EVERY: Duration = Duration::from_secs(30);
/// Nothing heard for this long (not even a pong): the socket is dead.
const SILENT: Duration = Duration::from_secs(90);
const STATE_WAIT: Duration = Duration::from_secs(10);
const STATE_EVERY: Duration = Duration::from_secs(5 * 60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const STABLE: Duration = Duration::from_secs(60);
const REPLACED_WAIT: Duration = Duration::from_secs(5 * 60);
/// A device's messages waiting for its channel; the relay already rate-limits devices.
const LINK_BUFFER: usize = 256;
/// A mailbox send older than this is refused (TTL 24 h + clock slack).
const MAILBOX_MAX_AGE_MS: i64 = 25 * 60 * 60 * 1000;
const REPLACED: u16 = 4009;
const RATE_LIMITED: u16 = 4008;

/// Keeps this computer on the relay while the cloud is on; restarts when it's reconfigured.
pub async fn run(hub: Arc<Hub>) {
    // C3b.1: origem durável nunca entra em register/hooks/mailbox/ACS4 legado.
    if hub.store.personal_configured().unwrap_or(true) {
        if hub.personal.configured() {
            crate::fork::personal_bootstrap::relay::run(hub).await;
        }
        return;
    }
    let mut config = hub.cloud.config.subscribe();
    loop {
        config.mark_unchanged();
        let url = cloud::url(&hub.store);
        cloud::update_status(&hub, |s| {
            s.enabled = url.is_some();
            s.url.clone_from(&url);
            s.approval = cloud::approval(&hub.store);
            s.registered = false;
            s.connected = false;
            if url.is_none() {
                s.owner = None;
                s.last_error = None;
            }
        });
        let restart = if let Some(url) = url {
            tokio::select! {
                () = supervise(&hub, &url) => Ok(()),
                r = config.changed() => r,
            }
        } else {
            // No cloud: account devices can't be renewed, so they aren't honored either.
            hub.cloud_synced.store(false, Ordering::Release);
            config.changed().await
        };
        hub.cloud_synced.store(false, Ordering::Release);
        if restart.is_err() {
            return;
        }
    }
}

/// Exponential backoff with full jitter.
struct Backoff {
    next: Duration,
    max: Duration,
}

impl Backoff {
    fn new(max: Duration) -> Self {
        Self { next: Duration::from_secs(1), max }
    }

    fn wait(&mut self) -> Duration {
        let cap = self.next;
        self.next = (self.next * 2).min(self.max);
        let r = u64::from_le_bytes(crypto::random());
        let cap_ms = u64::try_from(cap.as_millis()).unwrap_or(u64::MAX);
        Duration::from_millis(r % (cap_ms + 1)).max(Duration::from_millis(250))
    }

    fn reset(&mut self) {
        self.next = Duration::from_secs(1);
    }
}

async fn supervise(hub: &Arc<Hub>, url: &str) {
    let mut register_wait = Backoff::new(Duration::from_secs(30 * 60));
    let mut wait = Backoff::new(Duration::from_secs(60));
    let mut registered = false;
    loop {
        if !registered {
            if let Err(e) = cloud::register(hub, url).await {
                tracing::warn!(error = format!("{e:#}"), "couldn't register with the Donnabot cloud");
                cloud::update_status(hub, |s| s.last_error = Some(format!("{e:#}")));
                sleep(register_wait.wait()).await;
                continue;
            }
            registered = true;
            register_wait.reset();
        }
        let started = Instant::now();
        let outcome = session(hub, url).await;
        cloud::update_status(hub, |s| s.connected = false);
        match outcome {
            Ok(Some(REPLACED)) => {
                tracing::error!(
                    computer_id = hub.identity.computer_id(),
                    "another host took over this computer's relay; was identity.json copied to another machine?"
                );
                cloud::update_status(hub, |s| s.last_error = Some("Another computer is using this identity.".into()));
                sleep(REPLACED_WAIT).await;
                continue;
            }
            Ok(code) => tracing::info!(code, "relay connection closed"),
            Err(e) => {
                // The relay refused the upgrade: maybe the cloud forgot us; register again.
                if let Some(tungstenite::Error::Http(res)) = e.downcast_ref::<tungstenite::Error>() {
                    registered = !res.status().is_client_error();
                }
                tracing::warn!(
                    close_kind = "failure",
                    connection_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                    error = format!("{e:#}"),
                    "relay connection failed"
                );
                cloud::update_status(hub, |s| s.last_error = Some(format!("{e:#}")));
            }
        }
        if started.elapsed() >= STABLE {
            wait.reset();
        }
        sleep(wait.wait()).await;
    }
}

/// `https://x` → `wss://x/v1/relay/host?v=1`.
fn relay_url(base: &str) -> Result<String> {
    let rest = base
        .strip_prefix("https://")
        .map(|r| format!("wss://{r}"))
        .or_else(|| base.strip_prefix("http://").map(|r| format!("ws://{r}")))
        .ok_or_else(|| anyhow!("unsupported cloud URL"))?;
    Ok(format!("{rest}{PATH}"))
}

type Sink = futures::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    Message,
>;

/// One relay connection; returns the close code the relay sent, if any.
async fn session(hub: &Arc<Hub>, base: &str) -> Result<Option<u16>> {
    let mut req = relay_url(base)?.into_client_request()?;
    let sig = hub.identity.sign_request("GET", &cloud::authority(base)?, PATH, b"");
    req.headers_mut().insert("Codync-Sig", sig.parse()?);
    let (ws, _) = timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(req))
        .await
        .map_err(|_| anyhow!("timed out connecting to the relay"))??;
    // Account devices wait for this connection's state pull (§4.3).
    hub.cloud_synced.store(false, Ordering::Release);
    cloud::update_status(hub, |s| {
        s.connected = true;
        s.last_error = None;
    });
    tracing::info!(computer_id = hub.identity.computer_id(), "connected to the relay");
    let (sink, mut stream) = ws.split();
    let mut s = Session {
        hub: hub.clone(),
        base: base.to_owned(),
        sink,
        links: HashMap::new(),
        outs: StreamMap::new(),
        portas: HashMap::new(),
        pulls: Pulls::default(),
        held: None,
        ready: false,
        retried: false,
    };
    if timeout(STATE_WAIT, pull_state(hub.clone(), base.to_owned())).await.is_err() {
        tracing::warn!("the cloud state didn't arrive in time; account devices wait for it");
    }
    // A timed-out/failed initial upload must also enter the short retry path.
    if !hub.cloud_synced.load(Ordering::Acquire) {
        s.pulls.retry_initial(hub, base);
    }
    let mut auth = hub.auth.subscribe();
    auth.mark_unchanged();
    let mut hooks = hub.routines.subscribe_hooks();
    hooks.mark_unchanged();
    s.publish_acl().await?;
    let mut ping = tokio::time::interval(PING_EVERY);
    ping.reset();
    let mut poll = tokio::time::interval(STATE_EVERY);
    poll.reset();
    let mut heard = Instant::now();
    loop {
        tokio::select! {
            // Links first: a pairing answer and its `4100` go out before the ACL that ends the offer.
            biased;
            Some((link, out)) = s.outs.next() => s.link_out(link, out).await?,
            msg = stream.next() => {
                heard = Instant::now();
                match msg {
                    None => {
                        tracing::warn!(close_kind = "end_of_stream", "relay socket encerrado");
                        return Ok(None);
                    }
                    Some(Err(e)) => {
                        tracing::warn!(close_kind = "read_error", error = %e, "relay socket encerrado");
                        return Err(e).context("relay socket");
                    }
                    Some(Ok(Message::Text(t))) => s.on_message(t.as_str()).await?,
                    Some(Ok(Message::Close(frame))) => {
                        let code = frame.as_ref().map(|f| u16::from(f.code));
                        let reason = frame.as_ref().map_or("sem_motivo", |f| relay_close_reason(&f.reason));
                        tracing::warn!(close_kind = "close_frame", ?code, reason, "relay socket encerrado");
                        return Ok(code);
                    }
                    Some(Ok(_)) => {}
                }
            }
            changed = auth.changed() => {
                if changed.is_err() {
                    return Ok(None);
                }
                s.publish_acl().await?;
            }
            changed = hooks.changed() => {
                if changed.is_err() {
                    return Ok(None);
                }
                if s.ready {
                    s.publish_hooks().await?;
                }
            }
            _ = ping.tick() => {
                if heard.elapsed() > SILENT {
                    tracing::warn!(close_kind = "silence_timeout", silence_ms = u64::try_from(heard.elapsed().as_millis()).unwrap_or(u64::MAX),
                        "relay socket encerrado");
                    bail!("the relay stopped answering");
                }
                s.sink.send(Message::Text(PING.into())).await?;
                // The state pull failed on this connection: account devices stay out until one works.
                if !hub.cloud_synced.load(Ordering::Acquire) && s.pulls.is_empty() {
                    s.pull();
                }
            }
            // Also without an owner: a claim whose `cloud.changed` never arrived is picked up here.
            _ = poll.tick() => s.pull(),
            Some(()) = s.pulls.join_next(hub, base) => s.deliver_held().await?,
        }
    }
}

// O motivo é remoto; registre apenas valores do protocolo para não gravar dados arbitrários.
fn relay_close_reason(reason: &str) -> &str {
    match reason {
        "" => "sem_motivo",
        "timeout" | "replaced" | "deploymentMismatch" | "protocolError" | "rateLimited" | "forbidden"
        | "grantUnknown" | "revoked" | "bootstrapExpired" | "pairingExpired" => reason,
        _ => "outro",
    }
}

struct Session {
    hub: Arc<Hub>,
    base: String,
    sink: Sink,
    /// Device messages into each link's channel.
    links: HashMap<String, mpsc::Sender<Value>>,
    /// What each link's channel sends back.
    outs: StreamMap<String, std::pin::Pin<Box<dyn futures::Stream<Item = Out> + Send>>>,
    /// Each link's workspace gate, also checked when the relay closes a link itself.
    portas: HashMap<String, channel::Porta>,
    /// State pulls in flight; dropped (aborted) with the connection.
    pulls: Pulls,
    /// An account device's queued send that arrived before this connection's state pull
    /// succeeded. The relay sends the next item only after an ack, so there is at most one.
    held: Option<Value>,
    ready: bool,
    /// The last ACL was refused once already.
    retried: bool,
}

impl Session {
    async fn send(&mut self, v: &Value) -> Result<()> {
        self.sink.send(Message::Text(v.to_string().into())).await.context("relay socket")
    }

    fn pull(&mut self) {
        self.pulls.request(&self.hub, &self.base);
    }

    /// Delivers the held mailbox item once a state pull has succeeded.
    async fn deliver_held(&mut self) -> Result<()> {
        if !self.hub.cloud_synced.load(Ordering::Acquire) {
            return Ok(());
        }
        let Some(item) = self.held.take() else { return Ok(()) };
        let ack = mailbox_item(&self.hub, &item).await;
        self.send(&ack).await
    }

    async fn publish_acl(&mut self) -> Result<()> {
        let msg = signed_acl(&self.hub)?;
        self.send(&msg).await
    }

    /// The whole webhook set; the relay replaces what it had.
    async fn publish_hooks(&mut self) -> Result<()> {
        let hooks = self.hub.routines.hooks();
        self.send(&json!({"t": "hooks", "hooks": hooks})).await
    }

    async fn link_out(&mut self, link: String, out: Out) -> Result<()> {
        // Only the channel's durable ACS4 commit wakes receipt publication.
        let committed = matches!(&out, Out::Close(4100, "bootstrapCommitted"));
        let msg = match out {
            Out::Msg(m) => json!({"t": "data", "link": link, "m": m}),
            Out::Close(code, reason) => {
                self.links.remove(&link);
                self.outs.remove(&link);
                self.portas.remove(&link);
                json!({"t": "close", "link": link, "code": code, "reason": reason})
            }
        };
        self.send(&msg).await?;
        if committed {
            self.pull();
        }
        Ok(())
    }

    async fn close_link(&mut self, link: &str, code: u16, reason: &str) -> Result<()> {
        self.links.remove(link);
        self.outs.remove(link);
        if let Some(t) = self.portas.remove(link).and_then(|porta| porta.terminal()) {
            self.send(&json!({"t": "data", "link": link, "m": t.reject})).await?;
            return self.send(&json!({"t": "close", "link": link, "code": t.close, "reason": "rejected"})).await;
        }
        self.send(&json!({"t": "close", "link": link, "code": code, "reason": reason})).await
    }

    async fn on_message(&mut self, text: &str) -> Result<()> {
        let v: Value = serde_json::from_str(text).unwrap_or_default();
        match v["t"].as_str().unwrap_or_default() {
            "open" => {
                let (Some(link), Some(dk)) = (v["link"].as_str(), v["dk"].as_str()) else { return Ok(()) };
                let kind = match v.get("purpose") {
                    None => ChannelKind::Relay {
                        link: link.to_owned(),
                        dk: dk.to_owned(),
                        pair: v["pair"].as_bool().unwrap_or(false),
                    },
                    Some(purpose) if purpose == "bootstrap" && v["pair"] == true => {
                        let Some(id) = v["bootstrapId"].as_str().filter(|id| !id.is_empty()) else { return Ok(()) };
                        ChannelKind::BootstrapRelay {
                            link: link.to_owned(),
                            dk: dk.to_owned(),
                            bootstrap_id: id.to_owned(),
                        }
                    }
                    Some(_) => return Ok(()),
                };
                let (in_tx, incoming) = mpsc::channel(LINK_BUFFER);
                let (outgoing, out_rx) = mpsc::channel(64);
                let porta = channel::Porta::default();
                self.links.insert(link.to_owned(), in_tx);
                self.outs.insert(link.to_owned(), Box::pin(porta.clone().filtrar(out_rx)));
                self.portas.insert(link.to_owned(), porta.clone());
                tokio::spawn(channel::run(self.hub.clone(), Transport { incoming, outgoing, porta }, kind));
            }
            "data" => {
                let link = v["link"].as_str().unwrap_or_default();
                let Some(tx) = self.links.get(link) else { return Ok(()) };
                let m = if v["m"].is_object() { v["m"].clone() } else { Value::Null };
                match tx.try_send(m) {
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        tracing::warn!(link, "device link overflowed; closing it");
                        self.close_link(link, RATE_LIMITED, "rateLimited").await?;
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        self.links.remove(link);
                    }
                }
            }
            // The device is gone: its channel ends when its input does.
            "close" => {
                if let Some(link) = v["link"].as_str() {
                    self.links.remove(link);
                    self.outs.remove(link);
                    self.portas.remove(link);
                }
            }
            "mbox.item" => {
                let from = v["from"].as_str().unwrap_or_default();
                let account = self.hub.store.device(from).is_some_and(|d| d.source == DeviceSource::Account);
                if account && !self.hub.cloud_synced.load(Ordering::Acquire) {
                    // Refusing it would drop it for good; unacknowledged, it waits for the pull
                    // (or goes back to the queue if this connection ends).
                    self.held = Some(v);
                    if self.pulls.is_empty() {
                        self.pull();
                    }
                    return Ok(());
                }
                let ack = mailbox_item(&self.hub, &v).await;
                self.send(&ack).await?;
            }
            "hook.sealed" => {
                let ack = crate::fork::webhook::receive(&self.hub, &v);
                self.send(&ack).await?;
            }
            "cloud.changed" => self.pull(),
            "acl.ok" => {
                self.retried = false;
                if !self.ready {
                    self.ready = true;
                    self.send(&json!({"t": "ready"})).await?;
                    self.send(&json!({"t": "hook.capability", "version": "sealed-hook-v1"})).await?;
                    self.publish_hooks().await?;
                    tracing::info!(computer_id = self.hub.identity.computer_id(), "relay ready");
                }
            }
            "error" => {
                let code = v["code"].as_str().unwrap_or_default();
                if code == "badHooks" {
                    tracing::error!("the relay refused this computer's webhook list");
                }
                if !matches!(code, "staleVersion" | "badAcl") {
                    return Ok(());
                }
                if self.retried {
                    bail!("the relay refused this computer's ACL ({code})");
                }
                // The relay has a newer version than this database (deleted or restored): jump past it.
                self.retried = true;
                bump_acl_ver(&self.hub, v["ver"].as_i64().unwrap_or(0))?;
                self.publish_acl().await?;
            }
            _ => {}
        }
        Ok(())
    }
}

// MARK: ACL (§4.3)

fn bump_acl_ver(hub: &Hub, at_least: i64) -> Result<()> {
    let ver = hub.store.kv_get("acl_ver").and_then(|v| v.parse().ok()).unwrap_or(0_i64).max(at_least);
    hub.store.kv_set("acl_ver", &ver.to_string())
}

/// The next ACL (version bumped), signed: `{"t":"acl","d","sig"}`.
fn signed_acl(hub: &Hub) -> Result<Value> {
    let ver = hub.store.kv_get("acl_ver").and_then(|v| v.parse().ok()).unwrap_or(0_i64) + 1;
    hub.store.kv_set("acl_ver", &ver.to_string())?;
    let devices: Vec<Value> =
        hub.store.devices()?.into_iter().map(|d| json!({"dk": d.key, "grant": d.grant_id})).collect();
    let offers: Vec<Value> =
        hub.pairing.locked().offers().into_iter().map(|(id, exp)| json!({"id": id, "exp": exp})).collect();
    let mut acl = json!({
        "v": 1,
        "computerId": hub.identity.computer_id(),
        "ver": ver,
        "devices": devices,
        "offers": offers,
    });
    if let Ok(now) = crate::fork::bootstrap::clock::now()
        && let Ok(Some(g)) = hub.store.bootstrap_current(now)
        && g.14 == hub.identity.computer_id()
        && g.15 == hub.identity.sign_pub_b64()
        && g.16 == hub.identity.box_pub_b64()
    {
        acl["bootstrap"] = json!({"dk":g.12,"bootstrapId":g.1,"hubGeneration":g.4,"grantHash":crate::fork::bootstrap::codec::hash(&g)?,"exp":g.19});
    }
    let acl = acl.to_string();
    Ok(json!({"t": "acl", "d": b64(acl.as_bytes()), "sig": b64(&hub.identity.sign(acl.as_bytes()))}))
}

// MARK: mailbox (§6.4, §7.7)

/// Why a queued send was refused (`mbox.ack.code`).
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum MailboxFailure {
    Unauthorized,
    UnknownBot,
    Invalid,
}

/// Delivers one queued send and says how it went: `{"t":"mbox.ack","seq","ok","code"?}`.
async fn mailbox_item(hub: &Arc<Hub>, item: &Value) -> Value {
    let seq = item["seq"].clone();
    match deliver(hub, item).await {
        Ok(()) => json!({"t": "mbox.ack", "seq": seq, "ok": true}),
        Err(code) => {
            tracing::info!(?code, "refused a queued message");
            json!({"t": "mbox.ack", "seq": seq, "ok": false, "code": code})
        }
    }
}

async fn deliver(hub: &Arc<Hub>, item: &Value) -> Result<(), MailboxFailure> {
    use MailboxFailure::{Invalid, Unauthorized, UnknownBot};
    // A caixa de recados não leva sessão Clerk nem prova de posse: num Hub com vínculo (ou com o
    // vínculo ilegível), nenhum recado guardado vira pedido.
    if !matches!(crate::fork::sessao_clerk::exigencia(hub), crate::fork::sessao_clerk::Exigencia::Livre) {
        return Err(Unauthorized);
    }
    let from = item["from"].as_str().unwrap_or_default();
    let (_, caller) = crate::fork::autoridade::autenticar(hub, from).map_err(|_| Unauthorized)?;
    let dk = crypto::unb64_n::<32>(from).map_err(|_| Invalid)?;
    let nonce = item["nonce"].as_str().filter(|n| !n.is_empty()).ok_or(Invalid)?;
    let blob = item["d"].as_str().and_then(|d| crypto::unb64(d).ok()).ok_or(Invalid)?;
    let plain = crypto::open_mailbox(&hub.identity.box_secret, &hub.identity.cid_raw(), &dk, nonce, &blob)
        .map_err(|_| Invalid)?;
    let inner: Value = serde_json::from_slice(&plain).map_err(|_| Invalid)?;
    let b = &inner["b"];
    let fresh = inner["ts"].as_i64().is_some_and(|ts| crate::store::now_ms() - ts <= MAILBOX_MAX_AGE_MS);
    if inner["m"] != "send" || b["clientNonce"].as_str() != Some(nonce) || !fresh {
        return Err(Invalid);
    }
    let bot = b["botId"].as_str().ok_or(Invalid)?;
    if hub.store.bot(bot).ok().flatten().is_none_or(|r| r.deleted) {
        return Err(UnknownBot);
    }
    // `send` skips a clientNonce it already has: a redelivery never runs twice.
    api::dispatch(hub, &caller, "send", b.clone())
        .await
        .map(drop)
        .map_err(|e| if e.is::<devices::Forbidden>() { Unauthorized } else { Invalid })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    #[test]
    fn close_reason_only_keeps_known_protocol_values() {
        assert_eq!(relay_close_reason("timeout"), "timeout");
        assert_eq!(relay_close_reason("forbidden"), "forbidden");
        assert_eq!(relay_close_reason(""), "sem_motivo");
        assert_eq!(relay_close_reason("arbitrary remote text"), "outro");
    }

    use super::*;
    use crate::remote::identity::Identity;
    use crate::store::{BotConfig, Device, DeviceSource, Scope, Store, now_ms};
    use axum::extract::ws::{Message as AxMessage, WebSocket, WebSocketUpgrade};
    use axum::extract::{Request, State};
    use axum::routing::{any, get, post};
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    use ed25519_dalek::SigningKey;
    use hkdf::Hkdf;
    use sha2::Sha256;
    use x25519_dalek::StaticSecret;

    /// Pasta temporária do teste, apagada quando o guarda sai de escopo.
    struct TestDir(std::path::PathBuf);

    impl TestDir {
        fn new(prefix: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> String {
            self.0.to_string_lossy().into_owned()
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Hub com banco próprio; a pasta dele é apagada junto com o teste.
    fn temp_hub() -> (Arc<Hub>, TestDir) {
        let dir = TestDir::new("codync-relay");
        let store = Store::open(&dir.0.join("t.db")).unwrap();
        let hub = Hub::new(store, "host".into(), Identity::load_or_create(&dir.0).unwrap(), "token".into(), 0);
        (hub, dir)
    }

    /// Pasta de trabalho própria do bot. Não pode ser a `temp_dir` inteira: nos testes os dados do Hub ficam
    /// dentro dela (`codync-unit-<pid>`), e a validação de pasta (#42) recusa esse caso.
    fn temp_cwd() -> TestDir {
        TestDir::new("codync-relay-cwd")
    }

    /// What the fake cloud saw and does.
    #[derive(Default)]
    struct Fake {
        /// ACL version the "DO" already stored (a previous database).
        stored_ver: std::sync::Mutex<i64>,
        /// Messages from the host, in order.
        from_host: std::sync::Mutex<Vec<Value>>,
        /// Messages to push to the host after `ready`.
        script: std::sync::Mutex<Vec<Value>>,
        state: std::sync::Mutex<Value>,
        /// State pulls to fail (500) before answering.
        state_failures: std::sync::Mutex<u32>,
        signatures_ok: std::sync::Mutex<Vec<bool>>,
        receipts: std::sync::Mutex<Vec<Value>>,
    }

    fn check_sig(req: &Request, sign_pub: &[u8; 32], authority: &str) -> bool {
        let h = req.headers().get("codync-sig").and_then(|v| v.to_str().ok()).unwrap_or_default();
        let field = |k: &str| h.split(',').find_map(|p| p.strip_prefix(&format!("{k}="))).unwrap_or_default();
        let ts: i64 = field("ts").parse().unwrap_or(0);
        let path = req.uri().path_and_query().unwrap().as_str();
        let input = crypto::request_sig_input(req.method().as_str(), authority, path, ts, field("nonce"), b"");
        field("kid") == b64(sign_pub)
            && crypto::verify(sign_pub, input.as_bytes(), &crypto::unb64_n(field("sig")).unwrap()).is_ok()
    }

    async fn fake_cloud(fake: Arc<Fake>, hub: Arc<Hub>) -> String {
        use axum::response::IntoResponse as _;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let authority = listener.local_addr().unwrap().to_string();
        let sign_pub = hub.identity.sign_pub();
        let cid = hub.identity.computer_id();
        let a1 = authority.clone();
        let app = axum::Router::new()
            .route(
                "/v1/host/register",
                post(move |State(f): State<Arc<Fake>>, req: Request| async move {
                    let ok = req.headers().contains_key("codync-sig");
                    f.signatures_ok.lock().unwrap().push(ok);
                    axum::Json(json!({"computerId": cid, "owned": false}))
                }),
            )
            .route(
                "/v1/host/state",
                get(move |State(f): State<Arc<Fake>>, req: Request| async move {
                    f.signatures_ok.lock().unwrap().push(check_sig(&req, &sign_pub, &a1));
                    let mut failures = f.state_failures.lock().unwrap();
                    if *failures > 0 {
                        *failures -= 1;
                        return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({}))).into_response();
                    }
                    axum::Json(f.state.lock().unwrap().clone()).into_response()
                }),
            )
            .route(
                "/v1/host/bootstrap-receipts",
                post(move |State(f): State<Arc<Fake>>, axum::Json(body): axum::Json<Value>| async move {
                    let mut receipts = f.receipts.lock().unwrap();
                    receipts.push(body.clone());
                    if receipts.len() == 1 {
                        return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({})));
                    }
                    let r: crate::fork::bootstrap::codec::Receipt = crate::fork::bootstrap::codec::decode(
                        &serde_json::to_vec(&body["receipt"]).unwrap(),
                        &sign_pub,
                    ).unwrap();
                    *f.state.lock().unwrap() = json!({"owner":{"userId":r.6}, "requests":[],
                        "grants":[{"grantId":r.11,"deviceKey":r.8}],
                        "bootstrapPublication":{"bootstrapId":r.1,"hubGeneration":r.2,"grantId":r.11,
                            "deviceKey":r.8,"ownerUserId":r.6,"hashHubReceipt":crate::fork::bootstrap::codec::hash(&r).unwrap()}});
                    (axum::http::StatusCode::OK, axum::Json(json!({})))
                }),
            )
            .route(
                "/v1/host/bootstrap-activations",
                post(|| async { axum::Json(json!({})) }),
            )
            .route(
                "/v1/relay/host",
                any(move |State(f): State<Arc<Fake>>, ws: WebSocketUpgrade, req: Request| async move {
                    let _ = req;
                    ws.on_upgrade(move |socket| fake_do(f, socket))
                }),
            )
            .with_state(fake);
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{authority}")
    }

    async fn fake_do(f: Arc<Fake>, mut ws: WebSocket) {
        while let Some(Ok(AxMessage::Text(t))) = ws.recv().await {
            let v: Value = serde_json::from_str(t.as_str()).unwrap();
            f.from_host.lock().unwrap().push(v.clone());
            let reply = match v["t"].as_str().unwrap() {
                "acl" => {
                    let d = crypto::unb64(v["d"].as_str().unwrap()).unwrap();
                    let acl: Value = serde_json::from_slice(&d).unwrap();
                    let ver = acl["ver"].as_i64().unwrap();
                    let mut stored = f.stored_ver.lock().unwrap();
                    if ver <= *stored {
                        json!({"t": "error", "code": "staleVersion", "ver": *stored})
                    } else {
                        *stored = ver;
                        json!({"t": "acl.ok", "ver": ver})
                    }
                }
                "ready" => {
                    let script: Vec<Value> = f.script.lock().unwrap().drain(..).collect();
                    for m in script {
                        ws.send(AxMessage::Text(m.to_string().into())).await.unwrap();
                    }
                    continue;
                }
                _ => continue,
            };
            ws.send(AxMessage::Text(reply.to_string().into())).await.unwrap();
        }
    }

    /// Device side of §6.4, for the test.
    fn seal_mailbox(hub: &Hub, device: &SigningKey, nonce: &str, plain: &[u8]) -> String {
        let cid = hub.identity.cid_raw();
        let dk = device.verifying_key().to_bytes();
        let eph = StaticSecret::from(crypto::random::<32>());
        let epk = crypto::x25519_pub(&eph);
        let ss = crypto::x25519(&eph, &hub.identity.box_pub()).unwrap();
        let salt = [b"codync/mbox/v1".as_slice(), &cid, &dk, &epk].concat();
        let mut key = [0u8; 32];
        Hkdf::<Sha256>::new(Some(&salt), &ss).expand(b"codync/mbox-key/v1", &mut key).unwrap();
        let aad = [dk.as_slice(), nonce.as_bytes()].concat();
        let ct = chacha20poly1305::ChaCha20Poly1305::new(&key.into())
            .encrypt(&[0u8; 12].into(), Payload { msg: plain, aad: &aad })
            .unwrap();
        let sig = crypto::sign(device, &[b"codync/mbox/v1".as_slice(), &cid, &epk, &ct].concat());
        b64(&[epk.as_slice(), &sig, &ct].concat())
    }

    async fn wait_for(f: &Fake, pred: impl Fn(&[Value]) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while !pred(&f.from_host.lock().unwrap()) {
            assert!(Instant::now() < deadline, "timed out; host sent {:?}", f.from_host.lock().unwrap());
            sleep(Duration::from_millis(20)).await;
        }
    }

    #[tokio::test]
    async fn committed_bootstrap_close_pulls_state_without_waiting_for_poll() {
        let (hub, _dir) = temp_hub();
        let fake = Arc::new(Fake::default());
        *fake.state.lock().unwrap() = json!({"owner": null, "requests": [], "grants": []});
        let base = fake_cloud(fake.clone(), hub.clone()).await;
        let (ws, _) = tokio_tungstenite::connect_async(relay_url(&base).unwrap()).await.unwrap();
        let (sink, _incoming) = ws.split();
        let mut session = Session {
            hub: hub.clone(),
            base,
            sink,
            links: HashMap::new(),
            outs: StreamMap::new(),
            portas: HashMap::new(),
            pulls: Pulls::default(),
            held: None,
            ready: true,
            retried: false,
        };
        // A generic successful pairing close is not a committed ACS4 receipt.
        session.link_out("ordinary".into(), Out::Close(4100, "paired")).await.unwrap();
        assert!(session.pulls.is_empty());
        session.link_out("bootstrap".into(), Out::Close(4100, "bootstrapCommitted")).await.unwrap();
        let job = timeout(Duration::from_secs(60), session.pulls.join_next(&hub, &session.base)).await.unwrap();
        job.expect("the durable receipt must wake a pull immediately");
        assert!(hub.cloud_synced.load(Ordering::Acquire));
        assert_eq!(*fake.signatures_ok.lock().unwrap(), vec![true]);
        wait_for(&fake, |messages| messages.iter().any(|v| v["t"] == "close" && v["reason"] == "bootstrapCommitted"))
            .await;
    }

    #[tokio::test]
    async fn link_with_a_full_queue_sends_only_the_terminal_after_a_workspace_bump() {
        let (hub, _dir) = temp_hub();
        let device = |key: &str| Device {
            key: key.into(),
            name: "Fixture".into(),
            platform: "ios".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![Scope::Control],
            lease_until: None,
            created_at: now_ms(),
            last_seen_at: None,
        };
        hub.store.put_device(&device("stays")).unwrap();
        hub.store.put_device(&device("leaves")).unwrap();
        let fake = Arc::new(Fake::default());
        let base = fake_cloud(fake.clone(), hub.clone()).await;
        let (ws, _) = tokio_tungstenite::connect_async(relay_url(&base).unwrap()).await.unwrap();
        let (sink, _incoming) = ws.split();
        let mut s = Session {
            hub: hub.clone(),
            base,
            sink,
            links: HashMap::new(),
            outs: StreamMap::new(),
            portas: HashMap::new(),
            pulls: Pulls::default(),
            held: None,
            ready: true,
            retried: false,
        };
        // The link's real transport queue (64), full of frames the writer already released.
        let (outgoing, out_rx) = mpsc::channel::<Out>(64);
        for n in 0..63 {
            outgoing.send(Out::Msg(json!({"t": "f", "n": n}))).await.unwrap();
        }
        outgoing.send(Out::Close(4011, "subscription authorization ended")).await.unwrap();
        assert_eq!(outgoing.capacity(), 0);
        let porta = channel::Porta::armada(&hub, "stays");
        s.outs.insert("full".into(), Box::pin(porta.filtrar(out_rx)));
        assert!(hub.revoke_device("leaves").unwrap());
        while let Some((link, out)) = timeout(Duration::from_secs(5), s.outs.next()).await.unwrap() {
            s.link_out(link, out).await.unwrap();
        }
        wait_for(&fake, |messages| messages.iter().any(|v| v["t"] == "close" && v["link"] == "full")).await;
        let sent: Vec<Value> = fake.from_host.lock().unwrap().iter().filter(|v| v["link"] == "full").cloned().collect();
        assert_eq!(sent.len(), 2, "only the terminal leaves: {sent:?}");
        assert_eq!(
            (sent[0]["t"].as_str(), sent[0]["m"]["stableCode"].as_str()),
            (Some("data"), Some("workspace_revoked"))
        );
        assert_eq!((sent[1]["t"].as_str(), sent[1]["code"].as_u64()), (Some("close"), Some(4003)));
    }

    #[tokio::test]
    async fn relay_closing_a_link_itself_sends_the_terminal_only_after_a_workspace_bump() {
        let (hub, _dir) = temp_hub();
        let device = |key: &str| Device {
            key: key.into(),
            name: "Fixture".into(),
            platform: "ios".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![Scope::Control],
            lease_until: None,
            created_at: now_ms(),
            last_seen_at: None,
        };
        hub.store.put_device(&device("stays")).unwrap();
        hub.store.put_device(&device("leaves")).unwrap();
        let fake = Arc::new(Fake::default());
        let base = fake_cloud(fake.clone(), hub.clone()).await;
        let (ws, _) = tokio_tungstenite::connect_async(relay_url(&base).unwrap()).await.unwrap();
        let (sink, _incoming) = ws.split();
        let mut s = Session {
            hub: hub.clone(),
            base,
            sink,
            links: HashMap::new(),
            outs: StreamMap::new(),
            portas: HashMap::new(),
            pulls: Pulls::default(),
            held: None,
            ready: true,
            retried: false,
        };
        for link in ["calm", "bumped"] {
            s.portas.insert(link.into(), channel::Porta::armada(&hub, "stays"));
        }
        s.close_link("calm", RATE_LIMITED, "rateLimited").await.unwrap();
        assert!(hub.revoke_device("leaves").unwrap());
        s.close_link("bumped", RATE_LIMITED, "rateLimited").await.unwrap();
        wait_for(&fake, |messages| messages.iter().any(|v| v["t"] == "close" && v["link"] == "bumped")).await;
        let sent = |link: &str| -> Vec<Value> {
            fake.from_host.lock().unwrap().iter().filter(|v| v["link"] == link).cloned().collect()
        };
        let calm = sent("calm");
        assert_eq!(calm.len(), 1, "{calm:?}");
        assert_eq!((calm[0]["t"].as_str(), calm[0]["code"].as_u64()), (Some("close"), Some(u64::from(RATE_LIMITED))));
        let bumped = sent("bumped");
        assert_eq!(bumped.len(), 2, "{bumped:?}");
        assert_eq!(
            (bumped[0]["t"].as_str(), bumped[0]["m"]["stableCode"].as_str()),
            (Some("data"), Some("workspace_revoked"))
        );
        assert_eq!((bumped[1]["t"].as_str(), bumped[1]["code"].as_u64()), (Some("close"), Some(4003)));
        assert!(s.portas.is_empty());
    }

    #[tokio::test]
    async fn r1_coalesces_burst_and_periodic_pull() {
        let (hub, _dir) = temp_hub();
        let fake = Arc::new(Fake::default());
        *fake.state.lock().unwrap() = json!({"owner":null,"requests":[],"grants":[]});
        let base = fake_cloud(fake.clone(), hub.clone()).await;
        let (ws, _) = tokio_tungstenite::connect_async(relay_url(&base).unwrap()).await.unwrap();
        let (sink, _incoming) = ws.split();
        let mut s = Session {
            hub: hub.clone(),
            base: base.clone(),
            sink,
            links: HashMap::new(),
            outs: StreamMap::new(),
            portas: HashMap::new(),
            pulls: Pulls::default(),
            held: None,
            ready: true,
            retried: false,
        };
        s.pull(); // periodic pull overlaps the commit and a notification burst
        s.link_out("bootstrap".into(), Out::Close(4100, "bootstrapCommitted")).await.unwrap();
        for _ in 0..32 {
            s.on_message(r#"{"t":"cloud.changed"}"#).await.unwrap();
        }
        assert_eq!(s.pulls.len(), 1, "a burst must not enqueue one HTTP pull per event");
        timeout(Duration::from_secs(60), s.pulls.join_next(&hub, &base)).await.unwrap().unwrap();
        // Publication arriving during the first read must survive as one later read.
        *fake.state.lock().unwrap() = json!({"owner":{"userId":"updated"},"requests":[],"grants":[]});
        timeout(Duration::from_secs(60), s.pulls.join_next(&hub, &base)).await.unwrap().unwrap();
        assert!(s.pulls.is_empty());
        assert_eq!(fake.signatures_ok.lock().unwrap().len(), 2);
        assert_eq!(hub.cloud.status().owner.as_ref().unwrap().user_id, "updated");
    }

    fn pending_bootstrap(hub: &Hub, now: i64, ttl: i64) -> Value {
        use crate::fork::bootstrap::codec::{self, Challenge, Grant, Proof};
        use crate::store::bootstrap::TrustedGeneration;
        let issuer = SigningKey::from_bytes(&[1; 32]);
        let device = SigningKey::from_bytes(&[3; 32]);
        let cfg = TrustedGeneration {
            generation: b64(&[4; 32]),
            issuer_key_id: "issuer-1".into(),
            issuer_key: b64(issuer.verifying_key().as_bytes()),
            issuer: "donna".into(),
            deployment_id: "deployment".into(),
            env: "test".into(),
            manifest_hash: "a".repeat(64),
            resource_digest: "b".repeat(64),
            computer_id: hub.identity.computer_id(),
            sign_key: b64(&hub.identity.sign_pub()),
            box_key: b64(&hub.identity.box_pub()),
        };
        let g = Grant(
            "donna/bootstrap-grant/v1".into(),
            "boot".into(),
            "intent".into(),
            cfg.resource_digest.clone(),
            cfg.generation.clone(),
            cfg.issuer_key_id.clone(),
            cfg.issuer.clone(),
            cfg.deployment_id.clone(),
            cfg.env.clone(),
            cfg.manifest_hash.clone(),
            "owner".into(),
            "device".into(),
            b64(device.verifying_key().as_bytes()),
            b64(&[6; 32]),
            cfg.computer_id.clone(),
            cfg.sign_key.clone(),
            cfg.box_key.clone(),
            b64(&[7; 32]),
            now,
            now + ttl,
            vec!["control".into(), "owner.devices".into()],
        );
        hub.store.bootstrap_install_generation(&cfg).unwrap();
        hub.store.bootstrap_install_grant(&codec::encode(&g, |b| crypto::sign(&issuer, b)).unwrap(), now).unwrap();
        let cw = hub.store.bootstrap_challenge(&hub.identity.sign, "boot", &g.12, now).unwrap();
        let ch: Challenge = codec::decode(&cw, &hub.identity.sign_pub()).unwrap();
        let proof = Proof(
            "donna/bootstrap-proof/v1".into(),
            g.1.clone(),
            g.4.clone(),
            codec::hash(&g).unwrap(),
            codec::hash(&ch).unwrap(),
        );
        hub.store
            .bootstrap_commit_proof(
                &hub.identity.sign,
                &codec::encode(&proof, |b| crypto::sign(&device, b)).unwrap(),
                now,
            )
            .unwrap();
        hub.store.bootstrap_receipt_envelopes().unwrap().unwrap()
    }

    #[tokio::test]
    async fn r1_retries_original_receipt_before_grant_expiry() {
        let (hub, _dir) = temp_hub();
        let originals = pending_bootstrap(&hub, now_ms(), 300_000);
        let fake = Arc::new(Fake::default());
        *fake.state.lock().unwrap() = json!({"owner":null,"requests":[],"grants":[]});
        let base = fake_cloud(fake.clone(), hub.clone()).await;
        let task = tokio::spawn({
            let hub = hub.clone();
            async move { session(&hub, &base).await }
        });
        wait_for(&fake, |m| m.iter().any(|v| v["t"] == "ready")).await;
        assert!(!hub.cloud_synced.load(Ordering::Acquire), "GET success cannot confirm a pending receipt");
        let deadline = Instant::now() + Duration::from_secs(60);
        while hub.store.bootstrap_needs_publication().unwrap() || !hub.cloud_synced.load(Ordering::Acquire) {
            assert!(
                Instant::now() < deadline,
                "receipt must retry before the five minute poll; receipts={} status={:?} task_finished={}",
                fake.receipts.lock().unwrap().len(),
                hub.cloud.status(),
                task.is_finished()
            );
            sleep(Duration::from_millis(20)).await;
        }
        assert!(hub.cloud_synced.load(Ordering::Acquire));
        assert_eq!(*fake.receipts.lock().unwrap(), vec![originals.clone(), originals]);
        task.abort();
        let _ = task.await;
    }

    #[tokio::test]
    async fn r1_keeps_expired_originals_without_resending_receipt() {
        let now = now_ms();
        let ttl = 300_000; // O POST conserva seu timeout normal; a cerimônia vence só pelo relógio controlado.
        cloud::test_clock::at(now, async {
            let (hub, _dir) = temp_hub();
            let originals = pending_bootstrap(&hub, now, ttl);
            let fake = Arc::new(Fake::default());
            *fake.state.lock().unwrap() = json!({"owner":null,"requests":[],"grants":[]});
            let base = fake_cloud(fake.clone(), hub.clone()).await;
            assert!(!pull_state(hub.clone(), base.clone()).await);
            assert_eq!(*fake.receipts.lock().unwrap(), vec![originals.clone()], "primeiro envio antes da expiração");
            // Vence o mesmo grant assinado, sem esperar relógio de parede nem reconstruir envelopes.
            cloud::test_clock::set(now + ttl + 1);
            assert!(!pull_state(hub.clone(), base).await);
            assert_eq!(*fake.receipts.lock().unwrap(), vec![originals.clone()]);
            assert_eq!(hub.store.bootstrap_receipt_envelopes().unwrap().unwrap(), originals);
            assert!(hub.store.bootstrap_needs_publication().unwrap());
            assert!(hub.cloud.status().last_error.is_some());
            assert!(hub.store.devices().unwrap().is_empty());
            // A lost publication response remains reconcilable even after the ceremony.
            let r: crate::fork::bootstrap::codec::Receipt = crate::fork::bootstrap::codec::decode(
                &serde_json::to_vec(&originals["receipt"]).unwrap(),
                &hub.identity.sign_pub(),
            )
            .unwrap();
            *fake.state.lock().unwrap() = json!({"owner":{"userId":r.6}, "requests":[],
            "grants":[{"grantId":r.11,"deviceKey":r.8}],
            "bootstrapPublication":{"bootstrapId":r.1,"hubGeneration":r.2,"grantId":r.11,
                "deviceKey":r.8,"ownerUserId":r.6,"hashHubReceipt":crate::fork::bootstrap::codec::hash(&r).unwrap()}});
            let base = fake_cloud(fake.clone(), hub.clone()).await;
            assert!(pull_state(hub.clone(), base).await);
            assert_eq!(fake.receipts.lock().unwrap().len(), 1);
            assert!(hub.cloud_synced.load(Ordering::Acquire));
            assert_eq!(hub.store.devices().unwrap().len(), 1);
        })
        .await;
    }

    #[tokio::test]
    async fn hub_com_vinculo_nao_entrega_recado_guardado() {
        let (hub, _dir) = temp_hub();
        let phone = SigningKey::from_bytes(&crypto::random());
        let dk = b64(phone.verifying_key().as_bytes());
        hub.store
            .put_device(&Device {
                key: dk.clone(),
                name: "Phone".into(),
                platform: "ios".into(),
                source: DeviceSource::Local,
                grant_id: None,
                scopes: vec![Scope::Control, Scope::Screen],
                lease_until: None,
                created_at: now_ms(),
                last_seen_at: None,
            })
            .unwrap();
        let cwd = temp_cwd();
        let cfg: BotConfig = serde_json::from_value(
            json!({"id": "", "name": "B", "cwd": cwd.path(), "backend": "custom", "command": "true"}),
        )
        .unwrap();
        let bot_id = hub.create_bot(cfg).unwrap()["id"].as_str().unwrap().to_owned();
        let item = |nonce: &str| {
            let inner =
                json!({"m": "send", "b": {"botId": bot_id, "text": "oi", "clientNonce": nonce}, "ts": now_ms()});
            let blob = seal_mailbox(&hub, &phone, nonce, inner.to_string().as_bytes());
            json!({"t": "mbox.item", "seq": 1, "from": dk, "nonce": nonce, "d": blob, "exp": now_ms() + 1000})
        };
        let entregue = |nonce: &str| {
            hub.store.history(&bot_id, i64::MAX, 50).unwrap().iter().any(|e| e.data["clientNonce"] == nonce)
        };
        // Sem vínculo, o recado é entregue como sempre.
        let livre = "3F2504E0-4F89-11D3-9A0C-0305E82C3301";
        assert_eq!(mailbox_item(&hub, &item(livre)).await["ok"], true);
        assert!(entregue(livre));
        // Com vínculo (ou vínculo ilegível), o mesmo recado válido é recusado antes de virar pedido.
        for vinculo in [crate::fork::sessao_clerk::prova::vinculo_pessoal(), "{\"schema\":7}".to_owned()] {
            crate::fork::sessao_clerk::vinculo::gravar_para_prova(&hub, &vinculo);
            let nonce = uuid::Uuid::new_v4().to_string().to_uppercase();
            let ack = mailbox_item(&hub, &item(&nonce)).await;
            assert_eq!((ack["ok"].as_bool(), ack["code"].as_str()), (Some(false), Some("unauthorized")));
            assert!(!entregue(&nonce));
        }
    }

    #[tokio::test]
    async fn connects_publishes_acl_past_a_stale_version_and_delivers_mail() {
        let (hub, _dir) = temp_hub();
        let fake = Arc::new(Fake::default());
        // The relay remembers version 7 from before this database was deleted.
        *fake.stored_ver.lock().unwrap() = 7;
        let grant_key = SigningKey::from_bytes(&crypto::random());
        let phone = SigningKey::from_bytes(&crypto::random());
        let dk = b64(phone.verifying_key().as_bytes());
        hub.store
            .put_device(&Device {
                key: dk.clone(),
                name: "Phone".into(),
                platform: "ios".into(),
                source: DeviceSource::Local,
                grant_id: None,
                scopes: vec![Scope::Control, Scope::Screen],
                lease_until: None,
                created_at: now_ms(),
                last_seen_at: None,
            })
            .unwrap();
        let injected = b64(grant_key.verifying_key().as_bytes());
        *fake.state.lock().unwrap() = json!({"owner": null, "requests": [],
            "grants": [{"grantId": "grt_x", "deviceKey": injected, "deviceName": "x", "platform": "ios", "scopes": ["control"]}]});

        let cwd = temp_cwd();
        let cfg: BotConfig = serde_json::from_value(
            json!({"id": "", "name": "B", "cwd": cwd.path(), "backend": "custom", "command": "true"}),
        )
        .unwrap();
        let bot = hub.create_bot(cfg).unwrap();
        let bot_id = bot["id"].as_str().unwrap().to_owned();
        let nonce = "3F2504E0-4F89-11D3-9A0C-0305E82C3301";
        let inner = json!({"m": "send", "b": {"botId": bot_id, "text": "hi", "clientNonce": nonce}, "ts": now_ms()});
        let blob = seal_mailbox(&hub, &phone, nonce, inner.to_string().as_bytes());
        let stranger = SigningKey::from_bytes(&crypto::random());
        *fake.script.lock().unwrap() = vec![
            json!({"t": "mbox.item", "seq": 1, "from": dk, "nonce": nonce, "d": blob, "exp": now_ms() + 1000}),
            json!({"t": "mbox.item", "seq": 2, "from": dk, "nonce": nonce, "d": blob, "exp": now_ms() + 1000}),
            json!({"t": "mbox.item", "seq": 3, "from": b64(stranger.verifying_key().as_bytes()), "nonce": nonce, "d": blob, "exp": 0}),
            json!({"t": "open", "link": "L1", "dk": b64(stranger.verifying_key().as_bytes()), "pair": false}),
            json!({"t": "data", "link": "L1", "m": {"t": "hello", "v": 1, "dk": dk}}),
        ];

        let base = fake_cloud(fake.clone(), hub.clone()).await;
        hub.store.kv_set("test_cloud_url", &base).unwrap();
        tokio::spawn(run(hub.clone()));

        wait_for(&fake, |m| m.iter().filter(|v| v["t"] == "mbox.ack").count() == 3).await;
        wait_for(&fake, |m| m.iter().any(|v| v["t"] == "close" && v["link"] == "L1")).await;
        let sent = fake.from_host.lock().unwrap().clone();
        let kinds: Vec<&str> = sent.iter().map(|v| v["t"].as_str().unwrap()).collect();
        assert_eq!(&kinds[..3], ["acl", "acl", "ready"], "stale ACL is re-signed once, then ready");
        let acl: Value = serde_json::from_slice(&crypto::unb64(sent[1]["d"].as_str().unwrap()).unwrap()).unwrap();
        assert_eq!(acl["ver"], 8, "max(local, relay) + 1");
        assert_eq!(acl["devices"], json!([{"dk": dk, "grant": null}]), "a grant injected by the cloud isn't listed");
        crypto::verify(
            &hub.identity.sign_pub(),
            &crypto::unb64(sent[1]["d"].as_str().unwrap()).unwrap(),
            &crypto::unb64_n(sent[1]["sig"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        assert!(fake.signatures_ok.lock().unwrap().iter().all(|ok| *ok));
        assert!(hub.cloud_synced.load(Ordering::Acquire), "the state was applied on this connection");

        let acks: Vec<&Value> = sent.iter().filter(|v| v["t"] == "mbox.ack").collect();
        assert_eq!(acks[0]["ok"], true);
        assert_eq!(acks[1]["ok"], true, "a redelivery is acknowledged");
        assert_eq!((acks[2]["ok"].as_bool(), acks[2]["code"].as_str()), (Some(false), Some("unauthorized")));
        let history = hub.store.history(&bot_id, i64::MAX, 50).unwrap();
        assert_eq!(history.iter().filter(|e| e.data["clientNonce"] == nonce).count(), 1, "delivered exactly once");

        let close = sent.iter().find(|v| v["t"] == "close").unwrap();
        assert_eq!(close["code"], 4002, "hello.dk must match the link's open.dk");
        assert!(hub.cloud.status().connected);
    }

    #[tokio::test]
    async fn registers_webhooks_and_refuses_when_chain_authority_is_unavailable() {
        let (hub, _dir) = temp_hub();
        let fake = Arc::new(Fake::default());
        let cwd = temp_cwd();
        let cfg: BotConfig = serde_json::from_value(
            json!({"id": "", "name": "B", "cwd": cwd.path(), "backend": "custom", "command": "true"}),
        )
        .unwrap();
        let bot_id = hub.create_bot(cfg).unwrap()["id"].as_str().unwrap().to_owned();
        let saved = hub
            .routines
            .save(
                &hub,
                &bot_id,
                &json!({"name": "Hook", "instruction": "Report it", "triggers": [{"type": "webhook"}]}),
            )
            .unwrap();
        let id = saved["routine"]["id"].as_str().unwrap().to_owned();
        let key = hub.routines.hooks()[0].key.clone();
        let base = fake_cloud(fake.clone(), hub.clone()).await;
        hub.store.kv_set("test_cloud_url", &base).unwrap();
        let item = |seq: &str, auth: &str, delivery: &str| {
            crate::fork::webhook::fixture_item(&hub, &base, &id, delivery, auth, seq)
        };
        *fake.script.lock().unwrap() =
            vec![item("1", &key, "d1"), item("2", &key, "d1"), item("3", "wrong", "d2"), item("4", &key, "d3")];
        tokio::spawn(run(hub.clone()));

        wait_for(&fake, |m| m.iter().filter(|v| v["t"] == "hook.sealed.ack").count() == 4).await;
        let sent = fake.from_host.lock().unwrap().clone();
        let hooks = sent.iter().find(|v| v["t"] == "hooks").unwrap();
        assert_eq!(hooks["hooks"], json!([{"id": id, "key": key, "enabled": true}]), "registered right after ready");
        let acks: Vec<(Option<bool>, Option<bool>, Option<&str>)> = sent
            .iter()
            .filter(|v| v["t"] == "hook.sealed.ack")
            .map(|v| (v["ok"].as_bool(), v["retry"].as_bool(), v["code"].as_str()))
            .collect();
        assert_eq!(
            acks,
            [
                (Some(false), Some(true), Some("chainUnavailable")),
                (Some(false), Some(true), Some("chainUnavailable")),
                (Some(false), Some(false), Some("unauthorized")),
                (Some(false), Some(true), Some("chainUnavailable")),
            ],
            "chain admission fails closed and a wrong key remains unauthorized"
        );
        let runs = hub.routines.list(&bot_id)["runs"].as_array().unwrap().clone();
        assert_eq!(runs, [] as [Value; 0]);
        let headers = crate::routines::hooks::Headers::from([
            ("authorization".into(), format!("Bearer {key}")),
            ("x-chain-id".into(), "forged".into()),
        ]);
        let blocked =
            hub.routines.receive(&hub, &id, &headers, br#"{"chainId":"forged","depth":0,"count":0}"#).unwrap_err();
        assert_eq!(blocked, crate::routines::hooks::Refused::ChainUnavailable);
        assert_eq!(hub.routines.list(&bot_id)["runs"].as_array().unwrap().len(), 0);
        let history = hub.store.history(&bot_id, i64::MAX, 50).unwrap();
        assert_eq!(
            history
                .iter()
                .filter(|entry| entry.data["text"] == "Webhook bloqueado: controle de cadeia do Hub indisponível.")
                .count(),
            1
        );

        // Pausing changes what the relay should refuse: the set goes out again.
        hub.routines.set_enabled(&hub.store, &bot_id, &id, false).unwrap();
        wait_for(&fake, |m| m.iter().any(|v| v["t"] == "hooks" && v["hooks"][0]["enabled"] == false)).await;
        let creds = hub.routines.credentials(&hub, &bot_id, &id, true).unwrap();
        assert_ne!(creds["key"], key, "rotate replaces the key");
        assert_eq!(creds["url"], format!("{base}/v1/hooks/{}/{id}", hub.identity.computer_id()));
        wait_for(&fake, |m| m.iter().any(|v| v["t"] == "hooks" && v["hooks"][0]["key"] == creds["key"])).await;
    }

    #[tokio::test]
    async fn account_mail_waits_for_a_state_pull_instead_of_failing() {
        let (hub, _dir) = temp_hub();
        let fake = Arc::new(Fake::default());
        let phone = SigningKey::from_bytes(&crypto::random());
        let dk = b64(phone.verifying_key().as_bytes());
        hub.store
            .put_device(&Device {
                key: dk.clone(),
                name: "Phone".into(),
                platform: "ios".into(),
                source: DeviceSource::Account,
                grant_id: Some("grt_a".into()),
                scopes: vec![Scope::Control, Scope::Screen],
                lease_until: Some(now_ms() + 60_000),
                created_at: now_ms(),
                last_seen_at: None,
            })
            .unwrap();
        *fake.state.lock().unwrap() = json!({"owner": {"userId":"dono-teste"}, "requests": [], "grants": [{"grantId": "grt_a", "deviceKey": dk}]});
        // The pull on connect fails: without the fix the item below would be refused for good.
        *fake.state_failures.lock().unwrap() = 1;
        let cwd = temp_cwd();
        let cfg: BotConfig = serde_json::from_value(
            json!({"id": "", "name": "B", "cwd": cwd.path(), "backend": "custom", "command": "true"}),
        )
        .unwrap();
        let bot_id = hub.create_bot(cfg).unwrap()["id"].as_str().unwrap().to_owned();
        let nonce = "9A1B2C3D-4F89-11D3-9A0C-0305E82C3301";
        let inner = json!({"m": "send", "b": {"botId": bot_id, "text": "hi", "clientNonce": nonce}, "ts": now_ms()});
        let blob = seal_mailbox(&hub, &phone, nonce, inner.to_string().as_bytes());
        *fake.script.lock().unwrap() =
            vec![json!({"t": "mbox.item", "seq": 1, "from": dk, "nonce": nonce, "d": blob, "exp": now_ms() + 1000})];

        let base = fake_cloud(fake.clone(), hub.clone()).await;
        hub.store.kv_set("test_cloud_url", &base).unwrap();
        tokio::spawn(run(hub.clone()));

        wait_for(&fake, |m| m.iter().any(|v| v["t"] == "mbox.ack")).await;
        let sent = fake.from_host.lock().unwrap().clone();
        let ack = sent.iter().find(|v| v["t"] == "mbox.ack").unwrap();
        assert_eq!(ack["ok"], true, "delivered once the retried pull confirmed the grant: {ack}");
        assert!(hub.cloud_synced.load(Ordering::Acquire));
    }
}
