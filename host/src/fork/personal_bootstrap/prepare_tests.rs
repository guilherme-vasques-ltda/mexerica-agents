//! Fronteiras externas Worker/helper são controladas; SQLite, FS e crypto são reais.
use super::*;
use crate::{remote::identity::Identity, store::Store};
use chacha20poly1305::{
    ChaCha20Poly1305, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use ed25519_dalek::SigningKey;
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
fn vectors() -> Result<Value> {
    Ok(serde_json::from_str(include_str!("../../../../docs/reference/fixtures/remote-relay-vectors.json"))?)
}
fn signed(seed: &str, p: Value) -> Result<Value> {
    let key = SigningKey::from_bytes(&crypto::unb64_n::<32>(seed)?);
    let signature = crypto::b64(&crypto::sign(&key, &serde_json::to_vec(&p)?));
    Ok(Value::Array(vec![p, Value::String(signature)]))
}
struct ExternalWorker {
    originals: Value,
    issuer: String,
    app: String,
    context: Value,
    posts: HashMap<String, usize>,
    lost_candidate: bool,
    lost_begin: bool,
    terminal: bool,
    now: u64,
}
impl ExternalWorker {
    fn request(&mut self, route: &str, raw: &[u8], header: &str, host: &str) -> Result<Vec<u8>> {
        let b = protocol::packet(raw)?;
        let fields = header
            .split(',')
            .map(|p| p.split_once('=').context("HTTP sig field"))
            .collect::<Result<HashMap<_, _>>>()?;
        let ts = fields.get("ts").context("HTTP time")?.parse::<i64>()?;
        let nonce = *fields.get("nonce").context("HTTP nonce")?;
        ensure!(fields.len() == 5 && fields["kid"] == host && fields["v"] == "1", "HTTP role/fields");
        crypto::unb64_n::<16>(nonce)?;
        let input = crypto::request_sig_input(
            "POST",
            "cloud.example.invalid",
            &format!("/v1/personal-bootstrap/{route}"),
            ts,
            nonce,
            raw,
        );
        crypto::verify(&crypto::unb64_n::<32>(host)?, input.as_bytes(), &crypto::unb64_n::<64>(fields["sig"])?)?;
        self.now = u64::try_from(ts)?;
        *self.posts.entry(route.into()).or_default() += 1;
        let out = match route {
            "candidate" => {
                self.originals["local-ticket"] = b["ticket"].clone();
                if self.lost_candidate {
                    self.lost_candidate = false;
                    anyhow::bail!("controlled lost candidate reply");
                }
                json!({"intentId":b["ticket"][0][2],"candidateId":b["ticket"][0][4]})
            }
            "device" => {
                self.originals["local-ticket"] = b["ticket"].clone();
                let at = self.now + 47;
                let until = b["begin"][0][7].as_u64().context("begin expiry")? - 100;
                let out = json!({"intentId":b["ticket"][0][2],"candidateId":b["ticket"][0][4],"userCode":"ABCD2345EFGH","deviceCode":crypto::b64(&[77;32]),"createdAt":at,"expiresAt":until,"intervalMs":5000});
                if self.lost_begin {
                    self.lost_begin = false;
                    anyhow::bail!("controlled lost begin reply");
                }
                out
            }
            "read" => {
                let cfg = super::super::provisioning::fixture()?.0;
                let c = ceremony::history(&cfg, &self.originals, self.now)?;
                let g = self.originals.get("grant");
                // HOST-R1-01: o read de testemunho reflete o Worker real antes de R (personalConfirmed).
                let witness = b.get("witness") == Some(&json!(true)) && g.is_some() && !self.terminal;
                let phase = if self.terminal {
                    "expired"
                } else if witness {
                    "personalConfirmed"
                } else if g.is_some() {
                    "hubPending"
                } else if self.originals.get("local-consent").is_some() {
                    "personalConfirmed"
                } else {
                    "open"
                };
                let state = signed(
                    &self.issuer,
                    json!([
                        wire::domain("state"),
                        self.context,
                        c.tuple()[1],
                        c.tuple()[9],
                        c.digest(),
                        phase,
                        1,
                        null,
                        g.map(|g| wire::hash(&g[0])).transpose()?,
                        null,
                        null,
                        null,
                        null,
                        null,
                        b["read"][0][5],
                        self.now,
                        c.tuple()[18]
                    ]),
                )?;
                let mut originals = serde_json::Map::new();
                for (remote, local) in [
                    ("intent", "intent"),
                    ("ticket", "local-ticket"),
                    ("appConsent", "app-consent"),
                    ("localConsent", "local-consent"),
                    ("boxChallenge", "box-challenge"),
                    ("boxProof", "box-proof"),
                    ("approval", "approve"),
                    ("localConfirmation", "local-confirm"),
                    ("consume", "consume"),
                    ("grant", "grant"),
                    ("challenge", "challenge"),
                    ("proof", "proof"),
                    ("receipt", "receipt"),
                    ("publish", "publish"),
                    ("activation", "activation"),
                    ("revocation", "revocation"),
                    ("fence", "fence"),
                    ("fenceAck", "fence-ack"),
                ] {
                    originals.insert(remote.into(), self.originals.get(local).cloned().unwrap_or(Value::Null));
                }
                let mut reply = json!({"state":state,"originals":originals,"activationCurrent":null});
                if witness {
                    reply["consumptionWitness"] = signed(
                        &self.issuer,
                        json!([
                            wire::domain("consumption-witness"),
                            self.context,
                            c.digest(),
                            g.map(|g| wire::hash(&g[0])).transpose()?,
                            cfg.identity().computer_id,
                            b["read"][0][5],
                            self.now
                        ]),
                    )?;
                }
                reply
            }
            "consume" => {
                for (remote, local) in [
                    ("intent", "intent"),
                    ("ticket", "local-ticket"),
                    ("appConsent", "app-consent"),
                    ("localConsent", "local-consent"),
                    ("boxChallenge", "box-challenge"),
                    ("boxProof", "box-proof"),
                    ("approval", "approve"),
                    ("localConfirmation", "local-confirm"),
                    ("consume", "consume"),
                ] {
                    ensure!(self.originals[local] == b[remote] || local == "consume", "consume original changed");
                    self.originals[local] = b[remote].clone();
                }
                let cfg = super::super::provisioning::fixture()?.0;
                let c = ceremony::checked(&cfg, &self.originals, self.now)?;
                let p = json!([
                    wire::domain("grant"),
                    self.context,
                    c.tuple(),
                    c.original(super::super::shapes::Kind::BoxChallenge)?.digest(),
                    c.original(super::super::shapes::Kind::BoxProof)?.digest(),
                    c.original(super::super::shapes::Kind::Approve)?.digest(),
                    c.original(super::super::shapes::Kind::LocalConfirm)?.digest(),
                    c.original(super::super::shapes::Kind::Consume)?.digest(),
                    crypto::b64(&[76; 32]),
                    self.now,
                    (self.now + 60_000).min(c.tuple()[18].as_u64().context("T expiry")?),
                    ["control", "owner.devices"]
                ]);
                let g = signed(&self.issuer, p)?;
                self.originals["grant"] = g.clone();
                json!({"intentId":c.tuple()[1],"candidateId":c.tuple()[9],"grant":g})
            }
            "local-consent" | "box-proof" | "local-confirm" => {
                let field = match route {
                    "local-consent" => "localConsent",
                    "box-proof" => "boxProof",
                    _ => "localConfirmation",
                };
                let value = b[field].clone();
                crypto::verify(
                    &crypto::unb64_n::<32>(host)?,
                    &serde_json::to_vec(&value[0])?,
                    &pw::key::<64>(&value[1])?,
                )?;
                self.originals[route] = value;
                json!({"intentId":self.originals["intent"][0][2],"candidateId":self.originals["local-ticket"][0][4]})
            }
            _ => anyhow::bail!("unexpected external route"),
        };
        Ok(serde_json::to_vec(&out)?)
    }
}
struct Fixture {
    hub: Arc<Hub>,
    cloud: Arc<std::sync::Mutex<ExternalWorker>>,
    dir: PathBuf,
    v: Value,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Result<Self> {
        let v = vectors()?;
        let w = &v["personalBootstrap"]["wireVectors"];
        let (cfg, identity) = super::super::provisioning::fixture()?;
        let dir = std::env::temp_dir().join(format!("personal-prepare-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir)?;
        let store = Store::open(&dir.join("codync.db"))?;
        store.personal_install_generation(&cfg)?;
        let hub = Hub::new(store, "synthetic".into(), identity, "synthetic".into(), 0);
        hub.personal.install(cfg, None, Clock::fixture(w["now"].as_u64().context("fixture epoch")?)?)?;
        let cloud = Arc::new(std::sync::Mutex::new(ExternalWorker {
            originals: json!({"intent":w["packet"]["intent"]}),
            issuer: w["seeds"]["issuer"].as_str().context("issuer seed")?.into(),
            app: w["seeds"]["app"].as_str().context("A seed")?.into(),
            context: v["personalBootstrap"]["context"].clone(),
            posts: HashMap::new(),
            lost_candidate: false,
            lost_begin: false,
            terminal: false,
            now: w["now"].as_u64().context("epoch")?,
        }));
        let worker = cloud.clone();
        let host = hub.identity.sign_pub_b64();
        *hub.personal.test_transport.locked() = Some(Arc::new(move |route, bytes, header| {
            let worker = worker.clone();
            let host = host.clone();
            Box::pin(async move { worker.locked().request(&route, &bytes, &header, &host) })
        }));
        let root = dir.join("root");
        std::fs::create_dir(&root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o750))?;
        }
        *hub.personal.test_journal.locked() = Some(root.join("journal"));
        Ok(Self { hub, cloud, dir, v, root })
    }
    fn write_root(&self, originals: &Value, revision: u64) -> Result<()> {
        let w = &self.v["personalBootstrap"]["wireVectors"];
        let cfg: Value = self.hub.personal.with(|f| Ok(serde_json::from_slice(f.config.original_bytes())?))?;
        let req = w["packet"]["time-request"].clone();
        let time = signed(
            w["seeds"]["issuer"].as_str().context("E")?,
            json!([
                wire::domain("time"),
                cfg["context"],
                req[0][2],
                req[0][3],
                cfg["clock"]["observedEpochMs"],
                cfg["clock"]["validForMs"]
            ]),
        )?;
        let j = json!({"schema":1,"profile":"personal-linux-v1","wire":1,"context":cfg["context"],"identity":cfg["identity"],"revision":revision,"clock_evidence":{"timeRequest":req,"time":time,"bootId":cfg["clock"]["bootId"],"sentBoottimeMs":100,"receivedBoottimeMs":100},"originals":originals});
        std::fs::write(self.root.join("journal"), serde_json::to_vec(&j)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(self.root.join("journal"), std::fs::Permissions::from_mode(0o640))?;
        }
        Ok(())
    }
    async fn ticket(&self, mode: &str) -> Result<Value> {
        ceremony::helper(
            &self.hub,
            "root-session",
            &json!([
                super::super::helper::DOMAIN,
                1,
                crypto::b64(&[1; 16]),
                "ticket",
                mode,
                if mode == "command" {
                    self.v["personalBootstrap"]["wireVectors"]["packet"]["intent"].clone()
                } else {
                    Value::Null
                }
            ]),
        )
        .await
    }
    fn bound(&self, q: &Value) -> Result<Bound> {
        let c = self.v["personalBootstrap"]["context"].clone();
        Bound::lookup(
            &self.hub,
            &json!([
                wire::domain("prepare-hello"),
                1,
                c,
                pw::capture(&c, q)?,
                wire::hash(&q[0])?,
                protocol::hash(&serde_json::to_vec(q)?),
                q[0][3],
                if q[0][3] == "command" {
                    self.v["personalBootstrap"]["wireVectors"]["keys"]["app"].clone()
                } else {
                    Value::Null
                },
                crypto::b64(&[8; 32]),
                crypto::b64(&[9; 32])
            ]),
        )
    }
    fn candidate(b: &Bound, token: &str) -> Result<Value> {
        let body =
            format!("{{\"ticket\":{},\"token\":{}}}", serde_json::to_string(&b.q)?, serde_json::to_string(token)?);
        let digest = protocol::hash(body.as_bytes());
        Ok(json!([
            wire::domain("prepare-request"),
            1,
            crypto::b64(&[84; 16]),
            b.hq,
            "candidate",
            pw::operation(b.cfg.context(), &b.public, &b.hq, &b.wire_hash, &digest)?,
            digest,
            token
        ]))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
#[tokio::test]
async fn prepare_command_digest_real_sqlite_unknown_reconciles_without_second_post() -> Result<()> {
    let f = Fixture::new()?;
    let q = f.ticket("command").await?;
    let b = f.bound(&q)?;
    let token = crypto::b64(&[83; 32]);
    let request = Fixture::candidate(&b, &token)?;
    // Crash antes de possibleSend: declaração pública já durável; ressuprimento
    // usa exatamente os originais, pela mesma API Connection/FULL de produção.
    let i = b.intent.as_ref().context("I")?;
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
        request[6]
    ]);
    ceremony::step(&f.hub, &b.id, |_, _, s| {
        assert_eq!(db::declare(s.db, &b.public, &declaration)?, "preparedNotSent");
        assert_eq!(s.db.pragma_query_value(None, "synchronous", |r| r.get::<_, i64>(0))?, 2);
        assert_eq!(s.db.pragma_query_value(None, "recursive_triggers", |r| r.get::<_, i64>(0))?, 1);
        Ok(())
    })?;
    f.cloud.locked().lost_candidate = true;
    assert!(operation(&f.hub, &b, &request).await.is_err());
    assert_eq!(f.cloud.locked().posts["candidate"], 1);
    assert!(f.hub.store.bootstrap_claim_a1(None, "other", |_| panic!("PL1 loser must not sign")).is_err());
    let result = operation(&f.hub, &b, &request).await?;
    assert_eq!(result[4], "ok");
    assert_eq!(f.cloud.locked().posts["candidate"], 1);
    let changed = Fixture::candidate(&b, &crypto::b64(&[82; 32]))?;
    assert!(operation(&f.hub, &b, &changed).await.is_err());
    f.hub.personal.with(|p| {
        f.hub.store.personal_session(&p.config, &b.id, |s| {
            let history: String =
                s.db.query_row("SELECT declaration FROM personal_prepare_operations", [], |r| r.get(0))?;
            assert!(!history.contains(&token));
            assert!(!serde_json::to_string(s.originals())?.contains(&token));
            assert_eq!(s.db.query_row("SELECT count(*) FROM devices", [], |r| r.get::<_, i64>(0))?, 0);
            Ok(())
        })
    })?;
    Ok(())
}
#[tokio::test]
async fn prepare_device_original_worker_times_cipher_and_lost_begin_no_reissue() -> Result<()> {
    let f = Fixture::new()?;
    let q = f.ticket("device").await?;
    let b = f.bound(&q)?;
    let result =
        operation(&f.hub, &b, &json!([wire::domain("prepare-request"), 1, crypto::b64(&[2; 16]), b.hq, "enrollment"]))
            .await?;
    let facts = &result[6];
    assert_ne!(facts[5], q[0][11]);
    assert_ne!(facts[6], q[0][12]);
    assert_eq!(facts[7], 5000);
    assert!(!serde_json::to_string(&result)?.contains(&crypto::b64(&[77; 32])));
    let (binding, public, sealed) = ceremony::step(&f.hub, &b.id, |cfg, _, s| {
        let binding = enrollment::Binding::new(cfg, &q, &s.originals()["_begin"])?;
        let (public, sealed) = db::enrollment(s.db, &b.public)?.context("cipher missing")?;
        assert!(!serde_json::to_string(s.originals())?.contains(&crypto::b64(&[77; 32])));
        Ok((binding, public, sealed))
    })?;
    let raw = binding.open(&f.hub.identity, &sealed, &public)?;
    assert!(String::from_utf8_lossy(&raw).contains(&crypto::b64(&[77; 32])));
    let mut corrupt = sealed.clone();
    if let Some(last) = corrupt.last_mut() {
        *last ^= 1;
    }
    assert!(binding.open(&f.hub.identity, &corrupt, &public).is_err());
    let wrong = Identity { sign: SigningKey::from_bytes(&[3; 32]), box_secret: StaticSecret::from([4; 32]) };
    assert!(binding.open(&wrong, &sealed, &public).is_err());
    assert!(operation(&f.hub, &b, &Fixture::candidate(&b, &crypto::b64(&[83; 32]))?).await.is_err());
    let lost = Fixture::new()?;
    lost.cloud.locked().lost_begin = true;
    assert!(lost.ticket("device").await.is_err());
    assert!(lost.ticket("device").await.is_err());
    assert_eq!(lost.cloud.locked().posts["device"], 1);
    Ok(())
}
#[test]
fn prepare_cross_language_transcript_and_frame_are_exact_and_purpose_separated() -> Result<()> {
    let vectors = vectors()?;
    let wire_vectors = &vectors["personalBootstrap"]["wireVectors"];
    let context = &vectors["personalBootstrap"]["context"];
    let ticket = &wire_vectors["packet"]["local-ticket"];
    let cap = pw::capture(context, ticket)?;
    let hq = wire::hash(&ticket[0])?;
    let wh = protocol::hash(&serde_json::to_vec(ticket)?);
    let app_ephemeral = StaticSecret::from(crypto::unb64_n::<32>(
        vectors["handshake"]["deviceEphPriv"].as_str().context("ephemeral A")?,
    )?);
    let hub_ephemeral = StaticSecret::from(crypto::unb64_n::<32>(
        vectors["handshake"]["hostEphPriv"].as_str().context("ephemeral H")?,
    )?);
    let hello = json!([
        wire::domain("prepare-hello"),
        1,
        context,
        cap,
        hq,
        wh,
        "command",
        wire_vectors["keys"]["app"],
        crypto::b64(&crypto::x25519_pub(&app_ephemeral)),
        crypto::b64(&[81; 32])
    ]);
    let welcome = json!([
        wire::domain("prepare-welcome"),
        1,
        context,
        cap,
        hq,
        wh,
        crypto::b64(&crypto::x25519_pub(&hub_ephemeral)),
        crypto::b64(&[82; 32])
    ]);
    let th = pw::transcript(&hello, &welcome)?;
    assert_eq!(
        protocol::hash(&serde_json::to_vec(&json!([
            wire::domain("prepare-transcript"),
            1,
            "GET",
            "/personal-prepare",
            hello,
            welcome
        ]))?),
        "014fc9fd817b2269abe353795a34b94a3aa95ae27c1a45a6957bc630b1018b4e"
    );
    let body = format!(
        "{{\"ticket\":{},\"token\":{}}}",
        serde_json::to_string(ticket)?,
        serde_json::to_string(&crypto::b64(&[83; 32]))?
    );
    let digest = protocol::hash(body.as_bytes());
    let op = pw::operation(context, &cap, &hq, &wh, &digest)?;
    assert_eq!(op, "445831d23af34ac025ebfd100584bb43524a9c4993eb77358bc0d028a79ca176");
    let request = json!([
        wire::domain("prepare-request"),
        1,
        crypto::b64(&[84; 16]),
        hq,
        "candidate",
        op,
        digest,
        crypto::b64(&[83; 32])
    ]);
    let shared = crypto::x25519(&app_ephemeral, &crypto::x25519_pub(&hub_ephemeral))?;
    let mut key = [0; 32];
    Hkdf::<Sha256>::new(Some(&th), &shared)
        .expand(wire::domain("prepare-key-a2h").as_bytes(), &mut key)
        .map_err(|_| anyhow::anyhow!("test KDF"))?;
    let aad = serde_json::to_vec(&json!([
        wire::domain("prepare-frame"),
        1,
        context,
        cap,
        hq,
        wh,
        crypto::b64(&th),
        "a2h",
        0
    ]))?;
    let cipher = ChaCha20Poly1305::new_from_slice(&key).map_err(|_| anyhow::anyhow!("cipher"))?;
    let ct = cipher
        .encrypt(&Nonce::from([0; 12]), Payload { msg: &serde_json::to_vec(&request)?, aad: &aad })
        .map_err(|_| anyhow::anyhow!("seal"))?;
    let frame = json!([wire::domain("prepare-frame"), 1, "a2h", 0, crypto::b64(&ct)]);
    assert_eq!(wire::hash(&frame)?, "c746436aa5519f1d30e1851cfaf91755918c1e7361bd21bc2f4e9c7776e8c193");
    let mut frames = pw::Frames::new(&hello, th, &shared)?;
    assert_eq!(protocol::canonical(&frames.open_request(&frame)?)?, request);
    assert!(frames.open_request(&frame).is_err());
    for index in [0, 2, 3] {
        let mut changed = frame.clone();
        changed[index] = json!("wrong");
        assert!(pw::Frames::new(&hello, th, &shared)?.open_request(&changed).is_err());
    }
    assert!(pw::Frames::new(&hello, th, &[0; 32]).is_err());
    let binary = pw::binary(&hello, 16_384)?;
    assert_eq!(pw::message(&binary, 16_384)?, hello);
    assert!(pw::message(&binary[..binary.len() - 1], 16_384).is_err());
    Ok(())
}

#[tokio::test]
async fn personal_hub_vertical_root_consent_pop_consume_and_possession_handler_real_sqlite() -> Result<()> {
    use super::super::{channel, helper, shapes::Kind};
    use futures::{SinkExt, StreamExt};
    let fixture = Fixture::new()?;
    let ticket = fixture.ticket("command").await?;
    let bound = fixture.bound(&ticket)?;
    operation(&fixture.hub, &bound, &Fixture::candidate(&bound, &crypto::b64(&[83; 32]))?).await?;
    let (ceremony, now) = ceremony::step(&fixture.hub, &bound.id, |cfg, clock, s| {
        Ok((ceremony::checked(cfg, s.originals(), clock.now()?)?, clock.now()?))
    })?;
    let app = fixture.cloud.locked().app.clone();
    let ca = signed(
        &app,
        json!([
            wire::domain("app-consent"),
            bound.cfg.context(),
            ceremony.digest(),
            "independent-identity-v1",
            "approve",
            now,
            ceremony.tuple()[18]
        ]),
    )?;
    fixture.cloud.locked().originals["app-consent"] = ca.clone();
    let mut prefix = json!({"intent":bound.intent,"local-ticket":ticket,"app-consent":ca});
    fixture.write_root(&prefix, 1)?;
    let ch = ceremony::helper(
        &fixture.hub,
        &bound.id,
        &json!([helper::DOMAIN, 1, crypto::b64(&[10; 16]), "consent", prefix["intent"], ticket, ca, 1]),
    )
    .await?;
    prefix["local-consent"] = ch.clone();
    fixture.write_root(&prefix, 2)?;
    ceremony::helper(
        &fixture.hub,
        &bound.id,
        &json!([helper::DOMAIN, 1, crypto::b64(&[11; 16]), "retained", "localConsent", ch, 2]),
    )
    .await?;
    let status = || json!([helper::DOMAIN, 1, crypto::b64(&crypto::random::<16>()), "status", ceremony.digest()]);
    ceremony::helper(&fixture.hub, &bound.id, &status()).await?;
    let now = ceremony::step(&fixture.hub, &bound.id, |_, clock, _| clock.now())?;
    let ephemeral_key = StaticSecret::from(crypto::unb64_n::<32>(
        fixture.v["personalBootstrap"]["wireVectors"]["seeds"]["ephemeral"].as_str().context("ephemeral seed")?,
    )?);
    let secret = crypto::b64(&[55; 32]);
    let hca = wire::hash(&ca[0])?;
    let hch = wire::hash(&ch[0])?;
    let nonce = crypto::b64(&[56; 32]);
    let epk = crypto::b64(&crypto::x25519_pub(&ephemeral_key));
    let sh = wire::hash(&json!([wire::domain("box-secret-hash"), secret]))?;
    let end = now + 60_000;
    let aad = serde_json::to_vec(&json!([
        wire::domain("box-aad"),
        bound.cfg.context(),
        ceremony.digest(),
        hca,
        hch,
        nonce,
        epk,
        sh,
        now,
        end
    ]))?;
    let shared = crypto::x25519(&ephemeral_key, &fixture.hub.identity.box_pub())?;
    let mut key = [0; 32];
    Hkdf::<Sha256>::new(Some(&Sha256::digest(&aad)), &shared)
        .expand(wire::domain("box-key").as_bytes(), &mut key)
        .map_err(|_| anyhow::anyhow!("POP KDF"))?;
    let ciphertext = ChaCha20Poly1305::new_from_slice(&key)
        .map_err(|_| anyhow::anyhow!("POP cipher"))?
        .encrypt(
            &Nonce::from([0; 12]),
            Payload {
                msg: &serde_json::to_vec(&json!([wire::domain("box-secret-plaintext"), ceremony.digest(), secret]))?,
                aad: &aad,
            },
        )
        .map_err(|_| anyhow::anyhow!("POP seal"))?;
    let box_challenge = signed(
        &app,
        json!([
            wire::domain("box-challenge"),
            bound.cfg.context(),
            ceremony.digest(),
            hca,
            hch,
            nonce,
            epk,
            sh,
            crypto::b64(&ciphertext),
            now,
            end
        ]),
    )?;
    fixture.cloud.locked().originals["box-challenge"] = box_challenge;
    // Espera externa real: o ingresso de produção conserva seu piso BOOTTIME5s.
    tokio::time::sleep(Duration::from_millis(5010)).await;
    ceremony::helper(&fixture.hub, &bound.id, &status()).await?;
    let box_proof = fixture.cloud.locked().originals["box-proof"].clone();
    assert_eq!(box_proof[0][4], secret);
    let box_challenge = fixture.cloud.locked().originals["box-challenge"].clone();
    let now = ceremony::step(&fixture.hub, &bound.id, |_, clock, _| clock.now())?;
    let ap = signed(
        &app,
        json!([
            wire::domain("approve"),
            bound.cfg.context(),
            ceremony.digest(),
            wire::hash(&box_challenge[0])?,
            wire::hash(&box_proof[0])?,
            "independent-identity-v1",
            "approve",
            now,
            box_proof[0][7]
        ]),
    )?;
    fixture.cloud.locked().originals["approve"] = ap;
    tokio::time::sleep(Duration::from_millis(5010)).await;
    let st = ceremony::helper(&fixture.hub, &bound.id, &status()).await?;
    let confirmation = st[4].clone();
    assert!(!confirmation.is_null());
    assert!(fixture.cloud.locked().originals.get("local-confirm").is_none());
    let prefix = ceremony::step(&fixture.hub, &bound.id, |_, _, s| Ok(ceremony::packet(s.originals())))?;
    fixture.write_root(&prefix, 3)?;
    ceremony::helper(
        &fixture.hub,
        &bound.id,
        &json!([helper::DOMAIN, 1, crypto::b64(&[12; 16]), "retained", "localConfirmation", confirmation, 3]),
    )
    .await?;
    tokio::time::sleep(Duration::from_millis(5010)).await;
    ceremony::helper(&fixture.hub, &bound.id, &status()).await?;
    let (packet, grant) = ceremony::step(&fixture.hub, &bound.id, |cfg, clock, s| {
        let ceremony = ceremony::checked(cfg, s.originals(), clock.now()?)?;
        assert_eq!(ceremony.original(Kind::Grant)?.payload()[2], *ceremony.tuple());
        assert_eq!(ceremony.original(Kind::Grant)?.payload()[6], ceremony.original(Kind::LocalConfirm)?.digest());
        Ok((ceremony::packet(s.originals()), s.originals()["grant"].clone()))
    })?;
    // Laço WebSocket real, isolado no teste; nenhum serviço/produtor é instalado.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let router = axum::Router::new()
        .route("/personal-possession", axum::routing::get(channel::upgrade))
        .with_state(fixture.hub.clone());
    // O handler extrai o peer TCP real (D-2), como o serve do Host (main.rs).
    let server = tokio::spawn(async move {
        axum::serve(listener, router.into_make_service_with_connect_info::<std::net::SocketAddr>()).await
    });
    let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/personal-possession")).await?;
    let ephemeral_key = StaticSecret::from([61; 32]);
    let payload = json!([
        wire::domain("possession-hello"),
        bound.cfg.context(),
        ceremony.digest(),
        wire::hash(&grant[0])?,
        fixture.v["personalBootstrap"]["wireVectors"]["keys"]["app"],
        crypto::b64(&crypto::x25519_pub(&ephemeral_key)),
        crypto::b64(&[62; 32])
    ]);
    let akey = SigningKey::from_bytes(&crypto::unb64_n::<32>(&app)?);
    let mut hello = payload.as_array().context("hello")?.clone();
    hello.push(json!(crypto::b64(&crypto::sign(&akey, &serde_json::to_vec(&payload)?))));
    let hello = Value::Array(hello);
    socket.send(tokio_tungstenite::tungstenite::Message::Text(serde_json::to_string(&hello)?.into())).await?;
    let welcome: Value = serde_json::from_str(socket.next().await.context("welcome missing")??.to_text()?)?;
    let th =
        crypto::sha256(&[&serde_json::to_vec(&json!([wire::domain("possession-transcript"), hello, welcome[4]]))?]);
    assert_eq!(welcome[5], crypto::b64(&th));
    crypto::verify(&fixture.hub.identity.sign_pub(), &th, &pw::key::<64>(&welcome[6])?)?;
    let keys = crypto::channel_keys(&crypto::x25519(&ephemeral_key, &pw::key::<32>(&welcome[4])?)?, &th);
    let mut out = crypto::Sealer::new(keys.d2h);
    let mut incoming = crypto::Opener::new(keys.h2d, 16_384);
    let req = out.seal(&serde_json::to_vec(&json!(["challenge", grant]))?).context("seal challenge")?;
    socket.send(tokio_tungstenite::tungstenite::Message::Text(serde_json::to_string(&req[0])?.into())).await?;
    let frame: Value = serde_json::from_str(socket.next().await.context("K missing")??.to_text()?)?;
    let raw = incoming.open(&frame).map_err(|_| anyhow::anyhow!("K frame"))?.context("K fragment")?;
    let challenge = protocol::canonical(&raw)?;
    let now = ceremony::step(&fixture.hub, &bound.id, |_, clock, _| clock.now())?;
    let proof = signed(
        &app,
        json!([
            wire::domain("proof"),
            bound.cfg.context(),
            wire::hash(&grant[0])?,
            wire::hash(&challenge[0])?,
            ceremony.digest(),
            wire::hash(&packet["approve"][0])?,
            wire::hash(&confirmation[0])?,
            now,
            challenge[0][6]
        ]),
    )?;
    let req = out.seal(&serde_json::to_vec(&json!(["proof", grant, challenge, proof]))?).context("seal proof")?;
    socket.send(tokio_tungstenite::tungstenite::Message::Text(serde_json::to_string(&req[0])?.into())).await?;
    let frame: Value = serde_json::from_str(socket.next().await.context("R missing")??.to_text()?)?;
    let raw = incoming.open(&frame).map_err(|_| anyhow::anyhow!("R frame"))?.context("R fragment")?;
    let receipt = protocol::canonical(&raw)?;
    ceremony::step(&fixture.hub, &bound.id, |cfg, clock, s| {
        let checked = ceremony::checked(cfg, s.originals(), clock.now()?)?;
        assert_eq!(checked.original(Kind::Receipt)?.original_bytes(), raw.as_slice());
        assert_eq!(s.originals()["receipt"], receipt);
        assert_eq!(s.db.query_row("SELECT count(*) FROM devices", [], |r| r.get::<_, i64>(0))?, 0);
        assert_eq!(wire::hash(&s.originals()["_pending"])?, receipt[0][11]);
        assert_eq!(s.acl()?.2, receipt[0][10]);
        Ok(())
    })?;
    assert!(!fixture.cloud.locked().posts.contains_key("receipt"));
    server.abort();
    Ok(())
}

#[tokio::test]
async fn prepare_handler_real_handshake_command_a_device_null_and_capture_replay() -> Result<()> {
    use futures::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message as ClientMessage;
    let fixture = Fixture::new()?;
    let ticket = fixture.ticket("command").await?;
    let bound = fixture.bound(&ticket)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let router =
        axum::Router::new().route("/personal-prepare", axum::routing::get(upgrade)).with_state(fixture.hub.clone());
    let server = tokio::spawn(async move { axum::serve(listener, router).await });
    let ephemeral_key = StaticSecret::from([64; 32]);
    let hello = json!([
        wire::domain("prepare-hello"),
        1,
        bound.cfg.context(),
        bound.public,
        bound.hq,
        bound.wire_hash,
        "command",
        fixture.v["personalBootstrap"]["wireVectors"]["keys"]["app"],
        crypto::b64(&crypto::x25519_pub(&ephemeral_key)),
        crypto::b64(&[65; 32])
    ]);
    // Identidade A incorreta ou outro capture não chegam ao transporte Worker.
    for index in [3, 7] {
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{address}/personal-prepare")).await?;
        let mut wrong = hello.clone();
        wrong[index] = if index == 3 { json!("0".repeat(64)) } else { json!(crypto::b64(&[66; 32])) };
        ws.send(ClientMessage::Binary(pw::binary(&wrong, 16_384)?.into())).await?;
        assert!(matches!(ws.next().await, Some(Ok(ClientMessage::Close(_))) | None));
    }
    // A declarada correta sem assinatura finish não autentica command.
    let (mut bad, _) = tokio_tungstenite::connect_async(format!("ws://{address}/personal-prepare")).await?;
    bad.send(ClientMessage::Binary(pw::binary(&hello, 16_384)?.into())).await?;
    let welcome = pw::message(&bad.next().await.context("bad finish welcome")??.into_data(), 16_384)?;
    let th = pw::transcript(&hello, &Value::Array(welcome.as_array().context("welcome")?[..8].to_vec()))?;
    bad.send(ClientMessage::Binary(
        pw::binary(&json!([wire::domain("prepare-finish"), 1, crypto::b64(&th), crypto::b64(&[0; 64])]), 16_384)?
            .into(),
    ))
    .await?;
    assert!(matches!(bad.next().await, Some(Ok(ClientMessage::Close(_))) | None));
    assert!(!fixture.cloud.locked().posts.contains_key("candidate"));
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{address}/personal-prepare")).await?;
    ws.send(ClientMessage::Binary(pw::binary(&hello, 16_384)?.into())).await?;
    let raw = ws.next().await.context("welcome missing")??.into_data();
    let welcome = pw::message(&raw, 16_384)?;
    let w0 = Value::Array(welcome.as_array().context("welcome array")?[..8].to_vec());
    let th = pw::transcript(&hello, &w0)?;
    crypto::verify(&fixture.hub.identity.sign_pub(), &th, &pw::key::<64>(&welcome[8])?)?;
    let finish0 = json!([wire::domain("prepare-finish"), 1, crypto::b64(&th)]);
    let key = SigningKey::from_bytes(&crypto::unb64_n::<32>(fixture.cloud.locked().app.as_str())?);
    let mut finish = finish0.clone();
    finish
        .as_array_mut()
        .context("finish")?
        .push(json!(crypto::b64(&crypto::sign(&key, &serde_json::to_vec(&finish0)?))));
    ws.send(ClientMessage::Binary(pw::binary(&finish, 16_384)?.into())).await?;
    let request = Fixture::candidate(&bound, &crypto::b64(&[83; 32]))?;
    let shared = crypto::x25519(&ephemeral_key, &pw::key::<32>(&welcome[6])?)?;
    let mut akey = [0; 32];
    Hkdf::<Sha256>::new(Some(&th), &shared)
        .expand(wire::domain("prepare-key-a2h").as_bytes(), &mut akey)
        .map_err(|_| anyhow::anyhow!("a2h KDF"))?;
    let aad = |direction: &str| {
        serde_json::to_vec(&json!([
            wire::domain("prepare-frame"),
            1,
            bound.cfg.context(),
            bound.public,
            bound.hq,
            bound.wire_hash,
            crypto::b64(&th),
            direction,
            0
        ]))
    };
    let cipher = ChaCha20Poly1305::new_from_slice(&akey).map_err(|_| anyhow::anyhow!("cipher"))?;
    let ct = cipher
        .encrypt(&Nonce::from([0; 12]), Payload { msg: &serde_json::to_vec(&request)?, aad: &aad("a2h")? })
        .map_err(|_| anyhow::anyhow!("encrypt"))?;
    ws.send(ClientMessage::Binary(
        pw::binary(&json!([wire::domain("prepare-frame"), 1, "a2h", 0, crypto::b64(&ct)]), 32_768)?.into(),
    ))
    .await?;
    let frame = pw::message(&ws.next().await.context("response missing")??.into_data(), 32_768)?;
    let mut hkey = [0; 32];
    Hkdf::<Sha256>::new(Some(&th), &shared)
        .expand(wire::domain("prepare-key-h2a").as_bytes(), &mut hkey)
        .map_err(|_| anyhow::anyhow!("h2a KDF"))?;
    let plain = ChaCha20Poly1305::new_from_slice(&hkey)
        .map_err(|_| anyhow::anyhow!("cipher"))?
        .decrypt(
            &Nonce::from([0; 12]),
            Payload { msg: &crypto::unb64(frame[4].as_str().context("response cipher")?)?, aad: &aad("h2a")? },
        )
        .map_err(|_| anyhow::anyhow!("decrypt"))?;
    let response = protocol::canonical(&plain)?;
    assert_eq!(response[4], "ok");
    assert_eq!(fixture.cloud.locked().posts["candidate"], 1);
    assert!(!String::from_utf8_lossy(&raw).contains(&crypto::b64(&[83; 32])));
    let _ = ws.next().await;
    // Mesmo nonceA autenticado é retido: outra sessão não executa operação.
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{address}/personal-prepare")).await?;
    ws.send(ClientMessage::Binary(pw::binary(&hello, 16_384)?.into())).await?;
    let welcome = pw::message(&ws.next().await.context("replay welcome")??.into_data(), 16_384)?;
    let th = pw::transcript(&hello, &Value::Array(welcome.as_array().context("welcome")?[..8].to_vec()))?;
    let finish0 = json!([wire::domain("prepare-finish"), 1, crypto::b64(&th)]);
    let mut finish = finish0.clone();
    finish
        .as_array_mut()
        .context("finish")?
        .push(json!(crypto::b64(&crypto::sign(&key, &serde_json::to_vec(&finish0)?))));
    ws.send(ClientMessage::Binary(pw::binary(&finish, 16_384)?.into())).await?;
    assert!(matches!(ws.next().await, Some(Ok(ClientMessage::Close(_))) | None));
    assert_eq!(fixture.cloud.locked().posts["candidate"], 1);
    // Device não aceita A/finish de dono. Leitura dos fatos sem A não cria Device.
    let device = Fixture::new()?;
    let ticket = device.ticket("device").await?;
    let context = device.v["personalBootstrap"]["context"].clone();
    let mut hello = json!([
        wire::domain("prepare-hello"),
        1,
        context,
        pw::capture(&context, &ticket)?,
        wire::hash(&ticket[0])?,
        protocol::hash(&serde_json::to_vec(&ticket)?),
        "device",
        null,
        crypto::b64(&[8; 32]),
        crypto::b64(&[9; 32])
    ]);
    assert!(Bound::lookup(&device.hub, &hello).is_ok());
    hello[7] = json!(crypto::b64(&key.verifying_key().to_bytes()));
    assert!(Bound::lookup(&device.hub, &hello).is_err());
    server.abort();
    Ok(())
}

#[test]
fn personal_local_paired_after_install_never_becomes_legacy_or_reopens_possession() -> Result<()> {
    use crate::{
        api::devices::{self, Caller},
        store::{Device, DeviceSource, Scope},
    };
    let f = Fixture::new()?;
    let device = Device {
        key: crypto::b64(&[92; 32]),
        name: "synthetic".into(),
        platform: "fixture".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: 1,
        last_seen_at: None,
    };
    f.hub.store.put_device(&device)?;
    assert!(matches!(devices::authorize(&f.hub, &device.key), Err(devices::RejectCode::LeaseExpired)));
    let caller = Caller::Device { key: device.key.clone(), scopes: vec![Scope::Control], autoridade: None };
    assert!(super::super::guard_generic(&f.hub, &caller).is_err());
    assert!(super::super::guard_generic(&f.hub, &Caller::Local).is_ok());
    assert!(f.hub.store.personal_configured()?);
    f.hub.store.remove_device(&device.key)?;
    assert!(super::super::guard_generic(&f.hub, &caller).is_err());
    f.hub.personal.with(|p| {
        f.hub.store.personal_session(&p.config, "synthetic", |s| {
            assert!(s.claim().is_err());
            assert!(s.preparing().is_err());
            assert!(s.db.query_row("SELECT ever_claimed=1 FROM personal_generation", [], |r| r.get::<_, bool>(0))?);
            Ok(())
        })
    })?;
    Ok(())
}

#[tokio::test]
async fn enrollment_cipher_survives_expiry_until_fresh_terminal_read_and_cannot_purge_consumed() -> Result<()> {
    use super::super::{helper, shapes::Kind};
    let fixture = Fixture::new()?;
    let ticket = fixture.ticket("device").await?;
    let bound = fixture.bound(&ticket)?;
    let app = fixture.cloud.locked().app.clone();
    let facts = ceremony::step(&fixture.hub, &bound.id, |_, _, s| {
        Ok(db::enrollment(s.db, &bound.public)?.context("cipher")?.0)
    })?;
    tokio::time::sleep(Duration::from_millis(60)).await;
    let wire_vectors = &fixture.v["personalBootstrap"]["wireVectors"];
    let intent = signed(
        fixture.cloud.locked().issuer.as_str(),
        json!([
            wire::domain("intent"),
            bound.cfg.context(),
            ticket[0][2],
            "device",
            wire_vectors["packet"]["intent"][0][4],
            wire_vectors["packet"]["intent"][0][5],
            wire_vectors["keys"]["app"],
            crypto::b64(&[74; 32]),
            facts[5],
            facts[6],
            1
        ]),
    )?;
    // Attach estende só a história externa; não recebe Authority por ler enrollment.
    fixture.cloud.locked().originals["intent"] = intent.clone();
    let mut packet = json!({"intent":intent,"local-ticket":ticket});
    let ceremony = ceremony::checked(&bound.cfg, &packet, facts[5].as_u64().context("createdAt")?)?;
    let now = ceremony::step(&fixture.hub, &bound.id, |_, clock, _| clock.now())?
        .max(facts[5].as_u64().context("createdAt")?);
    let ca = signed(
        &app,
        json!([
            wire::domain("app-consent"),
            bound.cfg.context(),
            ceremony.digest(),
            "independent-identity-v1",
            "approve",
            now,
            ceremony.tuple()[18]
        ]),
    )?;
    packet["app-consent"] = ca.clone();
    fixture.cloud.locked().originals["app-consent"] = ca.clone();
    fixture.write_root(&packet, 1)?;
    ceremony::helper(
        &fixture.hub,
        &bound.id,
        &json!([helper::DOMAIN, 1, crypto::b64(&[70; 16]), "consent", intent, ticket, ca, 1]),
    )
    .await?;
    let (request, cfg) = fixture.hub.personal.with(|p| {
        let raw = p.clock.time_request(&p.config, &fixture.hub.identity)?;
        Ok((protocol::canonical(&raw)?, p.config.clone()))
    })?;
    let time = signed(
        fixture.cloud.locked().issuer.as_str(),
        json!([
            wire::domain("time"),
            cfg.context(),
            request[0][2],
            request[0][3],
            ticket[0][12].as_u64().context("expiry")? + 100,
            300_000
        ]),
    )?;
    fixture.hub.personal.accept_time(&serde_json::to_vec(&time)?)?;
    ceremony::step(&fixture.hub, &bound.id, |_, _, s| {
        assert!(db::enrollment(s.db, &bound.public)?.is_some());
        Ok(())
    })?;
    fixture.cloud.locked().terminal = true;
    let result = ceremony::read_state(&fixture.hub, &bound.id).await?.context("fresh terminal read")?;
    assert_eq!(result["state"][0][5], "expired");
    ceremony::step(&fixture.hub, &bound.id, |cfg, clock, s| {
        assert!(s.closed());
        assert!(db::enrollment(s.db, &bound.public).is_err());
        let history = ceremony::history(cfg, s.originals(), clock.now()?)?;
        assert!(history.original(Kind::Intent).is_ok());
        assert!(ceremony::checked(cfg, s.originals(), clock.now()?).is_err());
        Ok(())
    })?;
    assert!(fixture.ticket("device").await.is_err());
    assert_eq!(fixture.cloud.locked().posts["device"], 1);
    let retained = Fixture::new()?;
    let ticket = retained.ticket("device").await?;
    let bound = retained.bound(&ticket)?;
    ceremony::step(&retained.hub, &bound.id, |_, _, s| {
        s.reserve(&"a".repeat(64))?;
        assert!(db::purge_unconsumed(s.db, &bound.public, b"not an authenticated complete fence").is_err());
        assert!(db::enrollment(s.db, &bound.public)?.is_some());
        Ok(())
    })?;
    Ok(())
}

#[tokio::test]
async fn r24_begin_local_claim_during_worker_await_refuses_late_cipher_commit() -> Result<()> {
    use crate::store::{Device, DeviceSource, Scope};
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::Notify;
    let f = Fixture::new()?;
    let upstream = f.hub.personal.test_transport.locked().clone().context("fixture transport missing")?;
    let posted = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let first_response = Arc::new(AtomicBool::new(true));
    let seen = posted.clone();
    let resume = release.clone();
    *f.hub.personal.test_transport.locked() = Some(Arc::new(move |route, bytes, header| {
        let upstream = upstream.clone();
        let seen = seen.clone();
        let resume = resume.clone();
        let first_response = first_response.clone();
        Box::pin(async move {
            let response = upstream(route.clone(), bytes, header).await?;
            if route == "device" && first_response.swap(false, Ordering::AcqRel) {
                // Worker externo já recebeu o POST e produziu resposta válida;
                // nenhuma trava Store/Cloud permanece durante esta espera.
                seen.notify_one();
                resume.notified().await;
            }
            Ok(response)
        })
    }));
    let hub = f.hub.clone();
    let mut pending = tokio::spawn(async move {
        ceremony::helper(
            &hub,
            "root-session",
            &json!([super::super::helper::DOMAIN, 1, crypto::b64(&[117; 16]), "ticket", "device", null]),
        )
        .await
    });
    tokio::select! {
        () = posted.notified() => (),
        result = &mut pending => anyhow::bail!("fixture did not reach POST/device response barrier: {result:?}"),
    }
    let (q, begin, sent) = ceremony::step(&f.hub, "root-session", |_, _, s| {
        assert!(s.preparing().is_ok());
        assert_eq!(s.db.query_row("SELECT count(*) FROM personal_enrollment_secrets", [], |r| r.get::<_, i64>(0))?, 0);
        Ok((
            s.originals()["local-ticket"].clone(),
            s.originals()["_begin"].clone(),
            s.originals()["_sentBegin"].clone(),
        ))
    })?;
    let key = crypto::b64(&[118; 32]);
    f.hub.store.put_device(&Device {
        key: key.clone(),
        name: "discardable-local-fixture".into(),
        platform: "fixture".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: 1,
        last_seen_at: None,
    })?;
    let (generation, revision) = ceremony::step(&f.hub, "root-session", |cfg, _, s| {
        assert!(s.preparing().is_err());
        assert!(s.db.query_row("SELECT ever_claimed=1 AND tombstone=0 FROM personal_generation", [], |r| {
            r.get::<_, bool>(0)
        })?);
        assert!(
            s.db.query_row("SELECT origin='personal' AND ever_claimed=1 FROM first_owner", [], |r| r
                .get::<_, bool>(0))?
        );
        Ok((
            cfg.identity().generation.clone(),
            s.db.query_row("SELECT revision FROM personal_generation", [], |r| r.get::<_, i64>(0))?,
        ))
    })?;
    assert_eq!(f.hub.store.personal_origin(&key)?, Some(generation.clone()));
    release.notify_one();
    let outcome = pending.await?;
    // Repetir o pedido original não pode produzir outro /device ou novo segredo.
    let retry = f.ticket("device").await;
    assert_eq!(f.cloud.locked().posts["device"], 1);
    let retained = ceremony::step(&f.hub, "root-session", |cfg, _, s| {
        assert_eq!(s.originals()["local-ticket"], q);
        assert_eq!(s.originals()["_begin"], begin);
        assert_eq!(s.originals()["_sentBegin"], sent);
        assert!(s.originals().get("grant").is_none());
        assert!(s.originals().get("receipt").is_none());
        assert!(s.db.query_row("SELECT ever_claimed=1 AND tombstone=0 FROM personal_generation", [], |r| {
            r.get::<_, bool>(0)
        })?);
        assert!(
            s.db.query_row("SELECT origin='personal' AND ever_claimed=1 FROM first_owner", [], |r| r
                .get::<_, bool>(0))?
        );
        assert_eq!(s.db.query_row("SELECT revision FROM personal_generation", [], |r| r.get::<_, i64>(0))?, revision);
        assert_eq!(cfg.identity().generation, generation);
        assert_eq!(
            s.db.query_row(
                "SELECT count(*) FROM personal_device_origins WHERE key=? AND generation=?",
                rusqlite::params![key, generation],
                |r| r.get::<_, i64>(0)
            )?,
            1
        );
        s.db.query_row("SELECT count(*) FROM personal_enrollment_secrets", [], |r| r.get::<_, i64>(0))
            .map_err(Into::into)
    })?;
    // Falha vermelha esperada: a implementação vulnerável comita uma cifra/fatos
    // mesmo após o trigger real fechar a geração, sem erro de captura/shape/time.
    assert_eq!(retained, 0, "begin cifrado/fatos foram comitados depois da claim Local durante o await");
    assert!(outcome.is_err());
    assert!(retry.is_err());
    Ok(())
}

/// Cliente de teste somente para o handler device: handshake/AEAD de verdade,
/// sem mock de Session/Store ou declaração de dono antes de I.
struct DevicePreparePeer {
    socket: tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    server: tokio::task::JoinHandle<()>,
    hello: Value,
    th: [u8; 32],
    a2h: [u8; 32],
    h2a: [u8; 32],
    sent: bool,
}
impl Drop for DevicePreparePeer {
    fn drop(&mut self) {
        self.server.abort();
    }
}
impl DevicePreparePeer {
    async fn open(f: &Fixture, q: &Value) -> Result<Self> {
        use futures::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message as ClientMessage;
        let bound = f.bound(q)?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let router =
            axum::Router::new().route("/personal-prepare", axum::routing::get(upgrade)).with_state(f.hub.clone());
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/personal-prepare")).await?;
        let e = StaticSecret::from(crypto::random::<32>());
        let hello = json!([
            wire::domain("prepare-hello"),
            1,
            bound.cfg.context(),
            bound.public,
            bound.hq,
            bound.wire_hash,
            "device",
            null,
            crypto::b64(&crypto::x25519_pub(&e)),
            transport::nonce()
        ]);
        socket.send(ClientMessage::Binary(pw::binary(&hello, 16_384)?.into())).await?;
        let raw = socket.next().await.context("device welcome missing")??.into_data();
        let welcome = pw::message(&raw, 16_384)?;
        ensure!(welcome.as_array().is_some_and(|p| p.len() == 9), "device welcome shape");
        assert_eq!(welcome[0], wire::domain("prepare-welcome"));
        assert_eq!(welcome[1], 1);
        for i in 2..6 {
            assert_eq!(welcome[i], hello[i]);
        }
        pw::key::<32>(&welcome[7])?;
        let th = pw::transcript(&hello, &Value::Array(welcome.as_array().context("welcome")?[..8].to_vec()))?;
        crypto::verify(&f.hub.identity.sign_pub(), &th, &pw::key::<64>(&welcome[8])?)?;
        let shared = crypto::x25519(&e, &pw::key::<32>(&welcome[6])?)?;
        let hkdf = Hkdf::<Sha256>::new(Some(&th), &shared);
        let mut a2h = [0; 32];
        let mut h2a = [0; 32];
        hkdf.expand(wire::domain("prepare-key-a2h").as_bytes(), &mut a2h)
            .map_err(|_| anyhow::anyhow!("device a2h KDF"))?;
        hkdf.expand(wire::domain("prepare-key-h2a").as_bytes(), &mut h2a)
            .map_err(|_| anyhow::anyhow!("device h2a KDF"))?;
        Ok(Self { socket, server, hello, th, a2h, h2a, sent: false })
    }
    async fn finish(&mut self, signature: Value) -> Result<()> {
        use futures::SinkExt;
        self.socket
            .send(tokio_tungstenite::tungstenite::Message::Binary(
                pw::binary(&json!([wire::domain("prepare-finish"), 1, crypto::b64(&self.th), signature]), 16_384)?
                    .into(),
            ))
            .await?;
        Ok(())
    }
    fn aad(&self, direction: &str) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&json!([
            wire::domain("prepare-frame"),
            1,
            self.hello[2],
            self.hello[3],
            self.hello[4],
            self.hello[5],
            crypto::b64(&self.th),
            direction,
            0
        ]))?)
    }
    async fn request(&mut self, request: &Value) -> Result<Vec<u8>> {
        use futures::SinkExt;
        ensure!(!self.sent, "one device operation per connection");
        let plain = serde_json::to_vec(request)?;
        protocol::canonical(&plain)?;
        let cipher = ChaCha20Poly1305::new_from_slice(&self.a2h).map_err(|_| anyhow::anyhow!("device cipher"))?;
        let ct = cipher
            .encrypt(&Nonce::from([0; 12]), Payload { msg: &plain, aad: &self.aad("a2h")? })
            .map_err(|_| anyhow::anyhow!("device encrypt"))?;
        let frame = pw::binary(&json!([wire::domain("prepare-frame"), 1, "a2h", 0, crypto::b64(&ct)]), 32_768)?;
        self.socket.send(tokio_tungstenite::tungstenite::Message::Binary(frame.clone().into())).await?;
        self.sent = true;
        Ok(frame)
    }
    async fn response(&mut self) -> Result<Value> {
        use futures::StreamExt;
        let raw = self.socket.next().await.context("device response missing")??.into_data();
        let f = pw::message(&raw, 32_768)?;
        ensure!(
            f.as_array().is_some_and(|v| v.len() == 5)
                && f[0] == wire::domain("prepare-frame")
                && f[1] == 1
                && f[2] == "h2a"
                && f[3] == 0,
            "device response direction/sequence"
        );
        let ct = crypto::unb64(f[4].as_str().context("device response cipher")?)?;
        let plain = ChaCha20Poly1305::new_from_slice(&self.h2a)
            .map_err(|_| anyhow::anyhow!("device cipher"))?
            .decrypt(&Nonce::from([0; 12]), Payload { msg: &ct, aad: &self.aad("h2a")? })
            .map_err(|_| anyhow::anyhow!("device decrypt"))?;
        protocol::canonical(&plain)
    }
    async fn closed(&mut self) -> Result<()> {
        use futures::StreamExt;
        let message = self.socket.next().await.context("expected explicit device refusal")??;
        let tokio_tungstenite::tungstenite::Message::Close(Some(frame)) = message else {
            anyhow::bail!("device refusal must close before returning facts");
        };
        assert_eq!(u16::from(frame.code), 4003);
        Ok(())
    }
}
fn assert_device_read_did_not_associate(f: &Fixture) -> Result<()> {
    ceremony::step(&f.hub, "root-session", |_, _, s| {
        assert!(s.originals().get("intent").is_none());
        for name in ["app-consent", "local-consent", "consume", "grant", "receipt"] {
            assert!(s.originals().get(name).is_none());
        }
        assert_eq!(s.db.query_row("SELECT count(*) FROM devices", [], |r| r.get::<_, i64>(0))?, 0);
        assert_eq!(s.db.query_row("SELECT count(*) FROM personal_possession_slot", [], |r| r.get::<_, i64>(0))?, 0);
        assert!(s.db.query_row("SELECT ever_claimed=0 AND tombstone=0 FROM personal_generation", [], |r| {
            r.get::<_, bool>(0)
        })?);
        assert!(
            s.db.query_row("SELECT origin='personal' AND ever_claimed=0 FROM first_owner", [], |r| r
                .get::<_, bool>(0))?
        );
        Ok(())
    })?;
    assert_eq!(f.cloud.locked().posts["device"], 1);
    assert!(!f.cloud.locked().posts.contains_key("candidate"));
    Ok(())
}
#[tokio::test]
async fn r24_device_handler_null_finish_encrypted_enrollment_returns_only_original_public_facts() -> Result<()> {
    let f = Fixture::new()?;
    let q = f.ticket("device").await?;
    let (binding, facts, cipher) = ceremony::step(&f.hub, "root-session", |cfg, _, s| {
        let b = enrollment::Binding::new(cfg, &q, &s.originals()["_begin"])?;
        let (facts, cipher) = db::enrollment(s.db, &b.capture)?.context("original begin cipher missing")?;
        Ok((b, facts, cipher))
    })?;
    let original = protocol::canonical(&binding.open(&f.hub.identity, &cipher, &facts)?)?;
    let mut peer = DevicePreparePeer::open(&f, &q).await?;
    peer.finish(Value::Null).await?;
    let request =
        json!([wire::domain("prepare-request"), 1, crypto::b64(&[120; 16]), wire::hash(&q[0])?, "enrollment"]);
    let encrypted = peer.request(&request).await?;
    assert!(!String::from_utf8_lossy(&encrypted).contains(original["userCode"].as_str().context("userCode")?));
    let response = peer.response().await?;
    let public = json!([
        wire::hash(&q[0])?,
        protocol::hash(&serde_json::to_vec(&q)?),
        original["intentId"],
        original["candidateId"],
        original["userCode"],
        original["createdAt"],
        original["expiresAt"],
        5000
    ]);
    assert_eq!(response, json!([wire::domain("prepare-response"), 1, request[2], null, "ok", "enrollment", public]));
    assert_ne!(original["createdAt"], q[0][11]);
    assert_ne!(original["expiresAt"], q[0][12]);
    assert!(!serde_json::to_string(&response)?.contains(original["deviceCode"].as_str().context("deviceCode")?));
    ceremony::step(&f.hub, "root-session", |_, _, s| {
        assert_eq!(db::enrollment(s.db, &binding.capture)?, Some((facts.clone(), cipher.clone())));
        Ok(())
    })?;
    assert_device_read_did_not_associate(&f)
}
#[tokio::test]
async fn r24_device_handler_nonnull_finish_refuses_before_enrollment() -> Result<()> {
    let f = Fixture::new()?;
    let q = f.ticket("device").await?;
    let mut peer = DevicePreparePeer::open(&f, &q).await?;
    let a = SigningKey::from_bytes(&crypto::unb64_n::<32>(&f.cloud.locked().app)?);
    let signature = crypto::b64(&crypto::sign(
        &a,
        &serde_json::to_vec(&json!([wire::domain("prepare-finish"), 1, crypto::b64(&peer.th)]))?,
    ));
    peer.finish(json!(signature)).await?;
    peer.closed().await?;
    assert_device_read_did_not_associate(&f)
}
#[tokio::test]
async fn r24_device_handler_crossed_candidate_operation_refuses_after_null_finish() -> Result<()> {
    let f = Fixture::new()?;
    let q = f.ticket("device").await?;
    let b = f.bound(&q)?;
    let mut peer = DevicePreparePeer::open(&f, &q).await?;
    peer.finish(Value::Null).await?;
    peer.request(&Fixture::candidate(&b, &crypto::b64(&[121; 32]))?).await?;
    peer.closed().await?;
    assert_device_read_did_not_associate(&f)
}

