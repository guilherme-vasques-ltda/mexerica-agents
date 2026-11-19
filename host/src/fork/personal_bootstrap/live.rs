//! Snapshot/promoção pessoal: fatos duráveis e CAS na MESMA Connection de posse.
use super::{
    clock::{Clock, Lease},
    protocol,
    provisioning::AdmittedConfig,
    wire,
};
use crate::{LockExt, hub::Hub};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    pub(super) capture: String,
    pub(super) context: Value,
    pub(super) h_t: String,
    pub(super) computer: String,
    pub(super) generation: String,
    pub(super) sign: String,
    pub(super) box_key: String,
    pub(super) owner: String,
    pub(super) device: String,
    pub(super) app: String,
    pub(super) grant: String,
    pub(super) h_r: String,
}
impl Binding {
    pub(super) fn from_ceremony(capture: &str, ceremony: &wire::Ceremony) -> Result<Self> {
        let t = ceremony.tuple();
        let string = |i: usize| -> Result<String> { Ok(t[i].as_str().context("personal binding field")?.into()) };
        let receipt = ceremony.original(super::shapes::Kind::Receipt)?;
        Ok(Self {
            capture: capture.into(),
            context: t[0].clone(),
            h_t: ceremony.digest().into(),
            computer: string(11)?,
            generation: string(10)?,
            sign: string(12)?,
            box_key: string(13)?,
            owner: string(5)?,
            device: string(6)?,
            app: string(7)?,
            grant: receipt.payload()[6].as_str().context("receipt grant")?.into(),
            h_r: receipt.digest().into(),
        })
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    pub(super) binding: Binding,
    pub(super) sequence: i64,
    pub(super) root_revision: u64,
    pub(super) root_hash: String,
    pub(super) originals: Value,
    pub(super) request: Vec<u8>,
    pub(super) snapshot: Vec<u8>,
    pub(super) lease: Vec<u8>,
    pub(super) activation: Vec<u8>,
}
pub(super) struct Draft {
    pub(super) record: Record,
    pub(super) base_revision: u64,
    pub(super) base_acl: u64,
    pub(super) next_revision: u64,
    pub(super) next_acl: u64,
    pub(super) next_hash: String,
}
pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS personal_live(capture TEXT NOT NULL,sequence INTEGER NOT NULL CHECK(sequence>0),record BLOB NOT NULL,PRIMARY KEY(capture,sequence));
      CREATE TRIGGER IF NOT EXISTS personal_live_keep BEFORE DELETE ON personal_live BEGIN SELECT RAISE(ABORT,'personal live history retained'); END;
      CREATE TRIGGER IF NOT EXISTS personal_live_fixed BEFORE UPDATE ON personal_live BEGIN SELECT RAISE(ABORT,'personal live history immutable'); END;
      CREATE TABLE IF NOT EXISTS personal_live_current(singleton INTEGER PRIMARY KEY CHECK(singleton=1),capture TEXT NOT NULL,sequence INTEGER NOT NULL,accepted INTEGER NOT NULL CHECK(accepted IN(0,1)));
      CREATE TABLE IF NOT EXISTS personal_activation_sends(capture TEXT NOT NULL,sequence INTEGER NOT NULL,activation BLOB NOT NULL,PRIMARY KEY(capture,sequence));
      CREATE TRIGGER IF NOT EXISTS personal_activation_send_fixed BEFORE UPDATE ON personal_activation_sends BEGIN SELECT RAISE(ABORT,'personal activation send immutable'); END;
      CREATE TRIGGER IF NOT EXISTS personal_activation_send_keep BEFORE DELETE ON personal_activation_sends BEGIN SELECT RAISE(ABORT,'personal activation send retained'); END;
      CREATE TABLE IF NOT EXISTS personal_effects(capture TEXT NOT NULL,kind TEXT NOT NULL,original BLOB NOT NULL,PRIMARY KEY(capture,kind));
      CREATE TRIGGER IF NOT EXISTS personal_effect_fixed BEFORE UPDATE ON personal_effects BEGIN SELECT RAISE(ABORT,'personal effect immutable'); END;
      CREATE TRIGGER IF NOT EXISTS personal_effect_keep BEFORE DELETE ON personal_effects BEGIN SELECT RAISE(ABORT,'personal effect retained'); END;
      CREATE TABLE IF NOT EXISTS personal_fence_history(capture TEXT NOT NULL,revision INTEGER NOT NULL,w BLOB NOT NULL,y BLOB NOT NULL,x BLOB,PRIMARY KEY(capture,revision));
      CREATE TRIGGER IF NOT EXISTS personal_fence_history_fixed BEFORE UPDATE ON personal_fence_history BEGIN SELECT RAISE(ABORT,'personal fence history immutable'); END;
      CREATE TRIGGER IF NOT EXISTS personal_fence_history_keep BEFORE DELETE ON personal_fence_history BEGIN SELECT RAISE(ABORT,'personal fence history retained'); END;
      CREATE TABLE IF NOT EXISTS personal_fence_latest(capture TEXT PRIMARY KEY,revision INTEGER NOT NULL);
      CREATE TRIGGER IF NOT EXISTS personal_fence_latest_forward BEFORE UPDATE ON personal_fence_latest WHEN NEW.capture<>OLD.capture OR NEW.revision<=OLD.revision BEGIN SELECT RAISE(ABORT,'personal fence latest regressed'); END;
      CREATE TRIGGER IF NOT EXISTS personal_fence_latest_keep BEFORE DELETE ON personal_fence_latest BEGIN SELECT RAISE(ABORT,'personal fence latest retained'); END;
      CREATE TABLE IF NOT EXISTS personal_fence_confirmed(capture TEXT NOT NULL,revision INTEGER NOT NULL,evidence BLOB NOT NULL,PRIMARY KEY(capture,revision));
      CREATE TRIGGER IF NOT EXISTS personal_fence_confirmed_fixed BEFORE UPDATE ON personal_fence_confirmed BEGIN SELECT RAISE(ABORT,'personal fence confirmation immutable'); END;
      CREATE TRIGGER IF NOT EXISTS personal_fence_confirmed_keep BEFORE DELETE ON personal_fence_confirmed BEGIN SELECT RAISE(ABORT,'personal fence confirmation retained'); END;")?;
    Ok(())
}
pub(super) fn effect(db: &Connection, capture: &str, kind: &str, bytes: &[u8]) -> Result<bool> {
    ensure!(matches!(kind, "receipt"), "personal effect kind");
    if let Some(old) = db
        .query_row("SELECT original FROM personal_effects WHERE capture=? AND kind=?", params![capture, kind], |r| {
            r.get::<_, Vec<u8>>(0)
        })
        .optional()?
    {
        ensure!(old == bytes, "personal effect original conflict");
        return Ok(false);
    }
    db.execute("INSERT INTO personal_effects VALUES(?,?,?)", params![capture, kind, bytes])?;
    Ok(true)
}
pub(super) fn session(db: &Connection, capture: &str, cfg: &AdmittedConfig) -> Result<Value> {
    let (config, raw): (Vec<u8>, String) =
        db.query_row("SELECT config,originals FROM personal_sessions WHERE id=?", [capture], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
    ensure!(config == cfg.original_bytes(), "personal live configuration changed");
    Ok(serde_json::from_str(&raw)?)
}
pub(super) fn current(db: &Connection) -> Result<Option<(Record, bool)>> {
    let row: Option<(Vec<u8>,bool)> = db.query_row("SELECT l.record,c.accepted FROM personal_live_current c JOIN personal_live l ON l.capture=c.capture AND l.sequence=c.sequence WHERE c.singleton=1",[],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    row.map(|(bytes, accepted)| {
        ensure!(bytes.len() <= 512 * 1024, "personal record too large");
        Ok((serde_json::from_slice(&bytes)?, accepted))
    })
    .transpose()
}
pub(super) fn acl(db: &Connection, replacement: Option<(&str, &str, u64)>) -> Result<(u64, u64, String)> {
    let revision =
        u64::try_from(
            db.query_row("SELECT revision FROM personal_generation WHERE singleton=1", [], |r| r.get::<_, i64>(0))?,
        )?;
    let version =
        u64::try_from(db.query_row("SELECT version FROM personal_acl WHERE singleton=1", [], |r| r.get::<_, i64>(0))?)?;
    let mut query = db.prepare("SELECT key,source,grant_id,scopes,lease_until FROM devices ORDER BY key")?;
    let rows = query
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<i64>>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut values = std::collections::BTreeMap::new();
    for (key, source, grant, scopes, lease) in rows {
        let mut scopes: Vec<String> = serde_json::from_str(&scopes)?;
        scopes.sort();
        values.insert(key.clone(), json!([key, source, grant, scopes, lease]));
    }
    if let Some((key, grant, until)) = replacement {
        values.insert(key.into(), json!([key, "account", grant, ["control", "owner.devices"], until]));
    }
    let values: Vec<_> = values.into_values().collect();
    Ok((revision, version, wire::hash(&json!(["donna/active-acl-state/v1", values]))?))
}
fn basis(db: &Connection, cfg: &AdmittedConfig, binding: &Binding) -> Result<()> {
    ensure!(db.query_row("SELECT EXISTS(SELECT 1 FROM personal_generation p JOIN first_owner f ON f.singleton=p.singleton JOIN personal_possession_slot s ON s.singleton=p.singleton WHERE p.config=? AND p.generation=? AND p.computer=? AND p.sign_key=? AND p.box_key=? AND p.ever_claimed=1 AND p.tombstone=0 AND f.origin='personal' AND f.ever_claimed=1 AND s.capture=?)",params![cfg.original_bytes(),binding.generation,binding.computer,binding.sign,binding.box_key,binding.capture],|r|r.get::<_,bool>(0))?,"personal ownership closed/changed");
    ensure!(
        !db.query_row("SELECT EXISTS(SELECT 1 FROM personal_fence_latest WHERE capture=?)", [&binding.capture], |r| r
            .get::<_, bool>(0))?,
        "personal fence applied"
    );
    ensure!(
        db.query_row(
            "SELECT EXISTS(SELECT 1 FROM personal_device_origins WHERE key=? AND generation=? AND grant_id=?)",
            params![binding.app, binding.generation, binding.grant],
            |r| r.get::<_, bool>(0)
        )?,
        "personal origin changed"
    );
    if let Some((source, grant)) = db
        .query_row("SELECT source,grant_id FROM devices WHERE key=?", [&binding.app], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })
        .optional()?
    {
        ensure!(source == "account" && grant.as_deref() == Some(binding.grant.as_str()), "personal Device conflict");
    }
    Ok(())
}
/// Plano SELECT; verificação criptográfica após liberar DB, sem Cloud/runtime retidos.
pub(super) fn plan(
    hub: &Hub,
    cfg: &AdmittedConfig,
    capture: &str,
    root_revision: u64,
    root_hash: String,
    lease: &mut Lease,
    now: u64,
) -> Result<Draft> {
    lease.current()?;
    let snapshot = protocol::canonical(lease.snapshot())?;
    let request = protocol::canonical(lease.request_bytes())?;
    let (mut originals, revision, version, hash, sequence) = {
        let db = hub.store.db.locked();
        let originals = super::ceremony::packet(&session(&db, capture, cfg)?);
        let app = originals["intent"][0][6].as_str().context("A")?;
        let grant = originals["receipt"][0][6].as_str().context("R grant")?;
        let until = snapshot[0][17].as_u64().context("E expiry")?;
        let (revision, version, hash) = acl(&db, Some((app, grant, until)))?;
        let sequence =
            db.query_row("SELECT COALESCE(MAX(sequence),0)+1 FROM personal_live WHERE capture=?", [capture], |r| {
                r.get::<_, i64>(0)
            })?;
        (originals, revision, version, hash, sequence)
    };
    originals["host-state-request"] = request;
    originals["host-state"] = snapshot;
    let c = wire::ceremony(
        &cfg.verifier(originals["intent"][0][6].as_str().context("A")?)?,
        &serde_json::to_vec(&originals)?,
        wire::Time::Historical(now),
    )?;
    let binding = Binding::from_ceremony(capture, &c)?;
    let e = c.original(super::shapes::Kind::HostState)?.payload();
    ensure!(
        e[7] == binding.owner
            && e[8] == binding.device
            && e[9] == binding.app
            && e[6] == c.original(super::shapes::Kind::Publish)?.digest()
            && now < e[17].as_u64().context("E expiry")?,
        "personal positive snapshot mismatch/expiry"
    );
    lease.current()?;
    Ok(Draft {
        record: Record {
            binding,
            sequence,
            root_revision,
            root_hash,
            originals,
            request: lease.request_bytes().to_vec(),
            snapshot: lease.snapshot().to_vec(),
            lease: lease.bytes()?,
            activation: Vec::new(),
        },
        base_revision: revision,
        base_acl: version,
        next_revision: revision.checked_add(1).context("revision overflow")?,
        next_acl: version.checked_add(1).context("ACL overflow")?,
        next_hash: hash,
    })
}
pub(super) struct Certified {
    draft: Draft,
    record: Record,
}
impl Certified {
    pub(super) fn record_for_root(&self) -> &Record {
        &self.record
    }
    pub(super) fn binding(&self) -> &Binding {
        &self.record.binding
    }
}
pub(super) fn certify(cfg: &AdmittedConfig, draft: Draft, activation: &[u8], now: u64) -> Result<Certified> {
    let mut record = draft.record.clone();
    record.activation = activation.to_vec();
    record.originals["activation"] = protocol::canonical(activation)?;
    let c = wire::ceremony(
        &cfg.verifier(&record.binding.app)?,
        &serde_json::to_vec(&record.originals)?,
        wire::Time::Historical(now),
    )?;
    let v = c.original(super::shapes::Kind::Activation)?.payload();
    ensure!(
        v[10] == draft.next_revision
            && v[11] == draft.next_acl
            && v[12] == draft.next_hash
            && now < v[14].as_u64().context("V expiry")?,
        "personal V draft mismatch/expiry"
    );
    Ok(Certified { draft, record })
}
/// Consumidor apenas de draft certificado privado, na ordem Cloud → runtime → Connection.
pub(super) fn commit(
    db: &Connection,
    cfg: &AdmittedConfig,
    certified: &Certified,
    lease: &mut Lease,
    clock: &mut Clock,
) -> Result<Record> {
    let draft = &certified.draft;
    let mut record = certified.record.clone();
    db.pragma_update(None, "synchronous", "FULL")?;
    let tx = db.unchecked_transaction()?;
    basis(&tx, cfg, &record.binding)?;
    let current_acl = acl(&tx, None)?;
    ensure!(current_acl.0 == draft.base_revision && current_acl.1 == draft.base_acl, "personal promotion CAS");
    let originals = session(&tx, &record.binding.capture, cfg)?;
    for name in ["receipt", "publish", "grant", "local-ticket", "intent"] {
        ensure!(originals.get(name) == record.originals.get(name), "personal originals changed before promotion");
    }
    lease.current()?;
    let until = lease.until()?;
    ensure!(clock.validity()? < until, "snapshot expired before write");
    tx.execute("INSERT INTO devices(key,name,platform,source,grant_id,scopes,lease_until,created_at) VALUES(?,?,'desktop','account',?,'[\"control\",\"owner.devices\"]',?,?) ON CONFLICT(key) DO UPDATE SET source='account',grant_id=excluded.grant_id,scopes=excluded.scopes,lease_until=excluded.lease_until",params![record.binding.app,"App do dono",record.binding.grant,i64::try_from(until)?,i64::try_from(lease.observed()?)?])?;
    let actual = acl(&tx, None)?;
    ensure!(
        actual == (draft.next_revision, draft.next_acl, draft.next_hash.clone()),
        "personal ACL changed after triggers"
    );
    record.lease = lease.bytes()?;
    let bytes = serde_json::to_vec(&record)?;
    ensure!(bytes.len() <= 512 * 1024, "personal record too large");
    tx.execute("INSERT INTO personal_live VALUES(?,?,?)", params![record.binding.capture, record.sequence, bytes])?;
    tx.execute("INSERT INTO personal_live_current VALUES(1,?,?,0) ON CONFLICT(singleton) DO UPDATE SET capture=excluded.capture,sequence=excluded.sequence,accepted=0",params![record.binding.capture,record.sequence])?;
    lease.current()?;
    ensure!(clock.validity()? < until, "snapshot expired before commit");
    tx.commit()?;
    Ok(record)
}
pub(super) fn guard(
    db: &Connection,
    cfg: &AdmittedConfig,
    record: &Record,
    lease: &mut Lease,
    clock: &mut Clock,
    accepted: bool,
) -> Result<()> {
    basis(db, cfg, &record.binding)?;
    let (saved, is_accepted) = current(db)?.context("no personal snapshot applied")?;
    ensure!(
        saved.binding == record.binding
            && saved.activation == record.activation
            && saved.sequence == record.sequence
            && (!accepted || is_accepted),
        "personal activation changed"
    );
    let activation = protocol::canonical(&record.activation)?;
    let actual = acl(db, None)?;
    ensure!(
        activation[0][10] == actual.0 && activation[0][11] == actual.1 && activation[0][12] == actual.2,
        "personal live ACL changed"
    );
    ensure!(db.query_row("SELECT EXISTS(SELECT 1 FROM devices WHERE key=? AND source='account' AND grant_id=? AND scopes='[\"control\",\"owner.devices\"]')",params![record.binding.app,record.binding.grant],|r|r.get::<_,bool>(0))?,"personal device gone");
    lease.current()?;
    ensure!(clock.validity()? < activation[0][14].as_u64().context("V expiry")?, "personal V expired");
    Ok(())
}
