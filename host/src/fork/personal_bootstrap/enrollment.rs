//! Resultado begin original cifrado, domínio exclusivo; não usa cofre/keyring.
use super::{protocol, provisioning::AdmittedConfig, wire};
use crate::remote::{crypto, identity::Identity};
use anyhow::{Context, Result, ensure};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use hkdf::Hkdf;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;
#[derive(Clone)]
pub(super) struct Binding {
    pub context: Value,
    pub capture: String,
    pub q: Value,
    pub h_q: String,
    pub q_wire_hash: String,
    pub begin: Value,
    pub generation: String,
    pub sign: String,
    pub box_key: String,
}
impl Binding {
    pub(super) fn new(cfg: &AdmittedConfig, q: &Value, begin: &Value) -> Result<Self> {
        let h_q = wire::hash(&q[0])?;
        let q_wire_hash = protocol::hash(&serde_json::to_vec(q)?);
        let i = cfg.identity();
        let capture = wire::hash(&json!([
            wire::domain("prepare-capture"),
            1,
            cfg.context(),
            i.generation,
            i.computer_id,
            i.sign_key,
            i.box_key,
            h_q,
            q_wire_hash
        ]))?;
        ensure!(
            q[0][1] == *cfg.context()
                && q[0][3] == "device"
                && q[0][5] == i.generation
                && q[0][6] == i.computer_id
                && q[0][7] == i.sign_key
                && q[0][8] == i.box_key,
            "enrollment ticket mismatch"
        );
        ensure!(
            begin[0][0] == wire::domain("begin")
                && begin[0][1] == *cfg.context()
                && begin[0][2] == q[0][2]
                && begin[0][3] == q[0][4]
                && begin[0][4] == h_q
                && begin[0][5] == q[0][9],
            "enrollment begin mismatch"
        );
        Ok(Self {
            context: cfg.context().clone(),
            capture,
            q: q.clone(),
            h_q,
            q_wire_hash,
            begin: begin.clone(),
            generation: i.generation.clone(),
            sign: i.sign_key.clone(),
            box_key: i.box_key.clone(),
        })
    }
    fn key(&self, h: &Identity) -> Result<Zeroizing<[u8; 32]>> {
        ensure!(h.sign_pub_b64() == self.sign && h.box_pub_b64() == self.box_key, "enrollment identity changed");
        let salt = Sha256::digest(serde_json::to_vec(&json!([
            wire::domain("enrollment-secret-salt"),
            1,
            self.context,
            self.capture,
            self.h_q,
            self.q_wire_hash,
            wire::hash(&self.begin[0])?,
            self.generation,
            self.sign,
            self.box_key
        ]))?);
        let hk = Hkdf::<Sha256>::new(Some(&salt), h.box_secret.as_bytes());
        let mut key = Zeroizing::new([0; 32]);
        hk.expand(wire::domain("enrollment-secret-key").as_bytes(), key.as_mut())
            .map_err(|_| anyhow::anyhow!("enrollment KDF failed"))?;
        Ok(key)
    }
    fn aad(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&json!([
            wire::domain("enrollment-secret"),
            1,
            self.context,
            self.capture,
            self.h_q,
            self.q_wire_hash,
            wire::hash(&self.begin[0])?,
            self.q[0][2],
            self.q[0][4],
            "worker-begin-response"
        ]))?)
    }
    pub(super) fn facts(&self, response: &Value) -> Result<Value> {
        let m = response.as_object().context("invalid begin response")?;
        ensure!(
            m.len() == 7
                && ["intentId", "candidateId", "userCode", "deviceCode", "createdAt", "expiresAt", "intervalMs"]
                    .iter()
                    .all(|k| m.contains_key(*k)),
            "invalid begin response fields"
        );
        let code = response["deviceCode"].as_str().context("missing device code")?;
        ensure!(crypto::unb64_n::<32>(code).is_ok_and(|b| crypto::b64(&b) == code), "invalid device code");
        ensure!(
            response["intentId"] == self.q[0][2]
                && response["candidateId"] == self.q[0][4]
                && response["intervalMs"] == 5000
                && response["userCode"].as_str().is_some_and(
                    |s| s.len() == 12 && s.bytes().all(|b| b.is_ascii_uppercase() || (b'2'..=b'7').contains(&b))
                ),
            "begin identity/locator mismatch"
        );
        let at = response["createdAt"].as_u64().context("invalid begin createdAt")?;
        let end = response["expiresAt"].as_u64().context("invalid begin expiresAt")?;
        ensure!(
            at > 0
                && end > at
                && end - at <= 600_000
                && at.saturating_add(30_000) >= self.begin[0][6].as_u64().context("begin time")?
                && end <= self.begin[0][7].as_u64().context("begin expiry")?
                && end <= 9_007_199_254_740_991,
            "begin window mismatch"
        );
        Ok(json!([
            self.h_q,
            self.q_wire_hash,
            wire::hash(&self.begin[0])?,
            response["intentId"],
            response["candidateId"],
            at,
            end,
            5000
        ]))
    }
    pub(super) fn seal(&self, h: &Identity, raw: &[u8]) -> Result<(Value, Vec<u8>)> {
        ensure!(raw.len() <= 4096, "begin response too large");
        let response = protocol::canonical(raw)?;
        let facts = self.facts(&response)?;
        let key = self.key(h)?;
        let nonce = crypto::random::<24>();
        let cipher =
            XChaCha20Poly1305::new_from_slice(key.as_ref()).map_err(|_| anyhow::anyhow!("enrollment cipher failed"))?;
        let ct = cipher
            .encrypt(&XNonce::from(nonce), Payload { msg: raw, aad: &self.aad()? })
            .map_err(|_| anyhow::anyhow!("enrollment encryption failed"))?;
        let sealed =
            serde_json::to_vec(&json!([wire::domain("enrollment-secret"), 1, crypto::b64(&nonce), crypto::b64(&ct)]))?;
        ensure!(sealed.len() <= 8192, "enrollment cipher too large");
        Ok((facts, sealed))
    }
    pub(super) fn open(&self, h: &Identity, sealed: &[u8], facts: &Value) -> Result<Zeroizing<Vec<u8>>> {
        ensure!(sealed.len() <= 8192, "enrollment cipher too large");
        let e = protocol::canonical(sealed)?;
        ensure!(
            e.as_array().is_some_and(|a| a.len() == 4) && e[0] == wire::domain("enrollment-secret") && e[1] == 1,
            "enrollment format mismatch"
        );
        let nonce = crypto::unb64_n::<24>(e[2].as_str().context("enrollment nonce")?)?;
        let ct = crypto::unb64(e[3].as_str().context("enrollment ciphertext")?)?;
        ensure!(
            ct.len() <= 4112 && e[2] == crypto::b64(&nonce) && e[3] == crypto::b64(&ct),
            "enrollment encoding mismatch"
        );
        let key = self.key(h)?;
        let cipher =
            XChaCha20Poly1305::new_from_slice(key.as_ref()).map_err(|_| anyhow::anyhow!("enrollment cipher failed"))?;
        let plain = Zeroizing::new(
            cipher
                .decrypt(&XNonce::from(nonce), Payload { msg: &ct, aad: &self.aad()? })
                .map_err(|_| anyhow::anyhow!("enrollment authentication failed"))?,
        );
        ensure!(
            plain.len() <= 4096 && self.facts(&protocol::canonical(&plain)?)? == *facts,
            "enrollment facts changed"
        );
        Ok(plain)
    }
}
