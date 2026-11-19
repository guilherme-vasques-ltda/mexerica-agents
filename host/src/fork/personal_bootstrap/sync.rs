//! Snapshot atual, promoção e aceitação de V; nenhuma operação de rede sob guarda.
use super::{
    ceremony,
    clock::{Lease, SnapshotBinding, SnapshotFlight},
    live, protocol,
    runtime::Applied,
    shapes::Kind,
    transport, wire,
};
use crate::{LockExt, hub::Hub, remote::cloud::Owner};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;

pub(super) fn guard<T>(
    hub: &Hub,
    socket: Option<u64>,
    accepted: bool,
    action: impl FnOnce(&live::Record) -> Result<T>,
) -> Result<T> {
    hub.cloud.with_personal_status(|cloud| {
        hub.personal.with(|f| {
            f.config.matches(&hub.identity)?;
            let a = f.applied.as_mut().context("personal snapshot not applied")?;
            f.journal.as_ref().context("root journal absent")?.matches_applied(&a.record)?;
            ensure!(
                cloud.owner.as_ref().is_some_and(|o| o.user_id == a.record.binding.owner),
                "personal owner changed"
            );
            if let Some(epoch) = socket {
                ensure!(f.socket_live && f.socket_epoch == epoch, "personal socket replaced");
            }
            let db = hub.store.db.locked();
            live::guard(&db, &f.config, &a.record, &mut a.lease, &mut f.clock, accepted)?;
            action(&a.record)
        })
    })
}
fn notify(hub: &Hub) {
    let _ = hub.events.send(json!({"type":"cloud","cloud":hub.cloud.status()}));
}
pub(super) fn cut(hub: &Hub) {
    hub.cloud_synced.store(false, Ordering::Release);
    #[cfg(test)]
    {
        let schedule = hub.personal.test_cut_after_false.locked().take();
        if let Some(schedule) = schedule {
            schedule();
        }
    }
    let changed = hub.cloud.with_personal_status(|cloud| {
        hub.personal.with(|f| {
            let changed = cloud.connected || f.socket_live;
            f.socket_live = false;
            cloud.connected = false;
            hub.cloud_synced.store(false, Ordering::Release);
            f.socket_epoch = f.socket_epoch.checked_add(1).context("personal socket counter exhausted")?;
            Ok(changed)
        })
    });
    if changed.unwrap_or(false) {
        notify(hub);
    }
}
pub(super) fn socket(hub: &Hub) -> Result<u64> {
    guard(hub, None, true, |_| Ok(()))?;
    let epoch = hub.cloud.with_personal_status(|cloud| {
        hub.personal.with(|f| {
            let a = f.applied.as_mut().context("personal snapshot absent")?;
            f.journal.as_ref().context("root journal absent")?.matches_applied(&a.record)?;
            ensure!(
                cloud.owner.as_ref().is_some_and(|o| o.user_id == a.record.binding.owner),
                "personal owner changed"
            );
            let db = hub.store.db.locked();
            live::guard(&db, &f.config, &a.record, &mut a.lease, &mut f.clock, true)?;
            f.socket_epoch = f.socket_epoch.checked_add(1).context("personal socket epoch exhausted")?;
            f.socket_live = true;
            cloud.connected = true;
            hub.cloud_synced.store(true, Ordering::Release);
            Ok(f.socket_epoch)
        })
    })?;
    notify(hub);
    Ok(epoch)
}
pub(super) fn restore(hub: &Hub) -> Result<()> {
    hub.personal.refresh()?; // FS fora de Cloud/runtime/DB.
    let saved = hub.personal.with(|f| {
        if f.applied.is_some() {
            return Ok(None);
        }
        let db = hub.store.db.locked();
        Ok(live::current(&db)?.map(|r| (f.config.clone(), r)))
    })?;
    let Some((cfg, (record, accepted))) = saved else {
        return Ok(());
    };
    let now = hub.personal.with(|f| f.clock.validity())?;
    wire::ceremony(
        &cfg.verifier(&record.binding.app)?,
        &serde_json::to_vec(&record.originals)?,
        wire::Time::Historical(now),
    )?;
    let mut lease = Lease::restore(&cfg, &record.lease)?;
    hub.cloud.with_personal_status(|cloud| {
        hub.personal.with(|f| {
            ensure!(
                f.config.original_bytes() == cfg.original_bytes() && f.applied.is_none(),
                "personal restore changed"
            );
            f.journal.as_ref().context("root journal absent")?.matches_applied(&record)?;
            let db = hub.store.db.locked();
            live::guard(&db, &f.config, &record, &mut lease, &mut f.clock, false)?;
            ensure!(cloud.owner.as_ref().is_none_or(|o| o.user_id == record.binding.owner), "personal owner conflict");
            // Owner provém do E assinado e atual, nunca de IDs isolados/ausência de estado.
            cloud.owner = Some(Owner { user_id: record.binding.owner.clone(), email: None });
            cloud.connected = false;
            f.socket_live = false;
            hub.cloud_synced.store(false, Ordering::Release);
            f.applied = Some(Applied { record, lease, accepted });
            Ok(())
        })
    })
}
async fn receipt(hub: &Hub, id: &str, reconcile_same: bool) -> Result<()> {
    let body = ceremony::step(hub, id, |cfg, clock, s| {
        let c = ceremony::history(cfg, s.originals(), clock.validity()?)?;
        let r = c.original(Kind::Receipt)?;
        let body = json!({"grant":protocol::canonical(c.original(Kind::Grant)?.original_bytes())?,"challenge":protocol::canonical(c.original(Kind::Challenge)?.original_bytes())?,"proof":protocol::canonical(c.original(Kind::Proof)?.original_bytes())?,"receipt":protocol::canonical(r.original_bytes())?});
        // possibleSend FULL durável. Uma falha só permite Read dos mesmos originais.
        Ok((live::effect(s.db, id, "receipt", &serde_json::to_vec(&body)?)? || reconcile_same).then_some(body))
    })?;
    if let Some(body) = body {
        transport::post(hub, "receipt", body).await?;
    }
    Ok(())
}
async fn send_activation(hub: &Hub) -> Result<()> {
    let body = hub.cloud.with_personal_status(|cloud| {
        hub.personal.with(|f| {
            let a = f.applied.as_mut().context("personal snapshot absent")?;
            f.journal.as_ref().context("root journal absent")?.matches_applied(&a.record)?;
            ensure!(
                cloud.owner.as_ref().is_some_and(|o| o.user_id == a.record.binding.owner),
                "personal owner changed"
            );
            let db = hub.store.db.locked();
            db.pragma_update(None, "synchronous", "FULL")?;
            let tx = db.unchecked_transaction()?;
            live::guard(&tx, &f.config, &a.record, &mut a.lease, &mut f.clock, false)?;
            let changed = tx.execute(
                "INSERT OR IGNORE INTO personal_activation_sends VALUES(?,?,?)",
                rusqlite::params![a.record.binding.capture, a.record.sequence, a.record.activation],
            )?;
            let old: Vec<u8> = tx.query_row(
                "SELECT activation FROM personal_activation_sends WHERE capture=? AND sequence=?",
                rusqlite::params![a.record.binding.capture, a.record.sequence],
                |r| r.get(0),
            )?;
            ensure!(old == a.record.activation, "personal activation send conflict");
            let body = json!({"activation":protocol::canonical(&a.record.activation)?});
            tx.commit()?;
            Ok((changed == 1).then_some(body))
        })
    })?;
    if let Some(body) = body {
        transport::post(hub, "activation", body).await?;
    }
    Ok(())
}
fn accept_activation(hub: &Hub, id: &str, reply: &Value) -> Result<()> {
    hub.cloud.with_personal_status(|cloud| {
        hub.personal.with(|f| {
            let a = f.applied.as_mut().context("personal V absent")?;
            ensure!(
                a.record.binding.capture == id
                    && reply["activationCurrent"] == protocol::canonical(&a.record.activation)?
                    && reply["state"][0][5] == "hubActive"
                    && reply["state"][0][10] == wire::hash(&protocol::canonical(&a.record.activation)?[0])?,
                "personal activation not current"
            );
            f.journal.as_ref().context("root journal absent")?.matches_applied(&a.record)?;
            ensure!(
                cloud.owner.as_ref().is_some_and(|o| o.user_id == a.record.binding.owner),
                "personal owner changed"
            );
            let db = hub.store.db.locked();
            db.pragma_update(None, "synchronous", "FULL")?;
            let tx = db.unchecked_transaction()?;
            live::guard(&tx, &f.config, &a.record, &mut a.lease, &mut f.clock, false)?;
            ensure!(
                tx.execute(
                    "UPDATE personal_live_current SET accepted=1 WHERE singleton=1 AND capture=? AND sequence=?",
                    rusqlite::params![id, a.record.sequence]
                )? == 1,
                "personal acceptance CAS"
            );
            a.lease.current()?;
            tx.commit()?;
            a.accepted = true;
            Ok(())
        })
    })
}
pub(super) async fn cycle(hub: &Hub, id: &str) -> Result<bool> {
    hub.personal.refresh()?;
    let has_receipt = ceremony::step(hub, id, |_, _, s| Ok(s.originals().get("receipt").is_some()))?;
    if !has_receipt {
        return Ok(false);
    }
    receipt(hub, id, false).await?;
    let Some(reply) = ceremony::read_state(hub, id).await? else {
        return Ok(false);
    };
    if reply["originals"]["receipt"].is_null() {
        // Exceção fechada de reconciliação: Worker receipt aceita SOMENTE o mesmo R
        // committed e idempotente, inclusive para materializar W superior com R.
        // S ausente NÃO prova não envio. Não altera marker/bytes/nonce de cerimônia.
        receipt(hub, id, true).await?;
        return Ok(false);
    }
    if reply["originals"]["fence"].is_array() {
        cut(hub);
        return Ok(false);
    }
    let applied = hub.personal.with(|f| {
        if let Some(a) = &mut f.applied {
            Ok(!a.accepted || a.lease.until()?.saturating_sub(f.clock.validity()?) > 60_000)
        } else {
            let db = hub.store.db.locked();
            ensure!(live::current(&db)?.is_none_or(|(_, accepted)| accepted), "pending activation outcome unknown");
            Ok(false)
        }
    })?;
    if applied {
        let activation = guard(hub, None, false, |r| protocol::canonical(&r.activation))?;
        if reply["activationCurrent"] != activation {
            send_activation(hub).await?;
            return Ok(false);
        }
        accept_activation(hub, id, &reply)?;
        return Ok(true);
    }
    if !reply["originals"]["publish"].is_array() {
        return Ok(false);
    }
    let (cfg, flight, request) = hub.personal.with(|f| {
        let packet = {
            let db = hub.store.db.locked();
            ceremony::packet(&live::session(&db, id, &f.config)?)
        };
        let c = ceremony::history(&f.config, &packet, f.clock.validity()?)?;
        let r = c.original(Kind::Receipt)?;
        f.clock.issued_after(r.payload()[7].as_u64().context("R time")?, 9_007_199_254_710_991)?;
        let binding = SnapshotBinding {
            h_t: c.digest().into(),
            h_g: c.original(Kind::Grant)?.digest().into(),
            grant: r.payload()[6].as_str().context("grant")?.into(),
            h_r: r.digest().into(),
        };
        let (flight, request) = SnapshotFlight::request(&f.config, &hub.identity, &mut f.clock, binding)?;
        Ok((f.config.clone(), flight, request))
    })?;
    let response = transport::post(hub, "host-state", json!({"stateRequest":protocol::canonical(&request)?})).await?;
    ensure!(
        response.as_object().is_some_and(|r| r.len() == 1 && r.contains_key("hostState")),
        "host-state response fields"
    );
    let mut lease = flight.accept(&cfg, &serde_json::to_vec(&response["hostState"])?)?;
    hub.personal.refresh()?;
    let e = protocol::canonical(lease.snapshot())?;
    ceremony::wait_causal(hub, e[0][16].as_u64().context("E time")?, lease.until()?).await?;
    hub.personal.refresh()?; // Releitura após a espera causal, antes do draft.
    // Plan/cifra/assinatura fora da trava Cloud. A transação final revalida as versões.
    let (root_revision, root_hash, now) = hub.personal.with(|f| {
        let root = f.journal.as_ref().context("root journal absent")?;
        Ok((root.revision(), protocol::hash(root.original_bytes()), f.clock.validity()?))
    })?;
    let draft = live::plan(hub, &cfg, id, root_revision, root_hash.clone(), &mut lease, now)?;
    let payload = hub.personal.with(|f| {
        let e = protocol::canonical(&draft.record.snapshot)?;
        let at = f.clock.issued_after(e[0][16].as_u64().context("E time")?, lease.until()?)?;
        let b = &draft.record.binding;
        Ok(json!([
            wire::domain("activation"),
            f.config.context(),
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
        ]))
    })?;
    let activation = serde_json::to_vec(&transport::signed(hub, payload)?)?;
    let certified = live::certify(&cfg, draft, &activation, hub.personal.with(|f| f.clock.validity())?)?;
    promote(hub, &cfg, &certified, lease, root_revision, &root_hash)?;
    send_activation(hub).await?;
    // Um novo Read nonce-bound (poll mínimo 5s) confirmará V; publicação não conecta.
    Ok(false)
}

