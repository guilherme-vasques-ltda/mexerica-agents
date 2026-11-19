//! Isolamento prévio ao spawn para jobs Codex. Não lê nem copia credenciais de arquivo.
use anyhow::{Context, Result, bail, ensure};
use std::path::{Path, PathBuf};

/// A configuração do job não herda extensões executáveis da conversa.
pub(crate) fn codex_job_config(env: &[(String, String)], system: &str) -> Result<String> {
    let inherited = std::env::var("CODEX_CONFIG").ok();
    for raw in inherited
        .iter()
        .map(String::as_str)
        .chain(env.iter().filter(|(key, _)| key == "CODEX_CONFIG").map(|(_, value)| value.as_str()))
    {
        let value: serde_json::Value = serde_json::from_str(raw).context("CODEX_CONFIG inválido para job")?;
        let object = value.as_object().context("CODEX_CONFIG deve ser um objeto")?;
        ensure!(object.is_empty(), "CODEX_CONFIG externo não é permitido em tarefas de fundo.");
    }
    Ok(serde_json::json!({
        "developer_instructions": system,
        "compact_prompt": crate::fork::compaction_prompt::codex(),
        "approval_policy": "on-request",
        "sandbox_mode": "read-only",
        "approvals_reviewer": "user",
        "cli_auth_credentials_store": "file",
        "allow_login_shell": false,
        "shell_environment_policy": {
            "inherit": "all", "ignore_default_excludes": false,
            "exclude": crate::agent::environment::shell_excludes(),
            "include_only": crate::agent::environment::SHELL_ENV_ALLOWLIST,
            "experimental_use_profile": false
        },
        "features.memories": false,
        "memories.generate_memories": false,
        "memories.use_memories": false
    })
    .to_string())
}

pub(crate) struct PreparedJob {
    pub(crate) commands: Vec<String>,
    pub(crate) env: Vec<(String, String)>,
    isolation: Isolation,
}

impl PreparedJob {
    pub(crate) fn cwd(&self) -> String {
        self.isolation.cwd().to_string_lossy().into_owned()
    }
}

pub(crate) async fn prepare_job(
    hub: &std::sync::Arc<crate::hub::Hub>,
    cfg: &crate::store::BotConfig,
    system: &str,
) -> Result<PreparedJob> {
    anyhow::ensure!(
        matches!(cfg.backend.as_str(), "claude" | "codex"),
        "Motor sem isolamento comprovado para tarefa de fundo."
    );
    crate::fork::background_policy::enforce(cfg.backend == "codex")
        .await
        .map_err(|_| crate::fork::background_notice::Problem::Policy)?;
    let cwd = crate::service::data_dir().join("background");
    tokio::fs::create_dir_all(&cwd).await?;
    let cwd = cwd.to_string_lossy().into_owned();
    let selected = crate::fork::engine_accounts::managed::resolve_for_bot(&hub.store, cfg)?;
    if cfg.backend == "codex"
        && let Some(launch) = &selected
    {
        codex_job_config(&launch.env, system)?;
    }
    if selected.is_none() && hub.store.kv_read(&format!("agent-env:{}", cfg.backend))?.is_some() {
        crate::market::vault::unlock(hub.clone()).await?;
    }
    let mut env = if selected.is_some() { Vec::new() } else { crate::agent::auth::env(&hub.store, &cfg.backend)? };
    if selected.is_some() && cfg.backend == "codex" {
        env.push(("CODEX_CONFIG".into(), "{}".into()));
    }
    let mut commands = if cfg.backend == "claude" {
        crate::agent::launcher::check(cfg)?;
        crate::fork::background_adapter::commands()
    } else {
        crate::agent::bot::launch_commands(cfg, |_, _| {}).await?
    };
    crate::agent::own_memory::turn_off(&cfg.backend, &cwd, &mut env, &mut commands).await;
    crate::agent::harness::apply_background(&mut env, &cfg.backend, system)?;
    let selected_api_key = selected
        .as_ref()
        .is_some_and(|launch| launch.env.iter().any(|(key, value)| key == "OPENAI_API_KEY" && !value.is_empty()));
    if let Some(launch) = selected {
        if cfg.backend == "codex" {
            let mut safe = launch.env.clone();
            crate::agent::environment::retain_for_backend(&mut safe, "codex");
            safe.retain(|(key, _)| key != "CODEX_CONFIG");
            env.extend(safe);
            env.extend(launch.env.into_iter().filter(|(key, _)| matches!(key.as_str(), "CODEX_HOME" | "CODEX_PATH")));
        } else {
            env.extend(launch.env);
        }
    }
    let isolation = Isolation::prepare(&mut env, selected_api_key, cfg.backend == "codex")?;
    if cfg.backend == "claude" {
        env.retain(|(key, _)| key != "CLAUDE_CODE_MAX_OUTPUT_TOKENS");
        env.push(("CLAUDE_CODE_MAX_OUTPUT_TOKENS".into(), "4096".into()));
    }
    Ok(PreparedJob { commands, env, isolation })
}