/// HOST-R1-01: o testemunho só vale assinado pela raiz E, com C, Hub, nonce exatos e consumedAt seguro > 0.
#[test]
fn host_r1_01_consumption_witness_refuses_other_nonce_hub_signer_or_value() -> Result<()> {
    let (cfg, identity) = super::super::provisioning::fixture()?;
    let w = vectors()?["personalBootstrap"]["wireVectors"].clone();
    let issuer = w["seeds"]["issuer"].as_str().context("issuer seed")?;
    let nonce = crypto::b64(&[81; 32]);
    let base = json!([
        wire::domain("consumption-witness"),
        cfg.context(),
        "a".repeat(64),
        "b".repeat(64),
        identity.computer_id(),
        nonce,
        1_800_000_000_000_u64
    ]);
    let raw = |p: &Value, seed: &str| -> Result<Vec<u8>> { Ok(serde_json::to_vec(&signed(seed, p.clone())?)?) };
    assert_eq!(cfg.verify_witness(&raw(&base, issuer)?, &nonce)?.payload()[6], json!(1_800_000_000_000_u64));
    assert!(cfg.verify_witness(&raw(&base, issuer)?, &crypto::b64(&[82; 32])).is_err(), "nonce");
    for (index, value) in [(4, json!(crypto::computer_id(&[9; 32]))), (6, json!(0)), (1, json!(["outro"]))] {
        let mut p = base.clone();
        p[index] = value;
        assert!(cfg.verify_witness(&raw(&p, issuer)?, &nonce).is_err(), "campo {index}");
    }
    assert!(cfg.verify_witness(&raw(&base, &crypto::b64(&[7; 32]))?, &nonce).is_err(), "outro assinador");
    let mut longer = base.as_array().context("base")?.clone();
    longer.push(json!(1));
    assert!(cfg.verify_witness(&raw(&Value::Array(longer), issuer)?, &nonce).is_err(), "8 campos");
    Ok(())
}
