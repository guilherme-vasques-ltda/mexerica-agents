//! Verificadores PL1 puros. As chaves vêm do consumidor admitido, nunca do envelope.
use super::{protocol, shapes::Kind};
use crate::remote::crypto;
use anyhow::{Result, anyhow, ensure};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use hkdf::Hkdf;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use x25519_dalek::StaticSecret;
use zeroize::Zeroizing;

pub(super) fn domain(name: &str) -> String {
    format!("donna/personal-linux/{name}/v1")
}
pub(super) fn hash(v: &Value) -> Result<String> {
    Ok(protocol::hash(&serde_json::to_vec(v)?))
}
#[derive(Clone)]
pub(super) struct Keys {
    pub issuer: String,
    pub app: String,
    pub hub: String,
    pub box_key: String,
}
#[derive(Clone, Copy)]
pub(super) enum Time {
    Current(u64),
    Committed(u64),
    Historical(u64),
}
impl Time {
    fn at(self) -> u64 {
        match self {
            Self::Current(n) | Self::Committed(n) | Self::Historical(n) => n,
        }
    }
}
#[derive(Clone)]
pub(super) struct Signed {
    p: Value,
    wire: Vec<u8>,
    hash: String,
}
impl Signed {
    pub(super) fn payload(&self) -> &Value {
        &self.p
    }
    pub(super) fn original_bytes(&self) -> &[u8] {
        &self.wire
    }
    pub(super) fn digest(&self) -> &str {
        &self.hash
    }
}
#[derive(Clone, Copy)]
pub(super) enum Reader {
    App,
    Hub,
}
pub(super) struct OpenRequest<'a> {
    pub role: Reader,
    pub kid: &'a str,
    pub nonce: &'a str,
    pub path: &'a str,
    pub until: u64,
}
pub(super) struct Verifier {
    context: Value,
    keys: Keys,
}
fn number(v: &Value) -> Result<u64> {
    v.as_u64().filter(|n| *n > 0 && *n <= 9_007_199_254_740_991).ok_or_else(|| anyhow!("invalid personal integer"))
}
fn string(v: &Value) -> Result<&str> {
    v.as_str().ok_or_else(|| anyhow!("invalid personal string"))
}
fn binary<const N: usize>(v: &Value) -> Result<[u8; N]> {
    let s = string(v)?;
    let raw = crypto::unb64_n::<N>(s)?;
    ensure!(crypto::b64(&raw) == s, "noncanonical personal key");
    Ok(raw)
}
fn field(token: &str, v: &Value, c: &Value) -> bool {
    if let Some(t) = token.strip_prefix('?') {
        return v.is_null() || field(t, v, c);
    }
    if let Some(literal) = token.strip_prefix('=') {
        return v == literal;
    }
    match token {
        "C" => v == c,
        "T" => v.as_array().is_some_and(|a| {
            a.len() == 19
                && &a[0] == c
                && "id mode hash hash id id key key id key cid key key key =first-owner int int int"
                    .split(' ')
                    .enumerate()
                    .all(|(i, t)| field(t, &a[i + 1], c))
                && number(&a[18]).is_ok_and(|end| number(&a[17]).is_ok_and(|at| end > at && end - at <= 600_000))
        }),
        "id" => v.as_str().is_some_and(|s| {
            !s.is_empty() && s.len() <= 256 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_./:-".contains(&b))
        }),
        "grant" => v.as_str().and_then(|s| s.strip_prefix("grt_")).is_some_and(|s| binary::<16>(&json!(s)).is_ok()),
        "key" => binary::<32>(v).is_ok(),
        "cid" | "nonce16" => binary::<16>(v).is_ok(),
        "hash" => v
            .as_str()
            .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))),
        "cipher" => v
            .as_str()
            .is_some_and(|s| crypto::unb64(s).is_ok_and(|b| (16..=16384).contains(&b.len()) && crypto::b64(&b) == s)),
        "int" => number(v).is_ok(),
        "zero" => v == 0,
        "caps" => v == &json!(["control", "owner.devices"]),
        "mode" => v == "command" || v == "device",
        "role" => v == "host" || v == "device",
        "path" => v == "/v1/personal-relay/host" || v == "/v1/personal-relay/device",
        "reason" => ["ownerRevoked", "relink", "issuerRevoked"].iter().any(|x| v == *x),
        "phase" => [
            "open",
            "personalConfirmed",
            "hubPending",
            "published",
            "hubActive",
            "canceled",
            "expired",
            "refused",
            "revoking",
            "revoked",
        ]
        .iter()
        .any(|x| v == *x),
        _ => false,
    }
}
impl Verifier {
    pub(super) fn new(context: Value, keys: Keys) -> Result<Self> {
        for s in [&keys.issuer, &keys.app, &keys.hub, &keys.box_key] {
            binary::<32>(&json!(s))?;
        }
        ensure!(keys.issuer != keys.hub && keys.app != keys.hub, "personal role collision");
        Ok(Self { context, keys })
    }
    pub(super) fn verify(&self, kind: Kind, raw: &[u8], time: Time, reader: Reader) -> Result<Signed> {
        let value = protocol::canonical(raw)?;
        let a = value.as_array().ok_or_else(|| anyhow!("invalid personal envelope"))?;
        ensure!(a.len() == 2, "invalid personal envelope length");
        let p = &a[0];
        let (name, role, fields, at, end, max) = kind.shape();
        let tokens: Vec<&str> = fields.split(' ').collect();
        ensure!(
            p.as_array().is_some_and(|p| p.len() == tokens.len() + 2) && p[0] == domain(name) && p[1] == self.context,
            "invalid personal domain/context/length"
        );
        ensure!(tokens.iter().enumerate().all(|(i, t)| field(t, &p[i + 2], &self.context)), "invalid personal fields");
        let key = match role {
            "E" => &self.keys.issuer,
            "A" => &self.keys.app,
            "H" => &self.keys.hub,
            "reader" => {
                if matches!(reader, Reader::Hub) {
                    &self.keys.hub
                } else {
                    &self.keys.app
                }
            }
            "channel" => {
                if p[2] == "host" {
                    &self.keys.hub
                } else {
                    &self.keys.app
                }
            }
            _ => return Err(anyhow!("invalid signer role")),
        };
        crypto::verify(&binary::<32>(&json!(key))?, &serde_json::to_vec(p)?, &binary::<64>(&a[1])?)?;
        let now = time.at();
        ensure!(now > 0 && now <= 9_007_199_254_740_991, "invalid personal time");
        if max > 0 {
            let start = number(&p[at])?;
            let until = number(&p[end])?;
            ensure!(
                start <= now.saturating_add(30_000)
                    && (matches!(time, Time::Historical(_)) || until > now)
                    && until > start
                    && until - start <= max,
                "expired personal envelope"
            );
        }
        let computer = crypto::computer_id(&binary::<32>(&json!(self.keys.hub))?);
        match kind {
            Kind::LocalTicket => {
                ensure!(
                    p[7] == self.keys.hub && p[8] == self.keys.box_key && p[6] == computer,
                    "personal Hub mismatch"
                );
            }
            Kind::Intent => ensure!(p[6] == self.keys.app, "personal app mismatch"),
            Kind::ChannelOpen => ensure!(
                p[6] != Value::Null
                    && p[7] == self.keys.app
                    && p[4] == computer
                    && p[9] == format!("/v1/personal-relay/{}", string(&p[2])?),
                "invalid personal use"
            ),
            Kind::HostStateRequest | Kind::Fence | Kind::FenceAck => {
                ensure!(p[4].is_null() == p[5].is_null(), "incomplete personal reference");
            }
            Kind::Time => ensure!(
                number(&p[5])? <= 300_000 && number(&p[4])? <= now.saturating_add(30_000) && p[3] == computer,
                "invalid time reference"
            ),
            Kind::TimeRequest => ensure!(p[3] == computer, "invalid time computer"),
            _ => (),
        }
        let observed = match kind {
            Kind::Receipt => Some(7),
            Kind::Fence | Kind::Revocation => Some(8),
            Kind::FenceAck => Some(9),
            Kind::State => Some(15),
            _ => None,
        };
        if let Some(i) = observed {
            ensure!(number(&p[i])? <= now.saturating_add(30_000), "future personal observation");
        }
        Ok(Signed { p: p.clone(), wire: raw.to_vec(), hash: hash(p)? })
    }
    pub(super) fn verify_open(&self, raw: &[u8], time: Time, request: &OpenRequest<'_>) -> Result<Signed> {
        ensure!(matches!(time, Time::Current(_)), "historical open is not admission");
        let s = self.verify(Kind::ChannelOpen, raw, time, Reader::App)?;
        let (role, key) = match request.role {
            Reader::Hub => ("host", &self.keys.hub),
            Reader::App => ("device", &self.keys.app),
        };
        ensure!(
            request.kid == key.as_str()
                && s.p[2] == role
                && s.p[8] == request.nonce
                && s.p[9] == request.path
                && number(&s.p[12])? <= request.until,
            "personal HTTP open mismatch"
        );
        Ok(s)
    }
    pub(super) fn tuple(&self, intent: &Signed, ticket: &Signed) -> Result<Value> {
        let intent_payload = &intent.p;
        let ticket_payload = &ticket.p;
        ensure!(
            intent_payload[0] == domain("intent")
                && ticket_payload[0] == domain("local-ticket")
                && intent_payload[2] == ticket_payload[2]
                && intent_payload[3] == ticket_payload[3],
            "personal tuple mismatch"
        );
        let tuple = json!([
            self.context,
            intent_payload[2],
            intent_payload[3],
            intent.hash,
            ticket.hash,
            intent_payload[4],
            intent_payload[5],
            intent_payload[6],
            intent_payload[7],
            ticket_payload[4],
            ticket_payload[5],
            ticket_payload[6],
            ticket_payload[7],
            ticket_payload[8],
            ticket_payload[9],
            "first-owner",
            intent_payload[10],
            intent_payload[8],
            number(&intent_payload[9])?.min(number(&ticket_payload[12])?)
        ]);
        ensure!(field("T", &tuple, &self.context), "invalid derived personal tuple");
        Ok(tuple)
    }
}

