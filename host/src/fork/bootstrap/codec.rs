//! Closed, canonical ACS4 arrays. The signed bytes never include the envelope.
use crate::remote::crypto;
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

pub const LIMIT: usize = 16 * 1024;
const SAFE: i64 = 9_007_199_254_740_991;
pub const CAPS: [&str; 2] = ["control", "owner.devices"];

// Tuple structs force field order and reject omitted/extra entries.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Grant(
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub i64,
    pub i64,
    pub Vec<String>,
);
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Challenge(
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub i64,
    pub i64,
);
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Proof(pub String, pub String, pub String, pub String, pub String);
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt(
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub Vec<String>,
    pub i64,
    pub i64,
    pub String,
    pub i64,
    pub String,
);
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Revocation(
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub i64,
    pub i64,
    pub i64,
    pub String,
);

/// Snapshot of the actual active Device, never the pre-publication pending row.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Activation(
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub String,
    pub i64,
    pub i64,
    pub String,
    pub i64,
    pub i64,
    pub Vec<String>,
);

pub trait Payload: Serialize + DeserializeOwned {
    fn validate(&self) -> Result<()>;
}
fn id(s: &str) -> Result<()> {
    ensure!(
        !s.is_empty() && s.len() <= 256 && s.bytes().all(|c| c.is_ascii_alphanumeric() || b"_-.:/".contains(&c)),
        "invalid identifier"
    );
    Ok(())
}
pub fn b64<const N: usize>(s: &str) -> Result<[u8; N]> {
    let v = crypto::unb64_n::<N>(s)?;
    ensure!(crypto::b64(&v) == s, "noncanonical base64url");
    Ok(v)
}
fn bootstrap_id(s: &str) -> Result<()> {
    ensure!(
        !s.is_empty() && s.len() <= 64 && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'),
        "invalid bootstrap id"
    );
    Ok(())
}
fn hex(s: &str) -> Result<()> {
    ensure!(s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "invalid hash");
    Ok(())
}
fn time(t: i64) -> Result<()> {
    ensure!((0..=SAFE).contains(&t), "invalid time/counter");
    Ok(())
}
fn caps(v: &[String]) -> Result<()> {
    ensure!(v.len() == 2 && v[0] == CAPS[0] && v[1] == CAPS[1], "invalid capabilities");
    Ok(())
}
fn domain(actual: &str, expected: &str) -> Result<()> {
    ensure!(actual == expected, "invalid domain");
    Ok(())
}
impl Payload for Grant {
    fn validate(&self) -> Result<()> {
        domain(&self.0, "donna/bootstrap-grant/v1")?;
        bootstrap_id(&self.1)?;
        for s in [&self.2, &self.5, &self.6, &self.7, &self.8, &self.10, &self.11, &self.14] {
            id(s)?;
        }
        hex(&self.3)?;
        b64::<32>(&self.4)?;
        hex(&self.9)?;
        for s in [&self.12, &self.13, &self.15, &self.16, &self.17] {
            b64::<32>(s)?;
        }
        time(self.18)?;
        time(self.19)?;
        ensure!(self.19 > self.18 && self.19 - self.18 <= 300_000, "invalid grant deadline");
        caps(&self.20)
    }
}
impl Grant {
    pub fn valid_at(&self, now: i64) -> Result<()> {
        self.validate()?;
        time(now)?;
        ensure!(self.18 <= now.saturating_add(30_000) && now < self.19, "expired or future grant");
        Ok(())
    }
}
impl Payload for Challenge {
    fn validate(&self) -> Result<()> {
        domain(&self.0, "donna/bootstrap-challenge/v1")?;
        bootstrap_id(&self.1)?;
        b64::<32>(&self.2)?;
        hex(&self.3)?;
        b64::<32>(&self.4)?;
        b64::<32>(&self.5)?;
        b64::<32>(&self.6)?;
        time(self.7)?;
        time(self.8)?;
        ensure!(self.8 > self.7 && self.8 - self.7 <= 60_000, "invalid challenge deadline");
        Ok(())
    }
}
impl Payload for Proof {
    fn validate(&self) -> Result<()> {
        domain(&self.0, "donna/bootstrap-proof/v1")?;
        bootstrap_id(&self.1)?;
        b64::<32>(&self.2)?;
        hex(&self.3)?;
        hex(&self.4)
    }
}
impl Payload for Receipt {
    fn validate(&self) -> Result<()> {
        domain(&self.0, "donna/bootstrap-receipt/v1")?;
        bootstrap_id(&self.1)?;
        b64::<32>(&self.2)?;
        for s in [&self.3, &self.4, &self.5, &self.15, &self.17] {
            hex(s)?;
        }
        for s in [&self.6, &self.7, &self.9, &self.11] {
            id(s)?;
        }
        ensure!(self.11.starts_with("grt_"), "invalid grant id");
        b64::<16>(&self.11[4..])?;
        b64::<32>(&self.8)?;
        b64::<32>(&self.10)?;
        caps(&self.12)?;
        time(self.13)?;
        time(self.14)?;
        time(self.16)
    }
}

