//! Cercas por revisão: histórico imutável, segredo somente após terminal atual autenticado.
use super::{ceremony, live, protocol, shapes::Kind, sync, transport, wire};
use crate::{LockExt, hub::Hub};
use anyhow::{Context, Result, ensure};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;

type FenceRow = (Vec<u8>, Vec<u8>, Option<Vec<u8>>);

pub(super) fn project(db: &rusqlite::Connection, id: &str, packet: &mut Value, reply: &Value) -> Result<()> {
    let w = &reply["originals"]["fence"];
    if !w.is_array() {
        return Ok(());
    }
    let revision = w[0][6].as_u64().context("fence revision")?;
    if let Some(old) = packet.get("fence") {
        ensure!(old == w || old[0][6].as_u64().is_some_and(|r| r < revision), "fence regressed/changed");
    }
    let legacy_y = if packet.get("fence") == Some(w) { packet.get("fence-ack").cloned() } else { None };
    packet.as_object_mut().context("packet")?.remove("fence-ack");
    packet.as_object_mut().context("packet")?.remove("revocation");
    packet["fence"] = w.clone();
    let row: Option<FenceRow> = db
        .query_row(
            "SELECT w,y,x FROM personal_fence_history WHERE capture=? AND revision=?",
            params![id, i64::try_from(revision)?],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    if let Some((old, y, x)) = row {
        ensure!(old == serde_json::to_vec(w)?, "fence original conflict");
        packet["fence-ack"] = protocol::canonical(&y)?;
        if let Some(x) = x {
            packet["revocation"] = protocol::canonical(&x)?;
        }
    }
    if packet.get("fence-ack").is_none()
        && let Some(y) = legacy_y
    {
        packet["fence-ack"] = y;
    }
    for (remote, local) in [("fenceAck", "fence-ack"), ("revocation", "revocation")] {
        if let Some(v) = reply["originals"].get(remote).filter(|v| !v.is_null()) {
            ensure!(packet.get(local) == Some(v), "unknown local fence original");
        }
    }
    Ok(())
}
pub(super) async fn apply(hub: &Hub, id: &str, reply: &Value) -> Result<()> {
    sync::cut(hub);
    let w = &reply["originals"]["fence"];
    let revision = w[0][6].as_u64().context("fence revision")?;
    let (cfg, now) = hub.personal.with(|f| Ok((f.config.clone(), f.clock.validity()?)))?;
    let (packet, base, next) = {
        let db = hub.store.db.locked();
        let mut packet = ceremony::packet(&live::session(&db, id, &cfg)?);
        project(&db, id, &mut packet, reply)?;
        let base = live::acl(&db, None)?;
        let mut rows = Vec::new();
        let mut stmt = db.prepare("SELECT key,source,grant_id,scopes,lease_until FROM devices ORDER BY key")?;
        let mut deleted = 0u64;
        for row in stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<i64>>(4)?,
            ))
        })? {
            let (key, source, grant, scopes, until) = row?;
            if !w[0][4].is_null() && key == packet["intent"][0][6].as_str().context("A")? {
                ensure!(source == "account" && grant.as_deref() == w[0][4].as_str(), "fence Device changed");
                deleted = 1;
                continue;
            }
            let mut scopes: Vec<String> = serde_json::from_str(&scopes)?;
            scopes.sort();
            rows.push(json!([key, source, grant, scopes, until]));
        }
        let next = (
            base.0.checked_add(1 + deleted).context("revision overflow")?,
            base.1.checked_add(1 + deleted).context("ACL overflow")?,
            wire::hash(&json!(["donna/active-acl-state/v1", rows]))?,
        );
        (packet, base, next)
    };
    let c = ceremony::history(&cfg, &packet, now)?;
    let checked = c.original(Kind::Fence)?;
    ensure!(checked.payload() == &w[0], "fence changed");
    let existing = packet.get("fence-ack").cloned();
    let (y, x) = if let Some(y) = existing.clone() {
        (y, packet.get("revocation").cloned())
    } else {
        ceremony::wait_causal(hub, w[0][8].as_u64().context("W time")?, 9_007_199_254_740_991).await?;
        let at =
            hub.personal.with(|f| f.clock.issued_after(w[0][8].as_u64().context("W time")?, 9_007_199_254_740_991))?;
        let y = transport::signed(
            hub,
            json!([
                wire::domain("fence-ack"),
                cfg.context(),
                c.digest(),
                checked.digest(),
                w[0][4],
                w[0][5],
                next.0,
                next.1,
                next.2,
                at
            ]),
        )?;
        let x = if w[0][4].is_null() {
            None
        } else {
            Some(transport::signed(
                hub,
                json!([
                    wire::domain("revocation"),
                    cfg.context(),
                    c.digest(),
                    w[0][4],
                    w[0][5],
                    next.0,
                    next.1,
                    next.2,
                    at,
                    w[0][7]
                ]),
            )?)
        };
        (y, x)
    };
    let mut verified = packet.clone();
    verified["fence-ack"] = y.clone();
    if let Some(x) = &x {
        verified["revocation"] = x.clone();
    }
    ceremony::history(&cfg, &verified, hub.personal.with(|f| f.clock.validity())?)?;
    if existing.is_none() {
        hub.cloud.with_personal_status(|cloud|hub.personal.with(|f|{
            ensure!(f.config.original_bytes()==cfg.original_bytes(),"fence config changed");
            let db=hub.store.db.locked();db.pragma_update(None,"synchronous","FULL")?;let tx=db.unchecked_transaction()?;
            ensure!(live::acl(&tx,None)?==base,"fence ACL CAS");
            let latest:Option<i64>=tx.query_row("SELECT revision FROM personal_fence_latest WHERE capture=?",[id],|r|r.get(0)).optional()?;
            ensure!(latest.is_none_or(|r|u64::try_from(r).is_ok_and(|r|r<revision)),"fence revision CAS");
            let original=ceremony::packet(&live::session(&tx,id,&cfg)?);
            for name in ["receipt","grant","intent","local-ticket"] {ensure!(original.get(name)==packet.get(name),"fence originals changed");}
            if !w[0][4].is_null(){let app=packet["intent"][0][6].as_str().context("A")?;tx.execute("DELETE FROM devices WHERE key=? AND source='account' AND grant_id=?",params![app,w[0][4].as_str()])?;}
            tx.execute("UPDATE personal_generation SET tombstone=1,revision=revision+1 WHERE singleton=1",[])?;
            tx.execute("UPDATE personal_acl SET version=version+1 WHERE singleton=1",[])?;
            ensure!(live::acl(&tx,None)?==next,"fence ACL after triggers");
            tx.execute("INSERT INTO personal_fence_history VALUES(?,?,?,?,?)",params![id,i64::try_from(revision)?,serde_json::to_vec(w)?,serde_json::to_vec(&y)?,x.as_ref().map(serde_json::to_vec).transpose()?])?;
            tx.execute("INSERT INTO personal_fence_latest VALUES(?,?) ON CONFLICT(capture) DO UPDATE SET revision=excluded.revision WHERE personal_fence_latest.revision<excluded.revision",params![id,i64::try_from(revision)?])?;
            f.socket_live=false;cloud.connected=false;hub.cloud_synced.store(false,Ordering::Release);
            f.clock.validity()?;tx.commit()?;Ok(())
        }))?;
        hub.auth_changed();
    }
    // A resposta S já foi validada pelo Read nonce-bound original, antes desta chamada.
    if reply["state"][0][5] == "revoked"
        && reply["state"][0][12] == wire::hash(&w[0])?
        && reply["state"][0][13] == wire::hash(&y[0])?
    {
        let complete = if let Some(x) = &x {
            reply["state"][0][11] == wire::hash(&x[0])?
        } else {
            packet.get("receipt").is_none() && reply["state"][0][11].is_null()
        };
        if complete {
            let closure = serde_json::to_vec(&json!({"state":reply["state"],"fence":w,"fenceAck":y,"revocation":x}))?;
            let db = hub.store.db.locked();
            db.pragma_update(None, "synchronous", "FULL")?;
            let tx = db.unchecked_transaction()?;
            ensure!(
                tx.query_row(
                    "SELECT revision=? FROM personal_fence_latest WHERE capture=?",
                    params![i64::try_from(revision)?, id],
                    |r| r.get::<_, bool>(0)
                )?,
                "terminal fence revision changed"
            );
            tx.execute(
                "INSERT OR IGNORE INTO personal_fence_confirmed VALUES(?,?,?)",
                params![id, i64::try_from(revision)?, closure],
            )?;
            // Só segredo. Fatos, reservas, originais e ever_claimed permanecem.
            let public: Option<String> = tx
                .query_row("SELECT public_id FROM personal_captures WHERE session_id=?", [id], |r| r.get(0))
                .optional()?;
            if let Some(public) = public {
                tx.execute("UPDATE personal_enrollment_secrets SET sealed=NULL,purged=1,closure=? WHERE capture=? AND purged=0",params![closure,public])?;
            }
            tx.commit()?;
            return Ok(());
        }
    }
    transport::post(hub, "fence-ack", json!({"fenceAck":y})).await?;
    if let Some(x) = x {
        transport::post(hub, "revocation", json!({"revocation":x})).await?;
    }
    Ok(())
}
