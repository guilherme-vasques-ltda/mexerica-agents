use super::{manifest, protocol, status};
use crate::api::devices::Caller;
use crate::store::Scope;
use anyhow::Result;
use serde_json::{Value, json};

fn vectors() -> Result<Value> {
    Ok(serde_json::from_str(include_str!("../../../../docs/reference/fixtures/remote-relay-vectors.json"))?)
}

#[test]
fn personal_manifest_context_matches_shared_vectors() -> Result<()> {
    let all = vectors()?;
    let v = &all["personalBootstrap"];
    let raw = v["manifestCanonical"].as_str().ok_or_else(|| anyhow::anyhow!("fixture sem manifesto"))?;
    let context = manifest::inspect_fixture(raw.as_bytes())?;
    assert_eq!(protocol::hash(raw.as_bytes()), v["manifestHash"]);
    assert_eq!(context.0, v["context"]);
    let bytes = serde_json::to_vec(&context.0)?;
    assert_eq!(std::str::from_utf8(&bytes)?, v["contextCanonical"]);
    assert_eq!(protocol::hash(&bytes), v["contextHash"]);
    Ok(())
}

#[test]
fn personal_manifest_rejects_profile_root_and_noncanonical_input() -> Result<()> {
    let all = vectors()?;
    let m = &all["personalBootstrap"]["manifest"];
    for (field, value) in [
        ("schema", json!(2)),
        ("schemaVersion", json!(6)),
        ("env", json!("local")),
        ("extra", json!(true)),
        ("origin", json!("https://cloud.example.invalid/")),
    ] {
        let mut changed = m.clone();
        changed[field] = value;
        assert!(manifest::inspect_fixture(&serde_json::to_vec(&changed)?).is_err(), "{field}");
    }
    let mut changed = m.clone();
    changed["personalBootstrap"]["issuerPublicKey"] = changed["bootstrap"]["issuerPublicKey"].clone();
    assert!(manifest::inspect_fixture(&serde_json::to_vec(&changed)?).is_err());
    for (field, value) in [("profile", json!("ACS4")), ("wire", json!(2)), ("extra", json!(1))] {
        let mut changed = m.clone();
        changed["personalBootstrap"][field] = value;
        assert!(manifest::inspect_fixture(&serde_json::to_vec(&changed)?).is_err(), "{field}");
    }
    let mut changed = m.clone();
    changed["personalBootstrap"]["issuerPublicKey"] = json!(format!(
        "{}=",
        m["personalBootstrap"]["issuerPublicKey"].as_str().ok_or_else(|| anyhow::anyhow!("fixture sem chave"))?
    ));
    assert!(manifest::inspect_fixture(&serde_json::to_vec(&changed)?).is_err());
    for raw in [
        b"{\"a\":1,\"a\":2}".as_slice(),
        b"[1.0]",
        b"[1e0]",
        b"[-0]",
        b"[9007199254740992]",
        b" [1]",
        b"\xef\xbb\xbf[1]",
        b"[\"\\ud800\"]",
    ] {
        assert!(protocol::canonical(raw).is_err());
    }
    assert!(protocol::canonical(&vec![b' '; protocol::LIMIT + 1]).is_err());
    // Nem uma fixture válida oferece raiz ao runtime: o seam é somente cfg(test).
    assert!(manifest::admitted()?.is_none());
    Ok(())
}