pub(super) fn box_open(
    hub_secret: &StaticSecret,
    b: &Signed,
    h_t: &str,
    app_consent_hash: &str,
    local_authorization_hash: &str,
) -> Result<Zeroizing<String>> {
    let p = &b.p;
    ensure!(
        p[0] == domain("box-challenge") && p[2] == h_t && p[3] == app_consent_hash && p[4] == local_authorization_hash,
        "personal POP binding mismatch"
    );
    let aad = serde_json::to_vec(&json!([domain("box-aad"), p[1], p[2], p[3], p[4], p[5], p[6], p[7], p[9], p[10]]))?;
    let dh = Zeroizing::new(crypto::x25519(hub_secret, &binary::<32>(&p[6])?)?);
    let hk = Hkdf::<Sha256>::new(Some(&Sha256::digest(&aad)), dh.as_ref());
    let mut key = Zeroizing::new([0u8; 32]);
    hk.expand(domain("box-key").as_bytes(), key.as_mut()).map_err(|_| anyhow!("personal KDF failed"))?;
    let cipher = ChaCha20Poly1305::new_from_slice(key.as_ref()).map_err(|_| anyhow!("personal cipher failed"))?;
    let ciphertext = crypto::unb64(string(&p[8])?)?;
    let plain = Zeroizing::new(
        cipher
            .decrypt(&Nonce::from([0u8; 12]), Payload { msg: &ciphertext, aad: &aad })
            .map_err(|_| anyhow!("personal POP failed"))?,
    );
    let value = protocol::canonical(&plain)?;
    ensure!(
        value.as_array().is_some_and(|a| a.len() == 3)
            && value[0] == domain("box-secret-plaintext")
            && value[1] == h_t
            && binary::<32>(&value[2]).is_ok()
            && hash(&json!([domain("box-secret-hash"), value[2]]))? == p[7],
        "personal POP plaintext mismatch"
    );
    Ok(Zeroizing::new(string(&value[2])?.to_owned()))
}

