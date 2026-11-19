//! Declarações públicas de preparo e resposta begin cifrada. Mesma Connection de posse.
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

pub(crate) fn migrate(c: &Connection) -> Result<()> {
    c.execute_batch("CREATE TRIGGER IF NOT EXISTS personal_local_device_claim AFTER INSERT ON devices WHEN EXISTS(SELECT 1 FROM personal_generation WHERE ever_claimed=0) BEGIN UPDATE personal_generation SET ever_claimed=1,revision=revision+1 WHERE singleton=1; END;
      CREATE TABLE IF NOT EXISTS personal_captures(public_id TEXT PRIMARY KEY, session_id TEXT NOT NULL UNIQUE, q_hash TEXT NOT NULL, q_wire_hash TEXT NOT NULL);
      CREATE TRIGGER IF NOT EXISTS personal_capture_fixed BEFORE UPDATE ON personal_captures BEGIN SELECT RAISE(ABORT,'personal capture immutable'); END;
      CREATE TRIGGER IF NOT EXISTS personal_capture_keep BEFORE DELETE ON personal_captures BEGIN SELECT RAISE(ABORT,'personal capture retained'); END;
      CREATE TABLE IF NOT EXISTS personal_prepare_operations(capture TEXT PRIMARY KEY, declaration TEXT NOT NULL, status TEXT NOT NULL CHECK(status IN ('preparedNotSent','possibleSend','unknown','recorded','closed')), result TEXT, revision INTEGER NOT NULL CHECK(revision>0));
      CREATE TRIGGER IF NOT EXISTS personal_prepare_binding BEFORE UPDATE OF capture,declaration ON personal_prepare_operations BEGIN SELECT RAISE(ABORT,'personal prepare binding immutable'); END;
      CREATE TRIGGER IF NOT EXISTS personal_prepare_keep BEFORE DELETE ON personal_prepare_operations BEGIN SELECT RAISE(ABORT,'personal prepare retained'); END;
      CREATE TRIGGER IF NOT EXISTS personal_prepare_no_reopen BEFORE UPDATE ON personal_prepare_operations WHEN (OLD.status<>'preparedNotSent' AND NEW.status='preparedNotSent') OR (OLD.status='closed' AND NEW.status<>'closed') OR NEW.revision<=OLD.revision BEGIN SELECT RAISE(ABORT,'personal prepare closed'); END;
      CREATE TABLE IF NOT EXISTS personal_enrollment_secrets(capture TEXT PRIMARY KEY, facts TEXT NOT NULL, sealed BLOB, purged INTEGER NOT NULL DEFAULT 0 CHECK(purged IN (0,1)), closure BLOB, CHECK((purged=0 AND sealed IS NOT NULL AND closure IS NULL) OR (purged=1 AND sealed IS NULL AND closure IS NOT NULL)));
      CREATE TRIGGER IF NOT EXISTS personal_enrollment_fixed BEFORE UPDATE OF capture,facts ON personal_enrollment_secrets BEGIN SELECT RAISE(ABORT,'enrollment binding immutable'); END;
      CREATE TRIGGER IF NOT EXISTS personal_enrollment_keep BEFORE DELETE ON personal_enrollment_secrets BEGIN SELECT RAISE(ABORT,'enrollment retained'); END;
      CREATE TRIGGER IF NOT EXISTS personal_enrollment_no_reopen BEFORE UPDATE ON personal_enrollment_secrets WHEN OLD.purged=1 OR NEW.purged<>1 BEGIN SELECT RAISE(ABORT,'enrollment secret immutable'); END;")?;
    Ok(())
}
pub(crate) fn register(c: &Connection, id: &str, public: &str, hq: &str, wire_hash: &str) -> Result<()> {
    let old: Option<(String, String, String)> = c
        .query_row("SELECT session_id,q_hash,q_wire_hash FROM personal_captures WHERE public_id=?", [public], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?;
    if let Some(old) = old {
        ensure!(old == (id.into(), hq.into(), wire_hash.into()), "personal capture conflict");
    } else {
        c.execute("INSERT INTO personal_captures VALUES(?,?,?,?)", params![public, id, hq, wire_hash])?;
    }
    Ok(())
}
pub(crate) fn session(c: &Connection, public: &str) -> Result<String> {
    Ok(c.query_row("SELECT session_id FROM personal_captures WHERE public_id=?", [public], |r| r.get(0))?)
}
pub(crate) fn declare(c: &Connection, capture: &str, declaration: &Value) -> Result<String> {
    let raw = serde_json::to_string(declaration)?;
    let old: Option<(String, String)> = c
        .query_row("SELECT declaration,status FROM personal_prepare_operations WHERE capture=?", [capture], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?;
    if let Some((original, status)) = old {
        ensure!(raw == original, "candidate declaration conflict");
        return Ok(status);
    }
    c.execute("INSERT INTO personal_prepare_operations VALUES(?,?,'preparedNotSent',NULL,1)", params![capture, raw])?;
    Ok("preparedNotSent".into())
}
pub(crate) fn possible_send(c: &Connection, capture: &str) -> Result<()> {
    ensure!(c.execute("UPDATE personal_prepare_operations SET status='possibleSend',revision=revision+1 WHERE capture=? AND status='preparedNotSent'",[capture])?==1,"candidate send already possible");
    Ok(())
}
pub(crate) fn record(c: &Connection, capture: &str, result: &Value) -> Result<()> {
    let raw = serde_json::to_string(result)?;
    let old: Option<String> =
        c.query_row("SELECT result FROM personal_prepare_operations WHERE capture=?", [capture], |r| r.get(0))?;
    if let Some(old) = old {
        ensure!(old == raw, "candidate result changed");
        return Ok(());
    }
    ensure!(c.execute("UPDATE personal_prepare_operations SET status='recorded',result=?,revision=revision+1 WHERE capture=? AND status IN ('possibleSend','unknown')",params![raw,capture])?==1,"candidate result conflict");
    Ok(())
}
pub(crate) fn unknown(c: &Connection, capture: &str) -> Result<()> {
    c.execute("UPDATE personal_prepare_operations SET status='unknown',revision=revision+1 WHERE capture=? AND status='possibleSend'",[capture])?;
    Ok(())
}
pub(crate) fn enrollment(c: &Connection, capture: &str) -> Result<Option<(Value, Vec<u8>)>> {
    let row: Option<(String, Option<Vec<u8>>, bool)> = c
        .query_row("SELECT facts,sealed,purged FROM personal_enrollment_secrets WHERE capture=?", [capture], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?;
    row.map(|(facts, sealed, purged)| {
        ensure!(!purged, "enrollment secret purged");
        Ok((serde_json::from_str(&facts)?, sealed.context("enrollment secret missing")?))
    })
    .transpose()
}
pub(crate) fn retain_enrollment(c: &Connection, capture: &str, facts: &Value, sealed: &[u8]) -> Result<()> {
    if let Some((old, bytes)) = enrollment(c, capture)? {
        ensure!(old == *facts && bytes == sealed, "enrollment original changed");
        return Ok(());
    }
    c.execute(
        "INSERT INTO personal_enrollment_secrets(capture,facts,sealed) VALUES(?,?,?)",
        params![capture, serde_json::to_string(facts)?, sealed],
    )?;
    Ok(())
}
/// Somente o consumidor de S autenticado fornece closure pré-consumo; nunca relógio/body.
pub(crate) fn purge_unconsumed(c: &Connection, capture: &str, closure: &[u8]) -> Result<()> {
    ensure!(!c.query_row("SELECT EXISTS(SELECT 1 FROM personal_possession_slot) OR EXISTS(SELECT 1 FROM personal_generation WHERE ever_claimed=1)",[],|r|r.get::<_,bool>(0))?,"consumed enrollment requires complete fence closure");
    c.execute(
        "UPDATE personal_enrollment_secrets SET sealed=NULL,purged=1,closure=? WHERE capture=? AND purged=0",
        params![closure, capture],
    )?;
    Ok(())
}
