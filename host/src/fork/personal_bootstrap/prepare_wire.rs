//! Wire fechado da ponte prepare v1; não altera os quadros dos canais existentes.
use super::{protocol, wire};
use crate::remote::crypto;
use anyhow::{Context, Result, ensure};
use chacha20poly1305::{
    ChaCha20Poly1305, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use hkdf::Hkdf;
use serde_json::{Value, json};
use sha2::Sha256;
use zeroize::Zeroizing;
pub(super) fn binary(value: &Value, limit: usize) -> Result<Vec<u8>> {
    let raw = serde_json::to_vec(value)?;
    ensure!(!raw.is_empty() && raw.len() <= limit, "prepare message size");
    protocol::packet(&raw)?;
    Ok([u32::try_from(raw.len())?.to_be_bytes().as_slice(), raw.as_slice()].concat())
}
pub(super) fn message(raw: &[u8], limit: usize) -> Result<Value> {
    ensure!(raw.len() >= 5 && raw.len() <= limit + 4, "prepare framing size");
    let len = u32::from_be_bytes(raw[..4].try_into()?) as usize;
    ensure!(len > 0 && len <= limit && len + 4 == raw.len(), "prepare framing length");
    protocol::packet(&raw[4..])
}
pub(super) fn key<const N: usize>(value: &Value) -> Result<[u8; N]> {
    let s = value.as_str().context("prepare binary field")?;
    let b = crypto::unb64_n::<N>(s)?;
    ensure!(crypto::b64(&b) == s, "prepare binary encoding");
    Ok(b)
}
pub(super) fn hash_field(value: &Value) -> Result<&str> {
    let s = value.as_str().context("prepare hash")?;
    ensure!(
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "prepare hash encoding"
    );
    Ok(s)
}
pub(super) fn capture(context: &Value, q: &Value) -> Result<String> {
    wire::hash(&json!([
        wire::domain("prepare-capture"),
        1,
        context,
        q[0][5],
        q[0][6],
        q[0][7],
        q[0][8],
        wire::hash(&q[0])?,
        protocol::hash(&serde_json::to_vec(q)?)
    ]))
}
pub(super) fn operation(context: &Value, capture: &str, hq: &str, wire_hash: &str, digest: &str) -> Result<String> {
    wire::hash(&json!([wire::domain("prepare-operation"), 1, context, capture, hq, wire_hash, "candidate", digest]))
}
pub(super) fn transcript(hello: &Value, welcome0: &Value) -> Result<[u8; 32]> {
    Ok(crypto::sha256(&[&serde_json::to_vec(&json!([
        wire::domain("prepare-transcript"),
        1,
        "GET",
        "/personal-prepare",
        hello,
        welcome0
    ]))?]))
}
pub(super) struct Frames {
    context: Value,
    capture: String,
    hq: String,
    wire_hash: String,
    th: [u8; 32],
    a2h: Zeroizing<[u8; 32]>,
    h2a: Zeroizing<[u8; 32]>,
    read: bool,
    written: bool,
}
impl Frames {
    pub(super) fn new(hello: &Value, th: [u8; 32], dh: &[u8; 32]) -> Result<Self> {
        ensure!(dh.iter().any(|b| *b != 0), "prepare zero DH");
        let hk = Hkdf::<Sha256>::new(Some(&th), dh);
        let mut a2h = Zeroizing::new([0; 32]);
        let mut h2a = Zeroizing::new([0; 32]);
        hk.expand(wire::domain("prepare-key-a2h").as_bytes(), a2h.as_mut())
            .map_err(|_| anyhow::anyhow!("prepare KDF"))?;
        hk.expand(wire::domain("prepare-key-h2a").as_bytes(), h2a.as_mut())
            .map_err(|_| anyhow::anyhow!("prepare KDF"))?;
        Ok(Self {
            context: hello[2].clone(),
            capture: hash_field(&hello[3])?.into(),
            hq: hash_field(&hello[4])?.into(),
            wire_hash: hash_field(&hello[5])?.into(),
            th,
            a2h,
            h2a,
            read: false,
            written: false,
        })
    }
    fn aad(&self, direction: &str) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&json!([
            wire::domain("prepare-frame"),
            1,
            self.context,
            self.capture,
            self.hq,
            self.wire_hash,
            crypto::b64(&self.th),
            direction,
            0
        ]))?)
    }
    pub(super) fn open_request(&mut self, frame: &Value) -> Result<Zeroizing<Vec<u8>>> {
        ensure!(
            !self.read
                && frame.as_array().is_some_and(|v| v.len() == 5)
                && frame[0] == wire::domain("prepare-frame")
                && frame[1] == 1
                && frame[2] == "a2h"
                && frame[3] == 0,
            "prepare direction/sequence"
        );
        let ct = crypto::unb64(frame[4].as_str().context("prepare ciphertext")?)?;
        ensure!((17..=16400).contains(&ct.len()) && frame[4] == crypto::b64(&ct), "prepare ciphertext length/encoding");
        let cipher =
            ChaCha20Poly1305::new_from_slice(self.a2h.as_ref()).map_err(|_| anyhow::anyhow!("prepare cipher"))?;
        let plain = Zeroizing::new(
            cipher
                .decrypt(&Nonce::from([0; 12]), Payload { msg: &ct, aad: &self.aad("a2h")? })
                .map_err(|_| anyhow::anyhow!("prepare frame authentication"))?,
        );
        protocol::canonical(&plain)?;
        self.read = true;
        Ok(plain)
    }
    pub(super) fn response(&mut self, value: &Value) -> Result<Value> {
        ensure!(self.read && !self.written, "prepare response sequence");
        let raw = serde_json::to_vec(value)?;
        protocol::canonical(&raw)?;
        let cipher =
            ChaCha20Poly1305::new_from_slice(self.h2a.as_ref()).map_err(|_| anyhow::anyhow!("prepare cipher"))?;
        let ct = cipher
            .encrypt(&Nonce::from([0; 12]), Payload { msg: &raw, aad: &self.aad("h2a")? })
            .map_err(|_| anyhow::anyhow!("prepare encryption"))?;
        self.written = true;
        Ok(json!([wire::domain("prepare-frame"), 1, "h2a", 0, crypto::b64(&ct)]))
    }
}
