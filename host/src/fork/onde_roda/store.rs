use super::{
    autoridade::Authority,
    tipos::{Operation, Phase, Place},
};
use crate::{LockExt as _, store::Store};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension as _, params};
use serde_json::Value;

pub fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS donna_workplaces(
        id TEXT PRIMARY KEY, hub_id TEXT NOT NULL, resource_id TEXT NOT NULL UNIQUE,
        created_origin TEXT NOT NULL CHECK(created_origin='local'), owner_user_id TEXT CHECK(owner_user_id IS NULL),
        record TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS donna_workplace_pins(pin TEXT PRIMARY KEY, hub_id TEXT NOT NULL, resource_id TEXT NOT NULL, place_id TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS donna_workplace_requests(id TEXT PRIMARY KEY, hub_id TEXT NOT NULL, digest TEXT NOT NULL, response TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS donna_workplace_inspections(
            creation_order INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT NOT NULL UNIQUE,
            place_id TEXT NOT NULL, record TEXT NOT NULL);")?;
    // Migração é chamada uma vez ao abrir o Hub; nunca retomar consulta de outra geração.
    let mut stmt = db.prepare("SELECT id,record FROM donna_workplace_inspections")?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    for (id, raw) in rows {
        let mut op: Operation = serde_json::from_str(&raw)?;
        if op.phase == Phase::Verificando {
            op.phase = Phase::InterrompidaIncerta;
            op.reason = Some("hub_restarted".into());
            db.execute(
                "UPDATE donna_workplace_inspections SET record=? WHERE id=?",
                params![serde_json::to_string(&op)?, id],
            )?;
        }
    }
    Ok(())
}
pub fn transaction<T>(store: &Store, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    let mut db = store.db.locked();
    let tx = db.transaction()?;
    let answer = f(&tx)?;
    tx.commit()?;
    Ok(answer)
}
/// Raiz do DB deste Hub, nunca `service::data_dir` global. `:memory:` exige fixture explícita.
pub fn runtime_root(store: &Store) -> Result<std::path::PathBuf> {
    let db = store.db.locked();
    let path =
        db.path().filter(|s| !s.is_empty() && *s != ":memory:").ok_or_else(|| anyhow!("runtime_root_required"))?;
    let path = std::path::Path::new(path);
    let absolute = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir()?.join(path) };
    Ok(absolute.parent().ok_or_else(|| anyhow!("runtime_root_required"))?.join("donna-workplace-runtime"))
}
pub fn get(db: &Connection, id: &str) -> Result<Place> {
    let raw: String = db
        .query_row("SELECT record FROM donna_workplaces WHERE id=?", [id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| anyhow!("place_not_found"))?;
    Ok(serde_json::from_str(&raw)?)
}
pub fn list(db: &Connection, a: &Authority) -> Result<Vec<Value>> {
    let mut stmt = db.prepare("SELECT record FROM donna_workplaces WHERE hub_id=? ORDER BY id")?;
    let rows = stmt.query_map([a.hub_id()], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Vec::new();
    for raw in rows {
        let p: Place = serde_json::from_str(&raw)?;
        if !p.removed {
            a.target(&p)?;
            out.push(p.summary());
        }
    }
    Ok(out)
}
pub fn save(db: &Connection, p: &Place) -> Result<()> {
    db.execute(
        "INSERT INTO donna_workplaces(id,hub_id,resource_id,created_origin,owner_user_id,record)
        VALUES(?,?,?,'local',NULL,?) ON CONFLICT(id) DO UPDATE SET record=excluded.record",
        params![p.id, p.hub_id, p.resource_id, serde_json::to_string(p)?],
    )?;
    Ok(())
}
pub fn reserve(db: &Connection, p: &Place, pin: &str) -> Result<()> {
    let prior: Option<(String, String, String)> = db
        .query_row("SELECT hub_id,resource_id,place_id FROM donna_workplace_pins WHERE pin=?", [pin], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?;
    // Mesmo pin antigo é tombstone: só a identidade ATUAL é repetível via receipt.
    ensure!(prior.is_none(), "runner_pin_reserved");
    db.execute("INSERT INTO donna_workplace_pins VALUES(?,?,?,?)", params![pin, p.hub_id, p.resource_id, p.id])?;
    Ok(())
}
pub fn cas(p: &Place, revision: u64, admission: u64, connection: u64) -> Result<()> {
    ensure!(
        p.revision == revision && p.admission_revision == admission && p.connection_revision == connection,
        "revision_conflict"
    );
    Ok(())
}
pub fn receipt(db: &Connection, a: &Authority, id: &str, digest: &str) -> Result<Option<Value>> {
    let row: Option<(String, String, String)> = db
        .query_row("SELECT hub_id,digest,response FROM donna_workplace_requests WHERE id=?", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?;
    row.map(|(hub, old, raw)| {
        ensure!(hub == a.hub_id() && old == digest, "request_conflict");
        Ok(serde_json::from_str(&raw)?)
    })
    .transpose()
}
pub fn remember(db: &Connection, a: &Authority, id: &str, digest: &str, response: &Value) -> Result<()> {
    db.execute(
        "INSERT INTO donna_workplace_requests VALUES(?,?,?,?)",
        params![id, a.hub_id(), digest, serde_json::to_string(response)?],
    )?;
    Ok(())
}
pub fn operation(db: &Connection, id: &str) -> Result<Operation> {
    let raw: String = db
        .query_row("SELECT record FROM donna_workplace_inspections WHERE id=?", [id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| anyhow!("operation_not_found"))?;
    Ok(serde_json::from_str(&raw)?)
}
pub fn save_operation(db: &Connection, op: &Operation) -> Result<()> {
    db.execute(
        "INSERT INTO donna_workplace_inspections(id,place_id,record) VALUES(?,?,?)
        ON CONFLICT(id) DO UPDATE SET record=excluded.record",
        params![op.id, op.place_id, serde_json::to_string(op)?],
    )?;
    Ok(())
}
pub fn active(db: &Connection, place_id: &str) -> Result<bool> {
    let mut stmt = db.prepare("SELECT record FROM donna_workplace_inspections WHERE place_id=?")?;
    for row in stmt.query_map([place_id], |r| r.get::<_, String>(0))? {
        let op: Operation = serde_json::from_str(&row?)?;
        if op.phase == Phase::Verificando {
            return Ok(true);
        }
    }
    Ok(false)
}
pub fn latest_operations(db: &Connection, a: &Authority) -> Result<Vec<Operation>> {
    let mut stmt = db.prepare("SELECT record FROM donna_workplace_inspections ORDER BY creation_order")?;
    let mut latest = std::collections::BTreeMap::<String, Operation>::new();
    for row in stmt.query_map([], |r| r.get::<_, String>(0))? {
        let op: Operation = serde_json::from_str(&row?)?;
        let place = get(db, &op.place_id)?;
        if place.removed || op.hub_id != a.hub_id() {
            continue;
        }
        a.target(&place)?;
        if op.authority != "local"
            || op.admission_revision != place.admission_revision
            || op.connection_revision != place.connection_revision
        {
            continue;
        }
        // Ordem privada da primeira inserção, preservada por UPDATE/UPSERT;
        // UUID e relógio de parede não definem qual consulta foi criada depois.
        latest.insert(op.place_id.clone(), op);
    }
    Ok(latest.into_values().collect())
}
pub fn invalidate(db: &Connection, place_id: &str) -> Result<()> {
    let mut stmt = db.prepare("SELECT record FROM donna_workplace_inspections WHERE place_id=?")?;
    let rows = stmt.query_map([place_id], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    for raw in rows {
        let mut op: Operation = serde_json::from_str(&raw)?;
        if op.phase == Phase::Verificando {
            op.phase = Phase::CanceladaIncerta;
            op.reason = Some("target_invalidated".into());
            save_operation(db, &op)?;
        }
    }
    Ok(())
}
pub fn live(store: &Store, a: &Authority, id: &str) -> Result<(Place, Operation)> {
    transaction(store, |db| {
        let op = operation(db, id)?;
        let p = get(db, &op.place_id)?;
        a.operation(&p, &op)?;
        Ok((p, op))
    })
}
