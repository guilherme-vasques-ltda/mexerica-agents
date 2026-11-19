//! The agent process: which commands start it, the ACP handshake, the bot's model, and how its instructions are
//! attached to a session request.

use super::{Actor, Adapter, CompactionUsage, Conn, SessionCaps};
use crate::agent::acp::{self, Acp};
use crate::fork::activity_code::ActivityKind;
use crate::store::BotConfig;
use anyhow::{Result, anyhow, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// ACP's `systemPrompt`: the bot's instructions added to the agent's own.
pub(crate) fn system_prompt(system: &str) -> Value {
    json!({"mode": "append", "content": [{"type": "text", "text": system}]})
}

/// Preserva as regras de ferramentas do Claude Code, reconciliando a identidade pelo append.
pub(crate) fn claude_meta(system: &str, cfg: &BotConfig) -> Value {
    let mut meta = json!({"systemPrompt": {"type": "preset", "preset": "claude_code", "append": system}, "claudeCode": {"options": {
        "settings": {
            "permissions": {"defaultMode": "default", "allow": std::iter::once(crate::agent::harness::read_rule())
                .chain(crate::agent::engine_permissions::MCP_ALLOW_RULES.iter().map(|rule| (*rule).to_owned()))
                .collect::<Vec<_>>()},
            "includeGitInstructions": false,
            "attribution": {"commit": "", "pr": ""},
            "autoMemoryEnabled": false,
            "autoDreamEnabled": false,
        },
        "settingSources": [], "strictMcpConfig": true,
        "disallowedTools": crate::fork::claude_tools::disallowed(),
        "permissionMode": "default", "allowDangerouslySkipPermissions": false,
    }}});
    crate::chat::memory::engine::protect_claude(&mut meta, cfg);
    meta
}

impl Actor {
    /// Compatibilidade com adaptadores que ainda ignoram permissionMode nas opções de criação.
    /// Só ao abrir/recarregar, e só se o catálogo anunciar outro modo; nunca um RPC por turno.
    pub(super) async fn manual_mode(&mut self, sid: &str, res: Value) -> Result<Value> {
        let adapter = self.conn.as_ref().map(|c| c.adapter);
        let expected = match adapter {
            Some(Adapter::Claude) => "default",
            Some(Adapter::Codex) => "workspace-write",
            _ => return Ok(res),
        };
        let Some(mode) = mode_option(&res) else {
            if adapter == Some(Adapter::Codex) {
                if let Some(conn) = self.conn.take() {
                    conn.kill().await;
                }
                anyhow::bail!("o adaptador Codex não anunciou o modo seguro");
            }
            return Ok(res);
        };
        if mode["currentValue"] == expected {
            return Ok(res);
        }
        let id =
            mode["id"].as_str().ok_or_else(|| anyhow!("o adaptador não anunciou o identificador do modo seguro"))?;
        let result = self
            .session_request("session/set_config_option", json!({"sessionId": sid, "configId": id, "value": expected}))
            .await;
        let result = result.and_then(|answer| {
            verify_mode(&answer, expected)?;
            Ok(answer)
        });
        if result.is_err()
            && let Some(conn) = self.conn.take()
        {
            conn.kill().await;
        }
        result
    }
}

fn mode_option(res: &Value) -> Option<&Value> {
    res["configOptions"].as_array()?.iter().find(|o| o["category"] == "mode")
}

fn verify_mode(res: &Value, expected: &str) -> Result<()> {
    ensure!(
        mode_option(res).is_some_and(|o| o["currentValue"] == expected),
        "o adaptador não confirmou o modo seguro {expected}"
    );
    Ok(())
}

#[cfg(test)]
fn verify_codex_mode(res: &Value) -> Result<()> {
    verify_mode(res, "workspace-write")
}

pub(crate) fn verify_codex_background_mode(res: &Value) -> Result<()> {
    verify_mode(res, "read-only")
}

/// Shell commands that may start the bot's agent over ACP, best first.
/// `progress` receives short status lines with their stable code ("Downloading Cursor…", `downloading`).
pub(crate) async fn launch_commands(
    cfg: &BotConfig,
    progress: impl Fn(&str, Option<ActivityKind>),
) -> Result<Vec<String>> {
    if cfg.backend == "omp" {
        return Err(EngineNotReady(
            "O motor oh-my-pi não está disponível. Escolha outro motor nas configurações do bot.".into(),
        )
        .into());
    }
    crate::agent::launcher::check(cfg).map_err(|e| EngineNotReady::wrap(&e))?;
    if cfg.backend == "codex" && cfg.fork_editor.engine_account_id.is_none() {
        crate::agent::codex_audit::check_user_config().map_err(|e| EngineNotReady::wrap(&e))?;
    }
    if cfg.fork_editor.engine_account_id.is_some() {
        return crate::fork::engine_accounts::managed::adapter_commands(&cfg.backend).await;
    }
    Ok(match cfg.command.as_deref().map(str::trim) {
        Some(c) if !c.is_empty() => vec![c.to_owned()],
        _ => crate::agent::backends::launch_candidates(&cfg.backend, progress)
            .await
            .map_err(|e| EngineNotReady::wrap(&e))?
            .iter()
            .map(crate::agent::registry::Cmd::acp)
            .collect(),
    })
}

/// Motor sem login/instalação no caminho legado (sem conta gerenciada). O texto é o do erro original (nada é
/// perdido para o log nem para `needs_sign_in`); o tipo só deixa o Hub classificar o motivo sem ler a mensagem.
#[derive(Debug)]
pub(crate) struct EngineNotReady(pub(crate) String);

impl EngineNotReady {
    fn wrap(error: &anyhow::Error) -> anyhow::Error {
        Self(format!("{error:#}")).into()
    }
}

impl std::fmt::Display for EngineNotReady {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for EngineNotReady {}

/// Switches a session (`session/new` or `session/load` answered `res`) to the bot's
/// chosen model, then sets that model's options (effort, fast…). Returns the answer holding
/// the session's config options now: a model can change them.
pub(crate) async fn select_model(acp: &Acp, res: &Value, sid: &str, cfg: &BotConfig) -> Result<Value> {
    let Some(model) = cfg.model.as_deref().filter(|m| !m.is_empty()) else { return Ok(res.clone()) };
    let Some(config_id) = crate::agent::auth::model_config(res).and_then(|c| c["id"].as_str()) else {
        let r = acp.request("session/set_model", json!({"sessionId": sid, "modelId": model})).await;
        return r.map(|_| res.clone()).map_err(|e| model_error(model, &e));
    };
    let (model, options) = model_choice(res, model, &cfg.model_options);
    let set = |id: &str, value: &str| {
        acp.request("session/set_config_option", json!({"sessionId": sid, "configId": id, "value": value}))
    };
    let mut current = set(config_id, model).await.map_err(|e| model_error(model, &e))?;
    for (id, value) in options {
        let answer = set(id, value).await.map_err(|e| model_error(&format!("{model} with {id}={value}"), &e))?;
        if answer["configOptions"].is_array() {
            current = answer;
        }
    }
    Ok(if current["configOptions"].is_array() { current } else { res.clone() })
}

/// The model to select and the options to set on it: the model id's variant, then the bot's own. How the agent
/// approves tools is the bot's Approvals, never a model option (`opus[mode=acceptEdits]`).
fn model_choice<'a>(
    res: &Value,
    model: &'a str,
    own: &'a BTreeMap<String, String>,
) -> (&'a str, Vec<(&'a str, &'a str)>) {
    let (model, mut options) = split_variant(res, model);
    options.extend(own.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    let mode = res["configOptions"].as_array().into_iter().flatten().find(|o| o["category"] == "mode");
    options.retain(|(id, _)| mode.is_none_or(|m| m["id"] != *id));
    (model, options)
}

fn model_error(model: &str, e: &anyhow::Error) -> anyhow::Error {
    tracing::warn!(model, error = format!("{e:#}"), "couldn't set model");
    anyhow!("Couldn't select model {model}: {e:#}")
}

/// Cursor names a model variant `grok-4.7[reasoning_effort=high,fast=true]`. When the agent
/// lists plain models instead, the bracket becomes options of the plain model.
fn split_variant<'a>(res: &Value, model: &'a str) -> (&'a str, Vec<(&'a str, &'a str)>) {
    let listed = |id: &str| crate::agent::auth::model_ids(res).iter().any(|m| m == id);
    if let Some((name, params)) = model.strip_suffix(']').and_then(|m| m.split_once('['))
        && let Some(params) =
            params.split(',').filter(|p| !p.is_empty()).map(|p| p.split_once('=')).collect::<Option<Vec<_>>>()
        && !listed(model)
        && listed(name)
    {
        return (name, params);
    }
    (model, Vec::new())
}

/// Motor interno (backend `mexerica`): sem comando nem handshake — o "agente"
/// é o adaptador ACP em processo do Hub, preso ao cofre (o canal `fd 3` do
/// motor) e ao cartão de aprovação existente. `extra_env` é só para provas.
pub(crate) async fn start_mexerica(hub: &Arc<crate::hub::Hub>, extra_env: &[(&str, &str)]) -> Result<Conn> {
    #[cfg(not(unix))]
    {
        let _ = (hub, extra_env);
        anyhow::bail!("Motor Mexerica indisponível nesta plataforma")
    }
    #[cfg(unix)]
    {
        // Flag desligada (ou sem ajudante): nada abre, nem servidores MCP.
        crate::fork::motor_mexerica::ensure_runnable()?;
        let vault = crate::fork::motor_mexerica::hub_vault(hub.clone());
        let link = crate::fork::motor_mexerica::link::Link::new(vault, extra_env);
        let (acp, rx) = Acp::spawn_link(link);
        Ok(Conn {
            ponte: None,
            curated: crate::fork::conectores::Pool::default(),
            acp,
            rx,
            skill_reads: tokio::task::JoinSet::new(),
            sessions: SessionCaps { load: false, close: true, system_prompt: true },
            adapter: Adapter::Mexerica,
            adapter_version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            codex_config: false,
            config_warnings: Vec::new(),
            loaded: HashMap::new(),
            started: Instant::now(),
            fast_options: HashMap::new(),
            reports_compaction: false,
            compaction_usage: CompactionUsage::Idle,
            usage: HashMap::new(),
        })
    }
}

/// Spawns an ACP agent and completes the `initialize` handshake.
pub(crate) async fn start_agent(command: &str, cwd: &str, env: &[(String, String)], budget: Duration) -> Result<Conn> {
    start_agent_mode(command, cwd, env, budget, false, false).await
}

pub(crate) async fn start_agent_protected(
    command: &str,
    cwd: &str,
    env: &[(String, String)],
    budget: Duration,
) -> Result<Conn> {
    start_agent_mode(command, cwd, env, budget, true, false).await
}

pub(crate) async fn start_background_agent(
    command: &str,
    cwd: &str,
    env: &[(String, String)],
    budget: Duration,
) -> Result<Conn> {
    start_agent_mode(command, cwd, env, budget, false, true).await
}

async fn stop_failed(acp: &Acp, wait: bool) {
    if wait {
        acp.kill_and_wait().await;
    } else {
        acp.kill().await;
    }
}

async fn start_agent_mode(
    command: &str,
    cwd: &str,
    env: &[(String, String)],
    budget: Duration,
    protected: bool,
    wait_on_failure: bool,
) -> Result<Conn> {
    let (acp, rx) = if protected {
        Acp::spawn_protected(command, cwd, env)?
    } else if wait_on_failure {
        Acp::spawn_background(command, cwd, env)?
    } else {
        Acp::spawn(command, cwd, env)?
    };
    let init = tokio::time::timeout(
        budget,
        acp.request(
            "initialize",
            json!({
                "protocolVersion": 1,
                // `session.compaction`: tell us when the agent summarizes its context.
                // `parameterizedModelPicker`: Cursor lists plain models with their options
                // (effort, fast…) instead of one fixed variant each; model discovery asks the same.
                "clientCapabilities": {
                    "fs": {"readTextFile": false, "writeTextFile": false},
                    "terminal": false,
                    "session": {"compaction": {}},
                    "_meta": {"parameterizedModelPicker": true},
                },
                "clientInfo": {"name": "codync", "title": "Donnabot", "version": env!("CARGO_PKG_VERSION")},
            }),
        ),
    )
    .await;
    let init = match init {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            stop_failed(&acp, wait_on_failure).await;
            return Err(e);
        }
        Err(_) => {
            stop_failed(&acp, wait_on_failure).await;
            let message = format!("the agent didn't answer the ACP handshake within {}s", budget.as_secs());
            return Err(acp::Failure::error(acp::FailureKind::Timeout, message));
        }
    };
    let adapter = adapter(&init);
    if wait_on_failure && adapter == Adapter::Claude && !crate::fork::background_adapter::matches(&init) {
        stop_failed(&acp, wait_on_failure).await;
        return Err(crate::fork::background_notice::Problem::Version.into());
    }
    let codex_config = env.iter().any(|(k, _)| k == "CODEX_CONFIG");
    if adapter == Adapter::Codex && !codex_config {
        stop_failed(&acp, wait_on_failure).await;
        anyhow::bail!("Selecione o motor Codex gerenciado pela Mexerica para aplicar suas políticas de segurança.");
    }
    let config_warnings = if adapter == Adapter::Codex {
        match env
            .iter()
            .rev()
            .find(|(key, _)| key == "CODEX_HOME")
            .map_or_else(crate::agent::codex_audit::check_user_config, |(_, home)| {
                crate::agent::codex_audit::check(std::path::Path::new(home))
            }) {
            Ok(warnings) => warnings,
            Err(error) => {
                stop_failed(&acp, wait_on_failure).await;
                return Err(error);
            }
        }
    } else {
        Vec::new()
    };
    for warning in &config_warnings {
        tracing::warn!("{}", warning.message());
    }
    let sessions = SessionCaps {
        load: init["agentCapabilities"]["loadSession"].as_bool().unwrap_or(false),
        close: init["agentCapabilities"]["sessionCapabilities"]["close"].is_object(),
        system_prompt: init["agentCapabilities"]["sessionCapabilities"]["systemPrompt"].is_object(),
    };
    Ok(Conn {
        ponte: None,
        curated: crate::fork::conectores::Pool::default(),
        acp,
        rx,
        skill_reads: tokio::task::JoinSet::new(),
        sessions,
        adapter,
        adapter_version: init["agentInfo"]["version"].as_str().map(str::to_owned),
        codex_config,
        config_warnings,
        loaded: HashMap::new(),
        started: Instant::now(),
        fast_options: HashMap::new(),
        reports_compaction: false,
        compaction_usage: CompactionUsage::Idle,
        usage: HashMap::new(),
    })
}