#[tokio::test]
async fn personal_status_dispatch_is_local_closed_and_read_only() -> Result<()> {
    let store = crate::store::Store::open(std::path::Path::new(":memory:"))?;
    let directory = store.engine_setup_root.clone();
    std::fs::create_dir_all(&directory)?;
    let identity = crate::remote::identity::Identity::load_or_create(&directory)?;
    let hub = crate::hub::Hub::new(store, "personal-fixture".into(), identity, "fixture-token".into(), 0);
    let before = hub.store.current_rev();
    let answer = crate::api::dispatch(&hub, &Caller::Local, super::STATUS_METHOD, json!({})).await?;
    assert_eq!(answer, json!({"profile":"personal-linux-v1","wire":1,"available":false,"reason":"updateRequired"}));
    assert_eq!(before, hub.store.current_rev());
    for caller in [
        Caller::Device { key: "fixture".into(), scopes: vec![Scope::Control, Scope::OwnerDevices], autoridade: None },
        Caller::Pairing { key: "fixture".into() },
        Caller::Bootstrap { key: "fixture".into(), bootstrap_id: "fixture".into() },
    ] {
        assert!(crate::api::dispatch(&hub, &caller, super::STATUS_METHOD, json!({})).await.is_err());
    }
    assert!(status(&Caller::Local, &json!({"manifest":vectors()?["personalBootstrap"]["manifest"]})).is_err());
    assert!(status(&Caller::Local, &Value::Null).is_err());
    drop(hub);
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

fn wire_verifier(v: &Value) -> Result<super::wire::Verifier> {
    let s = |k: &str| {
        v["wireVectors"]["keys"][k].as_str().map(str::to_owned).ok_or_else(|| anyhow::anyhow!("fixture sem chave"))
    };
    super::wire::Verifier::new(
        v["context"].clone(),
        super::wire::Keys { issuer: s("issuer")?, app: s("app")?, hub: s("hub")?, box_key: s("box")? },
    )
}
#[test]
fn personal_all_envelopes_crypto_composition_and_pop() -> Result<()> {
    use super::{shapes::Kind, wire};
    let all = vectors()?;
    let v = &all["personalBootstrap"];
    let w = &v["wireVectors"];
    let verifier = wire_verifier(v)?;
    let now = wire::Time::Current(w["now"].as_u64().ok_or_else(|| anyhow::anyhow!("fixture sem tempo"))?);
    let packet = wire::ceremony(
        &verifier,
        w["packetCanonical"].as_str().ok_or_else(|| anyhow::anyhow!("fixture sem pacote"))?.as_bytes(),
        now,
    )?;
    assert_eq!(packet.tuple(), &w["tuple"]);
    assert_eq!(packet.digest(), w["tupleHash"]);
    for (name, expected) in w["payloadHashes"].as_object().ok_or_else(|| anyhow::anyhow!("fixture sem hashes"))? {
        let kind = Kind::parse(name).ok_or_else(|| anyhow::anyhow!("fixture com tipo desconhecido"))?;
        let value = if w["packet"][name].is_null() { &w["alternative"][name] } else { &w["packet"][name] };
        let bytes = serde_json::to_vec(value)?;
        let checked = verifier.verify(kind, &bytes, now, wire::Reader::App)?;
        assert_eq!(checked.digest(), *expected);
        assert_eq!(checked.original_bytes(), bytes.as_slice());
    }
    assert_eq!(
        verifier.verify(Kind::ChannelOpen, &serde_json::to_vec(&w["hostOpen"])?, now, wire::Reader::App)?.payload()[2],
        "host"
    );
    assert_eq!(wire::ceremony(&verifier, &serde_json::to_vec(&w["alternative"])?, now)?.tuple()[2], "device");
    assert_eq!(
        verifier.verify(Kind::Read, &serde_json::to_vec(&w["readHub"])?, now, wire::Reader::Hub)?.payload(),
        &w["packet"]["read"][0]
    );
    let open = &w["hostOpen"][0];
    let request = wire::OpenRequest {
        role: wire::Reader::Hub,
        kid: w["keys"]["hub"].as_str().ok_or_else(|| anyhow::anyhow!("fixture sem H"))?,
        nonce: open[8].as_str().ok_or_else(|| anyhow::anyhow!("fixture sem nonce"))?,
        path: "/v1/personal-relay/host",
        until: open[12].as_u64().ok_or_else(|| anyhow::anyhow!("fixture sem prazo"))?,
    };
    assert_eq!(verifier.verify_open(&serde_json::to_vec(&w["hostOpen"])?, now, &request)?.payload(), open);
    let wrong = wire::OpenRequest { nonce: "invalid", ..request };
    assert!(verifier.verify_open(&serde_json::to_vec(&w["hostOpen"])?, now, &wrong).is_err());
    let private = crate::remote::crypto::unb64_n::<32>(
        w["seeds"]["box"].as_str().ok_or_else(|| anyhow::anyhow!("fixture sem caixa"))?,
    )?;
    let secret = wire::box_open(
        &x25519_dalek::StaticSecret::from(private),
        packet.original(Kind::BoxChallenge)?,
        packet.digest(),
        packet.original(Kind::AppConsent)?.digest(),
        packet.original(Kind::LocalConsent)?.digest(),
    )?;
    assert_eq!(secret.as_str(), w["pop"]["secret"]);
    assert_eq!(
        wire::ceremony(
            &verifier,
            w["packetCanonical"].as_str().ok_or_else(|| anyhow::anyhow!("fixture sem pacote"))?.as_bytes(),
            wire::Time::Historical(w["now"].as_u64().ok_or_else(|| anyhow::anyhow!("fixture sem tempo"))? + 600_000)
        )?
        .digest(),
        w["tupleHash"]
    );
    Ok(())
}

#[test]
fn personal_signed_alternatives_and_pop_binding_are_rejected() -> Result<()> {
    use super::{shapes::Kind, wire};
    let all = vectors()?;
    let v = &all["personalBootstrap"];
    let w = &v["wireVectors"];
    let verifier = wire_verifier(v)?;
    let now = wire::Time::Current(w["now"].as_u64().ok_or_else(|| anyhow::anyhow!("fixture sem tempo"))?);
    for (name, role, index, value) in [
        ("channel-open", "app", 2, json!("host")),
        ("channel-open", "app", 3, json!("possession")),
        ("host-state", "issuer", 12, json!(["owner.devices", "control"])),
        ("host-state", "issuer", 14, json!(1)),
        ("activation", "hub", 9, w["keys"]["app"].clone()),
        ("proof", "app", 5, json!("0".repeat(64))),
        ("local-consent", "hub", 3, json!("0".repeat(64))),
        ("host-state", "hub", 0, json!(wire::domain("host-state"))),
        ("host-state", "issuer", 0, json!(wire::domain("state"))),
        ("host-state", "issuer", 1, json!(["ACS4", 1, "dev", "https://cloud.example.invalid"])),
    ] {
        let mut packet = w["packet"].clone();
        packet[name][0][index] = value;
        let seed = crate::remote::crypto::unb64_n::<32>(
            w["seeds"][role].as_str().ok_or_else(|| anyhow::anyhow!("fixture sem seed"))?,
        )?;
        packet[name][1] = json!(crate::remote::crypto::b64(&crate::remote::crypto::sign(
            &ed25519_dalek::SigningKey::from_bytes(&seed),
            &serde_json::to_vec(&packet[name][0])?
        )));
        assert!(wire::ceremony(&verifier, &serde_json::to_vec(&packet)?, now).is_err(), "{name}");
    }
    let b = verifier.verify(
        Kind::BoxChallenge,
        &serde_json::to_vec(&w["packet"]["box-challenge"])?,
        now,
        wire::Reader::App,
    )?;
    let private = crate::remote::crypto::unb64_n::<32>(
        w["seeds"]["box"].as_str().ok_or_else(|| anyhow::anyhow!("fixture sem caixa"))?,
    )?;
    assert!(
        wire::box_open(
            &x25519_dalek::StaticSecret::from(private),
            &b,
            w["tupleHash"].as_str().ok_or_else(|| anyhow::anyhow!("fixture sem T"))?,
            &"0".repeat(64),
            w["payloadHashes"]["local-consent"].as_str().ok_or_else(|| anyhow::anyhow!("fixture sem CH"))?
        )
        .is_err()
    );
    assert!(crate::remote::crypto::x25519(&x25519_dalek::StaticSecret::from(private), &[0; 32]).is_err());
    assert!(
        verifier
            .verify(
                Kind::Grant,
                &serde_json::to_vec(&w["packet"]["grant"])?,
                wire::Time::Current(w["now"].as_u64().ok_or_else(|| anyhow::anyhow!("fixture sem tempo"))? + 600_000),
                wire::Reader::App
            )
            .is_err()
    );
    Ok(())
}

fn wire_fixture_seal(payload: &Value, seed: &Value) -> Result<Value> {
    let seed = seed.as_str().ok_or_else(|| anyhow::anyhow!("fixture sem seed"))?;
    let seed = crate::remote::crypto::unb64_n::<32>(seed)?;
    let signature =
        crate::remote::crypto::sign(&ed25519_dalek::SigningKey::from_bytes(&seed), &serde_json::to_vec(payload)?);
    Ok(json!([payload, crate::remote::crypto::b64(&signature)]))
}

#[test]
fn personal_fence_ack_rejects_null_y_for_receipt_w() -> Result<()> {
    use super::{shapes::Kind, wire};
    let all = vectors()?;
    let v = &all["personalBootstrap"];
    let w = &v["wireVectors"];
    let verifier = wire_verifier(v)?;
    let now = wire::Time::Current(w["now"].as_u64().ok_or_else(|| anyhow::anyhow!("fixture sem tempo"))?);
    let mut packet = w["packet"].clone();
    // S guardaria hY antigo e mascararia a relação W/Y que esta regressão isola.
    packet.as_object_mut().ok_or_else(|| anyhow::anyhow!("fixture sem pacote"))?.remove("state");
    assert_eq!(wire::ceremony(&verifier, &serde_json::to_vec(&packet)?, now)?.digest(), w["tupleHash"]);
    packet["fence-ack"][0][3] = json!(wire::hash(&packet["fence"][0])?);
    packet["fence-ack"][0][4] = Value::Null;
    packet["fence-ack"][0][5] = Value::Null;
    packet["fence-ack"] = wire_fixture_seal(&packet["fence-ack"][0], &w["seeds"]["hub"])?;
    let fence = verifier.verify(Kind::Fence, &serde_json::to_vec(&packet["fence"])?, now, wire::Reader::App)?;
    let ack = verifier.verify(Kind::FenceAck, &serde_json::to_vec(&packet["fence-ack"])?, now, wire::Reader::App)?;
    assert_eq!(ack.payload()[3], fence.digest());
    assert_eq!(fence.payload()[4], packet["receipt"][0][6]);
    assert_eq!(fence.payload()[5], wire::hash(&packet["receipt"][0])?);
    assert!(ack.payload()[4].is_null() && ack.payload()[5].is_null());
    assert!(
        wire::ceremony(&verifier, &serde_json::to_vec(&packet)?, now).is_err(),
        "W com R deve recusar Y null/null, mesmo com assinatura H e hW corretos"
    );
    Ok(())
}

#[test]
fn personal_fence_ack_rejects_receipt_y_for_null_w() -> Result<()> {
    use super::{shapes::Kind, wire};
    let all = vectors()?;
    let v = &all["personalBootstrap"];
    let w = &v["wireVectors"];
    let verifier = wire_verifier(v)?;
    let now = wire::Time::Current(w["now"].as_u64().ok_or_else(|| anyhow::anyhow!("fixture sem tempo"))?);
    let mut packet = w["packet"].clone();
    packet.as_object_mut().ok_or_else(|| anyhow::anyhow!("fixture sem pacote"))?.remove("state");
    assert_eq!(wire::ceremony(&verifier, &serde_json::to_vec(&packet)?, now)?.digest(), w["tupleHash"]);
    packet["fence"][0][4] = Value::Null;
    packet["fence"][0][5] = Value::Null;
    packet["fence"] = wire_fixture_seal(&packet["fence"][0], &w["seeds"]["issuer"])?;
    packet["fence-ack"][0][3] = json!(wire::hash(&packet["fence"][0])?);
    packet["fence-ack"] = wire_fixture_seal(&packet["fence-ack"][0], &w["seeds"]["hub"])?;
    let fence = verifier.verify(Kind::Fence, &serde_json::to_vec(&packet["fence"])?, now, wire::Reader::App)?;
    let ack = verifier.verify(Kind::FenceAck, &serde_json::to_vec(&packet["fence-ack"])?, now, wire::Reader::App)?;
    assert_eq!(ack.payload()[3], fence.digest());
    assert!(fence.payload()[4].is_null() && fence.payload()[5].is_null());
    assert_eq!(ack.payload()[4], packet["receipt"][0][6]);
    assert_eq!(ack.payload()[5], wire::hash(&packet["receipt"][0])?);
    assert!(
        wire::ceremony(&verifier, &serde_json::to_vec(&packet)?, now).is_err(),
        "W null/null deve recusar Y com R, mesmo com assinaturas E/H e hW corretos"
    );
    Ok(())
}
