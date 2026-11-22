//! Estado e capacidades reais de onboarding da 1a instalacao.
//!
//! O Hub comprova apenas o que de fato existe no computador/cofre/cadastro; o estado de
//! navegacao do assistente e isolado por conta/contexto e jamais fabrica prontidao.

#[cfg(test)]
mod tests;

use crate::LockExt;
use crate::{
    agent::backends,
    api::devices::Caller,
    fork::{
        engine_accounts::{Registry, State},
        engine_setup,
        runtime_audiencia::screen as screen_projection,
    },
    hub::Hub,
    store::{Store, now_ms},
};
use anyhow::{Result, anyhow, ensure};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fmt::Write as _, sync::Arc, time::Duration};

pub(crate) const STATUS_METHOD: &str = "donnaOnboardingStatus";
pub(crate) const SAVE_METHOD: &str = "donnaOnboardingSave";
pub(crate) const DETECT_METHOD: &str = "donnaOnboardingDetect";
pub(crate) const METHODS: [&str; 3] = ["donnaOnboardingStatus", "donnaOnboardingSave", "donnaOnboardingDetect"];

const KV_PREFIX: &str = "fork.onboarding.v1.";
const MAX_ACCOUNT_KEY: usize = 128;
const MAX_SPACE_ID: usize = 128;

