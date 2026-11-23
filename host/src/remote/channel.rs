// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! The end-to-end encrypted channel between a device and this host (spec §6.1–§6.6,
//! §9.5), independent of how it travels: a direct WebSocket (`GET /channel`, served
//! here) or a relay link (the relay client feeds a [`Transport`] per link).
//!
//! Flow: `hello` → checks (cheap first) → `welcome` → encrypted frames carrying the
//! inner RPC: requests run through [`api::dispatch`] (responses may come back out of
//! order), `sub` streams events or a setup terminal, `cancel` ends either.
//! The device's authorization is re-checked on every request, whenever the device
//! table changes and when its lease runs out; a revoked device is closed at once.

mod assinatura_autorizada;
mod requisicao_autorizada;

use crate::LockExt;
use crate::api;
use crate::api::devices::{self, Caller, RejectCode};
use crate::fork::autoridade;
use crate::fork::browser::stream::{Stream as BrowserStream, canal as browser_canal};
use crate::hub::Hub;
use crate::remote::crypto::{self, FrameError, Opener, Sealer};
use crate::store::{Device, DeviceSource, Scope, now_ms};
use axum::extract::ws::{CloseFrame, Message, WebSocket};
use futures::{SinkExt as _, StreamExt as _};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::AbortHandle;
use tokio::time::{Instant, sleep_until, timeout};
use x25519_dalek::StaticSecret;

/// Largest WebSocket message accepted (and the Cloudflare limit); 256 KiB chunks fit.
pub const MAX_WS_MESSAGE: usize = 1 << 20;
/// Largest reassembled device → host message.
const MAX_INBOUND: usize = 1 << 20;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Channel keys are replaced (close `4011`) after this long.
const MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);
/// A pairing channel only lives long enough to redeem its code.
const PAIRING_MAX_AGE: Duration = Duration::from_secs(120);
const DIRECT_PING: Duration = Duration::from_secs(15);
const MAX_REQUESTS: usize = 32;
const MAX_SUBS: usize = 4;

/// WebSocket close codes (§7.6).
mod close {
    pub const NORMAL: u16 = 1000;
    pub const TIMEOUT: u16 = 1011;
    pub const UNAUTHORIZED: u16 = 4001;
    pub const PROTOCOL: u16 = 4002;
    pub const REKEY: u16 = 4011;
    pub const TOO_LARGE: u16 = 4013;
    pub const PAIRED: u16 = 4100;
    pub const PAIRING_EXPIRED: u16 = 4410;
}

/// Something to put on the wire.
#[derive(Debug)]
pub enum Out {
    /// A plaintext channel message (`welcome`, `reject`, `f`).
    Msg(Value),
    /// Close with this code and reason; nothing follows.
    Close(u16, &'static str),
}

/// One channel's wire, whatever carries it. `incoming` yields the device's channel
/// messages (`Value::Null` for anything that isn't a JSON text message) and ends when
/// the device is gone.
pub struct Transport {
    pub incoming: mpsc::Receiver<Value>,
    pub outgoing: mpsc::Sender<Out>,
}

pub enum ChannelKind {
    PersonalRelay {
        admission: Box<crate::fork::personal_bootstrap::probe::PersonalRelayAdmission>,
    },
    Direct {
        peer: IpAddr,
    },
    /// A relay link, with what the relay's `open` said about it.
    Relay {
        link: String,
        dk: String,
        pair: bool,
    },
    BootstrapRelay {
        link: String,
        dk: String,
        bootstrap_id: String,
    },
}

impl ChannelKind {
    /// Where pairing attempts are counted from.
    fn source(&self) -> String {
        match self {
            Self::PersonalRelay { .. } => "personal-probe".into(),
            Self::Direct { peer } => peer.to_canonical().to_string(),
            Self::Relay { link, .. } | Self::BootstrapRelay { link, .. } => format!("link:{link}"),
        }
    }
}

/// Serves `GET /channel` after the WebSocket upgrade.
pub async fn serve_direct(hub: Arc<Hub>, socket: WebSocket, peer: IpAddr) {
    let (mut sink, mut stream) = socket.split();
    let (in_tx, incoming) = mpsc::channel(64);
    let (outgoing, mut out_rx) = mpsc::channel::<Out>(64);
    let reader = tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            let v = match msg {
                Message::Text(t) => serde_json::from_str(t.as_str()).unwrap_or(Value::Null),
                Message::Binary(_) => Value::Null,
                Message::Close(_) => break,
                Message::Ping(_) | Message::Pong(_) => continue,
            };
            if in_tx.send(v).await.is_err() {
                break;
            }
        }
    });
    let writer = tokio::spawn(async move {
        let mut ping = tokio::time::interval(DIRECT_PING);
        ping.tick().await;
        loop {
            tokio::select! {
                out = out_rx.recv() => {
                    let msg = match out {
                        Some(Out::Msg(v)) => Message::Text(v.to_string().into()),
                        Some(Out::Close(code, reason)) => Message::Close(Some(CloseFrame { code, reason: reason.into() })),
                        None => Message::Close(None),
                    };
                    let last = matches!(msg, Message::Close(_));
                    if sink.send(msg).await.is_err() || last {
                        break;
                    }
                }
                _ = ping.tick() => {
                    if sink.send(Message::Ping(axum::body::Bytes::new())).await.is_err() {
                        break;
                    }
                }
            }
        }
    });
    run(hub, Transport { incoming, outgoing }, ChannelKind::Direct { peer }).await;
    let _ = writer.await;
    reader.abort();
}