pub(super) struct Ceremony {
    tuple: Value,
    hash: String,
    originals: BTreeMap<Kind, Signed>,
}
impl Ceremony {
    pub(super) fn tuple(&self) -> &Value {
        &self.tuple
    }
    pub(super) fn digest(&self) -> &str {
        &self.hash
    }
    pub(super) fn original(&self, kind: Kind) -> Result<&Signed> {
        get(&self.originals, kind)
    }
}
fn get(o: &BTreeMap<Kind, Signed>, k: Kind) -> Result<&Signed> {
    o.get(&k).ok_or_else(|| anyhow!("missing personal antecedent"))
}
fn eq(o: &BTreeMap<Kind, Signed>, k: Kind, index: usize, value: &Value) -> Result<()> {
    ensure!(&get(o, k)?.p[index] == value, "personal antecedent mismatch");
    Ok(())
}
fn preceding(originals: &BTreeMap<Kind, Signed>, tuple: &Value, kind: Kind, previous: &[Kind]) -> Result<()> {
    let payload = &get(originals, kind)?.p;
    let (_, _, _, at, end, _) = kind.shape();
    ensure!(
        number(&payload[at])? >= number(&tuple[17])? && number(&payload[end])? <= number(&tuple[18])?,
        "personal tuple time mismatch"
    );
    for prev in previous {
        let old = &get(originals, *prev)?.p;
        let (_, _, _, start_index, end_index, _) = prev.shape();
        ensure!(
            number(&payload[at])? >= number(&old[start_index])? && number(&payload[end])? <= number(&old[end_index])?,
            "personal order/window mismatch"
        );
    }
    Ok(())
}