pub(crate) struct Isolation {
    root: PathBuf,
    cwd: PathBuf,
}

impl Isolation {
    pub(crate) fn prepare(env: &mut Vec<(String, String)>, selected_api_key: bool, codex: bool) -> Result<Self> {
        // A configuração global pode adicionar servidores fora da camada do usuário.
        #[cfg(unix)]
        if codex
            && crate::fork::background_policy::policy_path(true)
                .try_exists()
                .map_err(|_| crate::fork::background_notice::Problem::Policy)?
        {
            return Err(crate::fork::background_notice::Problem::Policy.into());
        }
        #[cfg(target_os = "linux")]
        if !codex
            && crate::fork::background_policy::policy_path(false)
                .try_exists()
                .map_err(|_| crate::fork::background_notice::Problem::Policy)?
        {
            return Err(crate::fork::background_notice::Problem::Policy.into());
        }
        #[cfg(windows)]
        bail!("Isolamento de tarefa de fundo do Codex ainda não está disponível neste sistema.");
        let original = env
            .iter()
            .rev()
            .find(|(k, _)| k == "CODEX_HOME")
            .map(|(_, v)| PathBuf::from(v))
            .or_else(|| {
                if crate::fork::motor_exec::configured() {
                    Some(Path::new(crate::fork::motor_roots::MOTOR).join(".codex"))
                } else {
                    std::env::var_os("CODEX_HOME").map(PathBuf::from)
                }
            })
            .or_else(|| dirs::home_dir().map(|p| p.join(".codex")));
        let has_auth = if codex && !selected_api_key {
            if crate::fork::motor_exec::configured() {
                let profile = original.as_deref().unwrap_or_else(|| Path::new(crate::fork::motor_roots::MOTOR));
                crate::fork::motor_background::has_auth(profile)?
            } else {
                original.as_ref().is_some_and(|p| p.join("auth.json").exists())
            }
        } else {
            false
        };
        if has_auth {
            bail!(
                "Tarefa de fundo indisponível: o login Codex em arquivo não pode ser isolado sem risco de renovar a sessão original."
            );
        }
        let separate = crate::fork::motor_exec::configured();
        let isolation = if separate {
            let root = crate::fork::motor_background::create()?;
            Self { cwd: root.join("work"), root }
        } else {
            let base = std::fs::canonicalize(std::env::temp_dir()).context("pasta temporária indisponível")?;
            for ancestor in base.ancestors() {
                ensure!(!ancestor.join(".codex").exists(), "Configuração de projeto impede isolar a tarefa de fundo.");
            }
            let root = base.join(format!("donnabot-background-{}", uuid::Uuid::new_v4()));
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                std::fs::DirBuilder::new().mode(0o700).create(&root)?;
            }
            #[cfg(not(unix))]
            std::fs::create_dir(&root)?;
            let isolation = Self { cwd: root.join("work"), root };
            std::fs::create_dir(&isolation.cwd)?;
            std::fs::create_dir(isolation.root.join("codex"))?;
            isolation
        };
        let codex_dir = isolation.root.join("codex");
        env.retain(|(key, _)| key != "CODEX_HOME");
        env.push(("CODEX_HOME".into(), codex_dir.to_string_lossy().into_owned()));
        if !codex {
            env.retain(|(key, _)| !matches!(key.as_str(), "CODEX_CONFIG" | "INITIAL_AGENT_MODE"));
            env.push((
                "CODEX_CONFIG".into(),
                serde_json::json!({
                    "sandbox_mode":"read-only", "approval_policy":"on-request", "cli_auth_credentials_store":"file"
                })
                .to_string(),
            ));
            env.push(("INITIAL_AGENT_MODE".into(), "read-only".into()));
        }
        if codex {
            env.retain(|(key, _)| {
                !matches!(
                    key.as_str(),
                    "HOME" | "XDG_CONFIG_HOME" | "XDG_CACHE_HOME" | "XDG_DATA_HOME" | "XDG_STATE_HOME"
                )
            });
            env.push(("HOME".into(), isolation.root.to_string_lossy().into_owned()));
            env.push(("XDG_CONFIG_HOME".into(), isolation.root.join("config").to_string_lossy().into_owned()));
            env.push(("XDG_CACHE_HOME".into(), isolation.root.join("cache").to_string_lossy().into_owned()));
            env.push(("XDG_DATA_HOME".into(), isolation.root.join("data").to_string_lossy().into_owned()));
            env.push(("XDG_STATE_HOME".into(), isolation.root.join("state").to_string_lossy().into_owned()));
        }
        Ok(isolation)
    }

    pub(crate) fn cwd(&self) -> &Path {
        &self.cwd
    }
}

