#[cfg(unix)]
use super::{BrowserPreference, supervisor::Scope};
#[cfg(unix)]
use crate::store::Store;
use crate::{LockExt, api::devices::Caller, hub::Hub};
use anyhow::{Context, Result, ensure};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) const STATE: &str = "donnaBrowserLocalState";
pub(crate) const CONSENT: &str = "donnaBrowserLocalConsent";
pub(crate) const FRAME: &str = "donnaBrowserLocalFrame";
pub(crate) const METHODS: [&str; 3] = [STATE, CONSENT, FRAME];
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) const MAX_FRAME_CHARS: usize = 350_000;
const CONSENT_KEY: &str = "browser.mac.consent.v1";

pub(super) fn enabled_on(db: &Connection) -> bool {
    db.query_row("SELECT v FROM kv WHERE k = ?1", [CONSENT_KEY], |r| r.get::<_, String>(0))
        .is_ok_and(|value| value == "yes")
}

pub(super) fn availability(hub: &Hub) -> Result<(), &'static str> {
    if cfg!(target_os = "macos") && !enabled_on(&hub.store.db.locked()) {
        return Err("local_consent_required");
    }
    hub.browser.availability()
}

#[cfg(unix)]
pub(super) async fn availability_async(hub: &std::sync::Arc<Hub>) -> Result<(), &'static str> {
    let hub = hub.clone();
    tokio::task::spawn_blocking(move || availability(&hub)).await.unwrap_or(Err("containment_unavailable"))
}

#[cfg(target_os = "macos")]
async fn local_backend(hub: &Hub) -> Result<(), &'static str> {
    hub.browser.availability()?;
    super::supervisor::mac_policy::verify().await
}

#[cfg(not(target_os = "macos"))]
fn local_backend(hub: &Hub) -> impl Future<Output = Result<(), &'static str>> {
    std::future::ready(hub.browser.availability())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Consent {
    enabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FrameRequest {
    bot_id: String,
    space_id: String,
}

pub(crate) async fn call(hub: &Hub, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    ensure!(matches!(caller, Caller::Local), crate::api::devices::Forbidden);
    match method {
        STATE => {
            ensure!(body.as_object().is_some_and(serde_json::Map::is_empty), "invalid_field");
            let enabled = hub.store.kv_read(CONSENT_KEY)?.as_deref() == Some("yes");
            let backend = local_backend(hub).await;
            Ok(
                json!({"supported":cfg!(target_os = "macos") || backend.is_ok(), "canChange":cfg!(target_os = "macos"), "enabled":cfg!(target_os = "linux") || enabled, "ready": backend.is_ok(), "reason": backend.err()}),
            )
        }
        CONSENT => {
            ensure!(cfg!(target_os = "macos"), "browser_platform_unsupported");
            let Consent { enabled } = serde_json::from_value(body).context("invalid_field")?;
            let _guard = hub.browser.local_consent.lock().await;
            if enabled {
                local_backend(hub).await.map_err(anyhow::Error::msg)?;
            }
            hub.store.kv_set(CONSENT_KEY, if enabled { "yes" } else { "no" })?;
            if !enabled {
                hub.browser.consent_revision.fetch_add(1, std::sync::atomic::Ordering::Release);
                hub.browser.revoke_all();
            }
            Ok(json!({"enabled":enabled}))
        }
        FRAME => frame(hub, serde_json::from_value(body).context("invalid_field")?).await,
        _ => anyhow::bail!("browser_local_unknown"),
    }
}

#[cfg(unix)]
pub(super) fn frame_scope(db: &Connection, scope: &Scope) -> bool {
    if cfg!(target_os = "macos") && !enabled_on(db) {
        return false;
    }
    if crate::fork::spaces::require_space(db, &scope.space).is_err() {
        return false;
    }
    Store::bot_on(db, &scope.bot).ok().flatten().is_some_and(|row| {
        !row.deleted
            && !row.config.is_group()
            && row.config.space_id == scope.space
            && row.config.browser_preference == BrowserPreference::Integrated
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
async fn frame(hub: &Hub, request: FrameRequest) -> Result<Value> {
    let scope = Scope { hub: hub.host_id.clone(), space: request.space_id, bot: request.bot_id };
    ensure!(frame_scope(&hub.store.db.locked(), &scope), "browser_scope_unavailable");
    let Some((owner, instance)) = hub.browser.visible_session(&scope) else {
        return Ok(json!({"jpeg":null}));
    };
    hub.browser
        .capture(&scope, &owner, instance, || {
            super::authority_valid(hub, &scope, &owner)
                && hub.browser.visible_session(&scope) == Some((owner.clone(), instance))
        })
        .await
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
async fn frame(_hub: &Hub, request: FrameRequest) -> Result<Value> {
    let _ = (request.space_id, request.bot_id);
    anyhow::bail!("browser_platform_unsupported")
}