type AntecedentLinks<'a> = (Kind, &'a [(usize, Kind)], &'a [Kind]);

pub(super) fn ceremony(v: &Verifier, raw: &[u8], time: Time) -> Result<Ceremony> {
    use Kind::{
        Activation, AppConsent, Approve, Attach, Begin, BoxChallenge, BoxProof, Cancel, Challenge, ChannelOpen,
        Consume, Create, Fence, FenceAck, Grant, HostState, HostStateRequest, Intent, LocalConfirm, LocalConsent,
        LocalTicket, Proof, Publish, Read, Receipt, Revocation, Revoke, State, Time, TimeRequest,
    };
    let input = protocol::packet(raw)?;
    let map = input.as_object().ok_or_else(|| anyhow!("invalid personal packet"))?;
    let mut o = BTreeMap::new();
    let mut grant_time = time;
    if let Some(r) = map.get("receipt") {
        let r = v.verify(Receipt, &serde_json::to_vec(r)?, time, Reader::App)?;
        grant_time = self::Time::Committed(number(&r.p[7])?);
        o.insert(Receipt, r);
    }
    if let Some(g) = map.get("grant") {
        o.insert(Grant, v.verify(Grant, &serde_json::to_vec(g)?, grant_time, Reader::App)?);
    }
    let prep_time = if let Some(g) = o.get(&Grant) { self::Time::Committed(number(&g.p[9])?) } else { time };
    for (name, wire) in map {
        let k = Kind::parse(name).ok_or_else(|| anyhow!("unknown personal envelope"))?;
        if o.contains_key(&k) {
            continue;
        }
        let at = if matches!(
            k,
            Intent
                | LocalTicket
                | AppConsent
                | LocalConsent
                | BoxChallenge
                | BoxProof
                | Approve
                | LocalConfirm
                | Consume
        ) {
            prep_time
        } else if matches!(k, Challenge | Proof) {
            grant_time
        } else {
            time
        };
        o.insert(k, v.verify(k, &serde_json::to_vec(wire)?, at, Reader::App)?);
    }
    let t = v.tuple(get(&o, Intent)?, get(&o, LocalTicket)?)?;
    let h_t = hash(&t)?;
    for k in [AppConsent, LocalConsent, BoxChallenge, BoxProof, Approve, LocalConfirm, Consume] {
        if o.contains_key(&k) {
            eq(&o, k, 2, &json!(h_t))?;
        }
    }
    let links: &[AntecedentLinks<'_>] = &[
        (AppConsent, &[], &[]),
        (LocalConsent, &[(3, AppConsent)], &[AppConsent]),
        (BoxChallenge, &[(3, AppConsent), (4, LocalConsent)], &[AppConsent, LocalConsent]),
        (BoxProof, &[(3, BoxChallenge)], &[BoxChallenge]),
        (Approve, &[(3, BoxChallenge), (4, BoxProof)], &[BoxChallenge, BoxProof]),
        (LocalConfirm, &[(3, Approve), (4, BoxProof)], &[Approve, BoxProof, LocalConsent]),
        (Consume, &[(3, Approve), (4, LocalConfirm)], &[Approve, LocalConfirm]),
    ];
    for (k, refs, old) in links {
        if o.contains_key(k) {
            for (i, source) in *refs {
                eq(&o, *k, *i, &json!(get(&o, *source)?.hash))?;
            }
            preceding(&o, &t, *k, old)?;
        }
    }
    if o.contains_key(&BoxProof) {
        eq(&o, BoxChallenge, 7, &json!(hash(&json!([domain("box-secret-hash"), get(&o, BoxProof)?.p[4]]))?))?;
    }
    if o.contains_key(&Consume) {
        eq(&o, Consume, 5, &t[16])?;
    }
    if let Some(g) = o.get(&Grant) {
        eq(&o, Grant, 2, &t)?;
        for (i, k) in [BoxChallenge, BoxProof, Approve, LocalConfirm, Consume].iter().enumerate() {
            eq(&o, Grant, i + 3, &json!(get(&o, *k)?.hash))?;
        }
        ensure!(
            number(&g.p[9])? >= number(&get(&o, Consume)?.p[7])? && number(&g.p[10])? <= number(&t[18])?,
            "invalid grant time"
        );
        if o.contains_key(&Challenge) {
            eq(&o, Challenge, 2, &json!(g.hash))?;
            eq(&o, Challenge, 3, &json!(h_t))?;
            preceding(&o, &t, Challenge, &[Grant])?;
        }
        if o.contains_key(&Proof) {
            eq(&o, Proof, 2, &json!(g.hash))?;
            eq(&o, Proof, 3, &json!(get(&o, Challenge)?.hash))?;
            eq(&o, Proof, 4, &json!(h_t))?;
            eq(&o, Proof, 5, &json!(get(&o, Approve)?.hash))?;
            eq(&o, Proof, 6, &json!(get(&o, LocalConfirm)?.hash))?;
            preceding(&o, &t, Proof, &[Challenge])?;
        }
        if let Some(r) = o.get(&Receipt) {
            for (i, k) in [(3, Grant), (4, Challenge), (5, Proof)] {
                eq(&o, Receipt, i, &json!(get(&o, k)?.hash))?;
            }
            eq(&o, Receipt, 2, &json!(h_t))?;
            ensure!(
                number(&r.p[7])? >= number(&get(&o, Proof)?.p[7])? && number(&r.p[7])? < number(&g.p[10])?,
                "invalid receipt commit"
            );
            eq(
                &o,
                Receipt,
                11,
                &json!(hash(&json!([
                    domain("pending"),
                    t[0],
                    h_t,
                    r.p[6],
                    g.hash,
                    get(&o, Challenge)?.hash,
                    get(&o, Proof)?.hash,
                    r.p[7],
                    r.p[8],
                    "pendingPublication"
                ]))?),
            )?;
        }
    }
    for k in [Activation, HostState] {
        if let Some(s) = o.get(&k) {
            eq(&o, k, 2, &json!(h_t))?;
            let r = get(&o, Receipt)?;
            if k == Activation {
                eq(&o, k, 3, &r.p[6])?;
                eq(&o, k, 4, &json!(r.hash))?;
                for (i, j) in [(5, 5), (6, 6), (7, 7), (8, 11), (9, 10)] {
                    eq(&o, k, i, &t[j])?;
                }
                ensure!(
                    number(&s.p[10])? > number(&r.p[8])?
                        && number(&s.p[11])? > number(&r.p[9])?
                        && number(&s.p[13])? >= number(&r.p[7])?,
                    "invalid activation revision"
                );
            } else {
                eq(&o, k, 3, &json!(get(&o, Grant)?.hash))?;
                eq(&o, k, 4, &r.p[6])?;
                eq(&o, k, 5, &json!(r.hash))?;
                eq(&o, k, 6, &json!(get(&o, Publish)?.hash))?;
                for (i, j) in [(7, 5), (8, 6), (9, 7), (10, 11), (11, 10), (13, 16)] {
                    eq(&o, k, i, &t[j])?;
                }
                eq(&o, k, 15, &get(&o, HostStateRequest)?.p[6])?;
                ensure!(
                    number(&s.p[16])? >= number(&get(&o, HostStateRequest)?.p[7])?
                        && number(&s.p[16])? < number(&get(&o, HostStateRequest)?.p[8])?
                        && number(&s.p[16])? >= number(&r.p[7])?,
                    "snapshot outside request"
                );
            }
        }
    }
    if o.contains_key(&Publish) {
        eq(&o, Publish, 2, &t[1])?;
        eq(&o, Publish, 3, &json!(h_t))?;
        eq(&o, Publish, 4, &json!(get(&o, Grant)?.hash))?;
        eq(&o, Publish, 5, &get(&o, Receipt)?.p[6])?;
        eq(&o, Publish, 6, &json!(get(&o, Receipt)?.hash))?;
    }
    if o.contains_key(&HostStateRequest) {
        for (i, val) in [
            (2, json!(h_t)),
            (3, json!(get(&o, Grant)?.hash)),
            (4, get(&o, Receipt)?.p[6].clone()),
            (5, json!(get(&o, Receipt)?.hash)),
        ] {
            eq(&o, HostStateRequest, i, &val)?;
        }
    }
    if o.contains_key(&ChannelOpen) {
        for (i, val) in [(4, t[11].clone()), (5, json!(h_t)), (6, get(&o, Receipt)?.p[6].clone()), (7, t[7].clone())] {
            eq(&o, ChannelOpen, i, &val)?;
        }
    }
    if let (Some(a), Some(e)) = (o.get(&Activation), o.get(&HostState)) {
        ensure!(
            number(&a.p[13])? >= number(&e.p[16])? && number(&a.p[14])? <= number(&e.p[17])?,
            "activation exceeds snapshot"
        );
    }
    for k in [Revoke, Fence, FenceAck, Revocation] {
        if let Some(s) = o.get(&k) {
            if k == Revoke {
                eq(&o, k, 2, &t[1])?;
                eq(&o, k, 3, &json!(h_t))?;
                eq(&o, k, 4, &json!(get(&o, Grant)?.hash))?;
            } else {
                eq(&o, k, 2, &json!(h_t))?;
                if k == Fence {
                    eq(&o, k, 3, &json!(get(&o, Grant)?.hash))?;
                }
                if k == FenceAck {
                    eq(&o, k, 3, &json!(get(&o, Fence)?.hash))?;
                }
                if k == Revocation {
                    eq(&o, k, 3, &get(&o, Receipt)?.p[6])?;
                    eq(&o, k, 4, &json!(get(&o, Receipt)?.hash))?;
                    eq(&o, k, 9, &get(&o, Fence)?.p[7])?;
                } else if !s.p[4].is_null() {
                    eq(&o, k, 4, &get(&o, Receipt)?.p[6])?;
                    eq(&o, k, 5, &json!(get(&o, Receipt)?.hash))?;
                }
            }
        }
    }
    for k in [Create, Begin, Attach, Cancel, Read, State] {
        if let Some(s) = o.get(&k) {
            eq(&o, k, 2, &t[1])?;
            match k {
                Create => {
                    eq(&o, k, 3, &t[8])?;
                    ensure!(t[2] == "command", "wrong personal mode");
                }
                Begin => {
                    eq(&o, k, 3, &t[9])?;
                    eq(&o, k, 4, &json!(get(&o, LocalTicket)?.hash))?;
                    eq(&o, k, 5, &t[14])?;
                    ensure!(t[2] == "device", "wrong personal mode");
                }
                Attach => {
                    eq(&o, k, 3, &json!(get(&o, LocalTicket)?.hash))?;
                    eq(&o, k, 4, &t[8])?;
                    ensure!(t[2] == "device", "wrong personal mode");
                }
                Cancel => eq(&o, k, 3, &t[16])?,
                Read | State => {
                    eq(&o, k, 3, &t[9])?;
                    eq(&o, k, 4, &json!(h_t))?;
                }
                _ => (),
            }
            if k == State {
                for (i, source) in
                    [(8, Grant), (9, Receipt), (10, Activation), (11, Revocation), (12, Fence), (13, FenceAck)]
                {
                    if !s.p[i].is_null() {
                        eq(&o, k, i, &json!(get(&o, source)?.hash))?;
                    }
                }
                if !s.p[7].is_null() {
                    eq(&o, k, 7, &get(&o, Receipt)?.p[6])?;
                }
                if let Some(read) = o.get(&Read) {
                    eq(&o, k, 14, &read.p[5])?;
                }
            }
        }
    }
    if let (Some(_), Some(r)) = (o.get(&Time), o.get(&TimeRequest)) {
        eq(&o, Time, 2, &r.p[2])?;
        eq(&o, Time, 3, &r.p[3])?;
    }
    if let Some(y) = o.get(&FenceAck) {
        eq(&o, FenceAck, 4, &get(&o, Fence)?.p[4])?;
        eq(&o, FenceAck, 5, &get(&o, Fence)?.p[5])?;
        ensure!(number(&y.p[9])? >= number(&get(&o, Fence)?.p[8])?, "ack before fence");
        if let Some(r) = o.get(&Receipt) {
            ensure!(number(&y.p[6])? > number(&r.p[8])? && number(&y.p[7])? > number(&r.p[9])?, "stale ack revision");
        }
    }
    if let Some(x) = o.get(&Revocation) {
        let r = get(&o, Receipt)?;
        ensure!(
            number(&x.p[8])? >= number(&get(&o, Fence)?.p[8])?
                && number(&x.p[5])? > number(&r.p[8])?
                && number(&x.p[6])? > number(&r.p[9])?,
            "stale revocation revision"
        );
        if let Some(y) = o.get(&FenceAck) {
            for (a, b) in [(5, 6), (6, 7), (7, 8)] {
                eq(&o, Revocation, a, &y.p[b])?;
            }
        }
    }
    Ok(Ceremony { tuple: t, hash: h_t, originals: o })
}