impl Drop for Isolation {
    fn drop(&mut self) {
        let result = if crate::fork::motor_exec::configured() {
            crate::fork::motor_background::remove(&self.root)
        } else {
            std::fs::remove_dir_all(&self.root).map_err(Into::into)
        };
        if let Err(error) = result {
            tracing::warn!(%error, "não foi possível limpar a pasta privada do job");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recusa_extensoes_executaveis_e_constroi_config_fechada() {
        for raw in [
            r#"{"model_providers":{"x":{"auth":{"command":"escrever"}}}}"#,
            r#"{"model_providers.x.auth.command":"escrever"}"#,
            r#"{"plugins":{"x":{}}}"#,
            r#"{"marketplaces":{"x":{}}}"#,
        ] {
            assert!(codex_job_config(&[("CODEX_CONFIG".into(), raw.into())], "job").is_err());
        }
        let config: serde_json::Value = serde_json::from_str(&codex_job_config(&[], "job").unwrap()).unwrap();
        assert_eq!(config["sandbox_mode"], "read-only");
        assert_eq!(config["developer_instructions"], "job");
        assert_eq!(config["compact_prompt"], crate::fork::compaction_prompt::codex());
        assert!(config.get("model_providers").is_none());
    }

    #[test]
    fn nao_copia_auth_e_apaga_isolamento_ao_sair() {
        let policy = std::env::temp_dir().join(format!("donnabot-politica-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&policy).unwrap();
        let _root = crate::fork::background_policy::test_policy_root(&policy);
        let original = std::env::temp_dir().join(format!("donnabot-origem-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&original).unwrap();
        let auth = original.join("auth.json");
        std::fs::write(&auth, "credencial-ficticia").unwrap();
        let mut env = vec![("CODEX_HOME".into(), original.to_string_lossy().into_owned())];
        assert!(Isolation::prepare(&mut env, false, true).is_err());
        assert_eq!(std::fs::read_to_string(&auth).unwrap(), "credencial-ficticia");
        let isolated = Isolation::prepare(&mut env, true, true).unwrap();
        let root = isolated.root.clone();
        assert_ne!(Path::new(&env.iter().find(|(k, _)| k == "CODEX_HOME").unwrap().1), original);
        assert!(isolated.cwd().is_dir());
        drop(isolated);
        assert!(!root.exists());
        assert_eq!(std::fs::read_to_string(&auth).unwrap(), "credencial-ficticia");
        std::fs::remove_file(auth).unwrap();
        std::fs::remove_dir(original).unwrap();
        std::fs::remove_dir(policy).unwrap();
    }
}
