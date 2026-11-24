// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Methods about this computer itself: host updates, hello/sync, agents and setup
//! terminals, usage, folders, voice and the remote screen.

use super::devices::Caller;
use super::str_arg;
use crate::LockExt;
use crate::agent::backends;
use crate::fork::runtime_audiencia::screen as screen_projection;
use crate::hub::Hub;
use crate::usage;
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::ops::ControlFlow;
use std::sync::Arc;

pub(super) async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, b: Value) -> Result<ControlFlow<Value, Value>> {
    let space = b["spaceId"].as_str().unwrap_or(crate::fork::spaces::PERSONAL);
    Ok(ControlFlow::Break(match method {
        "hostUpdateStatus" => crate::update::status()?,
        "checkHostUpdate" => crate::update::check().await?,
        "setHostAutomaticUpdates" => {
            crate::update::set_automatic(b["enabled"].as_bool().context("enabled must be a boolean")?)?
        }
        "installHostUpdate" => {
            let port = hub.port;
            let force = b["force"].as_bool().unwrap_or(false);
            if !force && hub.busy() {
                bail!("host is busy; retry when idle or explicitly allow interruption");
            }
            tokio::task::spawn_blocking(move || crate::update::spawn_worker(port, force)).await??;
            json!({"scheduled": true})
        }
        "voiceStatus" => crate::voice::status(&hub.store)?,
        "setVoiceKey" => {
            let provider = crate::voice::Provider::parse(str_arg(&b, "provider")?)?;
            crate::voice::set_key(&hub.store, provider, b["key"].as_str().unwrap_or_default()).await?
        }
        "voiceSession" => {
            crate::fork::sessao_clerk::negar_sem_corte(hub, caller, "voiceSession")?;
            let provider = crate::voice::Provider::parse(str_arg(&b, "provider")?)?;
            crate::voice::session(&hub.store, provider, str_arg(&b, "model")?, str_arg(&b, "voice")?).await?
        }
        "voiceModels" => {
            crate::voice::models(&hub.store, crate::voice::Provider::parse(str_arg(&b, "provider")?)?).await?
        }
        "voiceTranscribe" => {
            let provider = crate::voice::Provider::parse(str_arg(&b, "provider")?)?;
            crate::voice::transcribe(
                &hub.store,
                provider,
                str_arg(&b, "model")?,
                str_arg(&b, "audio")?,
                b["mime"].as_str(),
            )
            .await?
        }
        "voiceSpeak" => {
            let provider = crate::voice::Provider::parse(str_arg(&b, "provider")?)?;
            crate::voice::speak(
                &hub.store,
                provider,
                str_arg(&b, "model")?,
                str_arg(&b, "voice")?,
                str_arg(&b, "text")?,
            )
            .await?
        }
        "hello" => {
            let port = hub.port;
            let device = tokio::task::spawn_blocking(crate::service::device).await?;
            // Conclui o I/O antes de projetar a identidade atual da tela.
            let urls = tokio::task::spawn_blocking(move || crate::service::addresses(port)).await?;
            json!({
                "hostId": hub.host_id,
                "computerId": hub.identity.computer_id(),
                "signKey": hub.identity.sign_pub_b64(),
                "boxKey": hub.identity.box_pub_b64(),
                "cloud": crate::remote::cloud::url(&hub.store),
                "name": crate::service::host_name(),
                "version": env!("CARGO_PKG_VERSION"),
                "minApp": crate::compat::MIN_APP,
                "os": std::env::consts::OS,
                "device": device,
                "home": dirs::home_dir().map(|p| p.to_string_lossy().into_owned()),
                "backends": backends::list(),
                "rev": hub.store.current_rev(),
                "screen": screen_projection::state(hub, space),
                "analytics": hub.analytics.state(),
                "avatarEditing": true,
                "avatarConfigVersion": 2,
                "spacesContract": 1,
                "spaceBirthContract": 1,
                "spaceLifecycleContract": 1,
                "spaceId": b["spaceId"].as_str().unwrap_or(crate::fork::spaces::PERSONAL),
                "conversations": hub.store.space_conversations(b["spaceId"].as_str().unwrap_or(crate::fork::spaces::PERSONAL))?,
                "spaces": hub.store.spaces()?,
                "urls": urls,
            })
        }
        "refreshBackends" => {
            tokio::task::spawn_blocking(backends::hydrate_path).await?;
            backends::refresh_sign_in(&hub.store).await;
            json!({"backends": backends::list()})
        }
        "usage" => {
            if b["refresh"].as_bool().unwrap_or(false) {
                usage::refresh(hub).await;
            }
            serde_json::to_value(&*hub.usage.locked())?
        }
        "listDirs" => {
            let path = b["path"].as_str().map(std::path::PathBuf::from);
            tokio::task::spawn_blocking(move || list_dirs(path)).await??
        }
        "agentSetup" => {
            let step: crate::agent::term::Step =
                serde_json::from_value(b["step"].clone()).context("`step` is install or login")?;
            let (cols, rows) = term_size(&b);
            // Node.js belongs to the computer, not to one agent.
            let backend = if step == crate::agent::term::Step::Node { "" } else { str_arg(&b, "backend")? };
            crate::fork::motores_retirados::exigir_oferecido(backend)?;
            backends::require_supported_execution(backend)?;
            let id = if step == crate::agent::term::Step::Login && !matches!(caller, Caller::Local) {
                anyhow::ensure!(b.get("engineAccountId").is_none(), "Use o login da conta em Contas de IA.");
                hub.terms
                    .start_login(
                        hub,
                        caller,
                        backend,
                        b["spaceId"].as_str().unwrap_or(crate::fork::spaces::PERSONAL),
                        cols,
                        rows,
                    )
                    .await?
            } else {
                hub.terms.start(caller, backend, step, b["method"].as_str(), cols, rows).await?
            };
            json!({"term":id, "restrictedLinks":step == crate::agent::term::Step::Login && !matches!(caller, Caller::Local)})
        }
        "agentModels" => {
            let backend = str_arg(&b, "backend")?;
            crate::fork::motores_retirados::exigir_oferecido(backend)?;
            if backend == crate::fork::antigravity::ID {
                return Ok(ControlFlow::Break(json!({"models":[]})));
            }
            backends::require_supported_execution(backend)?;
            // O motor próprio não tem programa para sondar: os modelos são os do último «Testar acesso» das chaves.
            if backend == crate::fork::motor_mexerica::ENGINE_ID {
                crate::fork::motor_mexerica::catalogo(
                    &hub.store,
                    b["spaceId"].as_str().unwrap_or(crate::fork::spaces::PERSONAL),
                )?
            } else {
                let env = crate::fork::terminal_login::runtime_env(
                    &hub.store,
                    backend,
                    b["spaceId"].as_str().unwrap_or(crate::fork::spaces::PERSONAL),
                )?;
                let descoberta = if env.is_empty() {
                    crate::agent::auth::models(&hub.store, backend).await
                } else {
                    crate::agent::auth::models_with_env(&hub.store, backend, &env).await
                };
                // Os planos de código ligados pela chave também aparecem na escolha de modelo do OpenCode, só no Space
                // pessoal (nos demais o bot é recusado). Se a descoberta do OpenCode falha, os modelos do plano ainda
                // valem; sem nenhum deles, o erro da descoberta segue para a tela.
                let planos = backend == "opencode"
                    && b["spaceId"].as_str().unwrap_or(crate::fork::spaces::PERSONAL) == crate::fork::spaces::PERSONAL;
                let (mut catalog, falha) = match descoberta {
                    Ok(catalog) => (catalog, None),
                    Err(erro) if planos => (json!({"models": []}), Some(erro)),
                    Err(erro) => return Err(erro),
                };
                if planos {
                    crate::fork::planos_codigo::acrescentar_modelos(&hub.store, &mut catalog)?;
                }
                if let Some(erro) = falha
                    && catalog["models"].as_array().is_some_and(Vec::is_empty)
                {
                    return Err(erro);
                }
                catalog
            }
        }
        "agentAuth" | "agentAuthenticate" | "setAgentEnv" => {
            let backend = str_arg(&b, "backend")?;
            crate::fork::motores_retirados::exigir_oferecido(backend)?;
            let status = match method {
                "agentAuth" if matches!(caller, Caller::Local) => {
                    crate::agent::auth::check(&hub.store, backend).await?
                }
                "agentAuth" => match crate::fork::terminal_login::status(
                    hub,
                    backend,
                    b["spaceId"].as_str().unwrap_or(crate::fork::spaces::PERSONAL),
                )
                .await?
                {
                    Some(value) => value,
                    None => crate::agent::auth::check(&hub.store, backend).await?,
                },
                "agentAuthenticate" => {
                    crate::fork::engine_setup::require_local(caller)?;
                    crate::agent::auth::authenticate(&hub.store, backend, str_arg(&b, "method")?).await?
                }
                _ => {
                    let vars = b["vars"].as_object().ok_or_else(|| anyhow!("`vars` is required"))?;
                    crate::agent::auth::set_env(&hub.store, backend, vars).await?
                }
            };
            if matches!(caller, Caller::Local) && status["signedIn"] == true {
                crate::market::requests::agent_signed_in(hub, backend)?;
            }
            status
        }
        "termInput" => {
            hub.terms.authorized(str_arg(&b, "term")?, caller)?;
            hub.terms.write(str_arg(&b, "term")?, str_arg(&b, "data")?)?;
            json!({})
        }
        "termResize" => {
            hub.terms.authorized(str_arg(&b, "term")?, caller)?;
            let (cols, rows) = term_size(&b);
            hub.terms.resize(str_arg(&b, "term")?, cols, rows)?;
            json!({})
        }
        "termClose" => {
            hub.terms.authorized(str_arg(&b, "term")?, caller)?;
            hub.terms.close(str_arg(&b, "term")?);
            json!({})
        }
        "screenStatus" => {
            let screen = screen_projection::state(hub, space);
            #[cfg(target_os = "linux")]
            if matches!(caller, Caller::Local)
                && let Some(notice) = hub.screen.portal_restore_notice()
            {
                let mut screen = screen;
                screen["portalRestoreNotice"] = json!(notice);
                return Ok(ControlFlow::Break(screen));
            }
            screen
        }
        "requestScreenPermission" => {
            hub.screen.request_permission(&b).await?;
            screen_projection::state(hub, space)
        }
        "screenPrepare" => {
            crate::fork::sessao_clerk::negar_sem_corte(hub, caller, "screenPrepare")?;
            if !hub.screen.enabled() {
                bail!("Remote screen is turned off on this computer.");
            }
            let ice = if b["relay"].as_bool().unwrap_or(false) {
                crate::remote::cloud::screen_ice(hub, caller.device_key()).await?
            } else {
                crate::screen::IceConfig::default()
            };
            let result = hub.screen.prepare(caller.device_key(), ice)?;
            let session = result["session"].as_str().context("missing screen session")?.to_owned();
            tokio::spawn(crate::screen::watch_viewer(hub.clone(), session, caller.device_key().to_owned()));
            result
        }
        "screenOffer" => {
            crate::fork::sessao_clerk::negar_sem_corte(hub, caller, "screenOffer")?;
            let display = b["display"].as_u64().and_then(|d| u32::try_from(d).ok());
            hub.screen.offer(caller.device_key(), str_arg(&b, "sdp")?, str_arg(&b, "session")?, display).await?
        }
        "screenClose" => {
            let session = str_arg(&b, "session")?;
            if hub.screen.owns_session(session, caller.device_key()) {
                hub.screen.close(session).await?;
            }
            json!({})
        }
        "screenTakeover" => {
            crate::fork::sessao_clerk::negar_sem_corte(hub, caller, "screenTakeover")?;
            hub.screen.takeover(b["on"].as_bool().unwrap_or(false));
            screen_projection::state(hub, space)
        }
        "setAnalytics" => {
            hub.analytics.set_enabled(&hub.store, b["enabled"].as_bool().unwrap_or(false))?;
            json!({"enabled": hub.analytics.state()})
        }
        "track" => {
            crate::analytics::track(hub, &b)?;
            json!({})
        }
        "reportError" => {
            crate::analytics::errors::report(hub, &b)?;
            json!({})
        }
        "setScreenEnabled" => {
            hub.screen.set_enabled(&hub.store, b["enabled"].as_bool().unwrap_or(false)).await?;
            screen_projection::state(hub, space)
        }
        "computerTools" => crate::screen::computer_tools(hub).await,
        _ => return Ok(ControlFlow::Continue(b)),
    }))
}

/// Blocking (`std::fs`): call through `spawn_blocking`.
fn list_dirs(path: Option<std::path::PathBuf>) -> Result<Value> {
    let dir = match path {
        Some(p) => p,
        None => dirs::home_dir().ok_or_else(|| anyhow!("no home directory"))?,
    };
    let mut dirs: Vec<Value> = std::fs::read_dir(&dir)
        .with_context(|| format!("listing {}", dir.display()))?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let p = e.path();
            Some(json!({"name": name, "path": p.to_string_lossy(), "isGit": p.join(".git").exists()}))
        })
        .collect();
    dirs.sort_by_key(|d| d["name"].as_str().unwrap_or_default().to_lowercase());
    Ok(json!({
        "path": dir.to_string_lossy(),
        "parent": dir.parent().map(|p| p.to_string_lossy().into_owned()),
        "isGit": dir.join(".git").exists(),
        "dirs": dirs,
    }))
}

fn term_size(b: &Value) -> (u16, u16) {
    let dim = |k: &str, default: u16| b[k].as_u64().and_then(|v| u16::try_from(v).ok()).unwrap_or(default);
    (dim("cols", 80), dim("rows", 24))
}
