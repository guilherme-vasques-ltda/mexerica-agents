//! Fork tables; independent of the upstream schema version.
use super::{Store, now_ms};
use crate::LockExt;
use crate::computers::{jobs::Job, registry::Computer};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

pub(super) fn migrate(c: &Connection) -> Result<()> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS donna_computers(
        id TEXT PRIMARY KEY, device_key TEXT NOT NULL UNIQUE, label TEXT NOT NULL,
        platform TEXT NOT NULL CHECK(platform IN ('macos','windows')),
        os_version TEXT, arch TEXT, app_version TEXT, capabilities TEXT NOT NULL DEFAULT '{}',
        presence TEXT NOT NULL DEFAULT '{}', enrolled_at INTEGER NOT NULL, last_seen_at INTEGER,
        last_used_at INTEGER, state_since INTEGER, removed_at INTEGER, last_boot_id TEXT);
        CREATE TABLE IF NOT EXISTS donna_computer_jobs(
        id TEXT PRIMARY KEY, computer_id TEXT NOT NULL, bot_id TEXT NOT NULL, turn_message_id TEXT NOT NULL,
        kind TEXT NOT NULL, spec TEXT NOT NULL, idem_key TEXT NOT NULL, state TEXT NOT NULL,
        lease_token TEXT, lease_until INTEGER, created_at INTEGER NOT NULL, start_by INTEGER NOT NULL,
        deadline_at INTEGER NOT NULL, started_at INTEGER, finished_at INTEGER, outcome TEXT, error_code TEXT, result TEXT,
        origin_kind TEXT NOT NULL DEFAULT 'chat', routine_id TEXT, run_id TEXT, scheduled_at INTEGER,
        origin TEXT NOT NULL, exec_timeout_ms INTEGER NOT NULL, notice_id TEXT, cancel_requested INTEGER NOT NULL DEFAULT 0,
        unknown_reported INTEGER NOT NULL DEFAULT 0);
        CREATE INDEX IF NOT EXISTS donna_cj_active ON donna_computer_jobs(computer_id,state);
        CREATE INDEX IF NOT EXISTS donna_cj_idem ON donna_computer_jobs(idem_key);
        CREATE TABLE IF NOT EXISTS donna_computer_audit(
        computer_id TEXT NOT NULL, seq INTEGER NOT NULL, prev TEXT NOT NULL, hash TEXT NOT NULL,
        record TEXT NOT NULL, received_at INTEGER NOT NULL, PRIMARY KEY(computer_id,seq));")?;
    let has_boot: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('donna_computers') WHERE name='last_boot_id')",
        [],
        |r| r.get(0),
    )?;
    if !has_boot {
        c.execute_batch("ALTER TABLE donna_computers ADD COLUMN last_boot_id TEXT")?;
    }
    Ok(())
}
fn row_computer(r: &rusqlite::Row<'_>) -> rusqlite::Result<Computer> {
    // The named projection is kept next to its decoder.
    Ok(Computer {
        id: r.get(0)?,
        device_key: r.get(1)?,
        label: r.get(2)?,
        platform: r.get(3)?,
        os_version: r.get(4)?,
        arch: r.get(5)?,
        app_version: r.get(6)?,
        capabilities: decode(r, 7)?,
        presence: decode(r, 8)?,
        enrolled_at: r.get(9)?,
        last_seen_at: r.get(10)?,
        last_used_at: r.get(11)?,
        state_since: r.get(12)?,
        removed_at: r.get(13)?,
        last_boot_id: r.get(14)?,
    })
}
fn decode<T: serde::de::DeserializeOwned>(r: &rusqlite::Row<'_>, i: usize) -> rusqlite::Result<T> {
    serde_json::from_str(&r.get::<_, String>(i)?)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Text, Box::new(e)))
}
fn write_computer(db: &Connection, c: &Computer) -> Result<()> {
    db.execute("INSERT INTO donna_computers VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15) ON CONFLICT(id) DO UPDATE SET label=?3,os_version=?5,arch=?6,app_version=?7,capabilities=?8,presence=?9,last_seen_at=?11,last_used_at=?12,state_since=?13,removed_at=?14,last_boot_id=?15",params![c.id,c.device_key,c.label,c.platform,c.os_version,c.arch,c.app_version,c.capabilities.to_string(),c.presence.to_string(),c.enrolled_at,c.last_seen_at,c.last_used_at,c.state_since,c.removed_at,c.last_boot_id])?;
    Ok(())
}

