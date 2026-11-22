//! Estado e capacidades reais de onboarding da 1a instalacao.
//!
//! O Hub comprova o que existe no computador/cofre/cadastro, sem expor progresso por conta.

#[cfg(test)]
mod tests;

use crate::LockExt;
use crate::{
    agent::backends,
    api::devices::Caller,
    fork::{
        engine_accounts::{Billing, LaunchReference, Registry, State, managed},
        engine_setup,
        runtime_audiencia::screen as screen_projection,
    },
    hub::Hub,
    store::{BotConfig, Lane, Store},
};
use anyhow::{Result, anyhow, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

pub(crate) const STATUS_METHOD: &str = "donnaOnboardingStatus";
pub(crate) const SAVE_METHOD: &str = "donnaOnboardingSave";
pub(crate) const DETECT_METHOD: &str = "donnaOnboardingDetect";
pub(crate) const METHODS: [&str; 3] = ["donnaOnboardingStatus", "donnaOnboardingSave", "donnaOnboardingDetect"];

const MAX_SPACE_ID: usize = 128;

#[cfg(test)]
pub(super) static OLLAMA_PROBE_URL: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StatusQuery {
    space_id: Option<String>,
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

fn berga_ai_configured(
    store: &Store,
    bot: &BotConfig,
    registry: &Registry,
    motor_available: bool,
    key_providers: &[String],
) -> bool {
    if bot.backend == crate::fork::motor_mexerica::ENGINE_ID {
        #[cfg(unix)]
        if motor_available
            && bot.command.is_none()
            && bot.fork_editor.engine_account_id.is_none()
            && let Some(model) = bot.model.as_deref()
            && let Ok((provider, model)) = crate::fork::motor_mexerica::link::provider_of(model)
        {
            return key_providers.contains(&provider)
                && crate::fork::motor_mexerica::chaves::modelo_no_catalogo(store, &provider, &model).unwrap_or(false);
        }
        #[cfg(not(unix))]
        let _ = (motor_available, key_providers);
        return false;
    }
    let Some(account) =
        bot.fork_editor.engine_account_id.as_deref().and_then(|id| registry.accounts.iter().find(|a| a.id == id))
    else {
        return false;
    };
    if (account.billing == Billing::Subscription && !account.subscription_verified())
        || managed::validate_selection_in(registry, bot).is_err()
        || engine_setup::in_setup(store, &account.id)
    {
        return false;
    }
    match &account.launch {
        LaunchReference::CliProfile { executable, directory } => {
            crate::fork::motor_install::executable(executable)
                && if account.managed.is_some() || managed::migrated_legacy(account) {
                    managed::validate_profile(store, &account.id, directory).is_ok()
                } else {
                    directory.is_dir()
                }
        }
        LaunchReference::VaultProfile { slot } => {
            store.kv_read(slot).is_ok_and(|slot| slot.is_some())
                && backends::which(account.backend.id()).is_some()
                && account
                    .managed
                    .as_ref()
                    .and_then(|meta| meta.profile.as_ref())
                    .is_none_or(|profile| managed::validate_profile(store, &account.id, profile).is_ok())
        }
    }
}

fn build_status(hub: &Hub, caller: &Caller, space_id: &str) -> Result<Value> {
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
        registry
            .accounts
            .iter()
            .filter(|account| {
                account.state == State::Connected
                    && (account.billing != Billing::Subscription || account.subscription_verified())
            })
            .count()
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
    let berga = if personal_space {
        match hub.store.team_settings()?.primary_bot_id {
            Some(id) => hub
                .store
                .bot(&id)?
                .filter(|row| !row.deleted && !row.config.is_group() && row.config.space_id == space_id),
            None => None,
        }
    } else {
        None
    };
    let has_ready_ai = can_manage_accounts
        && berga.as_ref().is_some_and(|row| {
            let mut config = row.config.clone();
            let lane = Lane::main(&config.id);
            let selection =
                crate::fork::sessoes::admissao::apply_configured(&hub.store.db.locked(), &lane, &mut config);
            selection.is_ok() && berga_ai_configured(&hub.store, &config, &registry, local_keys_oauth, &key_providers)
        });

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

    let hub_ready = berga.is_some();

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
        "persisted": Value::Null,
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

async fn read_status(hub: &Arc<Hub>, caller: &Caller, space_id: String) -> Result<Value> {
    let hub = hub.clone();
    let caller = caller.clone();
    tokio::task::spawn_blocking(move || build_status(&hub, &caller, &space_id)).await?
}

pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    match method {
        STATUS_METHOD => {
            engine_setup::require_local(caller)?;
            let query: StatusQuery =
                serde_json::from_value(body).map_err(|_| anyhow!("invalid_field: consulta de onboarding invalida."))?;
            let space_id = normalize_space_id(query.space_id.as_deref())?;
            read_status(hub, caller, space_id).await
        }
        SAVE_METHOD => Err(crate::api::devices::Forbidden.into()),
        DETECT_METHOD => detect_readonly(hub, caller).await,
        _ => Err(anyhow!("metodo desconhecido: {method}")),
    }
}
