use super::{Screen, fork_mac_identity::macos::Xpc};
use crate::{LockExt, agent::bot::Cmd, api::devices::Caller, hub::Hub, store::Store};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

pub(crate) const FIELD: &str = "macLocalScreenEnabled";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Change {
    id: String,
    mac_local_screen_enabled: bool,
}

pub(crate) fn update_consent(hub: &Hub, caller: &Caller, body: Value) -> Result<Value> {
    ensure!(is_private(hub) && matches!(caller, Caller::Local), "screen settings require the local app");
    let change: Change = serde_json::from_value(body).context("invalid screen setting")?;
    let owner = crate::fork::autoridade::Autoridade::capturar(hub, caller)?;
    let _emit = hub.emit_lock.locked();
    let changed = owner.with_db(hub, None, |db| {
        let row = Store::bot_on(db, &change.id)?.context("unknown bot")?;
        ensure!(
            !row.deleted && !row.config.is_group() && !row.config.hidden,
            "screen settings unavailable for this bot"
        );
        if super::fork_mac_consent::consent_on(db, &change.id)
            .is_ok_and(|current| current.is_some() == change.mac_local_screen_enabled)
        {
            return Ok(false);
        }
        super::fork_mac_consent::set_on(db, &change.id, change.mac_local_screen_enabled)?;
        let rev = crate::store::next_rev(db)?;
        db.execute("UPDATE bots SET rev=?2 WHERE id=?1", rusqlite::params![change.id, rev])?;
        Ok(true)
    })?;
    if changed {
        let _ = hub.send_cmd(&change.id, Cmd::RefreshTools);
    }
    let row = hub.store.bot(&change.id)?.context("unknown bot")?;
    let bot = hub.bot_json(&row);
    if changed {
        let _ = hub.events.send(json!({"type":"bot","rev":bot["rev"],"bot":bot}));
    }
    Ok(json!({"bot":bot}))
}

pub(crate) fn is_private(hub: &Hub) -> bool {
    hub.desktop_socket.get().is_some()
}

pub(crate) fn bot_enabled(hub: &Hub, bot: &str) -> bool {
    super::fork_mac_consent::consent_on(&hub.store.db.locked(), bot).is_ok_and(|value| value.is_some())
}

pub(crate) async fn observe(screen: Arc<Screen>) {
    let Ok(Ok(xpc)) = tokio::task::spawn_blocking(Xpc::packaged).await else {
        tracing::warn!("packaged screen helper identity unavailable");
        return;
    };
    let xpc = Arc::new(xpc);
    if screen.mac_xpc.set(xpc.clone()).is_err() {
        return;
    }
    loop {
        let latest = match xpc.send(&json!({"method":"status"})) {
            Ok(call) => tokio::task::spawn_blocking(move || call.wait(2)).await.ok().and_then(Result::ok),
            Err(_) => None,
        }
        .and_then(|reply| {
            let result = reply.get("result")?;
            Some(super::protocol::HelperStatus {
                platform: "macos".into(),
                permission_app: "Donnabot Dev Screen".into(),
                capture: result.get("capture")?.as_bool()?,
                input: result.get("input")?.as_bool()?,
                ..Default::default()
            })
        });
        let changed = {
            let mut live = screen.mac_status.locked();
            let changed = *live != latest;
            *live = latest;
            changed
        };
        if changed {
            screen.emit();
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

pub(crate) fn set_enabled(hub: &Hub, enabled: bool) -> Result<()> {
    let mut db = hub.store.db.locked();
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    super::fork_mac_consent::set_global_on(&tx, enabled)?;
    tx.commit()?;
    *hub.screen.enabled.locked() = Some(enabled);
    drop(db);
    hub.screen.emit();
    Ok(())
}

pub(crate) async fn request_permission(screen: &Screen, permission: &str) -> Result<()> {
    ensure!(matches!(permission, "capture" | "input"), "unsupported screen permission");
    let xpc = screen.mac_xpc.get().context("authenticated screen helper unavailable")?;
    let call = xpc.send(&json!({"method":"requestPermission","permission":permission}))?;
    let reply = tokio::task::spawn_blocking(move || call.wait(120)).await??;
    ensure!(reply["result"].is_object(), "screen permission request was refused");
    Ok(())
}

pub(crate) fn legacy_remote_method(method: &str) -> bool {
    matches!(method, "screenPrepare" | "screenOffer" | "screenClose" | "screenTakeover")
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_a_dedicated_change_can_set_screen_consent() {
        let change = serde_json::json!({"id":"a","macLocalScreenEnabled":true});
        assert!(serde_json::from_value::<super::Change>(change).is_ok());
        let mixed = serde_json::json!({"id":"a","macLocalScreenEnabled":true,"notify":false});
        assert!(serde_json::from_value::<super::Change>(mixed).is_err());
    }

    #[test]
    fn private_screen_does_not_fall_back_to_remote_viewing() {
        for method in ["screenPrepare", "screenOffer", "screenClose", "screenTakeover"] {
            assert!(super::legacy_remote_method(method));
        }
        assert!(!super::legacy_remote_method("screenStatus"));
        assert!(!super::legacy_remote_method("requestScreenPermission"));
    }
}
