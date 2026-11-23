//! Fronteira Hub → motor. Credenciais herdadas nunca são autenticação de um bot (D90).

use std::ffi::{OsStr, OsString};

/// Padrões também usados pela política de subprocessos do Codex (glob sem diferenciar caixa).
pub const CREDENTIAL_PATTERNS: &[&str] = &[
    "*KEY*",
    "*SECRET*",
    "*TOKEN*",
    "*PASSWORD*",
    "*PASSWD*",
    "*CREDENTIAL*",
    "*AUTH*",
    "ANTHROPIC_*",
    "OPENAI_*",
    "XAI_*",
    "OP_*",
    "AWS_*",
    "AZURE_*",
    "GOOGLE_*",
    "DATABASE_URL",
    "REDIS_URL",
    "*_DSN",
    "*CONNECTION_STRING*",
    "BASH_ENV",
    "ENV",
    "ZDOTDIR",
    "BASH_FUNC_*",
    "NODE_OPTIONS",
    "NODE_PATH",
    "PYTHONSTARTUP",
    "PYTHONPATH",
    "RUBYOPT",
    "RUBYLIB",
    "PERL5OPT",
    "PERL5LIB",
    "LD_*",
    "DYLD_*",
    "GIT_CONFIG_*",
];

/// Exceções explícitas de dados operacionais com nomes parecidos com segredos.
pub const NON_SECRET_NAMES: &[&str] = &["XAUTHORITY", "GOOGLE_CLOUD_PROJECT", "LD_LIBRARY_PATH"];

fn non_secret(name: &str) -> bool {
    name.starts_with("GIT_AUTHOR_") || name.starts_with("GIT_COMMITTER_") || NON_SECRET_NAMES.contains(&name)
}

/// Último filtro do Codex, DEPOIS de `set`. Arrays substituem a lista inferior;
/// mapas (inclusive `set={}`) são mesclados recursivamente pelo CLI.
pub const SHELL_ENV_ALLOWLIST: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "LC_MESSAGES",
    "LC_TIME",
    "LC_NUMERIC",
    "LC_COLLATE",
    "LC_MONETARY",
    "LC_PAPER",
    "LC_NAME",
    "LC_ADDRESS",
    "LC_TELEPHONE",
    "LC_MEASUREMENT",
    "LC_IDENTIFICATION",
    "TERM",
    "COLORTERM",
    "TZ",
    "TMPDIR",
    "TMP",
    "TEMP",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "VIRTUAL_ENV",
    "SYSTEMROOT",
    "WINDIR",
    "COMSPEC",
    "PATHEXT",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "GIT_AUTHOR_*",
    "GIT_COMMITTER_*",
    "XAUTHORITY",
    "GOOGLE_CLOUD_PROJECT",
    "LD_LIBRARY_PATH",
];

/// As exceções operacionais passam pela allowlist final, sem abrir as outras variáveis dessas famílias.
pub fn shell_excludes() -> Vec<&'static str> {
    CREDENTIAL_PATTERNS.iter().copied().filter(|p| !matches!(*p, "*AUTH*" | "GOOGLE_*" | "LD_*")).collect()
}

fn authenticated_proxy(name: &str, value: &OsStr) -> bool {
    name.ends_with("_PROXY") && value.to_string_lossy().contains('@')
}

fn url_credential(value: &OsStr) -> bool {
    reqwest::Url::parse(&value.to_string_lossy())
        .is_ok_and(|url| !url.username().is_empty() || url.password().is_some())
}

fn credential(name: &OsStr) -> bool {
    let name = name.to_string_lossy().to_ascii_uppercase();
    !non_secret(&name)
        && CREDENTIAL_PATTERNS.iter().any(|pattern| {
            let word = pattern.trim_matches('*');
            match (pattern.starts_with('*'), pattern.ends_with('*')) {
                (true, true) => name.contains(word),
                (true, false) => name.ends_with(word),
                (false, true) => name.starts_with(word),
                (false, false) => name == word,
            }
        })
}

/// Sem overrides de launcher/config vindos do ambiente. `CODEX_HOME` só pode vir do Hub,
/// para preservar seu auth.json; nunca do slot agent-env controlado pela configuração do motor.
fn launcher_override(name: &str) -> bool {
    matches!(name, "CODEX_CONFIG" | "CODEX_PATH" | "CODEX_PROFILE" | "INITIAL_AGENT_MODE" | "DEFAULT_AUTH_REQUEST")
}

