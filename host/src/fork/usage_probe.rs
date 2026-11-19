//! Regressões próprias do porte upstream-01. Não usam credenciais nem CLI do operador.
#[cfg(test)]
mod tests {
    use crate::{LockExt, market::vault, store::Store};
    use serde_json::{Value, json};
    use std::future::Future;
    use std::path::Path;
    use std::time::Duration;

    const WAIT: Duration = Duration::from_secs(60);

    struct TimeoutRelease<'a>(&'a Path);
    impl Drop for TimeoutRelease<'_> {
        fn drop(&mut self) {
            // Só cleanup: em caso de falha, a fixture não fica órfã esperando para sempre.
            let _ = std::fs::write(self.0.join("timeout-release"), "liberar");
        }
    }

    async fn expire_after_start<T>(root: &Path, budget: Duration, query: impl Future<Output = T>) -> T {
        let marker = root.join("timeout-ready.json");
        tokio::time::pause();
        let start = tokio::time::Instant::now();
        // A tarefa bloqueante impede autoavanço durante o I/O real da fixture.
        let ready = tokio::task::spawn_blocking(move || {
            let deadline = std::time::Instant::now() + WAIT;
            loop {
                if let Ok(raw) = std::fs::read(&marker)
                    && let Ok(value) = serde_json::from_slice::<Value>(&raw)
                    && value["pid"].as_u64().is_some_and(|pid| pid > 0)
                {
                    return;
                }
                assert!(std::time::Instant::now() < deadline, "a fixture deve sinalizar início em até 60 s");
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        tokio::pin!(query);
        tokio::select! {
            _ = &mut query => panic!("a consulta terminou antes do sinal da fixture"),
            result = ready => result.unwrap(),
        }
        assert_eq!(tokio::time::Instant::now(), start, "o início da fixture não consome o prazo observado");
        tokio::time::advance(budget + Duration::from_millis(1)).await;
        tokio::time::resume();
        query.await
    }

    fn stored_state(store: &Store) -> (Vec<(String, String)>, i64, i64) {
        let db = store.db.locked();
        let rows = db
            .prepare("SELECT k, v FROM kv ORDER BY k")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let count = |table| db.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0)).unwrap();
        (rows, count("bots"), count("entries"))
    }

    fn scenario(name: &str) {
        let root = std::env::temp_dir().join(format!("upstream-01-{}", uuid::Uuid::new_v4()));
        let profile = root.join("existing-profile");
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(root.join("scenario"), name).unwrap();
        std::fs::write(profile.join("auth.json"), r#"{"account":"conta-existente-ficticia"}"#).unwrap();
        let mcp = root.join("mcp.py");
        std::fs::write(&mcp, "import pathlib, sys\npathlib.Path(sys.argv[1]).touch()\n").unwrap();
        std::fs::write(
            profile.join("mcp.json"),
            json!({"mcpServers":{"configured":{"command":"/usr/bin/python3","args":[mcp,root.join("mcp-started")]}}})
                .to_string(),
        )
        .unwrap();
        let cli = root.join("claude");
        crate::fork::test_support::write_executable(
            &cli,
            include_bytes!("../../tests/fixtures/upstream_usage_cli.py"),
            0o700,
        );
        // Cada filho possui seu ambiente. Nenhum set_var/remove_var concorre com outros testes.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "fork::usage_probe::tests::consulta_cli_em_subprocesso", "--ignored", "--nocapture"])
            .env_clear()
            .envs([
                ("HOME", root.to_str().unwrap()),
                ("CLAUDE_CONFIG_DIR", profile.to_str().unwrap()),
                ("PATH", "/usr/bin:/bin"),
                ("GH_TOKEN", "segredo-ficticio-do-hub"),
                ("ANTHROPIC_API_KEY", "chave-herdada-proibida"),
                ("OPENAI_API_KEY", "outro-provedor-proibido"),
                ("NODE_OPTIONS", "--trace-warnings"),
            ])
            .output()
            .unwrap();
        let success = output.status.success();
        if success {
            std::fs::remove_dir_all(root).unwrap();
        } else {
            eprintln!(
                "fixture {name}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            eprintln!("evidência preservada em {}", root.display());
        }
        assert!(success, "a consulta deve preservar o contrato na fixture {name}");
    }

    #[test]
    fn consulta_isola_mcp_e_preserva_relatorio_conta_e_ambiente() {
        scenario("success");
    }

    #[test]
    fn consulta_com_cofre_travado_preserva_login_existente() {
        scenario("locked");
    }

    #[test]
    fn consulta_invalida_nao_cria_conversa_nem_conta() {
        scenario("invalid");
    }

    #[test]
    fn consulta_expirada_encerra_cli_sem_criar_conversa_nem_conta() {
        scenario("timeout");
    }

    #[tokio::test]
    #[ignore = "executado pelos testes acima em processo com ambiente próprio"]
    async fn consulta_cli_em_subprocesso() {
        let root = std::env::var_os("HOME").unwrap();
        let root = Path::new(&root);
        let profile = root.join("existing-profile");
        let account = std::fs::read(profile.join("auth.json")).unwrap();
        let config = std::fs::read(profile.join("mcp.json")).unwrap();
        let scenario = std::fs::read_to_string(root.join("scenario")).unwrap();
        let store = Store::open(Path::new(":memory:")).unwrap();
        *store.secret_key.locked() = Some(zeroize::Zeroizing::new(vec![7; 32]));
        vault::write(
            &store,
            "agent-env:claude",
            r#"{"ANTHROPIC_BASE_URL":"https://gateway.invalid","ANTHROPIC_API_KEY":"chave-do-slot-ficticia","OPENAI_API_KEY":"outro-slot-proibido","CLAUDE_CONFIG_DIR":"/perfil-indevido"}"#,
        )
        .unwrap();
        let saved = stored_state(&store);
        if scenario == "locked" {
            *store.secret_key.locked() = None;
        }
        let _release = (scenario == "timeout").then(|| TimeoutRelease(root));
        let bin = root.join("claude");
        let budget = if scenario == "timeout" { Duration::from_secs(1) } else { WAIT };
        let query = crate::usage::claude_cli_usage_at(&store, &bin, root, budget);
        let result = if scenario == "timeout" { expire_after_start(root, budget, query).await } else { query.await };
        if matches!(scenario.as_str(), "invalid" | "timeout") {
            assert!(result.is_none(), "falha não deve fabricar cota");
        } else {
            let windows = result.unwrap();
            assert_eq!(windows.len(), 2);
            assert_eq!(windows[0].id, "five_hour");
            assert_eq!(windows[0].percent, 23.0);
            assert_eq!(windows[0].resets_text.as_deref(), Some("Nov 4 at 10am (UTC)"));
            assert_eq!(windows[1].percent, 41.0);
        }
        let call: Value = serde_json::from_slice(&std::fs::read(root.join("invocation.json")).unwrap()).unwrap();
        assert_eq!(call["account"]["account"], "conta-existente-ficticia");
        assert_eq!(call["env"]["CLAUDE_CONFIG_DIR"], profile.to_str().unwrap());
        assert_eq!(call["env"]["HOME"], root.to_str().unwrap());
        assert_eq!(call["cwd"], root.to_str().unwrap());
        assert_eq!(call["stdin"], "");
        for key in ["GH_TOKEN", "OPENAI_API_KEY", "NODE_OPTIONS"] {
            assert!(call["env"][key].is_null(), "variável vedada: {key}");
        }
        if scenario == "locked" {
            assert!(call["env"]["ANTHROPIC_API_KEY"].is_null());
            assert!(call["env"]["ANTHROPIC_BASE_URL"].is_null());
        } else {
            assert_eq!(call["env"]["ANTHROPIC_API_KEY"], "chave-do-slot-ficticia");
            assert_eq!(call["env"]["ANTHROPIC_BASE_URL"], "https://gateway.invalid");
            assert_eq!(call["env"]["CLAUDE_CODE_DISABLE_AUTO_MEMORY"], "1");
        }
        let args = call["args"].as_array().unwrap();
        assert!(args.windows(2).any(|pair| pair == [json!("--setting-sources"), json!("")]));
        assert!(args.windows(2).any(|pair| pair == [json!("--output-format"), json!("json")]));
        assert!(!root.join("model-turn").exists());
        assert!(!profile.join("session.json").exists());
        assert_eq!(std::fs::read(profile.join("auth.json")).unwrap(), account);
        assert_eq!(std::fs::read(profile.join("mcp.json")).unwrap(), config);
        assert_eq!(std::fs::read_dir(&profile).unwrap().count(), 2);
        assert_eq!(stored_state(&store), saved, "consulta não cria conta, bot nem entrada no Hub");
        if scenario == "timeout" {
            let pid = call["pid"].as_u64().unwrap().to_string();
            let deadline = tokio::time::Instant::now() + WAIT;
            let status = loop {
                let status = std::process::Command::new("/bin/kill").args(["-0", &pid]).status().unwrap();
                if !status.success() {
                    break status;
                }
                assert!(tokio::time::Instant::now() < deadline, "kill_on_drop deve encerrar o processo da fixture");
                tokio::time::sleep(Duration::from_millis(20)).await;
            };
            assert!(!status.success(), "kill_on_drop deve encerrar o processo da fixture");
        }
        assert!(!root.join("mcp-started").exists(), "a consulta iniciou o MCP configurado");
    }
}
