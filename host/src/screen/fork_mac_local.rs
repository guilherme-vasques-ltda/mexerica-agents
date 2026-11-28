use super::{Screen, fork_mac_identity::macos::Xpc};
use crate::{LockExt, hub::Hub};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

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
    fn private_screen_does_not_fall_back_to_remote_viewing() {
        for method in ["screenPrepare", "screenOffer", "screenClose", "screenTakeover"] {
            assert!(super::legacy_remote_method(method));
        }
        assert!(!super::legacy_remote_method("screenStatus"));
        assert!(!super::legacy_remote_method("requestScreenPermission"));
    }
}
