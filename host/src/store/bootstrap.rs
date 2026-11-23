//! ACS4 durable state, sharing the device database and its write lock.
use super::{Device, DeviceSource, Scope, Store};
use crate::LockExt;
use crate::fork::bootstrap::clock;
use crate::fork::bootstrap::codec::{self, Activation, Challenge, Grant, Proof, Receipt, Revocation};
use crate::remote::crypto;
use anyhow::{Result, ensure};
use ed25519_dalek::SigningKey;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Only trusted provisioning code may construct and install this configuration.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedGeneration {
    pub generation: String,
    pub issuer_key_id: String,
    pub issuer_key: String,
    pub issuer: String,
    pub deployment_id: String,
    pub env: String,
    pub manifest_hash: String,
    pub resource_digest: String,
    pub computer_id: String,
    pub sign_key: String,
    pub box_key: String,
}

#[derive(Clone, Debug)]
pub struct CloudGrant {
    pub grant_id: String,
    pub device_key: String,
    pub owner_user_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Publication {
    pub bootstrap_id: String,
    #[serde(rename = "hubGeneration")]
    pub generation: String,
    pub grant_id: String,
    pub device_key: String,
    pub owner_user_id: String,
    #[serde(rename = "hashHubReceipt")]
    pub receipt_hash: String,
}

type ActiveSlotRow = (Vec<u8>, String, Option<Vec<u8>>, i64);
type ReceiptBundleRow = (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>);
type RevocationRow = (Option<String>, Option<String>, String, Option<Vec<u8>>);
type ActivationRow = (String, String, Option<Vec<u8>>);

pub(super) fn migrate(c: &Connection) -> Result<()> {
    c.execute_batch(
        "BEGIN;
        CREATE TABLE IF NOT EXISTS bootstrap_generation (
          singleton INTEGER PRIMARY KEY CHECK(singleton=1), config TEXT NOT NULL,
          ever_claimed INTEGER NOT NULL DEFAULT 0 CHECK(ever_claimed IN (0,1)),
          revision INTEGER NOT NULL DEFAULT 0, acl_version INTEGER NOT NULL DEFAULT 0,
          a1_input TEXT, a1_signature TEXT);
        CREATE TABLE IF NOT EXISTS bootstrap_slot (
          singleton INTEGER PRIMARY KEY CHECK(singleton=1), grant_wire BLOB NOT NULL,
          grant_hash TEXT NOT NULL, challenge_wire BLOB, status TEXT NOT NULL,
          pending TEXT, receipt_wire BLOB, receipt_hash TEXT);
        CREATE TRIGGER IF NOT EXISTS bootstrap_no_reopen BEFORE UPDATE OF ever_claimed ON bootstrap_generation
          WHEN OLD.ever_claimed=1 AND NEW.ever_claimed<>1 BEGIN SELECT RAISE(ABORT,'generation closed'); END;
        CREATE TRIGGER IF NOT EXISTS bootstrap_no_delete BEFORE DELETE ON bootstrap_generation
          BEGIN SELECT RAISE(ABORT,'generation immutable'); END;
        CREATE TRIGGER IF NOT EXISTS bootstrap_no_config BEFORE UPDATE OF config ON bootstrap_generation
          WHEN OLD.config<>NEW.config BEGIN SELECT RAISE(ABORT,'generation immutable'); END;
        CREATE TRIGGER IF NOT EXISTS bootstrap_acl_insert AFTER INSERT ON devices BEGIN
          UPDATE bootstrap_generation SET acl_version=acl_version+1 WHERE singleton=1; END;
        CREATE TRIGGER IF NOT EXISTS bootstrap_acl_delete AFTER DELETE ON devices BEGIN
          UPDATE bootstrap_generation SET acl_version=acl_version+1 WHERE singleton=1; END;
        COMMIT;",
    )?;
    // Upgrade limitado ao comparador ACL; guarda Rust reverte DDL em erro/commit falho.
    upgrade_acl_update(c)?;
    let columns: Vec<String> = c
        .prepare("PRAGMA table_info(bootstrap_slot)")?
        .query_map([], |r| r.get(1))?
        .collect::<rusqlite::Result<_>>()?;
    if !columns.iter().any(|v| v == "proof_wire") {
        c.execute("ALTER TABLE bootstrap_slot ADD COLUMN proof_wire BLOB", [])?;
    }
    if !columns.iter().any(|v| v == "revocation_wire") {
        c.execute("ALTER TABLE bootstrap_slot ADD COLUMN revocation_wire BLOB", [])?;
    }
    if !columns.iter().any(|v| v == "activation_wire") {
        c.execute("ALTER TABLE bootstrap_slot ADD COLUMN activation_wire BLOB", [])?;
    }
    let installed: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM bootstrap_generation)", [], |r| r.get(0))?;
    if installed {
        c.pragma_update(None, "synchronous", "FULL")?;
    }
    Ok(())
}

/// Não presume rollback implícito de `execute_batch`: Transaction desfaz o upgrade
/// em erro de instrução ou commit. Não altera a disciplina das demais migrações ACS4.
fn upgrade_acl_update(c: &Connection) -> Result<()> {
    let mut tx = c.unchecked_transaction()?;
    tx.set_drop_behavior(rusqlite::DropBehavior::Rollback);
    tx.execute_batch(
        "DROP TRIGGER IF EXISTS bootstrap_acl_update;
        CREATE TRIGGER bootstrap_acl_update AFTER UPDATE ON devices
          WHEN OLD.key IS NOT NEW.key OR OLD.source IS NOT NEW.source OR OLD.grant_id IS NOT NEW.grant_id
            OR CASE
              WHEN json_valid(OLD.scopes)=0 THEN OLD.scopes IS NOT NEW.scopes
              WHEN json_valid(NEW.scopes)=0 THEN OLD.scopes IS NOT NEW.scopes
              WHEN json_type(OLD.scopes)<>'array' THEN OLD.scopes IS NOT NEW.scopes
              WHEN json_type(NEW.scopes)<>'array' THEN OLD.scopes IS NOT NEW.scopes
              ELSE (SELECT json_group_array(value) FROM (SELECT value FROM json_each(OLD.scopes) ORDER BY value))
                IS NOT (SELECT json_group_array(value) FROM (SELECT value FROM json_each(NEW.scopes) ORDER BY value))
            END
            OR OLD.lease_until IS NOT NEW.lease_until BEGIN
          UPDATE bootstrap_generation SET acl_version=acl_version+1 WHERE singleton=1; END;",
    )?;
    #[cfg(test)]
    tests::oauth_upgrade_fault(&tx)?;
    tx.commit()?;
    Ok(())
}

