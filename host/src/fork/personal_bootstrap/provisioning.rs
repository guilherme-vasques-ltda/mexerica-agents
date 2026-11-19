//! Consumidor root PL1. Nenhum arquivo, body, env ou health instala uma raiz.
use super::{
    clock::{Clock, Reference},
    manifest, protocol, secure_fs,
    shapes::Kind,
    wire,
};
#[cfg(any(test, not(target_os = "linux")))]
use crate::store::personal_bootstrap;
use crate::{
    hub::Hub,
    remote::{crypto, identity::Identity},
    store::{
        Store,
        personal_bootstrap::Evidence,
        personal_database::{self, CapturedDatabase},
    },
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(test)]
use serde_json::json;
use std::path::Path;
const CONFIG: &str = "/etc/donnabot/bootstrap-personal-v1.json";
const JOURNAL: &str = "/etc/donnabot/bootstrap-personal-v1.journal";
const ACS4: &str = "/etc/donnabot/bootstrap-v1.json";
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PublicIdentity {
    #[serde(rename = "hubGeneration")]
    pub generation: String,
    #[serde(rename = "hubComputerId")]
    pub computer_id: String,
    #[serde(rename = "hubSignKey")]
    pub sign_key: String,
    #[serde(rename = "hubBoxKey")]
    pub box_key: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[derive(Clone)]
struct Installed {
    schema: u8,
    profile: String,
    wire: u8,
    context: Value,
    identity: PublicIdentity,
    clock: Reference,
}
#[derive(Clone)]
pub(crate) struct AdmittedConfig {
    installed: Installed,
    original: Vec<u8>,
    issuer_key: String,
}
impl AdmittedConfig {
    pub(crate) fn original_bytes(&self) -> &[u8] {
        &self.original
    }
    pub(crate) fn identity(&self) -> &PublicIdentity {
        &self.installed.identity
    }
    pub(crate) fn context(&self) -> &Value {
        &self.installed.context
    }
    pub(super) fn verifier(&self, app: &str) -> Result<wire::Verifier> {
        wire::Verifier::new(
            self.context().clone(),
            wire::Keys {
                issuer: self.issuer_key.clone(),
                app: app.into(),
                hub: self.identity().sign_key.clone(),
                box_key: self.identity().box_key.clone(),
            },
        )
    }
    pub(super) fn matches(&self, h: &Identity) -> Result<()> {
        let i = self.identity();
        ensure!(
            i.sign_key == h.sign_pub_b64() && i.box_key == h.box_pub_b64() && i.computer_id == h.computer_id(),
            "personal identity mismatch"
        );
        Ok(())
    }
    pub(super) fn verify_time(&self, raw: &[u8], nonce: &str) -> Result<wire::Signed> {
        wire::verify_time(raw, self.context(), &self.issuer_key, &self.identity().sign_key, nonce)
    }
    pub(super) fn verify_witness(&self, raw: &[u8], nonce: &str) -> Result<wire::Signed> {
        wire::verify_witness(raw, self.context(), &self.issuer_key, &self.identity().sign_key, nonce)
    }
    pub(super) fn verify_snapshot(&self, raw: &[u8], nonce: &str) -> Result<wire::Signed> {
        wire::verify_snapshot(
            raw,
            self.context(),
            &self.issuer_key,
            &self.identity().sign_key,
            &self.identity().generation,
            nonce,
        )
    }
}
fn parse(raw: &[u8], context: &Value, issuer_key: String) -> Result<AdmittedConfig> {
    protocol::canonical(raw)?;
    let installed: Installed = serde_json::from_slice(raw)?;
    ensure!(
        installed.schema == 1
            && installed.profile == protocol::PROFILE
            && installed.wire == 1
            && installed.context == *context,
        "personal configuration not admitted"
    );
    ensure!(serde_json::to_vec(&installed)? == raw, "personal configuration order mismatch");
    let i = &installed.identity;
    for key in [&i.generation, &i.sign_key, &i.box_key] {
        ensure!(crypto::b64(&crypto::unb64_n::<32>(key)?) == *key, "noncanonical personal identity");
    }
    ensure!(i.computer_id == crypto::computer_id(&crypto::unb64_n::<32>(&i.sign_key)?), "personal computer mismatch");
    ensure!(i.sign_key != issuer_key, "personal Hub/issuer role collision");
    let r = &installed.clock;
    ensure!(
        uuid::Uuid::parse_str(&r.boot_id).is_ok_and(|u| u.to_string() == r.boot_id)
            && (1..=9_007_199_254_740_991).contains(&r.observed_epoch_ms)
            && r.observed_boottime_ms <= 9_007_199_254_740_991
            && (1..=300_000).contains(&r.valid_for_ms),
        "invalid personal seed"
    );
    Ok(AdmittedConfig { installed, original: raw.to_vec(), issuer_key })
}
fn admitted(raw: &[u8]) -> Result<AdmittedConfig> {
    let (context, key) = manifest::admitted_source()?.context("personal M3 artifact unavailable")?;
    parse(raw, &context.0, key)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivateIdentity {
    v: u8,
    sign: String,
    #[serde(rename = "box")]
    box_key: String,
}
fn identity(raw: &[u8]) -> Result<Identity> {
    let p: PrivateIdentity =
        serde_json::from_slice(raw).map_err(|_| anyhow::anyhow!("invalid protected personal identity"))?;
    ensure!(p.v == 1, "unsupported personal identity");
    let sign = crypto::unb64_n::<32>(&p.sign)?;
    let secret = crypto::unb64_n::<32>(&p.box_key)?;
    ensure!(crypto::b64(&sign) == p.sign && crypto::b64(&secret) == p.box_key, "noncanonical private identity");
    Ok(Identity {
        sign: ed25519_dalek::SigningKey::from_bytes(&sign),
        box_secret: x25519_dalek::StaticSecret::from(secret),
    })
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClockEvidence {
    time_request: Value,
    time: Value,
    boot_id: String,
    sent_boottime_ms: u64,
    received_boottime_ms: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalWire {
    schema: u8,
    profile: String,
    wire: u8,
    context: Value,
    identity: PublicIdentity,
    revision: u64,
    clock_evidence: ClockEvidence,
    originals: Option<Value>,
}
pub(super) struct RootJournal {
    original: Vec<u8>,
    retained: Option<Value>,
    revision: u64,
    seed: super::epoch::VerifiedSeed,
}
impl RootJournal {
    pub(super) fn refresh(cfg: &AdmittedConfig, old: Option<&Self>) -> Result<Self> {
        let bytes = secure_fs::root_file(Path::new(JOURNAL), 192 * 1024)?.context("personal root journal missing")?;
        let next = Self::read(cfg, &bytes)?;
        if let Some(old) = old {
            ensure!(
                next.revision >= old.revision && (next.revision != old.revision || next.original == old.original),
                "root journal revision changed"
            );
            if let Some(packet) = &old.retained {
                for (name, value) in packet.as_object().context("invalid root prefix")? {
                    ensure!(next.retained.as_ref().and_then(|p| p.get(name)) == Some(value), "root original changed");
                }
            }
        }
        Ok(next)
    }
    pub(super) fn follows(&self, old: &Self) -> Result<()> {
        ensure!(
            self.revision >= old.revision && (self.revision != old.revision || self.original == old.original),
            "root journal regressed"
        );
        if let Some(packet) = &old.retained {
            for (name, value) in packet.as_object().context("invalid root prefix")? {
                ensure!(self.retained.as_ref().and_then(|p| p.get(name)) == Some(value), "root original changed");
            }
        }
        Ok(())
    }
    pub(super) fn authorize(&self, cfg: &AdmittedConfig, packet: &Value, now: u64) -> Result<()> {
        let root = self.retained.as_ref().context("root authorization missing")?;
        for name in ["intent", "local-ticket", "app-consent"] {
            ensure!(
                root.get(name).is_some() && root.get(name) == packet.get(name),
                "root consent does not authorize this tuple"
            );
        }
        let app = root["intent"][0][6].as_str().context("invalid root app")?;
        let checked = wire::ceremony(&cfg.verifier(app)?, &serde_json::to_vec(root)?, wire::Time::Current(now))?;
        ensure!(checked.tuple()[10] == cfg.identity().generation, "root generation mismatch");
        Ok(())
    }

    pub(super) fn read(cfg: &AdmittedConfig, raw: &[u8]) -> Result<Self> {
        protocol::packet(raw)?;
        let journal: JournalWire = serde_json::from_slice(raw)?;
        ensure!(
            journal.schema == 1
                && journal.profile == protocol::PROFILE
                && journal.wire == 1
                && journal.context == *cfg.context()
                && journal.revision > 0
                && journal.revision <= 9_007_199_254_740_991
                && serde_json::to_value(&journal.identity)? == serde_json::to_value(cfg.identity())?
                && serde_json::to_vec(&journal)? == raw,
            "personal root journal mismatch"
        );
        let evidence = &journal.clock_evidence;
        let request = wire::verify_time_request(
            &serde_json::to_vec(&evidence.time_request)?,
            cfg.context(),
            &cfg.identity().sign_key,
        )?;
        let nonce = request.payload()[2].as_str().context("time request nonce missing")?;
        let signed_time = cfg.verify_time(&serde_json::to_vec(&evidence.time)?, nonce)?;
        let payload = signed_time.payload();
        let rtt =
            evidence.received_boottime_ms.checked_sub(evidence.sent_boottime_ms).context("seed BOOTTIME regressed")?;
        let reference = &cfg.installed.clock;
        ensure!(
            rtt <= 5000
                && evidence.boot_id == reference.boot_id
                && request.payload()[2] == payload[2]
                && request.payload()[3] == payload[3]
                && payload[4].as_u64().and_then(|n| n.checked_add(rtt)) == Some(reference.observed_epoch_ms)
                && evidence.received_boottime_ms == reference.observed_boottime_ms
                && payload[5].as_u64().and_then(|n| n.checked_sub(rtt)) == Some(reference.valid_for_ms),
            "root seed lacks matching signed time evidence"
        );
        let retained = if let Some(packet) = journal.originals {
            ensure!(
                packet.as_object().is_some_and(|m| m.keys().all(|k| matches!(
                    k.as_str(),
                    "intent"
                        | "local-ticket"
                        | "app-consent"
                        | "local-consent"
                        | "box-challenge"
                        | "box-proof"
                        | "approve"
                        | "local-confirm"
                ))),
                "unsupported root journal originals"
            );
            Some(packet)
        } else {
            None
        };
        Ok(Self {
            seed: super::epoch::VerifiedSeed::checked(
                cfg.installed.clock.clone(),
                &signed_time,
                &evidence.boot_id,
                evidence.sent_boottime_ms,
                evidence.received_boottime_ms,
            )?,
            original: raw.to_vec(),
            retained,
            revision: journal.revision,
        })
    }
    /// Compara apenas originais já verificados no Record; sem FS/crypto na guarda.
    pub(super) fn matches_applied(&self, record: &super::live::Record) -> Result<()> {
        ensure!(self.revision >= record.root_revision, "root revision regressed");
        if self.revision == record.root_revision {
            ensure!(protocol::hash(&self.original) == record.root_hash, "root bytes changed");
        }
        let prefix = self.retained.as_ref().context("root retained originals absent")?;
        for name in [
            "intent",
            "local-ticket",
            "app-consent",
            "local-consent",
            "box-challenge",
            "box-proof",
            "approve",
            "local-confirm",
        ] {
            ensure!(
                prefix.get(name).is_some() && prefix.get(name) == record.originals.get(name),
                "root retained tuple changed"
            );
        }
        Ok(())
    }
    pub(super) fn seed_clock(&self) -> Clock {
        let reference = self.seed.reference.clone();
        Clock::seeded(super::epoch::VerifiedSeed { reference, origin: self.seed.origin.clone() })
    }
    pub(super) fn retained(&self, cfg: &AdmittedConfig, clock: &mut Clock, kind: Kind, bytes: &[u8]) -> Result<()> {
        ensure!(matches!(kind, Kind::LocalConsent | Kind::LocalConfirm), "root retain kind invalid");
        let packet = self.retained.as_ref().context("root authorization missing")?;
        let app = packet["intent"][0][6].as_str().context("root journal intent missing")?;
        let verifier = wire::Verifier::new(
            cfg.context().clone(),
            wire::Keys {
                issuer: cfg.issuer_key.clone(),
                app: app.into(),
                hub: cfg.identity().sign_key.clone(),
                box_key: cfg.identity().box_key.clone(),
            },
        )?;
        let checked =
            wire::ceremony(&verifier, &serde_json::to_vec(packet)?, wire::Time::Historical(clock.validity()?))?;
        ensure!(
            checked.tuple()[10] == cfg.identity().generation && checked.original(kind)?.original_bytes() == bytes,
            "root retain original mismatch"
        );
        Ok(())
    }
    pub(super) fn original_bytes(&self) -> &[u8] {
        &self.original
    }
    pub(super) fn revision(&self) -> u64 {
        self.revision
    }
}
pub(crate) struct Prepared {
    cfg: AdmittedConfig,
    identity: Identity,
    journal: Option<RootJournal>,
    database: CapturedDatabase,
}
pub(crate) struct Installation {
    cfg: AdmittedConfig,
    journal: Option<RootJournal>,
}
fn precheck(e: &Evidence, config: bool, journal: bool, acs4_file: bool) -> Result<bool> {
    let personal = config || journal || e.personal;
    if personal {
        ensure!(e.tracked_birth, "untracked/missing personal database; no recovery by new ownership");
        ensure!(!e.inconsistent, "personal durable origin has no generation");
        ensure!(config, "personal configuration missing; identity cannot be repaired");
        ensure!(
            !acs4_file && !e.acs4 && (!e.prior_devices || e.personal),
            "ACS4/legacy/personal installation conflict"
        );
    }
    Ok(personal)
}
/// Antes de `open_store`/`Identity::load_or_create`. Nenhuma criação/rename/remoção.
pub(crate) fn prepare(dir: &Path) -> Result<Option<Prepared>> {
    #[cfg(target_os = "linux")]
    {
        let cfg = secure_fs::root_file(Path::new(CONFIG), protocol::LIMIT)?;
        let journal = secure_fs::root_file(Path::new(JOURNAL), 192 * 1024)?;
        let database = personal_database::capture(&dir.join("codync.db"))?;
        let absent = Evidence::default();
        let e = database.as_ref().map_or(&absent, CapturedDatabase::evidence);
        let acs4 = std::fs::symlink_metadata(ACS4)
            .map(|_| true)
            .or_else(|err| if err.kind() == std::io::ErrorKind::NotFound { Ok(false) } else { Err(err) })?;
        if !precheck(e, cfg.is_some(), journal.is_some(), acs4)? {
            return Ok(None);
        }
        let cfg = admitted(cfg.as_deref().context("personal configuration missing")?)?;
        if let Some(old) = &e.config {
            ensure!(old.as_slice() == cfg.original_bytes(), "personal root/DB configuration mismatch");
            let i = cfg.identity();
            ensure!(
                e.binding.as_ref()
                    == Some(&(
                        i.generation.clone(),
                        serde_json::to_string(cfg.context())?,
                        i.computer_id.clone(),
                        i.sign_key.clone(),
                        i.box_key.clone()
                    )),
                "personal DB identity mismatch"
            );
        }
        let bytes = secure_fs::identity_file(dir)?.context("personal identity missing; restore required")?;
        let identity = identity(&bytes)?;
        cfg.matches(&identity)?;
        let journal = journal.map(|b| RootJournal::read(&cfg, &b)).transpose()?;
        let database = database.context("personal preflight database missing")?;
        Ok(Some(Prepared { cfg, identity, journal, database }))
    }
    #[cfg(not(target_os = "linux"))]
    {
        ensure!(
            !personal_bootstrap::preflight(&dir.join("codync.db"))?.personal,
            "personal installation requires Linux"
        );
        Ok(None)
    }
}
impl Prepared {
    // A referência tipada exige Prepared admitido; a fábrica não aceita bool/FD do chamador.
    pub(crate) fn listener(_prepared: &Self) -> Result<super::helper::Listener> {
        super::helper::Listener::adopt()
    }

    pub(crate) fn open_store(self) -> Result<(Store, Identity, Installation)> {
        let store = Store::open_personal(self.database)?;
        Ok((store, self.identity, Installation { cfg: self.cfg, journal: self.journal }))
    }
}
impl Installation {
    pub(crate) fn install(self, hub: &Hub) -> Result<()> {
        Clock::prepare_sampler()?;
        self.cfg.matches(&hub.identity)?;
        hub.store.personal_install_generation(&self.cfg)?;
        let clock = if let Some(journal) = &self.journal { journal.seed_clock() } else { Clock::default() };
        hub.personal.install(self.cfg, self.journal, clock)
    }
}
/// Entrada compartilhada do CLI canônico; subcomandos não instalam nem recuperam PL1.
pub(crate) fn open_legacy_store(dir: &Path) -> Result<Store> {
    open_legacy_store_with(dir, secure_fs::root_file)
}
/// Ganho da main: com o motor configurado, pasta de dados própria, sem links nos ancestrais e 0700. Só depois das recusas PL1.
fn harden_data_dir(dir: &Path) -> Result<()> {
    #[cfg(unix)]
    crate::fork::private_fs::Directory::open(dir, true)?;
    #[cfg(not(unix))]
    std::fs::create_dir_all(dir)?;
    Ok(())
}
fn open_legacy_store_with(dir: &Path, read_root: impl Fn(&Path, usize) -> Result<Option<Vec<u8>>>) -> Result<Store> {
    // D430 PR-A: pasta de instalação isolada nunca abre pelo CLI legado, nem antes da raiz.
    ensure!(
        !crate::fork::personal_link::installation::selecionado(dir)?,
        "Este comando não pode alterar os dados de um vínculo pessoal. O fluxo próprio ainda não está disponível."
    );
    let path = dir.join("codync.db");
    #[cfg(target_os = "linux")]
    {
        let config = read_root(Path::new(CONFIG), protocol::LIMIT)?;
        let journal = read_root(Path::new(JOURNAL), 192 * 1024)?;
        ensure!(
            config.is_none() && journal.is_none(),
            "Este comando não pode alterar os dados de um vínculo pessoal. O fluxo próprio ainda não está disponível."
        );
        if let Some(database) = personal_database::capture(&path)? {
            ensure!(
                !database.evidence().personal,
                "Este comando não pode alterar os dados de um vínculo pessoal. O fluxo próprio ainda não está disponível."
            );
            // DB existente: conserva o handle observado; não reabre após a guarda.
            harden_data_dir(dir)?;
            return Store::open_personal(database);
        }
        #[cfg(test)]
        tests::after_absent(&path)?;
        harden_data_dir(dir)?;
        Store::open_created(personal_database::create_exclusive(&path)?)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = read_root;
        ensure!(
            !personal_bootstrap::preflight(&path)?.personal,
            "Este comando não pode alterar os dados de um vínculo pessoal. O fluxo próprio ainda não está disponível."
        );
        harden_data_dir(dir)?;
        Store::open(&path)
    }
}

/// D430: raiz PL1/ACS4 em `/etc` conflita com o nascimento isolado; lido antes de criar algo.
pub(crate) fn raiz_pessoal_presente() -> Result<bool> {
    #[cfg(target_os = "linux")]
    {
        Ok(secure_fs::root_file(Path::new(CONFIG), protocol::LIMIT)?.is_some()
            || secure_fs::root_file(Path::new(JOURNAL), 192 * 1024)?.is_some()
            || std::fs::symlink_metadata(ACS4)
                .map(|_| true)
                .or_else(|err| if err.kind() == std::io::ErrorKind::NotFound { Ok(false) } else { Err(err) })?)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(false)
    }
}

pub(crate) fn precheck_claim(store: &Store) -> Result<()> {
    ensure!(!store.personal_configured()?, "personal installation excludes legacy claim");
    #[cfg(target_os = "linux")]
    {
        ensure!(
            secure_fs::root_file(Path::new(CONFIG), protocol::LIMIT)?.is_none()
                && secure_fs::root_file(Path::new(JOURNAL), 192 * 1024)?.is_none(),
            "personal root state excludes legacy claim"
        );
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn fixture() -> Result<(AdmittedConfig, Identity)> {
    let v: Value = serde_json::from_str(include_str!("../../../../docs/reference/fixtures/remote-relay-vectors.json"))?;
    let p = &v["personalBootstrap"];
    let w = &p["wireVectors"];
    let id = Identity {
        sign: ed25519_dalek::SigningKey::from_bytes(&crypto::unb64_n::<32>(
            w["seeds"]["hub"].as_str().context("fixture H")?,
        )?),
        box_secret: x25519_dalek::StaticSecret::from(crypto::unb64_n::<32>(
            w["seeds"]["box"].as_str().context("fixture box")?,
        )?),
    };
    let raw = serde_json::to_vec(
        &json!({"schema":1,"profile":protocol::PROFILE,"wire":1,"context":p["context"],"identity":{"hubGeneration":w["tuple"][10],"hubComputerId":id.computer_id(),"hubSignKey":id.sign_pub_b64(),"hubBoxKey":id.box_pub_b64()},"clock":{"bootId":"c5386ca1-bad0-4cfb-b19b-88badf199264","observedEpochMs":w["now"],"observedBoottimeMs":100,"validForMs":300_000}}),
    )?;
    Ok((
        parse(
            &raw,
            &p["context"],
            p["manifest"]["personalBootstrap"]["issuerPublicKey"].as_str().context("fixture root")?.into(),
        )?,
        id,
    ))
}

/// Ator root sintético privado; passa pelo leitor/assinaturas de produção, sem instalar confiança.
#[cfg(test)]
pub(super) fn fixture_journal(cfg: &AdmittedConfig, h: &Identity, prefix: Value) -> Result<RootJournal> {
    let vectors: Value =
        serde_json::from_str(include_str!("../../../../docs/reference/fixtures/remote-relay-vectors.json"))?;
    let key = ed25519_dalek::SigningKey::from_bytes(&crypto::unb64_n::<32>(
        vectors["personalBootstrap"]["wireVectors"]["seeds"]["issuer"].as_str().context("fixture E")?,
    )?);
    let nonce = crypto::b64(&[202; 32]);
    let request = json!([wire::domain("time-request"), cfg.context(), nonce, h.computer_id()]);
    let time = json!([
        wire::domain("time"),
        cfg.context(),
        nonce,
        h.computer_id(),
        cfg.installed.clock.observed_epoch_ms,
        cfg.installed.clock.valid_for_ms
    ]);
    let j = JournalWire {
        schema: 1,
        profile: protocol::PROFILE.into(),
        wire: 1,
        context: cfg.context().clone(),
        identity: cfg.identity().clone(),
        revision: 1,
        clock_evidence: ClockEvidence {
            time_request: json!([request, crypto::b64(&h.sign(&serde_json::to_vec(&request)?))]),
            time: json!([time, crypto::b64(&crypto::sign(&key, &serde_json::to_vec(&time)?))]),
            boot_id: cfg.installed.clock.boot_id.clone(),
            sent_boottime_ms: cfg.installed.clock.observed_boottime_ms,
            received_boottime_ms: cfg.installed.clock.observed_boottime_ms,
        },
        originals: Some(prefix),
    };
    RootJournal::read(cfg, &serde_json::to_vec(&j)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LockExt;
    type Hook = Box<dyn FnOnce(&Path) -> Result<()>>;
    std::thread_local! { static AFTER_ABSENT: std::cell::RefCell<Option<Hook>> = const { std::cell::RefCell::new(None) }; }
    pub(super) fn after_absent(path: &Path) -> Result<()> {
        let hook = AFTER_ABSENT.with(|slot| slot.borrow_mut().take());
        if let Some(hook) = hook {
            hook(path)?;
        }
        Ok(())
    }

    #[test]
    fn config_shape_context_identity_and_admission_fail_closed() -> Result<()> {
        let (cfg, h) = fixture()?;
        cfg.matches(&h)?;
        assert!(admitted(cfg.original_bytes()).is_err()); // ADMITTED=None nunca aprende a fixture.
        let base: Value = serde_json::from_slice(cfg.original_bytes())?;
        for (key, value) in
            [("profile", json!("ACS4")), ("schema", json!(2)), ("extra", json!(true)), ("context", json!(["wrong"]))]
        {
            let mut m = base.clone();
            m[key] = value;
            assert!(parse(&serde_json::to_vec(&m)?, cfg.context(), cfg.issuer_key.clone()).is_err());
        }
        let mut bad = base.clone();
        bad["identity"]["hubBoxKey"] = json!(crypto::b64(&[111; 32]));
        let changed = parse(&serde_json::to_vec(&bad)?, cfg.context(), cfg.issuer_key.clone())?;
        assert!(changed.matches(&h).is_err());
        let mut bad = base.clone();
        bad["clock"]["validForMs"] = json!(300_001);
        assert!(parse(&serde_json::to_vec(&bad)?, cfg.context(), cfg.issuer_key.clone()).is_err());
        let mut raw = b" ".to_vec();
        raw.extend_from_slice(cfg.original_bytes());
        assert!(parse(&raw, cfg.context(), cfg.issuer_key.clone()).is_err());
        let parsed = identity(&serde_json::to_vec(
            &json!({"v":1,"sign":crypto::b64(h.sign.as_bytes()),"box":crypto::b64(h.box_secret.as_bytes())}),
        )?)?;
        cfg.matches(&parsed)?;
        Ok(())
    }
    #[test]
    fn missing_config_or_identity_cannot_erase_personal_origin_or_write_database() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("personal-precheck-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir)?;
        let path = dir.join("codync.db");
        let store = Store::open(&path)?;
        let (cfg, _) = fixture()?;
        store.personal_install_generation(&cfg)?;
        drop(store);
        let before = std::fs::read(&path)?;
        let e = personal_bootstrap::preflight(&path)?;
        assert!(e.personal && e.tracked_birth);
        assert!(precheck(&e, false, false, false).is_err());
        assert!(precheck(&e, true, false, true).is_err());
        assert!(!dir.join("identity.json").exists());
        assert_eq!(std::fs::read(&path)?, before);
        let absent = Evidence::default();
        assert!(precheck(&absent, true, false, false).is_err());
        assert!(!precheck(&absent, false, false, false)?);
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }
    // O seam substitui somente a leitura externa dos caminhos root fixos por
    // arquivos privados de fixture. A guarda e a abertura são as de produção.
    #[cfg(target_os = "linux")]
    fn cli_fixture_open(dir: &Path, root: &Path) -> Result<Store> {
        open_legacy_store_with(dir, |path, limit| {
            ensure!(path == Path::new(CONFIG) || path == Path::new(JOURNAL), "unexpected fixture root path");
            let name = path.file_name().context("fixture root filename missing")?;
            match std::fs::read(root.join(name)) {
                Ok(bytes) => {
                    ensure!(bytes.len() <= limit, "fixture root file too large");
                    Ok(Some(bytes))
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error.into()),
            }
        })
    }
    #[test]
    #[cfg(target_os = "linux")]
    fn cli_shared_open_refuses_lost_personal_db_without_new_birth_host_token_or_identity() -> Result<()> {
        let (dir, prepared, expected) = handoff_fixture()?;
        drop(prepared); // Serviço parado.
        let root = dir.join("root-fixture");
        std::fs::create_dir(&root)?;
        let config = fixture()?.0.original_bytes().to_vec();
        let config_path = root.join("bootstrap-personal-v1.json");
        std::fs::write(&config_path, &config)?;
        let identity = std::fs::read(dir.join("identity.json"))?;
        let path = dir.join("codync.db");
        let old = dir.join("retained.db");
        let original = std::fs::read(&path)?;
        std::fs::rename(&path, &old)?;
        // install e reset-token convergem nesta abertura antes de seus efeitos.
        assert!(cli_fixture_open(&dir, &root).is_err());
        assert!(cli_fixture_open(&dir, &root).is_err());
        assert!(!path.exists());
        assert!(!dir.join("token").exists());
        assert_eq!(std::fs::read(dir.join("identity.json"))?, identity);
        assert_eq!(std::fs::read(&config_path)?, config);
        assert_eq!(std::fs::read(&old)?, original);
        assert!(personal_bootstrap::preflight(&old)? == expected);
        let missing = personal_database::capture(&path)?;
        assert!(missing.is_none());
        assert!(precheck(&Evidence::default(), true, false, false).is_err()); // Startup não aceita birth novo.
        let c = rusqlite::Connection::open_with_flags(&old, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        assert_eq!(
            c.query_row("SELECT count(*) FROM kv WHERE k IN ('host_id','token')", [], |r| r.get::<_, u32>(0))?,
            0
        );
        drop(c);
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }
    #[test]
    #[cfg(target_os = "linux")]
    fn cli_shared_open_refuses_existing_personal_profile_even_without_root_files() -> Result<()> {
        let (dir, prepared, expected) = handoff_fixture()?;
        drop(prepared);
        let root = dir.join("root-fixture");
        std::fs::create_dir(&root)?;
        let cfg = fixture()?.0.original_bytes().to_vec();
        let path = root.join("bootstrap-personal-v1.json");
        std::fs::write(&path, &cfg)?;
        let original = std::fs::read(dir.join("codync.db"))?;
        let identity = std::fs::read(dir.join("identity.json"))?;
        assert!(cli_fixture_open(&dir, &root).is_err()); // Config pessoal presente.
        assert_eq!(std::fs::read(&path)?, cfg);
        std::fs::remove_file(path)?; // Agora só a origem durável prova PL1.
        assert!(cli_fixture_open(&dir, &root).is_err());
        assert!(personal_bootstrap::preflight(&dir.join("codync.db"))? == expected);
        assert_eq!(std::fs::read(dir.join("codync.db"))?, original);
        assert_eq!(std::fs::read(dir.join("identity.json"))?, identity);
        assert!(!dir.join("token").exists());
        let c =
            rusqlite::Connection::open_with_flags(dir.join("codync.db"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        assert_eq!(c.query_row("SELECT v FROM kv WHERE k='schema'", [], |r| r.get::<_, String>(0))?, "5");
        drop(c);
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }
    #[test]
    #[cfg(target_os = "linux")]
    fn cli_shared_open_refuses_personal_config_or_journal_before_creating_data_directory() -> Result<()> {
        let base = std::env::temp_dir().join(format!("personal-cli-{}", uuid::Uuid::new_v4()));
        let root = base.join("root");
        std::fs::create_dir_all(&root)?;
        let dir = base.join("missing-data");
        let config = root.join("bootstrap-personal-v1.json");
        let bytes = fixture()?.0.original_bytes().to_vec();
        std::fs::write(&config, &bytes)?;
        assert!(cli_fixture_open(&dir, &root).is_err());
        assert!(!dir.exists());
        let journal = root.join("bootstrap-personal-v1.journal");
        std::fs::rename(&config, &journal)?; // Presença irregular também recusa, não qualifica raiz.
        assert!(cli_fixture_open(&dir, &root).is_err());
        assert!(!dir.exists());
        assert_eq!(std::fs::read(journal)?, bytes);
        std::fs::remove_dir_all(base)?;
        Ok(())
    }
    /// HOST-MERGE-01: com o motor configurado (subprocesso isolado por `motor_exec::test_support::case`), as recusas PL1 vêm antes de criar
    /// ou endurecer a pasta; só o legado qualificado cria ou endurece (dono, sem links, 0700) e entrega o DB existente pelo handle capturado.
    #[test]
    #[cfg(target_os = "linux")]
    fn cli_legacy_open_hardens_data_dir_only_after_personal_refusals() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let Some(_case) = crate::fork::motor_exec::test_support::case(
            "fork::personal_bootstrap::provisioning::tests::cli_legacy_open_hardens_data_dir_only_after_personal_refusals",
        ) else {
            return Ok(());
        };
        assert!(crate::fork::motor_exec::configured());
        let base = std::env::temp_dir().join(format!("personal-harden-{}", uuid::Uuid::new_v4()));
        let root = base.join("root");
        std::fs::create_dir_all(&root)?;
        let mode = |p: &Path| -> Result<u32> { Ok(std::fs::metadata(p)?.permissions().mode() & 0o7777) };
        // Recusa com config pessoal: pasta ausente continua ausente; pasta existente 0755 continua 0755.
        let config = root.join("bootstrap-personal-v1.json");
        std::fs::write(&config, fixture()?.0.original_bytes())?;
        let missing = base.join("missing-data");
        assert!(cli_fixture_open(&missing, &root).is_err());
        assert!(!missing.exists());
        let open = base.join("open-data");
        std::fs::create_dir(&open)?;
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755))?;
        assert!(cli_fixture_open(&open, &root).is_err());
        assert_eq!(mode(&open)?, 0o755, "recusa não endurece");
        std::fs::remove_file(&config)?;
        // Legado qualificado: pasta ausente é criada já 0700, com DB novo exclusivo.
        let created = base.join("created-data");
        let store = cli_fixture_open(&created, &root)?;
        store.kv_set("host_id", "host-harden")?;
        drop(store);
        assert_eq!(mode(&created)?, 0o700);
        // DB existente: pasta 0755 volta a 0700 e a MESMA Connection capturada abre o Store (dados preservados).
        std::fs::set_permissions(&created, std::fs::Permissions::from_mode(0o755))?;
        let reopened = cli_fixture_open(&created, &root)?;
        assert_eq!(mode(&created)?, 0o700);
        assert_eq!(reopened.kv_get("host_id").as_deref(), Some("host-harden"));
        drop(reopened);
        std::fs::remove_dir_all(base)?;
        Ok(())
    }
    #[test]
    #[cfg(target_os = "linux")]
    fn cli_creation_refuses_personal_restore_after_absence_before_any_migration() -> Result<()> {
        let (dir, prepared, expected) = handoff_fixture()?;
        drop(prepared);
        let root = dir.join("root-fixture");
        std::fs::create_dir(&root)?;
        let path = dir.join("codync.db");
        let retained = dir.join("retained.db");
        let original = std::fs::read(&path)?;
        let identity = std::fs::read(dir.join("identity.json"))?;
        std::fs::rename(&path, &retained)?;
        AFTER_ABSENT.with(|slot| {
            assert!(
                slot.borrow_mut()
                    .replace(Box::new(move |path| {
                        assert!(!path.exists());
                        std::fs::rename(&retained, path)?;
                        Ok(())
                    }))
                    .is_none()
            );
        });
        assert!(cli_fixture_open(&dir, &root).is_err());
        assert_eq!(std::fs::read(&path)?, original);
        assert_eq!(std::fs::read(dir.join("identity.json"))?, identity);
        assert!(personal_bootstrap::preflight(&path)? == expected);
        assert!(!dir.join("token").exists());
        let c = rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        assert_eq!(c.query_row("SELECT v FROM kv WHERE k='schema'", [], |r| r.get::<_, String>(0))?, "5");
        assert_eq!(
            c.query_row("SELECT count(*) FROM kv WHERE k IN ('host_id','token')", [], |r| r.get::<_, u32>(0))?,
            0
        );
        drop(c);
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }
    #[test]
    #[cfg(target_os = "linux")]
    fn cli_creation_keeps_captured_acs4_claim_and_same_input_rule() -> Result<()> {
        let base = std::env::temp_dir().join(format!("acs4-cli-{}", uuid::Uuid::new_v4()));
        let root = base.join("root");
        std::fs::create_dir_all(&root)?;
        let dir = base.join("data");
        let store = cli_fixture_open(&dir, &root)?;
        let (_, h) = fixture()?;
        let cfg = crate::store::bootstrap::TrustedGeneration {
            generation: crypto::b64(&[222; 32]),
            issuer_key_id: "operator_fixture".into(),
            issuer_key: crypto::b64(&[52; 32]),
            issuer: "https://issuer.invalid".into(),
            deployment_id: "fixture".into(),
            env: "dev".into(),
            manifest_hash: "a".repeat(64),
            resource_digest: "b".repeat(64),
            computer_id: h.computer_id(),
            sign_key: h.sign_pub_b64(),
            box_key: h.box_pub_b64(),
        };
        store.bootstrap_install_generation(&cfg)?;
        let input =
            crypto::claim_input("claim_fixture", "nonce_fixture", "owner_fixture", &h.computer_id(), &h.box_pub_b64());
        let signature = crypto::b64(&h.sign(input.as_bytes()));
        assert_eq!(
            store.bootstrap_claim_a1(Some(&cfg.generation), &input, |_| Ok(signature.clone()))?,
            Some(signature.clone())
        );
        drop(store);
        let expected = personal_bootstrap::preflight(&dir.join("codync.db"))?;
        let store = cli_fixture_open(&dir, &root)?;
        assert!(personal_bootstrap::preflight(&dir.join("codync.db"))? == expected);
        assert_eq!(
            store.bootstrap_claim_a1(Some(&cfg.generation), &input, |_| panic!(
                "same ACS4 input must retain original signature"
            ))?,
            Some(signature)
        );
        assert!(
            store
                .bootstrap_claim_a1(Some(&cfg.generation), "other-input", |_| panic!("ACS4 must not sign new input"))
                .is_err()
        );
        drop(store);
        std::fs::remove_dir_all(base)?;
        Ok(())
    }
    #[test]
    #[cfg(target_os = "linux")]
    fn cli_shared_open_keeps_legacy_metadata_and_token_reset_without_personal_profile() -> Result<()> {
        let base = std::env::temp_dir().join(format!("legacy-cli-{}", uuid::Uuid::new_v4()));
        let root = base.join("root");
        std::fs::create_dir_all(&root)?;
        let dir = base.join("data");
        let store = cli_fixture_open(&dir, &root)?;
        let born = personal_bootstrap::preflight(&dir.join("codync.db"))?;
        assert!(born.tracked_birth && !born.personal);
        assert_eq!(store.db.locked().pragma_query_value(None, "recursive_triggers", |r| r.get::<_, i64>(0))?, 1);
        let (_, h) = fixture()?;
        let first = crypto::claim_input(
            "claim_fixture_1",
            "nonce_fixture_1",
            "owner_fixture",
            &h.computer_id(),
            &h.box_pub_b64(),
        );
        let next = crypto::claim_input(
            "claim_fixture_2",
            "nonce_fixture_2",
            "owner_fixture",
            &h.computer_id(),
            &h.box_pub_b64(),
        );
        store.kv_set("host_id", "host-fixture")?;
        store.kv_set("token", "token-fixture-1")?;
        store.bootstrap_claim_a1(None, &first, |_| Ok(crypto::b64(&h.sign(first.as_bytes()))))?;
        drop(store);
        let store = cli_fixture_open(&dir, &root)?;
        assert_eq!(store.kv_get("host_id").as_deref(), Some("host-fixture"));
        assert_eq!(store.kv_get("token").as_deref(), Some("token-fixture-1"));
        store.kv_set("token", "token-fixture-2")?;
        assert_eq!(store.kv_get("token").as_deref(), Some("token-fixture-2"));
        assert_eq!(
            store.bootstrap_claim_a1(None, &next, |_| Ok(crypto::b64(&h.sign(next.as_bytes()))))?,
            Some(crypto::b64(&h.sign(next.as_bytes())))
        );
        assert!(!dir.join("identity.json").exists()); // Abertura de Store não gera H.
        drop(store);
        std::fs::remove_dir_all(base)?;
        Ok(())
    }

    // Seams só cfg(test): configuração/identidade públicas sintéticas; a abertura
    // usa Prepared::open_store e Store::open_personal, as mesmas APIs de serve.
    fn prepared_existing(dir: &Path) -> Result<Prepared> {
        let (cfg, identity) = fixture()?;
        let database = personal_database::capture(&dir.join("codync.db"))?.context("fixture database missing")?;
        ensure!(precheck(database.evidence(), true, false, false)?, "fixture must be personal");
        Ok(Prepared { cfg, identity, journal: None, database })
    }
    fn handoff_fixture() -> Result<(std::path::PathBuf, Prepared, Evidence)> {
        use crate::store::{Device, DeviceSource, Scope};
        let dir = std::env::temp_dir().join(format!("personal-handoff-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir)?;
        let path = dir.join("codync.db");
        let store = Store::open(&path)?;
        let (cfg, h) = fixture()?;
        store.personal_install_generation(&cfg)?;
        let key = crypto::b64(&[212; 32]);
        store.put_device(&Device {
            key: key.clone(),
            name: "fixture".into(),
            platform: "fixture".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![Scope::Control],
            lease_until: None,
            created_at: 1,
            last_seen_at: None,
        })?;
        store.remove_device(&key)?;
        store
            .db
            .locked()
            .execute("UPDATE personal_generation SET ever_claimed=1,tombstone=1,revision=revision+1", [])?;
        // Uma migração reaberta mudaria esta sentinela. Não é uma marca de posse.
        store.kv_set("schema", "5")?;
        std::fs::write(
            dir.join("identity.json"),
            serde_json::to_vec(
                &json!({"v":1,"sign":crypto::b64(h.sign.as_bytes()),"box":crypto::b64(h.box_secret.as_bytes())}),
            )?,
        )?;
        drop(store);
        let expected = personal_bootstrap::preflight(&path)?;
        let prepared = prepared_existing(&dir)?;
        Ok((dir, prepared, expected))
    }
    #[derive(Clone, Copy)]
    enum Swap {
        Remove,
        Rename,
        Virgin,
    }
    fn rejected_handoff(swap: Swap) -> Result<()> {
        let (dir, prepared, expected) = handoff_fixture()?;
        let path = dir.join("codync.db");
        let retained = dir.join("retained.db");
        let h = std::fs::read(dir.join("identity.json"))?;
        let original = std::fs::read(&path)?;
        let virgin_path = dir.join("virgin.db");
        let replacement = if matches!(swap, Swap::Virgin) {
            let virgin = Store::open(&virgin_path)?; // Outra DB realmente nova, já com birth.
            virgin.kv_set("schema", "5")?;
            drop(virgin);
            Some(std::fs::read(&virgin_path)?)
        } else {
            None
        };
        match swap {
            Swap::Remove => {
                std::fs::hard_link(&path, &retained)?;
                std::fs::remove_file(&path)?;
            }
            Swap::Rename | Swap::Virgin => std::fs::rename(&path, &retained)?,
        }
        if replacement.is_some() {
            std::fs::rename(&virgin_path, &path)?;
        }
        assert!(prepared.open_store().is_err());
        assert_eq!(std::fs::read(dir.join("identity.json"))?, h);
        assert_eq!(std::fs::read(&retained)?, original);
        assert!(personal_bootstrap::preflight(&retained)? == expected);
        if let Some(bytes) = replacement {
            assert_eq!(std::fs::read(&path)?, bytes);
            let replacement = personal_bootstrap::preflight(&path)?;
            assert!(replacement.tracked_birth && !replacement.personal && replacement.config.is_none());
            let c = rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            assert_eq!(c.query_row("SELECT v FROM kv WHERE k='schema'", [], |r| r.get::<_, String>(0))?, "5");
        } else {
            assert!(!path.exists());
        }
        let c = rusqlite::Connection::open_with_flags(&retained, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        assert_eq!(c.query_row("SELECT v FROM kv WHERE k='schema'", [], |r| r.get::<_, String>(0))?, "5");
        drop(c);
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }
    #[test]
    fn personal_preflight_rejects_removed_database_before_migration_or_identity_creation() -> Result<()> {
        rejected_handoff(Swap::Remove)
    }
    #[test]
    fn personal_preflight_rejects_renamed_database_before_migration_or_identity_creation() -> Result<()> {
        rejected_handoff(Swap::Rename)
    }
    #[test]
    fn personal_preflight_rejects_replacement_with_birth_stamped_virgin_database() -> Result<()> {
        rejected_handoff(Swap::Virgin)
    }
    #[test]
    fn personal_preflight_rejects_same_file_ledger_change_before_migrations() -> Result<()> {
        let (dir, prepared, _) = handoff_fixture()?;
        let path = dir.join("codync.db");
        let h = std::fs::read(dir.join("identity.json"))?;
        let writer = rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        let prior: u32 = writer.query_row("SELECT revision FROM personal_generation", [], |r| r.get(0))?;
        let changed_revision = prior.checked_add(1).context("fixture revision exhausted")?;
        writer.execute("UPDATE personal_generation SET revision=revision+1", [])?;
        drop(writer);
        let expected = personal_bootstrap::preflight(&path)?;
        assert!(prepared.open_store().is_err());
        assert!(personal_bootstrap::preflight(&path)? == expected);
        assert_eq!(std::fs::read(dir.join("identity.json"))?, h);
        let c = rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        assert_eq!(c.query_row("SELECT v FROM kv WHERE k='schema'", [], |r| r.get::<_, String>(0))?, "5");
        assert_eq!(
            c.query_row("SELECT ever_claimed,tombstone,revision FROM personal_generation", [], |r| Ok((
                r.get::<_, u32>(0)?,
                r.get::<_, u32>(1)?,
                r.get::<_, u32>(2)?
            )))?,
            (1, 1, changed_revision)
        );
        drop(c);
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }
    #[test]
    fn personal_preflight_same_connection_handoff_and_restart_preserve_claimed_history() -> Result<()> {
        let (dir, prepared, expected) = handoff_fixture()?;
        let h = std::fs::read(dir.join("identity.json"))?;
        let (store, identity, installation) = prepared.open_store()?;
        let hub = Hub::new(store, "fixture".into(), identity, "fixture-token".into(), 0);
        installation.install(&hub)?;
        assert!(personal_bootstrap::preflight(&dir.join("codync.db"))? == expected);
        assert_eq!(hub.store.db.locked().pragma_query_value(None, "recursive_triggers", |r| r.get::<_, i64>(0))?, 1);
        assert!(
            hub.store
                .bootstrap_claim_a1(None, "another-claim", |_| panic!("personal history must not sign A1"))
                .is_err()
        );
        drop(hub);
        let (store, identity, installation) = prepared_existing(&dir)?.open_store()?;
        let hub = Hub::new(store, "fixture".into(), identity, "fixture-token".into(), 0);
        installation.install(&hub)?;
        assert!(personal_bootstrap::preflight(&dir.join("codync.db"))? == expected);
        assert_eq!(std::fs::read(dir.join("identity.json"))?, h);
        drop(hub);
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }
    #[test]
    fn root_journal_requires_signed_seed_and_does_not_turn_file_into_consent() -> Result<()> {
        let (mut cfg, h) = fixture()?;
        let v: Value =
            serde_json::from_str(include_str!("../../../../docs/reference/fixtures/remote-relay-vectors.json"))?;
        let w = &v["personalBootstrap"]["wireVectors"];
        let at = w["packet"]["time"][0][4].as_u64().context("fixture epoch")?;
        cfg.installed.clock.observed_epoch_ms = at + 50;
        cfg.installed.clock.observed_boottime_ms = 150;
        cfg.installed.clock.valid_for_ms = 299_950;
        let req = json!([wire::domain("time-request"), cfg.context(), w["packet"]["time"][0][2], h.computer_id()]);
        let request = json!([req, crypto::b64(&h.sign(&serde_json::to_vec(&req)?))]);
        let j = JournalWire {
            schema: 1,
            profile: protocol::PROFILE.into(),
            wire: 1,
            context: cfg.context().clone(),
            identity: serde_json::from_value(serde_json::to_value(cfg.identity())?)?,
            revision: 1,
            clock_evidence: ClockEvidence {
                time_request: request,
                time: w["packet"]["time"].clone(),
                boot_id: cfg.installed.clock.boot_id.clone(),
                sent_boottime_ms: 100,
                received_boottime_ms: 150,
            },
            originals: None,
        };
        let bytes = serde_json::to_vec(&j)?;
        let journal = RootJournal::read(&cfg, &bytes)?;
        assert_eq!(journal.original_bytes(), bytes);
        assert_eq!(journal.revision(), 1);
        assert!(journal.retained(&cfg, &mut Clock::default(), Kind::LocalConsent, b"[]").is_err());
        let mut altered: Value = serde_json::from_slice(&bytes)?;
        altered["clock_evidence"]["receivedBoottimeMs"] = json!(151);
        assert!(RootJournal::read(&cfg, &serde_json::to_vec(&altered)?).is_err());
        let mut altered: Value = serde_json::from_slice(&bytes)?;
        altered["clock_evidence"]["time"][1] = json!(crypto::b64(&[0; 64]));
        assert!(RootJournal::read(&cfg, &serde_json::to_vec(&altered)?).is_err());
        Ok(())
    }
}