pub fn inherited(vars: impl IntoIterator<Item = (OsString, OsString)>) -> Vec<(OsString, OsString)> {
    let separate = crate::fork::motor_exec::configured();
    let mut values: Vec<_> = vars
        .into_iter()
        .filter(|(name, value)| {
            let upper = name.to_string_lossy().to_ascii_uppercase();
            !(credential(name)
                || crate::fork::assinatura_pessoal::known_meta_credential(&upper, &value.to_string_lossy())
                || launcher_override(&upper)
                || authenticated_proxy(&upper, value)
                || url_credential(value)
                || (separate
                    && matches!(
                        upper.as_str(),
                        "HOME"
                            | "USER"
                            | "LOGNAME"
                            | "CODEX_HOME"
                            | "CLAUDE_CONFIG_DIR"
                            | "XDG_CONFIG_HOME"
                            | "XDG_DATA_HOME"
                            | "XDG_CACHE_HOME"
                    )))
        })
        .collect();
    if separate {
        values.push(("HOME".into(), crate::fork::motor_roots::MOTOR.into()));
    }
    values
}

/// O slot do motor escolhido pode repor sua própria autenticação; nunca a de outro motor.
pub fn retain_for_backend(env: &mut Vec<(String, String)>, backend: &str) {
    let provider_keys = if backend == "codex" {
        let config = env
            .iter()
            .rev()
            .find(|(key, _)| key == "CODEX_CONFIG")
            .map(|(_, raw)| raw.clone())
            .or_else(|| std::env::var("CODEX_CONFIG").ok());
        provider_env_keys(config.as_deref())
    } else {
        Vec::new()
    };
    env.retain(|(name, value)| {
        let upper = name.to_ascii_uppercase();
        if matches!(upper.as_str(), "CODEX_CONFIG") {
            return true; // mesclado e endurecido por harness::apply, nunca repassado cru
        }
        if launcher_override(&upper)
            || crate::fork::assinatura_pessoal::known_meta_credential(&upper, value)
            || authenticated_proxy(&upper, OsStr::new(value))
            || url_credential(OsStr::new(value))
            || matches!(
                upper.as_str(),
                "BASH_ENV"
                    | "ENV"
                    | "ZDOTDIR"
                    | "NODE_OPTIONS"
                    | "NODE_PATH"
                    | "PYTHONSTARTUP"
                    | "PYTHONPATH"
                    | "RUBYOPT"
                    | "RUBYLIB"
                    | "PERL5OPT"
                    | "PERL5LIB"
            )
            || ["BASH_FUNC_", "LD_", "DYLD_", "GIT_CONFIG_"]
                .iter()
                .any(|prefix| upper.starts_with(prefix) && !non_secret(&upper))
            || matches!(
                upper.as_str(),
                "SSH_AUTH_SOCK"
                    | "HOME"
                    | "USERPROFILE"
                    | "HOMEDRIVE"
                    | "HOMEPATH"
                    | "CODEX_HOME"
                    | "CLAUDE_CONFIG_DIR"
            )
            || upper.starts_with("XDG_")
        {
            return false;
        }
        // O slot já foi escolhido pelo backend: preserva modelos, gateway, Bedrock e env_key de provedores.
        // Só famílias reconhecidas de OUTRO motor são retiradas; nomes arbitrários pertencem a este slot.
        let families: &[&str] = match backend {
            "claude" => &["OPENAI_", "CODEX_", "GEMINI_", "XAI_"],
            "codex" => &["ANTHROPIC_", "CLAUDE_", "GEMINI_", "XAI_"],
            "gemini" => &["ANTHROPIC_", "CLAUDE_", "OPENAI_", "CODEX_", "XAI_"],
            _ => &[],
        };
        provider_keys.iter().any(|key| key == name) || !families.iter().any(|prefix| upper.starts_with(prefix))
    });
}