fn config(c: &Connection) -> Result<Option<TrustedGeneration>> {
    let raw: Option<String> =
        c.query_row("SELECT config FROM bootstrap_generation WHERE singleton=1", [], |r| r.get(0)).optional()?;
    raw.map(|s| serde_json::from_str(&s).map_err(Into::into)).transpose()
}
fn match_grant(g: &Grant, cfg: &TrustedGeneration) -> Result<()> {
    ensure!(
        g.3 == cfg.resource_digest
            && g.4 == cfg.generation
            && g.5 == cfg.issuer_key_id
            && g.6 == cfg.issuer
            && g.7 == cfg.deployment_id
            && g.8 == cfg.env
            && g.9 == cfg.manifest_hash
            && g.14 == cfg.computer_id
            && g.15 == cfg.sign_key
            && g.16 == cfg.box_key,
        "grant/config mismatch"
    );
    Ok(())
}
fn decode_grant(wire: &[u8], cfg: &TrustedGeneration, now: i64) -> Result<Grant> {
    let key = codec::b64::<32>(&cfg.issuer_key)?;
    let g: Grant = codec::decode(wire, &key)?;
    g.valid_at(now)?;
    match_grant(&g, cfg)?;
    Ok(g)
}
fn sign_key_matches(key: &SigningKey, cfg: &TrustedGeneration) -> Result<()> {
    ensure!(crypto::b64(key.verifying_key().as_bytes()) == cfg.sign_key, "wrong host identity");
    Ok(())
}
fn revision(c: &Connection) -> Result<i64> {
    Ok(c.query_row(
        "UPDATE bootstrap_generation SET revision=revision+1 WHERE singleton=1 RETURNING revision",
        [],
        |r| r.get(0),
    )?)
}
fn acl(c: &Connection) -> Result<(i64, String)> {
    let version = c.query_row("SELECT acl_version FROM bootstrap_generation WHERE singleton=1", [], |r| r.get(0))?;
    let mut stmt = c.prepare("SELECT key, source, grant_id, scopes, lease_until FROM devices ORDER BY key")?;
    let rows = stmt
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
    let mut values = Vec::new();
    for (key, source, grant, raw_scopes, lease) in rows {
        let mut scopes: Vec<String> = serde_json::from_str(&raw_scopes)?;
        scopes.sort();
        values.push(json!([key, source, grant, scopes, lease]));
    }
    Ok((version, codec::hash_bytes(&serde_json::to_vec(&json!(["donna/active-acl-state/v1", values]))?)))
}

pub(crate) fn assert_engine_authority_tx(db: &Connection, key: &str, owner: &str, binding: &str) -> Result<()> {
    ensure!(
        engine_binding(db, key, owner, super::now_ms())?.as_deref() == Some(binding),
        "remote_setup_unavailable: vínculo de contas de IA encerrado."
    );
    Ok(())
}

