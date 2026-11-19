//! Sessão privada do consumidor PL1. Uma Connection, FULL, originais imutáveis.
use super::Store;
use crate::{LockExt, fork::personal_bootstrap::provisioning::AdmittedConfig};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

fn checked_revision(value: i64) -> Result<u64> {
    let revision = u64::try_from(value).context("negative personal revision")?;
    ensure!(revision > 0 && revision <= 9_007_199_254_740_991, "invalid personal revision");
    Ok(revision)
}

pub(crate) struct Session<'a> {
    pub(crate) db: &'a Connection,
    id: &'a str,
    originals: Value,
    revision: u64,
}
impl Session<'_> {
    pub(crate) fn id(&self) -> &str {
        self.id
    }
    pub(crate) fn private(&mut self, name: &str, value: Value) -> Result<()> {
        ensure!(
            matches!(
                name,
                "closure"
                    | "begin"
                    | "lastPoll"
                    | "sentLocalConsent"
                    | "sentBoxProof"
                    | "sentLocalConfirmation"
                    | "sentConsume"
                    | "sentBegin"
                    | "pending"
            ),
            "invalid private session field"
        );
        let key = format!("_{name}");
        if name != "lastPoll"
            && let Some(old) = self.originals.get(&key)
        {
            ensure!(*old == value, "private original changed");
            return Ok(());
        }
        self.originals.as_object_mut().context("invalid session")?.insert(key, value);
        Ok(())
    }
    pub(crate) fn originals(&self) -> &Value {
        &self.originals
    }
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }
    pub(crate) fn freeze(&mut self, name: &str, value: Value) -> Result<()> {
        ensure!(
            matches!(
                name,
                "intent"
                    | "local-ticket"
                    | "app-consent"
                    | "local-consent"
                    | "box-challenge"
                    | "box-proof"
                    | "approve"
                    | "local-confirm"
                    | "consume"
                    | "grant"
                    | "challenge"
                    | "proof"
                    | "receipt"
                    | "publish"
                    | "revocation"
                    | "fence"
                    | "fence-ack"
            ),
            "invalid personal original"
        );
        if let Some(old) = self.originals.get(name) {
            ensure!(*old == value, "personal original changed");
            return Ok(());
        }
        ensure!(!self.fenced() && !self.closed(), "personal session fenced");
        self.originals.as_object_mut().context("personal originals corrupted")?.insert(name.into(), value);
        Ok(())
    }
    pub(crate) fn closed(&self) -> bool {
        self.originals.get("_closure").is_some()
    }
    pub(crate) fn fenced(&self) -> bool {
        self.originals.get("fence").is_some()
    }
    #[cfg(test)]
    pub(crate) fn fence(&mut self, w: Value, y: Value) -> Result<()> {
        if let Some(old) = self.originals.get("fence") {
            ensure!(*old == w && self.originals["fence-ack"] == y, "personal fence conflict");
            return Ok(());
        }
        self.originals.as_object_mut().context("personal originals corrupted")?.insert("fence".into(), w);
        self.originals.as_object_mut().context("personal originals corrupted")?.insert("fence-ack".into(), y);
        self.db.execute("UPDATE personal_generation SET tombstone=1 WHERE singleton=1", [])?;
        Ok(())
    }
    pub(crate) fn reserve(&self, h_g: &str) -> Result<()> {
        ensure!(
            !self.db.query_row(
                "SELECT ever_claimed=1 OR tombstone=1 FROM personal_generation WHERE singleton=1",
                [],
                |r| r.get::<_, bool>(0)
            )?,
            "personal generation closed"
        );
        self.db.execute("INSERT INTO personal_possession_slot(singleton,capture,grant_hash) SELECT 1,?,? WHERE NOT EXISTS(SELECT 1 FROM personal_possession_slot)",params![self.id,h_g])?;
        ensure!(
            self.db.query_row(
                "SELECT capture=? AND grant_hash=? FROM personal_possession_slot WHERE singleton=1",
                params![self.id, h_g],
                |r| r.get::<_, bool>(0)
            )?,
            "personal slot conflict"
        );
        Ok(())
    }
    pub(crate) fn preparing(&self) -> Result<()> {
        ensure!(!self.closed() && !self.fenced() && self.db.query_row("SELECT ever_claimed=0 AND tombstone=0 AND NOT EXISTS(SELECT 1 FROM devices) AND NOT EXISTS(SELECT 1 FROM personal_device_origins) FROM personal_generation WHERE singleton=1",[],|r|r.get::<_,bool>(0))?,"personal preparation closed");
        Ok(())
    }
    pub(crate) fn claim(&self) -> Result<()> {
        ensure!(!self.fenced(), "personal session fenced");
        ensure!(self.db.execute("UPDATE personal_generation SET ever_claimed=1,revision=revision+1 WHERE singleton=1 AND ever_claimed=0 AND tombstone=0 AND NOT EXISTS(SELECT 1 FROM devices) AND NOT EXISTS(SELECT 1 FROM personal_device_origins)",[])?==1,"personal generation already claimed");
        Ok(())
    }
    pub(crate) fn acl(&self) -> Result<(u64, u64, String)> {
        let revision: i64 =
            self.db.query_row("SELECT revision FROM personal_generation WHERE singleton=1", [], |r| r.get(0))?;
        let version: i64 = self.db.query_row("SELECT version FROM personal_acl WHERE singleton=1", [], |r| r.get(0))?;
        let revision = checked_revision(revision)?;
        let version = checked_revision(version)?;
        let mut stmt = self.db.prepare("SELECT key,source,grant_id,scopes,lease_until FROM devices ORDER BY key")?;
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
        for (key, source, grant, scopes, lease) in rows {
            let mut scopes: Vec<String> = serde_json::from_str(&scopes)?;
            scopes.sort();
            values.push(json!([key, source, grant, scopes, lease]));
        }
        let hash = crate::fork::bootstrap::codec::hash_bytes(&serde_json::to_vec(&json!([
            "donna/active-acl-state/v1",
            values
        ]))?);
        Ok((revision, version, hash))
    }
    pub(crate) fn remember(&self, key: &str, grant: &str) -> Result<()> {
        self.db.execute("INSERT INTO personal_device_origins(key,generation,grant_id) SELECT ?,generation,? FROM personal_generation",params![key,grant])?;
        Ok(())
    }
}
pub(super) fn migrate(c: &Connection) -> Result<()> {
    // A transação de personal_bootstrap::migrate contém também este upgrade.
    // Escritas nas nove colunas anteriores (inclusive NOOP/name) invalidam o CAS;
    // a escrita interna de donna_oauth_epoch não é uma segunda mudança da ACL PL1.
    c.execute_batch("CREATE TABLE IF NOT EXISTS personal_possession_slot(singleton INTEGER PRIMARY KEY CHECK(singleton=1),capture TEXT NOT NULL,grant_hash TEXT NOT NULL);
        CREATE TRIGGER IF NOT EXISTS personal_possession_keep BEFORE DELETE ON personal_possession_slot BEGIN SELECT RAISE(ABORT,'personal possession retained'); END;
        CREATE TRIGGER IF NOT EXISTS personal_possession_fixed BEFORE UPDATE ON personal_possession_slot BEGIN SELECT RAISE(ABORT,'personal possession immutable'); END;
        CREATE TABLE IF NOT EXISTS personal_sessions(id TEXT PRIMARY KEY, config BLOB NOT NULL, originals TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0));
        CREATE TRIGGER IF NOT EXISTS personal_session_keep BEFORE DELETE ON personal_sessions BEGIN SELECT RAISE(ABORT,'personal session retained'); END;
        CREATE TRIGGER IF NOT EXISTS personal_session_binding BEFORE UPDATE OF id,config ON personal_sessions BEGIN SELECT RAISE(ABORT,'personal session binding immutable'); END;
        CREATE TABLE IF NOT EXISTS personal_acl(singleton INTEGER PRIMARY KEY CHECK(singleton=1), version INTEGER NOT NULL CHECK(version>0));
        INSERT INTO personal_acl SELECT 1,1 WHERE NOT EXISTS(SELECT 1 FROM personal_acl);
        CREATE TRIGGER IF NOT EXISTS personal_acl_insert AFTER INSERT ON devices BEGIN UPDATE personal_acl SET version=version+1 WHERE singleton=1; UPDATE personal_generation SET revision=revision+1 WHERE singleton=1; END;
        DROP TRIGGER IF EXISTS personal_acl_update;
        CREATE TRIGGER personal_acl_update AFTER UPDATE OF key,name,platform,source,grant_id,scopes,lease_until,created_at,last_seen_at ON devices BEGIN UPDATE personal_acl SET version=version+1 WHERE singleton=1; UPDATE personal_generation SET revision=revision+1 WHERE singleton=1; END;
        CREATE TRIGGER IF NOT EXISTS personal_acl_delete AFTER DELETE ON devices BEGIN UPDATE personal_acl SET version=version+1 WHERE singleton=1; UPDATE personal_generation SET revision=revision+1 WHERE singleton=1; END;")?;
    Ok(())
}
impl Store {
    pub(crate) fn personal_prepare_lookup(&self, cfg: &AdmittedConfig, public: &str) -> Result<String> {
        let c = self.db.locked();
        let id = super::personal_prepare::session(&c, public)?;
        ensure!(
            c.query_row("SELECT config=? FROM personal_sessions WHERE id=?", params![cfg.original_bytes(), id], |r| r
                .get::<_, bool>(0))?,
            "personal capture configuration changed"
        );
        Ok(id)
    }
    pub(crate) fn personal_capture(&self, cfg: &AdmittedConfig, h_t: &str) -> Result<String> {
        let c = self.db.locked();
        Ok(c.query_row(
            "SELECT id FROM personal_sessions WHERE config=? AND json_extract(originals,'$.\"app-consent\"[0][2]')=?",
            params![cfg.original_bytes(), h_t],
            |r| r.get(0),
        )?)
    }
    /// Privado ao consumidor; callbacks síncronos Connection-only, nunca rede/FS/Store.
    pub(crate) fn personal_session<T>(
        &self,
        cfg: &AdmittedConfig,
        id: &str,
        action: impl FnOnce(&mut Session<'_>) -> Result<T>,
    ) -> Result<T> {
        let c = self.db.locked();
        c.pragma_update(None, "synchronous", "FULL")?;
        let tx = c.unchecked_transaction()?;
        let config: Vec<u8> =
            tx.query_row("SELECT config FROM personal_generation WHERE singleton=1", [], |r| r.get(0))?;
        ensure!(config == cfg.original_bytes(), "personal configuration changed");
        let row: Option<(Vec<u8>, String, i64)> = tx
            .query_row("SELECT config,originals,revision FROM personal_sessions WHERE id=?", [id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?;
        let (originals, revision) = match row {
            Some((old, raw, revision)) => {
                ensure!(old == config, "personal capture changed");
                (serde_json::from_str(&raw)?, checked_revision(revision)?)
            }
            None => (json!({}), 0),
        };
        ensure!(
            originals.get("_userCode").is_none()
                && originals.get("_deviceCode").is_none()
                && originals.get("_candidateToken").is_none()
                && originals.get("_sentCandidate").is_none(),
            "unsupported clear personal secret format"
        );
        let mut s = Session { db: &tx, id, originals, revision };
        let result = action(&mut s)?;
        let next =
            revision.checked_add(1).filter(|n| *n <= 9_007_199_254_740_991).context("personal revision exhausted")?;
        let next_sql = i64::try_from(next).context("personal revision exceeds SQLite INTEGER")?;
        let revision_sql = i64::try_from(revision).context("personal revision exceeds SQLite INTEGER")?;
        let raw = serde_json::to_string(&s.originals)?;
        ensure!(raw.len() <= 192 * 1024, "personal session too large");
        if revision == 0 {
            tx.execute("INSERT INTO personal_sessions VALUES(?,?,?,?)", params![s.id, config, raw, next_sql])?;
        } else {
            ensure!(
                tx.execute(
                    "UPDATE personal_sessions SET originals=?,revision=? WHERE id=? AND revision=?",
                    params![raw, next_sql, s.id, revision_sql]
                )? == 1,
                "personal session CAS conflict"
            );
        }
        tx.commit()?;
        Ok(result)
    }
}
