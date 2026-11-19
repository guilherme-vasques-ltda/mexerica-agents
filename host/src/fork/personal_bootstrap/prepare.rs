//! /personal-prepare: duas operações, sem Caller/dispatcher/Root ou posse.
use super::{
    ceremony, clock::Clock, enrollment, prepare_wire as pw, protocol, provisioning::AdmittedConfig, transport, wire,
};
use crate::{
    LockExt,
    hub::Hub,
    remote::crypto,
    store::{personal_prepare as db, personal_session::Session},
};
use anyhow::{Context, Result, ensure};
use axum::{
    body::HttpBody,
    extract::{
        Request, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use x25519_dalek::StaticSecret;
use zeroize::Zeroizing;
#[derive(Default)]
pub(super) struct Gate {
    captures: HashMap<String, Replay>,
}
struct Replay {
    boot: String,
    deadline: u64,
    nonces: HashSet<String>,
    active: bool,
}
struct Flight<'a> {
    hub: &'a Hub,
    capture: String,
}
impl Drop for Flight<'_> {
    fn drop(&mut self) {
        if let Some(c) = self.hub.personal.prepare.locked().captures.get_mut(&self.capture) {
            c.active = false;
        }
    }
}
impl<'a> Flight<'a> {
    fn enter(hub: &'a Hub, b: &Bound) -> Result<Self> {
        let (boot, now) = Clock::monotonic()?;
        let left = hub.personal.with(|f| {
            b.guard(hub, &f.config, &mut f.clock, |_, now| b.until.checked_sub(now).context("prepare expired"))
        })?;
        let mut gate = hub.personal.prepare.locked();
        gate.captures.retain(|_, r| r.active || (r.boot == boot && now < r.deadline));
        ensure!(gate.captures.values().filter(|r| r.active).count() < 16, "prepare busy");
        if !gate.captures.contains_key(&b.public) {
            ensure!(gate.captures.len() < 64, "prepare capture limit");
            gate.captures.insert(
                b.public.clone(),
                Replay {
                    boot: boot.clone(),
                    deadline: now.checked_add(left).context("prepare deadline overflow")?,
                    nonces: HashSet::new(),
                    active: false,
                },
            );
        }
        let r = gate.captures.get_mut(&b.public).context("prepare capture missing")?;
        ensure!(!r.active && r.boot == boot && now < r.deadline && r.nonces.len() < 128, "prepare busy/expired");
        r.active = true;
        Ok(Self { hub, capture: b.public.clone() })
    }
    fn nonce(&self, nonce: &str) -> Result<()> {
        let (boot, now) = Clock::monotonic()?;
        let mut gate = self.hub.personal.prepare.locked();
        let r = gate.captures.get_mut(&self.capture).context("prepare capture missing")?;
        ensure!(
            r.boot == boot && now < r.deadline && r.nonces.len() < 128 && r.nonces.insert(nonce.into()),
            "prepare replay"
        );
        Ok(())
    }
}
pub(super) struct Bound {
    pub public: String,
    pub id: String,
    pub cfg: AdmittedConfig,
    pub q: Value,
    pub intent: Option<Value>,
    pub hq: String,
    pub wire_hash: String,
    pub until: u64,
}
impl Bound {
    fn lookup(hub: &Hub, hello: &Value) -> Result<Self> {
        ensure!(
            hello.as_array().is_some_and(|a| a.len() == 10)
                && hello[0] == wire::domain("prepare-hello")
                && hello[1] == 1,
            "invalid prepare hello"
        );
        let public = pw::hash_field(&hello[3])?;
        pw::hash_field(&hello[4])?;
        pw::hash_field(&hello[5])?;
        pw::key::<32>(&hello[8])?;
        pw::key::<32>(&hello[9])?;
        hub.personal.with(|f| {
            ensure!(hello[2] == *f.config.context(), "prepare context mismatch");
            let id = hub.store.personal_prepare_lookup(&f.config, public)?;
            hub.store.personal_session(&f.config, &id, |s| {
                let q = s.originals()["local-ticket"].clone();
                let p = &q[0];
                ensure!(
                    q.as_array().is_some_and(|v| v.len() == 2)
                        && p.as_array().is_some_and(|v| v.len() == 13)
                        && p[0] == wire::domain("local-ticket")
                        && p[1] == *f.config.context()
                        && p[3] == hello[6]
                        && p[5] == f.config.identity().generation
                        && p[6] == hub.identity.computer_id()
                        && p[7] == hub.identity.sign_pub_b64()
                        && p[8] == hub.identity.box_pub_b64()
                        && p[10] == "first-owner",
                    "prepare ticket identity mismatch"
                );
                crypto::verify(&hub.identity.sign_pub(), &serde_json::to_vec(p)?, &pw::key::<64>(&q[1])?)?;
                pw::key::<32>(&p[9])?;
                let hq = wire::hash(p)?;
                let wire_hash = protocol::hash(&serde_json::to_vec(&q)?);
                ensure!(
                    hello[4] == hq && hello[5] == wire_hash && pw::capture(f.config.context(), &q)? == public,
                    "prepare capture/ticket mismatch"
                );
                let intent = s.originals().get("intent").cloned();
                let until = p[12].as_u64().context("ticket expiry")?;
                let b = Self {
                    public: public.into(),
                    id: id.clone(),
                    cfg: f.config.clone(),
                    q,
                    intent,
                    hq,
                    wire_hash,
                    until,
                };
                if hello[6] == "command" {
                    let i = b.intent.as_ref().context("command intent missing")?;
                    ensure!(hello[7] == i[0][6], "prepare A differs from I");
                } else {
                    ensure!(hello[6] == "device" && hello[7].is_null(), "device prepare has no owner A");
                }
                b.verify(hub, &f.config, &mut f.clock, s)?;
                Ok(b)
            })
        })
    }
    fn verify(&self, hub: &Hub, cfg: &AdmittedConfig, clock: &mut Clock, s: &Session<'_>) -> Result<u64> {
        cfg.matches(&hub.identity)?;
        ensure!(
            cfg.original_bytes() == self.cfg.original_bytes()
                && s.originals()["local-ticket"] == self.q
                && s.originals().get("intent") == self.intent.as_ref(),
            "prepare original binding changed"
        );
        ensure!(
            !s.closed()
                && !s.fenced()
                && ["consume", "grant", "receipt", "_sentConsume"].iter().all(|n| s.originals().get(*n).is_none()),
            "prepare already advanced/closed"
        );
        ensure!(
            !s.db.query_row(
                "SELECT ever_claimed=1 OR tombstone=1 FROM personal_generation WHERE singleton=1",
                [],
                |r| r.get::<_, bool>(0)
            )?,
            "prepare generation closed"
        );
        let now = clock.validity()?;
        let p = &self.q[0];
        let at = p[11].as_u64().context("ticket time")?;
        ensure!(
            at <= now.saturating_add(30_000) && self.until > now && self.until > at && self.until - at <= 600_000,
            "prepare ticket expired"
        );
        if p[3] == "command" {
            let c = ceremony::checked(cfg, s.originals(), now)?;
            ensure!(c.tuple()[1] == p[2] && c.tuple()[2] == "command", "prepare intent/ticket mismatch");
        }
        Ok(now)
    }
    fn guard<T>(
        &self,
        hub: &Hub,
        cfg: &AdmittedConfig,
        clock: &mut Clock,
        action: impl FnOnce(&mut Session<'_>, u64) -> Result<T>,
    ) -> Result<T> {
        hub.store.personal_session(cfg, &self.id, |s| {
            let now = self.verify(hub, cfg, clock, s)?;
            let result = action(s, now)?;
            self.verify(hub, cfg, clock, s)?;
            Ok(result)
        })
    }
    fn current(&self, hub: &Hub) -> Result<()> {
        hub.personal.with(|f| self.guard(hub, &f.config, &mut f.clock, |_, _| Ok(())))
    }
}
fn deadline(start: &(String, u64)) -> Result<()> {
    let (boot, now) = Clock::monotonic()?;
    ensure!(boot == start.0 && now.checked_sub(start.1).is_some_and(|n| n < 5000), "prepare timeout");
    Ok(())
}
pub(crate) async fn upgrade(State(hub): State<Arc<Hub>>, ws: WebSocketUpgrade, request: Request) -> Response {
    if !hub.personal.configured() {
        return StatusCode::NOT_FOUND.into_response();
    }
    if request.uri().path() != "/personal-prepare"
        || request.uri().query().is_some()
        || request.body().size_hint().exact() != Some(0)
        || ["origin", "authorization", "cookie", "sec-websocket-protocol"]
            .iter()
            .any(|h| request.headers().contains_key(*h))
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    ws.max_message_size(32772)
        .max_frame_size(32772)
        .on_upgrade(move |mut socket| async move {
            if run(&hub, &mut socket).await.is_err() {
                let _ = socket
                    .send(Message::Close(Some(axum::extract::ws::CloseFrame { code: 4003, reason: "".into() })))
                    .await;
            }
        })
        .into_response()
}
async fn receive(socket: &mut WebSocket, limit: usize) -> Result<Value> {
    match tokio::time::timeout(Duration::from_secs(5), socket.recv()).await?.context("prepare closed")?? {
        Message::Binary(b) => pw::message(&b, limit),
        _ => anyhow::bail!("prepare requires binary"),
    }
}
async fn send(socket: &mut WebSocket, value: &Value, limit: usize) -> Result<()> {
    socket.send(Message::Binary(pw::binary(value, limit)?.into())).await?;
    Ok(())
}
async fn run(hub: &Hub, socket: &mut WebSocket) -> Result<()> {
    let start = Clock::monotonic()?;
    let hello = receive(socket, 16384).await?;
    let bound = Bound::lookup(hub, &hello)?;
    let flight = Flight::enter(hub, &bound)?;
    let eph = StaticSecret::from(crypto::random::<32>());
    let welcome0 = json!([
        wire::domain("prepare-welcome"),
        1,
        hello[2],
        hello[3],
        hello[4],
        hello[5],
        crypto::b64(&crypto::x25519_pub(&eph)),
        transport::nonce()
    ]);
    let th = pw::transcript(&hello, &welcome0)?;
    bound.current(hub)?;
    deadline(&start)?;
    let mut welcome = welcome0.clone();
    welcome.as_array_mut().context("prepare welcome")?.push(json!(crypto::b64(&hub.identity.sign(&th))));
    send(socket, &welcome, 16384).await?;
    let finish = receive(socket, 16384).await?;
    deadline(&start)?;
    bound.current(hub)?;
    ensure!(
        finish.as_array().is_some_and(|v| v.len() == 4)
            && finish[0] == wire::domain("prepare-finish")
            && finish[1] == 1
            && finish[2] == crypto::b64(&th),
        "prepare finish mismatch"
    );
    if hello[6] == "command" {
        crypto::verify(
            &pw::key::<32>(&hello[7])?,
            &serde_json::to_vec(&json!([wire::domain("prepare-finish"), 1, crypto::b64(&th)]))?,
            &pw::key::<64>(&finish[3])?,
        )?;
    } else {
        ensure!(finish[3].is_null(), "device finish must not claim A");
    }
    flight.nonce(hello[9].as_str().context("prepare nonce")?)?;
    let dh = Zeroizing::new(crypto::x25519(&eph, &pw::key::<32>(&hello[8])?)?);
    let mut frames = pw::Frames::new(&hello, th, &dh)?;
    let outer = receive(socket, 32768).await?;
    let op_start = Clock::monotonic()?;
    let raw = frames.open_request(&outer)?;
    let request = protocol::canonical(&raw)?;
    request_shape(&bound, &request)?;
    let result = tokio::time::timeout(Duration::from_secs(5), operation(hub, &bound, &request)).await;
    deadline(&op_start)?;
    bound.current(hub)?;
    let reply = if let Ok(Ok(value)) = result {
        value
    } else {
        ensure!(
            request.as_array().is_some_and(|r| r.len() == 5 || r.len() == 8)
                && request[0] == wire::domain("prepare-request")
                && request[1] == 1
                && request[3] == bound.hq,
            "invalid prepare request"
        );
        pw::key::<16>(&request[2])?;
        let operation = if request[4] == "candidate" { request[5].clone() } else { Value::Null };
        json!([wire::domain("prepare-response"), 1, request[2], operation, "error", "unknown"])
    };
    send(socket, &frames.response(&reply)?, 32768).await?;
    socket.send(Message::Close(None)).await?;
    Ok(())
}
fn request_shape(b: &Bound, request: &Value) -> Result<()> {
    let a = request.as_array().context("invalid prepare request")?;
    ensure!(
        a.len() >= 5 && a[0] == wire::domain("prepare-request") && a[1] == 1 && a[3] == b.hq,
        "invalid prepare request"
    );
    pw::key::<16>(&a[2])?;
    match a[4].as_str() {
        Some("candidate") => {
            ensure!(a.len() == 8 && b.q[0][3] == "command", "candidate only command");
            pw::key::<32>(&a[7])?;
            pw::hash_field(&a[5])?;
            let digest = pw::hash_field(&a[6])?;
            let body = Zeroizing::new(
                format!("{{\"ticket\":{},\"token\":{}}}", serde_json::to_string(&b.q)?, serde_json::to_string(&a[7])?)
                    .into_bytes(),
            );
            ensure!(
                protocol::hash(&body) == digest
                    && pw::operation(b.cfg.context(), &b.public, &b.hq, &b.wire_hash, digest)? == a[5],
                "candidate body mismatch"
            );
        }
        Some("enrollment") => ensure!(a.len() == 5 && b.q[0][3] == "device", "enrollment only device"),
        _ => anyhow::bail!("prepare operation refused"),
    }
    Ok(())
}
pub(super) async fn operation(hub: &Hub, b: &Bound, request: &Value) -> Result<Value> {
    request_shape(b, request)?;
    let a = request.as_array().context("invalid prepare request")?;
    ensure!(
        a.len() >= 5 && a[0] == wire::domain("prepare-request") && a[1] == 1 && a[3] == b.hq,
        "invalid prepare request"
    );
    pw::key::<16>(&a[2])?;
    let result = match a[4].as_str() {
        Some("candidate") => {
            ensure!(a.len() == 8 && b.q[0][3] == "command", "candidate only command");
            candidate(hub, b, request).await?
        }
        Some("enrollment") => {
            ensure!(a.len() == 5 && b.q[0][3] == "device", "enrollment only device");
            enrollment_facts(hub, b)?
        }
        _ => anyhow::bail!("prepare operation refused"),
    };
    b.current(hub)?;
    Ok(json!([
        wire::domain("prepare-response"),
        1,
        a[2],
        if a[4] == "candidate" { a[5].clone() } else { Value::Null },
        "ok",
        a[4],
        result
    ]))
}
fn enrollment_facts(hub: &Hub, b: &Bound) -> Result<Value> {
    let (binding, facts, sealed) = hub.personal.with(|f| {
        b.guard(hub, &f.config, &mut f.clock, |s, _| {
            let binding = enrollment::Binding::new(&f.config, &b.q, &s.originals()["_begin"])?;
            let (facts, sealed) = db::enrollment(s.db, &b.public)?.context("begin outcome unknown")?;
            Ok((binding, facts, sealed))
        })
    })?;
    let plain = binding.open(&hub.identity, &sealed, &facts)?;
    let body = protocol::canonical(&plain)?;
    b.current(hub)?;
    Ok(json!([
        b.hq,
        b.wire_hash,
        body["intentId"],
        body["candidateId"],
        body["userCode"],
        body["createdAt"],
        body["expiresAt"],
        body["intervalMs"]
    ]))
}
async fn candidate(hub: &Hub, b: &Bound, request: &Value) -> Result<Value> {
    let token = Zeroizing::new(request[7].as_str().context("candidate token")?.to_owned());
    pw::key::<32>(&request[7])?;
    let digest = pw::hash_field(&request[6])?;
    let body = Zeroizing::new(
        format!("{{\"ticket\":{},\"token\":{}}}", serde_json::to_string(&b.q)?, serde_json::to_string(token.as_str())?)
            .into_bytes(),
    );
    ensure!(
        protocol::hash(&body) == digest
            && pw::operation(b.cfg.context(), &b.public, &b.hq, &b.wire_hash, digest)? == request[5],
        "candidate body/operation conflict"
    );
    let i = b.intent.as_ref().context("candidate original I missing")?;
    let declaration = json!([
        1,
        b.cfg.context(),
        b.cfg.identity(),
        b.hq,
        b.wire_hash,
        wire::hash(&i[0])?,
        i[0][4],
        i[0][5],
        i[0][6],
        request[2],
        request[5],
        digest
    ]);
    let status = hub
        .personal
        .with(|f| b.guard(hub, &f.config, &mut f.clock, |s, _| db::declare(s.db, &b.public, &declaration)))?;
    if status == "preparedNotSent" {
        hub.personal.with(|f| b.guard(hub, &f.config, &mut f.clock, |s, _| db::possible_send(s.db, &b.public)))?;
        let reply = transport::post_bytes(hub, "candidate", &body).await;
        match reply {
            Ok(reply) => {
                let r = &reply.value;
                ensure!(
                    r.as_object()
                        .is_some_and(|m| m.len() == 2 && m.contains_key("intentId") && m.contains_key("candidateId"))
                        && r["intentId"] == b.q[0][2]
                        && r["candidateId"] == b.q[0][4],
                    "candidate response mismatch"
                );
                hub.personal.with(|f| b.guard(hub, &f.config, &mut f.clock, |s, _| db::record(s.db, &b.public, r)))?;
            }
            Err(error) => {
                hub.personal
                    .with(|f| hub.store.personal_session(&f.config, &b.id, |s| db::unknown(s.db, &b.public)))?;
                return Err(error);
            }
        }
    } else if status == "possibleSend" || status == "unknown" {
        let Some(reply) = ceremony::read_state(hub, &b.id).await? else { anyhow::bail!("candidate read not due") };
        ensure!(
            reply["originals"]["intent"] == *i
                && reply["originals"]["ticket"] == b.q
                && reply["originals"]["grant"].is_null()
                && matches!(reply["state"][0][5].as_str(), Some("open" | "personalConfirmed")),
            "candidate not currently preparable"
        );
        hub.personal.with(|f| {
            b.guard(hub, &f.config, &mut f.clock, |s, _| {
                db::record(s.db, &b.public, &json!({"intentId":b.q[0][2],"candidateId":b.q[0][4]}))
            })
        })?;
    } else {
        ensure!(status == "recorded", "candidate closed");
    }
    b.current(hub)?;
    Ok(json!([b.hq, b.wire_hash, wire::hash(&i[0])?, b.q[0][2], b.q[0][4], digest]))
}

#[cfg(test)]
#[path = "prepare_tests.rs"]
mod tests;