pub(crate) fn engine_binding(db: &Connection, key: &str, owner: &str, now: i64) -> Result<Option<String>> {
    let Some(cfg) = config(db)? else { return Ok(None) };
    let row: Option<ActivationRow> = db.query_row(
            "SELECT pending,receipt_hash,activation_wire FROM bootstrap_slot WHERE singleton=1 AND status='activeDevice'",
            [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).optional()?;
    let Some((pending, receipt, Some(wire))) = row else { return Ok(None) };
    let p: serde_json::Value = serde_json::from_str(&pending)?;
    let a: Activation = codec::decode(&wire, &codec::b64::<32>(&cfg.sign_key)?)?;
    let device: Option<(String, Option<String>, String, Option<i64>)> = db
        .query_row("SELECT source,grant_id,scopes,lease_until FROM devices WHERE key=?1", [key], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .optional()?;
    let Some((source, grant, scopes, Some(lease))) = device else { return Ok(None) };
    let active_scopes: Vec<String> = serde_json::from_str(&scopes)?;
    if source != "account"
        || lease <= now
        || a.12 > now
        || a.13 <= now
        || a.1 != p[2]
        || a.2 != cfg.generation
        || a.2 != p[3]
        || Some(a.3.as_str()) != grant.as_deref()
        || a.3 != p[1]
        || a.4 != owner
        || a.4 != p[4]
        || a.5 != p[5]
        || a.6 != key
        || a.6 != p[6]
        || a.7 != cfg.computer_id
        || a.8 != receipt
        || a.14 != active_scopes
        || active_scopes != codec::CAPS
    {
        return Ok(None);
    }
    // The signed snapshot and current Device must both be live. Ordinary lease
    // renewal or unrelated ACL changes do not invalidate this same grant/receipt.
    Ok(Some(json!([a.2, a.3, a.4, a.6, a.8]).to_string()))
}

impl Store {
    /// Local read-only observation. This is not an attestation or a permission.
    /// It intentionally works after the initial clock expires, without reopening a slot.
    pub fn bootstrap_inspection(&self) -> Result<serde_json::Value> {
        let db = self.db.locked();
        let Some(cfg) = config(&db)? else { return Ok(json!({"schema":1,"configured":false})) };
        let claimed: i64 =
            db.query_row("SELECT ever_claimed FROM bootstrap_generation WHERE singleton=1", [], |r| r.get(0))?;
        let slot: Option<(Vec<u8>, String, String)> = db
            .query_row("SELECT grant_wire,grant_hash,status FROM bootstrap_slot WHERE singleton=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?;
        let (id, hash, state) = if let Some((wire, hash, state)) = slot {
            let grant: Grant = codec::decode(&wire, &codec::b64::<32>(&cfg.issuer_key)?)?;
            match_grant(&grant, &cfg)?;
            ensure!(codec::hash(&grant)? == hash, "stored bootstrap hash mismatch");
            (Some(grant.1), Some(hash), state)
        } else {
            (None, None, "empty".into())
        };
        let qualified_clock = clock::now().ok().and_then(|now| {
            now.checked_add(300_000)
                .and_then(|deadline| clock::remaining_until(deadline).ok())
                .map(|remaining| json!({"now":now,"remainingMs":remaining.as_millis()}))
        });
        Ok(json!({"schema":1,"configured":true,"generation":cfg.generation,
            "resourceDigest":cfg.resource_digest,"computerId":cfg.computer_id,"everClaimed":claimed != 0,
            "bootstrapId":id,"grantHash":hash,"state":state,"qualifiedClock":qualified_clock}))
    }

    /// Trusted clean-install provisioning only. Existing authorization fails closed.
    pub fn bootstrap_install_generation(&self, cfg: &TrustedGeneration) -> Result<()> {
        codec::b64::<32>(&cfg.generation)?;
        codec::b64::<32>(&cfg.issuer_key)?;
        codec::b64::<32>(&cfg.sign_key)?;
        codec::b64::<32>(&cfg.box_key)?;
        let mut db = self.db.locked();
        if let Some(installed) = config(&db)? {
            ensure!(serde_json::to_vec(&installed)? == serde_json::to_vec(cfg)?, "generation already installed");
            db.pragma_update(None, "synchronous", "FULL")?;
            return Ok(());
        }
        let count: i64 = db.query_row("SELECT COUNT(*) FROM devices", [], |r| r.get(0))?;
        ensure!(count == 0, "existing devices require separate migration");
        db.pragma_update(None, "synchronous", "FULL")?;
        let tx = db.transaction()?;
        tx.execute("INSERT INTO bootstrap_generation(singleton,config) VALUES(1,?1)", [serde_json::to_string(cfg)?])?;
        tx.commit()?;
        Ok(())
    }

    /// A1 signs under the same Connection/transaction, also without an ACS4 configuration.
    pub fn bootstrap_claim_a1(
        &self,
        generation: Option<&str>,
        input: &str,
        sign: impl FnOnce(&Connection) -> Result<String>,
    ) -> Result<Option<String>> {
        let mut db = self.db.locked();
        // HOST-R1-03: a primeira A1 (first_owner/ever_claimed) não pode ficar só no WAL/NORMAL antes do ACK assinado.
        db.pragma_update(None, "synchronous", "FULL")?;
        let tx = db.transaction()?;
        let Some(cfg) = config(&tx)? else {
            ensure!(generation.is_none(), "generation unavailable");
            let signature = super::personal_bootstrap::legacy_a1(&tx, input, sign)?;
            tx.commit()?;
            return Ok(Some(signature));
        };
        ensure!(generation == Some(cfg.generation.as_str()), "generation mismatch");
        let row: (i64, Option<String>, Option<String>) = tx.query_row(
            "SELECT ever_claimed,a1_input,a1_signature FROM bootstrap_generation WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        if row.0 != 0 {
            ensure!(row.1.as_deref() == Some(input), "generation already claimed");
            return Ok(Some(row.2.ok_or_else(|| anyhow::anyhow!("incomplete A1 reservation"))?));
        }
        let occupied: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM bootstrap_slot)", [], |r| r.get(0))?;
        ensure!(!occupied, "bootstrap slot occupied");
        ensure!(tx.execute("UPDATE bootstrap_generation SET ever_claimed=1,a1_input=?1,revision=revision+1 WHERE singleton=1 AND ever_claimed=0", [input])? == 1, "claim race");
        let signature = sign(&tx)?;
        tx.execute("UPDATE bootstrap_generation SET a1_signature=?1 WHERE singleton=1", [&signature])?;
        tx.commit()?;
        Ok(Some(signature))
    }

    /// Returns only an active, unclaimed slot authenticated by the installed root.
    pub fn bootstrap_active(&self, now: i64) -> Result<Option<Grant>> {
        let db = self.db.locked();
        let Some(cfg) = config(&db)? else { return Ok(None) };
        let row: Option<ActiveSlotRow> = db.query_row(
            "SELECT s.grant_wire,s.status,s.challenge_wire,g.ever_claimed FROM bootstrap_slot s CROSS JOIN bootstrap_generation g WHERE s.singleton=1 AND g.singleton=1", [],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        let Some((wire, status, challenge, claimed)) = row else { return Ok(None) };
        if status != "active" || claimed != 0 || challenge.is_some() {
            return Ok(None);
        }
        Ok(Some(decode_grant(&wire, &cfg, now)?))
    }

    /// Recheck active slot after a challenge has already been persisted.
    pub fn bootstrap_current(&self, now: i64) -> Result<Option<Grant>> {
        let db = self.db.locked();
        let Some(cfg) = config(&db)? else { return Ok(None) };
        let row: Option<(Vec<u8>, String, i64)> = db.query_row(
            "SELECT s.grant_wire,s.status,g.ever_claimed FROM bootstrap_slot s CROSS JOIN bootstrap_generation g WHERE s.singleton=1 AND g.singleton=1", [],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        let Some((wire, status, claimed)) = row else { return Ok(None) };
        if status != "active" || claimed != 0 {
            return Ok(None);
        }
        Ok(Some(decode_grant(&wire, &cfg, now)?))
    }

    pub fn bootstrap_install_grant(&self, wire: &[u8], now: i64) -> Result<()> {
        let mut db = self.db.locked();
        let tx = db.transaction()?;
        let cfg = config(&tx)?.ok_or_else(|| anyhow::anyhow!("generation unavailable"))?;
        let g = decode_grant(wire, &cfg, now)?;
        let claimed: i64 =
            tx.query_row("SELECT ever_claimed FROM bootstrap_generation WHERE singleton=1", [], |r| r.get(0))?;
        ensure!(claimed == 0, "generation closed");
        tx.execute(
            "INSERT INTO bootstrap_slot(singleton,grant_wire,grant_hash,status) VALUES(1,?1,?2,'active')",
            params![wire, codec::hash(&g)?],
        )?;
        revision(&tx)?;
        tx.commit()?;
        Ok(())
    }

    /// Persist the unique signed challenge before returning it. Expired or consumed slots never issue another.
    pub fn bootstrap_challenge(
        &self,
        key: &SigningKey,
        bootstrap_id: &str,
        device_key: &str,
        now: i64,
    ) -> Result<Vec<u8>> {
        let mut db = self.db.locked();
        let tx = db.transaction()?;
        let cfg = config(&tx)?.ok_or_else(|| anyhow::anyhow!("generation unavailable"))?;
        sign_key_matches(key, &cfg)?;
        let (grant, hash, existing, status): (Vec<u8>, String, Option<Vec<u8>>, String) = tx.query_row(
            "SELECT grant_wire,grant_hash,challenge_wire,status FROM bootstrap_slot WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        let g = decode_grant(&grant, &cfg, now)?;
        ensure!(
            status == "active" && g.1 == bootstrap_id && g.12 == device_key && existing.is_none(),
            "challenge unavailable"
        );
        let ch = Challenge(
            "donna/bootstrap-challenge/v1".into(),
            g.1,
            g.4,
            hash,
            g.12,
            g.13,
            crypto::b64(&crypto::random::<32>()),
            now,
            std::cmp::min(g.19, now.checked_add(60_000).ok_or_else(|| anyhow::anyhow!("clock overflow"))?),
        );
        let wire = codec::encode(&ch, |b| crypto::sign(key, b))?;
        ensure!(tx.execute("UPDATE bootstrap_slot SET challenge_wire=?1 WHERE singleton=1 AND status='active' AND challenge_wire IS NULL", [&wire])? == 1, "challenge race");
        revision(&tx)?;
        tx.commit()?;
        Ok(wire)
    }

    pub fn bootstrap_commit_proof(&self, key: &SigningKey, proof_wire: &[u8], now: i64) -> Result<Vec<u8>> {
        let mut db = self.db.locked();
        let tx = db.transaction()?;
        let cfg = config(&tx)?.ok_or_else(|| anyhow::anyhow!("generation unavailable"))?;
        sign_key_matches(key, &cfg)?;
        let (gw, gh, cw, status): (Vec<u8>, String, Option<Vec<u8>>, String) = tx.query_row(
            "SELECT grant_wire,grant_hash,challenge_wire,status FROM bootstrap_slot WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        ensure!(status == "active", "bootstrap closed");
        let g = decode_grant(&gw, &cfg, now)?;
        let challenge: Challenge =
            codec::decode(&cw.ok_or_else(|| anyhow::anyhow!("no challenge"))?, &codec::b64::<32>(&cfg.sign_key)?)?;
        ensure!(now >= challenge.7 && now < challenge.8 && challenge.8 <= g.19, "challenge expired");
        let ch = codec::hash(&challenge)?;
        let proof: Proof = codec::decode(proof_wire, &codec::b64::<32>(&g.12)?)?;
        ensure!(
            proof.1 == g.1
                && proof.2 == g.4
                && proof.3 == gh
                && proof.4 == ch
                && challenge.1 == g.1
                && challenge.2 == g.4
                && challenge.3 == gh
                && challenge.4 == g.12
                && challenge.5 == g.13,
            "proof mismatch"
        );
        ensure!(
            tx.execute("UPDATE bootstrap_generation SET ever_claimed=1 WHERE singleton=1 AND ever_claimed=0", [])? == 1,
            "generation closed"
        );
        let rev = revision(&tx)?;
        let grant_id = format!("grt_{}", crypto::b64(&crypto::random::<16>()));
        let ph = codec::hash(&proof)?;
        let pending = json!([
            "donna/bootstrap-pending/v1",
            grant_id,
            g.1,
            g.4,
            g.10,
            g.11,
            g.12,
            g.20,
            gh,
            ch,
            ph,
            now,
            rev,
            "pendingPublication"
        ]);
        let pending_hash = codec::hash_bytes(&serde_json::to_vec(&pending)?);
        let (acl_version, acl_hash) = acl(&tx)?;
        let rec = Receipt(
            "donna/bootstrap-receipt/v1".into(),
            g.1,
            g.4,
            gh,
            ch,
            ph,
            g.10,
            g.11,
            g.12,
            g.14,
            g.15,
            grant_id,
            g.20,
            now,
            rev,
            pending_hash,
            acl_version,
            acl_hash,
        );
        let wire = codec::encode(&rec, |b| crypto::sign(key, b))?;
        let receipt_hash = codec::hash(&rec)?;
        ensure!(tx.execute("UPDATE bootstrap_slot SET status='pendingPublication',pending=?1,receipt_wire=?2,receipt_hash=?3,proof_wire=?4 WHERE singleton=1 AND status='active'", params![pending.to_string(), wire, receipt_hash, proof_wire])? == 1, "proof race");
        tx.commit()?;
        Ok(wire)
    }

    /// The exact four signed envelopes persisted with the proof commit.
    pub fn bootstrap_receipt_envelopes(&self) -> Result<Option<serde_json::Value>> {
        let db = self.db.locked();
        let row: Option<ReceiptBundleRow> = db.query_row(
            "SELECT grant_wire,challenge_wire,proof_wire,receipt_wire FROM bootstrap_slot WHERE singleton=1 AND status IN ('pendingPublication','activeDevice') AND proof_wire IS NOT NULL AND receipt_wire IS NOT NULL", [],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        row.map(|(g,c,p,r)| Ok(json!({"grant":serde_json::from_slice::<serde_json::Value>(&g)?, "challenge":serde_json::from_slice::<serde_json::Value>(&c)?, "proof":serde_json::from_slice::<serde_json::Value>(&p)?, "receipt":serde_json::from_slice::<serde_json::Value>(&r)?}))).transpose()
    }

    /// For an exact fence, persist tombstone and the original signed revocation atomically.
    /// `on_tombstone` runs once, inside the same transaction, when the slot becomes a tombstone.
    pub fn bootstrap_revoke_fenced(
        &self,
        key: &SigningKey,
        grant_id: &str,
        device_key: &str,
        now: i64,
        on_tombstone: impl FnOnce(&Connection) -> Result<()>,
    ) -> Result<bool> {
        let mut db = self.db.locked();
        let tx = db.transaction()?;
        let Some(cfg) = config(&tx)? else { return Ok(false) };
        sign_key_matches(key, &cfg)?;
        let row: Option<RevocationRow> = tx
            .query_row(
                "SELECT pending,receipt_hash,status,revocation_wire FROM bootstrap_slot WHERE singleton=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let Some((Some(pending), Some(receipt_hash), status, existing)) = row else { return Ok(false) };
        let p: serde_json::Value = serde_json::from_str(&pending)?;
        if p[1].as_str() != Some(grant_id) {
            return Ok(false);
        }
        ensure!(p[6].as_str() == Some(device_key), "fence key mismatch");
        if status == "tombstone" {
            ensure!(existing.is_some(), "missing signed revocation");
            return Ok(true);
        }
        ensure!(status == "pendingPublication" || status == "activeDevice", "slot cannot be revoked");
        tx.execute("DELETE FROM devices WHERE key=?1 AND grant_id=?2", params![device_key, grant_id])?;
        tx.execute("DELETE FROM push_tickets WHERE device_key=?1", [device_key])?;
        tx.execute("DELETE FROM activity_tickets WHERE device_key=?1", [device_key])?;
        on_tombstone(&tx)?;
        let rev = revision(&tx)?;
        let (acl_version, acl_hash) = acl(&tx)?;
        let payload = Revocation(
            "donna/bootstrap-revocation/v1".into(),
            p[2].as_str().ok_or_else(|| anyhow::anyhow!("bad pending"))?.into(),
            cfg.generation,
            grant_id.into(),
            receipt_hash,
            rev,
            now,
            acl_version,
            acl_hash,
        );
        let wire = codec::encode(&payload, |b| crypto::sign(key, b))?;
        tx.execute("UPDATE bootstrap_slot SET status='tombstone',revocation_wire=?1 WHERE singleton=1", [wire])?;
        tx.commit()?;
        Ok(true)
    }
    pub fn bootstrap_revocation_envelope(&self) -> Result<Option<serde_json::Value>> {
        let db = self.db.locked();
        let wire: Option<Vec<u8>> = db
            .query_row("SELECT revocation_wire FROM bootstrap_slot WHERE singleton=1 AND status='tombstone'", [], |r| {
                r.get(0)
            })
            .optional()?
            .flatten();
        wire.map(|v| serde_json::from_slice(&v).map_err(Into::into)).transpose()
    }
    pub fn bootstrap_is_grant(&self, grant_id: &str) -> Result<bool> {
        let db = self.db.locked();
        let pending: Option<String> = db
            .query_row("SELECT pending FROM bootstrap_slot WHERE singleton=1", [], |r| r.get(0))
            .optional()?
            .flatten();
        Ok(match pending {
            Some(p) => serde_json::from_str::<serde_json::Value>(&p)?[1].as_str() == Some(grant_id),
            None => false,
        })
    }

    pub fn bootstrap_receipt(&self) -> Result<Option<Vec<u8>>> {
        let db = self.db.locked();
        Ok(db
            .query_row("SELECT receipt_wire FROM bootstrap_slot WHERE singleton=1", [], |r| r.get(0))
            .optional()?
            .flatten())
    }
    /// Permanently closes an uncommitted slot, including an expired grant.
    pub fn bootstrap_cancel_slot(&self, bootstrap_id: &str, generation: &str) -> Result<()> {
        let mut db = self.db.locked();
        let tx = db.transaction()?;
        let cfg = config(&tx)?.ok_or_else(|| anyhow::anyhow!("generation unavailable"))?;
        ensure!(cfg.generation == generation, "generation mismatch");
        let (wire, status): (Vec<u8>, String) =
            tx.query_row("SELECT grant_wire,status FROM bootstrap_slot WHERE singleton=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        let grant: Grant = codec::decode(&wire, &codec::b64::<32>(&cfg.issuer_key)?)?;
        match_grant(&grant, &cfg)?;
        ensure!(grant.1 == bootstrap_id, "slot mismatch");
        if status != "tombstone" {
            ensure!(status == "active", "committed slot requires grant tombstone");
            tx.execute("UPDATE bootstrap_slot SET status='tombstone' WHERE singleton=1", [])?;
            revision(&tx)?;
        }
        tx.commit()?;
        Ok(())
    }
    /// The original issuer deadline, only while publication is still unconfirmed.
    /// Reading it never renews the ceremony or consumes the durable receipt.
    pub fn bootstrap_pending_deadline(&self) -> Result<Option<i64>> {
        let db = self.db.locked();
        let wire: Option<Vec<u8>> = db
            .query_row(
                "SELECT grant_wire FROM bootstrap_slot WHERE singleton=1 AND status='pendingPublication'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let Some(wire) = wire else { return Ok(None) };
        let cfg = config(&db)?.ok_or_else(|| anyhow::anyhow!("generation unavailable"))?;
        let grant: Grant = codec::decode(&wire, &codec::b64::<32>(&cfg.issuer_key)?)?;
        Ok(Some(grant.19))
    }

    pub fn bootstrap_needs_publication(&self) -> Result<bool> {
        let db = self.db.locked();
        let status: Option<String> =
            db.query_row("SELECT status FROM bootstrap_slot WHERE singleton=1", [], |r| r.get(0)).optional()?;
        Ok(status.as_deref() == Some("pendingPublication"))
    }
    #[cfg(test)]
    /// Linearize result promotion against revocation and lease changes under the DB lock.
    pub(crate) fn bootstrap_engine_set(
        &self,
        key: &str,
        owner: &str,
        binding: &str,
        slot: &str,
        value: &str,
    ) -> Result<()> {
        let db = self.db.locked();
        ensure!(engine_binding(&db, key, owner, super::now_ms())?.as_deref() == Some(binding), "setup_authority_lost");
        db.execute("INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=?2", params![slot, value])?;
        Ok(())
    }
    /// Sign only a live authorization snapshot under the same DB lock. Retries
    /// reuse the persisted bytes; a new ACL/lease yields a newer snapshot.
    pub fn bootstrap_activation_envelope(&self, key: &SigningKey, now: i64) -> Result<Option<serde_json::Value>> {
        let mut db = self.db.locked();
        let tx = db.transaction()?;
        let Some(cfg) = config(&tx)? else {
            return Ok(None);
        };
        let row: Option<ActivationRow> = tx.query_row(
            "SELECT pending,receipt_hash,activation_wire FROM bootstrap_slot WHERE singleton=1 AND status='activeDevice'",
            [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).optional()?;
        let Some((pending, receipt_hash, previous)) = row else {
            return Ok(None);
        };
        sign_key_matches(key, &cfg)?;
        let p: serde_json::Value = serde_json::from_str(&pending)?;
        let device_key = p[6].as_str().ok_or_else(|| anyhow::anyhow!("missing pending device"))?;
        let device: Option<(String, Option<String>, String, Option<i64>)> = tx
            .query_row("SELECT source,grant_id,scopes,lease_until FROM devices WHERE key=?1", [device_key], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .optional()?;
        let Some((source, grant_id, scopes, Some(lease_until))) = device else {
            return Ok(None);
        };
        let active_scopes: Vec<String> = serde_json::from_str(&scopes)?;
        if source != "account"
            || grant_id.as_deref() != p[1].as_str()
            || lease_until <= now
            || active_scopes != codec::CAPS
        {
            return Ok(None);
        }
        ensure!(p[3].as_str() == Some(cfg.generation.as_str()), "activation generation changed");
        let store_revision =
            tx.query_row("SELECT revision FROM bootstrap_generation WHERE singleton=1", [], |r| r.get(0))?;
        let (acl_version, acl_hash) = acl(&tx)?;
        let activation = Activation(
            "donna/bootstrap-activation/v1".into(),
            p[2].as_str().ok_or_else(|| anyhow::anyhow!("missing bootstrap id"))?.into(),
            cfg.generation,
            grant_id.ok_or_else(|| anyhow::anyhow!("missing grant"))?,
            p[4].as_str().ok_or_else(|| anyhow::anyhow!("missing owner"))?.into(),
            p[5].as_str().ok_or_else(|| anyhow::anyhow!("missing device id"))?.into(),
            device_key.into(),
            cfg.computer_id,
            receipt_hash,
            store_revision,
            acl_version,
            acl_hash,
            now,
            lease_until,
            active_scopes,
        );
        let wire = if let Some(old_wire) = previous {
            let old: Activation = codec::decode(&old_wire, &key.verifying_key().to_bytes())?;
            ensure!(old.12 <= now, "activation clock moved backwards");
            let mut same_snapshot = activation.clone();
            same_snapshot.12 = old.12;
            // Compare fields only: a renewed lease at the old observedAt can
            // exceed the lease limit. This synthetic value is never signed or
            // persisted; decode validated old, and encode validates activation.
            if serde_json::to_vec(&old)? == serde_json::to_vec(&same_snapshot)? {
                old_wire
            } else {
                codec::encode(&activation, |raw| crypto::sign(key, raw))?
            }
        } else {
            codec::encode(&activation, |raw| crypto::sign(key, raw))?
        };
        tx.execute(
            "UPDATE bootstrap_slot SET activation_wire=?1 WHERE singleton=1 AND status='activeDevice'",
            [&wire],
        )?;
        tx.commit()?;
        Ok(Some(serde_json::from_slice(&wire)?))
    }
    /// Atomically recheck the deciding grant and write only its attenuated scopes.
    pub fn bootstrap_put_delegated(
        &self,
        decider_key: &str,
        decider_grant: &str,
        expected_scopes: &[Scope],
        device: &Device,
        now: i64,
    ) -> Result<()> {
        let mut db = self.db.locked();
        let tx = db.transaction()?;
        let row: (String, Option<String>, String, Option<i64>) =
            tx.query_row("SELECT source,grant_id,scopes,lease_until FROM devices WHERE key=?1", [decider_key], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?;
        let scopes: Vec<Scope> = serde_json::from_str(&row.2)?;
        let allowed: Vec<Scope> =
            scopes.iter().copied().filter(|s| *s != Scope::OwnerDevices && *s != Scope::Executor).collect();
        ensure!(
            row.0 == "account"
                && row.1.as_deref() == Some(decider_grant)
                && row.3.is_some_and(|t| t > now)
                && scopes.contains(&Scope::OwnerDevices)
                && !allowed.is_empty()
                && allowed == expected_scopes
                && device.source == DeviceSource::Account
                && device.scopes == allowed
                && device.grant_id.is_some(),
            "deciding grant changed"
        );
        tx.execute("INSERT INTO devices(key,name,platform,source,grant_id,scopes,lease_until,created_at) VALUES(?1,?2,?3,'account',?4,?5,?6,?7)",
            params![device.key,device.name,device.platform,device.grant_id,serde_json::to_string(&device.scopes)?,device.lease_until,device.created_at])?;
        tx.commit()?;
        Ok(())
    }

    /// Only the authenticated cloud pull may construct these exact inputs.
    pub fn bootstrap_promote(&self, publication: &Publication, grant: &CloudGrant, now: i64) -> Result<bool> {
        ensure!(
            grant.grant_id == publication.grant_id
                && grant.device_key == publication.device_key
                && grant.owner_user_id == publication.owner_user_id,
            "cloud confirmation required"
        );
        let lease_until =
            now.checked_add(crate::remote::cloud::LEASE_MS).ok_or_else(|| anyhow::anyhow!("clock overflow"))?;
        let mut db = self.db.locked();
        let tx = db.transaction()?;
        let cfg = config(&tx)?.ok_or_else(|| anyhow::anyhow!("generation unavailable"))?;
        let (pending, rh, status): (String, String, String) =
            tx.query_row("SELECT pending,receipt_hash,status FROM bootstrap_slot WHERE singleton=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?;
        let p: serde_json::Value = serde_json::from_str(&pending)?;
        ensure!(
            cfg.generation == publication.generation
                && p[1].as_str() == Some(publication.grant_id.as_str())
                && p[2].as_str() == Some(publication.bootstrap_id.as_str())
                && p[3].as_str() == Some(publication.generation.as_str())
                && p[4].as_str() == Some(publication.owner_user_id.as_str())
                && p[6].as_str() == Some(publication.device_key.as_str())
                && rh == publication.receipt_hash,
            "publication mismatch"
        );
        if status == "activeDevice" || status == "tombstone" {
            return Ok(false);
        }
        ensure!(status == "pendingPublication", "slot not pending");
        tx.execute("INSERT INTO devices(key,name,platform,source,grant_id,scopes,lease_until,created_at) VALUES(?1,'','', 'account',?2,?3,?4,?5)", params![publication.device_key,publication.grant_id,serde_json::to_string(&p[7])?,lease_until,now])?;
        ensure!(
            tx.execute(
                "UPDATE bootstrap_slot SET status='activeDevice' WHERE singleton=1 AND status='pendingPublication'",
                []
            )? == 1,
            "promotion race"
        );
        revision(&tx)?;
        tx.commit()?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::store::tests::temp_store;

    /// Workspace epoch and whether the fenced key is blocked, as the fence's transaction left them.
    fn workspace_state(s: &Store, key: &str, grant: &str) -> (Option<String>, bool) {
        let db = s.db.locked();
        let epoch =
            db.query_row("SELECT v FROM kv WHERE k='fork.workspace.epoch'", [], |r| r.get(0)).optional().unwrap();
        (epoch, crate::fork::workspace::bloqueado_on(&db, key, Some(grant)).unwrap())
    }

    fn fixture() -> (Store, SigningKey, SigningKey, SigningKey, TrustedGeneration, Grant) {
        let store = temp_store();
        let issuer = SigningKey::from_bytes(&[1; 32]);
        let host = SigningKey::from_bytes(&[2; 32]);
        let device = SigningKey::from_bytes(&[3; 32]);
        let cfg = TrustedGeneration {
            generation: crypto::b64(&[4; 32]),
            issuer_key_id: "issuer-1".into(),
            issuer_key: crypto::b64(issuer.verifying_key().as_bytes()),
            issuer: "donna".into(),
            deployment_id: "deployment".into(),
            env: "test".into(),
            manifest_hash: "a".repeat(64),
            resource_digest: "b".repeat(64),
            computer_id: "computer".into(),
            sign_key: crypto::b64(host.verifying_key().as_bytes()),
            box_key: crypto::b64(&[5; 32]),
        };
        let grant = Grant(
            "donna/bootstrap-grant/v1".into(),
            "boot".into(),
            "intent".into(),
            cfg.resource_digest.clone(),
            cfg.generation.clone(),
            cfg.issuer_key_id.clone(),
            cfg.issuer.clone(),
            cfg.deployment_id.clone(),
            cfg.env.clone(),
            cfg.manifest_hash.clone(),
            "owner".into(),
            "device".into(),
            crypto::b64(device.verifying_key().as_bytes()),
            crypto::b64(&[6; 32]),
            cfg.computer_id.clone(),
            cfg.sign_key.clone(),
            cfg.box_key.clone(),
            crypto::b64(&[7; 32]),
            1_000,
            200_000,
            vec!["control".into(), "owner.devices".into()],
        );
        (store, issuer, host, device, cfg, grant)
    }

    #[test]
    fn legacy_and_a1_reservation_exclude_bootstrap() {
        let (s, issuer, _, _, cfg, grant) = fixture();
        let legacy = Store::open(std::path::Path::new(":memory:")).unwrap();
        assert_eq!(
            legacy.bootstrap_claim_a1(None, "legacy", |_| Ok("legacy signature".into())).unwrap().as_deref(),
            Some("legacy signature")
        );
        assert!(legacy.bootstrap_install_generation(&cfg).is_err());
        s.bootstrap_install_generation(&cfg).unwrap();
        s.bootstrap_install_generation(&cfg).unwrap();
        assert_eq!(s.db.locked().pragma_query_value(None, "synchronous", |r| r.get::<_, i64>(0)).unwrap(), 2);
        assert_eq!(
            s.bootstrap_claim_a1(Some(&cfg.generation), "a1", |_| Ok("signature".into())).unwrap().as_deref(),
            Some("signature")
        );
        assert_eq!(
            s.bootstrap_claim_a1(Some(&cfg.generation), "a1", |_| panic!("replay must not sign")).unwrap().as_deref(),
            Some("signature")
        );
        assert!(s.bootstrap_claim_a1(Some(&cfg.generation), "different", |_| Ok("bad".into())).is_err());
        let wire = codec::encode(&grant, |b| crypto::sign(&issuer, b)).unwrap();
        assert!(s.bootstrap_install_grant(&wire, 2_000).is_err());
    }

    #[test]
    fn proof_is_pending_until_exact_publication_and_tombstone_blocks_promotion() {
        let (s, issuer, host, device, cfg, grant) = fixture();
        s.bootstrap_install_generation(&cfg).unwrap();
        let gw = codec::encode(&grant, |b| crypto::sign(&issuer, b)).unwrap();
        s.bootstrap_install_grant(&gw, 2_000).unwrap();
        let observed = s.bootstrap_inspection().unwrap();
        assert_eq!(observed["grantHash"], codec::hash(&grant).unwrap());
        assert_eq!(observed["state"], "active");
        assert!(s.bootstrap_claim_a1(Some(&cfg.generation), "a1", |_| Ok("bad".into())).is_err());
        assert!(s.bootstrap_challenge(&host, "boot", "wrong", 2_000).is_err());
        let cw = s.bootstrap_challenge(&host, "boot", &grant.12, 2_000).unwrap();
        assert!(s.bootstrap_challenge(&host, "boot", &grant.12, 2_000).is_err());
        let challenge: Challenge = codec::decode(&cw, &host.verifying_key().to_bytes()).unwrap();
        let mut proof = Proof(
            "donna/bootstrap-proof/v1".into(),
            grant.1.clone(),
            grant.4.clone(),
            codec::hash(&grant).unwrap(),
            codec::hash(&challenge).unwrap(),
        );
        proof.4 = "a".repeat(64);
        let wrong = codec::encode(&proof, |b| crypto::sign(&device, b)).unwrap();
        assert!(s.bootstrap_commit_proof(&host, &wrong, 3_000).is_err());
        proof.4 = codec::hash(&challenge).unwrap();
        let bad_key = codec::encode(&proof, |b| crypto::sign(&issuer, b)).unwrap();
        assert!(s.bootstrap_commit_proof(&host, &bad_key, 3_000).is_err());
        let pw = codec::encode(&proof, |b| crypto::sign(&device, b)).unwrap();
        let receipt_wire = s.bootstrap_commit_proof(&host, &pw, 3_000).unwrap();
        assert_eq!(s.bootstrap_inspection().unwrap()["state"], "pendingPublication");
        assert!(s.bootstrap_inspection().unwrap()["everClaimed"].as_bool().unwrap());
        assert_eq!(s.bootstrap_receipt().unwrap().unwrap(), receipt_wire);
        let originals = s.bootstrap_receipt_envelopes().unwrap().unwrap();
        assert_eq!(originals["proof"], serde_json::from_slice::<serde_json::Value>(&pw).unwrap());
        assert_eq!(originals["grant"], serde_json::from_slice::<serde_json::Value>(&gw).unwrap());
        assert_eq!(originals["challenge"], serde_json::from_slice::<serde_json::Value>(&cw).unwrap());
        assert_eq!(originals["receipt"], serde_json::from_slice::<serde_json::Value>(&receipt_wire).unwrap());
        assert!(s.bootstrap_commit_proof(&host, &pw, 3_000).is_err());
        assert!(s.devices().unwrap().is_empty());
        let receipt: Receipt = codec::decode(&receipt_wire, &host.verifying_key().to_bytes()).unwrap();
        let mut publication = Publication {
            bootstrap_id: receipt.1.clone(),
            generation: receipt.2.clone(),
            grant_id: receipt.11.clone(),
            device_key: receipt.8.clone(),
            owner_user_id: receipt.6.clone(),
            receipt_hash: codec::hash(&receipt).unwrap(),
        };
        let listed = CloudGrant {
            grant_id: publication.grant_id.clone(),
            device_key: publication.device_key.clone(),
            owner_user_id: publication.owner_user_id.clone(),
        };
        publication.receipt_hash = "f".repeat(64);
        assert!(s.bootstrap_promote(&publication, &listed, 4_000).is_err());
        publication.receipt_hash = codec::hash(&receipt).unwrap();
        let (grant_id, device_key) = (publication.grant_id.clone(), publication.device_key.clone());
        assert!(s.bootstrap_revoke_fenced(&host, &grant_id, &device_key, 4_000, |_| anyhow::bail!("fixture")).is_err());
        assert!(s.bootstrap_revocation_envelope().unwrap().is_none(), "a failing hook rolls the fence back");
        let mut tombstones = 0;
        assert!(
            s.bootstrap_revoke_fenced(&host, &grant_id, &device_key, 4_000, |tx| {
                tombstones += 1;
                crate::fork::workspace::bloquear_on(tx, &device_key, Some(&grant_id)).map(drop)
            })
            .unwrap()
        );
        let first = s.bootstrap_revocation_envelope().unwrap().unwrap();
        assert!(
            s.bootstrap_revoke_fenced(&host, &grant_id, &device_key, 5_000, |_| panic!("tombstone already written"))
                .unwrap()
        );
        assert_eq!(tombstones, 1);
        assert_eq!(workspace_state(&s, &device_key, &grant_id), (Some("1".into()), true));
        assert_eq!(s.bootstrap_revocation_envelope().unwrap().unwrap(), first);
        let _: Revocation =
            codec::decode(&serde_json::to_vec(&first).unwrap(), &host.verifying_key().to_bytes()).unwrap();
        assert!(s.bootstrap_receipt_envelopes().unwrap().is_none());
        assert!(!s.bootstrap_promote(&publication, &listed, 4_000).unwrap());
        assert!(s.devices().unwrap().is_empty());
    }
    #[test]
    fn matching_publication_promotes_with_lease() {
        let (s, issuer, host, device, cfg, grant) = fixture();
        s.bootstrap_install_generation(&cfg).unwrap();
        s.bootstrap_install_grant(&codec::encode(&grant, |b| crypto::sign(&issuer, b)).unwrap(), 2_000).unwrap();
        let cw = s.bootstrap_challenge(&host, "boot", &grant.12, 2_000).unwrap();
        let ch: Challenge = codec::decode(&cw, &host.verifying_key().to_bytes()).unwrap();
        let p = Proof(
            "donna/bootstrap-proof/v1".into(),
            grant.1.clone(),
            grant.4.clone(),
            codec::hash(&grant).unwrap(),
            codec::hash(&ch).unwrap(),
        );
        let rw =
            s.bootstrap_commit_proof(&host, &codec::encode(&p, |b| crypto::sign(&device, b)).unwrap(), 3_000).unwrap();
        let receipt: Receipt = codec::decode(&rw, &host.verifying_key().to_bytes()).unwrap();
        let publication = Publication {
            bootstrap_id: receipt.1.clone(),
            generation: receipt.2.clone(),
            grant_id: receipt.11.clone(),
            device_key: receipt.8.clone(),
            owner_user_id: receipt.6.clone(),
            receipt_hash: codec::hash(&receipt).unwrap(),
        };
        let listed = CloudGrant {
            grant_id: publication.grant_id.clone(),
            device_key: publication.device_key.clone(),
            owner_user_id: publication.owner_user_id.clone(),
        };
        assert!(s.bootstrap_activation_envelope(&host, 4_000).unwrap().is_none());
        s.bootstrap_promote(&publication, &listed, 4_000).unwrap();
        let active_wire = s.bootstrap_activation_envelope(&host, 4_001).unwrap().unwrap();
        let active: Activation =
            codec::decode(&serde_json::to_vec(&active_wire).unwrap(), &host.verifying_key().to_bytes()).unwrap();
        assert_eq!(active.3, publication.grant_id);
        assert_eq!(active.8, publication.receipt_hash);
        assert_eq!(active.13, 4_000 + crate::remote::cloud::LEASE_MS);
        assert!(active.9 > receipt.14 && active.10 > receipt.16);
        assert_eq!(s.bootstrap_activation_envelope(&host, 4_002).unwrap().unwrap(), active_wire);
        let health =
            crate::fork::bootstrap::health::test_snapshot(&s, &host, &cfg.generation, &"a".repeat(64), 4_002, true)
                .unwrap();
        assert_eq!(health[0][0], "donna/control-hub-health/v1");
        assert_eq!(health[0][7], "healthy");
        crypto::verify(
            &host.verifying_key().to_bytes(),
            &serde_json::to_vec(&health[0]).unwrap(),
            &crypto::unb64_n::<64>(health[1].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            crate::fork::bootstrap::health::test_snapshot(&s, &host, &cfg.generation, &"b".repeat(64), 4_002, true)
                .unwrap()[0][7],
            "failed"
        );
        assert_eq!(
            crate::fork::bootstrap::health::test_snapshot(&s, &host, &cfg.generation, &"a".repeat(64), 4_002, false)
                .unwrap()[0][7],
            "failed"
        );
        assert!(
            crate::fork::bootstrap::health::test_snapshot(
                &s,
                &host,
                &crypto::b64(&[8; 32]),
                &"a".repeat(64),
                4_002,
                true
            )
            .is_err()
        );
        assert!(s.bootstrap_activation_envelope(&issuer, 4_002).is_err());
        let device = s.device(&publication.device_key).unwrap();
        assert_eq!(device.lease_until, Some(4_000 + crate::remote::cloud::LEASE_MS));
        assert_eq!(device.scopes, vec![Scope::Control, Scope::OwnerDevices]);
        assert!(!s.bootstrap_promote(&publication, &listed, 4_000).unwrap());
        // A normal cloud pull renews the actual Device lease after activation.
        // Comparing snapshots must not validate this new lease at the old observedAt.
        s.set_lease(&publication.device_key, 4_500 + crate::remote::cloud::LEASE_MS).unwrap();
        let renewed_wire = s.bootstrap_activation_envelope(&host, 4_501).unwrap().unwrap();
        let renewed: Activation =
            codec::decode(&serde_json::to_vec(&renewed_wire).unwrap(), &host.verifying_key().to_bytes()).unwrap();
        assert_eq!(renewed.12, 4_501);
        assert_eq!(renewed.13, 4_500 + crate::remote::cloud::LEASE_MS);
        assert_eq!(renewed.3, active.3);
        assert_eq!(renewed.8, active.8);
        assert_ne!(renewed_wire, active_wire);
        assert_eq!(s.bootstrap_activation_envelope(&host, 4_502).unwrap().unwrap(), renewed_wire);
        assert!(s.bootstrap_activation_envelope(&host, 4_500).is_err());
        assert!(s.device(&publication.device_key).is_some());
        s.bootstrap_revoke_fenced(&host, &publication.grant_id, &publication.device_key, 5_000, |tx| {
            crate::fork::workspace::bloquear_on(tx, &publication.device_key, Some(&publication.grant_id)).map(drop)
        })
        .unwrap();
        assert!(s.device(&publication.device_key).is_none());
        assert_eq!(workspace_state(&s, &publication.device_key, &publication.grant_id), (Some("1".into()), true));
        assert!(s.bootstrap_activation_envelope(&host, 5_001).unwrap().is_none());
        assert_eq!(
            crate::fork::bootstrap::health::test_snapshot(&s, &host, &cfg.generation, &"a".repeat(64), 5_001, true)
                .unwrap()[0][7],
            "failed"
        );
    }

    #[test]
    fn delegated_insert_rechecks_live_grant_in_same_transaction() {
        let (s, _, _, _, _, _) = fixture();
        let owner = Device {
            key: "owner-key".into(),
            name: String::new(),
            platform: "test".into(),
            source: DeviceSource::Account,
            grant_id: Some("grt_owner".into()),
            scopes: vec![Scope::Control, Scope::OwnerDevices],
            lease_until: Some(10_000),
            created_at: 1,
            last_seen_at: None,
        };
        s.put_device(&owner).unwrap();
        let next = Device {
            key: "next".into(),
            grant_id: Some("grt_next".into()),
            scopes: vec![Scope::Control],
            ..owner.clone()
        };
        assert!(s.bootstrap_put_delegated("owner-key", "grt_other", &[Scope::Control], &next, 2_000).is_err());
        let widened = Device { scopes: vec![Scope::Control, Scope::Screen], ..next.clone() };
        assert!(s.bootstrap_put_delegated("owner-key", "grt_owner", &[Scope::Control], &widened, 2_000).is_err());
        assert!(s.device("next").is_none());
        s.bootstrap_put_delegated("owner-key", "grt_owner", &[Scope::Control], &next, 2_000).unwrap();
        assert_eq!(s.device("next").unwrap().scopes, vec![Scope::Control]);
    }

    // D70 do encaixe de época OAuth, sem ampliar os emissores/autoridade ACS4.
    include!("../fork/conectores/oauth_bootstrap_tests.rs");
}