// Fronteira C3a: time inicial não tem A/T nem epoch local qualificado.
// Estes ingressos fechados não usam Verifier::new ou Time artificial.
fn worker_message(kind: Kind, raw: &[u8], context: &Value, key: &str) -> Result<Signed> {
    ensure!(
        matches!(kind, Kind::Time | Kind::TimeRequest | Kind::HostState | Kind::ConsumptionWitness),
        "unsupported clock message"
    );
    let value = protocol::canonical(raw)?;
    ensure!(value.as_array().is_some_and(|a| a.len() == 2), "invalid clock envelope");
    let p = &value[0];
    let (name, _, fields, _, _, _) = kind.shape();
    let tokens: Vec<_> = fields.split(' ').collect();
    ensure!(
        p.as_array().is_some_and(|a| a.len() == tokens.len() + 2)
            && p[0] == domain(name)
            && p[1] == *context
            && tokens.iter().enumerate().all(|(i, t)| field(t, &p[i + 2], context)),
        "invalid clock message"
    );
    crypto::verify(&binary::<32>(&json!(key))?, &serde_json::to_vec(p)?, &binary::<64>(&value[1])?)?;
    Ok(Signed { p: p.clone(), wire: raw.to_vec(), hash: hash(p)? })
}
pub(super) fn verify_time_request(raw: &[u8], context: &Value, hub: &str) -> Result<Signed> {
    let s = worker_message(Kind::TimeRequest, raw, context, hub)?;
    ensure!(s.p[3] == crypto::computer_id(&binary::<32>(&json!(hub))?), "time request Hub mismatch");
    Ok(s)
}
pub(super) fn verify_time(raw: &[u8], context: &Value, issuer: &str, hub: &str, nonce: &str) -> Result<Signed> {
    let s = worker_message(Kind::Time, raw, context, issuer)?;
    ensure!(
        s.p[2] == nonce && s.p[3] == crypto::computer_id(&binary::<32>(&json!(hub))?) && number(&s.p[5])? <= 300_000,
        "time response binding mismatch"
    );
    Ok(s)
}
/// HOST-R1-01: `[D, C, hT, hG, computerIdH, nonceRead, consumedAt]` assinado pela raiz E; liga nonce e Hub exatos.
pub(super) fn verify_witness(raw: &[u8], context: &Value, issuer: &str, hub: &str, nonce: &str) -> Result<Signed> {
    let s = worker_message(Kind::ConsumptionWitness, raw, context, issuer)?;
    ensure!(
        s.p[5] == nonce && s.p[4] == crypto::computer_id(&binary::<32>(&json!(hub))?) && number(&s.p[6])? > 0,
        "consumption witness binding mismatch"
    );
    Ok(s)
}
pub(super) fn verify_snapshot(
    raw: &[u8],
    context: &Value,
    issuer: &str,
    hub: &str,
    generation: &str,
    nonce: &str,
) -> Result<Signed> {
    let s = worker_message(Kind::HostState, raw, context, issuer)?;
    ensure!(
        s.p[10] == crypto::computer_id(&binary::<32>(&json!(hub))?) && s.p[11] == generation && s.p[15] == nonce,
        "snapshot binding mismatch"
    );
    let duration =
        number(&s.p[17])?.checked_sub(number(&s.p[16])?).ok_or_else(|| anyhow!("snapshot lease reversed"))?;
    ensure!(duration > 0 && duration <= 900_000, "snapshot lease duration invalid");
    Ok(s)
}