#[cfg(test)]
pub(super) static OLLAMA_PROBE_URL: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WhereChoice {
    Local,
    Remote,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PersistedOnboarding {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) step: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) where_choice: Option<WhereChoice>,
    #[serde(default)]
    pub(crate) ai_skipped: bool,
    #[serde(default)]
    pub(crate) completed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) updated_at: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StatusQuery {
    #[serde(default)]
    account_key: Option<String>,
    #[serde(default)]
    space_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SaveRequest {
    #[serde(default)]
    account_key: Option<String>,
    #[serde(default)]
    space_id: Option<String>,
    #[serde(default)]
    step: Option<u8>,
    #[serde(default, rename = "where")]
    where_choice: Option<WhereChoice>,
    #[serde(default)]
    ai_skipped: Option<bool>,
    #[serde(default)]
    deferred_before_hub: bool,
    #[serde(default)]
    completed: Option<bool>,
}

fn normalize_account_key(raw: Option<&str>) -> Result<String> {
    let key = raw.map(str::trim).unwrap_or_default();
    if key.is_empty() {
        return Ok("default".to_owned());
    }
    ensure!(
        key.len() <= MAX_ACCOUNT_KEY && !key.contains('\0') && key.chars().all(|c| !c.is_control()),
        "invalid_field: identificador de conta invalido."
    );
    Ok(key.to_owned())
}

fn normalize_space_id(raw: Option<&str>) -> Result<String> {
    let space = raw.map(str::trim).unwrap_or_default();
    if space.is_empty() {
        return Ok(crate::fork::spaces::PERSONAL.to_owned());
    }
    ensure!(
        space.len() <= MAX_SPACE_ID && !space.contains('\0') && space.chars().all(|c| !c.is_control()),
        "invalid_field: spaceId invalido."
    );
    Ok(space.to_owned())
}

pub(crate) fn kv_key(account_key: &str, space_id: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(account_key.as_bytes());
    hash.update([0]);
    hash.update(space_id.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in hash.finalize() {
        let _ = write!(hex, "{byte:02x}");
    }
    format!("{KV_PREFIX}{hex}")
}

fn read_persisted(store: &Store, key: &str) -> Result<Option<PersistedOnboarding>> {
    let Some(raw) = store.kv_read(key)? else {
        return Ok(None);
    };
    let parsed: PersistedOnboarding =
        serde_json::from_str(&raw).map_err(|_| anyhow!("onboarding_unavailable: estado salvo invalido."))?;
    Ok(Some(parsed))
}

fn build_status(hub: &Hub, caller: &Caller, account_key: &str, space_id: &str) -> Result<Value> {
    let open = crate::environment::Environment::is_open();
    let can_manage_accounts = engine_setup::access::resolve(hub, caller).is_ok();
    let is_local = matches!(caller, Caller::Local);
    let motor_available =
        crate::fork::motor_mexerica::enabled() && cfg!(target_os = "linux") && crate::fork::motor_exec::configured();
    let local_keys_oauth = motor_available && is_local && !open;

    let mut registry = Registry::load(&hub.store)?;
    if open {
        registry.accounts.retain(engine_setup::is_open_account);
    }
    let connected_accounts = if can_manage_accounts {
        registry.accounts.iter().filter(|account| account.state == State::Connected).count()
    } else {
        0
    };

    let personal_space = space_id == crate::fork::spaces::PERSONAL;
    let key_providers = if open || !can_manage_accounts || !personal_space || !motor_available {
        Vec::new()
    } else {
        crate::fork::motor_mexerica::onboarding_ai_summary(&hub.store)?
    };
    let configured_keys = key_providers.len();
    let has_ready_ai = connected_accounts > 0 || configured_keys > 0;

    let screen = screen_projection::state(hub, space_id);
    let screen_connected = screen["connected"] == true;
    let screen_enabled = screen["enabled"] == true;
    let screen_capture = screen["capture"] == true;
    let screen_input = screen["input"] == true;
    let screen_platform = screen["platform"].as_str();
    let screen_available = screen_connected || screen_enabled;
    let screen_ready = if screen_platform == Some("windows") {
        screen_enabled && screen_connected
    } else {
        screen_enabled && screen_connected && screen_capture && screen_input
    };

    let hub_ready = personal_space && hub.store.team_settings().is_ok_and(|settings| settings.primary_bot_id.is_some());

    let mut items = vec![
        json!({ "id": "hub", "ready": hub_ready, "available": true }),
        json!({ "id": "ai", "ready": has_ready_ai, "available": true }),
    ];
    if !open {
        items.push(json!({ "id": "voice", "ready": false, "available": false }));
        items.push(json!({ "id": "computer", "ready": screen_ready, "available": screen_available }));
    }

    let available_items: Vec<&Value> = items.iter().filter(|item| item["available"] == true).collect();
    let total_count = available_items.len();
    let ready_count = available_items.iter().filter(|item| item["ready"] == true).count();

    let key = kv_key(account_key, space_id);
    let persisted = read_persisted(&hub.store, &key)?;
    let spaces = hub.store.spaces().ok();
    let personal_space_id = spaces.as_ref().map(|snapshot| snapshot.personal_space_id.as_str());

    Ok(json!({
        "hubReady": hub_ready,
        "hostOs": std::env::consts::OS,
        "spaceContext": {
            "currentSpaceId": space_id,
            "personalSpaceId": personal_space_id,
        },
        "capabilities": {
            "motorMexerica": motor_available,
            "localKeysAndOAuth": local_keys_oauth,
            "engineAccounts": can_manage_accounts,
            "computerControl": screen_available,
            "voice": !open,
        },
        "ai": {
            "hasReadyAi": has_ready_ai,
            "connectedAccounts": connected_accounts,
            "configuredKeys": configured_keys,
            "providers": key_providers,
        },
        "voice": {
            "configured": false,
        },
        "computerAccess": {
            "available": screen_available,
            "enabled": screen_enabled,
            "connected": screen_connected,
            "capture": screen_capture,
            "input": screen_input,
            "ready": screen_ready,
        },
        "readiness": {
            "readyCount": ready_count,
            "totalCount": total_count,
            "items": items,
        },
        "persisted": persisted,
    }))
}

async fn probe_ollama_loopback(base_url: Option<&str>) -> Value {
    let installed_binary = backends::which("ollama").is_some();
    let url = if let Some(custom) = base_url {
        format!("{}/api/tags", custom.trim_end_matches('/'))
    } else {
        if cfg!(test) {
            return json!({
                "installed": installed_binary,
                "reachable": false,
                "modelsCount": 0,
            });
        }
        "http://127.0.0.1:11434/api/tags".to_owned()
    };
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_millis(800))
        .build();
    let Ok(client) = client else {
        return json!({
            "installed": installed_binary,
            "reachable": false,
            "modelsCount": 0,
        });
    };
    let Ok(mut resp) = client.get(&url).send().await else {
        return json!({
            "installed": installed_binary,
            "reachable": false,
            "modelsCount": 0,
        });
    };
    if !resp.status().is_success() {
        return json!({
            "installed": installed_binary,
            "reachable": false,
            "modelsCount": 0,
        });
    }
    let unavailable = json!({ "installed": installed_binary, "reachable": false, "modelsCount": 0 });
    let mut bytes = Vec::new();
    loop {
        match resp.chunk().await {
            Ok(Some(chunk)) if bytes.len() + chunk.len() <= 64 * 1024 => bytes.extend_from_slice(&chunk),
            Ok(None) => break,
            _ => return unavailable,
        }
    }
    let Ok(body) = serde_json::from_slice::<Value>(&bytes) else {
        return json!({
            "installed": installed_binary,
            "reachable": false,
            "modelsCount": 0,
        });
    };
    let Some(models) = body["models"].as_array() else {
        return json!({
            "installed": installed_binary,
            "reachable": false,
            "modelsCount": 0,
        });
    };
    let models_count = models.len();
    json!({
        "installed": installed_binary || models_count > 0,
        "reachable": true,
        "modelsCount": models_count,
    })
}

async fn detect_readonly(hub: &Arc<Hub>, caller: &Caller) -> Result<Value> {
    engine_setup::require_local(caller)?;
    let open = crate::environment::Environment::is_open();
    let backend_list = tokio::task::spawn_blocking(backends::list).await?;
    let mut registry = Registry::load(&hub.store)?;
    if open {
        registry.accounts.retain(engine_setup::is_open_account);
    }
    let can_manage_accounts = engine_setup::access::resolve(hub, caller).is_ok();
    let motor_available =
        crate::fork::motor_mexerica::enabled() && cfg!(target_os = "linux") && crate::fork::motor_exec::configured();
    let key_providers = if open || !can_manage_accounts || !motor_available {
        Vec::new()
    } else {
        crate::fork::motor_mexerica::onboarding_ai_summary(&hub.store)?
    };

    let ollama_url: Option<String> = {
        #[cfg(test)]
        {
            OLLAMA_PROBE_URL.lock().ok().and_then(|guard| guard.clone())
        }
        #[cfg(not(test))]
        {
            None
        }
    };
    let ollama = probe_ollama_loopback(ollama_url.as_deref()).await;

    let mut engines = Vec::new();
    for b in backend_list {
        let Some(id) = b["id"].as_str() else { continue };
        if !matches!(id, "codex" | "claude" | "grok" | "mexerica") {
            continue;
        }
        if open && id != "codex" {
            continue;
        }
        let connected =
            registry.accounts.iter().filter(|a| a.backend.id() == id && a.state == State::Connected).count();
        engines.push(json!({
            "id": id,
            "name": b["name"],
            "installed": b["installed"] == true,
            "canInstall": b["canInstall"] == true,
            "signedIn": b["signedIn"],
            "node": b["node"],
            "connectedAccounts": connected,
        }));
    }

    Ok(json!({
        "readOnly": true,
        "extractedCredentials": false,
        "engines": engines,
        "configuredKeyProviders": key_providers,
        "ollama": ollama,
    }))
}

pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    match method {
        STATUS_METHOD => {
            engine_setup::require_local(caller)?;
            let query: StatusQuery =
                serde_json::from_value(body).map_err(|_| anyhow!("invalid_field: consulta de onboarding invalida."))?;
            let account_key = normalize_account_key(query.account_key.as_deref())?;
            let space_id = normalize_space_id(query.space_id.as_deref())?;
            build_status(hub, caller, &account_key, &space_id)
        }
        SAVE_METHOD => {
            engine_setup::require_local(caller)?;
            let req: SaveRequest =
                serde_json::from_value(body).map_err(|_| anyhow!("invalid_field: pedido de onboarding invalido."))?;
            if let Some(step) = req.step {
                ensure!((1..=4).contains(&step), "invalid_field: etapa de onboarding invalida.");
            }
            let account_key = normalize_account_key(req.account_key.as_deref())?;
            let space_id = normalize_space_id(req.space_id.as_deref())?;
            if req.deferred_before_hub {
                ensure!(
                    req.account_key.as_deref().is_some_and(|key| !key.trim().is_empty())
                        && req.space_id.as_deref() == Some(crate::fork::spaces::PERSONAL)
                        && req.step.is_none()
                        && req.where_choice.is_none()
                        && req.ai_skipped.is_none()
                        && req.completed.is_none(),
                    "invalid_field: adiamento inicial exige conta e Space pessoal, sem outras alterações."
                );
            }
            let key = kv_key(&account_key, &space_id);
            let now = now_ms();
            let mut db = hub.store.db.locked();
            let tx = db.transaction()?;
            let existing_raw: Option<String> =
                tx.query_row("SELECT v FROM kv WHERE k = ?1", [&key], |r| r.get(0)).optional()?;
            if req.deferred_before_hub && existing_raw.is_some() {
                drop(tx);
                drop(db);
                return build_status(hub, caller, &account_key, &space_id);
            }
            let mut state = match existing_raw {
                Some(raw) => serde_json::from_str::<PersistedOnboarding>(&raw)
                    .map_err(|_| anyhow!("onboarding_unavailable: estado salvo invalido."))?,
                None => PersistedOnboarding::default(),
            };
            if let Some(step) = req.step {
                state.step = Some(step);
            }
            if let Some(where_choice) = req.where_choice {
                state.where_choice = Some(where_choice);
            }
            if req.deferred_before_hub {
                state.ai_skipped = true;
            } else if let Some(ai_skipped) = req.ai_skipped {
                state.ai_skipped = ai_skipped;
            }
            if let Some(completed) = req.completed {
                state.completed = completed;
            }
            state.updated_at = Some(now);
            let serialized = serde_json::to_string(&state)?;
            tx.execute(
                "INSERT INTO kv(k, v) VALUES(?1, ?2) ON CONFLICT(k) DO UPDATE SET v = excluded.v",
                rusqlite::params![key, serialized],
            )?;
            tx.commit()?;
            drop(db);
            build_status(hub, caller, &account_key, &space_id)
        }
        DETECT_METHOD => detect_readonly(hub, caller).await,
        _ => Err(anyhow!("metodo desconhecido: {method}")),
    }
}