impl Store {
    pub fn computers(&self) -> Result<Vec<Computer>> {
        let c = self.db.locked();
        let mut st = c.prepare("SELECT c.id,c.device_key,c.label,c.platform,c.os_version,c.arch,c.app_version,c.capabilities,c.presence,c.enrolled_at,c.last_seen_at,c.last_used_at,c.state_since,c.removed_at,c.last_boot_id FROM donna_computers c JOIN devices d ON d.key=c.device_key WHERE c.removed_at IS NULL AND d.source='local' AND d.scopes='[\"executor\"]' ORDER BY c.enrolled_at,c.id")?;
        Ok(st.query_map([], row_computer)?.collect::<rusqlite::Result<_>>()?)
    }
    pub fn save_computer(&self, c: &Computer) -> Result<()> {
        write_computer(&self.db.locked(), c)
    }
    /// Authorize the executor and register its identity in the same SQLite commit. The code shown
    /// on this computer is a new pairing: it lifts a revocation block on the same key.
    pub fn enroll_computer(&self, c: &Computer) -> Result<()> {
        let mut db = self.db.locked();
        let tx = db.transaction()?;
        crate::fork::workspace::liberar_pareamento_on(&tx, &c.device_key)?;
        tx.execute("INSERT INTO devices(key,name,platform,source,grant_id,scopes,lease_until,created_at) VALUES(?1,?2,?3,'local',NULL,'[\"executor\"]',NULL,?4)",params![c.device_key,c.label,c.platform,c.enrolled_at])?;
        crate::fork::tela_por_computador::clear_target_on(&tx, &c.id)?;
        write_computer(&tx, c)?;
        tx.commit()?;
        Ok(())
    }
    pub fn computer_jobs(&self) -> Result<Vec<Job>> {
        let c = self.db.locked();
        let mut st = c.prepare("SELECT id,computer_id,bot_id,turn_message_id,kind,spec,idem_key,state,lease_token,lease_until,created_at,start_by,deadline_at,started_at,finished_at,outcome,error_code,result,origin_kind,routine_id,run_id,scheduled_at,origin,exec_timeout_ms,notice_id,cancel_requested,unknown_reported FROM donna_computer_jobs ORDER BY created_at,id")?;
        Ok(st
            .query_map([], |r| {
                Ok(Job {
                    id: r.get(0)?,
                    computer_id: r.get(1)?,
                    bot_id: r.get(2)?,
                    turn_message_id: r.get(3)?,
                    kind: r.get(4)?,
                    spec: decode(r, 5)?,
                    idem_key: r.get(6)?,
                    state: serde_json::from_value(Value::String(r.get(7)?)).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(e))
                    })?,
                    lease_token: r.get(8)?,
                    lease_until: r.get(9)?,
                    created_at: r.get(10)?,
                    start_by: r.get(11)?,
                    deadline_at: r.get(12)?,
                    started_at: r.get(13)?,
                    finished_at: r.get(14)?,
                    outcome: r.get(15)?,
                    error_code: r.get(16)?,
                    result: r.get::<_, Option<String>>(17)?.map(|v| serde_json::from_str(&v)).transpose().map_err(
                        |e| rusqlite::Error::FromSqlConversionFailure(17, rusqlite::types::Type::Text, Box::new(e)),
                    )?,
                    origin_kind: r.get(18)?,
                    routine_id: r.get(19)?,
                    run_id: r.get(20)?,
                    scheduled_at: r.get(21)?,
                    origin: decode(r, 22)?,
                    exec_timeout_ms: r.get(23)?,
                    notice_id: r.get(24)?,
                    cancel_requested: r.get(25)?,
                    unknown_reported: r.get(26)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn save_computer_job(&self, j: &Job) -> Result<()> {
        self.save_computer_job_in_execution(j, None, None)
    }
    pub(crate) fn save_computer_job_in_execution(
        &self,
        j: &Job,
        execution: Option<&crate::fork::sessoes::ExecutionRef>,
        computer: Option<&Computer>,
    ) -> Result<()> {
        let state = serde_json::to_value(j.state)?;
        let mut db = self.db.locked();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        crate::fork::sessoes::bindings::check_optional(&tx, execution)?;
        // A admissão e o registro 05b usam a MESMA Connection/Tx IMMEDIATE.
        // Renovações, resultados e reconciliação não são novas admissões.
        let previous: Option<String> =
            tx.query_row("SELECT state FROM donna_computer_jobs WHERE id=?1", [&j.id], |r| r.get(0)).optional()?;
        let starting = j.state == crate::computers::jobs::Phase::Running && previous.as_deref() != Some("running");
        let queuing = j.state == crate::computers::jobs::Phase::Queued
            && !matches!(previous.as_deref(), Some("queued" | "delivered"));
        let admitting = queuing
            || matches!(
                j.state,
                crate::computers::jobs::Phase::Delivered
                    | crate::computers::jobs::Phase::Accepted
                    | crate::computers::jobs::Phase::AwaitingApproval
            ) && previous.as_deref() != state.as_str()
            || starting;
        if admitting {
            crate::fork::atividades::revisao::estado::work_allowed_on(&tx)?;
        }
        if (j.state.before_start() || starting)
            && let Some(id) = j.origin["sessionBinding"].as_str()
        {
            let binding = crate::fork::sessoes::bindings::resolve(&tx, id, &j.bot_id)?;
            crate::fork::sessoes::bindings::assert_binding_live(&tx, &binding)?;
        }
        if let Some(computer) = computer {
            ensure!(computer.id == j.computer_id, "unknown_job: computador de outro trabalho");
            write_computer(&tx, computer)?;
        }
        tx.execute("INSERT INTO donna_computer_jobs VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27) ON CONFLICT(id) DO UPDATE SET state=?8,lease_token=?9,lease_until=?10,started_at=?14,finished_at=?15,outcome=?16,error_code=?17,result=?18,notice_id=?25,cancel_requested=?26,unknown_reported=?27",params![j.id,j.computer_id,j.bot_id,j.turn_message_id,j.kind,j.spec.to_string(),j.idem_key,state.as_str(),j.lease_token,j.lease_until,j.created_at,j.start_by,j.deadline_at,j.started_at,j.finished_at,j.outcome,j.error_code,j.result.as_ref().map(Value::to_string),j.origin_kind,j.routine_id,j.run_id,j.scheduled_at,j.origin.to_string(),j.exec_timeout_ms,j.notice_id,j.cancel_requested,j.unknown_reported])?;
        tx.commit()?;
        Ok(())
    }
    pub fn computer_audit(&self, id: &str, records: &[Value]) -> Result<i64> {
        let mut c = self.db.locked();
        let tx = c.transaction()?;
        let (mut seq, mut hash): (i64, String) = tx
            .query_row(
                "SELECT seq,hash FROM donna_computer_audit WHERE computer_id=?1 ORDER BY seq DESC LIMIT 1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .unwrap_or((0, String::new()));
        let mut gap = false;
        for r in records {
            let n = r["seq"].as_i64().ok_or_else(|| anyhow::anyhow!("chain_gap: seq inválido"))?;
            if n <= seq {
                let previous: Option<String> = tx
                    .query_row(
                        "SELECT record FROM donna_computer_audit WHERE computer_id=?1 AND seq=?2",
                        params![id, n],
                        |r| r.get(0),
                    )
                    .optional()?;
                ensure!(previous.as_deref() == Some(r.to_string().as_str()), "chain_gap: registro reescrito");
                continue;
            }
            gap |= n != seq + 1 || r["prev"].as_str() != Some(hash.as_str());
            r["hash"]
                .as_str()
                .filter(|v| !v.is_empty())
                .ok_or_else(|| anyhow::anyhow!("chain_gap: hash ausente"))?
                .clone_into(&mut hash);
            tx.execute(
                "INSERT INTO donna_computer_audit VALUES(?1,?2,?3,?4,?5,?6)",
                params![id, n, r["prev"].as_str(), hash, r.to_string(), now_ms()],
            )?;
            seq = n;
        }
        tx.commit()?;
        ensure!(!gap, "chain_gap: cadeia interrompida; registros preservados para inspeção");
        Ok(seq)
    }
    pub fn clean_computer_jobs(&self, now: i64) -> Result<()> {
        let c = self.db.locked();
        // Spec may contain file contents too; only metadata survives the result retention.
        c.execute(
            // Só o tipo "planilha" sobrevive (sem conteúdo): sem ele um resultado tardio passaria sem a conferência do recibo.
            "UPDATE donna_computer_jobs SET result=NULL,origin='{}',spec=CASE WHEN kind='fs.write' AND json_extract(spec,'$.purpose')='spreadsheet' THEN '{\"purpose\":\"spreadsheet\"}' ELSE '{}' END WHERE finished_at < ?1",
            [now - 86_400_000],
        )?;
        c.execute("DELETE FROM donna_computer_jobs WHERE finished_at < ?1", [now - 30 * 86_400_000])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_migration_preserves_legacy_computers_and_is_idempotent() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE donna_computers(
                id TEXT PRIMARY KEY, device_key TEXT NOT NULL UNIQUE, label TEXT NOT NULL,
                platform TEXT NOT NULL, os_version TEXT, arch TEXT, app_version TEXT,
                capabilities TEXT NOT NULL DEFAULT '{}', presence TEXT NOT NULL DEFAULT '{}',
                enrolled_at INTEGER NOT NULL, last_seen_at INTEGER, last_used_at INTEGER,
                state_since INTEGER, removed_at INTEGER);
             INSERT INTO donna_computers(id,device_key,label,platform,enrolled_at)
                VALUES('computer','device','Computador de teste','macos',123);",
        )
        .unwrap();
        migrate(&db).unwrap();
        let (label, at, boot): (String, i64, Option<String>) = db
            .query_row("SELECT label,enrolled_at,last_boot_id FROM donna_computers", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!(label, "Computador de teste");
        assert_eq!(at, 123);
        assert_eq!(boot, None);
        db.execute("UPDATE donna_computers SET last_boot_id='boot-1'", []).unwrap();
        migrate(&db).unwrap();
        assert_eq!(
            db.query_row("SELECT last_boot_id FROM donna_computers", [], |r| r.get::<_, String>(0)).unwrap(),
            "boot-1"
        );
    }
}
