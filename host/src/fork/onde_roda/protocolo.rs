//! Codec compartilhado por controlador e runner; domínios independentes de PL1.
use super::tipos::{Manifest, Observation, PURPOSE};
use anyhow::{Result, anyhow, ensure};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signature, Signer as _, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest as _, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};

pub const FRAME_LIMIT: usize = 384 * 1024;
pub const PAYLOAD_LIMIT: usize = 64 * 1024;
pub fn b64(b: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(b)
}
pub fn bytes(s: &str) -> Result<Vec<u8>> {
    let b = URL_SAFE_NO_PAD.decode(s).map_err(|_| anyhow!("wire_base64"))?;
    ensure!(b64(&b) == s, "wire_base64");
    Ok(b)
}
pub fn fixed<const N: usize>(s: &str) -> Result<[u8; N]> {
    bytes(s)?.try_into().map_err(|_| anyhow!("wire_length"))
}
pub fn random<const N: usize>() -> Result<[u8; N]> {
    let mut b = [0; N];
    getrandom::fill(&mut b).map_err(|_| anyhow!("random_unavailable"))?;
    Ok(b)
}
pub fn hash(b: &[u8]) -> [u8; 32] {
    Sha256::digest(b).into()
}
pub fn fields(items: &[&[u8]]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for b in items {
        out.extend(u32::try_from(b.len())?.to_be_bytes());
        out.extend_from_slice(b);
    }
    Ok(out)
}
pub fn verify(key: &[u8; 32], msg: &[u8], sig: &str) -> Result<()> {
    VerifyingKey::from_bytes(key)
        .map_err(|_| anyhow!("wire_key"))?
        .verify_strict(msg, &Signature::from_bytes(&fixed::<64>(sig)?))
        .map_err(|_| anyhow!("wire_signature"))
}
pub fn signature(key: &SigningKey, msg: &[u8]) -> String {
    b64(&key.sign(msg).to_bytes())
}
fn id(s: &str) -> Result<()> {
    ensure!(!s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control), "wire_id");
    Ok(())
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub protocol: u32,
    pub hub_id: String,
    pub place_id: String,
    pub runner_id: String,
    pub resource_id: String,
    pub control_key: String,
    pub nonce: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Welcome {
    pub runner_key: String,
    pub nonce: String,
    #[serde(with = "uuid_wire")]
    pub boot_id: uuid::Uuid,
    pub generation: String,
    pub manifest: Manifest,
    pub role: RunnerRole,
    pub hub_id: String,
    pub resource_id: String,
    pub signature: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunnerRole {
    Runner,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Confirm {
    pub signature: String,
}
pub fn transcript(h: &Hello, w: &Welcome) -> Result<Vec<u8>> {
    ensure!(h.protocol == 1 && w.hub_id == h.hub_id && w.resource_id == h.resource_id, "wire_binding");
    for s in [&h.hub_id, &h.place_id, &h.runner_id, &h.resource_id] {
        id(s)?;
    }
    w.manifest.validate()?;
    fields(&[
        b"donna/runner/handshake/v1",
        &1_u32.to_be_bytes(),
        h.hub_id.as_bytes(),
        h.place_id.as_bytes(),
        h.runner_id.as_bytes(),
        h.resource_id.as_bytes(),
        &fixed::<32>(&h.control_key)?,
        &fixed::<32>(&w.runner_key)?,
        &fixed::<32>(&h.nonce)?,
        &fixed::<32>(&w.nonce)?,
        w.boot_id.as_bytes(),
        &fixed::<32>(&w.generation)?,
        &hash(&serde_json::to_vec(&w.manifest)?),
    ])
}
pub fn hello_input(role: Direction, channel: &[u8; 32]) -> Result<Vec<u8>> {
    fields(&[
        match role {
            Direction::Control => b"donna/runner/hello-control/v1",
            Direction::Runner => b"donna/runner/hello-runner/v1",
        },
        channel,
    ])
}
#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Control,
    Runner,
}
impl Direction {
    fn literal(self) -> &'static [u8] {
        match self {
            Self::Control => b"control",
            Self::Runner => b"runner",
        }
    }
}
#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Command {
    ChallengeRequest,
    LeaseChallenge,
    InspectGrant,
    Inspect,
    Status,
    Cancel,
    Result,
    Cancelled,
}
impl Command {
    fn literal(self) -> &'static [u8] {
        match self {
            Self::ChallengeRequest => b"challenge_request",
            Self::LeaseChallenge => b"lease_challenge",
            Self::InspectGrant => b"inspect_grant",
            Self::Inspect => b"inspect",
            Self::Status => b"status",
            Self::Cancel => b"cancel",
            Self::Result => b"result",
            Self::Cancelled => b"cancelled",
        }
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "type", deny_unknown_fields, rename_all = "snake_case")]
pub enum Payload {
    ChallengeRequest {
        #[serde(with = "uuid_wire")]
        intent: uuid::Uuid,
        admission_revision: u64,
        connection_revision: u64,
    },
    LeaseChallenge {
        #[serde(with = "uuid_wire")]
        intent: uuid::Uuid,
        nonce: String,
        #[serde(with = "uuid_wire")]
        boot_id: uuid::Uuid,
        generation: String,
        socket_probe_ready: bool,
    },
    InspectGrant {
        purpose: String,
        #[serde(with = "uuid_wire")]
        intent: uuid::Uuid,
        nonce: String,
        #[serde(with = "uuid_wire")]
        boot_id: uuid::Uuid,
        generation: String,
        duration_ms: u32,
        admission_revision: u64,
        connection_revision: u64,
    },
    Inspect {},
    Status {},
    Cancel {},
    Result {
        observation: Observation,
    },
    Cancelled {},
}
impl Payload {
    pub fn command(&self) -> Command {
        match self {
            Self::ChallengeRequest { .. } => Command::ChallengeRequest,
            Self::LeaseChallenge { .. } => Command::LeaseChallenge,
            Self::InspectGrant { .. } => Command::InspectGrant,
            Self::Inspect { .. } => Command::Inspect,
            Self::Status { .. } => Command::Status,
            Self::Cancel { .. } => Command::Cancel,
            Self::Result { .. } => Command::Result,
            Self::Cancelled { .. } => Command::Cancelled,
        }
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub version: u32,
    pub channel_id: String,
    pub direction: Direction,
    pub sequence: u64,
    #[serde(with = "uuid_wire")]
    pub operation_id: uuid::Uuid,
    pub command: Command,
    pub payload_b64: String,
    pub payload_sha256: String,
    pub signature: String,
}
impl Envelope {
    pub fn input(&self) -> Result<Vec<u8>> {
        fields(&[
            b"donna/runner/message/v1",
            &self.version.to_be_bytes(),
            self.direction.literal(),
            &fixed::<32>(&self.channel_id)?,
            &self.sequence.to_be_bytes(),
            self.operation_id.as_bytes(),
            self.command.literal(),
            &fixed::<32>(&self.payload_sha256)?,
        ])
    }
}
pub struct Channel {
    pub id: [u8; 32],
    pub direction: Direction,
    pub key: SigningKey,
    pub peer: [u8; 32],
    sent: u64,
    received: u64,
}
impl Channel {
    pub fn new(id: [u8; 32], direction: Direction, key: SigningKey, peer: [u8; 32]) -> Self {
        Self { id, direction, key, peer, sent: 0, received: 0 }
    }
    pub fn seal(&mut self, operation: uuid::Uuid, payload: &Payload) -> Result<Envelope> {
        let data = serde_json::to_vec(payload)?;
        ensure!(data.len() <= PAYLOAD_LIMIT, "wire_payload_limit");
        self.sent = self.sent.checked_add(1).ok_or_else(|| anyhow!("wire_sequence"))?;
        let mut e = Envelope {
            version: 1,
            channel_id: b64(&self.id),
            direction: self.direction,
            sequence: self.sent,
            operation_id: operation,
            command: payload.command(),
            payload_b64: b64(&data),
            payload_sha256: b64(&hash(&data)),
            signature: String::new(),
        };
        e.signature = signature(&self.key, &e.input()?);
        Ok(e)
    }
    pub fn open(&mut self, e: &Envelope, op: uuid::Uuid) -> Result<Payload> {
        ensure!(
            e.version == 1
                && fixed::<32>(&e.channel_id)? == self.id
                && e.direction != self.direction
                && e.operation_id == op
                && self.received.checked_add(1) == Some(e.sequence),
            "wire_fence"
        );
        ensure!(e.payload_b64.len() <= PAYLOAD_LIMIT.div_ceil(3) * 4, "wire_payload_limit");
        let data = bytes(&e.payload_b64)?;
        ensure!(data.len() <= PAYLOAD_LIMIT && hash(&data) == fixed::<32>(&e.payload_sha256)?, "wire_payload_hash");
        verify(&self.peer, &e.input()?, &e.signature)?;
        let p: Payload = serde_json::from_slice(&data).map_err(|_| anyhow!("wire_payload_invalid"))?;
        ensure!(p.command() == e.command, "wire_command");
        self.received = e.sequence;
        Ok(p)
    }
}
pub async fn read<R: AsyncRead + Unpin, T: DeserializeOwned>(r: &mut R) -> Result<T> {
    let n = usize::try_from(r.read_u32().await?)?;
    ensure!((1..=FRAME_LIMIT).contains(&n), "wire_frame_limit");
    let mut b = vec![0; n];
    r.read_exact(&mut b).await?;
    serde_json::from_slice(&b).map_err(|_| anyhow!("wire_frame_invalid"))
}
pub async fn write<W: AsyncWrite + Unpin, T: Serialize>(w: &mut W, v: &T) -> Result<()> {
    let b = serde_json::to_vec(v)?;
    ensure!(b.len() <= FRAME_LIMIT, "wire_frame_limit");
    w.write_u32(u32::try_from(b.len())?).await?;
    w.write_all(&b).await?;
    w.flush().await?;
    Ok(())
}

/// O prazo começa na emissão local. Desafio é consumido mesmo em grant inválido.
pub struct Challenge {
    pub op: uuid::Uuid,
    pub intent: uuid::Uuid,
    pub nonce: String,
    pub boot: uuid::Uuid,
    pub generation: String,
    pub admission: u64,
    pub connection: u64,
    issued: std::time::Instant,
    wall: std::time::SystemTime,
    boottime: std::time::Duration,
    used: bool,
}
pub struct Lease {
    issued: std::time::Instant,
    wall: std::time::SystemTime,
    boottime: std::time::Duration,
    duration: std::time::Duration,
}
impl Lease {
    pub fn remaining(&self) -> Result<std::time::Duration> {
        let elapsed = boottime()?.checked_sub(self.boottime).ok_or_else(|| anyhow!("clock_discontinuity"))?;
        let wall = self.wall.elapsed().map_err(|_| anyhow!("clock_discontinuity"))?;
        ensure!(
            elapsed.abs_diff(wall) <= std::time::Duration::from_secs(5)
                && elapsed.abs_diff(self.issued.elapsed()) <= std::time::Duration::from_secs(5),
            "clock_discontinuity"
        );
        self.duration.checked_sub(elapsed).filter(|r| !r.is_zero()).ok_or_else(|| anyhow!("inspect_expired"))
    }
}
impl Challenge {
    pub fn issue(
        op: uuid::Uuid,
        intent: uuid::Uuid,
        boot: uuid::Uuid,
        generation: String,
        admission: u64,
        connection: u64,
    ) -> Result<Self> {
        Ok(Self {
            op,
            intent,
            nonce: b64(&random::<32>()?),
            boot,
            generation,
            admission,
            connection,
            issued: std::time::Instant::now(),
            wall: std::time::SystemTime::now(),
            boottime: boottime()?,
            used: false,
        })
    }
    pub fn grant(&mut self, op: uuid::Uuid, payload: &Payload) -> Result<Lease> {
        ensure!(!self.used, "challenge_used");
        self.used = true;
        let Payload::InspectGrant {
            purpose,
            intent,
            nonce,
            boot_id,
            generation,
            duration_ms,
            admission_revision,
            connection_revision,
        } = payload
        else {
            anyhow::bail!("inspect_grant_required")
        };
        let elapsed = boottime()?.checked_sub(self.boottime).ok_or_else(|| anyhow!("clock_discontinuity"))?;
        let wall = self.wall.elapsed().map_err(|_| anyhow!("clock_discontinuity"))?;
        ensure!(
            elapsed.abs_diff(wall) <= std::time::Duration::from_secs(5)
                && elapsed.abs_diff(self.issued.elapsed()) <= std::time::Duration::from_secs(5),
            "clock_discontinuity"
        );
        ensure!(
            purpose == PURPOSE
                && self.op == op
                && self.intent == *intent
                && self.nonce == *nonce
                && self.boot == *boot_id
                && self.generation == *generation
                && self.admission == *admission_revision
                && self.connection == *connection_revision
                && (1..=60_000).contains(duration_ms)
                && elapsed < std::time::Duration::from_millis(u64::from(*duration_ms)),
            "challenge_invalid"
        );
        Ok(Lease {
            issued: self.issued,
            wall: self.wall,
            boottime: self.boottime,
            duration: std::time::Duration::from_millis(u64::from(*duration_ms)),
        })
    }
}

/// Adaptador local: o pacote uuid existente não habilita serde.
fn boottime() -> Result<std::time::Duration> {
    #[cfg(target_os = "linux")]
    {
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        // SAFETY: ponteiro válido para timespec; clock_gettime não retém o ponteiro.
        ensure!(unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &raw mut ts) } == 0, "clock_unavailable");
        Ok(std::time::Duration::new(u64::try_from(ts.tv_sec)?, u32::try_from(ts.tv_nsec)?))
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?)
    }
}

pub mod uuid_wire {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(id: &uuid::Uuid, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&id.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<uuid::Uuid, D::Error> {
        let s = String::deserialize(d)?;
        let id = uuid::Uuid::parse_str(&s).map_err(serde::de::Error::custom)?;
        if id.to_string() != s {
            return Err(serde::de::Error::custom("noncanonical uuid"));
        }
        Ok(id)
    }
}
