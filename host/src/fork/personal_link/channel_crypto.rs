//! PR-B: domínios próprios; nenhuma chave/AAD de hs1/hs2.
use super::canon;
use crate::remote::crypto;
use anyhow::{Result, ensure};
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use hkdf::Hkdf;
use serde_json::{Value, json};
use sha2::Sha256;
use zeroize::Zeroizing;

pub(super) fn random<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|_| anyhow::anyhow!("not_allowed: RNG indisponível"))?;
    Ok(bytes)
}
pub(super) fn id<const N: usize>() -> Result<String> {
    Ok(crypto::b64(&random::<N>()?))
}

pub(super) struct Cifra {
    key: Zeroizing<[u8; 32]>,
    session: String,
    th: String,
    direction: &'static str,
    counter: u64,
}
impl Cifra {
    pub(super) fn nova(shared: &[u8; 32], transcript: &Value, session: &str, direction: &'static str) -> Result<Self> {
        let salt = crypto::sha256(&[&canon::j(transcript)?]);
        let th = canon::hash(transcript)?;
        let info = canon::j(&json!(["donna.personal-link/channel-key/v1", 1, "manager", direction, th, session]))?;
        let mut key = Zeroizing::new([0; 32]);
        Hkdf::<Sha256>::new(Some(&salt), shared)
            .expand(&info, &mut key[..])
            .map_err(|_| anyhow::anyhow!("not_allowed: KDF"))?;
        Ok(Self { key, session: session.into(), th, direction, counter: 0 })
    }
    fn contexto(&self) -> Result<([u8; 12], Vec<u8>)> {
        ensure!(self.counter < u64::from(u32::MAX), "not_allowed: contador esgotado");
        let mut nonce = [0; 12];
        nonce[4..].copy_from_slice(&self.counter.to_be_bytes());
        Ok((
            nonce,
            canon::j(&json!([
                "donna.personal-link/frame/v1",
                1,
                self.th,
                self.session,
                "manager",
                self.direction,
                self.counter
            ]))?,
        ))
    }
    pub(super) fn abrir(&mut self, raw: &[u8]) -> Result<Value> {
        ensure!(raw.len() <= canon::LIMITE, "not_allowed: frame grande");
        canon::profundidade(raw)?;
        let frame: Value = serde_json::from_slice(raw)?;
        ensure!(serde_json::to_vec(&frame)? == raw, "not_allowed: frame não canônico");
        let a = frame.as_array().filter(|a| a.len() == 3).ok_or_else(|| anyhow::anyhow!("not_allowed: frame"))?;
        ensure!(a[0] == self.session && a[1] == self.counter, "not_allowed: sessão/contador");
        let text = a[2].as_str().ok_or_else(|| anyhow::anyhow!("not_allowed: cifra"))?;
        let bytes = crypto::unb64(text)?;
        ensure!(crypto::b64(&bytes) == text, "not_allowed: cifra canônica");
        let (nonce, aad) = self.contexto()?;
        let plain = Zeroizing::new(
            ChaCha20Poly1305::new((&*self.key).into())
                .decrypt(&Nonce::from(nonce), Payload { msg: &bytes, aad: &aad })
                .map_err(|_| anyhow::anyhow!("not_allowed: tag"))?,
        );
        let value = canon::ler(&plain)?;
        self.counter += 1;
        Ok(value)
    }
    pub(super) fn selar(&mut self, plain: &Value) -> Result<Vec<u8>> {
        let (nonce, aad) = self.contexto()?;
        let bytes = ChaCha20Poly1305::new((&*self.key).into())
            .encrypt(&Nonce::from(nonce), Payload { msg: &canon::j(plain)?, aad: &aad })
            .map_err(|_| anyhow::anyhow!("not_allowed: cifra"))?;
        let frame = json!([self.session, self.counter, crypto::b64(&bytes)]);
        let bytes = serde_json::to_vec(&frame)?;
        ensure!(bytes.len() + 8 <= canon::LIMITE, "not_allowed: frame grande");
        self.counter += 1;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;

    // Golden público independente (Python SHA/HMAC); bytes originais SHA256 3b46d2b735de82ff72fd0b912e396d826dee0fa5de5f93758eff9c073d5a651f.
    const VECTOR_JSON: &str = r#"{
  "schema": "donna.personal-link/pr-b-vectors/v1",
  "note": "Material sintético público; transcrição mínima para KDF/AAD, não cadeia admitida. Gerado offline com SHA256/HMAC RFC5869, sem executar implementação B.",
  "transcript": [
    "donna.personal-link/channel-final/v1",
    1,
    [
      "CH"
    ],
    [
      "CR"
    ],
    [
      "W"
    ]
  ],
  "transcriptHex": "5b22646f6e6e612e706572736f6e616c2d6c696e6b2f6368616e6e656c2d66696e616c2f7631222c312c5b224348225d2c5b224352225d2c5b2257225d5d",
  "TH": "521dfe3b2e8240bfd49268dd94595a0932d947e8cc28d81de155f26c379b9a07",
  "saltHex": "521dfe3b2e8240bfd49268dd94595a0932d947e8cc28d81de155f26c379b9a07",
  "sharedHex": "0707070707070707070707070707070707070707070707070707070707070707",
  "sessionId": "CQkJCQkJCQkJCQkJCQkJCQ",
  "nonce0Hex": "000000000000000000000000",
  "directions": {
    "c2h": {
      "info": "[\"donna.personal-link/channel-key/v1\",1,\"manager\",\"c2h\",\"521dfe3b2e8240bfd49268dd94595a0932d947e8cc28d81de155f26c379b9a07\",\"CQkJCQkJCQkJCQkJCQkJCQ\"]",
      "keyHex": "d9ed37d89b87ce92de40d20bd950898b2aac1f4dcab2cde7894087bebdde1509",
      "aad0": "[\"donna.personal-link/frame/v1\",1,\"521dfe3b2e8240bfd49268dd94595a0932d947e8cc28d81de155f26c379b9a07\",\"CQkJCQkJCQkJCQkJCQkJCQ\",\"manager\",\"c2h\",0]"
    },
    "h2c": {
      "info": "[\"donna.personal-link/channel-key/v1\",1,\"manager\",\"h2c\",\"521dfe3b2e8240bfd49268dd94595a0932d947e8cc28d81de155f26c379b9a07\",\"CQkJCQkJCQkJCQkJCQkJCQ\"]",
      "keyHex": "60be1195dd027a681b50eaa1b55f24d6240f2f1469c59ca1d56a2220baf9398a",
      "aad0": "[\"donna.personal-link/frame/v1\",1,\"521dfe3b2e8240bfd49268dd94595a0932d947e8cc28d81de155f26c379b9a07\",\"CQkJCQkJCQkJCQkJCQkJCQ\",\"manager\",\"h2c\",0]"
    }
  }
}
"#;
    #[test]
    fn pr_b_cifra_direcao_tag_contador_e_dh_zero() -> Result<()> {
        let transcript = json!(["donna.personal-link/channel-final/v1", 1, ["CH"], ["CR"], ["W"]]);
        let shared = [7; 32];
        let sid = crypto::b64(&[9; 16]);
        let mut sender = Cifra::nova(&shared, &transcript, &sid, "c2h")?;
        let mut receiver = Cifra::nova(&shared, &transcript, &sid, "c2h")?;
        let bytes = sender.selar(&json!(["Q", 1]))?;
        assert_eq!(receiver.abrir(&bytes)?, json!(["Q", 1]));
        assert!(receiver.abrir(&bytes).is_err());
        assert!(Cifra::nova(&shared, &transcript, &sid, "h2c")?.abrir(&bytes).is_err());
        let mut bad: Value = serde_json::from_slice(&bytes)?;
        bad[2] = json!(crypto::b64(&[0; 32]));
        assert!(Cifra::nova(&shared, &transcript, &sid, "c2h")?.abrir(&serde_json::to_vec(&bad)?).is_err());
        sender.counter = u64::from(u32::MAX);
        assert!(sender.selar(&json!(["Q", 1])).is_err());
        assert!(crypto::x25519(&x25519_dalek::StaticSecret::from([7; 32]), &[0; 32]).is_err());
        // Salt é digest RAW32; info liga TH/direção/session. Hex ASCII ou J duplo mudam a chave.
        let salt = crypto::sha256(&[&canon::j(&transcript)?]);
        let th = canon::hash(&transcript)?;
        let info = canon::j(&json!(["donna.personal-link/channel-key/v1", 1, "manager", "c2h", th, sid]))?;
        let mut expected = [0; 32];
        Hkdf::<Sha256>::new(Some(&salt), &shared).expand(&info, &mut expected).map_err(|_| anyhow::anyhow!("KDF"))?;
        assert_eq!(*receiver.key, expected);
        let vector: Value = serde_json::from_str(VECTOR_JSON)?;
        assert_eq!(th, vector["TH"]);
        assert_eq!(crate::fork::bootstrap::codec::hash_bytes(&canon::j(&transcript)?), vector["saltHex"]);
        let mut hex = String::with_capacity(expected.len() * 2);
        for byte in &expected {
            write!(&mut hex, "{byte:02x}")?;
        }
        assert_eq!(hex, vector["directions"]["c2h"]["keyHex"]);

        Ok(())
    }
}
