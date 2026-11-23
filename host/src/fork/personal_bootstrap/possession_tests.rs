//! SQLite/Ed25519 reais. Não testa PR4, peers privilegiados ou execução do app.
use super::{ceremony, channel, clock::Clock, provisioning, wire};
use crate::{hub::Hub, remote::crypto, store::Store};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::sync::Arc;
fn setup() -> Result<(Arc<Hub>, Value, std::path::PathBuf)> {
    let all: Value =
        serde_json::from_str(include_str!("../../../../docs/reference/fixtures/remote-relay-vectors.json"))?;
    let v = all["personalBootstrap"]["wireVectors"].clone();
    let dir = std::env::temp_dir().join(format!("personal-possession-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir)?;
    let store = Store::open(&dir.join("codync.db"))?;
    let (cfg, identity) = provisioning::fixture()?;
    store.personal_install_generation(&cfg)?;
    let hub = Hub::new(store, "fixture".into(), identity, "fixture".into(), 0);
    hub.personal.install(cfg, None, Clock::fixture(v["now"].as_u64().context("fixture time")?)?)?;
    hub.personal.with(|f| {
        hub.store.personal_session(&f.config, "captured-fixture", |s| {
            for name in [
                "intent",
                "local-ticket",
                "app-consent",
                "local-consent",
                "box-challenge",
                "box-proof",
                "approve",
                "local-confirm",
                "consume",
                "grant",
            ] {
                s.freeze(name, v["packet"][name].clone())?;
            }
            Ok(())
        })
    })?;
    Ok((hub, v, dir))
}
fn kf(hub: &Hub, v: &Value) -> Result<(Value, Value)> {
    let k = hub.personal.with(|f| {
        hub.store.personal_session(&f.config, "captured-fixture", |s| {
            // Costura só de teste: testemunho já conferido com consumedAt anterior a G (predecessor = G[9]).
            channel::challenge(hub, &f.config, &mut f.clock, s, Some(&ceremony::Witness::fixture(1)?))
        })
    })?;
    let proof = hub.personal.with(|f| {
        hub.store.personal_session(&f.config, "captured-fixture", |s| {
            let c = ceremony::checked(&f.config, s.originals(), f.clock.now()?)?;
            let now = f.clock.now()?;
            let payload = json!([
                wire::domain("proof"),
                f.config.context(),
                c.original(super::shapes::Kind::Grant)?.digest(),
                wire::hash(&k[0])?,
                c.digest(),
                c.original(super::shapes::Kind::Approve)?.digest(),
                c.original(super::shapes::Kind::LocalConfirm)?.digest(),
                now,
                k[0][6]
            ]);
            let key = ed25519_dalek::SigningKey::from_bytes(&crypto::unb64_n::<32>(
                v["seeds"]["app"].as_str().context("fixture A")?,
            )?);
            Ok(json!([payload, crypto::b64(&crypto::sign(&key, &serde_json::to_vec(&payload)?))]))
        })
    })?;
    Ok((k, proof))
}
fn fence(hub: &Hub, v: &Value) -> Result<()> {
    hub.personal.with(|f| {
        hub.store.personal_session(&f.config, "captured-fixture", |s| {
            let mut p = ceremony::packet(s.originals());
            let c = ceremony::checked(&f.config, &p, f.clock.now()?)?;
            let payload = json!([
                wire::domain("fence"),
                f.config.context(),
                c.digest(),
                c.original(super::shapes::Kind::Grant)?.digest(),
                null,
                null,
                1,
                "ownerRevoked",
                f.clock.now()?
            ]);
            let e = ed25519_dalek::SigningKey::from_bytes(&crypto::unb64_n::<32>(
                v["seeds"]["issuer"].as_str().context("fixture E")?,
            )?);
            p["fence"] = json!([payload, crypto::b64(&crypto::sign(&e, &serde_json::to_vec(&payload)?))]);
            let c = ceremony::checked(&f.config, &p, f.clock.now()?)?;
            channel::pre_fence(hub, &f.config, &mut f.clock, s, &c)
        })
    })
}
#[test]
fn personal_possession_real_commit_same_r_acl_pending_and_no_active_device() -> Result<()> {
    let (hub, v, dir) = setup()?;
    let (k, f) = kf(&hub, &v)?;
    let run = || {
        hub.personal.with(|p| {
            hub.store.personal_session(&p.config, "captured-fixture", |s| {
                channel::proof(&hub, &p.config, &mut p.clock, s, &k, &f)
            })
        })
    };
    let first = run()?;
    assert_eq!(first, run()?);
    hub.personal.with(|p| {
        hub.store.personal_session(&p.config, "captured-fixture", |s| {
            assert_eq!(s.originals()["receipt"], first);
            assert_eq!(wire::hash(&s.originals()["_pending"])?, first[0][11]);
            assert_eq!(s.db.query_row("SELECT count(*) FROM devices", [], |r| r.get::<_, i64>(0))?, 0);
            assert!(s.db.query_row("SELECT ever_claimed=1 FROM personal_generation", [], |r| r.get::<_, bool>(0))?);
            assert!(s.db.query_row("SELECT ever_claimed=1 FROM first_owner", [], |r| r.get::<_, bool>(0))?);
            assert_eq!(s.acl()?.2, first[0][10]);
            Ok(())
        })
    })?;
    assert!(
        hub.store.bootstrap_claim_a1(None, "other input", |_| panic!("PL1 must exclude A1 before signing")).is_err()
    );
    fence(&hub, &v)?;
    hub.personal.with(|p| {
        hub.store.personal_session(&p.config, "captured-fixture", |s| {
            assert!(s.originals()["fence-ack"][0][4].is_null());
            assert!(s.originals()["fence-ack"][0][5].is_null());
            assert_eq!(s.originals()["receipt"], first);
            Ok(())
        })
    })?;
    drop(hub);
    let reopened = Store::open(&dir.join("codync.db"))?;
    assert!(reopened.personal_configured()?);
    assert!(reopened.bootstrap_claim_a1(None, "retry", |_| panic!("closed PL1 must not sign A1")).is_err());
    drop(reopened);
    std::fs::remove_dir_all(dir)?;
    Ok(())
}
#[test]
fn personal_fence_commits_before_proof_and_no_receipt_or_new_possession() -> Result<()> {
    let (hub, v, dir) = setup()?;
    let (k, f) = kf(&hub, &v)?;
    // Barreira explícita de ordem: W termina commit antes de liberar o perdedor F.
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let other = barrier.clone();
    let worker = hub.clone();
    let waiter = std::thread::spawn(move || {
        other.wait();
        worker.personal.with(|p| {
            worker.store.personal_session(&p.config, "captured-fixture", |s| {
                channel::proof(&worker, &p.config, &mut p.clock, s, &k, &f)
            })
        })
    });
    fence(&hub, &v)?;
    barrier.wait();
    assert!(waiter.join().map_err(|_| anyhow::anyhow!("fixture worker panicked"))?.is_err());
    hub.personal.with(|p| {
        hub.store.personal_session(&p.config, "captured-fixture", |s| {
            assert!(s.originals().get("receipt").is_none());
            assert!(s.originals().get("proof").is_none());
            assert!(s.db.query_row("SELECT tombstone=1 AND ever_claimed=0 FROM personal_generation", [], |r| {
                r.get::<_, bool>(0)
            })?);
            Ok(())
        })
    })?;
    drop(hub);
    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn personal_receipt_refuses_future_proof_before_claim_or_durable_receipt() -> Result<()> {
    use super::shapes::Kind;
    use crate::LockExt;

    let (hub, vectors, dir) = setup()?;
    let (challenge, mut proof) = kf(&hub, &vectors)?;
    // Só A sintética muda F[7]. G/K, hashes, chaves e expiries permanecem originais.
    let app = ed25519_dalek::SigningKey::from_bytes(&crypto::unb64_n::<32>(
        vectors["seeds"]["app"].as_str().context("fixture A")?,
    )?);
    hub.personal.with(|runtime| {
        hub.store.personal_session(&runtime.config, "captured-fixture", |session| {
            let issued = runtime.clock.now()?.checked_add(2000).context("fixture issuedAt overflow")?;
            assert!(issued < challenge[0][6].as_u64().context("K expiry")?);
            proof[0][7] = json!(issued);
            proof[1] = json!(crypto::b64(&crypto::sign(&app, &serde_json::to_vec(&proof[0])?)));
            let mut packet = session.originals().clone();
            packet["proof"] = proof.clone();
            let now = runtime.clock.now()?;
            assert!(now < issued, "fixture must present a genuinely future F");
            let verified = ceremony::checked(&runtime.config, &packet, now)?;
            assert_eq!(verified.original(Kind::Proof)?.payload()[7], proof[0][7]);
            assert_eq!(verified.original(Kind::Challenge)?.original_bytes(), serde_json::to_vec(&challenge)?);
            Ok(())
        })
    })?;
    // Leitura direta da MESMA Store: não avança a revisão da sessão para medir rollback.
    let snapshot = || -> Result<Value> {
        let db = hub.store.db.locked();
        let raw: String = db.query_row(
            "SELECT json_object(
                'session',json((SELECT json_object('originals',json(originals),'revision',revision,'config',hex(config)) FROM personal_sessions WHERE id='captured-fixture')),
                'generation',json((SELECT json_object('generation',generation,'config',hex(config),'everClaimed',ever_claimed,'tombstone',tombstone,'revision',revision) FROM personal_generation WHERE singleton=1)),
                'owner',json((SELECT json_object('origin',origin,'everClaimed',ever_claimed) FROM first_owner WHERE singleton=1)),
                'slot',json((SELECT json_object('capture',capture,'grantHash',grant_hash) FROM personal_possession_slot WHERE singleton=1)),
                'acl',(SELECT version FROM personal_acl WHERE singleton=1),
                'devices',(SELECT count(*) FROM devices),
                'origins',(SELECT count(*) FROM personal_device_origins),
                'grants',(SELECT count(*) FROM personal_device_origins WHERE grant_id IS NOT NULL))",
            [],
            |row| row.get(0),
        )?;
        Ok(serde_json::from_str(&raw)?)
    };
    let before = snapshot()?;
    assert_eq!(before["generation"]["everClaimed"], 0);
    assert_eq!(before["owner"]["everClaimed"], 0);
    assert_eq!(before["slot"]["capture"], "captured-fixture");
    assert_eq!(before["slot"]["grantHash"], wire::hash(&vectors["packet"]["grant"][0])?);
    let result = hub.personal.with(|runtime| {
        hub.store.personal_session(&runtime.config, "captured-fixture", |session| {
            assert!(runtime.clock.now()? < proof[0][7].as_u64().context("F issuedAt")?);
            channel::proof(&hub, &runtime.config, &mut runtime.clock, session, &challenge, &proof)
        })
    });
    assert!(
        result.is_err(),
        "future F must be refused before claim/freeze: R[7]={:?}, F[7]={}",
        result.as_ref().ok().map(|receipt| &receipt[0][7]),
        proof[0][7]
    );
    let after = snapshot()?;
    let originals = &after["session"]["originals"];
    assert!(originals.get("receipt").is_none());
    assert!(originals.get("_pending").is_none());
    assert!(originals.get("proof").is_none());
    assert_eq!(after["devices"], 0);
    assert_eq!(after["origins"], 0);
    assert_eq!(after["grants"], 0);
    assert_eq!(after, before, "rollback must retain G/K, reservation, ACL, first owner and all prior history");
    drop(hub);
    std::fs::remove_dir_all(dir)?;
    Ok(())
}

// C3b.2 Host: Worker/root são fronteiras externas controladas; Store/SQLite e toda crypto são reais.
struct PersonalLiveFixture {
    hub: Arc<Hub>,
    vectors: Value,
    directory: std::path::PathBuf,
}
impl Drop for PersonalLiveFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
#[cfg(unix)]
fn personal_root_file(fixture: &PersonalLiveFixture, name: &str, bytes: &[u8]) -> Result<std::path::PathBuf> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
    std::fs::set_permissions(&fixture.directory, std::fs::Permissions::from_mode(0o700))?;
    // Root exige 0750/0640 exatos; o diretório externo 0700 confina a fixture inteira.
    let root = fixture.directory.join(name);
    std::fs::DirBuilder::new().mode(0o750).create(&root)?;
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o750))?;
    let path = root.join("journal");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o640)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o640))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::File::open(&root)?.sync_all()?;
    std::fs::File::open(&fixture.directory)?.sync_all()?;
    Ok(path)
}
#[cfg(not(unix))]
fn personal_root_file(_fixture: &PersonalLiveFixture, _name: &str, _bytes: &[u8]) -> Result<std::path::PathBuf> {
    anyhow::bail!("personal root fixture requires Unix")
}
fn personal_signed(v: &Value, role: &str, payload: &Value) -> Result<Value> {
    let seed = crypto::unb64_n::<32>(v["seeds"][role].as_str().context("fixture seed")?)?;
    Ok(json!([
        payload,
        crypto::b64(&crypto::sign(&ed25519_dalek::SigningKey::from_bytes(&seed), &serde_json::to_vec(payload)?))
    ]))
}
async fn personal_live_fixture() -> Result<PersonalLiveFixture> {
    use super::{
        clock::{SnapshotBinding, SnapshotFlight},
        live, protocol,
        shapes::Kind,
        sync, transport,
    };
    let (hub, vectors, directory) = setup()?;
    let (challenge, proof) = kf(&hub, &vectors)?;
    let receipt = hub.personal.with(|foundation| {
        hub.store.personal_session(&foundation.config, "captured-fixture", |s| {
            channel::proof(&hub, &foundation.config, &mut foundation.clock, s, &challenge, &proof)
        })
    })?;
    let (cfg, public, now) = hub.personal.with(|foundation| {
        hub.store.personal_session(&foundation.config, "captured-fixture", |s| {
            Ok((foundation.config.clone(), ceremony::packet(s.originals()), foundation.clock.now()?))
        })
    })?;
    let checked_ceremony = ceremony::history(&cfg, &public, now)?;
    let publish = personal_signed(
        &vectors,
        "app",
        &json!([
            wire::domain("publish"),
            cfg.context(),
            checked_ceremony.tuple()[1],
            checked_ceremony.digest(),
            checked_ceremony.original(Kind::Grant)?.digest(),
            receipt[0][6],
            wire::hash(&receipt[0])?,
            2,
            format!("op_{}", transport::nonce()),
            now,
            now + 30_000
        ]),
    )?;
    hub.store.personal_session(&cfg, "captured-fixture", |s| s.freeze("publish", publish))?;
    let prefix_names = [
        "intent",
        "local-ticket",
        "app-consent",
        "local-consent",
        "box-challenge",
        "box-proof",
        "approve",
        "local-confirm",
    ];
    let mut prefix = json!({});
    for name in prefix_names {
        prefix[name] = public[name].clone();
    }
    let journal = provisioning::fixture_journal(&cfg, &hub.identity, prefix)?;
    let root_revision = journal.revision();
    let root_hash = protocol::hash(journal.original_bytes());
    hub.personal.with(|foundation| {
        foundation.journal = Some(journal);
        Ok(())
    })?;
    let (flight, request) = hub.personal.with(|foundation| {
        SnapshotFlight::request(
            &cfg,
            &hub.identity,
            &mut foundation.clock,
            SnapshotBinding {
                h_t: checked_ceremony.digest().into(),
                h_g: checked_ceremony.original(Kind::Grant)?.digest().into(),
                grant: receipt[0][6].as_str().context("grant")?.into(),
                h_r: wire::hash(&receipt[0])?,
            },
        )
    })?;
    let request = protocol::canonical(&request)?;
    let at = request[0][7].as_u64().context("request time")?;
    let snapshot = personal_signed(
        &vectors,
        "issuer",
        &json!([
            wire::domain("host-state"),
            cfg.context(),
            checked_ceremony.digest(),
            checked_ceremony.original(Kind::Grant)?.digest(),
            receipt[0][6],
            wire::hash(&receipt[0])?,
            wire::hash(
                &hub.store.personal_session(&cfg, "captured-fixture", |s| Ok(s.originals()["publish"][0].clone()))?
            )?,
            checked_ceremony.tuple()[5],
            checked_ceremony.tuple()[6],
            checked_ceremony.tuple()[7],
            checked_ceremony.tuple()[11],
            checked_ceremony.tuple()[10],
            ["control", "owner.devices"],
            checked_ceremony.tuple()[16],
            0,
            request[0][6],
            at,
            at + 300_000
        ]),
    )?;
    let mut lease = flight.accept(&cfg, &serde_json::to_vec(&snapshot)?)?;
    let now = hub.personal.with(|foundation| foundation.clock.validity())?;
    let draft = live::plan(&hub, &cfg, "captured-fixture", root_revision, root_hash.clone(), &mut lease, now)?;
    ceremony::wait_causal(&hub, at, lease.until()?).await?;
    let at = hub.personal.with(|foundation| foundation.clock.issued_after(at, lease.until()?))?;
    let binding = &draft.record.binding;
    let activation = transport::signed(
        &hub,
        json!([
            wire::domain("activation"),
            cfg.context(),
            binding.h_t,
            binding.grant,
            binding.h_r,
            binding.owner,
            binding.device,
            binding.app,
            binding.computer,
            binding.generation,
            draft.next_revision,
            draft.next_acl,
            draft.next_hash,
            at,
            lease.until()?,
            ["control", "owner.devices"]
        ]),
    )?;
    let certified = live::certify(
        &cfg,
        draft,
        &serde_json::to_vec(&activation)?,
        hub.personal.with(|foundation| foundation.clock.validity())?,
    )?;
    sync::promote(&hub, &cfg, &certified, lease, root_revision, &root_hash)?;
    Ok(PersonalLiveFixture { hub, vectors, directory })
}
#[tokio::test]
async fn personal_host_b2_promotion_real_sqlite_snapshot_is_not_connected_and_cas_rollback() -> Result<()> {
    use super::{clock::Lease, live, sync};
    use crate::LockExt;
    let fixture = personal_live_fixture().await?;
    let hub = &fixture.hub;
    assert!(!hub.cloud_synced.load(std::sync::atomic::Ordering::Acquire));
    assert!(!hub.cloud.status().connected);
    assert!(sync::socket(hub).is_err()); // Sem activationCurrent não cria ingresso.
    let (record, accepted) = {
        let db = hub.store.db.locked();
        live::current(&db)?.context("record missing")?
    };
    assert!(!accepted);
    assert_eq!(hub.cloud.status().owner.context("owner")?.user_id, record.binding.owner);
    hub.personal.with(|p| {
        let db = hub.store.db.locked();
        let acl = live::acl(&db, None)?;
        let v: Value = serde_json::from_slice(&record.activation)?;
        assert_eq!((json!(acl.0), json!(acl.1), json!(acl.2)), (v[0][10].clone(), v[0][11].clone(), v[0][12].clone()));
        let mut lease = Lease::restore(&p.config, &record.lease)?;
        live::guard(&db, &p.config, &record, &mut lease, &mut p.clock, false)?;
        assert_eq!(db.query_row("SELECT count(*) FROM devices WHERE source='account'", [], |r| r.get::<_, i64>(0))?, 1);
        assert!(db.execute("DELETE FROM personal_live", []).is_err());
        Ok(())
    })?;
    // O mesmo original não é republicado como uma segunda geração/snapshot por CAS.
    let (cfg, now) = hub.personal.with(|p| Ok((p.config.clone(), p.clock.validity()?)))?;
    let mut lease = Lease::restore(&cfg, &record.lease)?;
    let draft = live::plan(
        hub,
        &cfg,
        &record.binding.capture,
        record.root_revision,
        record.root_hash.clone(),
        &mut lease,
        now,
    )?;
    let b = &draft.record.binding;
    let at = hub.personal.with(|p| p.clock.issued_after(lease.observed()?, lease.until()?))?;
    let activation = super::transport::signed(
        hub,
        json!([
            wire::domain("activation"),
            cfg.context(),
            b.h_t,
            b.grant,
            b.h_r,
            b.owner,
            b.device,
            b.app,
            b.computer,
            b.generation,
            draft.next_revision,
            draft.next_acl,
            draft.next_hash,
            at,
            lease.until()?,
            ["control", "owner.devices"]
        ]),
    )?;
    let cert = live::certify(&cfg, draft, &serde_json::to_vec(&activation)?, now)?;
    hub.store.db.locked().execute("UPDATE devices SET name='alterado durante assinatura'", [])?;
    assert!(sync::promote(hub, &cfg, &cert, lease, record.root_revision, &record.root_hash).is_err());
    let db = hub.store.db.locked();
    assert_eq!(live::current(&db)?.context("record")?.0.sequence, record.sequence);
    assert_eq!(db.query_row("SELECT count(*) FROM personal_live", [], |r| r.get::<_, i64>(0))?, 1);
    assert!(db.query_row("SELECT ever_claimed=1 FROM first_owner", [], |r| r.get::<_, bool>(0))?);
    Ok(())
}
async fn personal_accept_live(fixture: &PersonalLiveFixture) -> Result<u64> {
    use super::{live, protocol, sync};
    use crate::LockExt;
    let hub = &fixture.hub;
    let (cfg, root) =
        hub.personal.with(|p| Ok((p.config.clone(), p.journal.as_ref().context("root")?.original_bytes().to_vec())))?;
    let root_path = personal_root_file(fixture, "root-journal-fixture", &root)?;
    *hub.personal.test_journal.locked() = Some(root_path);
    let record = {
        let db = hub.store.db.locked();
        live::current(&db)?.context("record")?.0
    };
    let v = fixture.vectors.clone();
    let original_record = record.clone();
    let host = hub.identity.sign_pub_b64();
    *hub.personal.test_transport.locked() = Some(Arc::new(move |route, raw, header| {
        let cfg = cfg.clone();
        let r = original_record.clone();
        let v = v.clone();
        let host = host.clone();
        Box::pin(async move {
            let fields = header
                .split(',')
                .map(|s| s.split_once('=').context("sig field"))
                .collect::<Result<std::collections::HashMap<_, _>>>()?;
            let at = fields.get("ts").context("ts")?.parse::<i64>()?;
            let nonce = fields.get("nonce").context("nonce")?;
            let input = crypto::request_sig_input(
                "POST",
                "cloud.example.invalid",
                &format!("/v1/personal-bootstrap/{route}"),
                at,
                nonce,
                &raw,
            );
            crypto::verify(
                &crypto::unb64_n::<32>(&host)?,
                input.as_bytes(),
                &crypto::unb64_n::<64>(fields.get("sig").context("sig")?)?,
            )?;
            let body = protocol::packet(&raw)?;
            let out = match route.as_str() {
                "receipt" => {
                    assert_eq!(body["receipt"], r.originals["receipt"]);
                    json!({"intentId":r.originals["intent"][0][2],"candidateId":r.originals["local-ticket"][0][4]})
                }
                "read" => {
                    cfg.verifier(&r.binding.app)?.verify(
                        super::shapes::Kind::Read,
                        &serde_json::to_vec(&body["read"])?,
                        wire::Time::Current(u64::try_from(at)?),
                        wire::Reader::Hub,
                    )?;
                    let state = personal_signed(
                        &v,
                        "issuer",
                        &json!([
                            wire::domain("state"),
                            cfg.context(),
                            r.originals["intent"][0][2],
                            r.originals["local-ticket"][0][4],
                            r.binding.h_t,
                            "hubActive",
                            5,
                            r.binding.grant,
                            wire::hash(&r.originals["grant"][0])?,
                            r.binding.h_r,
                            wire::hash(&protocol::canonical(&r.activation)?[0])?,
                            null,
                            null,
                            null,
                            body["read"][0][5],
                            u64::try_from(at)?,
                            protocol::canonical(&r.activation)?[0][14]
                        ]),
                    )?;
                    let mut originals = json!({});
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
                    ] {
                        originals[remote] = r.originals[local].clone();
                    }
                    json!({"state":state,"originals":originals,"activationCurrent":protocol::canonical(&r.activation)?})
                }
                _ => anyhow::bail!("unexpected controlled route"),
            };
            Ok(serde_json::to_vec(&out)?)
        })
    }));
    assert!(sync::cycle(hub, "captured-fixture").await?);
    sync::socket(hub)
}
#[tokio::test]
async fn personal_host_b2_actual_channel_probe_and_old_activation_cut() -> Result<()> {
    use super::{probe::PersonalRelayAdmission, sync};
    use crate::{
        LockExt,
        remote::channel::{self, ChannelKind, Out, Transport},
    };
    let fixture = personal_live_fixture().await?;
    let hub = &fixture.hub;
    let epoch = personal_accept_live(&fixture).await?;
    let app = fixture.vectors["packet"]["intent"][0][6].as_str().context("A")?;
    let admission = PersonalRelayAdmission::from_open(hub, epoch, "controlled-worker-link", app)?;
    assert!(PersonalRelayAdmission::from_open(hub, epoch, "other", &crypto::b64(&[89; 32])).is_err());
    let (tx, incoming) = tokio::sync::mpsc::channel(4);
    let (outgoing, mut rx) = tokio::sync::mpsc::channel(1);
    let h = hub.clone();
    let run = tokio::spawn(async move {
        channel::run(
            h,
            Transport { incoming, outgoing, porta: channel::Porta::default() },
            ChannelKind::PersonalRelay { admission: Box::new(admission) },
        )
        .await;
    });
    let key = ed25519_dalek::SigningKey::from_bytes(&crypto::unb64_n::<32>(
        fixture.vectors["seeds"]["app"].as_str().context("A seed")?,
    )?);
    let ephemeral = x25519_dalek::StaticSecret::from([51; 32]);
    let public = crypto::x25519_pub(&ephemeral);
    let nonce = [52; 32];
    let dk = key.verifying_key().to_bytes();
    tx.send(json!({"t":"hello","v":1,"dk":crypto::b64(&dk),"ek":crypto::b64(&public),"n":crypto::b64(&nonce),"sig":crypto::b64(&crypto::sign(&key,&crypto::hs1_input(&hub.identity.cid_raw(),&dk,&public,&nonce)))})).await?;
    let Out::Msg(welcome) = rx.recv().await.context("welcome")? else {
        anyhow::bail!("welcome closed");
    };
    let host = crypto::unb64_n::<32>(welcome["ek"].as_str().context("H ephemeral")?)?;
    let transcript = crypto::transcript_hash(&hub.identity.cid_raw(), &dk, &public, &nonce, &host);
    crypto::verify(
        &hub.identity.sign_pub(),
        &transcript,
        &crypto::unb64_n::<64>(welcome["sig"].as_str().context("H sig")?)?,
    )?;
    let keys = crypto::channel_keys(&crypto::x25519(&ephemeral, &host)?, &transcript);
    let mut sealer = crypto::Sealer::new(keys.d2h);
    let mut opener = crypto::Opener::new(keys.h2d, 80 * 1024);
    for (id, method, body) in
        [(1, "hello", json!({})), (2, "donnaPersonalSessionProof", json!({"nonce":crypto::b64(&[53;32])}))]
    {
        for f in sealer.seal(&serde_json::to_vec(&json!({"id":id,"m":method,"b":body}))?).context("frames")? {
            tx.send(f).await?;
        }
        let Out::Msg(frame) = rx.recv().await.context("response")? else {
            anyhow::bail!("probe closed");
        };
        let raw = opener.open(&frame).map_err(|_| anyhow::anyhow!("response crypto"))?.context("response fragment")?;
        let response: Value = serde_json::from_slice(&raw)?;
        assert_eq!(response["id"], id);
        if id == 1 {
            assert_eq!(response["ok"].as_object().context("hello")?.len(), 7);
            assert_eq!(response["ok"]["computerId"], hub.identity.computer_id());
        } else {
            assert_eq!(response["ok"]["nonce"], body["nonce"]);
            assert_eq!(response["ok"].as_object().context("proof")?.len(), 11);
            assert_eq!(response["ok"]["profile"], "personal-linux-v1");
        }
    }
    assert!(!run.is_finished(), "duas RPCs não fecham o probe antes do PIN/peerclose");
    drop(tx);
    run.await?;
    let old = PersonalRelayAdmission::from_open(hub, epoch, "queued-old", app)?;
    hub.store.db.locked().execute("UPDATE devices SET name='ACL changed after await'", [])?;
    assert!(old.recheck(hub).is_err());
    assert!(sync::guard(hub, Some(epoch), true, |_| Ok(())).is_err());
    assert!(
        super::guard_generic(
            hub,
            &crate::api::devices::Caller::Device {
                key: app.into(),
                scopes: vec![crate::store::Scope::Control],
                autoridade: None,
            }
        )
        .is_err()
    );
    Ok(())
}
#[tokio::test]
async fn personal_host_b2_null_fence_after_r_then_superior_fence_preserves_history() -> Result<()> {
    use super::{fences, live, protocol};
    use crate::LockExt;
    let fixture = personal_live_fixture().await?;
    let hub = &fixture.hub;
    let (record, _) = {
        let db = hub.store.db.locked();
        live::current(&db)?.context("record")?
    };
    *hub.personal.test_transport.locked() =
        Some(Arc::new(|_, _, _| Box::pin(async { anyhow::bail!("controlled ACK transport loss") })));
    let cfg = hub.personal.with(|p| Ok(p.config.clone()))?;
    let mut previous_y = None;
    for revision in [1, 2] {
        let now = hub.personal.with(|p| p.clock.issued())?;
        let (grant, receipt) = if revision == 1 {
            (Value::Null, Value::Null)
        } else {
            (json!(record.binding.grant), json!(record.binding.h_r))
        };
        let w = personal_signed(
            &fixture.vectors,
            "issuer",
            &json!([
                wire::domain("fence"),
                record.binding.context,
                record.binding.h_t,
                wire::hash(&record.originals["grant"][0])?,
                grant,
                receipt,
                revision,
                "ownerRevoked",
                now
            ]),
        )?;
        // W e sua cadeia são verificados realmente; S/HTTP são fronteira de transporte controlada.
        assert!(
            fences::apply(hub, &record.binding.capture, &json!({"originals":{"fence":w},"state":null})).await.is_err()
        );
        // Sem backend externo de ACK: resultado é unknown APÓS commit, jamais rollback falso.
        let db = hub.store.db.locked();
        let (y, x): (Vec<u8>, Option<Vec<u8>>) = db.query_row(
            "SELECT y,x FROM personal_fence_history WHERE capture=? AND revision=?",
            rusqlite::params![record.binding.capture, revision],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let y = protocol::canonical(&y)?;
        assert_eq!(y[0][4], grant);
        assert_eq!(y[0][5], receipt);
        assert_eq!(db.query_row("SELECT count(*) FROM devices", [], |r| r.get::<_, i64>(0))?, i64::from(revision == 1));
        if revision == 1 {
            assert!(x.is_none());
            previous_y = Some(y);
        } else {
            assert!(x.is_some());
            assert_ne!(previous_y.as_ref(), Some(&y));
        }
        assert_eq!(db.query_row("SELECT count(*) FROM personal_fence_history", [], |r| r.get::<_, i64>(0))?, revision);
        assert!(db.query_row("SELECT ever_claimed=1 AND tombstone=1 FROM personal_generation", [], |r| {
            r.get::<_, bool>(0)
        })?);
        assert_eq!(live::session(&db, &record.binding.capture, &cfg)?["receipt"], record.originals["receipt"]);
        assert!(db.execute("DELETE FROM personal_fence_history", []).is_err());
    }
    assert!(!hub.cloud_synced.load(std::sync::atomic::Ordering::Acquire));
    Ok(())
}
#[tokio::test]
async fn personal_host_b2_causal_wait_reuses_original_kf_and_commits_only_after_f() -> Result<()> {
    let (hub, v, dir) = setup()?;
    hub.personal.with(|p| p.clock.freeze_fixture_time())?;
    let (k, mut f) = kf(&hub, &v)?;
    let predecessor = hub.personal.with(|p| p.clock.issued())?.checked_add(40).context("future F")?;
    f[0][7] = json!(predecessor);
    f = personal_signed(&v, "app", &f[0])?;
    assert!(
        hub.personal
            .with(|p| hub.store.personal_session(&p.config, "captured-fixture", |s| channel::proof(
                &hub,
                &p.config,
                &mut p.clock,
                s,
                &k,
                &f
            )))
            .is_err()
    );
    hub.personal.with(|p| {
        hub.store.personal_session(&p.config, "captured-fixture", |s| {
            assert!(s.originals().get("receipt").is_none());
            assert!(s.originals().get("_pending").is_none());
            assert!(s.originals().get("proof").is_none());
            Ok(())
        })
    })?;
    {
        let mut waiting =
            std::pin::pin!(ceremony::wait_causal(&hub, predecessor, f[0][8].as_u64().context("F expiry")?));
        assert!(futures::poll!(&mut waiting).is_pending());
        // Só o contador desta instância avança. Nenhum epoch/deadline/original é reancorado.
        hub.personal.with(|p| p.clock.advance_fixture_time(41))?;
        waiting.await?;
    }
    let r = hub.personal.with(|p| {
        hub.store.personal_session(&p.config, "captured-fixture", |s| {
            channel::proof(&hub, &p.config, &mut p.clock, s, &k, &f)
        })
    })?;
    assert!(r[0][7].as_u64().context("R time")? >= predecessor);
    hub.personal.with(|p| {
        hub.store.personal_session(&p.config, "captured-fixture", |s| {
            assert_eq!(s.originals()["challenge"], k);
            assert_eq!(s.originals()["proof"], f);
            assert_eq!(s.originals()["receipt"], r);
            Ok(())
        })
    })?;
    drop(hub);
    std::fs::remove_dir_all(dir)?;
    Ok(())
}
#[tokio::test]
async fn personal_host_b2_reopen_restores_original_deadline_without_socket_or_auto_acceptance() -> Result<()> {
    use super::{live, sync};
    use crate::LockExt;
    let fixture = personal_live_fixture().await?;
    let original = {
        let db = fixture.hub.store.db.locked();
        live::current(&db)?.context("record")?.0
    };
    let cfg = fixture.hub.personal.with(|p| Ok(p.config.clone()))?;
    let root = fixture.hub.personal.with(|p| Ok(p.journal.as_ref().context("root")?.original_bytes().to_vec()))?;
    let root_path = personal_root_file(&fixture, "restart-root-fixture", &root)?;
    let journal = provisioning::RootJournal::read(&cfg, &root)?;
    let (_, identity) = provisioning::fixture()?;
    let reopened = Hub::new(
        Store::open(&fixture.directory.join("codync.db"))?,
        "fixture".into(),
        identity,
        "fixture-token".into(),
        0,
    );
    reopened.personal.install(cfg, Some(journal), Clock::fixture(fixture.vectors["now"].as_u64().context("time")?)?)?;
    *reopened.personal.test_journal.locked() = Some(root_path);
    assert!(reopened.cloud.status().owner.is_none());
    sync::restore(&reopened)?;
    assert!(!reopened.cloud_synced.load(std::sync::atomic::Ordering::Acquire));
    assert!(sync::socket(&reopened).is_err());
    reopened.personal.with(|p| {
        let applied = p.applied.as_ref().context("restored")?;
        let saved: Value = serde_json::from_slice(&original.lease)?;
        let current: Value = serde_json::from_slice(&applied.lease.bytes()?)?;
        assert_eq!(saved["deadline"], current["deadline"]);
        assert_eq!(saved["flight"], current["flight"]);
        assert_eq!(applied.record.activation, original.activation);
        assert!(!applied.accepted);
        Ok(())
    })?;
    assert_eq!(reopened.cloud.status().owner.context("owner")?.user_id, original.binding.owner);
    Ok(())
}
#[tokio::test]
async fn personal_host_b2_real_open_builder_binds_empty_get_http_nonce_kid_and_own_signature() -> Result<()> {
    use super::{live, protocol, relay};
    use crate::LockExt;
    let fixture = personal_live_fixture().await?;
    let hub = &fixture.hub;
    let record = {
        let db = hub.store.db.locked();
        live::current(&db)?.context("record")?.0
    };
    let request = relay::open_request(hub, &record)?;
    assert_eq!(request.method(), "GET");
    assert_eq!(request.uri().path(), "/v1/personal-relay/host");
    assert!(request.uri().query().is_none());
    assert!(request.headers().get("Content-Length").is_none());
    assert!(request.headers().get("Transfer-Encoding").is_none());
    let fields = request.headers()["Codync-Sig"]
        .to_str()?
        .split(',')
        .map(|s| s.split_once('=').context("sig field"))
        .collect::<Result<std::collections::HashMap<_, _>>>()?;
    assert_eq!(fields["kid"], record.binding.sign);
    let at = fields["ts"].parse::<i64>()?;
    let input = crypto::request_sig_input(
        "GET",
        request.uri().authority().context("authority")?.as_str(),
        request.uri().path(),
        at,
        fields["nonce"],
        b"",
    );
    crypto::verify(&hub.identity.sign_pub(), input.as_bytes(), &crypto::unb64_n::<64>(fields["sig"])?)?;
    let open = crypto::unb64(request.headers()["X-Donna-Personal-Open"].to_str()?)?;
    let value = protocol::canonical(&open)?;
    assert_eq!(value[0][8], fields["nonce"]);
    assert_eq!(value[0][9], request.uri().path());
    assert_eq!(value[0][3], "use");
    hub.personal.with(|p| {
        let verifier = p.config.verifier(&record.binding.app)?;
        let now = p.clock.validity()?;
        let check = |nonce: &str, path: &str, kid: &str| {
            verifier.verify_open(
                &open,
                wire::Time::Current(now),
                &wire::OpenRequest {
                    role: wire::Reader::Hub,
                    kid,
                    nonce,
                    path,
                    until: value[0][12].as_u64().context("open end")?,
                },
            )
        };
        check(fields["nonce"], request.uri().path(), &record.binding.sign)?;
        assert!(check("wrong", request.uri().path(), &record.binding.sign).is_err());
        assert!(check(fields["nonce"], "/channel", &record.binding.sign).is_err());
        assert!(check(fields["nonce"], request.uri().path(), &record.binding.app).is_err());
        Ok(())
    })?;
    Ok(())
}

#[tokio::test]
async fn personal_r32_cut_socket_interleaving_publishes_false_with_final_locked_state() -> Result<()> {
    use super::sync;
    use crate::LockExt;
    use std::sync::{Barrier, atomic::Ordering};
    let fixture = personal_live_fixture().await?;
    let hub = &fixture.hub;
    let original_epoch = personal_accept_live(&fixture).await?;
    hub.personal.with(|foundation| foundation.clock.freeze_fixture_time())?;
    assert!(hub.cloud_synced.load(Ordering::Acquire));
    assert!(hub.cloud.status().connected);
    assert!(hub.personal.with(|foundation| Ok(foundation.socket_live))?);

    let entered = Arc::new(Barrier::new(2));
    let released = Arc::new(Barrier::new(2));
    let cut_entered = entered.clone();
    let cut_released = released.clone();
    *hub.personal.test_cut_after_false.locked() = Some(Arc::new(move || {
        cut_entered.wait();
        cut_released.wait();
    }));
    let cutting = hub.clone();
    let cut_thread = std::thread::spawn(move || sync::cut(&cutting));
    entered.wait(); // O cut REAL publicou false; ainda não adquiriu Cloud/runtime.
    let first_false = !hub.cloud_synced.load(Ordering::Acquire);
    let before_cut_locks = hub.cloud.status().connected;
    let old_socket_live = hub.personal.with(|foundation| Ok(foundation.socket_live));
    let reopened = sync::socket(hub); // Valida E/V/activationCurrent/Root/Store REAIS.
    let reopened_synced = hub.cloud_synced.load(Ordering::Acquire);
    let reopened_connected = hub.cloud.status().connected;
    let reopened_live = hub.personal.with(|foundation| Ok(foundation.socket_live));
    // Liberar/join antes de qualquer asserção falhar: não deixar thread própria bloqueada.
    released.wait();
    cut_thread.join().map_err(|_| anyhow::anyhow!("cut fixture thread panicked"))?;

    assert!(first_false);
    assert!(before_cut_locks);
    assert!(old_socket_live?);
    assert!(reopened? > original_epoch);
    assert!(reopened_synced);
    assert!(reopened_connected);
    assert!(reopened_live?);
    assert!(!hub.personal.with(|foundation| Ok(foundation.socket_live))?);
    assert!(!hub.cloud.status().connected);
    assert!(!hub.cloud_synced.load(Ordering::Acquire), "cut must publish false with its final locked socket state");
    Ok(())
}

#[tokio::test]
async fn personal_r32_fence_commit_after_socket_reopens_during_causal_wait_publishes_false() -> Result<()> {
    use super::{fences, live, protocol, sync};
    use crate::LockExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let fixture = personal_live_fixture().await?;
    let hub = &fixture.hub;
    let original_epoch = personal_accept_live(&fixture).await?;
    hub.personal.with(|foundation| foundation.clock.freeze_fixture_time())?;
    let record = {
        let db = hub.store.db.locked();
        live::current(&db)?.context("live fixture record missing")?.0
    };
    let cfg = hub.personal.with(|foundation| Ok(foundation.config.clone()))?;
    let predecessor = hub.personal.with(|foundation| foundation.clock.issued())?.checked_add(40).context("future W")?;
    let fence = personal_signed(
        &fixture.vectors,
        "issuer",
        &json!([
            wire::domain("fence"),
            record.binding.context,
            record.binding.h_t,
            wire::hash(&record.originals["grant"][0])?,
            record.binding.grant,
            record.binding.h_r,
            1,
            "ownerRevoked",
            predecessor
        ]),
    )?;
    let reply = json!({"originals":{"fence":fence},"state":null});
    // Somente ACK externo é controlado; W/X/Y, assinaturas, CAS e efeitos SQLite não são simulados.
    let ack_attempts = Arc::new(AtomicUsize::new(0));
    let sent_acks = ack_attempts.clone();
    *hub.personal.test_transport.locked() = Some(Arc::new(move |route, _, _| {
        let sent_acks = sent_acks.clone();
        Box::pin(async move {
            assert_eq!(route, "fence-ack");
            sent_acks.fetch_add(1, Ordering::AcqRel);
            anyhow::bail!("controlled R32 fence ACK loss")
        })
    }));
    {
        let mut applying = std::pin::pin!(fences::apply(hub, &record.binding.capture, &reply));
        assert!(futures::poll!(&mut applying).is_pending());
        // Primeiro await do ramo sem Y é wait_causal. Cut já terminou, W ainda não comitou.
        assert!(hub.personal.with(|foundation| Ok(foundation.clock.window()?.lower < predecessor))?);
        assert!(!hub.cloud_synced.load(Ordering::Acquire));
        assert!(!hub.cloud.status().connected);
        assert!(!hub.personal.with(|foundation| Ok(foundation.socket_live))?);
        assert_eq!(ack_attempts.load(Ordering::Acquire), 0);
        {
            let db = hub.store.db.locked();
            assert_eq!(db.query_row("SELECT count(*) FROM personal_fence_history", [], |row| row.get::<_, i64>(0))?, 0);
            assert_eq!(
                db.query_row("SELECT count(*) FROM devices WHERE key=?", [&record.binding.app], |row| row
                    .get::<_, i64>(0))?,
                1
            );
            assert!(db.query_row("SELECT tombstone=0 AND ever_claimed=1 FROM personal_generation", [], |row| {
                row.get::<_, bool>(0)
            })?);
        }
        let reopened = sync::socket(hub)?;
        assert!(reopened > original_epoch);
        assert!(hub.cloud_synced.load(Ordering::Acquire));
        assert!(hub.cloud.status().connected);
        assert!(hub.personal.with(|foundation| Ok(foundation.socket_live))?);
        hub.personal.with(|foundation| foundation.clock.advance_fixture_time(41))?;
        let error = applying.await.expect_err("controlled ACK loss follows the real fence commit");
        assert_eq!(error.to_string(), "controlled R32 fence ACK loss");
    }
    assert_eq!(ack_attempts.load(Ordering::Acquire), 1);
    let now = hub.personal.with(|foundation| foundation.clock.validity())?;
    let mut committed = record.originals.clone();
    {
        let db = hub.store.db.locked();
        let (stored_fence, ack, revocation): (Vec<u8>, Vec<u8>, Vec<u8>) = db.query_row(
            "SELECT w,y,x FROM personal_fence_history WHERE capture=? AND revision=1",
            [&record.binding.capture],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(protocol::canonical(&stored_fence)?, fence);
        committed["fence"] = protocol::canonical(&stored_fence)?;
        committed["fence-ack"] = protocol::canonical(&ack)?;
        committed["revocation"] = protocol::canonical(&revocation)?;
        let actual_acl = live::acl(&db, None)?;
        assert_eq!(committed["fence-ack"][0][4], record.binding.grant);
        assert_eq!(committed["fence-ack"][0][5], record.binding.h_r);
        assert_eq!(committed["fence-ack"][0][6], actual_acl.0);
        assert_eq!(committed["fence-ack"][0][7], actual_acl.1);
        assert_eq!(committed["fence-ack"][0][8], actual_acl.2);
        assert_eq!(
            db.query_row("SELECT count(*) FROM devices WHERE key=?", [&record.binding.app], |row| row
                .get::<_, i64>(0))?,
            0
        );
        assert!(db.query_row("SELECT tombstone=1 AND ever_claimed=1 FROM personal_generation", [], |row| {
            row.get::<_, bool>(0)
        })?);
        assert!(db.query_row(
            "SELECT revision=1 FROM personal_fence_latest WHERE capture=?",
            [&record.binding.capture],
            |row| row.get::<_, bool>(0)
        )?);
    }
    ceremony::history(&cfg, &committed, now)?;
    assert!(sync::guard(hub, None, true, |_| Ok(())).is_err());
    assert!(!hub.personal.with(|foundation| Ok(foundation.socket_live))?);
    assert!(!hub.cloud.status().connected);
    assert!(
        !hub.cloud_synced.load(Ordering::Acquire),
        "fence commit must publish false even after a socket reopened after cut"
    );
    Ok(())
}

// Handler/Hub/SQLite/crypto reais; só transporte MPSC e tempo externo controlados.
struct PersonalCompletedProbe {
    fixture: PersonalLiveFixture,
    tx: tokio::sync::mpsc::Sender<Value>,
    rx: tokio::sync::mpsc::Receiver<crate::remote::channel::Out>,
    run: tokio::task::JoinHandle<()>,
    sealer: crypto::Sealer,
    changes: u64,
}
async fn personal_completed_probe_for_lifetime() -> Result<PersonalCompletedProbe> {
    use super::probe::PersonalRelayAdmission;
    use crate::{
        LockExt,
        remote::channel::{self, ChannelKind, Out, Transport},
    };
    let fixture = personal_live_fixture().await?;
    let hub = &fixture.hub;
    let epoch = personal_accept_live(&fixture).await?;
    let app = fixture.vectors["packet"]["intent"][0][6].as_str().context("A")?;
    let admission = PersonalRelayAdmission::from_open(hub, epoch, "controlled-worker-link", app)?;
    assert!(PersonalRelayAdmission::from_open(hub, epoch, "other", &crypto::b64(&[89; 32])).is_err());
    let (tx, incoming) = tokio::sync::mpsc::channel(4);
    let (outgoing, mut rx) = tokio::sync::mpsc::channel(1);
    let changes = hub.store.db.locked().total_changes();
    let h = hub.clone();
    let run = tokio::spawn(async move {
        channel::run(
            h,
            Transport { incoming, outgoing, porta: channel::Porta::default() },
            ChannelKind::PersonalRelay { admission: Box::new(admission) },
        )
        .await;
    });
    let key = ed25519_dalek::SigningKey::from_bytes(&crypto::unb64_n::<32>(
        fixture.vectors["seeds"]["app"].as_str().context("A seed")?,
    )?);
    let ephemeral = x25519_dalek::StaticSecret::from([51; 32]);
    let public = crypto::x25519_pub(&ephemeral);
    let nonce = [52; 32];
    let dk = key.verifying_key().to_bytes();
    tx.send(json!({"t":"hello","v":1,"dk":crypto::b64(&dk),"ek":crypto::b64(&public),"n":crypto::b64(&nonce),"sig":crypto::b64(&crypto::sign(&key,&crypto::hs1_input(&hub.identity.cid_raw(),&dk,&public,&nonce)))})).await?;
    let Out::Msg(welcome) = rx.recv().await.context("welcome")? else {
        anyhow::bail!("welcome closed");
    };
    let host = crypto::unb64_n::<32>(welcome["ek"].as_str().context("H ephemeral")?)?;
    let transcript = crypto::transcript_hash(&hub.identity.cid_raw(), &dk, &public, &nonce, &host);
    crypto::verify(
        &hub.identity.sign_pub(),
        &transcript,
        &crypto::unb64_n::<64>(welcome["sig"].as_str().context("H sig")?)?,
    )?;
    let keys = crypto::channel_keys(&crypto::x25519(&ephemeral, &host)?, &transcript);
    let mut sealer = crypto::Sealer::new(keys.d2h);
    let mut opener = crypto::Opener::new(keys.h2d, 80 * 1024);
    for (id, method, body) in
        [(1, "hello", json!({})), (2, "donnaPersonalSessionProof", json!({"nonce":crypto::b64(&[53;32])}))]
    {
        for f in sealer.seal(&serde_json::to_vec(&json!({"id":id,"m":method,"b":body}))?).context("frames")? {
            tx.send(f).await?;
        }
        let Out::Msg(frame) = rx.recv().await.context("response")? else {
            anyhow::bail!("probe closed");
        };
        let raw = opener.open(&frame).map_err(|_| anyhow::anyhow!("response crypto"))?.context("response fragment")?;
        let response: Value = serde_json::from_slice(&raw)?;
        assert_eq!(response["id"], id);
        if id == 1 {
            assert_eq!(response["ok"].as_object().context("hello")?.len(), 7);
            assert_eq!(response["ok"]["computerId"], hub.identity.computer_id());
        } else {
            assert_eq!(response["ok"]["nonce"], body["nonce"]);
            assert_eq!(response["ok"].as_object().context("proof")?.len(), 11);
            assert_eq!(response["ok"]["profile"], "personal-linux-v1");
        }
    }
    assert!(!run.is_finished());
    assert_eq!(hub.store.db.locked().total_changes(), changes);
    Ok(PersonalCompletedProbe { fixture, tx, rx, run, sealer, changes })
}
#[tokio::test]
async fn personal_m2_probe_extra_rpc_after_two_responses_is_refused_without_effects() -> Result<()> {
    use crate::{LockExt, remote::channel::Out};
    let mut probe = personal_completed_probe_for_lifetime().await?;
    let extra = json!({"id":3,"m":"hello","b":{}});
    for frame in probe.sealer.seal(&serde_json::to_vec(&extra)?).context("extra frame")? {
        probe.tx.send(frame).await?;
    }
    let closed = tokio::time::timeout(std::time::Duration::from_secs(1), probe.rx.recv())
        .await?
        .context("extra frame must close")?;
    assert!(matches!(closed, Out::Close(_, _)), "nenhuma terceira resposta RPC");
    probe.run.await?;
    assert_eq!(probe.fixture.hub.store.db.locked().total_changes(), probe.changes);
    Ok(())
}
#[tokio::test(start_paused = true)]
async fn personal_m2_probe_two_responses_do_not_extend_original_five_seconds() -> Result<()> {
    use crate::{LockExt, remote::channel::Out};
    let mut probe = personal_completed_probe_for_lifetime().await?;
    tokio::time::advance(std::time::Duration::from_millis(4999)).await;
    assert!(!probe.run.is_finished(), "probe retido até peerclose/deadline original");
    tokio::time::advance(std::time::Duration::from_millis(2)).await;
    tokio::task::yield_now().await;
    probe.run.await?;
    assert!(matches!(probe.rx.recv().await, Some(Out::Close(_, _))));
    assert_eq!(probe.fixture.hub.store.db.locked().total_changes(), probe.changes);
    Ok(())
}

/// Estado durável que a posse poderia tocar; leitura direta, sem transação de sessão.
fn possession_db_state(hub: &Hub) -> Result<Value> {
    use crate::LockExt;
    let db = hub.store.db.locked();
    let raw: String = db.query_row(
        "SELECT json_object(
            'sessions',(SELECT json_group_array(json_object('id',id,'revision',revision,'originals',json(originals))) FROM personal_sessions),
            'slot',(SELECT json_object('capture',capture,'grantHash',grant_hash) FROM personal_possession_slot WHERE singleton=1),
            'devices',(SELECT count(*) FROM devices),
            'origins',(SELECT count(*) FROM personal_device_origins))",
        [],
        |row| row.get(0),
    )?;
    Ok(serde_json::from_str(&raw)?)
}
async fn possession_settle(hub: &Hub, total: usize) -> Result<()> {
    use crate::LockExt;
    for _ in 0..200 {
        if hub.personal.possession.locked().total == total {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    anyhow::bail!("possession admission did not settle at {total}")
}

/// D-1/D-2 (parecer de Segurança #489): Origin/Cookie/Authorization/subprotocolo/query recusados antes de captura/DB;
/// teto por peer antes do upgrade; a vaga volta em close, erro e timeout, e uma sessão nova segue aceita.
#[tokio::test]
async fn personal_possession_refuses_browser_headers_and_saturation_before_db_then_releases() -> Result<()> {
    use crate::LockExt;
    use futures::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{Error as WsError, Message as ClientMessage, client::IntoClientRequest};
    let (hub, _v, dir) = setup()?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let router =
        axum::Router::new().route("/personal-possession", axum::routing::get(channel::upgrade)).with_state(hub.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, router.into_make_service_with_connect_info::<std::net::SocketAddr>()).await
    });
    let url = format!("ws://{address}/personal-possession");
    let refused = |r: std::result::Result<_, WsError>| match r {
        Err(WsError::Http(response)) => Some(response.status().as_u16()),
        _ => None,
    };
    let before = possession_db_state(&hub)?;
    for (name, value) in [
        ("origin", "http://pagina.example"),
        ("cookie", "s=1"),
        ("authorization", "Bearer x"),
        ("sec-websocket-protocol", "x"),
    ] {
        let mut request = url.as_str().into_client_request()?;
        request.headers_mut().insert(name, value.parse()?);
        assert_eq!(refused(tokio_tungstenite::connect_async(request).await), Some(400), "{name}");
    }
    assert_eq!(refused(tokio_tungstenite::connect_async(format!("{url}?x=1")).await), Some(400));
    assert_eq!(hub.personal.possession.locked().total, 0);
    assert_eq!(possession_db_state(&hub)?, before, "recusa sem captura/efeito");
    // Saturação do mesmo peer: o próximo é recusado antes do upgrade e de qualquer hello/DB.
    let mut held = Vec::new();
    for _ in 0..channel::POSSESSION_PER_PEER {
        held.push(tokio_tungstenite::connect_async(url.as_str()).await?.0);
    }
    possession_settle(&hub, channel::POSSESSION_PER_PEER).await?;
    assert_eq!(refused(tokio_tungstenite::connect_async(url.as_str()).await), Some(503));
    assert_eq!(possession_db_state(&hub)?, before, "saturação sem DB");
    // Close e erro (hello inválida) liberam; os demais saem pelo timeout de 5 s do receive.
    let mut closed = held.pop().context("held")?;
    closed.close(None).await?;
    possession_settle(&hub, channel::POSSESSION_PER_PEER - 1).await?;
    let mut invalid = held.pop().context("held")?;
    invalid.send(ClientMessage::Text("[]".into())).await?;
    possession_settle(&hub, channel::POSSESSION_PER_PEER - 2).await?;
    tokio::time::sleep(std::time::Duration::from_millis(5200)).await;
    possession_settle(&hub, 0).await?;
    assert!(hub.personal.possession.locked().peers.is_empty());
    // Controle: uma sessão nova volta a ser aceita.
    let (mut control, _) = tokio_tungstenite::connect_async(url.as_str()).await?;
    possession_settle(&hub, 1).await?;
    control.close(None).await?;
    let _ = control.next().await;
    possession_settle(&hub, 0).await?;
    drop(held);
    server.abort();
    std::fs::remove_dir_all(dir)?;
    Ok(())
}

/// D-2 sem rede: teto total e por peer (IPv4 e o mesmo IPv4 mapeado em IPv6 contam juntos); o Drop libera.
#[test]
fn personal_possession_admission_total_and_per_peer_ceiling_released_on_drop() -> Result<()> {
    use crate::LockExt;
    let (hub, _v, dir) = setup()?;
    let ip = |n: u8| std::net::IpAddr::from([10, 0, 0, n]);
    let all = (0..channel::POSSESSION_TOTAL)
        .map(|n| channel::Slot::enter(&hub, ip(u8::try_from(n)?)).context("slot under total"))
        .collect::<Result<Vec<_>>>()?;
    assert!(channel::Slot::enter(&hub, ip(250)).is_none(), "teto total");
    drop(all);
    assert_eq!(hub.personal.possession.locked().total, 0);
    let same = (0..channel::POSSESSION_PER_PEER)
        .map(|_| channel::Slot::enter(&hub, ip(1)).context("slot under per-peer"))
        .collect::<Result<Vec<_>>>()?;
    assert!(channel::Slot::enter(&hub, "::ffff:10.0.0.1".parse()?).is_none(), "teto por peer");
    assert!(channel::Slot::enter(&hub, ip(2)).is_some(), "outro peer segue");
    drop(same);
    let a = hub.personal.possession.locked();
    assert_eq!(a.total, 0);
    assert!(a.peers.is_empty());
    drop(a);
    std::fs::remove_dir_all(dir)?;
    Ok(())
}

/// HOST-R1-01 (contraexemplo da Sol 52, deslocado ao lower do fixture): consumedAt autenticado à frente do lower ⇒ zero K;
/// sem testemunho ou com orçamento de 5 s vencido ⇒ zero K; quando lower alcança c, K[5] >= c; K existente volta com os mesmos bytes.
#[test]
fn host_r1_01_first_k_waits_authenticated_consumed_at_budget_and_retry_keeps_bytes() -> Result<()> {
    let (hub, _v, dir) = setup()?;
    let call = |w: Option<&ceremony::Witness>| {
        hub.personal.with(|f| {
            hub.store.personal_session(&f.config, "captured-fixture", |s| {
                channel::challenge(&hub, &f.config, &mut f.clock, s, w)
            })
        })
    };
    let frozen = || -> Result<bool> {
        hub.personal.with(|f| {
            hub.store.personal_session(&f.config, "captured-fixture", |s| Ok(s.originals().get("challenge").is_some()))
        })
    };
    let lower = hub.personal.with(|f| {
        f.clock.freeze_fixture_time()?;
        Ok(f.clock.window()?.lower)
    })?;
    let consumed = lower + 700;
    assert!(call(Some(&ceremony::Witness::fixture(consumed)?)).is_err(), "lower < consumedAt não congela K");
    assert!(call(None).is_err(), "sem testemunho, sem K");
    assert!(call(Some(&ceremony::Witness::fixture_aged(lower, 5_001)?)).is_err(), "orçamento de 5 s vencido");
    assert!(!frozen()?);
    hub.personal.with(|f| f.clock.advance_fixture_time(800))?;
    let k = call(Some(&ceremony::Witness::fixture(consumed)?))?;
    assert!(k[0][5].as_u64().context("K time")? >= consumed);
    assert_eq!(call(None)?, k, "K existente: mesmos bytes, sem novo testemunho");
    std::fs::remove_dir_all(dir)?;
    Ok(())
}

/// R2 (HOST-R1-01 / CLOUD-R2-01): o `post_bytes` REAL admite o read de testemunho só com `witness: true`, com e sem deviceCode,
/// e continua aceitando os reads legados. Recusas estritas não chegam à costura (posterior à checagem e à assinatura).
#[tokio::test]
async fn host_r2_post_bytes_read_admits_witness_true_only_and_legacy_reads() -> Result<()> {
    use crate::LockExt;
    let (hub, _v, dir) = setup()?;
    let seen = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let sink = seen.clone();
    *hub.personal.test_transport.locked() = Some(Arc::new(move |route, raw, _header| {
        let sink = sink.clone();
        Box::pin(async move {
            assert_eq!(route, "read");
            sink.locked().push(serde_json::from_slice(&raw)?);
            Ok(b"{}".to_vec())
        })
    }));
    let read = json!([[wire::domain("read"), 1], "sig"]);
    let code = crypto::b64(&[5; 32]);
    let accepted = [
        json!({"read":read}),
        json!({"read":read,"deviceCode":code}),
        json!({"read":read,"witness":true}),
        json!({"read":read,"deviceCode":code,"witness":true}),
    ];
    for body in &accepted {
        super::transport::post_bytes(&hub, "read", &serde_json::to_vec(body)?).await?;
    }
    assert_eq!(*seen.locked(), accepted.to_vec());
    for body in [
        json!({"read":read,"witness":false}),
        json!({"read":read,"witness":null}),
        json!({"read":read,"witness":"true"}),
        json!({"read":read,"witness":1}),
        json!({"read":read,"witness":true,"extra":1}),
        json!({"read":read,"deviceCode":code,"extra":1}),
    ] {
        assert!(super::transport::post_bytes(&hub, "read", &serde_json::to_vec(&body)?).await.is_err(), "{body}");
    }
    assert_eq!(seen.locked().len(), accepted.len(), "recusas não chegam ao transporte");
    std::fs::remove_dir_all(dir)?;
    Ok(())
}
