//! Posse E2EE dedicada. Não chama dispatcher, pairing, relay ou RPC de bot.
use super::{ceremony, protocol, shapes::Kind, transport, wire};
use crate::{LockExt, hub::Hub, remote::crypto, store::personal_session::Session};
use anyhow::{Context, Result, ensure};
use axum::{
    body::HttpBody,
    extract::{
        ConnectInfo, Request, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use x25519_dalek::StaticSecret;
/// Teto de sessões de posse abertas (D-2), por peer TCP real; nunca por header do cliente.
pub(super) const POSSESSION_TOTAL: usize = 16;
pub(super) const POSSESSION_PER_PEER: usize = 4;
#[derive(Default)]
pub(crate) struct Admission {
    pub(super) total: usize,
    pub(super) peers: HashMap<IpAddr, usize>,
}
/// Vaga retida pela sessão inteira; o Drop libera em toda saída (fim, erro, timeout, close, upgrade abandonado).
pub(super) struct Slot {
    hub: Arc<Hub>,
    peer: IpAddr,
}
impl Slot {
    pub(super) fn enter(hub: &Arc<Hub>, peer: IpAddr) -> Option<Self> {
        let peer = peer.to_canonical();
        let mut a = hub.personal.possession.locked();
        if a.total >= POSSESSION_TOTAL || a.peers.get(&peer).copied().unwrap_or(0) >= POSSESSION_PER_PEER {
            return None;
        }
        a.total += 1;
        *a.peers.entry(peer).or_insert(0) += 1;
        Some(Self { hub: hub.clone(), peer })
    }
}
impl Drop for Slot {
    fn drop(&mut self) {
        let mut a = self.hub.personal.possession.locked();
        a.total = a.total.saturating_sub(1);
        if let Some(n) = a.peers.get_mut(&self.peer) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                a.peers.remove(&self.peer);
            }
        }
    }
}
pub(crate) async fn upgrade(
    State(hub): State<Arc<Hub>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    ws: WebSocketUpgrade,
    request: Request,
) -> Response {
    if !hub.personal.configured() {
        return StatusCode::NOT_FOUND.into_response();
    }
    // D-1: as mesmas recusas do /personal-prepare, antes de captura, DB ou upgrade.
    if request.uri().path() != "/personal-possession"
        || request.uri().query().is_some()
        || request.body().size_hint().exact() != Some(0)
        || ["origin", "authorization", "cookie", "sec-websocket-protocol"]
            .iter()
            .any(|h| request.headers().contains_key(*h))
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    // D-2: vaga antes do upgrade e de qualquer trabalho no DB.
    let Some(slot) = Slot::enter(&hub, peer.ip()) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    ws.max_message_size(256 * 1024)
        .max_frame_size(256 * 1024)
        .on_upgrade(move |mut socket| async move {
            let _slot = slot;
            let _ = run(&hub, &mut socket).await;
        })
        .into_response()
}
async fn receive(socket: &mut WebSocket) -> Result<Value> {
    let message =
        tokio::time::timeout(Duration::from_secs(5), socket.recv()).await?.context("personal channel closed")??;
    let bytes = match message {
        Message::Text(t) => t.as_bytes().to_vec(),
        _ => anyhow::bail!("invalid possession frame"),
    };
    protocol::packet(&bytes)
}
async fn send(socket: &mut WebSocket, value: &Value) -> Result<()> {
    socket.send(Message::Text(serde_json::to_string(value)?.into())).await?;
    Ok(())
}
async fn run(hub: &Hub, socket: &mut WebSocket) -> Result<()> {
    let hello = receive(socket).await?;
    let a = hello.as_array().context("invalid possession hello")?;
    ensure!(a.len() == 8 && a[0] == wire::domain("possession-hello"), "invalid possession purpose");
    let (capture, grant) = hub.personal.with(|f| {
        ensure!(a[1] == *f.config.context(), "personal context changed");
        let capture = hub.store.personal_capture(&f.config, a[2].as_str().context("missing T")?)?;
        hub.store.personal_session(&f.config, &capture, |s| {
            let c = ceremony::checked(&f.config, s.originals(), f.clock.validity()?)?;
            let g = c.original(Kind::Grant)?;
            ensure!(
                a[2] == c.digest() && a[3] == g.digest() && a[4] == c.tuple()[7] && !s.fenced() && !s.closed(),
                "possession identity mismatch"
            );
            let input = serde_json::to_vec(&json!([a[0], a[1], a[2], a[3], a[4], a[5], a[6]]))?;
            crypto::verify(
                &crypto::unb64_n::<32>(a[4].as_str().context("invalid app key")?)?,
                &input,
                &crypto::unb64_n::<64>(a[7].as_str().context("invalid hello signature")?)?,
            )?;
            Ok((capture.clone(), g.original_bytes().to_vec()))
        })
    })?;
    let eph = StaticSecret::from(crypto::random::<32>());
    let epk = crypto::x25519_pub(&eph);
    let dev = crypto::unb64_n::<32>(a[5].as_str().context("invalid possession ephemeral")?)?;
    crypto::unb64_n::<32>(a[6].as_str().context("invalid possession nonce")?)?;
    let transcript = json!([wire::domain("possession-transcript"), hello, crypto::b64(&epk)]);
    let th = crypto::sha256(&[&serde_json::to_vec(&transcript)?]);
    let keys = crypto::channel_keys(&crypto::x25519(&eph, &dev)?, &th);
    send(
        socket,
        &json!([
            wire::domain("possession-welcome"),
            a[1],
            a[2],
            a[3],
            crypto::b64(&epk),
            crypto::b64(&th),
            crypto::b64(&hub.identity.sign(&th))
        ]),
    )
    .await?;
    let mut incoming = crypto::Opener::new(keys.d2h, 192 * 1024);
    let mut outgoing = crypto::Sealer::new(keys.h2d);
    for expected in ["challenge", "proof"] {
        let frame = receive(socket).await?;
        let raw = incoming
            .open(&frame)
            .map_err(|_| anyhow::anyhow!("invalid possession ciphertext"))?
            .context("fragmented possession request refused")?;
        let request = protocol::packet(&raw)?;
        let r = request.as_array().context("invalid possession request")?;
        ensure!(
            r.first() == Some(&json!(expected)) && r.len() == if expected == "challenge" { 2 } else { 4 },
            "possession operation refused"
        );
        ensure!(serde_json::to_vec(&r[1])? == grant, "possession grant changed");
        hub.personal.refresh()?;
        // HOST-R1-01: antes do PRIMEIRO K, consumo autenticado (consumedAt da linha D1, assinado pela raiz) vira predecessor.
        // K já existente devolve os mesmos bytes, sem novo read.
        let witness = if expected == "challenge"
            && ceremony::step(hub, &capture, |_, _, s| Ok(s.originals().get("challenge").is_none()))?
        {
            Some(ceremony::consumption_witness(hub, &capture).await?)
        } else {
            None
        };
        let (predecessor, until) = ceremony::step(hub, &capture, |cfg, clock, s| {
            let mut packet = s.originals().clone();
            if expected == "proof" {
                packet["proof"] = r[3].clone();
            }
            let c = ceremony::checked(cfg, &packet, clock.validity()?)?;
            let g = c.original(Kind::Grant)?;
            let at = if expected == "proof" {
                c.original(Kind::Proof)?.payload()[7].as_u64().context("F time")?
            } else {
                g.payload()[9].as_u64().context("G time")?
            };
            Ok((at, g.payload()[10].as_u64().context("G expiry")?))
        })?;
        let predecessor = witness.as_ref().map_or(predecessor, |w| predecessor.max(w.consumed_at));
        ceremony::wait_causal(hub, predecessor, until).await?;
        if let Some(w) = &witness {
            w.current()?;
        }
        let response = hub.personal.with(|f| {
            f.config.matches(&hub.identity)?;
            let root = f.journal.as_ref().context("root journal missing")?;
            hub.store.personal_session(&f.config, &capture, |s| {
                ensure!(!s.fenced() && !s.closed(), "personal session fenced");
                let now = f.clock.validity()?;
                let c = ceremony::checked(&f.config, s.originals(), now)?;
                root.retained(
                    &f.config,
                    &mut f.clock,
                    Kind::LocalConfirm,
                    c.original(Kind::LocalConfirm)?.original_bytes(),
                )?;
                let result = if expected == "challenge" {
                    challenge(hub, &f.config, &mut f.clock, s, witness.as_ref())?
                } else {
                    proof(hub, &f.config, &mut f.clock, s, &r[2], &r[3])?
                };
                f.clock.validity()?;
                Ok(result)
            })
        })?;
        for frame in outgoing.seal(&serde_json::to_vec(&response)?).context("possession rekey required")? {
            send(socket, &frame).await?;
        }
    }
    socket.send(Message::Close(None)).await?;
    Ok(())
}
pub(super) fn challenge(
    hub: &Hub,
    cfg: &super::provisioning::AdmittedConfig,
    clock: &mut super::clock::Clock,
    s: &mut Session<'_>,
    witness: Option<&ceremony::Witness>,
) -> Result<Value> {
    ensure!(!s.fenced() && !s.closed(), "personal fenced");
    let c = ceremony::checked(cfg, s.originals(), clock.validity()?)?;
    if let Some(old) = s.originals().get("challenge") {
        return Ok(old.clone());
    }
    // Sem testemunho, sem K: lower só emite depois de alcançar max(G.issuedAt, consumedAt), dentro do orçamento.
    let witness = witness.context("personal consumption witness required before first K")?;
    witness.current()?;
    let g = c.original(Kind::Grant)?;
    s.reserve(g.digest())?;
    let predecessor = g.payload()[9].as_u64().context("G time")?.max(witness.consumed_at);
    let now = clock.issued_after(predecessor, g.payload()[10].as_u64().context("G expiry")?)?;
    let until = g.payload()[10].as_u64().context("G expiry")?.min(now + 60_000);
    ensure!(until > now, "grant expired");
    let k = transport::signed(
        hub,
        json!([wire::domain("challenge"), cfg.context(), g.digest(), c.digest(), transport::nonce(), now, until]),
    )?;
    s.freeze("challenge", k.clone())?;
    Ok(k)
}
pub(super) fn proof(
    hub: &Hub,
    cfg: &super::provisioning::AdmittedConfig,
    clock: &mut super::clock::Clock,
    session: &mut Session<'_>,
    challenge: &Value,
    proof: &Value,
) -> Result<Value> {
    ensure!(
        !session.fenced() && !session.closed() && session.originals().get("challenge") == Some(challenge),
        "possession challenge changed"
    );
    if let Some(receipt) = session.originals().get("receipt") {
        ensure!(session.originals().get("proof") == Some(proof), "possession proof changed");
        return Ok(receipt.clone());
    }
    session.freeze("proof", proof.clone())?;
    let ceremony = ceremony::checked(cfg, session.originals(), clock.validity()?)?;
    let grant_envelope = ceremony.original(Kind::Grant)?;
    let challenge = ceremony.original(Kind::Challenge)?;
    let proof = ceremony.original(Kind::Proof)?;
    let until = grant_envelope.payload()[10].as_u64().context("grant expiry")?;
    let now = clock.issued_after(proof.payload()[7].as_u64().context("proof issuedAt")?, until)?;
    ensure!(now >= proof.payload()[7].as_u64().context("proof issuedAt")?, "proof is ahead of receipt time");
    ensure!(now < until, "grant expired before claim");
    session.claim()?;
    let grant = format!("grt_{}", crypto::b64(&crypto::random::<16>()));
    session.remember(ceremony.tuple()[7].as_str().context("app key missing")?, &grant)?;
    let (revision, version, acl) = session.acl()?;
    ensure!(clock.validity()? < until, "grant expired before commit");
    let pending = json!([
        wire::domain("pending"),
        cfg.context(),
        ceremony.digest(),
        grant,
        grant_envelope.digest(),
        challenge.digest(),
        proof.digest(),
        now,
        revision,
        "pendingPublication"
    ]);
    let receipt = transport::signed(
        hub,
        json!([
            wire::domain("receipt"),
            cfg.context(),
            ceremony.digest(),
            grant_envelope.digest(),
            challenge.digest(),
            proof.digest(),
            grant,
            now,
            revision,
            version,
            acl,
            wire::hash(&pending)?
        ]),
    )?;
    session.private("pending", pending)?;
    session.freeze("receipt", receipt.clone())?;
    ensure!(clock.validity()? < until, "grant expired during commit");
    Ok(receipt)
}
#[cfg(test)]
pub(super) fn pre_fence(
    hub: &Hub,
    cfg: &super::provisioning::AdmittedConfig,
    clock: &mut super::clock::Clock,
    s: &mut Session<'_>,
    c: &wire::Ceremony,
) -> Result<()> {
    let w = c.original(Kind::Fence)?;
    ensure!(w.payload()[4].is_null() && w.payload()[5].is_null(), "post-R fence requires complete close checkpoint");
    if s.fenced() {
        ensure!(s.originals()["fence"] == protocol::canonical(w.original_bytes())?, "personal fence revision conflict");
        return Ok(());
    }
    // Mesmo W nulo depois de R mantém Y nulo; não inventa sucesso de revogação.
    s.db.execute("UPDATE personal_generation SET revision=revision+1 WHERE singleton=1", [])?;
    s.db.execute("UPDATE personal_acl SET version=version+1 WHERE singleton=1", [])?;
    let (revision, version, acl) = s.acl()?;
    let now = clock.issued_after(w.payload()[8].as_u64().context("W time")?, 9_007_199_254_740_991)?;
    let y = transport::signed(
        hub,
        json!([
            wire::domain("fence-ack"),
            cfg.context(),
            c.digest(),
            w.digest(),
            null,
            null,
            revision,
            version,
            acl,
            now
        ]),
    )?;
    s.fence(protocol::canonical(w.original_bytes())?, y)?;
    Ok(())
}