fn adapter(init: &Value) -> Adapter {
    if init["agentCapabilities"]["_meta"]["claudeCode"].is_object() {
        Adapter::Claude
    } else if init["agentInfo"]["name"].as_str().is_some_and(|name| name.ends_with("codex-acp")) {
        Adapter::Codex
    } else {
        Adapter::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn omp_salvo_e_recusado_antes_de_executar() {
        let cfg: BotConfig =
            serde_json::from_value(json!({"id":"omp-antigo", "name":"Antigo", "backend":"omp"})).unwrap();
        let erro = launch_commands(&cfg, |_, _| {}).await.unwrap_err().to_string();
        assert!(erro.contains("oh-my-pi") && erro.contains("Escolha outro motor"));
    }

    #[test]
    fn e2_guia_cabe_na_leitura_do_perfil_restrito_sem_conceder_ferramentas() {
        let cfg: BotConfig =
            serde_json::from_value(json!({"id":"e2-restrito", "name":"Restrito", "permission":"ask"})).unwrap();
        let system = crate::agent::harness::compile();
        let meta = claude_meta(&system, &cfg);
        let options = &meta["claudeCode"]["options"];
        assert_eq!(options["permissionMode"], "default");
        assert_eq!(options["allowDangerouslySkipPermissions"], false);
        let docs = crate::agent::harness::docs_path().parent().unwrap().to_path_buf();
        let entry = docs.join("extensibilidade/criador-de-skills/SKILL.md");
        assert!(system.contains(&entry.display().to_string()));
        let allow = options["settings"]["permissions"]["allow"].as_array().unwrap();
        assert!(allow.contains(&json!(crate::agent::harness::read_rule())));
        assert!(allow.iter().all(|v| v.as_str().unwrap().starts_with("Read(")));
        assert!(cfg.skills.is_empty() && cfg.connectors.is_empty() && !cfg.computer);
        assert!(!system.contains("# Criador de skills da Mexerica"), "corpo não entra no prompt estável");
    }

    #[test]
    fn claude_preserva_preset_isola_settings_e_exige_confirmacao_do_modo() {
        let cfg: BotConfig =
            serde_json::from_value(json!({"id":"memory-policy","name":"Memory","sharedMemory":true})).unwrap();
        let meta = claude_meta("identidade", &cfg);
        assert_eq!(meta["systemPrompt"], json!({"type":"preset", "preset":"claude_code", "append":"identidade"}));
        let options = &meta["claudeCode"]["options"];
        for layer in [Some("memory-policy"), None] {
            let path = crate::chat::memory::files::folder(layer).display().to_string();
            let absolute = format!("//{}", path.trim_start_matches('/'));
            let permissions = &options["settings"]["permissions"];
            assert!(permissions["allow"].as_array().unwrap().contains(&json!(format!("Read({absolute}/**)"))));
            for tool in ["Edit", "Write", "NotebookEdit"] {
                assert!(permissions["deny"].as_array().unwrap().contains(&json!(format!("{tool}({absolute}/**)"))));
            }
        }
        assert_eq!(options["settingSources"], json!([]));
        assert_eq!(options["strictMcpConfig"], true);
        assert_eq!(options["permissionMode"], "default");
        assert_eq!(options["allowDangerouslySkipPermissions"], false);
        assert_eq!(options["settings"]["autoMemoryEnabled"], false);
        assert_eq!(options["settings"]["autoDreamEnabled"], false);
        // O manual se lê sem aprovação, e só se lê: nenhuma regra de escrita.
        let allow = options["settings"]["permissions"]["allow"].to_string();
        assert!(allow.contains("Read(//") && allow.contains("/docs/**)"), "{allow}");
        assert!(!allow.contains("Edit") && !allow.contains("Write"));
        let servers: Vec<_> = options["settings"]["permissions"]["allow"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|value| value.as_str()?.strip_prefix("mcp__"))
            .collect();
        assert_eq!(servers, Vec::<&str>::new());
        assert!(
            verify_mode(
                &json!({"configOptions": [{"id":"permission-mode", "category":"mode", "currentValue":"default"}]}),
                "default"
            )
            .is_ok()
        );
        for mode in ["bypassPermissions", "acceptEdits", "auto"] {
            assert!(
                verify_mode(
                    &json!({"configOptions": [{"id":"permission-mode", "category":"mode", "currentValue":mode}]}),
                    "default"
                )
                .is_err()
            );
        }
        assert!(verify_mode(&json!({}), "default").is_err(), "resposta vazia não confirma o modo");
        assert!(verify_mode(&json!({"configOptions":[]}), "default").is_err());
        assert!(mode_option(&json!({"configOptions":[{"id":"mode", "category":"other"}]})).is_none());
    }

    #[test]
    fn codex_exige_modo_sem_revisor() {
        for mode in ["agent", "agent-full-access", "read-only"] {
            assert!(
                verify_codex_mode(&json!({"configOptions":[{"id":"mode", "category":"mode", "currentValue":mode}]}))
                    .is_err()
            );
        }
        assert!(verify_codex_mode(&json!({})).is_err());
        assert!(
            verify_codex_mode(
                &json!({"configOptions":[{"id":"mode", "category":"mode", "currentValue":"workspace-write"}]})
            )
            .is_ok()
        );
    }

    #[test]
    fn variant_ids_become_options_only_when_the_agent_lists_plain_models() {
        let plain = json!({"configOptions": [{"id": "model", "category": "model", "type": "select",
            "options": [{"value": "grok-4.7"}]}]});
        let variant = "grok-4.7[context=256k,reasoning_effort=medium,fast=false]";
        assert_eq!(
            split_variant(&plain, variant),
            ("grok-4.7", vec![("context", "256k"), ("reasoning_effort", "medium"), ("fast", "false")])
        );
        assert_eq!(split_variant(&plain, "grok-4.7"), ("grok-4.7", vec![]));
        let variants = json!({"configOptions": [{"id": "model", "category": "model", "type": "select",
            "options": [{"value": variant}]}]});
        assert_eq!(split_variant(&variants, variant), (variant, vec![]));
        assert_eq!(split_variant(&plain, "other[x=1]"), ("other[x=1]", vec![]));
        // Claude's `opus[1m]` is a model id of its own, not a variant.
        assert_eq!(split_variant(&json!({"models": {"availableModels": [{"modelId": "opus"}]}}), "opus[1m]").1, vec![]);
        // Cursor's Auto: `default[]` with variants, `default` without.
        let auto = json!({"models": {"availableModels": [{"modelId": "default"}]}});
        assert_eq!(split_variant(&auto, "default[]"), ("default", vec![]));
    }

    #[test]
    fn no_model_option_sets_how_the_agent_approves_tools() {
        let res = json!({"configOptions": [
            {"id": "model", "category": "model", "type": "select", "options": [{"value": "opus"}]},
            {"id": "mode", "category": "mode", "type": "select", "options": [{"value": "default"}, {"value": "auto"}]},
        ]});
        let own = [("mode", "auto"), ("effort", "high")].map(|(k, v)| (k.to_owned(), v.to_owned())).into();
        assert_eq!(model_choice(&res, "opus[mode=acceptEdits]", &own), ("opus", vec![("effort", "high")]));
    }
}