impl Payload for Revocation {
    fn validate(&self) -> Result<()> {
        domain(&self.0, "donna/bootstrap-revocation/v1")?;
        bootstrap_id(&self.1)?;
        b64::<32>(&self.2)?;
        ensure!(self.3.starts_with("grt_"), "invalid grant");
        b64::<16>(&self.3[4..])?;
        hex(&self.4)?;
        time(self.5)?;
        time(self.6)?;
        time(self.7)?;
        hex(&self.8)
    }
}

impl Payload for Activation {
    fn validate(&self) -> Result<()> {
        domain(&self.0, "donna/bootstrap-activation/v1")?;
        bootstrap_id(&self.1)?;
        b64::<32>(&self.2)?;
        ensure!(self.3.starts_with("grt_"), "invalid grant");
        b64::<16>(&self.3[4..])?;
        for value in [&self.4, &self.5, &self.7] {
            id(value)?;
        }
        b64::<32>(&self.6)?;
        hex(&self.8)?;
        time(self.9)?;
        time(self.10)?;
        hex(&self.11)?;
        time(self.12)?;
        time(self.13)?;
        ensure!(
            self.12 > 0 && self.13 > self.12 && self.13 - self.12 <= crate::remote::cloud::LEASE_MS,
            "invalid active lease"
        );
        caps(&self.14)
    }
}

pub fn bytes<T: Payload>(value: &T) -> Result<Vec<u8>> {
    value.validate()?;
    Ok(serde_json::to_vec(value)?)
}
pub fn hash_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in crypto::sha256(&[bytes]) {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 15)]));
    }
    output
}
pub fn hash<T: Payload>(value: &T) -> Result<String> {
    Ok(hash_bytes(&bytes(value)?))
}
/// Parse a canonical envelope and verify the exact canonical payload with the preselected key.
pub fn decode<T: Payload>(wire: &[u8], key: &[u8; 32]) -> Result<T> {
    ensure!(wire.len() <= LIMIT, "message too large");
    let (payload, signature): (Value, String) = serde_json::from_slice(wire)?;
    let signed = serde_json::to_vec(&payload)?;
    ensure!(serde_json::to_vec(&(payload.clone(), &signature))? == wire, "noncanonical envelope");
    let sig = b64::<64>(&signature)?;
    crypto::verify(key, &signed, &sig)?;
    let parsed: T = serde_json::from_value(payload)?;
    ensure!(bytes(&parsed)? == signed, "noncanonical payload");
    Ok(parsed)
}
pub fn encode<T: Payload>(payload: &T, sign: impl FnOnce(&[u8]) -> [u8; 64]) -> Result<Vec<u8>> {
    let raw = bytes(payload)?;
    let v: Value = serde_json::from_slice(&raw)?;
    let envelope = serde_json::to_vec(&(v, crypto::b64(&sign(&raw))))?;
    if envelope.len() > LIMIT {
        bail!("message too large");
    }
    Ok(envelope)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use ed25519_dalek::SigningKey;

    #[test]
    fn canonical_envelope_signature_domain_and_hash_are_bound() {
        let key = SigningKey::from_bytes(&[9; 32]);
        let proof = Proof(
            "donna/bootstrap-proof/v1".into(),
            "boot".into(),
            crypto::b64(&[2; 32]),
            "a".repeat(64),
            "b".repeat(64),
        );
        let wire = encode(&proof, |b| crypto::sign(&key, b)).unwrap();
        let decoded: Proof = decode(&wire, &key.verifying_key().to_bytes()).unwrap();
        assert_eq!(hash(&proof).unwrap(), hash(&decoded).unwrap());
        let mut spaced = vec![b' '];
        spaced.extend(&wire);
        assert!(decode::<Proof>(&spaced, &key.verifying_key().to_bytes()).is_err());
        assert!(decode::<Proof>(&wire, &SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes()).is_err());
        let mut bad = proof.clone();
        bad.0 = "other".into();
        assert!(encode(&bad, |b| crypto::sign(&key, b)).is_err());
    }
}