fn provider_env_keys(raw: Option<&str>) -> Vec<String> {
    let Some(config) = raw.and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok()) else {
        return Vec::new();
    };
    let nested = config["model_providers"]
        .as_object()
        .into_iter()
        .flat_map(|providers| providers.values())
        .filter_map(|provider| provider["env_key"].as_str());
    let dotted = config
        .as_object()
        .into_iter()
        .flat_map(|object| object.iter())
        .filter(|(key, _)| key.starts_with("model_providers.") && key.ends_with(".env_key"))
        .filter_map(|(_, value)| value.as_str());
    nested.chain(dotted).map(str::to_owned).collect()
}

/// Sondagens e uso local recebem a mesma fronteira do ACP. Com o slot indisponível,
/// ainda podem consultar o login por assinatura usando só o ambiente herdado filtrado.
pub fn for_cli(store: &crate::store::Store, backend: &str) -> Vec<(OsString, OsString)> {
    let Ok(mut slot) = super::auth::env(store, backend) else {
        return inherited(std::env::vars_os());
    };
    super::harness::apply(&mut slot, backend, "");
    let mut env = inherited(std::env::vars_os());
    env.extend(slot.into_iter().map(|(k, v)| (k.into(), v.into())));
    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LockExt, market::vault, store::Store};

    #[test]
    fn sondagem_com_cofre_travado_usa_somente_ambiente_herdado_filtrado() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        for backend in ["codex", "claude"] {
            vault::write(&store, &format!("agent-env:{backend}"), r#"{"PRIVATE_API_KEY":"ficticia"}"#).unwrap();
        }
        *store.secret_key.locked() = None;
        let expected = inherited(std::env::vars_os());
        for backend in ["codex", "claude"] {
            assert!(super::super::auth::env(&store, backend).is_err());
            let env = for_cli(&store, backend);
            assert!(env.iter().eq(&expected), "a sondagem deve receber apenas o ambiente filtrado");
            assert!(!env.iter().any(|(key, _)| key == "PRIVATE_API_KEY"));
        }
    }

    #[test]
    fn ambiente_do_motor_nao_herda_credenciais_do_hub_e_repoe_so_sua_chave() {
        let names = [
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_BASE_URL",
            "OPENAI_API_KEY",
            "XAI_API_KEY",
            "CLAUDE_CODE_OAUTH_TOKEN",
            "OP_SERVICE_ACCOUNT_TOKEN",
            "OP_SESSION_test",
            "AWS_ACCESS_KEY_ID",
            "GOOGLE_APPLICATION_CREDENTIALS",
            "GH_TOKEN",
            "DB_PASSWORD",
            "DATABASE_URL",
            "REDIS_URL",
            "SENTRY_DSN",
            "CUSTOM_SECRET",
            "api_key",
            "SSH_AUTH_SOCK",
            "CODEX_PATH",
            "INITIAL_AGENT_MODE",
        ];
        let mut parent: Vec<_> =
            names.iter().map(|name| (OsString::from(name), OsString::from("credencial-ficticia"))).collect();
        parent.extend([
            ("HOME".into(), "/login-do-hub".into()),
            ("CODEX_HOME".into(), "/login-codex".into()),
            ("PATH".into(), "/bin".into()),
            ("LANG".into(), "pt_BR.UTF-8".into()),
        ]);
        let inherited = inherited(parent);
        assert!(names.iter().all(|name| !inherited.iter().any(|(key, _)| key == name)));
        assert_eq!(inherited.len(), 4, "preserva caminhos de login e ambiente comum");
        let mut saved = vec![
            ("OPENAI_API_KEY".into(), "chave-do-codex".into()),
            ("ANTHROPIC_API_KEY".into(), "outra-chave".into()),
            ("OP_SERVICE_ACCOUNT_TOKEN".into(), "cofre".into()),
            ("CODEX_HOME".into(), "/login-do-bot".into()),
            ("HOME".into(), "/outro".into()),
        ];
        retain_for_backend(&mut saved, "codex");
        assert_eq!(
            saved,
            [("OPENAI_API_KEY".into(), "chave-do-codex".into()), ("OP_SERVICE_ACCOUNT_TOKEN".into(), "cofre".into())]
        );
        let mut command = std::process::Command::new("motor-ficticio");
        command.env_clear().envs(inherited).envs(saved);
        let visible: Vec<_> = command.get_envs().map(|(k, _)| k.to_string_lossy().into_owned()).collect();
        assert!(visible.contains(&"OPENAI_API_KEY".to_owned()));
        assert!(!visible.contains(&"ANTHROPIC_API_KEY".to_owned()));
    }

    #[test]
    fn preserva_configuracao_do_slot_e_excecoes_operacionais() {
        let names = [
            "GIT_AUTHOR_NAME",
            "GIT_AUTHOR_EMAIL",
            "GIT_COMMITTER_NAME",
            "GIT_COMMITTER_EMAIL",
            "XAUTHORITY",
            "GOOGLE_CLOUD_PROJECT",
            "LD_LIBRARY_PATH",
        ];
        let vars: Vec<_> = names.iter().map(|k| (OsString::from(k), OsString::from("operacional"))).collect();
        assert_eq!(inherited(vars.clone()), vars);
        for (backend, names) in [
            (
                "claude",
                vec![
                    "ANTHROPIC_MODEL",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL",
                    "ANTHROPIC_CUSTOM_HEADERS",
                    "AWS_REGION",
                    "AWS_BEARER_TOKEN_BEDROCK",
                    "GOOGLE_CLOUD_PROJECT",
                ],
            ),
            ("codex", vec!["OPENROUTER_API_KEY", "AZURE_OPENAI_API_KEY", "OPENAI_BASE_URL"]),
        ] {
            let mut slot: Vec<_> = names.iter().map(|k| ((*k).to_owned(), "configurada".into())).collect();
            let original = slot.clone();
            retain_for_backend(&mut slot, backend);
            assert_eq!(slot, original);
        }
    }

    #[test]
    fn codex_preserva_env_key_de_provedor_declarado() {
        for config in [
            serde_json::json!({"model_providers":{"gateway":{"env_key":"XAI_API_KEY"}}}),
            serde_json::json!({"model_providers.gateway.env_key":"XAI_API_KEY"}),
        ] {
            let mut slot = vec![
                ("CODEX_CONFIG".into(), config.to_string()),
                ("XAI_API_KEY".into(), "configurada".into()),
                ("CLAUDE_CODE_OAUTH_TOKEN".into(), "outra".into()),
            ];
            retain_for_backend(&mut slot, "codex");
            assert!(slot.iter().any(|(key, _)| key == "XAI_API_KEY"));
            assert!(!slot.iter().any(|(key, _)| key == "CLAUDE_CODE_OAUTH_TOKEN"));
        }
        for name in ["GIT_AUTHOR_NAME", "GIT_COMMITTER_EMAIL", "XAUTHORITY", "GOOGLE_CLOUD_PROJECT", "LD_LIBRARY_PATH"]
        {
            assert!(
                !shell_excludes().iter().any(|p| {
                    let word = p.trim_matches('*');
                    match (p.starts_with('*'), p.ends_with('*')) {
                        (true, true) => name.contains(word),
                        (true, false) => name.ends_with(word),
                        (false, true) => name.starts_with(word),
                        (false, false) => name == word,
                    }
                }),
                "{name} deve chegar ao último filtro do shell"
            );
        }
    }

    #[test]
    fn inicializacao_automatica_e_proxies_autenticados_nao_passam_pelas_duas_entradas() {
        let blocked = [
            "BASH_ENV",
            "ENV",
            "ZDOTDIR",
            "NODE_OPTIONS",
            "LD_PRELOAD",
            "DYLD_INSERT_LIBRARIES",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
            "PIP_INDEX_URL",
            "NPM_CONFIG_REGISTRY",
        ];
        let pairs: Vec<(String, String)> =
            blocked.iter().map(|key| ((*key).into(), "http://usuario:ficticia@proxy.invalid".into())).collect();
        let safe = ("HTTP_PROXY".to_owned(), "http://proxy.invalid".to_owned());
        let mut parent: Vec<_> = pairs.iter().map(|(k, v)| (OsString::from(k), OsString::from(v))).collect();
        parent.push((safe.0.clone().into(), safe.1.clone().into()));
        assert_eq!(inherited(parent), [(OsString::from(&safe.0), OsString::from(&safe.1))]);
        let mut slot = pairs;
        slot.push(safe.clone());
        retain_for_backend(&mut slot, "codex");
        assert_eq!(slot, [safe]);
    }
}
