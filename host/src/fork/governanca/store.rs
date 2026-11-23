pub(crate) use super::model::GovernancePolicy;
use super::model::{Save, Snapshot};
use crate::{fork::spaces, remote::cloud::Conflict};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const MAX_REVISION: i64 = (1_i64 << 53) - 1;

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    let tx = db.unchecked_transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_governance_policies (
            hub_id TEXT NOT NULL, space_id TEXT NOT NULL,
            revision INTEGER NOT NULL CHECK(revision > 0), policy TEXT NOT NULL,
            PRIMARY KEY(hub_id, space_id)
        );
        CREATE TABLE IF NOT EXISTS donna_governance_receipts (
            hub_id TEXT NOT NULL, space_id TEXT NOT NULL, client_request_id TEXT NOT NULL,
            payload_digest BLOB NOT NULL, answer TEXT NOT NULL,
            PRIMARY KEY(hub_id, space_id, client_request_id)
        );",
    )?;
    tx.commit()?;
    Ok(())
}

pub(crate) fn read_policy(db: &Connection, hub: &str, space: &str) -> Result<GovernancePolicy> {
    Ok(snapshot(db, hub, space)?.policy)
}

pub(super) fn snapshot<'a>(db: &Connection, hub: &'a str, space: &'a str) -> Result<Snapshot<'a>> {
    spaces::require_space(db, space)?;
    let row: Option<(i64, String)> = db
        .query_row(
            "SELECT revision, policy FROM donna_governance_policies WHERE hub_id=?1 AND space_id=?2",
            params![hub, space],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let (revision, policy) = if let Some((revision, raw)) = row {
        ensure!((1..=MAX_REVISION).contains(&revision), "governance_unavailable: revisão armazenada inválida.");
        let policy: GovernancePolicy =
            serde_json::from_str(&raw).map_err(|_| anyhow!("governance_unavailable: política armazenada inválida."))?;
        policy.validate().map_err(|_| anyhow!("governance_unavailable: política armazenada inválida."))?;
        (revision, policy)
    } else {
        (0, GovernancePolicy::default())
    };
    Ok(Snapshot { version: 1, hub_id: hub, space_id: space, revision, policy })
}

pub(super) fn save(db: &Connection, hub: &str, space: &str, request: &Save) -> Result<(Value, bool)> {
    request.policy.validate()?;
    ensure!((0..=MAX_REVISION).contains(&request.expected_revision), "invalid_field: expectedRevision inválida.");
    let id = &request.client_request_id;
    ensure!(
        uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id.as_str()),
        "invalid_field: clientRequestId deve ser um UUID canônico."
    );
    let payload = serde_json::to_vec(&(request.expected_revision, &request.policy))?;
    let digest = Sha256::digest(payload);
    let digest: &[u8] = digest.as_ref();
    let mut current = snapshot(db, hub, space)?;
    let receipt: Option<(Vec<u8>, String)> = db
        .query_row(
            "SELECT payload_digest, answer FROM donna_governance_receipts
             WHERE hub_id=?1 AND space_id=?2 AND client_request_id=?3",
            params![hub, space, id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some((saved_digest, answer)) = receipt {
        if saved_digest.as_slice() != digest {
            return Err(Conflict("idempotency_conflict").into());
        }
        return Ok((serde_json::from_str(&answer)?, false));
    }
    if current.revision != request.expected_revision {
        return Err(Conflict("revision_conflict").into());
    }
    let changed = current.policy != request.policy;
    if changed {
        ensure!(current.revision < MAX_REVISION, "governance_unavailable: limite de revisão atingido.");
        let next = current.revision + 1;
        let raw = serde_json::to_string(&request.policy)?;
        let rows = db.execute(
            "INSERT INTO donna_governance_policies(hub_id, space_id, revision, policy)
             VALUES(?1, ?2, ?3, ?4) ON CONFLICT(hub_id, space_id) DO UPDATE
             SET revision=excluded.revision, policy=excluded.policy WHERE donna_governance_policies.revision=?5",
            params![hub, space, next, raw, current.revision],
        )?;
        if rows != 1 {
            return Err(Conflict("revision_conflict").into());
        }
        current.revision = next;
        current.policy = request.policy.clone();
    }
    let answer = json!({"snapshot":current,"receipt":{"clientRequestId":id,"revision":current.revision}});
    db.execute(
        "INSERT INTO donna_governance_receipts(hub_id, space_id, client_request_id, payload_digest, answer)
         VALUES(?1, ?2, ?3, ?4, ?5)",
        params![hub, space, id, digest, serde_json::to_string(&answer)?],
    )?;
    Ok((answer, changed))
}
