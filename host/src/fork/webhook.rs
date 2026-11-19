//! Abertura de eventos públicos cifrados para a identidade deste Hub.
use anyhow::{Result, anyhow, ensure};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use x25519_dalek::StaticSecret;
use zeroize::Zeroize;

const DOMAIN: &[u8] = b"donna/webhook-sealed/v1";
const MAX_CIPHERTEXT: usize = 96 * 1024;

fn context(fields: [&str; 4]) -> Result<Vec<u8>> {
    let mut out = DOMAIN.to_vec();
    for field in fields {
        ensure!(!field.is_empty() && field.len() <= 1024, "invalid context");
        let size = u16::try_from(field.len()).map_err(|_| anyhow!("invalid context"))?;
        out.extend_from_slice(&size.to_be_bytes());
        out.extend_from_slice(field.as_bytes());
    }
    Ok(out)
}

pub(crate) fn open(
    secret: &StaticSecret,
    fields: [&str; 4],
    ephemeral: &[u8; 32],
    nonce: &[u8; 12],
    ciphertext: &[u8],
) -> Result<Vec<u8>> {
    ensure!((16..=MAX_CIPHERTEXT).contains(&ciphertext.len()), "invalid envelope");
    let aad = context(fields)?;
    let public = crate::remote::crypto::x25519_pub(secret);
    let mut shared = crate::remote::crypto::x25519(secret, ephemeral)?;
    let salt = Sha256::digest([DOMAIN, ephemeral.as_slice(), public.as_slice()].concat());
    let hk = Hkdf::<Sha256>::new(Some(&salt), &shared);
    let mut key = [0u8; 32];
    let result = (|| {
        hk.expand(&aad, &mut key).map_err(|_| anyhow!("invalid envelope"))?;
        ChaCha20Poly1305::new((&key).into())
            .decrypt(&Nonce::from(*nonce), Payload { msg: ciphertext, aad: &aad })
            .map_err(|_| anyhow!("invalid envelope"))
    })();
    shared.zeroize();
    key.zeroize();
    result
}

/// A conexão autenticada só encaminha envelopes; a rotina valida novamente a credencial.
pub(crate) fn receive(hub: &std::sync::Arc<crate::hub::Hub>, item: &serde_json::Value) -> serde_json::Value {
    use crate::remote::crypto::{unb64, unb64_n};
    use serde_json::json;
    use zeroize::Zeroizing;

    let request = item["request"].as_str().filter(|s| s.len() <= 64 && !s.is_empty()).unwrap_or("");
    let fail = || json!({"t":"hook.sealed.ack", "request":request, "ok":false, "code":"invalid"});
    let result = (|| -> Result<serde_json::Value> {
        ensure!(!request.is_empty(), "invalid request");
        let envelope = item["envelope"].as_object().ok_or_else(|| anyhow!("invalid envelope"))?;
        ensure!(
            envelope.len() == 8 && envelope.get("v").and_then(serde_json::Value::as_u64) == Some(1),
            "invalid envelope"
        );
        let field = |name: &str| envelope.get(name).and_then(|v| v.as_str()).ok_or_else(|| anyhow!("invalid envelope"));
        let deployment = field("deployment")?;
        let computer = field("computerId")?;
        let hook = field("hookId")?;
        let delivery = field("delivery")?;
        ensure!(crate::remote::cloud::url(&hub.store).as_deref() == Some(deployment), "different deployment");
        ensure!(
            hub.identity.computer_id() == computer
                && !hook.is_empty()
                && hook.len() <= 64
                && !delivery.is_empty()
                && delivery.len() <= 200,
            "different destination"
        );
        let ephemeral = unb64_n::<32>(field("ephemeral")?)?;
        let nonce = unb64_n::<12>(field("nonce")?)?;
        let encoded = field("ciphertext")?;
        ensure!(encoded.len() <= 128 * 1024, "invalid envelope");
        let ciphertext = unb64(encoded)?;
        let plaintext = Zeroizing::new(open(
            &hub.identity.box_secret,
            [deployment, computer, hook, delivery],
            &ephemeral,
            &nonce,
            &ciphertext,
        )?);
        ensure!(plaintext.len() >= 4, "invalid envelope");
        let n = u32::from_be_bytes(plaintext[0..4].try_into()?) as usize;
        ensure!(
            n <= 16 * 1024 && n <= plaintext.len() - 4 && plaintext.len() - 4 - n <= crate::routines::hooks::MAX_BODY,
            "invalid envelope"
        );
        let headers: crate::routines::hooks::Headers = serde_json::from_slice(&plaintext[4..4 + n])?;
        ensure!(crate::routines::hooks::delivery_id(&headers)?.as_deref() == Some(delivery), "different delivery");
        match hub.routines.receive(hub, hook, &headers, &plaintext[4 + n..]) {
            Ok(_) => Ok(json!({"t":"hook.sealed.ack", "request":request, "ok":true})),
            Err(e) => {
                Ok(json!({"t":"hook.sealed.ack", "request":request, "ok":false, "retry":e.retry(), "code":e.code()}))
            }
        }
    })();
    result.unwrap_or_else(|_| fail())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "Fixture sintética de teste.")]