/// What the encrypted part of the channel sends: inner messages get sealed, the rest passes.
enum ToDevice {
    Inner(Value),
    /// Quadro JPEG já codificado (`MXJ1`); viaja cifrado como qualquer inner.
    InnerBytes(Vec<u8>),
    Plain(Value),
    Close(u16, &'static str),
}

/// Owns the host → device counter: seals inner messages in order, stops at the first close.
async fn write_frames(mut sealer: Sealer, mut rx: mpsc::Receiver<ToDevice>, out: mpsc::Sender<Out>) {
    while let Some(msg) = rx.recv().await {
        let sent = match msg {
            ToDevice::Plain(v) => out.send(Out::Msg(v)).await.is_ok(),
            ToDevice::Close(code, reason) => {
                let _ = out.send(Out::Close(code, reason)).await;
                return;
            }
            ToDevice::InnerBytes(bytes) => {
                let Some(frames) = sealer.seal(&bytes) else {
                    let _ = out.send(Out::Close(close::REKEY, "rekey")).await;
                    return;
                };
                let mut ok = true;
                for f in frames {
                    ok = ok && out.send(Out::Msg(f)).await.is_ok();
                }
                ok
            }
            ToDevice::Inner(v) => {
                let Some(frames) = sealer.seal(v.to_string().as_bytes()) else {
                    let _ = out.send(Out::Close(close::REKEY, "rekey")).await;
                    return;
                };
                let mut ok = true;
                for f in frames {
                    ok = ok && out.send(Out::Msg(f)).await.is_ok();
                }
                ok
            }
        };
        if !sent {
            return;
        }
    }
}

// A slot has at most one live channel in this host process. Durable challenge CAS
// remains the cross-process/crash fence; dropping this guard never resets the slot.
static BOOTSTRAP_LINKS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
struct BootstrapGuard(String);
impl BootstrapGuard {
    fn acquire(generation: &str, id: &str) -> Option<Self> {
        let name = format!("{generation}:{id}");
        let mut links = BOOTSTRAP_LINKS.get_or_init(|| Mutex::new(HashSet::new())).locked();
        if !links.insert(name.clone()) {
            return None;
        }
        Some(Self(name))
    }
}
impl Drop for BootstrapGuard {
    fn drop(&mut self) {
        BOOTSTRAP_LINKS.get_or_init(|| Mutex::new(HashSet::new())).locked().remove(&self.0);
    }
}
struct ActiveBootstrapSlot {
    hub: Arc<Hub>,
    id: String,
    generation: String,
}
impl Drop for ActiveBootstrapSlot {
    fn drop(&mut self) {
        // Closes only an active slot. A committed proof requires its separate grant tombstone.
        if self.hub.store.bootstrap_cancel_slot(&self.id, &self.generation).is_ok() {
            self.hub.auth_changed();
        }
    }
}
struct BootstrapLink {
    id: String,
    generation: String,
    expires_at: i64,
    _guard: BootstrapGuard,
}

/// Runs one channel to its end.
pub async fn run(hub: Arc<Hub>, t: Transport, kind: ChannelKind) {
    if let ChannelKind::PersonalRelay { admission } = kind {
        crate::fork::personal_bootstrap::probe::run(hub, t, *admission).await;
        return;
    }
    let Transport { mut incoming, outgoing } = t;
    let Some(hs) = handshake(&hub, &mut incoming, &outgoing, &kind).await else { return };
    let (tx, rx) = mpsc::channel(256);
    let mut writer = tokio::spawn(write_frames(Sealer::new(hs.keys.h2d), rx, outgoing));
    let caller = if let Some(ref b) = hs.bootstrap {
        Caller::Bootstrap { key: hs.key.clone(), bootstrap_id: b.id.clone() }
    } else if let Some((_, caller)) = hs.device.as_ref() {
        caller.clone()
    } else {
        Caller::Pairing { key: hs.key.clone() }
    };
    let mut ch = Channel {
        hub,
        tx,
        jobs: Arc::default(),
        key: hs.key,
        caller: Arc::new(caller),
        lease: None,
        online: None,
        quadros: hs.quadros,
        browser: None,
    };
    let opener = Opener::new(
        hs.keys.d2h,
        if hs.bootstrap.is_some() { crate::fork::bootstrap::codec::LIMIT } else { MAX_INBOUND },
    );
    let (code, reason) = if let Some(ref bootstrap) = hs.bootstrap {
        ch.bootstrap(&mut incoming, opener, bootstrap).await
    } else if let Some((d, _)) = hs.device {
        ch.set_device(&d);
        ch.serve(&mut incoming, opener, &mut writer).await
    } else {
        ch.caller = Arc::new(Caller::Pairing { key: ch.key.clone() });
        ch.pair(&mut incoming, opener, &kind.source()).await
    };
    for job in ch.jobs.locked().drain() {
        if let (_, Job::Sub(h)) = job {
            h.abort();
        }
    }
    let _ = ch.tx.send(ToDevice::Close(code, reason)).await;
    let hub = ch.hub.clone();
    drop(ch);
    // `serve` already took the writer's end when it stopped first; polling it again panics.
    if !writer.is_finished() {
        let _ = writer.await;
    }
    // Only now, with the answer and the `4100` handed to the transport: a relay publishes the
    // new ACL (offer gone) behind them on the same socket, so the pairing link isn't cut first.
    if code == close::PAIRED {
        hub.auth_changed();
    }
}

struct Handshake {
    key: String,
    keys: crypto::Keys,
    /// `None` for a pairing channel.
    device: Option<(Device, Caller)>,
    bootstrap: Option<BootstrapLink>,
    /// Capacidade de quadros negociada no `hello`; ausente no cliente antigo.
    quadros: bool,
}

async fn send_reject(out: &mpsc::Sender<Out>, code: RejectCode) {
    let _ = out.send(Out::Msg(json!({"t": "reject", "code": code, "message": code.message()}))).await;
    let _ = out.send(Out::Close(code.close_code(), "rejected")).await;
}

async fn handshake(
    hub: &Hub,
    incoming: &mut mpsc::Receiver<Value>,
    out: &mpsc::Sender<Out>,
    kind: &ChannelKind,
) -> Option<Handshake> {
    let protocol_error = async || {
        let _ = out.send(Out::Close(close::PROTOCOL, "protocolError")).await;
    };
    let hello = match timeout(HANDSHAKE_TIMEOUT, incoming.recv()).await {
        Ok(Some(v)) => v,
        Ok(None) => return None,
        Err(_) => {
            let _ = out.send(Out::Close(close::TIMEOUT, "no hello")).await;
            return None;
        }
    };
    let dk = hello["dk"].as_str().unwrap_or_default().to_owned();
    let Ok(dk_raw) = crypto::unb64_n::<32>(&dk) else {
        protocol_error().await;
        return None;
    };
    if hello["t"] != "hello" {
        protocol_error().await;
        return None;
    }
    if hello["v"] != 1 {
        send_reject(out, RejectCode::UnsupportedVersion).await;
        return None;
    }
    let pair = hello["pair"].as_bool().unwrap_or(false);
    if let ChannelKind::Relay { dk: open_dk, pair: open_pair, .. } = kind
        && (*open_dk != dk || *open_pair != pair)
    {
        protocol_error().await;
        return None;
    }
    // Bootstrap is admitted only from an explicit relay purpose and an installed slot.
    let bootstrap = if let ChannelKind::BootstrapRelay { dk: open_dk, bootstrap_id, .. } = kind {
        if !pair
            || open_dk != &dk
            || hello["purpose"] != "bootstrap"
            || hello["bootstrapId"] != *bootstrap_id
            || hello.to_string().len() > crate::fork::bootstrap::codec::LIMIT
        {
            protocol_error().await;
            return None;
        }
        let Ok(now) = crate::fork::bootstrap::clock::now() else {
            send_reject(out, RejectCode::Unauthorized).await;
            return None;
        };
        let Ok(Some(g)) = hub.store.bootstrap_active(now) else {
            send_reject(out, RejectCode::Unauthorized).await;
            return None;
        };
        if g.1 != *bootstrap_id
            || g.12 != dk
            || g.14 != hub.identity.computer_id()
            || g.15 != hub.identity.sign_pub_b64()
            || g.16 != hub.identity.box_pub_b64()
        {
            send_reject(out, RejectCode::Unauthorized).await;
            return None;
        }
        let Some(guard) = BootstrapGuard::acquire(&g.4, &g.1) else {
            send_reject(out, RejectCode::RateLimited).await;
            return None;
        };
        Some(BootstrapLink { id: g.1, generation: g.4, expires_at: g.19, _guard: guard })
    } else {
        if hello.get("purpose").is_some() || hello.get("bootstrapId").is_some() {
            protocol_error().await;
            return None;
        }
        None
    };
    // Authorization before any cryptography.
    let device = if bootstrap.is_some() {
        None
    } else if pair {
        if !hub.pairing.locked().open() && !crate::computers::pairing::open(hub) {
            send_reject(out, RejectCode::PairingClosed).await;
            return None;
        }
        None
    } else {
        match autoridade::autenticar(hub, &dk) {
            Ok(d) => Some(d),
            Err(code) => {
                send_reject(out, code).await;
                return None;
            }
        }
    };
    let field = |k: &str| hello[k].as_str().unwrap_or_default();
    let (Ok(ek_d), Ok(n), Ok(sig)) =
        (crypto::unb64_n::<32>(field("ek")), crypto::unb64_n::<32>(field("n")), crypto::unb64_n::<64>(field("sig")))
    else {
        protocol_error().await;
        return None;
    };
    let cid = hub.identity.cid_raw();
    if crypto::verify(&dk_raw, &crypto::hs1_input(&cid, &dk_raw, &ek_d, &n), &sig).is_err() {
        send_reject(out, RejectCode::BadSignature).await;
        return None;
    }
    let ek_h = StaticSecret::from(crypto::random::<32>());
    let ek_h_pub = crypto::x25519_pub(&ek_h);
    let Ok(shared) = crypto::x25519(&ek_h, &ek_d) else {
        protocol_error().await;
        return None;
    };
    let th = crypto::transcript_hash(&cid, &dk_raw, &ek_d, &n, &ek_h_pub);
    if let Some(ref slot) = bootstrap {
        let Ok(now) = crate::fork::bootstrap::clock::now() else {
            send_reject(out, RejectCode::Unauthorized).await;
            return None;
        };
        if !matches!(hub.store.bootstrap_active(now), Ok(Some(ref g)) if g.1 == slot.id && g.4 == slot.generation && g.12 == dk)
        {
            send_reject(out, RejectCode::Unauthorized).await;
            return None;
        }
    }
    let quadros = browser_canal::negociar(hello.get("caps"));
    let mut welcome =
        json!({"t": "welcome", "v": 1, "ek": crypto::b64(&ek_h_pub), "sig": crypto::b64(&hub.identity.sign(&th))});
    if quadros {
        welcome["caps"] = json!([browser_canal::CAPACIDADE_QUADROS]);
    }
    if out.send(Out::Msg(welcome)).await.is_err() {
        return None;
    }
    Some(Handshake { key: dk, keys: crypto::channel_keys(&shared, &th), device, bootstrap, quadros })
}

enum Job {
    Request(AbortHandle),
    Sub(AbortHandle),
}

/// Marks a device connected while it has a live channel.
struct Online(Arc<Hub>, String);

impl Online {
    fn new(hub: Arc<Hub>, key: String) -> Self {
        *hub.connected.locked().entry(key.clone()).or_default() += 1;
        if let Err(error) = hub.store.touch_device(&key) {
            tracing::warn!(error = format!("{error:#}"), "couldn't record when a device was last seen");
        }
        crate::analytics::phone_connected(&hub, &key);
        Self(hub, key)
    }
}

impl Drop for Online {
    fn drop(&mut self) {
        let mut map = self.0.connected.locked();
        if let Some(n) = map.get_mut(&self.1) {
            *n -= 1;
            if *n == 0 {
                map.remove(&self.1);
            }
        }
    }
}

/// Posse do stream de quadros deste canal; ao cair, a sessão é desregistrada.
struct BrowserConn {
    estado: Arc<Mutex<BrowserStream>>,
    autoridade: Arc<browser_canal::AutoridadeCanal>,
    _guarda: browser_canal::GuardaSessao,
}

struct Channel {
    hub: Arc<Hub>,
    tx: mpsc::Sender<ToDevice>,
    jobs: Arc<Mutex<HashMap<u64, Job>>>,
    key: String,
    caller: Arc<Caller>,
    lease: Option<Instant>,
    online: Option<Online>,
    quadros: bool,
    browser: Option<BrowserConn>,
}

type CloseWith = (u16, &'static str);

fn frame_close(e: &FrameError) -> CloseWith {
    match e {
        FrameError::Protocol => (close::PROTOCOL, "protocolError"),
        FrameError::TooLarge => (close::TOO_LARGE, "tooLarge"),
        FrameError::Rekey => (close::REKEY, "rekey"),
    }
}

fn err(id: u64, status: u16, message: &str) -> Value {
    json!({"id": id, "err": {"status": status, "message": message}})
}

impl Channel {
    fn set_device(&mut self, d: &Device) {
        self.lease = match (d.source, d.lease_until) {
            (DeviceSource::Account, Some(until)) => {
                let left = u64::try_from(until - now_ms()).unwrap_or(0) + 1;
                Some(Instant::now() + Duration::from_millis(left))
            }
            _ => None,
        };
    }

    async fn send(&self, v: Value) {
        let _ = self.tx.send(ToDevice::Inner(v)).await;
    }

    /// Re-checks the device; on failure sends `reject` and says how to close.
    async fn recheck(&mut self) -> Result<(), CloseWith> {
        match autoridade::reconferir(&self.hub, &self.caller) {
            Ok((d, caller)) => {
                self.caller = Arc::new(caller);
                self.set_device(&d);
                Ok(())
            }
            Err(code) => {
                // Gone from the table while connected: it was revoked.
                let code = if code == RejectCode::Unauthorized { RejectCode::Revoked } else { code };
                tracing::info!(device = self.key, ?code, "closing channel: device no longer authorized");
                let reject = json!({"t": "reject", "code": code, "message": code.message()});
                let _ = self.tx.send(ToDevice::Plain(reject)).await;
                Err((code.close_code(), "rejected"))
            }
        }
    }

    async fn serve(
        &mut self,
        incoming: &mut mpsc::Receiver<Value>,
        mut opener: Opener,
        writer: &mut tokio::task::JoinHandle<()>,
    ) -> CloseWith {
        let mut auth = self.hub.auth.subscribe();
        let rekey_at = Instant::now() + MAX_AGE;
        let mut vazao = tokio::time::interval(Duration::from_millis(50));
        loop {
            let lease = self.lease;
            tokio::select! {
                _ = vazao.tick(), if self.browser.is_some() => {
                    self.flush_browser().await;
                }
                msg = incoming.recv() => {
                    let Some(msg) = msg else { return (close::NORMAL, "") };
                    let message = match opener.open(&msg) {
                        Ok(m) => m,
                        Err(e) => return frame_close(&e),
                    };
                    if self.online.is_none() {
                        self.online = Some(Online::new(self.hub.clone(), self.key.clone()));
                    }
                    if let Some(bytes) = message
                        && let Err(close) = self.handle(&bytes).await
                    {
                        return close;
                    }
                }
                changed = auth.changed() => {
                    if changed.is_err() {
                        return (close::NORMAL, "");
                    }
                    if let Err(close) = self.recheck().await {
                        return close;
                    }
                }
                () = sleep_until(lease.unwrap_or(rekey_at)), if lease.is_some() => {
                    if let Err(close) = self.recheck().await {
                        return close;
                    }
                }
                () = sleep_until(rekey_at) => return (close::REKEY, "rekey"),
                _ = &mut *writer => return (close::NORMAL, ""),
            }
        }
    }

    /// One inner message from an authorized device.
    async fn handle(&mut self, bytes: &[u8]) -> Result<(), CloseWith> {
        let protocol = (close::PROTOCOL, "protocolError");
        // Quadro binário nunca vem do aparelho: corrompido, gigante ou válido, é recusa.
        if !bytes.starts_with(b"{") {
            if bytes.len() > browser_canal::MAX_BINARIO {
                return Err((close::TOO_LARGE, "tooLarge"));
            }
            if browser_canal::eh_binario(bytes) {
                let _ = browser_canal::decodificar(bytes);
            }
            return Err(protocol);
        }
        let v: Value = serde_json::from_slice(bytes).map_err(|_| protocol)?;
        let id = v["id"].as_u64().filter(|id| u32::try_from(*id).is_ok()).ok_or(protocol)?;
        if v["cancel"] == true {
            self.cancel(id).await;
            return Ok(());
        }
        if self.jobs.locked().contains_key(&id) {
            self.send(err(id, 400, "that id is already in use")).await;
            return Ok(());
        }
        // Every request re-checks the device (revocation, lease) before it runs.
        self.recheck().await?;
        if let Some(method) = v["m"].as_str().filter(|m| browser_canal::METODOS.contains(m)) {
            // A classe da tabela vem antes das guardas próprias do navegador (escopo, Hub/Space/bot e sessão).
            if devices::permit(&self.caller, method).is_err() {
                tracing::warn!(reason = "rpc_class_refused", origin = api::origem(&self.caller), "rpc refused");
                self.send(err(id, 403, &devices::Forbidden.to_string())).await;
                return Ok(());
            }
            self.handle_browser(id, method, &v["b"]).await;
            return Ok(());
        }
        if let Some(kind) = v["sub"].as_str() {
            self.subscribe(id, kind, &v["b"]).await;
        } else if let Some(method) = v["m"].as_str() {
            self.request(id, method.to_owned(), v["b"].clone()).await;
        } else {
            return Err(protocol);
        }
        Ok(())
    }

    /// Não antecipa a imagem enquanto houver intenção pendente de entrada.
    async fn flush_browser(&self) {
        let Some(conn) = &self.browser else { return };
        let frame = conn.estado.locked().next_frame(&*conn.autoridade, &self.key);
        if let Some(frame) = frame
            && let Ok(bytes) = browser_canal::codificar(&frame)
        {
            let _ = self.tx.send(ToDevice::InnerBytes(bytes)).await;
        }
    }

    async fn handle_browser(&mut self, id: u64, method: &str, b: &Value) {
        match method {
            "browserStreamSubscribe" => {
                let Some((escopo, sessao, target)) = browser_canal::analisar_inscricao(b) else {
                    self.send(err(id, 400, "browser_scope_unavailable")).await;
                    return;
                };
                let autoridade = Arc::new(browser_canal::AutoridadeCanal::nova(
                    self.hub.clone(),
                    (*self.caller).clone(),
                    escopo,
                    sessao.clone(),
                ));
                if !autoridade.autorizada() {
                    self.send(err(id, 403, "browser_scope_unavailable")).await;
                    return;
                }
                let estado = Arc::new(Mutex::new(BrowserStream::new(sessao.clone(), target, self.quadros)));
                let Some(guarda) = browser_canal::registrar(&sessao, estado.clone(), autoridade.clone()) else {
                    self.send(err(id, 403, "browser_scope_unavailable")).await;
                    return;
                };
                self.browser = Some(BrowserConn { estado, autoridade, _guarda: guarda });
                self.send(json!({"id": id, "ok": {"subscribed": true, "frames": self.quadros, "session": sessao}}))
                    .await;
            }
            "browserStreamInput" => {
                let Some(conn) = &self.browser else {
                    self.send(err(id, 400, "browser_stream_unavailable")).await;
                    return;
                };
                let aceita = browser_canal::analisar_entrada(b)
                    .is_some_and(|entrada| conn.estado.locked().accept_input(&*conn.autoridade, &self.key, &entrada));
                self.send(json!({"id": id, "ok": {"accepted": aceita, "sequence": b["sequence"]}})).await;
            }
            "browserStreamAck" => {
                if let Some(conn) = &self.browser {
                    conn.estado.locked().acknowledge_frame();
                }
                self.send(json!({"id": id, "ok": {"acknowledged": true}})).await;
            }
            "browserStreamControl" | "browserStreamTarget"
                if !self.browser.as_ref().is_some_and(|conn| conn.autoridade.autorizada()) =>
            {
                self.send(err(id, 403, "browser_scope_unavailable")).await;
            }
            "browserStreamControl" => {
                let controlador = match b["controller"].as_str() {
                    Some("person") => crate::fork::browser::stream::Controller::Person,
                    Some("agent") => crate::fork::browser::stream::Controller::Agent,
                    _ => {
                        self.send(err(id, 400, "browser_stream_unavailable")).await;
                        return;
                    }
                };
                let epoca = self.browser.as_ref().and_then(|conn| conn.estado.locked().set_controller(controlador));
                match epoca {
                    Some(epoca) => self.send(json!({"id": id, "ok": {"epoch": epoca}})).await,
                    None => self.send(err(id, 400, "browser_stream_unavailable")).await,
                }
            }
            "browserStreamTarget" => {
                let Some(target) = b["target"].as_str().filter(|t| !t.is_empty() && t.len() <= 256) else {
                    self.send(err(id, 400, "browser_stream_unavailable")).await;
                    return;
                };
                let geracao = self.browser.as_ref().and_then(|conn| conn.estado.locked().set_target(target.to_owned()));
                match geracao {
                    Some(geracao) => self.send(json!({"id": id, "ok": {"targetGeneration": geracao}})).await,
                    None => self.send(err(id, 400, "browser_stream_unavailable")).await,
                }
            }
            _ => self.send(err(id, 400, "browser_stream_unavailable")).await,
        }
    }

    fn count(&self, subs: bool) -> usize {
        self.jobs.locked().values().filter(|j| matches!(j, Job::Sub(_)) == subs).count()
    }

    async fn request(&self, id: u64, method: String, b: Value) {
        if self.count(false) >= MAX_REQUESTS {
            self.send(err(id, 429, "too many requests in flight")).await;
            return;
        }
        let (hub, caller, jobs, tx) = (self.hub.clone(), self.caller.clone(), self.jobs.clone(), self.tx.clone());
        let mut list = self.jobs.locked();
        let task = tokio::spawn(requisicao_autorizada::executar(id, method, b, hub, caller, jobs, tx));
        list.insert(id, Job::Request(task.abort_handle()));
    }

    async fn subscribe(&self, id: u64, kind: &str, b: &Value) {
        if devices::permit(&self.caller, "subscribe").is_err() {
            self.send(err(id, 403, "Control scope required")).await;
            return;
        }
        if self.count(true) >= MAX_SUBS {
            self.send(err(id, 429, "too many subscriptions")).await;
            return;
        }
        let login = if kind == "term" {
            self.hub.terms.get(b["term"].as_str().unwrap_or_default()).and_then(|term| term.login.clone())
        } else {
            None
        };
        let stream = match kind {
            "events" => match api::scoped_events_stream(
                &self.hub,
                b["since"].as_i64().unwrap_or(0),
                b["client"].as_str(),
                &self.caller,
                b["spaceId"].as_str(),
            ) {
                Ok(s) => s.boxed(),
                Err(e) => {
                    self.send(err(id, 500, &format!("{e:#}"))).await;
                    return;
                }
            },
            "term" => match api::term_events(&self.hub, b["term"].as_str().unwrap_or_default(), &self.caller) {
                Ok(Some(stream)) => stream.boxed(),
                Ok(None) => {
                    self.send(err(id, 404, "that terminal is gone")).await;
                    return;
                }
                Err(error) => {
                    let (status, message) = api::error_status(&error);
                    self.send(err(id, status.as_u16(), &message)).await;
                    return;
                }
            },
            _ => {
                self.send(err(id, 400, "unknown subscription")).await;
                return;
            }
        };
        let (jobs, tx, hub, caller) = (self.jobs.clone(), self.tx.clone(), self.hub.clone(), self.caller.clone());
        // Held while spawning so the task can't finish (and remove itself) before it's listed.
        let mut list = self.jobs.locked();
        let task = tokio::spawn(assinatura_autorizada::executar(id, stream, hub, caller, jobs, tx, login));
        list.insert(id, Job::Sub(task.abort_handle()));
    }

    async fn cancel(&self, id: u64) {
        let job = self.jobs.locked().remove(&id);
        if let Some(Job::Sub(h)) = job {
            h.abort();
            self.send(json!({"id": id, "end": true})).await;
        }
    }

    /// Dedicated bootstrap loop: never calls the normal dispatcher or subscriptions.
    async fn bootstrap(
        &mut self,
        incoming: &mut mpsc::Receiver<Value>,
        mut opener: Opener,
        link: &BootstrapLink,
    ) -> CloseWith {
        let protocol = (close::PROTOCOL, "protocolError");
        let Caller::Bootstrap { key, bootstrap_id } = self.caller.as_ref() else { return protocol };
        if key != &self.key || bootstrap_id != &link.id {
            return protocol;
        }
        let _slot =
            ActiveBootstrapSlot { hub: self.hub.clone(), id: link.id.clone(), generation: link.generation.clone() };
        let mut deadline_ms = link.expires_at;
        let mut challenged = false;
        let mut auth = self.hub.auth.subscribe();
        loop {
            let Ok(left) = crate::fork::bootstrap::clock::remaining_until(deadline_ms) else {
                return (close::PAIRING_EXPIRED, "bootstrapExpired");
            };
            let frame = tokio::select! {
                frame = incoming.recv() => frame,
                () = tokio::time::sleep(left.min(Duration::from_millis(250))) => {
                    let Ok(now) = crate::fork::bootstrap::clock::now() else { return (close::PAIRING_EXPIRED, "bootstrapExpired") };
                    if now >= deadline_ms || !matches!(self.hub.store.bootstrap_current(now), Ok(Some(ref g)) if g.1 == link.id && g.4 == link.generation && g.12 == self.key) {
                        return (close::UNAUTHORIZED, "bootstrapClosed");
                    }
                    continue;
                },
                _ = auth.changed() => {
                    let Ok(now) = crate::fork::bootstrap::clock::now() else { return (close::PAIRING_EXPIRED, "bootstrapExpired") };
                    if !matches!(self.hub.store.bootstrap_current(now), Ok(Some(ref g)) if g.1 == link.id && g.4 == link.generation && g.12 == self.key) {
                        return (close::UNAUTHORIZED, "bootstrapClosed");
                    }
                    continue;
                }
            };
            let Some(frame) = frame else { return (close::NORMAL, "") };
            let bytes = match opener.open(&frame) {
                Ok(Some(b)) => b,
                Ok(None) => continue,
                Err(e) => return frame_close(&e),
            };
            let Ok(v) = serde_json::from_slice::<Value>(&bytes) else { return protocol };
            let Some(id) = v["id"].as_u64() else { return protocol };
            if v.as_object().is_none_or(|o| o.len() != 3 || !o.contains_key("b") || !o.contains_key("m")) {
                return protocol;
            }
            let Ok(now) = crate::fork::bootstrap::clock::now() else {
                return (close::PAIRING_EXPIRED, "bootstrapExpired");
            };
            if !matches!(self.hub.store.bootstrap_current(now), Ok(Some(ref g)) if g.1 == link.id && g.4 == link.generation && g.12 == self.key)
            {
                return (close::UNAUTHORIZED, "bootstrapClosed");
            }
            match v["m"].as_str() {
                Some("bootstrap.challenge") if !challenged && v["b"] == json!({}) => {
                    let Ok(wire) =
                        self.hub.store.bootstrap_challenge(&self.hub.identity.sign, &link.id, &self.key, now)
                    else {
                        return (close::UNAUTHORIZED, "bootstrapClosed");
                    };
                    let Ok(challenge) = crate::fork::bootstrap::codec::decode::<crate::fork::bootstrap::codec::Challenge>(
                        &wire,
                        &self.hub.identity.sign_pub(),
                    ) else {
                        return protocol;
                    };
                    deadline_ms = deadline_ms.min(challenge.8);
                    let Ok(value) = serde_json::from_slice::<Value>(&wire) else { return protocol };
                    challenged = true;
                    self.send(json!({"id":id,"ok":value})).await;
                }
                Some("bootstrap.prove") if challenged => {
                    let Ok(wire) = serde_json::to_vec(&v["b"]) else { return protocol };
                    let Ok(receipt) = self.hub.store.bootstrap_commit_proof(&self.hub.identity.sign, &wire, now) else {
                        return (close::UNAUTHORIZED, "bootstrapClosed");
                    };
                    let Ok(Some(persisted)) = self.hub.store.bootstrap_receipt() else {
                        return (close::UNAUTHORIZED, "bootstrapReceiptUnknown");
                    };
                    if persisted != receipt {
                        return protocol;
                    }
                    let Ok(value) = serde_json::from_slice::<Value>(&persisted) else { return protocol };
                    self.send(json!({"id":id,"ok":value})).await;
                    return (close::PAIRED, "bootstrapCommitted");
                }
                _ => return protocol,
            }
        }
    }

    /// A pairing channel: exactly one `pair`, answered, then closed with `4100` (§4.1).
    async fn pair(&mut self, incoming: &mut mpsc::Receiver<Value>, mut opener: Opener, source: &str) -> CloseWith {
        let protocol = (close::PROTOCOL, "protocolError");
        let deadline = Instant::now() + PAIRING_MAX_AGE;
        let bytes = loop {
            let msg = tokio::select! {
                msg = incoming.recv() => msg,
                () = sleep_until(deadline) => return (close::PAIRING_EXPIRED, "pairingExpired"),
            };
            let Some(msg) = msg else { return (close::NORMAL, "") };
            match opener.open(&msg) {
                Ok(Some(bytes)) => break bytes,
                Ok(None) => {}
                Err(e) => return frame_close(&e),
            }
        };
        let Ok(v) = serde_json::from_slice::<Value>(&bytes) else { return protocol };
        let Some(id) = v["id"].as_u64() else { return protocol };
        if v["m"] != "pair" {
            return protocol;
        }
        let b = &v["b"];
        if b["role"] == "executor" {
            let (reply, code) = match crate::computers::pairing::redeem(&self.hub, &self.key, b, source) {
                Ok(ok) => (json!({"id": id, "ok": ok}), close::PAIRED),
                Err(e) => (err(id, 403, &e.to_string()), close::UNAUTHORIZED),
            };
            self.send(reply).await;
            return (code, "executor enrollment");
        }
        let name = b["name"].as_str().map(str::trim).filter(|n| !n.is_empty() && n.chars().count() <= 100);
        let platform = b["platform"].as_str().filter(|p| matches!(*p, "ios" | "macos" | "android"));
        let (Some(name), Some(platform)) = (name, platform) else {
            self.send(err(id, 400, "`name` and `platform` (ios, macos or android) are required")).await;
            return protocol;
        };
        if !self.hub.pairing.locked().attempt(source) {
            let code = RejectCode::RateLimited;
            let _ =
                self.tx.send(ToDevice::Plain(json!({"t": "reject", "code": code, "message": code.message()}))).await;
            return (code.close_code(), "rateLimited");
        }
        if !self.hub.pairing.locked().redeem(b["code"].as_str().unwrap_or_default()) {
            self.send(err(id, 403, "This pairing code expired. Show a new one on the computer.")).await;
            return (close::UNAUTHORIZED, "unauthorized");
        }
        let device = Device {
            key: self.key.clone(),
            name: name.to_owned(),
            platform: platform.to_owned(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![Scope::Control, Scope::Screen],
            lease_until: None,
            created_at: now_ms(),
            last_seen_at: None,
        };
        if let Err(e) = self.hub.store.put_device(&device) {
            tracing::error!(error = format!("{e:#}"), "couldn't save a paired device");
            self.send(err(id, 500, "couldn't save this device")).await;
            return (close::NORMAL, "");
        }
        tracing::info!(device = self.key, platform, "device paired");
        crate::analytics::device_paired(&self.hub, crate::analytics::Via::Qr, platform, false);
        let name = crate::service::host_name();
        self.send(json!({"id": id, "ok": {"computerId": self.hub.identity.computer_id(), "name": name}})).await;
        (close::PAIRED, "paired")
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::remote::identity::Identity;
    use crate::store::Store;
    use ed25519_dalek::SigningKey;

    mod observador_concessao;
    mod provas_encerramento;
    mod provas_grupos;
    mod provas_navegador;
    mod provas_requisicao_expirada;
    mod provas_requisicoes;
    mod provas_tabela;

    use observador_concessao::geracao_persistida;

    fn temp_hub() -> Arc<Hub> {
        let dir = std::env::temp_dir().join(format!("codync-ch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("t.db")).unwrap();
        let identity = Identity::load_or_create(&dir).unwrap();
        Hub::new(store, "host".into(), identity, "token".into(), 0)
    }

    /// The device end of a channel, over in-memory pipes.
    struct Phone {
        key: SigningKey,
        tx: mpsc::Sender<Value>,
        rx: mpsc::Receiver<Out>,
        sealer: Option<Sealer>,
        opener: Option<Opener>,
        /// The host's side of this channel.
        run: tokio::task::JoinHandle<()>,
    }

    impl Phone {
        fn connect(hub: &Arc<Hub>, key: SigningKey, kind: ChannelKind) -> Self {
            let (tx, incoming) = mpsc::channel(64);
            let (outgoing, rx) = mpsc::channel(64);
            let run = tokio::spawn(run(hub.clone(), Transport { incoming, outgoing }, kind));
            Self { key, tx, rx, sealer: None, opener: None, run }
        }

        fn direct(hub: &Arc<Hub>, key: SigningKey) -> Self {
            Self::connect(hub, key, ChannelKind::Direct { peer: IpAddr::from([10, 0, 0, 2]) })
        }

        fn dk(&self) -> String {
            crypto::b64(self.key.verifying_key().as_bytes())
        }

        async fn next(&mut self) -> Out {
            timeout(Duration::from_secs(60), self.rx.recv()).await.expect("host answers in time").expect("still open")
        }

        async fn hello(&mut self, hub: &Hub, pair: bool) -> Out {
            let ek = StaticSecret::from(crypto::random::<32>());
            let ek_pub = crypto::x25519_pub(&ek);
            let n = crypto::random::<32>();
            let dk = self.key.verifying_key().to_bytes();
            let cid = hub.identity.cid_raw();
            let sig = crypto::sign(&self.key, &crypto::hs1_input(&cid, &dk, &ek_pub, &n));
            let hello = json!({"t": "hello", "v": 1, "dk": self.dk(), "ek": crypto::b64(&ek_pub),
                "n": crypto::b64(&n), "sig": crypto::b64(&sig), "pair": pair});
            self.tx.send(hello).await.unwrap();
            let out = self.next().await;
            if let Out::Msg(w) = &out
                && w["t"] == "welcome"
            {
                let ek_h: [u8; 32] = crypto::unb64_n(w["ek"].as_str().unwrap()).unwrap();
                let th = crypto::transcript_hash(&cid, &dk, &ek_pub, &n, &ek_h);
                crypto::verify(&hub.identity.sign_pub(), &th, &crypto::unb64_n(w["sig"].as_str().unwrap()).unwrap())
                    .expect("welcome is signed by the host");
                let keys = crypto::channel_keys(&crypto::x25519(&ek, &ek_h).unwrap(), &th);
                self.sealer = Some(Sealer::new(keys.d2h));
                self.opener = Some(Opener::new(keys.h2d, 16 << 20));
            }
            out
        }

        async fn send(&mut self, v: Value) {
            for f in self.sealer.as_mut().unwrap().seal(v.to_string().as_bytes()).unwrap() {
                self.tx.send(f).await.unwrap();
            }
        }

        /// The next inner message, or the close that ended the channel.
        async fn recv(&mut self) -> Result<Value, (u16, Value)> {
            let mut last_plain = Value::Null;
            loop {
                match self.next().await {
                    Out::Msg(f) if f["t"] == "f" => {
                        if let Some(bytes) = self.opener.as_mut().unwrap().open(&f).unwrap() {
                            return Ok(serde_json::from_slice(&bytes).unwrap());
                        }
                    }
                    Out::Msg(plain) => last_plain = plain,
                    Out::Close(code, _) => return Err((code, last_plain)),
                }
            }
        }

        async fn call(&mut self, id: u64, m: &str, b: Value) -> Value {
            self.send(json!({"id": id, "m": m, "b": b})).await;
            loop {
                let v = self.recv().await.expect("channel stays open");
                if v["id"] == id && (v.get("ok").is_some() || v.get("err").is_some()) {
                    return v;
                }
            }
        }
    }

    fn closed(out: &Out) -> u16 {
        match out {
            Out::Close(code, _) => *code,
            Out::Msg(v) => panic!("expected a close, got {v}"),
        }
    }

    async fn paired_phone(hub: &Arc<Hub>) -> SigningKey {
        let key = SigningKey::from_bytes(&crypto::random());
        let code = hub.pairing.locked().issue().code;
        let mut p = Phone::direct(hub, key.clone());
        assert!(matches!(p.hello(hub, true).await, Out::Msg(v) if v["t"] == "welcome"));
        p.send(json!({"id": 1, "m": "pair", "b": {"code": code, "name": "Kevin's iPhone", "platform": "ios"}})).await;
        let reply = p.recv().await.unwrap();
        assert_eq!(reply["ok"]["computerId"], hub.identity.computer_id());
        assert_eq!(p.recv().await.unwrap_err().0, close::PAIRED);
        key
    }

    #[tokio::test]
    async fn c1_handshake_na_fila_nao_captura_reconcessao_apos_auth() {
        let hub = temp_hub();
        let chave = paired_phone(&hub).await;
        let dk = crypto::b64(chave.verifying_key().as_bytes());
        let aparelho = hub.store.device(&dk).unwrap();
        let notificacao = hub.auth.subscribe();
        let efemera = StaticSecret::from(crypto::random::<32>());
        let publica = crypto::x25519_pub(&efemera);
        let nonce = crypto::random::<32>();
        let assinatura = crypto::sign(
            &chave,
            &crypto::hs1_input(&hub.identity.cid_raw(), &chave.verifying_key().to_bytes(), &publica, &nonce),
        );
        let (entrada, mut mensagens) = mpsc::channel(1);
        let (saida, mut respostas) = mpsc::channel(1);
        entrada
            .send(json!({"t":"hello","v":1,"dk":dk,"ek":crypto::b64(&publica),
            "n":crypto::b64(&nonce),"sig":crypto::b64(&assinatura),"pair":false}))
            .await
            .unwrap();
        saida.send(Out::Msg(json!({"ocupado":true}))).await.unwrap();
        let tipo = ChannelKind::Direct { peer: IpAddr::from([10, 0, 0, 2]) };
        let mut autenticacao = Box::pin(handshake(&hub, &mut mensagens, &saida, &tipo));
        // Pendente na vaga de welcome: assinatura e captura já foram conferidas.
        assert!(futures::poll!(&mut autenticacao).is_pending());
        hub.revoke_device(&dk).unwrap();
        hub.store.put_device(&aparelho).unwrap();
        hub.auth_changed();
        assert!(notificacao.has_changed().unwrap());
        respostas.recv().await.unwrap();
        let concluida = autenticacao.await.unwrap();
        let (_, caller) = concluida.device.unwrap();
        assert!(autoridade::Autoridade::capturar(&hub, &caller).is_err());
        assert!(
            api::dispatch(&hub, &caller, "createBot", json!({"name":"Não criar","backend":"codex"})).await.is_err()
        );
        assert!(hub.store.bots().unwrap().is_empty());
    }

    #[test]
    fn bootstrap_guard_is_exclusive_and_dispatch_is_forbidden() {
        let guard = BootstrapGuard::acquire("test-generation", "test-slot").unwrap();
        assert!(BootstrapGuard::acquire("test-generation", "test-slot").is_none());
        let caller = Caller::Bootstrap { key: "device".into(), bootstrap_id: "test-slot".into() };
        for method in ["hello", "pair", "send", "devices", "bootstrap.challenge", "bootstrap.prove"] {
            assert!(devices::permit(&caller, method).is_err());
        }
        drop(guard);
        assert!(BootstrapGuard::acquire("test-generation", "test-slot").is_some());
    }

    #[tokio::test]
    async fn bootstrap_relay_refuses_missing_purpose_before_pairing() {
        let hub = temp_hub();
        let key = SigningKey::from_bytes(&[19; 32]);
        let dk = crypto::b64(key.verifying_key().as_bytes());
        let mut phone = Phone::connect(
            &hub,
            key,
            ChannelKind::BootstrapRelay { link: "test".into(), dk, bootstrap_id: "boot".into() },
        );
        assert_eq!(closed(&phone.hello(&hub, true).await), close::PROTOCOL);
    }

    #[tokio::test]
    async fn android_pairing_preserves_authorization_and_one_time_code() {
        let hub = temp_hub();
        let key = SigningKey::from_bytes(&crypto::random());
        let code = hub.pairing.locked().issue().code;
        let mut phone = Phone::direct(&hub, key.clone());
        assert!(matches!(phone.hello(&hub, true).await, Out::Msg(v) if v["t"] == "welcome"));
        let reply = phone.call(1, "pair", json!({"code": code, "name": "Android phone", "platform": "android"})).await;
        assert_eq!(reply["ok"]["computerId"], hub.identity.computer_id());
        assert_eq!(phone.recv().await.unwrap_err().0, close::PAIRED);
        let device = hub.store.device(&phone.dk()).unwrap();
        assert_eq!(device.platform, "android");
        assert_eq!(device.source, DeviceSource::Local);
        assert_eq!(device.scopes, [Scope::Control, Scope::Screen]);
        assert!(!hub.pairing.locked().open());

        let mut connected = Phone::direct(&hub, key);
        assert!(matches!(connected.hello(&hub, false).await, Out::Msg(v) if v["t"] == "welcome"));
        let hello = connected.call(1, "hello", json!({})).await;
        assert_eq!(hello["ok"]["computerId"], hub.identity.computer_id());
        let refused = connected.call(2, "setScreenEnabled", json!({"enabled": true})).await;
        assert_eq!(refused["err"]["status"], 403);
    }

    #[tokio::test]
    async fn unknown_device_is_rejected() {
        let hub = temp_hub();
        let mut p = Phone::direct(&hub, SigningKey::from_bytes(&crypto::random()));
        let Out::Msg(reject) = p.hello(&hub, false).await else { panic!("expected reject") };
        assert_eq!(reject["code"], "unauthorized");
        assert_eq!(closed(&p.next().await), 4001);
    }

    #[tokio::test]
    async fn pairing_then_normal_connection() {
        let hub = temp_hub();
        let key = paired_phone(&hub).await;
        let d = hub.store.device(&crypto::b64(key.verifying_key().as_bytes())).unwrap();
        assert_eq!((d.source, d.name.as_str()), (DeviceSource::Local, "Kevin's iPhone"));
        assert!(!hub.pairing.locked().open(), "the code is used up");

        let mut p = Phone::direct(&hub, key);
        assert!(matches!(p.hello(&hub, false).await, Out::Msg(v) if v["t"] == "welcome"));
        let hello = p.call(1, "hello", json!({})).await;
        assert_eq!(hello["ok"]["computerId"], hub.identity.computer_id());
        assert_eq!(hello["ok"]["signKey"], hub.identity.sign_pub_b64());
        assert_eq!(hello["ok"]["minApp"], crate::compat::MIN_APP);
        assert_eq!(hub.connected.locked().get(&p.dk()), Some(&1));
        assert!(hub.store.device(&p.dk()).unwrap().last_seen_at.is_some());

        // Loopback-only methods are refused over the channel.
        for m in [
            "setScreenEnabled",
            "setAnalytics",
            "track",
            "reportError",
            "pairing",
            "devices",
            "revokeDevice",
            "claimSign",
            "computerCall",
            "computerTools",
        ] {
            assert_eq!(p.call(2, m, json!({})).await["err"]["status"], 403, "{m}");
        }
        assert_eq!(p.call(3, "pair", json!({})).await["err"]["status"], 403);
    }

    #[tokio::test]
    async fn pair_hello_without_a_code_is_refused_before_any_crypto() {
        let hub = temp_hub();
        let (tx, incoming) = mpsc::channel(4);
        let (outgoing, mut rx) = mpsc::channel(4);
        tokio::spawn(run(
            hub.clone(),
            Transport { incoming, outgoing },
            ChannelKind::Direct { peer: [10, 0, 0, 2].into() },
        ));
        let dk = crypto::b64(SigningKey::from_bytes(&[1; 32]).verifying_key().as_bytes());
        // A garbage key and signature: only a pre-crypto check can answer pairingClosed.
        tx.send(json!({"t": "hello", "v": 1, "dk": dk, "ek": "x", "n": "x", "sig": "x", "pair": true})).await.unwrap();
        let Some(Out::Msg(reject)) = rx.recv().await else { panic!("expected reject") };
        assert_eq!(reject["code"], "pairingClosed");
        assert_eq!(closed(&rx.recv().await.unwrap()), 4410);
    }

    #[tokio::test]
    async fn pairing_channel_only_takes_pair_and_codes_are_single_use() {
        let hub = temp_hub();
        let code = hub.pairing.locked().issue().code;
        hub.pairing.locked().issue();
        let mut p = Phone::direct(&hub, SigningKey::from_bytes(&crypto::random()));
        p.hello(&hub, true).await;
        p.send(json!({"id": 1, "m": "hello", "b": {}})).await;
        assert_eq!(p.recv().await.unwrap_err().0, close::PROTOCOL);

        let mut p = Phone::direct(&hub, SigningKey::from_bytes(&crypto::random()));
        p.hello(&hub, true).await;
        p.send(json!({"id": 1, "m": "pair", "b": {"code": code, "name": "A", "platform": "ios"}})).await;
        assert!(p.recv().await.unwrap()["ok"].is_object());

        let mut p = Phone::direct(&hub, SigningKey::from_bytes(&crypto::random()));
        p.hello(&hub, true).await;
        p.send(json!({"id": 1, "m": "pair", "b": {"code": code, "name": "B", "platform": "ios"}})).await;
        assert_eq!(p.recv().await.unwrap()["err"]["status"], 403, "a used code doesn't work twice");
        assert_eq!(p.recv().await.unwrap_err().0, close::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn revoking_closes_the_channel_and_its_subscription() {
        let hub = temp_hub();
        let bot = serde_json::from_value(json!({"id": "b1", "name": "Test", "createdAt": 0})).unwrap();
        hub.store.save_bot(&bot).unwrap();
        hub.set_runtime("b1", |runtime| runtime.status = crate::hub::BotStatus::Working);
        let key = paired_phone(&hub).await;
        let dk = crypto::b64(key.verifying_key().as_bytes());
        let mut p = Phone::direct(&hub, key);
        p.hello(&hub, false).await;
        assert!(p.call(1, "registerDevice", json!({"ticket": "t1", "name": "iPhone"})).await["ok"].is_object());
        assert!(p.call(2, "registerActivity", json!({"ticket": "a1", "botId": "b1"})).await["ok"].is_object());
        p.send(json!({"id": 3, "sub": "events", "b": {"since": hub.store.current_rev(), "client": "ios"}})).await;
        let hello = p.recv().await.unwrap();
        assert_eq!((hello["id"].as_u64(), hello["ev"]["type"].as_str()), (Some(3), Some("hello")));
        assert!(hub.phone_connected());

        assert!(hub.revoke_device(&dk).unwrap());
        let (code, reject) = p.recv().await.unwrap_err();
        assert_eq!(code, 4003);
        assert_eq!(reject["code"], "revoked");
        assert!(hub.store.push_tickets().is_empty());
        assert_eq!(hub.store.activity_tickets("b1").len(), 0);
        timeout(Duration::from_secs(60), async {
            while hub.phone_connected() || !hub.connected.locked().is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("o canal e a assinatura devem encerrar");
        assert!(!hub.phone_connected(), "the events subscription ended with the channel");
        assert!(hub.connected.locked().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn account_devices_need_a_synced_live_lease() {
        let hub = temp_hub();
        let key = SigningKey::from_bytes(&crypto::random());
        let dk = crypto::b64(key.verifying_key().as_bytes());
        let device = Device {
            key: dk.clone(),
            name: "Phone".into(),
            platform: "ios".into(),
            source: DeviceSource::Account,
            grant_id: Some("grt_x".into()),
            scopes: vec![Scope::Control],
            lease_until: Some(now_ms() + 120_000),
            created_at: now_ms(),
            last_seen_at: None,
        };
        hub.store.put_device(&device).unwrap();
        let mut p = Phone::direct(&hub, key.clone());
        let Out::Msg(reject) = p.hello(&hub, false).await else { panic!("expected reject") };
        assert_eq!(reject["code"], "leaseExpired", "refused until the cloud state was applied");

        crate::remote::cloud::update_status(&hub, |s| {
            s.owner = Some(crate::remote::cloud::Owner { user_id: "dono-teste".into(), email: None });
        });
        hub.cloud_synced.store(true, std::sync::atomic::Ordering::Release);
        let mut p = Phone::direct(&hub, key.clone());
        assert!(matches!(p.hello(&hub, false).await, Out::Msg(v) if v["t"] == "welcome"));
        assert_eq!(p.call(1, "screenOffer", json!({})).await["err"]["status"], 403, "no screen scope");
        // O TTL real não compete com o handshake. Vence no DB e avança somente
        // o timer do canal: sem RPC nem notificação de auth para provocar recheck.
        hub.store.set_lease(&dk, now_ms() - 1).unwrap();
        tokio::time::advance(Duration::from_secs(121)).await;
        let (code, reject) = p.recv().await.unwrap_err();
        assert_eq!((code, reject["code"].as_str()), (4001, Some("leaseExpired")), "closed when the lease ran out");
    }

    #[tokio::test]
    async fn relay_link_hello_must_match_open() {
        let hub = temp_hub();
        let key = paired_phone(&hub).await;
        let other = crypto::b64(SigningKey::from_bytes(&crypto::random()).verifying_key().as_bytes());
        let kind = ChannelKind::Relay { link: "l1".into(), dk: other, pair: false };
        let mut p = Phone::connect(&hub, key.clone(), kind);
        assert_eq!(closed(&p.hello(&hub, false).await), close::PROTOCOL);

        let dk = crypto::b64(key.verifying_key().as_bytes());
        let mut p = Phone::connect(&hub, key, ChannelKind::Relay { link: "l2".into(), dk, pair: false });
        assert!(matches!(p.hello(&hub, false).await, Out::Msg(v) if v["t"] == "welcome"));
    }

    #[tokio::test]
    async fn replayed_or_bad_frames_close_the_channel() {
        let hub = temp_hub();
        let key = paired_phone(&hub).await;
        let mut p = Phone::direct(&hub, key);
        p.hello(&hub, false).await;
        let frames = p.sealer.as_mut().unwrap().seal(br#"{"id":1,"m":"hello","b":{}}"#).unwrap();
        p.tx.send(frames[0].clone()).await.unwrap();
        assert!(p.recv().await.unwrap()["ok"].is_object());
        p.tx.send(frames[0].clone()).await.unwrap();
        assert_eq!(p.recv().await.unwrap_err().0, close::PROTOCOL, "replay");
    }

    #[tokio::test]
    async fn cancelled_subscription_ends() {
        let hub = temp_hub();
        let key = paired_phone(&hub).await;
        let mut p = Phone::direct(&hub, key);
        p.hello(&hub, false).await;
        p.send(json!({"id": 7, "sub": "events", "b": {"since": 0}})).await;
        assert_eq!(p.recv().await.unwrap()["ev"]["type"], "hello");
        p.send(json!({"id": 7, "cancel": true})).await;
        assert_eq!(p.recv().await.unwrap(), json!({"id": 7, "end": true}));
        assert_eq!(p.call(8, "hello", json!({})).await["id"], 8, "the channel goes on");
        p.send(json!({"id": 9, "sub": "term", "b": {"term": "nope"}})).await;
        assert_eq!(p.recv().await.unwrap()["err"]["status"], 404);
    }

    #[tokio::test]
    async fn device_table_changes_only_after_the_pairing_close_is_queued() {
        let hub = temp_hub();
        let code = hub.pairing.locked().issue().code;
        let key = SigningKey::from_bytes(&crypto::random());
        let dk = crypto::b64(key.verifying_key().as_bytes());
        let mut auth = hub.auth.subscribe();
        auth.mark_unchanged();
        let mut p = Phone::connect(&hub, key, ChannelKind::Relay { link: "p".into(), dk, pair: true });
        p.hello(&hub, true).await;
        p.send(json!({"id": 1, "m": "pair", "b": {"code": code, "name": "A", "platform": "ios"}})).await;
        auth.changed().await.unwrap();
        // The relay republishes the ACL on this change: the answer and 4100 must already be queued.
        let mut last = None;
        while let Ok(out) = p.rx.try_recv() {
            last = Some(out);
        }
        assert!(matches!(last, Some(Out::Close(close::PAIRED, _))), "got {last:?}");
    }

    #[tokio::test]
    async fn a_device_that_stops_reading_ends_the_channel_cleanly() {
        let hub = temp_hub();
        let key = paired_phone(&hub).await;
        let mut p = Phone::direct(&hub, key);
        p.hello(&hub, false).await;
        assert_eq!(p.call(1, "hello", json!({})).await["id"], 1);
        // The transport is gone: the next answer can't be written, so the writer ends first.
        p.rx.close();
        p.send(json!({"id": 2, "m": "hello", "b": {}})).await;
        timeout(Duration::from_secs(60), p.run).await.expect("the channel ends").expect("without a panic");
    }
}
