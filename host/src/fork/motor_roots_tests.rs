//! D70: fronteiras sem UID fictício; o caso ignorado exige a VM root da prova.
use super::{motor_exec, motor_roots as roots};
use std::{
    io::Read,
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    path::Path,
    sync::atomic::AtomicBool,
};

#[test]
fn descritores_recusam_links_folha_e_ancestral() {
    use motor_exec::fd::Directory;
    let root = std::env::temp_dir().join(format!("d330-f3-fd-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let physical = root.canonicalize().unwrap();
    let dir = Directory::create(&physical.join("dentro"), 0o700).unwrap();
    std::fs::write(physical.join("token-ficticio"), b"TOKEN_FICTICIO").unwrap();
    std::os::unix::fs::symlink(physical.join("token-ficticio"), physical.join("dentro/link")).unwrap();
    assert!(dir.file("link".as_ref(), libc::O_RDONLY, 0).is_err());
    std::os::unix::fs::symlink(physical.join("dentro"), physical.join("ancestral")).unwrap();
    assert!(Directory::open(&physical.join("ancestral")).is_err());
    assert!(Directory::create(&physical.join("dentro/../fora"), 0o700).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "exige VM descartável, ajudante com capacidades e dois UIDs"]
fn raizes_prova_uid_real() {
    assert!(motor_exec::configured());
    let temp = std::env::temp_dir().join(format!("d330-f3-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&temp).unwrap();
    std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o700)).unwrap();
    let token = temp.join("token");
    std::fs::write(&token, b"TOKEN_FICTICIO_D330").unwrap();
    let snapshots = temp.join("files");
    let cancel = AtomicBool::new(false);
    let negative = crate::chat::files::snapshot(&snapshots, Path::new("/"), token.to_str().unwrap(), None, &cancel);
    assert!(negative.is_err(), "send_file nunca copia token privado como Hub");
    assert!(std::fs::read_dir(&snapshots).unwrap().next().is_none());
    println!("PASS send_file token fictício recusado, nenhuma cópia publicada");
    let bot = uuid::Uuid::new_v4().to_string();
    roots::prepare_workspace(&roots::workspace(&bot)).unwrap();
    let upload = uuid::Uuid::new_v4().to_string();
    let uploads = roots::uploads(&bot);
    crate::chat::uploads::append(&uploads, &upload, "anexo.txt", 0, b"ANEXO_FICTICIO", true).unwrap();
    let (path, _) = crate::chat::uploads::resolve(&uploads, &upload).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o7777, 0o640);
    assert_eq!(std::fs::metadata(path.parent().unwrap()).unwrap().mode() & 0o7777, 0o2750);
    let mut delegated = motor_exec::read::Input::open(&path).unwrap();
    let mut bytes = Vec::new();
    delegated.read_to_end(&mut bytes).unwrap();
    delegated.finish().unwrap();
    assert_eq!(bytes, b"ANEXO_FICTICIO");
    println!("PASS anexo novo 0640/pasta 2750 lido pelo motor com outro UID sob UMask 0077");
    let positive =
        crate::chat::files::snapshot(&snapshots, Path::new("/"), path.to_str().unwrap(), None, &cancel).unwrap();
    assert_eq!(positive.meta.size, 14);
    drop(positive);
    let symlink = roots::workspace(&bot).join("link");
    std::os::unix::fs::symlink(&uploads, &symlink).unwrap();
    assert!(
        crate::chat::files::snapshot(
            &snapshots,
            Path::new("/"),
            symlink.join(&upload).join("anexo.txt").to_str().unwrap(),
            None,
            &cancel
        )
        .is_err()
    );
    println!("PASS send_file recusa link em ancestral, inclusive para arquivo legível pelo motor");
    let store = crate::store::Store::open(Path::new(":memory:")).unwrap();
    let account = format!("ac_{}", "a".repeat(64));
    let profile =
        super::engine_accounts::managed::new_profile(&store, &account, super::engine_accounts::Backend::Codex).unwrap();
    super::engine_accounts::managed::validate_profile(&store, &account, &profile).unwrap();
    assert!(std::fs::read_dir(&profile).is_err(), "perfil 0700 não é atravessado pelo Hub");
    let output = std::process::Command::new(motor_exec::helper_path())
        .args(["--cwd", profile.to_str().unwrap(), "--", "/bin/sh", "-c", "printf perfil > auth.json"])
        .env("DONNABOT_MOTOR_PRIVATE", "1")
        .output()
        .unwrap();
    assert!(output.status.success());
    super::engine_accounts::managed::secure_files(&profile).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(audit_profile(&profile, &roots::workspace(&bot), false));

    super::engine_accounts::managed::remove_profile_home(profile.parent().unwrap()).unwrap();
    assert!(roots::profile("validar", &[profile.as_os_str()]).is_err());
    println!("PASS perfil criar/validar/proteger/remover e cwd após troca, Hub sem acesso");
    let private_config = temp.join("background-private.json");
    std::fs::write(&private_config, b"CONFIG_PRIVADA_FICTICIA").unwrap();
    runtime.block_on(async {
        super::engine_accounts::tests::migrated_discovery_root(&roots::workspace(&bot), &temp).await;
        let pwd = motor_exec::output(&mut motor_exec::command("/bin/pwd", &crate::service::data_dir())).await.unwrap();
        assert!(pwd.status.success());
        assert_eq!(String::from_utf8(pwd.stdout).unwrap().trim(), roots::MOTOR);
        let policy = temp.join("politica-vazia");
        std::fs::create_dir(&policy).unwrap();
        let _root = super::background_policy::test_policy_root(&policy);
        let mut env = Vec::new();
        let isolated = super::background_isolation::Isolation::prepare(&mut env,true,true).unwrap();
        let codex_home = env.iter().find(|(key, _)| key == "CODEX_HOME").unwrap().1.clone();
        audit_profile(Path::new(&codex_home), isolated.cwd(), true).await;
        let cwd = isolated.cwd().to_string_lossy().into_owned();
        let script = format!(r#"import os,json,sys
for line in sys.stdin:
 r=json.loads(line)
 try:
  private=open({:?}).read()
 except PermissionError:
  private=None
 print(json.dumps({{"jsonrpc":"2.0","id":r['id'],"result":{{"uid":os.getuid(),"private":private,"cwd":os.getcwd()}}}}),flush=True)
"#,private_config.to_str().unwrap());
        let command = format!("python3 -u -c {}",crate::shell::quote(&script));
        let (job,_inbox) = crate::agent::acp::Acp::spawn_background(&command,&cwd,&env).unwrap();
        let result = job.request("proof",serde_json::json!({})).await.unwrap();
        // SAFETY: getter sem efeitos colaterais, apenas compara os UIDs da prova.
        let hub_uid = unsafe { libc::geteuid() };
        assert_ne!(result["uid"].as_u64().unwrap(),u64::from(hub_uid));
        assert!(result["private"].is_null(),"job não lê configuração privada do Hub");
        assert_eq!(result["cwd"],cwd);
        job.kill_and_wait().await;
        drop(isolated);
        let status = std::process::Command::new(motor_exec::helper_path()).args(["--cwd",&cwd,"--","/bin/true"]).output().unwrap();
        assert!(!status.status.success(),"pasta temporária removida como motor");
    });
    println!(
        "PASS job background isolado executa como motor e não lê configuração privada do Hub; limpeza pelo ajudante"
    );
    super::motor_migration::root_proof::proof(&temp);
    std::fs::remove_dir_all(temp).unwrap();
    std::fs::remove_dir_all(roots::bot(&bot)).unwrap();
}

async fn audit_profile(profile: &Path, cwd: &Path, background: bool) {
    let status = std::process::Command::new(motor_exec::helper_path())
        .args([
            "--cwd",
            profile.to_str().expect("perfil UTF-8"),
            "--",
            "/bin/sh",
            "-c",
            "printf \"notify=['SENTINELA_PRIVADA']\\n\" > config.toml",
        ])
        .env("DONNABOT_MOTOR_PRIVATE", "1")
        .output()
        .expect("criar config motor");
    assert!(status.status.success());
    let script = r#"import sys,json
for line in sys.stdin:
 r=json.loads(line)
 print(json.dumps({"jsonrpc":"2.0","id":r['id'],"result":{"protocolVersion":1,"agentInfo":{"name":"codex-acp"}}}),flush=True)
"#;
    let command = format!("python3 -u -c {}", crate::shell::quote(script));
    let env = vec![("CODEX_HOME".into(), profile.to_string_lossy().into_owned()), ("CODEX_CONFIG".into(), "{}".into())];
    let budget = std::time::Duration::from_secs(10);
    let conn = if background {
        crate::agent::bot::start_background_agent(&command.into(), cwd.to_str().expect("cwd UTF-8"), &env, budget).await
    } else {
        crate::agent::bot::start_agent_protected(&command.into(), cwd.to_str().expect("cwd UTF-8"), &env, budget).await
    }
    .expect("initialize e auditoria motor");
    assert!(conn.config_warnings.iter().any(|w| w.message().contains("notify")));
    assert!(conn.config_warnings.iter().all(|w| !w.message().contains("SENTINELA_PRIVADA")));
    conn.acp.kill_and_wait().await;
    println!(
        "PASS F3-REV1 auditoria após initialize real, perfil/job 0700 motor, aviso sem conteúdo privado; background={background}"
    );
}

#[test]
#[ignore = "exige VM descartável, ajudante com capacidades e dois UIDs"]
fn d520_negacoes_novas_e_antigas_uid_real() {
    assert!(motor_exec::configured());
    let root = std::env::temp_dir().join(format!("d520-negacoes-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    // Pai atravessável: a prova precisa depender da permissão de cada raiz privada.
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
    let root = root.canonicalize().unwrap();
    let publico = root.join("controle-publico");
    std::fs::write(&publico, b"CONTROLE_FICTICIO").unwrap();
    std::fs::set_permissions(&publico, std::fs::Permissions::from_mode(0o644)).unwrap();
    let executar =
        |args: &[&std::ffi::OsStr]| std::process::Command::new(motor_exec::helper_path()).args(args).output().unwrap();
    let controle =
        executar(&["--cwd".as_ref(), root.as_os_str(), "--".as_ref(), "/bin/cat".as_ref(), publico.as_os_str()]);
    assert!(controle.status.success());
    assert_eq!(controle.stdout, b"CONTROLE_FICTICIO");
    let cancel = AtomicBool::new(false);
    for nome in [
        ".mexerica",
        ".mexerica-dev",
        ".mexerica-dev-teste",
        ".mexerica-backup",
        ".donnabot",
        ".donnabot-dev",
        ".donnabot-dev-teste",
        ".donnabot-backup",
        ".codync",
        ".codync-dev",
        ".codync-dev-teste",
        ".codync-backup",
        "override-privado",
    ] {
        let privada = root.join(nome);
        std::fs::DirBuilder::new().mode(0o700).create(&privada).unwrap();
        let token = privada.join("token-ficticio");
        std::fs::write(&token, b"SEGREDO_FICTICIO_D520").unwrap();
        std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
        for args in [
            vec!["--cwd".as_ref(), root.as_os_str(), "--".as_ref(), "/bin/cat".as_ref(), token.as_os_str()],
            vec!["read-file".as_ref(), token.as_os_str()],
            vec!["--cwd".as_ref(), privada.as_os_str(), "--".as_ref(), "/bin/true".as_ref()],
            vec!["--cwd".as_ref(), root.as_os_str(), "--".as_ref(), "/usr/bin/touch".as_ref(), token.as_os_str()],
        ] {
            let result = executar(&args);
            assert!(!result.status.success(), "motor acessou {nome}");
            assert!(result.stdout.is_empty(), "negativo publicou conteúdo");
        }
        let snapshots = root.join(format!("snapshots-{nome}"));
        assert!(
            crate::chat::files::snapshot(&snapshots, Path::new("/"), token.to_str().unwrap(), None, &cancel).is_err()
        );
        assert!(std::fs::read_dir(&snapshots).unwrap().next().is_none());
        assert_eq!(std::fs::read(&token).unwrap(), b"SEGREDO_FICTICIO_D520");
    }
    std::fs::remove_dir_all(root).unwrap();
    println!("PASS D520: raízes novas, antigas, Dev, backups e override recusados pelo UID do motor");
}