pub(crate) fn fixture_item(
    hub: &crate::hub::Hub,
    deployment: &str,
    hook: &str,
    delivery: &str,
    key: &str,
    request: &str,
) -> serde_json::Value {
    use chacha20poly1305::aead::Aead;
    use serde_json::json;
    let ephemeral_secret = StaticSecret::from([2u8; 32]);
    let ephemeral = crate::remote::crypto::x25519_pub(&ephemeral_secret);
    let public = hub.identity.box_pub();
    let shared = crate::remote::crypto::x25519(&ephemeral_secret, &public).unwrap();
    let salt = Sha256::digest([DOMAIN, ephemeral.as_slice(), public.as_slice()].concat());
    let aad = context([deployment, &hub.identity.computer_id(), hook, delivery]).unwrap();
    let mut cipher_key = [0; 32];
    Hkdf::<Sha256>::new(Some(&salt), &shared).expand(&aad, &mut cipher_key).unwrap();
    let headers =
        serde_json::to_vec(&json!({"authorization":format!("Bearer {key}"), "x-delivery-id":delivery})).unwrap();
    let plain =
        [&u32::try_from(headers.len()).unwrap().to_be_bytes()[..], &headers, br#"{"text":"deploy failed"}"#].concat();
    let nonce = [3u8; 12];
    let ciphertext = ChaCha20Poly1305::new((&cipher_key).into())
        .encrypt(&Nonce::from(nonce), Payload { msg: &plain, aad: &aad })
        .unwrap();
    let b64 = crate::remote::crypto::b64;
    json!({"t":"hook.sealed", "request":request, "envelope":{"v":1,"deployment":deployment,"computerId":hub.identity.computer_id(),"hookId":hook,"delivery":delivery,"ephemeral":b64(&ephemeral),"nonce":b64(&nonce),"ciphertext":b64(&ciphertext)}})
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "Vetores sintéticos de teste.")]
mod tests {
    use super::*;
    use crate::remote::crypto::{unb64, unb64_n};
    use serde_json::Value;

    #[test]
    fn typescript_vector_and_context_tampering() {
        let v: Value =
            serde_json::from_str(include_str!("../../../docs/reference/fixtures/remote-relay-vectors.json")).unwrap();
        let v = &v["webhookSealedV1"];
        let secret = StaticSecret::from(unb64_n::<32>(v["hubSecret"].as_str().unwrap()).unwrap());
        let eph = unb64_n::<32>(v["ephemeral"].as_str().unwrap()).unwrap();
        let nonce = unb64_n::<12>(v["nonce"].as_str().unwrap()).unwrap();
        let ct = unb64(v["ciphertext"].as_str().unwrap()).unwrap();
        let fields = ["fixture-deployment", "fixture-hub", "fixture-hook", "fixture-delivery"];
        assert_eq!(open(&secret, fields, &eph, &nonce, &ct).unwrap(), unb64(v["plaintext"].as_str().unwrap()).unwrap());
        for index in 0..4 {
            let mut bad = fields;
            bad[index] = "other";
            assert!(open(&secret, bad, &eph, &nonce, &ct).is_err());
        }
        assert!(open(&StaticSecret::from([7; 32]), fields, &eph, &nonce, &ct).is_err());
        let mut altered = ct;
        *altered.last_mut().unwrap() ^= 1;
        assert!(open(&secret, fields, &eph, &nonce, &altered).is_err());
    }
}