pub(super) fn promote(
    hub: &Hub,
    cfg: &super::provisioning::AdmittedConfig,
    certified: &live::Certified,
    mut lease: Lease,
    root_revision: u64,
    root_hash: &str,
) -> Result<()> {
    hub.cloud.with_personal_status(|cloud| {
        hub.personal.with(|f| {
            ensure!(f.config.original_bytes() == cfg.original_bytes(), "personal promotion config changed");
            let root = f.journal.as_ref().context("root journal absent")?;
            ensure!(
                root.revision() == root_revision && protocol::hash(root.original_bytes()) == root_hash,
                "root changed before promotion"
            );
            ensure!(
                cloud.owner.as_ref().is_none_or(|o| o.user_id == certified.binding().owner),
                "personal owner conflict"
            );
            root.matches_applied(certified.record_for_root())?;
            let record = {
                let db = hub.store.db.locked();
                live::commit(&db, &f.config, certified, &mut lease, &mut f.clock)?
            };
            cloud.owner = Some(Owner { user_id: record.binding.owner.clone(), email: None });
            cloud.connected = false;
            f.applied = Some(Applied { record, lease, accepted: false });
            f.socket_live = false;
            hub.cloud_synced.store(false, Ordering::Release);
            Ok(())
        })
    })?;
    hub.auth_changed();
    notify(hub);
    Ok(())
}
