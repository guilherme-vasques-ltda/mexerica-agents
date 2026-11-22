#![allow(clippy::unwrap_used)]
use super::*;
use serde_json::json;

#[test]
fn sem_configuracao_preserva_raizes_e_referencia_antigas() {
    if super::super::motor_exec::configured() {
        return;
    }
    assert_eq!(root(), crate::service::data_dir());
    assert!(crate::agent::node::search_dirs().iter().all(|p| p.starts_with(crate::service::data_dir())));
    let dir = std::env::temp_dir().join(format!("d330-f3b-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let old = dir.join("codex");
    std::fs::write(&old, "fixture legado").unwrap();
    assert!(executable(&old));
    assert_eq!(npm_prefix(), "");
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn adaptadores_do_path_do_hub_nao_entram_no_modo_separado() {
    use crate::fork::engine_accounts::managed;
    use std::os::unix::fs::PermissionsExt;

    let names = ["codex-acp", "claude-agent-acp", "claude-code-acp"];
    if let Some(dir) = std::env::var_os("DONNABOT_TEST_ADAPTER_PATH") {
        let dir = PathBuf::from(dir);
        let separate = super::super::motor_exec::configured();
        for name in names {
            let found = crate::agent::backends::which(name);
            assert_eq!(found, (!separate).then(|| dir.join(name)));
        }
        for (backend, name) in [("codex", names[0]), ("claude", names[1])] {
            let commands = managed::adapter_commands(backend).await;
            if backend == "claude" && !crate::fork::claude_ponte::legado() {
                // A ponte é o próprio executável do Hub; nada do PATH entra, em nenhum modo.
                assert_eq!(commands.unwrap(), vec![crate::fork::claude_ponte::comando(None).unwrap()]);
            } else if separate {
                assert!(commands.is_err(), "sem adaptador do motor, a seleção deve recusar");
            } else {
                assert_eq!(commands.unwrap(), vec![crate::shell::quote(&dir.join(name).to_string_lossy())]);
            }
        }
        return;
    }
    // Processos isolados evitam alterar PATH/configuração dos demais testes.
    let dir = std::env::temp_dir().join(format!("d330-adapters-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    for name in names {
        let path = dir.join(name);
        std::fs::write(&path, "#!/bin/sh\nexit 91\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let helper = dir.join("helper");
    std::fs::write(&helper, "#!/bin/sh\n/bin/cat >/dev/null\nprintf '{\"result\":\"path\",\"path\":null}'\n").unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
    for (separate, legado) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .env_clear()
            .env("PATH", &dir)
            .env("HOME", &dir)
            .env("MEXERICA_HOME", &dir)
            .env("DONNABOT_TEST_ADAPTER_PATH", &dir)
            .args([
                "--exact",
                "fork::motor_install::tests::adaptadores_do_path_do_hub_nao_entram_no_modo_separado",
                "--nocapture",
            ]);
        if separate {
            command.env("DONNABOT_MOTOR_TEST_HELPER", &helper);
        }
        if legado {
            command.env(crate::fork::claude_ponte::LEGADO_ENV, "1");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "separado={separate} legado={legado}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "somente VM descartável com root, ajudante e binário do host"]
async fn instalador_prova_uid_real() {
    use crate::fork::{
        engine_accounts::{self as accounts, LaunchReference, managed},
        motor_exec, motor_roots,
    };
    assert!(motor_exec::configured() && !worker());
    let old = PathBuf::from(std::env::var_os("DONNABOT_PROOF_OLD").unwrap());
    let store = crate::store::Store::open(Path::new(":memory:")).unwrap();
    let created = managed::create(&store, Backend::Codex, "Prova", "cli", "d330-f3b").unwrap();
    let id = created["id"].as_str().unwrap();
    let mut registry = accounts::Registry::load(&store).unwrap();
    let account = &mut registry.accounts[0];
    let LaunchReference::CliProfile { executable: saved, directory } = &mut account.launch else {
        panic!("perfil CLI")
    };
    assert!(directory.starts_with(motor_roots::accounts()));
    *saved = old.clone();
    account.state = accounts::State::Connected;
    account.managed.as_mut().unwrap().enabled = true;
    store.kv_set(accounts::SLOT, &serde_json::to_string(&registry).unwrap()).unwrap();
    let bot = serde_json::from_value(
        json!({"id":"d330-f3b","name":"Prova","backend":"codex","forkEditor":{"engineAccountId":id}}),
    )
    .unwrap();
    assert!(managed::resolve_for_bot(&store, &bot).err().unwrap().to_string().starts_with("cli_unavailable"));
    assert!(!published(&old));
    println!("PASS perfil migrado não libera nem move LaunchReference.executable antigo do Hub");

    // Node é pré-requisito real do npm; se ausente, usa o download fixado do produto.
    invoke(Request::Node).await.unwrap();
    let package = std::env::var("DONNABOT_PROOF_PACKAGE").unwrap();
    let agent = json!({"id":"d330-fixture","name":"Prova D330","distribution":{"npx":{"package":package}}});
    for name in ["codex-acp", "claude-agent-acp", "claude-code-acp"] {
        assert!(crate::agent::backends::which(name).is_none(), "não selecionar o adaptador privado do Hub");
    }
    crate::agent::registry::command(&agent, |_, _| {}).await.unwrap();
    let status = invoke(Request::Npm { args: vec!["install".into(), "--global".into(), package] }).await.unwrap();
    assert!(matches!(status, Reply::Status { code: 0 }));
    for name in ["codex-acp", "claude-agent-acp", "claude-code-acp"] {
        assert!(crate::agent::backends::which(name).unwrap().starts_with(root()));
    }
    let commands = managed::adapter_commands("codex").await.unwrap();
    let claude = managed::adapter_commands("claude").await.unwrap();
    assert!(commands[0].contains(root().to_str().unwrap()));
    assert!(claude[0].contains(root().to_str().unwrap()));
    println!("PASS seleção ignora os três adaptadores exclusivos do PATH do Hub e escolhe instalação do motor");
    let output =
        motor_exec::output(motor_exec::command("/bin/cat", &root()).arg(root().join("postinstall-proof.json")))
            .await
            .unwrap();
    assert!(output.status.success());
    let proof: Value = serde_json::from_slice(&output.stdout).unwrap();
    // SAFETY: getter usado apenas para comparar as identidades da prova.
    assert_ne!(proof["uid"].as_u64().unwrap(), u64::from(unsafe { libc::geteuid() }));
    assert_eq!(proof["readHub"], false);
    assert_eq!(proof["writeHub"], false);
    assert_eq!(proof["writeBinary"], false);
    assert_eq!(proof["hubEnvironment"], false);
    println!("PASS npm/postinstall roda como motor; não lê data_dir nem grava token/DB/binário do Hub");

    let cwd = motor_roots::workspace("d330-f3b");
    motor_roots::prepare_workspace(&cwd).unwrap();
    let path = format!("{}:/usr/local/bin:/usr/bin:/bin", crate::agent::node::search_dirs()[0].display());
    let (acp, mut inbox) =
        crate::agent::acp::Acp::spawn(&commands[0], cwd.to_str().unwrap(), &[("PATH".into(), path)]).unwrap();
    if let Err(error) = acp.request("initialize", json!({})).await {
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), inbox.recv()).await.unwrap();
        if let Some(crate::agent::acp::Incoming::Closed { stderr_tail }) = event {
            panic!("adaptador instalado encerrou: {error}: {stderr_tail}");
        }
        panic!("adaptador instalado encerrou: {error}");
    }
    let result = acp
        .request("session/prompt", json!({"sessionId":"prova","prompt":[{"type":"text","text":"prova"}]}))
        .await
        .unwrap();
    assert_eq!(result["uid"], proof["uid"]);
    assert_eq!(result["readHub"], false);
    let installed = PathBuf::from(result["executable"].as_str().unwrap());
    assert!(installed.starts_with(root()) && published(&installed));
    assert!(!installed.starts_with(crate::service::data_dir()));
    acp.kill_and_wait().await;
    println!("PASS bot usa o adaptador instalado via ACP como motor; binário publicado só na raiz do motor");

    let link = root().join("link-hub");
    let output = motor_exec::output(motor_exec::command("/bin/ln", &root()).args([
        "-s".as_ref(),
        old.as_os_str(),
        link.as_os_str(),
    ]))
    .await
    .unwrap();
    assert!(output.status.success());
    assert!(!published(&link), "publicação não segue symlink nem aceita caminho do Hub");
    assert!(crate::agent::backends::which("npx").is_some());
    assert_ne!(crate::agent::node::state(), Some(crate::agent::node::State::Missing));
    println!("PASS descoberta/estado consultados como motor; link para instalação antiga recusado");
}
