#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::*;

const TEST_WAIT: Duration = Duration::from_secs(60);
#[cfg(target_os = "linux")]
const OPERATION_TTL: Duration = Duration::from_secs(300);

async fn esperar(condicao: impl Fn() -> bool) {
    tokio::time::timeout(TEST_WAIT, async {
        while !condicao() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("condição da fixture não foi atingida em 60 s");
}

#[test]
fn fronteiras_de_url_codigo_e_saida_fragmentada() {
    assert!(
        transport::challenge(Backend::Codex, "device_code", "https://auth.openai.com/codex/device", Some("ABCD-1234"))
            .is_ok()
    );
    for url in [
        "http://auth.openai.com/codex/device",
        "https://auth.openai.com.evil.invalid/codex/device",
        "https://evil@auth.openai.com/codex/device",
        "https://auth.openai.com:123/codex/device",
        "https://auth.openai.com/codex/device#evil",
    ] {
        assert!(transport::challenge(Backend::Codex, "device_code", url, Some("ABCD")).is_err());
    }
    assert!(transport::validate_code("a\ncomando").is_err());
    assert!(transport::validate_code("$(id)").is_err());
    let mut parser = transport::Presentation::default();
    assert!(parser.push(b"\x1b]52;c;segredo\x07https://cla").unwrap().is_none());
    let challenge = parser.push(b"ude.ai/oauth/authorize?state=ficticio\n").unwrap().unwrap();
    assert_eq!(challenge.display_domain, "claude.ai");
    assert!(!challenge.url.contains("segredo"));
}
#[test]
fn dto_e_parse_nao_concedem_autoridade_a_cmd_ou_path() {
    assert!(
        parse::<Create>(json!({"backend":"codex","label":"Ficticia","kind":"cli","clientRequestId":"test","cmd":"sh"}))
            .is_err()
    );
    assert!(parse::<Start>(json!({"engineAccountId":"test","expectedRevision":1,"action":"login","clientRequestId":"test","path":"/tmp"})).is_err());
    assert!(require_local(&Caller::Pairing { key: "teste".into() }).is_err());
}

struct Fixture {
    hub: Arc<Hub>,
    root: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        use crate::remote::identity::Identity;
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let root = store.engine_setup_root.clone();
        std::fs::create_dir_all(&root).unwrap();
        let identity = Identity::load_or_create(&root).unwrap();
        let hub = Hub::new(store, "fixture-host".into(), identity, "fixture-token".into(), 0);
        Self { hub, root }
    }
    fn cli(&self, backend: Backend) -> std::path::PathBuf {
        let name = format!(
            "{}-{}-{}",
            backend.id(),
            if backend == Backend::Codex { "0.164.0" } else { "2.1.309" },
            std::env::consts::ARCH
        );
        let dir = self.root.join("bin").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join(backend.id());
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/engine_setup_cli.py"),
            &target,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        target
    }
    async fn create(&self, backend: &str, kind: &str, request: &str) -> Value {
        call(
            &self.hub,
            &Caller::Local,
            "donnaEngineAccountCreate",
            json!({"backend":backend,"kind":kind,"label":"Fictícia","clientRequestId":request}),
        )
        .await
        .unwrap()
    }
    async fn start(&self, account: &Value, request: &str) -> Value {
        call(&self.hub,&Caller::Local,"donnaEngineSetupStart",json!({"engineAccountId":account["id"],"expectedRevision":account["revision"],"action":"login","clientRequestId":request})).await.unwrap()
    }
    fn action(&self, op: &Value, kind: Value) -> Result<Value> {
        let mut body = json!({"operationId":op["operationId"],"generation":op["generation"]});
        let Value::Object(fields) = kind else { panic!("ação inválida") };
        for (key, value) in fields {
            body[&key] = value;
        }
        action(&self.hub, parse(body)?)
    }
    async fn phase(&self, op: &Value, target: &str) -> Value {
        tokio::time::timeout(TEST_WAIT, async {
            loop {
                let value = self.action(op, json!({"action":"read"})).unwrap();
                if value["phase"] == target {
                    return value;
                }
                assert!(
                    !["failed", "expired", "cancelled"].iter().any(|phase| value["phase"] == *phase),
                    "fase inesperada ao esperar {target}: {value}"
                );
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("operação não chegou a {target} em 60 s"))
    }

    fn account(&self, id: &Value) -> Value {
        Registry::load(&self.hub.store).unwrap().dto_for(&self.hub.store, id.as_str().unwrap()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn codex_profile_finalization_waits_for_eof_cleanup() {
    let f = Fixture::new();
    let cli = f.cli(Backend::Codex);
    std::fs::write(cli.parent().unwrap().join("scenario"), "codex_arg0_cleanup").unwrap();
    let account = f.create("codex", "cli", "arg0-create").await;
    let op = f.start(&account, "arg0-login").await;
    f.phase(&op, "connected").await;
    let registry = Registry::load(&f.hub.store).unwrap();
    let connected = registry.account(account["id"].as_str().unwrap()).unwrap();
    let LaunchReference::CliProfile { directory, .. } = &connected.launch else { panic!("perfil ausente") };
    assert!(directory.parent().unwrap().join(".fixture-eof-complete").is_file());
    assert!(!directory.join("tmp/arg0-linux/codex-arg0fixture").exists());
    assert!(connected.subscription_verified());

    // Graceful cleanup never permits a different symlink or follows it to chmod a target.
    #[cfg(unix)]
    {
        use std::os::unix::{fs::PermissionsExt, fs::symlink};
        let outside = f.root.join("outside");
        std::fs::write(&outside, "preservado").unwrap();
        std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o640)).unwrap();
        symlink(&outside, directory.join("untrusted-link")).unwrap();
        assert!(
            managed::secure_files(directory.parent().unwrap()).unwrap_err().to_string().starts_with("profile_invalid")
        );
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "preservado");
        assert_eq!(std::fs::metadata(&outside).unwrap().permissions().mode() & 0o777, 0o640);
    }
}

#[tokio::test]
async fn codex_profile_finalization_refuses_nonzero_shutdown_and_preserves_old_login() {
    let f = Fixture::new();
    let cli = f.cli(Backend::Codex);
    let scenario = cli.parent().unwrap().join("scenario");
    std::fs::write(&scenario, "codex_arg0_cleanup").unwrap();
    let account = f.create("codex", "cli", "arg0-preserve-create").await;
    let op = f.start(&account, "arg0-preserve-first").await;
    f.phase(&op, "connected").await;
    let before = f.account(&account["id"]);
    let registry = Registry::load(&f.hub.store).unwrap();
    let launch = registry.account(account["id"].as_str().unwrap()).unwrap().launch.clone();
    std::fs::write(&scenario, "codex_arg0_nonzero").unwrap();
    let next = f.start(&before, "arg0-preserve-second").await;
    let failed = f.phase(&next, "failed").await;
    assert_eq!(failed["errorCode"], "auth_shutdown_failed");
    assert_eq!(f.account(&account["id"]), before);
    let registry = Registry::load(&f.hub.store).unwrap();
    assert_eq!(registry.account(account["id"].as_str().unwrap()).unwrap().launch, launch);
}

#[tokio::test]
async fn duas_contas_perfis_estaveis_login_correlacionado_uso_e_reconnect_identidade() {
    let f = Fixture::new();
    let cli = f.cli(Backend::Codex);
    let first = f.create("codex", "cli", "first").await;
    let repeated = f.create("codex", "cli", "first").await;
    assert_eq!(first, repeated);
    let second = f.create("codex", "cli", "second").await;
    assert_ne!(first["id"], second["id"]);
    let op = f.start(&first, "login-first").await;
    f.phase(&op, "connected").await;
    let first = f.account(&first["id"]);
    assert_eq!(first["state"], "connected");
    assert_eq!(first["revision"], 2);
    let op2 = f.start(&second, "login-second").await;
    f.phase(&op2, "connected").await;
    let second = f.account(&second["id"]);
    let bot:crate::store::BotConfig = serde_json::from_value(json!({"id":"bot-fixture","name":"Fixture","backend":"codex","forkEditor":{"engineAccountId":first["id"]},"model":"gpt-6.1-sol","modelOptions":{"reasoning_effort":"high"}})).unwrap();
    managed::validate_selection(&f.hub.store, &bot).unwrap();
    f.hub.store.save_bot(&bot).unwrap(); // F6-A: a leitura de uso exige o vínculo da conta com o Space (o Pessoal)
    let launch = managed::resolve_for_bot(&f.hub.store, &bot).unwrap().unwrap();
    let first_profile = launch.env.iter().find(|(k, _)| k == "CODEX_HOME").unwrap().1.clone();
    let mut changed = bot.clone();
    changed.fork_editor.engine_account_id = Some(second["id"].as_str().unwrap().into());
    let launch2 = managed::resolve_for_bot(&f.hub.store, &changed).unwrap().unwrap();
    assert_ne!(launch.env, launch2.env);
    assert!(!first_profile.contains("owner@"));
    assert_eq!(usage(&f.hub, first["id"].as_str().unwrap().into(), None).await.unwrap()["windows"][0]["percent"], 23.0);
    // Reconectar em perfil vazio com outro titular não retargeta a conta nem troca o perfil.
    std::fs::write(cli.parent().unwrap().join("identity"), "different@example.invalid").unwrap();
    let reconnect = f.start(&first, "reconnect-different").await;
    let state = f.phase(&reconnect, "failed").await;
    assert_eq!(state["errorCode"], "identity_changed");
    assert_eq!(managed::resolve_for_bot(&f.hub.store, &bot).unwrap().unwrap().env, launch.env);
    assert_eq!(f.account(&first["id"])["revision"], 2);
    let registry = Registry::load(&f.hub.store).unwrap();
    assert_eq!(
        registry.accounts.iter().find(|a| a.id == first["id"]).unwrap().quota_identity,
        registry.accounts.iter().find(|a| a.id == second["id"]).unwrap().quota_identity
    );
}

#[tokio::test]
async fn cancelar_stale_restart_e_caller_nao_promovem_nem_expoem_desafio() {
    let f = Fixture::new();
    let cli = f.cli(Backend::Codex);
    std::fs::write(cli.parent().unwrap().join("scenario"), "hang").unwrap();
    let account = f.create("codex", "cli", "cancel").await;
    let before = f.hub.store.kv_read("fork.engine-accounts.v1").unwrap().unwrap();
    let op = f.start(&account, "cancel-login").await;
    let pending = f.phase(&op, "awaiting_authorization").await;
    assert!(pending["challenge"].is_object());
    assert!(f.action(&op, json!({"action":"terminal_input","data":"bHMK"})).is_err());
    assert!(f.action(&op, json!({"action":"read","cmd":"sh"})).is_err());
    let mut stale = op.clone();
    stale["generation"] = 999.into();
    assert!(f.action(&stale, json!({"action":"cancel"})).is_err());
    let cancelled = f.action(&op, json!({"action":"cancel"})).unwrap();
    assert_eq!(cancelled["phase"], "cancelled");
    assert!(cancelled["challenge"].is_null());
    assert!(f.action(&op, json!({"action":"submit_code","code":"FIXTURE-CODE"})).is_err());
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(f.account(&account["id"])["state"], "unverified");
    assert_eq!(f.account(&account["id"])["revision"], 1);
    assert!(before.contains("generation"));
    assert!(!f.hub.store.kv_read("fork.engine-accounts.v1").unwrap().unwrap().contains("FIXTURE-CODE"));
    // Ausência da memória de operação (reinício) encerra o pedido sem relançá-lo.
    OPERATIONS.locked().as_mut().unwrap().remove(op["operationId"].as_str().unwrap());
    assert!(f.action(&op, json!({"action":"read"})).unwrap_err().to_string().starts_with("operation_expired"));
    let device = Caller::Device { key: "fixture".into(), scopes: vec![crate::store::Scope::Control], autoridade: None };
    assert!(
        crate::api::dispatch(&f.hub, &device, "donnaEngineAccountCreate", json!({}))
            .await
            .unwrap_err()
            .to_string()
            .starts_with("remote_setup_unavailable")
    );
    std::fs::write(cli.parent().unwrap().join("scenario"), "stale").unwrap();
    let next = f.start(&account, "stale-login").await;
    let value = f.phase(&next, "failed").await;
    assert_eq!(value["errorCode"], "auth_correlation_invalid");
}

#[tokio::test]
async fn api_explicita_preserva_legado_e_cofre_bytes_e_perfil_separado() {
    let f = Fixture::new();
    crate::market::vault::write(&f.hub.store, "agent-env:codex", r#"{"OPENAI_API_KEY":"LEGACY_FIXTURE"}"#).unwrap();
    let before = f.hub.store.kv_read("agent-env:codex").unwrap();
    let account = f.create("codex", "api", "api-first").await;
    assert!(
        set_api_env(
            &f.hub,
            &Caller::Local,
            json!({"engineAccountId":account["id"],"expectedRevision":1,"backend":"codex","vars":{"HOME":"/tmp"}})
        )
        .await
        .is_err()
    );
    let saved = set_api_env(&f.hub,&Caller::Local,json!({"engineAccountId":account["id"],"expectedRevision":1,"backend":"codex","vars":{"OPENAI_API_KEY":"API_FIXTURE"}})).await.unwrap();
    assert_eq!(saved["billing"], "paid_api");
    assert_eq!(saved["state"], "connected");
    let bot: crate::store::BotConfig = serde_json::from_value(
        json!({"id":"api-bot","name":"Fixture","backend":"codex","forkEditor":{"engineAccountId":account["id"]}}),
    )
    .unwrap();
    f.hub.store.save_bot(&bot).unwrap();
    let launch = managed::resolve_for_bot(&f.hub.store, &bot).unwrap().unwrap();
    assert!(launch.env.iter().any(|(key, value)| key == "OPENAI_API_KEY" && value == "API_FIXTURE"));
    assert!(launch.env.iter().any(|(key, value)| key == "CODEX_HOME" && value.starts_with(f.root.to_str().unwrap())));
    assert_eq!(f.hub.store.kv_read("agent-env:codex").unwrap(), before);
    assert_eq!(usage(&f.hub, account["id"].as_str().unwrap().into(), None).await.unwrap()["state"], "unavailable");
    assert!(!saved.to_string().contains("API_FIXTURE"));
    let root = f.hub.store.engine_setup_root.clone();
    let registry = Registry::load(&f.hub.store).unwrap();
    let stored = registry.account(account["id"].as_str().unwrap()).unwrap();
    let profile = stored.managed.as_ref().unwrap().profile.as_ref().unwrap();
    assert!(profile.starts_with(root));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(profile).unwrap().permissions().mode() & 0o777, 0o700);
    }
}

#[tokio::test]
async fn claude_pty_so_aceita_codigo_no_processo_fechado_e_confirma_status_final() {
    let f = Fixture::new();
    let cli = f.cli(Backend::Claude);
    let account = f.create("claude", "cli", "claude-pty").await;
    let profile = managed::new_profile(&f.hub.store, account["id"].as_str().unwrap(), Backend::Claude).unwrap();
    let (events, mut output) = mpsc::channel(8);
    let (input, receiver) = mpsc::channel(4);
    let (_cancel, watch) = watch::channel(false);
    let executable = cli.clone();
    let directory = profile.clone();
    let task =
        tokio::spawn(
            async move { process::login(Backend::Claude, &executable, &directory, events, receiver, watch).await },
        );
    let challenge = output.recv().await.unwrap();
    let process::Event::Challenge(challenge) = challenge else { panic!("desafio ausente") };
    assert_eq!(challenge.kind, "paste_code");
    assert_eq!(challenge.display_domain, "claude.ai");
    assert!(!challenge.url.contains("TEST_SECRET"));
    input.send(process::Control::Resize(100, 24)).await.unwrap();
    input.send(process::Control::Code("FIXTURE-CODE".into())).await.unwrap();
    let proof = task.await.unwrap().unwrap();
    assert_eq!(proof.state, State::Connected);
    assert_eq!(proof.billing, Billing::Subscription);
    assert!(proof.identity.unwrap().contains("owner@example.invalid"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(profile.join(".fixture-auth")).unwrap().permissions().mode() & 0o777, 0o600);
    }
}

#[tokio::test]
async fn ttl_posse_e_symlink_impedem_entrada_e_lancamento() {
    let f = Fixture::new();
    let cli = f.cli(Backend::Codex);
    std::fs::write(cli.parent().unwrap().join("scenario"), "hang").unwrap();
    let account = f.create("codex", "cli", "ttl").await;
    let op = f.start(&account, "ttl-login").await;
    f.phase(&op, "awaiting_authorization").await;
    let stranger = Fixture::new();
    assert!(stranger.action(&op, json!({"action":"read"})).is_err());
    OPERATIONS.locked().as_mut().unwrap().get_mut(op["operationId"].as_str().unwrap()).unwrap().expires = 0;
    let expired = f.action(&op, json!({"action":"read"})).unwrap();
    assert_eq!(expired["phase"], "expired");
    assert!(expired["challenge"].is_null());
    assert!(f.action(&op, json!({"action":"open_terminal"})).is_err());
    #[cfg(unix)]
    {
        let registry = Registry::load(&f.hub.store).unwrap();
        let account = registry.account(account["id"].as_str().unwrap()).unwrap();
        let LaunchReference::CliProfile { directory, .. } = &account.launch else { panic!("perfil ausente") };
        std::fs::remove_dir(directory).unwrap();
        std::os::unix::fs::symlink(&stranger.root, directory).unwrap();
        assert!(managed::validate_profile(&f.hub.store, &account.id, directory).is_err());
    }
}

#[cfg(target_os = "linux")]
fn ler_pids(file: &std::path::Path) -> Option<Vec<u32>> {
    let contents = std::fs::read_to_string(file).ok()?;
    contents.lines().map(|pid| pid.parse::<u32>().ok()).collect()
}

#[cfg(target_os = "linux")]
async fn pids_iniciados(file: &std::path::Path, expected: usize) -> Vec<u32> {
    let file = file.to_owned();
    // I/O real: a tarefa bloqueante também inibe o autoavanço do Tokio pausado.
    tokio::task::spawn_blocking(move || {
        let deadline = std::time::Instant::now() + TEST_WAIT;
        loop {
            if let Some(pids) = ler_pids(&file)
                && pids.len() == expected
                && pids.iter().all(|pid| *pid > 0)
            {
                return pids;
            }
            assert!(std::time::Instant::now() < deadline, "a fixture deve publicar todos os PIDs numéricos");
            std::thread::sleep(Duration::from_millis(20));
        }
    })
    .await
    .unwrap()
}

#[cfg(target_os = "linux")]
async fn assert_group_stopped(file: &std::path::Path, expected: usize) {
    let pids = pids_iniciados(file, expected).await;
    esperar(|| {
        pids.iter().all(|pid| {
            let state = std::fs::read_to_string(format!("/proc/{pid}/stat"));
            state.is_err() || state.unwrap().split_whitespace().nth(2) == Some("Z")
        })
    })
    .await;
}

#[cfg(target_os = "linux")]
async fn avancar_deadline(deadline: tokio::time::Instant) {
    // O relógio já está pausado; só o teste vence o prazo após o sinal da fixture.
    tokio::time::advance(deadline.saturating_duration_since(tokio::time::Instant::now()) + Duration::from_millis(1))
        .await;
    tokio::time::resume();
}

#[cfg(target_os = "linux")]
async fn expirar_apos_inicio<T>(
    marker: &std::path::Path,
    cancel: watch::Receiver<bool>,
    work: impl Future<Output = Result<T>>,
) -> Result<T> {
    tokio::time::pause();
    let inicio = tokio::time::Instant::now();
    let deadline = inicio + Duration::from_secs(1);
    let work = until_deadline(deadline, cancel, work);
    tokio::pin!(work);
    tokio::select! {
        result = &mut work => panic!("o trabalho encerrou antes de a fixture iniciar: {:?}", result.err()),
        _ = pids_iniciados(marker, 2) => {}
    }
    assert_eq!(tokio::time::Instant::now(), inicio, "a subida não pode consumir o prazo observado");
    avancar_deadline(deadline).await;
    work.await
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn prazo_original_encerra_busca_executavel_sem_read_e_retry_expira_sem_promover() {
    let f = Fixture::new();
    let cli = f.cli(Backend::Codex);
    std::fs::write(cli.parent().unwrap().join("scenario"), "version_hang").unwrap();
    let account = f.create("codex", "cli", "deadline-search").await;
    let before = Registry::load(&f.hub.store).unwrap().account(account["id"].as_str().unwrap()).unwrap().launch.clone();
    let request: Start = parse(json!({"engineAccountId":account["id"],"expectedRevision":1,"action":"login","clientRequestId":"deadline-search-login"})).unwrap();
    let marker = cli.parent().unwrap().join("fixture-version-pids");
    tokio::time::pause();
    let inicio = tokio::time::Instant::now();
    // A espera bloqueante protege inclusive o await do mutex de criação da operação.
    let (op, _) = tokio::join!(start_with_ttl(&f.hub, request, OPERATION_TTL), pids_iniciados(&marker, 2));
    let op = op.unwrap();
    let deadline = OPERATIONS.locked().as_ref().unwrap()[op["operationId"].as_str().unwrap()].deadline;
    // Não chamar read: o worker precisa encerrar sozinho a busca de executável e seu grupo.
    assert_eq!(tokio::time::Instant::now(), inicio, "a subida não pode consumir o prazo observado");
    avancar_deadline(deadline).await;
    assert_group_stopped(&marker, 2).await;
    let retry = f.start(&account, "deadline-search-login").await;
    assert_eq!(retry["phase"], "expired");
    assert!(retry["challenge"].is_null());
    assert!(retry["terminal"].is_null());
    assert_eq!(retry["operationId"], op["operationId"]);
    assert_eq!(f.account(&account["id"])["revision"], 1);
    assert_eq!(Registry::load(&f.hub.store).unwrap().account(account["id"].as_str().unwrap()).unwrap().launch, before);
    let profiles = f.root.join("accounts").join(account["id"].as_str().unwrap());
    esperar(|| std::fs::read_dir(&profiles).unwrap().count() == 1).await;
    assert_eq!(std::fs::read_dir(profiles).unwrap().count(), 1, "perfil de tentativa deve ser limpo");
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn mesmo_deadline_cancela_handshake_e_catalogo_posterior_com_grupo_inteiro() {
    let f = Fixture::new();
    let cli = f.cli(Backend::Claude);
    let account = f.create("claude", "cli", "catalog-deadline").await;
    let LaunchReference::CliProfile { directory, .. } =
        Registry::load(&f.hub.store).unwrap().account(account["id"].as_str().unwrap()).unwrap().launch.clone()
    else {
        panic!("perfil ausente")
    };
    let adapter = f.root.join("claude-fixture-acp");
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/engine_setup_catalog.py"),
        &adapter,
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&adapter, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let commands = vec![crate::shell::quote(&adapter.to_string_lossy())];
    for stage in ["initialize", "session"] {
        std::fs::write(directory.join("fixture-hang-stage"), stage).unwrap();
        let (_tx, cancel) = watch::channel(false);
        let marker = directory.join("fixture-catalog-pids");
        let _ = std::fs::remove_file(&marker);
        let error = expirar_apos_inicio(&marker, cancel, process::catalog_from_commands(&cli, &directory, &commands))
            .await
            .unwrap_err();
        assert!(error.to_string().starts_with("setup_expired"));
        assert_group_stopped(&marker, 2).await;
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn deadline_cancela_tambem_processo_auxiliar_de_descoberta() {
    let f = Fixture::new();
    let cli = f.cli(Backend::Codex);
    std::fs::write(cli.parent().unwrap().join("scenario"), "version_hang").unwrap();
    let account = f.create("codex", "cli", "auxiliary-deadline").await;
    let LaunchReference::CliProfile { directory, .. } =
        Registry::load(&f.hub.store).unwrap().account(account["id"].as_str().unwrap()).unwrap().launch.clone()
    else {
        panic!("perfil ausente")
    };
    let mut command = process::command(&cli, &directory, Backend::Codex);
    command.arg("--version");
    let (_tx, cancel) = watch::channel(false);
    let marker = cli.parent().unwrap().join("fixture-version-pids");
    let error = expirar_apos_inicio(&marker, cancel, async { Ok(discovery_output(command).await?) }).await.unwrap_err();
    assert!(error.to_string().starts_with("setup_expired"));
    assert_group_stopped(&marker, 2).await;
}

#[cfg(target_os = "linux")]
async fn pty_fechado(cli: &std::path::Path) -> Value {
    let marker = cli.parent().unwrap().join("pty-closed.json");
    tokio::time::timeout(TEST_WAIT, async {
        loop {
            if let Ok(bytes) = std::fs::read(&marker)
                && let Ok(value) = serde_json::from_slice(&bytes)
            {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("fixture não fechou PTY")
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn claude_pty_eof_aguarda_exit_real_e_exige_status_final_sem_relogin() {
    for scenario in ["pty_eof_success", "pty_eof_nonzero", "pty_eof_unverified"] {
        let f = Fixture::new();
        let cli = f.cli(Backend::Claude);
        let base = cli.parent().unwrap();
        std::fs::write(base.join("scenario"), scenario).unwrap();
        let account = f.create("claude", "cli", scenario).await;
        let profile = managed::new_profile(&f.hub.store, account["id"].as_str().unwrap(), Backend::Claude).unwrap();
        let (events, mut output) = mpsc::channel(8);
        let (input, receiver) = mpsc::channel(4);
        let (_cancel, watch) = watch::channel(false);
        let executable = cli.clone();
        let directory = profile.clone();
        let mut task = tokio::spawn(async move {
            process::login(Backend::Claude, &executable, &directory, events, receiver, watch).await
        });
        let event = output.recv().await.unwrap();
        let process::Event::Challenge(challenge) = event else { panic!("desafio ausente") };
        assert_eq!(challenge.kind, "paste_code");
        assert_eq!(challenge.display_domain, "claude.ai");
        input.send(process::Control::Code("FIXTURE-CODE".into())).await.unwrap();
        let marker = pty_fechado(&cli).await;
        assert_eq!(marker["profile"], profile.to_string_lossy().as_ref());
        let early = tokio::time::timeout(Duration::from_millis(250), &mut task).await;
        assert!(
            early.is_err(),
            "EOF não é resultado de processo: {scenario}; resultado={:?}",
            early.ok().map(|joined| joined.map(|result| result.err().map(|error| error.to_string())))
        );
        assert!(matches!(output.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
        assert!(!base.join("pty-status-calls").exists(), "status antes de exit confirmado");
        assert_eq!(std::fs::read_to_string(base.join("pty-login-pids")).unwrap().lines().count(), 1);
        std::fs::write(base.join("pty-release"), "release").unwrap();
        let outcome = tokio::time::timeout(TEST_WAIT, task).await.unwrap().unwrap();
        assert_group_stopped(&base.join("pty-login-pids"), 1).await;
        if scenario == "pty_eof_nonzero" {
            assert_eq!(outcome.err().expect("exit não zero aceito").to_string(), "auth_failed");
            assert!(!base.join("pty-status-calls").exists());
            assert!(output.recv().await.is_none());
        } else {
            let proof = outcome.unwrap();
            assert!(matches!(output.recv().await, Some(process::Event::Verifying)));
            assert_eq!(std::fs::read_to_string(base.join("pty-status-calls")).unwrap(), "status\n");
            if scenario == "pty_eof_success" {
                assert_eq!(proof.state, State::Connected);
                assert_eq!(proof.billing, Billing::Subscription);
                assert!(proof.identity.unwrap().contains("owner@example.invalid"));
                assert!(proof.subscription_proof.is_some());
            } else {
                assert_eq!(proof.state, State::Disconnected);
                assert!(proof.identity.is_none());
                assert!(proof.subscription_proof.is_none());
            }
        }
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn claude_pty_eof_preserva_deadline_cancelamento_correlacao_e_perfil() {
    for cancelled in [false, true] {
        let f = Fixture::new();
        let cli = f.cli(Backend::Claude);
        let base = cli.parent().unwrap();
        std::fs::write(base.join("scenario"), "pty_eof_success").unwrap();
        let account = f.create("claude", "cli", "pty-deadline").await;
        let before = Registry::load(&f.hub.store).unwrap().account(account["id"].as_str().unwrap()).unwrap().clone();
        let request: Start = parse(json!({"engineAccountId":account["id"],"expectedRevision":1,"action":"login","clientRequestId":"pty-deadline-login"})).unwrap();
        let marker = base.join("pty-login-pids");
        tokio::time::pause();
        let inicio = tokio::time::Instant::now();
        let (op, _) = tokio::join!(start_with_ttl(&f.hub, request, OPERATION_TTL), pids_iniciados(&marker, 1));
        let op = op.unwrap();
        let deadline = OPERATIONS.locked().as_ref().unwrap().get(op["operationId"].as_str().unwrap()).unwrap().deadline;
        assert_eq!(tokio::time::Instant::now(), inicio, "a subida não pode consumir o prazo observado");
        tokio::time::resume();
        f.phase(&op, "awaiting_authorization").await;
        let mut stale = op.clone();
        stale["generation"] = json!(op["generation"].as_u64().unwrap() + 1);
        assert!(f.action(&stale, json!({"action":"submit_code","code":"FIXTURE-CODE"})).is_err());
        f.action(&op, json!({"action":"submit_code","code":"FIXTURE-CODE"})).unwrap();
        let marker = pty_fechado(&cli).await;
        tokio::time::sleep(Duration::from_millis(250)).await;
        let live = f.action(&op, json!({"action":"read"})).unwrap();
        assert_eq!(live["phase"], "verifying", "EOF sozinho não encerra operação");
        assert_eq!(live["expiresAt"], op["expiresAt"]);
        let repeated = f.start(&account, "pty-deadline-login").await;
        assert_eq!(repeated["operationId"], op["operationId"]);
        assert_eq!(repeated["generation"], op["generation"]);
        assert_eq!(repeated["expiresAt"], op["expiresAt"]);
        assert_eq!(
            OPERATIONS.locked().as_ref().unwrap().get(op["operationId"].as_str().unwrap()).unwrap().deadline,
            deadline
        );
        assert_eq!(std::fs::read_to_string(base.join("pty-login-pids")).unwrap().lines().count(), 1);
        assert!(!base.join("pty-status-calls").exists());
        if cancelled {
            assert!(f.action(&stale, json!({"action":"cancel"})).is_err());
            let result = f.action(&op, json!({"action":"cancel"})).unwrap();
            assert_eq!(result["phase"], "cancelled");
        } else {
            // Nenhum read até o deadline: o worker deve encerrar sozinho, sem renovar TTL no EOF.
            tokio::time::pause();
            avancar_deadline(deadline).await;
        }
        let staging = std::path::PathBuf::from(marker["profile"].as_str().unwrap());
        tokio::time::timeout(TEST_WAIT, async {
            while staging.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("perfil staging não foi limpo");
        assert_group_stopped(&base.join("pty-login-pids"), 1).await;
        let result = f.action(&op, json!({"action":"read"})).unwrap();
        assert_eq!(result["phase"], if cancelled { "cancelled" } else { "expired" });
        assert!(result["challenge"].is_null());
        assert!(result["terminal"].is_null());
        let after = Registry::load(&f.hub.store).unwrap().account(account["id"].as_str().unwrap()).unwrap().clone();
        assert_eq!(after.launch, before.launch);
        assert_eq!(after.managed.as_ref().unwrap().revision, before.managed.as_ref().unwrap().revision);
        assert_eq!(after.managed.as_ref().unwrap().principal, before.managed.as_ref().unwrap().principal);
        assert_ne!(after.state, State::Connected);
        assert!(!base.join("pty-status-calls").exists());
        assert!(!base.join("pty-release").exists());
        eprintln!(
            "D171_PTY_BOUNDARY op={} generation={} request=pty-deadline-login remaining_ms={} result={}",
            op["operationId"],
            op["generation"],
            deadline.saturating_duration_since(tokio::time::Instant::now()).as_millis(),
            result["phase"]
        );
    }
}

impl Fixture {
    /// Cadeia ACS4 completa, com assinaturas próprias e sem rede/credencial real.
    fn owner(&self) -> Caller {
        use crate::{
            fork::bootstrap::codec::{self, Challenge, Grant, Proof, Receipt},
            remote::{cloud, crypto},
            store::bootstrap::{CloudGrant, Publication, TrustedGeneration},
        };
        use ed25519_dalek::SigningKey;
        let issuer = SigningKey::from_bytes(&[21; 32]);
        let device = SigningKey::from_bytes(&[22; 32]);
        let host = &self.hub.identity.sign;
        let now = crate::store::now_ms();
        let cfg = TrustedGeneration {
            generation: crypto::b64(&[23; 32]),
            issuer_key_id: "issuer".into(),
            issuer_key: crypto::b64(issuer.verifying_key().as_bytes()),
            issuer: "donna".into(),
            deployment_id: "fixture".into(),
            env: "test".into(),
            manifest_hash: "a".repeat(64),
            resource_digest: "b".repeat(64),
            computer_id: "computer".into(),
            sign_key: crypto::b64(host.verifying_key().as_bytes()),
            box_key: crypto::b64(&[24; 32]),
        };
        let g = Grant(
            "donna/bootstrap-grant/v1".into(),
            "boot".into(),
            "intent".into(),
            cfg.resource_digest.clone(),
            cfg.generation.clone(),
            cfg.issuer_key_id.clone(),
            cfg.issuer.clone(),
            cfg.deployment_id.clone(),
            cfg.env.clone(),
            cfg.manifest_hash.clone(),
            "owner".into(),
            "device".into(),
            crypto::b64(device.verifying_key().as_bytes()),
            crypto::b64(&[25; 32]),
            cfg.computer_id.clone(),
            cfg.sign_key.clone(),
            cfg.box_key.clone(),
            crypto::b64(&[26; 32]),
            now - 1000,
            now + 200_000,
            vec!["control".into(), "owner.devices".into()],
        );
        let store = &self.hub.store;
        store.bootstrap_install_generation(&cfg).unwrap();
        store.bootstrap_install_grant(&codec::encode(&g, |b| crypto::sign(&issuer, b)).unwrap(), now).unwrap();
        let wire = store.bootstrap_challenge(host, "boot", &g.12, now).unwrap();
        let ch: Challenge = codec::decode(&wire, &host.verifying_key().to_bytes()).unwrap();
        let proof = Proof(
            "donna/bootstrap-proof/v1".into(),
            g.1.clone(),
            g.4.clone(),
            codec::hash(&g).unwrap(),
            codec::hash(&ch).unwrap(),
        );
        let wire = store
            .bootstrap_commit_proof(host, &codec::encode(&proof, |b| crypto::sign(&device, b)).unwrap(), now)
            .unwrap();
        let r: Receipt = codec::decode(&wire, &host.verifying_key().to_bytes()).unwrap();
        let publication = Publication {
            bootstrap_id: r.1.clone(),
            generation: r.2.clone(),
            grant_id: r.11.clone(),
            device_key: r.8.clone(),
            owner_user_id: r.6.clone(),
            receipt_hash: codec::hash(&r).unwrap(),
        };
        let grant = CloudGrant { grant_id: r.11, device_key: r.8.clone(), owner_user_id: r.6 };
        assert!(store.bootstrap_promote(&publication, &grant, now).unwrap());
        assert!(store.bootstrap_activation_envelope(host, now).unwrap().is_some());
        cloud::update_status(&self.hub, |s| s.owner = Some(cloud::Owner { user_id: "owner".into(), email: None }));
        self.hub.cloud_synced.store(true, std::sync::atomic::Ordering::Release);
        crate::fork::autoridade::autenticar(&self.hub, &r.8).unwrap().1
    }
    async fn remote(&self, caller: &Caller, method: &str, body: Value) -> Value {
        crate::api::dispatch(&self.hub, caller, method, body).await.unwrap()
    }
    async fn remote_phase(&self, caller: &Caller, op: &Value, phase: &str) -> Value {
        tokio::time::timeout(TEST_WAIT, async {
            loop {
                let value = self
                    .remote(
                        caller,
                        "donnaEngineSetupAction",
                        json!({"operationId":op["operationId"],"generation":op["generation"],"action":"read"}),
                    )
                    .await;
                if value["phase"] == phase {
                    return value;
                }
                assert!(!["failed", "expired", "cancelled"].iter().any(|s| value["phase"] == *s), "{value}");
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("setup remoto não chegou a {phase} em 60 s"))
    }
}

#[tokio::test]
async fn saas04_dono_login_assincrono_selecao_replay_e_wire_privado() {
    let f = Fixture::new();
    let cli = f.cli(Backend::Codex);
    std::fs::write(cli.parent().unwrap().join("scenario"), "controlled_login").unwrap();
    let owner = f.owner();
    assert_eq!(f.remote(&owner, "donnaEngineAccess", json!({})).await["canManage"], true);
    let body = json!({"backend":"codex","kind":"cli","label":"Dono","clientRequestId":"remote-create"});
    let account = f.remote(&owner, "donnaEngineAccountCreate", body.clone()).await;
    assert_eq!(f.remote(&owner, "donnaEngineAccountCreate", body).await, account);
    let body =
        json!({"engineAccountId":account["id"],"expectedRevision":1,"action":"login","clientRequestId":"remote-login"});
    let op = f.remote(&owner, "donnaEngineSetupStart", body.clone()).await;
    assert_eq!(op["phase"], "preparing"); // responde antes de concluir o login
    let challenge = f.remote_phase(&owner, &op, "awaiting_authorization").await;
    assert_eq!(challenge["challenge"]["url"], "https://auth.openai.com/codex/device");
    assert_eq!(f.remote(&owner, "donnaEngineSetupStart", body.clone()).await["operationId"], op["operationId"]);
    let mut conflict = body.clone();
    conflict["expectedRevision"] = 99.into();
    assert!(crate::api::dispatch(&f.hub, &owner, "donnaEngineSetupStart", conflict).await.is_err());
    std::fs::write(cli.parent().unwrap().join("login-release"), "liberar").unwrap();
    f.remote_phase(&owner, &op, "connected").await;
    assert_eq!(f.remote(&owner, "donnaEngineSetupStart", body).await["phase"], "connected");
    let list = f.remote(&owner, "donnaEngineAccounts", json!({"refresh":true})).await;
    for secret in ["FIXTURE-CODE", "owner@example.invalid", ".fixture-auth", "auth.openai.com"] {
        assert!(!list.to_string().contains(secret));
    }
    let bot = f
        .remote(
            &owner,
            "createBot",
            json!({"name":"Fixture","backend":"codex","forkEditor":{"engineAccountId":account["id"]}}),
        )
        .await;
    let id = &bot["bot"]["id"];
    let mut delegated = f.hub.store.device(owner.device_key()).unwrap();
    delegated.key = "delegated".into();
    f.hub.store.put_device(&delegated).unwrap();
    let other = Caller::Device {
        key: "delegated".into(),
        scopes: vec![crate::store::Scope::Control, crate::store::Scope::OwnerDevices],
        autoridade: None,
    };
    let limited = f.remote(&other, "donnaBotEditor", json!({"botId":id})).await;
    assert_eq!(limited["engineAccounts"], json!([]));
    assert!(limited["engineRuntime"].is_null());
    assert!(crate::api::dispatch(&f.hub, &other, "donnaBotEditorSave", json!({"botId":id,"expectedVersion":limited["settingsVersion"],"patch":{"forkEditor":{"engineAccountId":null}}})).await.is_err());
    let editor = f.remote(&owner, "donnaBotEditor", json!({"botId":id})).await;
    let saved=f.remote(&owner,"donnaBotEditorSave",json!({"botId":id,"expectedVersion":editor["settingsVersion"],"patch":{"forkEditor":{"engineAccountId":account["id"]}}})).await;
    assert_eq!(saved["profile"]["forkEditor"]["engineAccountId"], account["id"]);
    assert!(f.remote(&owner, "teamSettings", json!({})).await.get("primaryBotId").is_some());
    for method in ["setTeamSettings", "cloudStatus", "setCloud", "computerCall", "manageCall", "donnaRules"] {
        assert!(crate::api::dispatch(&f.hub, &owner, method, json!({})).await.is_err(), "{method}");
    }
    assert!(
        crate::api::dispatch(&f.hub, &owner, "setAgentEnv", json!({"engineAccountId":account["id"]})).await.is_err()
    );
    assert!(crate::api::dispatch(&f.hub, &owner, "agentAuthenticate", json!({"backend":"codex"})).await.is_err());
    // Conta em uso não pode desaparecer; desvincular e remover mantém a guarda original.
    assert!(
        crate::api::dispatch(
            &f.hub,
            &owner,
            "donnaEngineAccountRemove",
            json!({"engineAccountId":account["id"],"expectedRevision":2})
        )
        .await
        .is_err()
    );
    f.remote(&owner, "updateBot", json!({"id":id,"forkEditor":{"engineAccountId":null}})).await;
    // D372: desvincular o padrão não muda o retrato da conversa principal, que continua usando a conta.
    let remove = json!({"engineAccountId":account["id"],"expectedRevision":2});
    let error = crate::api::dispatch(&f.hub, &owner, "donnaEngineAccountRemove", remove.clone()).await.unwrap_err();
    assert!(format!("{error:#}").contains("account_in_use"), "{error:#}");
    // Sem referência viva (bot excluído pelo fluxo Local canônico), a remoção remota segue a guarda original.
    f.remote(&Caller::Local, "deleteBot", json!({"botId":id})).await;
    f.remote(&owner, "donnaEngineAccountRemove", remove).await;
}

/// A conversa principal do bot, como o app a lê (`sessions`, a principal primeiro).
async fn principal_de(f: &Fixture, bot: &str) -> Value {
    f.remote(&Caller::Local, "sessions", json!({"botId":bot})).await["sessions"][0].clone()
}

/// D422: no Berga (principal fixa), `sessionEngineSet` é do dono pelo app. Aparelho sem Control ou sem a autoridade
/// original, Pairing, Bootstrap e a Ponte real do Berga recusam sem efeito; o Device do dono (cadeia ACS4 canônica)
/// troca o modelo da conversa principal dele e a de outro bot, cada troca sem mexer na outra conversa; revogado entre a
/// captura e a escrita (`before_owner_write`), recusa e a sessão fica igual.
#[tokio::test]
async fn d422_donna_troca_da_conversa_so_pelo_dono_com_autoridade_original() {
    use crate::fork::sessoes::bindings as vinculos;
    use std::sync::atomic::{AtomicBool, Ordering};
    let f = Fixture::new();
    let owner = f.owner();
    let created = f.remote(&Caller::Local, "createBot", json!({"name":"Principal","backend":"codex"})).await;
    let donna = created["bot"]["id"].as_str().unwrap().to_owned();
    f.hub.store.set_team_settings(&json!({"primaryBotId":donna})).unwrap();
    let created = f.remote(&Caller::Local, "createBot", json!({"name":"Outro","backend":"codex"})).await;
    let other = created["bot"]["id"].as_str().unwrap().to_owned();
    let troca_em = |bot: &str, session: &Value, model: &str| {
        let engine = json!({"accountId":null,"backend":"codex","model":model,"modelOptions":{},"fastMode":false});
        json!({"botId":bot,"sessionId":null,"expectedRevision":session["revision"],"engine":engine})
    };
    let troca = |session: &Value, model: &str| troca_em(&donna, session, model);
    let before = principal_de(&f, &donna).await;
    let other_before = principal_de(&f, &other).await;
    assert_eq!(before["principal"], true);
    let device = |scopes: Vec<crate::store::Scope>| Caller::Device { key: "outro".into(), scopes, autoridade: None };
    for caller in [
        device(vec![crate::store::Scope::OwnerDevices]),
        device(vec![crate::store::Scope::Control]),
        Caller::Pairing { key: owner.device_key().into() },
        Caller::Bootstrap { key: owner.device_key().into(), bootstrap_id: "boot".into() },
    ] {
        let body = troca(&before, "nao-dono");
        assert!(crate::api::dispatch(&f.hub, &caller, "sessionEngineSet", body).await.is_err());
        assert_eq!(principal_de(&f, &donna).await, before, "recusa sem efeito");
    }
    assert!(!crate::fork::ponte_credencial::PERMITIDOS.contains(&"sessionEngineSet"), "Ponte (bot/MCP) não chega");
    // Ponte real do Berga: vínculo canônico da principal, credencial provisionada na lista de servidores da fixture e
    // autenticada só em memória (sem HTTP nem log do token).
    let binding = vinculos::issue(&f.hub.store, &donna, &donna, &donna, Some("acp-ponte")).unwrap();
    vinculos::activate(&f.hub.store, &binding, "acp-ponte").unwrap();
    let mut servers = vec![json!({"env":[],"args":["mcp","remote","--context","original"]})];
    crate::fork::ponte_credencial::provisionar(&f.hub, &donna, None, &binding, &mut servers).unwrap();
    let token = zeroize::Zeroizing::new(servers[0]["env"][0]["value"].as_str().unwrap().to_owned());
    let ponte = f.hub.pontes.autenticar(&f.hub, &token).unwrap();
    assert!(matches!(&ponte, Caller::Ponte { bot, .. } if *bot == donna));
    let body = troca(&before, "ponte");
    assert!(crate::api::dispatch(&f.hub, &ponte, "sessionEngineSet", body).await.is_err());
    assert_eq!(principal_de(&f, &donna).await, before, "Ponte recusada sem efeito");
    // O dono pelo app troca o modelo da conversa principal da Donna.
    let changed = f.remote(&owner, "sessionEngineSet", troca(&before, "modelo-dono")).await;
    assert_eq!(changed["session"]["engine"]["model"], "modelo-dono");
    assert_eq!(principal_de(&f, &other).await, other_before, "a conversa do outro bot não muda");
    // Outro bot, mesma regra: a troca dele não mexe na Donna.
    let current = principal_de(&f, &donna).await;
    let changed = f.remote(&owner, "sessionEngineSet", troca_em(&other, &other_before, "modelo-outro")).await;
    assert_eq!(changed["session"]["engine"]["model"], "modelo-outro");
    assert_eq!(principal_de(&f, &donna).await, current, "a Donna não muda");
    // Revogado depois da captura e antes da escrita: recusa e a sessão não muda.
    let fired = Arc::new(AtomicBool::new(false));
    {
        let (revoker, fired, key) = (f.hub.clone(), fired.clone(), owner.device_key().to_owned());
        *f.hub.store.before_owner_write.lock().unwrap() = Some(Box::new(move || {
            fired.store(true, Ordering::SeqCst);
            revoker.revoke_device(&key).unwrap();
        }));
    }
    let body = troca(&current, "revogado");
    assert!(crate::api::dispatch(&f.hub, &owner, "sessionEngineSet", body).await.is_err());
    assert!(fired.load(Ordering::SeqCst), "passou pelo ponto de gravação do dono");
    assert_eq!(principal_de(&f, &donna).await, current, "sessão inalterada");
}

#[tokio::test]
async fn saas04_nao_dono_pairing_grant_owner_expiracao_e_revogacao_recusados() {
    use crate::fork::bootstrap::codec::{self, Activation};
    use crate::store::{DeviceSource, Scope};
    let f = Fixture::new();
    let owner = f.owner();
    let key = owner.device_key();
    let mut other = f.hub.store.device(key).unwrap();
    other.key = "other".into();
    f.hub.store.put_device(&other).unwrap();
    f.hub.store.bootstrap_activation_envelope(&f.hub.identity.sign, crate::store::now_ms()).unwrap();
    for caller in [
        Caller::Device { key: "other".into(), scopes: vec![Scope::Control, Scope::OwnerDevices], autoridade: None },
        Caller::Pairing { key: key.into() },
        Caller::Bootstrap { key: key.into(), bootstrap_id: "boot".into() },
    ] {
        assert!(access::require(&f.hub, &caller).is_err());
        for method in [
            "donnaEngineAccounts",
            "donnaEngineAccountCreate",
            "donnaEngineAccountModelVisibility",
            "donnaEngineSetupStart",
            "donnaEngineSetupAction",
            "teamSettings",
        ] {
            assert!(crate::api::dispatch(&f.hub, &caller, method, json!({})).await.is_err(), "{method}");
        }
        assert!(crate::api::dispatch(&f.hub, &caller, "updateBot", json!({"id":"x","forkEditor":{}})).await.is_err());
    }
    assert!(access::require(&f.hub, &owner).is_ok());
    // Renovar o lease não muda a posse nem exige esperar a publicação do snapshot novo.
    let authority = access::resolve(&f.hub, &owner).unwrap();
    f.hub.store.set_lease(key, crate::store::now_ms() + crate::remote::cloud::LEASE_MS).unwrap();
    assert!(authority.current(&f.hub));
    f.hub.store.bootstrap_activation_envelope(&f.hub.identity.sign, crate::store::now_ms()).unwrap();
    assert!(authority.current(&f.hub));
    crate::remote::cloud::update_status(&f.hub, |s| s.owner.as_mut().unwrap().user_id = "new-owner".into());
    assert!(access::require(&f.hub, &owner).is_err());
    crate::remote::cloud::update_status(&f.hub, |s| s.owner.as_mut().unwrap().user_id = "owner".into());
    f.hub.cloud_synced.store(false, std::sync::atomic::Ordering::Release);
    assert!(access::require(&f.hub, &owner).is_err());
    f.hub.cloud_synced.store(true, std::sync::atomic::Ordering::Release);
    let original = f.hub.store.device(key).unwrap();
    let mut changed = original.clone();
    changed.grant_id = Some("other-grant".into());
    f.hub.store.put_device(&changed).unwrap();
    assert!(access::require(&f.hub, &owner).is_err());
    changed = original.clone();
    changed.source = DeviceSource::Local;
    f.hub.store.put_device(&changed).unwrap();
    assert!(access::require(&f.hub, &owner).is_err());
    changed = original.clone();
    changed.lease_until = Some(crate::store::now_ms() - 1);
    f.hub.store.put_device(&changed).unwrap();
    assert!(access::require(&f.hub, &owner).is_err());
    f.hub.store.put_device(&original).unwrap();
    let wire =
        f.hub.store.bootstrap_activation_envelope(&f.hub.identity.sign, crate::store::now_ms()).unwrap().unwrap();
    // Assinatura inválida nunca concede, mesmo com DB/owner/lease atuais.
    f.hub.store.db.locked().execute("UPDATE bootstrap_slot SET activation_wire=?1", [b"invalid".as_slice()]).unwrap();
    assert!(access::require(&f.hub, &owner).is_err());
    // Snapshot assinado mas vencido também não concede.
    let mut a: Activation =
        codec::decode(&serde_json::to_vec(&wire).unwrap(), &f.hub.identity.sign.verifying_key().to_bytes()).unwrap();
    a.13 = crate::store::now_ms() - 1;
    a.12 = a.13 - 1000;
    let expired = codec::encode(&a, |b| crate::remote::crypto::sign(&f.hub.identity.sign, b)).unwrap();
    f.hub.store.db.locked().execute("UPDATE bootstrap_slot SET activation_wire=?1", [expired]).unwrap();
    assert!(access::require(&f.hub, &owner).is_err());
    f.hub.store.remove_device(key).unwrap();
    assert!(access::require(&f.hub, &owner).is_err());
}

#[tokio::test]
async fn saas04_revogacao_em_login_cancela_sem_read_e_nao_promove_nem_reaproveita_id() {
    let f = Fixture::new();
    let cli = f.cli(Backend::Codex);
    let owner = f.owner();
    std::fs::write(cli.parent().unwrap().join("scenario"), "hang").unwrap();
    let account = f
        .remote(
            &owner,
            "donnaEngineAccountCreate",
            json!({"backend":"codex","kind":"cli","label":"Dono","clientRequestId":"revoke-create"}),
        )
        .await;
    let body =
        json!({"engineAccountId":account["id"],"expectedRevision":1,"action":"login","clientRequestId":"revoke-login"});
    let op = f.remote(&owner, "donnaEngineSetupStart", body.clone()).await;
    f.remote_phase(&owner, &op, "awaiting_authorization").await;
    assert!(
        call(
            &f.hub,
            &Caller::Local,
            "donnaEngineSetupAction",
            json!({"operationId":op["operationId"],"generation":op["generation"],"action":"read"})
        )
        .await
        .is_err()
    );
    let access::Authority::Owner { binding, .. } = access::resolve(&f.hub, &owner).unwrap() else {
        panic!("expected owner")
    };
    f.hub.store.remove_device(owner.device_key()).unwrap();
    let before = f.hub.store.kv_read(accounts::SLOT).unwrap();
    assert!(
        f.hub.store.bootstrap_engine_set(owner.device_key(), "owner", &binding, accounts::SLOT, "invalid").is_err()
    );
    assert_eq!(f.hub.store.kv_read(accounts::SLOT).unwrap(), before);
    // Observa o monitor, sem chamar read (que também revogaria a operação).
    esperar(|| {
        let guard = OPERATIONS.locked();
        guard.as_ref().unwrap()[op["operationId"].as_str().unwrap()].phase == Phase::Cancelled
    })
    .await;
    {
        let guard = OPERATIONS.locked();
        let stored = &guard.as_ref().unwrap()[op["operationId"].as_str().unwrap()];
        assert!(stored.phase == Phase::Cancelled);
        assert!(stored.challenge.is_none());
    }
    assert_eq!(f.account(&account["id"])["revision"], 1);
    assert!(crate::api::dispatch(&f.hub, &owner, "donnaEngineSetupStart", body.clone()).await.is_err());
    assert!(call(&f.hub, &Caller::Local, "donnaEngineSetupStart", body).await.is_err());
}

// Provas independentes da revisão: só fixtures próprias, sem rede/conta real.
async fn review_ready() -> (Fixture, Caller, Value) {
    let f = Fixture::new();
    f.cli(Backend::Codex);
    let owner = f.owner();
    let account = f.create("codex", "cli", "review-create").await;
    let op = f.start(&account, "review-login").await;
    f.phase(&op, "connected").await;
    assert!(access::require(&f.hub, &owner).is_ok());
    (f, owner, account)
}

#[tokio::test]
async fn saas04_r1_update_recusa_selecao_apos_revogacao_na_fila() {
    let (f, owner, account) = review_ready().await;
    let bot = f.remote(&Caller::Local, "createBot", json!({"name":"Alvo","backend":"codex"})).await;
    let id = bot["bot"]["id"].as_str().unwrap();
    let untouched = f.remote(&Caller::Local, "createBot", json!({"name":"Outro","backend":"codex"})).await;
    let lock = f.hub.store.team_lock.lock().await;
    let mut request = Box::pin(crate::api::dispatch(
        &f.hub,
        &owner,
        "updateBot",
        json!({"id":id,"forkEditor":{"engineAccountId":account["id"]}}),
    ));
    assert!(tokio::time::timeout(Duration::from_millis(30), &mut request).await.is_err());
    f.hub.store.remove_device(owner.device_key()).unwrap();
    assert!(access::require(&f.hub, &owner).is_err());
    drop(lock);
    let result = request.await;
    assert!(result.is_err(), "pedido revogado foi persistido: {result:?}");
    assert!(f.hub.store.bot(id).unwrap().unwrap().config.fork_editor.engine_account_id.is_none());
    assert!(
        f.hub
            .store
            .bot(untouched["bot"]["id"].as_str().unwrap())
            .unwrap()
            .unwrap()
            .config
            .fork_editor
            .engine_account_id
            .is_none()
    );
}

#[tokio::test]
async fn saas04_r1_create_recusa_selecao_apos_lease_expirado_na_fila() {
    let (f, owner, account) = review_ready().await;
    let before = f.hub.store.bots().unwrap().len();
    let lock = f.hub.store.team_lock.lock().await;
    let mut request = Box::pin(crate::api::dispatch(
        &f.hub,
        &owner,
        "createBot",
        json!({"name":"Criado após expiração","backend":"codex","forkEditor":{"engineAccountId":account["id"]}}),
    ));
    assert!(tokio::time::timeout(Duration::from_millis(30), &mut request).await.is_err());
    f.hub.store.set_lease(owner.device_key(), crate::store::now_ms() - 1).unwrap();
    assert!(access::require(&f.hub, &owner).is_err());
    drop(lock);
    let result = request.await;
    assert!(result.is_err(), "pedido vencido criou bot: {result:?}");
    assert_eq!(f.hub.store.bots().unwrap().len(), before);
}

#[tokio::test]
async fn saas04_r1_editor_refuta_mesma_janela_apos_revogacao() {
    let (f, owner, account) = review_ready().await;
    let bot = f.remote(&Caller::Local, "createBot", json!({"name":"Editor","backend":"codex"})).await;
    let id = bot["bot"]["id"].as_str().unwrap();
    let editor = f.remote(&owner, "donnaBotEditor", json!({"botId":id})).await;
    let lock = f.hub.store.team_lock.lock().await;
    let mut request = Box::pin(crate::api::dispatch(
        &f.hub,
        &owner,
        "donnaBotEditorSave",
        json!({"botId":id,"expectedVersion":editor["settingsVersion"],"patch":{"forkEditor":{"engineAccountId":account["id"]}}}),
    ));
    assert!(tokio::time::timeout(Duration::from_millis(30), &mut request).await.is_err());
    f.hub.store.remove_device(owner.device_key()).unwrap();
    drop(lock);
    let error = request.await.unwrap_err().to_string();
    assert!(error.starts_with("remote_setup_unavailable"));
    assert!(f.hub.store.bot(id).unwrap().unwrap().config.fork_editor.engine_account_id.is_none());
}

#[tokio::test]
async fn saas04_r1_usage_recusa_cota_ao_dono_anterior() {
    let (f, owner, account) = review_ready().await;
    // F6-A: o uso só é lido com a conta vinculada ao Space (aqui o Pessoal), senão a consulta nem chega ao CLI.
    let vinculo: crate::store::BotConfig = serde_json::from_value(
        json!({"id":"bot-r1","name":"Fixture","backend":"codex","forkEditor":{"engineAccountId":account["id"]}}),
    )
    .unwrap();
    f.hub.store.save_bot(&vinculo).unwrap();
    let cli = f.cli(Backend::Codex);
    let original = std::fs::read_to_string(&cli).unwrap();
    let needle = "elif method == 'account/rateLimits/read':\n";
    assert!(original.contains(needle));
    let injected = "elif method == 'account/rateLimits/read':\n        (base / 'review-rates-entered').write_text('ready')\n        while not (base / 'review-rates-release').is_file():\n            time.sleep(.01)\n";
    std::fs::write(&cli, original.replace(needle, injected)).unwrap();
    let mut request = Box::pin(crate::api::dispatch(
        &f.hub,
        &owner,
        "donnaEngineAccountUsage",
        json!({"engineAccountId":account["id"]}),
    ));
    let entered = cli.parent().unwrap().join("review-rates-entered");
    let waiter = esperar(|| entered.is_file());
    tokio::select! {
        result = &mut request => panic!("consulta acabou antes da barreira: {result:?}"),
        () = waiter => {}
    }
    crate::remote::cloud::update_status(&f.hub, |s| s.owner.as_mut().unwrap().user_id = "new-owner".into());
    assert!(crate::api::devices::authorize(&f.hub, owner.device_key()).is_ok());
    assert!(access::require(&f.hub, &owner).is_err());
    std::fs::write(cli.parent().unwrap().join("review-rates-release"), "go").unwrap();
    let result = request.await;
    assert!(result.is_err(), "dono anterior recebeu cota: {result:?}");
}

#[tokio::test]
async fn saas04_r1_contas_na_fila_nao_mudam_de_dono() {
    let (f, mut owner, account) = review_ready().await;
    for (method, body) in [
        (
            "donnaEngineAccountCreate",
            json!({"backend":"codex","kind":"cli","label":"Outra","clientRequestId":"queued-create"}),
        ),
        ("donnaEngineAccountRename", json!({"engineAccountId":account["id"],"expectedRevision":2,"label":"Outra"})),
        ("donnaEngineAccountDisconnect", json!({"engineAccountId":account["id"],"expectedRevision":2})),
        ("donnaEngineAccountRemove", json!({"engineAccountId":account["id"],"expectedRevision":2})),
        (
            "donnaEngineSetupStart",
            json!({"engineAccountId":account["id"],"expectedRevision":2,"action":"login","clientRequestId":"queued-login"}),
        ),
    ] {
        let before = f.hub.store.kv_read(accounts::SLOT).unwrap();
        let lock = accounts::MUTATION.lock().await;
        let mut request = Box::pin(crate::api::dispatch(&f.hub, &owner, method, body));
        assert!(tokio::time::timeout(Duration::from_millis(30), &mut request).await.is_err());
        crate::remote::cloud::update_status(&f.hub, |s| s.owner.as_mut().unwrap().user_id = "new-owner".into());
        drop(lock);
        assert!(request.await.is_err(), "{method} mudou conta após troca do dono");
        assert_eq!(f.hub.store.kv_read(accounts::SLOT).unwrap(), before, "{method}");
        crate::remote::cloud::update_status(&f.hub, |s| s.owner.as_mut().unwrap().user_id = "owner".into());
        owner = crate::fork::autoridade::autenticar(&f.hub, owner.device_key()).unwrap().1;
    }
}

#[tokio::test]
async fn saas04_r1_transacao_reverte_expiracao_e_serializa_troca_de_dono() {
    let (f, owner, account) = review_ready().await;
    let cfg: crate::store::BotConfig = serde_json::from_value(
        json!({"id":"atomic-bot","name":"Atômico","backend":"codex","forkEditor":{"engineAccountId":account["id"]}}),
    )
    .unwrap();
    f.hub.store.set_lease(owner.device_key(), crate::store::now_ms() + 120_000).unwrap();
    let owner = crate::fork::autoridade::autenticar(&f.hub, owner.device_key()).unwrap().1;
    let authority = access::resolve(&f.hub, &owner).unwrap();
    let mut escrita_aplicada = false;
    let result = authority.with_db(&f.hub, |db| {
        crate::store::bots::save_bot(db, &cfg)?;
        escrita_aplicada = true;
        assert_eq!(
            db.execute(
                "UPDATE devices SET lease_until=?1 WHERE key=?2",
                rusqlite::params![crate::store::now_ms() - 1, owner.device_key()],
            )?,
            1
        );
        Ok(())
    });
    assert!(escrita_aplicada, "a escrita deve ocorrer antes do vencimento do lease");
    assert!(result.is_err(), "lease venceu dentro da transação");
    assert!(f.hub.store.bot(&cfg.id).unwrap().is_none(), "escrita foi revertida");
    f.hub.store.set_lease(owner.device_key(), crate::store::now_ms() + crate::remote::cloud::LEASE_MS).unwrap();
    let (start_tx, start_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let hub = f.hub.clone();
    let transfer = std::thread::spawn(move || {
        start_rx.recv_timeout(TEST_WAIT).unwrap();
        crate::remote::cloud::update_status(&hub, |s| s.owner.as_mut().unwrap().user_id = "new-owner".into());
        done_tx.send(()).unwrap();
    });
    authority
        .with_db(&f.hub, |db| {
            crate::store::bots::save_bot(db, &cfg)?;
            start_tx.send(()).unwrap();
            assert!(done_rx.recv_timeout(Duration::from_millis(30)).is_err(), "troca de dono atravessou a escrita");
            Ok(())
        })
        .unwrap();
    done_rx.recv_timeout(TEST_WAIT).unwrap();
    transfer.join().unwrap();
    assert!(f.hub.store.bot(&cfg.id).unwrap().is_some(), "escrita precedeu transferência");
    assert!(authority.save_bot(&f.hub, &cfg).is_err(), "dono anterior não escreve após transferência");
}

#[tokio::test]
async fn roteamento_authority_original_recusa_revogacao_lease_e_ativacao_na_fila() {
    for change in ["revoke", "lease", "activation", "sync"] {
        let (f, owner, _) = review_ready().await;
        let bot = f.remote(&Caller::Local, "createBot", json!({"name":"Rota02","backend":"codex"})).await;
        let id = bot["bot"]["id"].as_str().unwrap();
        let body = json!({"botId":id,"hubId":f.hub.host_id,"spaceId":crate::fork::spaces::PERSONAL,
            "expectedRevision":0,"expectedGeneralRevision":0,"policy":{"mode":"auto","guide":"","reserves":[]},"generalGuide":""});
        let authority = access::resolve(&f.hub, &owner).unwrap();
        let lock = f.hub.store.team_lock.lock().await;
        let mut request = Box::pin(crate::api::dispatch(&f.hub, &owner, "donnaRoutingSave", body));
        // O poll chega ao lock com a Authority original capturada antes da revogação.
        assert!(futures::poll!(&mut request).is_pending());
        match change {
            "revoke" => {
                f.hub.store.remove_device(owner.device_key()).unwrap();
            }
            "lease" => {
                f.hub.store.set_lease(owner.device_key(), crate::store::now_ms() - 1).unwrap();
            }
            "activation" => {
                f.hub
                    .store
                    .db
                    .locked()
                    .execute("UPDATE bootstrap_slot SET activation_wire=?1", [b"invalid".as_slice()])
                    .unwrap();
            }
            _ => {
                f.hub.cloud_synced.store(false, std::sync::atomic::Ordering::Release);
            }
        }
        // Mesma Authority não escreve nem autoriza resposta privada após a mudança.
        assert!(
            authority
                .with_db(&f.hub, |db| {
                    db.execute("INSERT INTO kv(k,v) VALUES('rota02-should-not-write','secret')", [])?;
                    Ok(())
                })
                .is_err()
        );
        assert!(authority.require_current(&f.hub).is_err());
        drop(lock);
        assert!(request.await.is_err());
        assert!(f.hub.store.kv_read("rota02-should-not-write").unwrap().is_none());
        assert!(
            f.hub
                .store
                .kv_read(&format!("donna.routing.v1:{}:bot:{id}", crate::fork::spaces::PERSONAL))
                .unwrap()
                .is_none()
        );
    }
}

/// BN-02 reutiliza a cadeia/guarda SAAS-04 real, sem duplicar seu emissor de teste.
#[tokio::test]
async fn bn02_inclusao_recibo_e_fila_revalidam_posse_original_antes_do_commit_e_replay() {
    use crate::fork::native_catalog::ADD_METHOD;
    let f = Fixture::new();
    let owner = f.owner();
    let a = json!({"hubId":f.hub.host_id,"spaceId":crate::fork::spaces::PERSONAL,
        "catalogKey":"pesquisador","profileVersion":2,"clientRequestKey":"A","intent":"add",
        "expectedGeneration":0,"expectedRevision":0,"backend":"codex"});
    let first = crate::api::dispatch(&f.hub, &owner, ADD_METHOD, a.clone()).await.unwrap();
    assert_eq!(first["state"], "active");
    assert_eq!(crate::api::dispatch(&f.hub, &owner, ADD_METHOD, a.clone()).await.unwrap(), first);
    // Expiração entre callback e commit deve reverter inclusive bot/sessão/slot/recibo.
    f.hub
        .store
        .db
        .locked()
        .execute_batch(
            "CREATE TEMP TRIGGER bn02_expira_lease AFTER INSERT ON donna_native_receipts
         WHEN NEW.client_request_key='expira-no-commit'
         BEGIN UPDATE devices SET lease_until=0; END;",
        )
        .unwrap();
    let mut second = a.clone();
    second["catalogKey"] = json!("redator");
    second["clientRequestKey"] = json!("expira-no-commit");
    assert!(crate::api::dispatch(&f.hub, &owner, ADD_METHOD, second.clone()).await.is_err());
    assert!(access::require(&f.hub, &owner).is_ok(), "lease expirado pelo trigger foi revertido");
    for table in ["bots", "donna_native_slots", "donna_native_receipts", "donna_bot_sessions"] {
        let count: i64 =
            f.hub.store.db.locked().query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1, "efeito não confirmado em {table}");
    }
    f.hub.store.db.locked().execute_batch("DROP TRIGGER bn02_expira_lease").unwrap();
    // poll chega deterministicamente ao lock: a Authority já foi capturada.
    let guard = f.hub.store.team_lock.clone().lock_owned().await;
    second["clientRequestKey"] = json!("em-fila");
    let mut queued = Box::pin(crate::api::dispatch(&f.hub, &owner, ADD_METHOD, second));
    assert!(futures::poll!(&mut queued).is_pending());
    f.hub.store.remove_device(owner.device_key()).unwrap();
    drop(guard);
    assert!(queued.await.is_err());
    assert!(crate::api::dispatch(&f.hub, &owner, ADD_METHOD, a).await.is_err(), "recibo não concede acesso revogado");
    assert_eq!(f.hub.store.bots().unwrap().len(), 1);
    f.hub.shutdown().await;
    let id = first["botId"].as_str().unwrap();
    let _ = std::fs::remove_dir(crate::agent::workspace::path(id));
}

#[test]
fn c1_compoe_saas04_da_mesma_captura_e_recusa_aba_sem_promocao() {
    let f = Fixture::new();
    let owner = f.owner();
    let original = crate::fork::autoridade::Autoridade::capturar(&f.hub, &owner).unwrap();
    let contas = access::resolve(&f.hub, &owner).unwrap();
    original.with_db(&f.hub, Some(&contas), |_| Ok(())).unwrap();
    // Outra requisição do mesmo canal não empresta sua Authority para esta.
    let (_, outra) = crate::fork::autoridade::reconferir(&f.hub, &owner).unwrap();
    let outras_contas = access::resolve(&f.hub, &outra).unwrap();
    assert!(original.with_db(&f.hub, Some(&outras_contas), |_| Ok(())).is_err());
    assert!(original.with_db(&f.hub, Some(&access::Authority::Local), |_| Ok(())).is_err());
    let aparelho = f.hub.store.device(owner.device_key()).unwrap();
    f.hub.revoke_device(&aparelho.key).unwrap();
    f.hub.store.put_device(&aparelho).unwrap();
    let mut executou = false;
    assert!(
        contas
            .with_db(&f.hub, |_| {
                executou = true;
                Ok(())
            })
            .is_err()
    );
    assert!(!executou);
    assert!(access::resolve(&f.hub, &owner).is_err());
    assert!(crate::api::devices::permit(&owner, "setCloud").is_err());
}

#[test]
fn c1_perda_de_sincronizacao_nao_renova_authority_saas04_antiga() {
    let f = Fixture::new();
    let owner = f.owner();
    let original = crate::fork::autoridade::Autoridade::capturar(&f.hub, &owner).unwrap();
    let contas = access::resolve(&f.hub, &owner).unwrap();
    original.with_db(&f.hub, Some(&contas), |_| Ok(())).unwrap();
    f.hub.cloud_synced.store(false, std::sync::atomic::Ordering::Release);
    f.hub.cloud_synced.store(true, std::sync::atomic::Ordering::Release);
    let mut executou = false;
    assert!(
        contas
            .with_db(&f.hub, |_| {
                executou = true;
                Ok(())
            })
            .is_err()
    );
    assert!(!executou);
    let (tx, mut rx) = mpsc::channel(1);
    let resposta = json!({"perfil":"antigo"});
    let vaga = tx.try_reserve().unwrap();
    assert!(original.com_saida(&f.hub, Some(&contas), || vaga.send(resposta)).is_err());
    assert!(rx.try_recv().is_err());
    let (_, nova_solicitacao) = crate::fork::autoridade::reconferir(&f.hub, &owner).unwrap();
    let nova = crate::fork::autoridade::Autoridade::capturar(&f.hub, &nova_solicitacao).unwrap();
    let novas_contas = access::resolve(&f.hub, &nova_solicitacao).unwrap();
    nova.with_db(&f.hub, Some(&novas_contas), |_| Ok(())).unwrap();
    assert!(nova.with_db(&f.hub, Some(&contas), |_| Ok(())).is_err());
    assert!(contas.with_db(&f.hub, |_| Ok(())).is_err());
}

/// `sessionEngineSet` real na principal de `dev`, com a revisão atual dela e a conta `account`.
async fn choose_principal(hub: &Arc<Hub>, account: &str) -> Result<Value> {
    let listed = crate::api::dispatch(hub, &Caller::Local, "sessions", json!({"botId":"dev"})).await?;
    let engine = json!({"accountId":account,"backend":"codex","model":null,"modelOptions":{},"fastMode":false});
    let revision = listed["sessions"][0]["revision"].clone();
    let body = json!({"botId":"dev","sessionId":null,"expectedRevision":revision,"engine":engine});
    crate::api::dispatch(hub, &Caller::Local, "sessionEngineSet", body).await
}

/// D372 (R1): a conta usada só pela seleção de uma conversa do Pessoal (efetiva ou próxima) conta como em uso:
/// remover recusa `account_in_use`, setup com o bot ocupado recusa `account_busy`, seleção ilegível falha (nunca vira
/// ausência), e a remoção reconfere o uso na mesma transação da escrita (referência criada depois do preflight).
#[tokio::test]
async fn conta_usada_por_conversa_bloqueia_remocao_e_setup() {
    let f = Fixture::new();
    let account = f.create("codex", "api", "d372-uso").await;
    let id = account["id"].as_str().unwrap().to_owned();
    set_api_env(
        &f.hub,
        &Caller::Local,
        json!({"engineAccountId":id,"expectedRevision":1,"backend":"codex","vars":{"OPENAI_API_KEY":"API_FIXTURE"}}),
    )
    .await
    .unwrap();
    let bot: crate::store::BotConfig =
        serde_json::from_value(json!({"id":"dev","name":"Dev","backend":"codex","cwd":f.root})).unwrap();
    f.hub.store.save_bot(&bot).unwrap();
    assert!(managed::bot_ids(&f.hub.store, &id).unwrap().is_empty(), "o padrão do bot não usa a conta");
    let next = json!({"schemaVersion":2,"source":"own","accountId":id,"backend":"codex","model":null,
        "modelOptions":{},"fastMode":false})
    .to_string();
    let set_next = |raw: Option<&str>| {
        f.hub.store.db.locked().execute("UPDATE donna_bot_sessions SET engine_next=?1 WHERE id='dev'", [raw]).unwrap();
    };
    // Só a PRÓXIMA da principal usa a conta.
    set_next(Some(&next));
    assert_eq!(managed::bot_ids(&f.hub.store, &id).unwrap(), ["dev"]);
    let revision = f.account(&account["id"])["revision"].clone();
    let remove = json!({"engineAccountId":id,"expectedRevision":revision});
    let error = call(&f.hub, &Caller::Local, "donnaEngineAccountRemove", remove).await.unwrap_err();
    assert!(error.to_string().contains("account_in_use"), "{error:#}");
    f.hub.set_runtime("dev", |r| r.status = crate::hub::BotStatus::Working);
    assert!(managed::idle(&f.hub, &id).unwrap_err().to_string().contains("account_busy"));
    f.hub.set_runtime("dev", |r| r.status = crate::hub::BotStatus::Idle);
    // Ilegível falha fechado.
    set_next(Some("{}"));
    assert!(managed::bot_ids(&f.hub.store, &id).is_err());
    set_next(None);
    // Ordem 1, commits reais: preflight sem uso → `sessionEngineSet` comita a conta → a escrita da remoção relê na
    // mesma transação e recusa; nada é removido.
    assert_eq!(managed::bot_ids(&f.hub.store, &id).unwrap(), Vec::<String>::new());
    let mut registry = Registry::load(&f.hub.store).unwrap();
    registry.accounts.retain(|a| a.id != id);
    choose_principal(&f.hub, &id).await.unwrap();
    let error = access::Authority::Local.persist_removing(&f.hub, &registry, None, Some(&id)).unwrap_err();
    assert!(error.to_string().contains("account_in_use"));
    assert!(Registry::load(&f.hub.store).unwrap().account(&id).is_ok(), "nada removido");
    // Ordem 2: a remoção comita primeiro (sem referência) → `sessionEngineSet` vê o cadastro sem a conta e recusa,
    // sem gravar referência.
    set_next(None);
    access::Authority::Local.persist_removing(&f.hub, &registry, None, Some(&id)).unwrap();
    assert!(choose_principal(&f.hub, &id).await.unwrap_err().to_string().contains("engine_unavailable"));
    assert!(managed::bot_ids(&f.hub.store, &id).unwrap().is_empty(), "nenhuma referência à conta retirada");
}

/// D372 (R3): a guarda de setup do preparo confere a conta da execução CAPTURADA da lane. Setup da conta do padrão
/// (A) não bloqueia um turno capturado na conta da conversa (B); setup da própria conta capturada bloqueia.
#[tokio::test]
async fn preparo_confere_setup_da_conta_capturada_e_nao_do_padrao() {
    let f = Fixture::new();
    let cli = f.cli(Backend::Codex);
    std::fs::write(cli.parent().unwrap().join("scenario"), "hang").unwrap();
    let default = f.create("codex", "cli", "d372-padrao").await;
    let captured = f.create("codex", "api", "d372-capturada").await;
    let (a, b) = (default["id"].as_str().unwrap().to_owned(), captured["id"].as_str().unwrap().to_owned());
    let bot: crate::store::BotConfig = serde_json::from_value(
        json!({"id":"dev","name":"Dev","backend":"codex","cwd":f.root,"forkEditor":{"engineAccountId":a}}),
    )
    .unwrap();
    f.hub.store.save_bot(&bot).unwrap();
    let op = f.start(&default, "d372-login").await;
    f.phase(&op, "awaiting_authorization").await;
    assert!(in_setup(&f.hub.store, &a));
    f.hub.prepare_captured_turn("dev", None, Some(b.as_str()), |_| {}).unwrap();
    let error = f.hub.prepare_captured_turn("dev", None, Some(a.as_str()), |_| {}).unwrap_err();
    assert!(error.to_string().contains("setup_in_progress"));
    // Chamador sem captura continua conferindo o padrão salvo.
    assert!(f.hub.prepare_bot_turn("dev", None, |_| {}).is_err());
    let _ = f.action(&op, json!({"action":"cancel"}));
}

/// Gancho de barreira para a chave (`clientRequestId` ou id da conta) e o momento indicados.
fn gancho(chave: &str, momento: Barreira, hook: impl FnOnce() + Send + 'static) {
    BARREIRA.locked().push((chave.into(), momento, Box::new(hook)));
}

/// Outro thread (com o contexto Tokio) que avisa ao começar e devolve o resultado. O gancho dentro da cerca só espera o
/// aviso de partida; nunca espera o resultado, que depende da cerca ser solta.
fn em_outro_thread<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> (std::sync::mpsc::Receiver<()>, std::thread::JoinHandle<T>) {
    let (started, rx) = std::sync::mpsc::channel();
    let runtime = tokio::runtime::Handle::current();
    let handle = std::thread::spawn(move || {
        let _context = runtime.enter();
        started.send(()).unwrap();
        work()
    });
    (rx, handle)
}

/// Bot `dev` sem conta no padrão; a seleção EFETIVA da principal usa `account`.
fn principal_na_conta(f: &Fixture, account: &str) {
    let bot: crate::store::BotConfig =
        serde_json::from_value(json!({"id":"dev","name":"Dev","backend":"codex","cwd":f.root})).unwrap();
    f.hub.store.save_bot(&bot).unwrap();
    let effective = json!({"schemaVersion":2,"source":"own","accountId":account,"backend":"codex","model":null,
        "modelOptions":{},"fastMode":false})
    .to_string();
    let sql = "UPDATE donna_bot_sessions SET engine_choice=?1 WHERE id='dev'";
    f.hub.store.db.locked().execute(sql, [effective]).unwrap();
}

/// Cadastro completo (inclui `generation` e `setup_request`, que o DTO omite).
fn cadastro(f: &Fixture) -> Value {
    serde_json::to_value(Registry::load(&f.hub.store).unwrap()).unwrap()
}

/// Diretórios de perfil da conta.
fn perfis(f: &Fixture, account: &str) -> Vec<std::ffi::OsString> {
    let mut names: Vec<_> = std::fs::read_dir(f.root.join("accounts").join(account))
        .map(|dir| dir.map(|e| e.unwrap().file_name()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

fn fase_do_pedido(request: &str) -> Option<Phase> {
    OPERATIONS.locked().as_ref().and_then(|ops| ops.values().find(|op| op.request == request).map(|op| op.phase))
}

fn ocioso(f: &Fixture) {
    f.hub.set_runtime("dev", |r| {
        r.status = crate::hub::BotStatus::Idle;
        r.preparing_turn_token = None;
        r.memory_turn_token = None;
    });
}

/// Conta API gerenciada e conectada, usada só pela seleção efetiva da principal.
async fn conta_conectada(f: &Fixture, request: &str) -> String {
    let account = f.create("codex", "api", request).await;
    let id = account["id"].as_str().unwrap().to_owned();
    let vars = json!({"OPENAI_API_KEY":"API_FIXTURE"});
    let body = json!({"engineAccountId":id,"expectedRevision":1,"backend":"codex","vars":vars});
    set_api_env(&f.hub, &Caller::Local, body).await.unwrap();
    principal_na_conta(f, &id);
    id
}

fn desconectar(id: &str) -> Value {
    json!({"engineAccountId":id,"expectedRevision":2})
}

/// D372 (R1): a desconexão que passa o preflight e perde para um turno admitido na conta (Waiting do preparo, e
/// Working depois da ativação) recusa `account_busy` na cerca final e não altera nenhum campo do cadastro.
#[tokio::test]
async fn desconexao_perde_para_turno_admitido_e_preserva_o_cadastro() {
    let f = Fixture::new();
    let a = conta_conectada(&f, "d372-desc-perde").await;
    let before = cadastro(&f);
    for ativar in [false, true] {
        let (hub, conta) = (f.hub.clone(), a.clone());
        gancho(&a, Barreira::Antes, move || {
            hub.prepare_captured_turn("dev", None, Some(conta.as_str()), |_| {}).unwrap();
            if ativar {
                let token = hub.runtime("dev").preparing_turn_token.unwrap();
                hub.activate_bot_turn("dev", &token).unwrap();
            }
        });
        let error = call(&f.hub, &Caller::Local, "donnaEngineAccountDisconnect", desconectar(&a)).await.unwrap_err();
        assert!(error.to_string().contains("account_busy"), "{error:#}");
        let expected = if ativar { crate::hub::BotStatus::Working } else { crate::hub::BotStatus::Waiting };
        assert_eq!(f.hub.runtime("dev").status, expected, "o turno admitido é conservado");
        assert_eq!(cadastro(&f), before, "cadastro intacto: estado, habilitação, modelos, catálogo e revisão");
        ocioso(&f);
    }
}

/// D372 (R1): a desconexão que já passou a ociosidade final dentro da cerca vence. Um preparo na conta disparado
/// nesse ponto espera o runtime e só roda depois do commit (testemunho: o próprio preparo lê o cadastro
/// desconectado); a cadeia restante (ativação/permit e lançamento) recusa a conta, então não há prompt.
#[tokio::test]
async fn desconexao_ganha_e_o_preparo_seguinte_nao_chega_ao_prompt() {
    let f = Fixture::new();
    let a = conta_conectada(&f, "d372-desc-ganha").await;
    let main = crate::store::Lane::main("dev");
    let mut cfg = f.hub.store.bot("dev").unwrap().unwrap().config;
    crate::fork::sessoes::admissao::apply_choice(&f.hub.store.db.locked(), &main, &mut cfg).unwrap();
    assert_eq!(cfg.fork_editor.engine_account_id.as_deref(), Some(a.as_str()));
    managed::validate_selection_in(&Registry::load(&f.hub.store).unwrap(), &cfg).unwrap();
    let (hub, conta) = (f.hub.clone(), a.clone());
    let preparo = Arc::new(Mutex::new(None));
    let saida = preparo.clone();
    gancho(&a, Barreira::Cerca, move || {
        assert!(hub.runtime_held(), "a cerca retém o runtime");
        let observador = hub.clone();
        let (started, handle) = em_outro_thread(move || {
            let mut visto = None;
            let result = observador.prepare_captured_turn("dev", None, Some(conta.as_str()), |_| {
                visto = Some(Registry::load(&observador.store).unwrap().account(&conta).unwrap().state);
            });
            (result.map(|_| ()), visto)
        });
        started.recv().unwrap();
        *saida.locked() = Some(handle);
    });
    let dto = call(&f.hub, &Caller::Local, "donnaEngineAccountDisconnect", desconectar(&a)).await.unwrap();
    assert_eq!(dto["revision"], 3);
    let handle = preparo.locked().take().expect("gancho rodou dentro da cerca");
    let (result, visto) = handle.join().unwrap();
    result.unwrap();
    assert_eq!(visto, Some(State::Disconnected), "o preparo só atravessou o runtime depois do commit");
    let registry = Registry::load(&f.hub.store).unwrap();
    let error = managed::validate_selection_in(&registry, &cfg).unwrap_err();
    assert!(error.to_string().contains("account_unavailable"), "ativação/permit: {error:#}");
    assert!(managed::resolve_for_bot(&f.hub.store, &cfg).is_err(), "lançamento recusa");
    ocioso(&f);
}

/// D372 (R1/R1-S): o preparo entre o preflight e a cerca final do início faz o setup recusar `account_busy` ANTES de
/// aceitar a tentativa: sem operação, cadastro completo e perfis idênticos. O mesmo pedido (mesma revisão) é aceito
/// depois do turno, também sem nenhuma operação em memória (como após reiniciar), e avança a generation uma vez.
#[tokio::test]
async fn setup_perde_para_preparo_sem_consumir_o_pedido() {
    let f = Fixture::new();
    let cli = f.cli(Backend::Codex);
    std::fs::write(cli.parent().unwrap().join("scenario"), "hang").unwrap();
    let account = f.create("codex", "cli", "d372-setup-perde").await;
    let a = account["id"].as_str().unwrap().to_owned();
    principal_na_conta(&f, &a);
    let (before, dirs) = (cadastro(&f), perfis(&f, &a));
    let generation = Registry::load(&f.hub.store).unwrap().account(&a).unwrap().managed.as_ref().unwrap().generation;
    let (hub, conta) = (f.hub.clone(), a.clone());
    gancho("d372-perde", Barreira::Antes, move || {
        hub.prepare_captured_turn("dev", None, Some(conta.as_str()), |_| {}).unwrap();
    });
    let body = json!({"engineAccountId":a,"expectedRevision":account["revision"],"action":"login",
        "clientRequestId":"d372-perde"});
    let error = call(&f.hub, &Caller::Local, "donnaEngineSetupStart", body.clone()).await.unwrap_err();
    assert!(error.to_string().contains("account_busy"), "{error:#}");
    assert!(fase_do_pedido("d372-perde").is_none(), "nenhuma operação");
    assert_eq!(cadastro(&f), before, "pedido não consumido, generation intacta");
    assert_eq!(perfis(&f, &a), dirs, "perfil novo descartado");
    ocioso(&f);
    if let Some(ops) = OPERATIONS.locked().as_mut() {
        ops.retain(|_, op| !op.hub.ptr_eq(&Arc::downgrade(&f.hub)));
    }
    let op = call(&f.hub, &Caller::Local, "donnaEngineSetupStart", body).await.unwrap();
    assert_eq!(op["phase"], "preparing");
    let meta = Registry::load(&f.hub.store).unwrap().account(&a).unwrap().managed.clone().unwrap();
    assert_eq!(meta.generation, generation + 1);
    assert_eq!(op["generation"], generation + 1);
    let _ = f.action(&op, json!({"action":"cancel"}));
}

/// D372 (R1): o início que passa a cerca vence. Preparo e leitura de operação disparados com runtime e OPERATIONS
/// retidos, antes da primeira escrita, só atravessam depois de metadados e operação publicados juntos: o preparo
/// recusa `setup_in_progress` e a leitura vê a operação ativa, coerente com a generation gravada. O preparo depois
/// da cerca (`Depois`) também recusa.
#[tokio::test]
async fn setup_ganha_publica_metadados_e_operacao_juntos() {
    let f = Fixture::new();
    let cli = f.cli(Backend::Codex);
    std::fs::write(cli.parent().unwrap().join("scenario"), "hang").unwrap();
    let account = f.create("codex", "cli", "d372-setup-ganha").await;
    let a = account["id"].as_str().unwrap().to_owned();
    principal_na_conta(&f, &a);
    let threads = Arc::new(Mutex::new(None));
    let (hub, conta, saida) = (f.hub.clone(), a.clone(), threads.clone());
    gancho("d372-ganha", Barreira::Cerca, move || {
        assert!(hub.runtime_held());
        let (observador, leitor, c1, c2) = (hub.clone(), hub.clone(), conta.clone(), conta);
        let (p, preparo) = em_outro_thread(move || {
            observador.prepare_captured_turn("dev", None, Some(c1.as_str()), |_| {}).map(|_| ())
        });
        let (l, leitura) = em_outro_thread(move || in_setup(&leitor.store, &c2));
        p.recv().unwrap();
        l.recv().unwrap();
        *saida.locked() = Some((preparo, leitura));
    });
    let (hub, conta) = (f.hub.clone(), a.clone());
    gancho("d372-ganha", Barreira::Depois, move || {
        let error = hub.prepare_captured_turn("dev", None, Some(conta.as_str()), |_| {}).unwrap_err();
        assert!(error.to_string().contains("setup_in_progress"));
    });
    let body = json!({"engineAccountId":a,"expectedRevision":account["revision"],"action":"login",
        "clientRequestId":"d372-ganha"});
    let op = call(&f.hub, &Caller::Local, "donnaEngineSetupStart", body).await.unwrap();
    assert!(BARREIRA.locked().iter().all(|(chave, ..)| chave != "d372-ganha"), "os dois ganchos rodaram");
    let (preparo, leitura) = threads.locked().take().expect("gancho rodou dentro da cerca");
    assert!(preparo.join().unwrap().unwrap_err().to_string().contains("setup_in_progress"));
    assert!(leitura.join().unwrap(), "a leitura só vê a operação já publicada, ativa");
    let meta = Registry::load(&f.hub.store).unwrap().account(&a).unwrap().managed.clone().unwrap();
    assert_eq!(op["generation"], meta.generation);
    assert_eq!(meta.setup_request.as_ref().map(|(id, _)| id.as_str()), Some("d372-ganha"));
    let _ = f.action(&op, json!({"action":"cancel"}));
}

/// D372 (R1-S): falha da primeira escrita (Authority original revogada entre preflight e cerca) não publica operação,
/// não grava nada, descarta o perfil novo e não consome o pedido.
#[tokio::test]
async fn falha_da_primeira_escrita_nao_consome_o_pedido() {
    let f = Fixture::new();
    let cli = f.cli(Backend::Codex);
    std::fs::write(cli.parent().unwrap().join("scenario"), "hang").unwrap();
    let owner = f.owner();
    let body = json!({"backend":"codex","kind":"cli","label":"Dono","clientRequestId":"d372-falha-create"});
    let account = f.remote(&owner, "donnaEngineAccountCreate", body).await;
    let a = account["id"].as_str().unwrap().to_owned();
    let (before, dirs) = (cadastro(&f), perfis(&f, &a));
    let (hub, device) = (f.hub.clone(), owner.device_key().to_owned());
    gancho("d372-falha", Barreira::Antes, move || {
        hub.store.remove_device(&device).unwrap();
    });
    let body = json!({"engineAccountId":a,"expectedRevision":1,"action":"login","clientRequestId":"d372-falha"});
    assert!(crate::api::dispatch(&f.hub, &owner, "donnaEngineSetupStart", body.clone()).await.is_err());
    assert!(fase_do_pedido("d372-falha").is_none());
    assert_eq!(cadastro(&f), before);
    assert_eq!(perfis(&f, &a), dirs);
    let op = call(&f.hub, &Caller::Local, "donnaEngineSetupStart", body).await.unwrap();
    assert_eq!(op["phase"], "preparing", "o mesmo pedido continua disponível");
    let _ = f.action(&op, json!({"action":"cancel"}));
}

/// D372 (R1): conclusão do setup. (1) Cancelamento e preparo na conta entre a prova e a cerca: a conclusão recusa e
/// nada é promovido (revisão, perfil, estado); o perfil novo é descartado. (2) Preparo disparado com a cerca da
/// conclusão retida só atravessa depois do commit e vê a conta conectada na revisão nova.
#[tokio::test]
async fn conclusao_do_setup_e_preparo_na_mesma_conta() {
    let f = Fixture::new();
    f.cli(Backend::Codex);
    let account = f.create("codex", "cli", "d372-conclusao").await;
    let a = account["id"].as_str().unwrap().to_owned();
    principal_na_conta(&f, &a);
    let dirs = perfis(&f, &a);
    let before = Registry::load(&f.hub.store).unwrap().account(&a).unwrap().clone();
    let (hub, conta) = (f.hub.clone(), a.clone());
    gancho("d372-conclusao-1", Barreira::Conclusao, move || {
        for op in OPERATIONS.locked().as_mut().unwrap().values_mut().filter(|op| op.request == "d372-conclusao-1") {
            clear(op, Phase::Cancelled, None);
        }
        hub.prepare_captured_turn("dev", None, Some(conta.as_str()), |_| {}).unwrap();
    });
    let op = f.start(&account, "d372-conclusao-1").await;
    esperar(|| fase_do_pedido("d372-conclusao-1") == Some(Phase::Cancelled) && perfis(&f, &a) == dirs).await;
    let saved = Registry::load(&f.hub.store).unwrap().account(&a).unwrap().clone();
    assert_eq!(saved.managed.as_ref().unwrap().revision, 1, "nada promovido");
    assert_eq!((saved.state, &saved.launch), (before.state, &before.launch));
    assert_eq!(saved.managed.as_ref().unwrap().profile, before.managed.as_ref().unwrap().profile);
    assert_eq!(f.action(&op, json!({"action":"read"})).unwrap()["phase"], "cancelled");
    ocioso(&f);
    let (hub, conta) = (f.hub.clone(), a.clone());
    let preparo = Arc::new(Mutex::new(None));
    let saida = preparo.clone();
    gancho("d372-conclusao-2", Barreira::ConclusaoCerca, move || {
        assert!(hub.runtime_held());
        let observador = hub.clone();
        let (started, handle) = em_outro_thread(move || {
            let mut visto = None;
            let result = observador.prepare_captured_turn("dev", None, Some(conta.as_str()), |_| {
                let saved = Registry::load(&observador.store).unwrap().account(&conta).unwrap().clone();
                visto = Some((saved.state, saved.managed.unwrap().revision));
            });
            (result.map(|_| ()), visto)
        });
        started.recv().unwrap();
        *saida.locked() = Some(handle);
    });
    let op = f.start(&account, "d372-conclusao-2").await;
    f.phase(&op, "connected").await;
    let handle = preparo.locked().take().expect("gancho rodou dentro da cerca da conclusão");
    let (result, visto) = handle.join().unwrap();
    result.unwrap();
    assert_eq!(visto, Some((State::Connected, 2)), "o preparo só atravessou depois do commit da conclusão");
    ocioso(&f);
}

/// MX-6: ocultar/mostrar modelo é intenção do dono (e do computador local), idempotente,
/// com CAS de revisão. Não incrementa a revisão — o carimbo das sessões a inclui, e uma
/// sessão em uso não pode cair por uma preferência de listagem. A consulta devolve o campo;
/// remover a conta apaga os ocultos dela.
#[tokio::test]
async fn mx6_modelos_ocultos_pela_intencao_com_revisao_idempotencia_e_remocao() {
    let f = Fixture::new();
    let owner = f.owner();
    let account = f
        .remote(
            &owner,
            "donnaEngineAccountCreate",
            json!({"backend":"codex","kind":"cli","label":"Dono","clientRequestId":"mx6-create"}),
        )
        .await;
    let id = account["id"].as_str().unwrap();
    let body = json!({"engineAccountId":id,"expectedRevision":1,"modelId":"gpt-6.1-sol","hidden":true});
    let hidden = f.remote(&owner, "donnaEngineAccountModelVisibility", body.clone()).await;
    assert_eq!(hidden["hiddenModelIds"], json!(["gpt-6.1-sol"]));
    assert_eq!(hidden["revision"], 1, "ocultar não consome revisão");
    // Idempotente: repetir devolve o mesmo estado, inclusive pelo computador local.
    assert_eq!(
        call(&f.hub, &Caller::Local, "donnaEngineAccountModelVisibility", body.clone()).await.unwrap()["hiddenModelIds"],
        json!(["gpt-6.1-sol"])
    );
    // CAS: revisão divergente recusa sem efeito.
    let mut stale = body.clone();
    stale["expectedRevision"] = 99.into();
    let error = crate::api::dispatch(&f.hub, &owner, "donnaEngineAccountModelVisibility", stale).await.unwrap_err();
    assert!(format!("{error:#}").contains("account_conflict"), "{error:#}");
    // Id fora do catálogo é aceito e guardado sem erro.
    let extra = json!({"engineAccountId":id,"expectedRevision":1,"modelId":"fora-do-catalogo","hidden":true});
    assert_eq!(
        f.remote(&owner, "donnaEngineAccountModelVisibility", extra).await["hiddenModelIds"],
        json!(["gpt-6.1-sol", "fora-do-catalogo"])
    );
    // Id inválido recusa antes de gravar.
    for bad in [json!(""), json!("a\nb"), json!("   ")] {
        let mut body = body.clone();
        body["modelId"] = bad;
        assert!(crate::api::dispatch(&f.hub, &owner, "donnaEngineAccountModelVisibility", body).await.is_err());
    }
    // O DTO da conta devolvido pela intenção já traz o campo para o app filtrar.
    assert_eq!(f.account(&id.into())["hiddenModelIds"], json!(["gpt-6.1-sol", "fora-do-catalogo"]));
    // Mostrar remove só o id pedido.
    let shown = f
        .remote(
            &owner,
            "donnaEngineAccountModelVisibility",
            json!({"engineAccountId":id,"expectedRevision":1,"modelId":"gpt-6.1-sol","hidden":false}),
        )
        .await;
    assert_eq!(shown["hiddenModelIds"], json!(["fora-do-catalogo"]));
    // Remover a conta apaga os ocultos dela junto com o registro.
    f.remote(&owner, "donnaEngineAccountRemove", json!({"engineAccountId":id,"expectedRevision":shown["revision"]}))
        .await;
    let registry = Registry::load(&f.hub.store).unwrap();
    assert!(registry.accounts.iter().all(|a| a.id != id));
    assert!(registry.accounts.iter().all(|a| a.hidden_model_ids.is_empty()));
}

/// Perfil antigo (sem conta gerenciada): "Excluir" só vale desconectado e só tira o registro da lista; o diretório do
/// perfil do computador (login) não é tocado, e conectado/revisão errada/cofre seguem preservados.
#[tokio::test]
async fn remover_perfil_antigo_desconectado_tira_so_o_registro() {
    use crate::fork::engine_accounts::{Account, AuthorizedScope};
    let f = Fixture::new();
    let profile = f.root.join("perfil-antigo");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("login.json"), "FIXTURE").unwrap();
    let id = format!("ac_{}", "a".repeat(64));
    let legacy = |state: State, launch: LaunchReference| Account {
        managed: None,
        id: id.clone(),
        backend: Backend::Claude,
        billing: Billing::Subscription,
        state,
        launch,
        scope: AuthorizedScope::Owner,
        quota_identity: None,
        verified_at: None,
        subscription_proof: None,
        models: Vec::new(),
        hidden_model_ids: Vec::new(),
    };
    let cli = || LaunchReference::CliProfile { executable: "/fixture/claude".into(), directory: profile.clone() };
    let seed = |account: Account| {
        let mut registry = Registry::load(&f.hub.store).unwrap();
        registry.accounts.clear();
        registry.accounts.push(account);
        registry.persist(&f.hub.store).unwrap();
    };
    let remove = |revision: u64| {
        let hub = f.hub.clone();
        let body = json!({"engineAccountId":id,"expectedRevision":revision});
        async move { call(&hub, &Caller::Local, "donnaEngineAccountRemove", body).await }
    };
    let present = || Registry::load(&f.hub.store).unwrap().account(&id).is_ok();

    seed(legacy(State::Connected, cli()));
    assert!(remove(1).await.unwrap_err().to_string().contains("legacy_preserved"));
    seed(legacy(State::Disconnected, LaunchReference::VaultProfile { slot: "slot-fixture".into() }));
    assert!(remove(1).await.unwrap_err().to_string().contains("legacy_preserved"));
    seed(legacy(State::Disconnected, cli()));
    assert!(remove(2).await.unwrap_err().to_string().contains("account_conflict"));
    assert!(present(), "recusas não removem");
    // Desconectar continua recusando perfil antigo.
    let disconnect = json!({"engineAccountId":id,"expectedRevision":1});
    assert!(
        call(&f.hub, &Caller::Local, "donnaEngineAccountDisconnect", disconnect)
            .await
            .unwrap_err()
            .to_string()
            .contains("legacy_preserved")
    );

    assert_eq!(remove(1).await.unwrap(), json!({}));
    assert!(!present());
    assert_eq!(std::fs::read_to_string(profile.join("login.json")).unwrap(), "FIXTURE", "o perfil não é apagado");
}

#[tokio::test]
async fn chave_go_remota_exige_acs4_e_nao_abre_cofre_generico() {
    let f = Fixture::new();
    let owner = f.owner();
    let ordinary =
        Caller::Device { key: "pareado-sem-acs4".into(), scopes: vec![crate::store::Scope::Control], autoridade: None };
    let go = json!({"backend":"opencode","vars":{"OPENCODE_API_KEY":"fixture-go-key"}});
    assert!(crate::api::dispatch(&f.hub, &ordinary, "engineGoKeySave", go.clone()).await.is_err());
    for body in [
        json!({"backend":"opencode","vars":{"OPENCODE_API_KEY":"fixture","OUTRA":"fixture"}}),
        json!({"backend":"opencode","vars":{"OUTRA":"fixture"}}),
        json!({"backend":"cursor","vars":{"OPENCODE_API_KEY":"fixture"}}),
        json!({"backend":"opencode","engineAccountId":"fixture","vars":{"OPENCODE_API_KEY":"fixture"}}),
        json!({"backend":"opencode","spaceId":"personal","vars":{"OPENCODE_API_KEY":"fixture"}}),
    ] {
        assert!(crate::api::dispatch(&f.hub, &owner, "engineGoKeySave", body).await.is_err());
    }
    assert!(f.hub.store.kv_read("agent-env:opencode").unwrap().is_none());
    assert!(
        crate::api::dispatch(&f.hub, &owner, "setAgentEnv", go.clone())
            .await
            .unwrap_err()
            .to_string()
            .contains("remote_setup_unavailable")
    );
    let result = f.remote(&owner, "engineGoKeySave", go).await;
    assert_eq!(result["signedIn"], Value::Null);
    assert!(!result.to_string().contains("fixture-go-key"));
    let raw = f.hub.store.kv_read("agent-env:opencode").unwrap().unwrap();
    assert!(raw.starts_with("vault:v1:") && !raw.contains("fixture-go-key"));
    assert_eq!(
        crate::agent::auth::env(&f.hub.store, "opencode").unwrap(),
        [("OPENCODE_API_KEY".into(), "fixture-go-key".into())]
    );
    assert_eq!(crate::agent::auth::env(&f.hub.store, "cursor").unwrap(), [] as [(String, String); 0]);
    f.remote(&owner, "engineGoKeySave", json!({"backend":"opencode","vars":{"OPENCODE_API_KEY":""}})).await;
    assert_eq!(crate::agent::auth::env(&f.hub.store, "opencode").unwrap(), [] as [(String, String); 0]);
    f.hub.cloud_synced.store(false, std::sync::atomic::Ordering::Release);
    assert!(
        crate::api::dispatch(
            &f.hub,
            &owner,
            "engineGoKeySave",
            json!({"backend":"opencode","vars":{"OPENCODE_API_KEY":"fixture"}})
        )
        .await
        .is_err()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn terminal_login_acs4_sem_replay_sem_escape_ou_eco_para_outro_cliente() {
    use base64::Engine as _;
    use futures::StreamExt;
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let owner = f.owner();
    let path = f.root.join("cursor-fixture");
    std::fs::write(
        &path,
        r"#!/usr/bin/python3
import os,sys,pathlib
marker=pathlib.Path(os.environ['HOME'])/'signed'
if sys.argv[1:]==['status']:
 print('Logged in as fixture' if marker.exists() else 'Not logged in');sys.exit(0 if marker.exists() else 1)
assert sys.argv[1:]==['login']
print('https://cursor.com/login?challenge=fixture',flush=True)
import time
time.sleep(1)
marker.touch();print('Login concluido',flush=True)
sys.stdin.readline()
",
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let id = f.hub.terms.fixture_login(&f.hub, &owner, "cursor", path).unwrap();
    assert!(crate::api::term_events(&f.hub, &id, &Caller::Local).is_err());
    f.remote(&owner, "termResize", json!({"term":id,"cols":90,"rows":30})).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(f.hub.terms.get(&id).is_some(), "resize antes do attach conserva o processo");
    let mut stream = Box::pin(crate::api::term_events(&f.hub, &id, &owner).unwrap().unwrap());
    let event = tokio::time::timeout(Duration::from_secs(3), stream.next()).await.unwrap().unwrap();
    assert_eq!(event["type"], "output");
    assert!(crate::api::term_events(&f.hub, &id, &owner).is_err(), "sem replay ou segundo leitor");
    for (text, reason) in [
        ("!ls\n", "sem comandos ou controles"),
        ("/shell\n", "Comandos não são aceitos"),
        ("\u{1a}", "sem comandos ou controles"),
        ("\u{3}", "sem comandos ou controles"),
        ("\u{4}", "sem comandos ou controles"),
        ("\u{1b}", "Tecla indisponível"),
        ("vi /tmp/test\n", "sem comandos ou controles"),
        ("less /tmp/test\n", "sem comandos ou controles"),
        ("$(whoami)\n", "sem comandos ou controles"),
        ("`whoami`\n", "sem comandos ou controles"),
        (";whoami\n", "sem comandos ou controles"),
    ] {
        let data = base64::prelude::BASE64_STANDARD.encode(text);
        let error =
            crate::api::dispatch(&f.hub, &owner, "termInput", json!({"term":id,"data":data})).await.unwrap_err();
        let _ = reason;
        assert!(error.to_string().contains("Aguarde"), "{text:?}: {error:#}");
    }
    tokio::time::timeout(Duration::from_secs(8), async {
        while let Some(event) = stream.next().await {
            if let Some(data) = event["data"].as_str() {
                let bytes = base64::prelude::BASE64_STANDARD.decode(data).unwrap();
                assert!(!String::from_utf8_lossy(&bytes).contains("FIXTURE-CODE"));
            }
            if event["type"] == "verified" {
                return;
            }
        }
        panic!("login sem confirmação");
    })
    .await
    .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn terminal_login_revogado_no_meio_corta_entrada_e_leitura() {
    use futures::StreamExt;
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let owner = f.owner();
    let path = f.root.join("cursor-fixture");
    std::fs::write(&path,"#!/usr/bin/python3\nimport sys\nif sys.argv[1:]==['status']: print('Not logged in');sys.exit(1)\nprint('https://cursor.com/login?challenge=fixture',flush=True)\nsys.stdin.readline()\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let id = f.hub.terms.fixture_login(&f.hub, &owner, "cursor", path).unwrap();
    let mut stream = Box::pin(crate::api::term_events(&f.hub, &id, &owner).unwrap().unwrap());
    tokio::time::timeout(Duration::from_secs(3), stream.next()).await.unwrap().unwrap();
    f.hub.cloud_synced.store(false, std::sync::atomic::Ordering::Release);
    assert!(crate::api::dispatch(&f.hub, &owner, "termInput", json!({"term":id,"data":"RklYVFVSRQ0="})).await.is_err());
    assert!(crate::api::term_events(&f.hub, &id, &owner).is_err());
    assert!(tokio::time::timeout(Duration::from_secs(3), stream.next()).await.unwrap().is_none());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn terminal_login_saida_do_cli_encerra_descendente_e_remove_registro() {
    use futures::StreamExt;
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let owner = f.owner();
    let path = f.root.join("cursor-descendente");
    std::fs::write(
        &path,
        r"#!/usr/bin/python3
import os,sys,time,pathlib
if sys.argv[1:]==['status']: print('Not logged in');sys.exit(1)
pid=os.fork()
if pid==0:
 time.sleep(60);os._exit(0)
pathlib.Path(__file__).with_suffix('.pid').write_text(str(pid))
print('https://cursor.com/login?challenge=fixture',flush=True)
time.sleep(0.1)
",
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let id = f.hub.terms.fixture_login(&f.hub, &owner, "cursor", path.clone()).unwrap();
    let mut stream = Box::pin(crate::api::term_events(&f.hub, &id, &owner).unwrap().unwrap());
    tokio::time::timeout(Duration::from_secs(5), async { while stream.next().await.is_some() {} }).await.unwrap();
    assert!(f.hub.terms.get(&id).is_none());
    let pid: i32 = std::fs::read_to_string(path.with_extension("pid")).unwrap().parse().unwrap();
    let alive = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .is_ok_and(|s| s.rsplit_once(") ").is_some_and(|(_, tail)| !tail.starts_with('Z')));
    if alive {
        // SAFETY: PID devolvido pelo fork da fixture própria, só para não deixar processo em falha.
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
    assert!(!alive, "o descendente não sobrevive ao login");
}

#[cfg(unix)]
#[tokio::test]
async fn terminal_login_revogacao_antes_do_attach_remove_registro() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let owner = f.owner();
    let path = f.root.join("cursor-sem-attach");
    std::fs::write(&path, "#!/usr/bin/python3\nimport time\ntime.sleep(60)\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let id = f.hub.terms.fixture_login(&f.hub, &owner, "cursor", path).unwrap();
    f.hub.revoke_device(owner.device_key()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while f.hub.terms.get(&id).is_some() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn conclusao_terminal_reconfere_acs4_no_commit_e_na_admissao() {
    use crate::fork::terminal_login::conclusao::{self, Boundary};
    for boundary in [Boundary::Commit, Boundary::Send] {
        let f = Fixture::new();
        let owner = f.owner();
        let authority = access::resolve(&f.hub, &owner).unwrap();
        let bot: crate::store::BotConfig =
            serde_json::from_value(json!({"id":"login-fixture","name":"Fixture","backend":"cursor"})).unwrap();
        f.hub.store.save_bot(&bot).unwrap();
        let mut commands = f.hub.test_bot_commands(&bot.id);
        let lane = crate::store::Lane::main(&bot.id);
        let entry = f
            .hub
            .add_entry(&lane, crate::store::EntryKind::User, 0, &json!({"text":"Fixture","status":"held"}))
            .unwrap();
        crate::market::requests::invite_agent_sign_in(
            &f.hub,
            &bot.id,
            &lane,
            "cursor",
            "Cursor",
            std::slice::from_ref(&entry.id),
        )
        .unwrap();
        let revoke = f.hub.clone();
        let key = owner.device_key().to_owned();
        conclusao::before_test(&f.hub, boundary, move || {
            revoke.revoke_device(&key).unwrap();
        });
        assert!(conclusao::finish(&f.hub, "cursor", crate::fork::spaces::PERSONAL, &authority).is_err());
        assert!(commands.try_recv().is_err());
        assert_eq!(f.hub.store.pending_sign_ins("cursor").unwrap().len(), 1);
        // A recuperação do ator usa o estado durável: não basta deixar o canal vazio.
        assert_eq!(f.hub.store.entry(&entry.id).unwrap().data["status"], "held");
    }
}

#[path = "../terminal_login/lifecycle_tests.rs"]
mod terminal_login_lifecycle;
#[path = "../terminal_login/p3_tests.rs"]
mod terminal_login_p3;
/// MEX-1292 (P2 autoridade): o RPC `donnaSkillsDoutor` pelo despacho. Dono e Local passam; Device sem
/// `engine.accounts` (Executor, Screen, sem escopo), Control sem vínculo de dono e pareado são recusados.
#[tokio::test]
async fn skills_doutor_rpc_so_responde_ao_dono_das_contas() {
    use crate::store::Scope;
    let f = Fixture::new();
    let owner = f.owner();
    let bot: crate::store::BotConfig =
        serde_json::from_value(json!({"id":"doutor","name":"Doutor","backend":"claude","cwd":f.root})).unwrap();
    f.hub.store.save_bot(&bot).unwrap();
    let pedido = json!({"botId":"doutor"});
    for caller in [&Caller::Local, &owner] {
        let resposta = crate::api::dispatch(&f.hub, caller, "donnaSkillsDoutor", pedido.clone()).await.unwrap();
        assert_eq!(resposta["botId"], "doutor");
        assert_eq!(resposta["limpo"], false, "cobertura parcial nunca é limpa");
        assert!(resposta["harness"]["verificado"].is_boolean());
        let texto = resposta.to_string();
        assert!(!texto.contains(f.root.to_str().unwrap()), "nenhum caminho absoluto vai ao cliente: {texto}");
    }
    let mut outro = f.hub.store.device(owner.device_key()).unwrap();
    outro.key = "outro".into();
    f.hub.store.put_device(&outro).unwrap();
    for caller in [
        Caller::Device { key: "executor".into(), scopes: vec![Scope::Executor], autoridade: None },
        Caller::Device { key: "screen".into(), scopes: vec![Scope::Screen], autoridade: None },
        Caller::Device { key: "sem-escopo".into(), scopes: vec![], autoridade: None },
        Caller::Device { key: "outro".into(), scopes: vec![Scope::Control], autoridade: None },
        Caller::Pairing { key: "pareado".into() },
    ] {
        assert!(
            crate::api::dispatch(&f.hub, &caller, "donnaSkillsDoutor", pedido.clone()).await.is_err(),
            "chamador sem autoridade de contas consultou o doutor"
        );
    }
}

/// MEX-1292 (r2 #4): revogado DEPOIS da captura (`resolve`) e ANTES da resposta, o RPC do doutor recusa e
/// não devolve nenhum dado.
#[tokio::test]
async fn skills_doutor_revogado_durante_a_coleta_nao_devolve_dados() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let f = Fixture::new();
    let owner = f.owner();
    let bot: crate::store::BotConfig =
        serde_json::from_value(json!({"id":"doutor","name":"Doutor","backend":"claude","cwd":f.root})).unwrap();
    f.hub.store.save_bot(&bot).unwrap();
    let pedido = json!({"botId":"doutor"});
    assert!(crate::api::dispatch(&f.hub, &owner, "donnaSkillsDoutor", pedido.clone()).await.is_ok());
    let fired = Arc::new(AtomicBool::new(false));
    {
        let (revoker, fired, key) = (f.hub.clone(), fired.clone(), owner.device_key().to_owned());
        *f.hub.store.before_owner_write.lock().unwrap() = Some(Box::new(move || {
            fired.store(true, Ordering::SeqCst);
            revoker.revoke_device(&key).unwrap();
        }));
    }
    let erro = crate::api::dispatch(&f.hub, &owner, "donnaSkillsDoutor", pedido).await.unwrap_err();
    assert!(fired.load(Ordering::SeqCst), "passou pela janela entre a coleta e a resposta");
    assert!(!erro.to_string().contains("botId"), "a recusa não carrega dados do bot: {erro}");
}

/// F6-A: o uso é lido por conta E por Space; conta de outro Space (o Pessoal incluído) nunca é consultada.
fn leituras_do_cli(cli: &std::path::Path) -> usize {
    std::fs::read_to_string(cli.parent().unwrap().join("rate-reads")).map_or(0, |t| t.lines().count())
}

fn bot_do_space(f: &Fixture, id: &str, space: &str) {
    let cfg: crate::store::BotConfig =
        serde_json::from_value(json!({"id":id,"name":"Fixture","backend":"codex","spaceId":space})).unwrap();
    f.hub.store.save_bot(&cfg).unwrap();
}

async fn codex_conectado(f: &Fixture, request: &str) -> (std::path::PathBuf, String) {
    let cli = f.cli(Backend::Codex);
    let account = f.create("codex", "cli", request).await;
    let op = f.start(&account, &format!("{request}-login")).await;
    f.phase(&op, "connected").await;
    (cli, account["id"].as_str().unwrap().to_owned())
}

#[tokio::test]
async fn uso_por_conta_e_por_space() {
    use crate::api::dispatch;
    let f = Fixture::new();
    let (cli, id) = codex_conectado(&f, "f6a").await;
    let pessoal = crate::fork::spaces::PERSONAL;

    // `null` ou tipo errado = invalid_space; Space que não existe = space_not_found; nada consulta a conta.
    for bad in [Value::Null, json!(7), json!(true), json!("")] {
        assert_eq!(usage(&f.hub, id.clone(), Some(&bad)).await.unwrap_err().to_string(), "invalid_space");
    }
    assert_eq!(
        usage(&f.hub, id.clone(), Some(&json!("inexistente"))).await.unwrap_err().to_string(),
        "space_not_found"
    );
    // O Pessoal também é um Space: conta sem vínculo com ele não é lida, com Space ausente ou explícito.
    for space in [None, Some(json!(pessoal))] {
        let erro = usage(&f.hub, id.clone(), space.as_ref()).await.unwrap_err();
        assert_eq!(erro.to_string(), "account_wrong_space", "{space:?}");
    }
    let spaces = dispatch(&f.hub, &Caller::Local, "donnaSpaces", json!({"hubId":f.hub.host_id})).await.unwrap();
    let trabalho = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaSpaceCreate",
        json!({"hubId":f.hub.host_id,"expectedRevision":spaces["revision"],"name":"Trabalho",
            "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}}}),
    )
    .await
    .unwrap()["spaces"][1]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(usage(&f.hub, id.clone(), Some(&json!(trabalho))).await.unwrap_err().to_string(), "account_wrong_space");
    assert_eq!(leituras_do_cli(&cli), 0, "recusa antes de consultar o CLI");

    // Vínculo só com o Trabalho (conversa do Space com a conta): o Trabalho lê e o Pessoal continua recusado.
    bot_do_space(&f, "bot-trabalho", &trabalho);
    f.hub
        .store
        .db
        .locked()
        .execute(
            "INSERT INTO donna_space_conversations(id,space_id,bot_id,title,revision,is_primary,engine_account_id)
             VALUES('conv-f6a',?1,'bot-trabalho','',1,0,?2)",
            rusqlite::params![trabalho, id],
        )
        .unwrap();
    let no_trabalho = usage(&f.hub, id.clone(), Some(&json!(trabalho))).await.unwrap();
    assert_eq!(no_trabalho["spaceId"], trabalho);
    assert_eq!(no_trabalho["state"], "available");
    assert_eq!(usage(&f.hub, id.clone(), None).await.unwrap_err().to_string(), "account_wrong_space");

    // O caso que ficava de fora: a seleção da conversa do Pessoal (D372) vincula a conta ao Pessoal.
    bot_do_space(&f, "bot-pessoal", pessoal);
    let escolha = json!({"schemaVersion":2,"source":"own","accountId":id,"backend":"codex","model":null,
        "modelOptions":{},"fastMode":false});
    f.hub
        .store
        .db
        .locked()
        .execute(
            "INSERT INTO donna_bot_sessions(id,chat_id,principal,origin,title_source,created_at,last_used_at,revision,engine_next)
             VALUES('sessao-f6a','bot-pessoal',0,'test','auto',0,0,1,?1)",
            [escolha.to_string()],
        )
        .unwrap();
    let no_pessoal = usage(&f.hub, id.clone(), None).await.unwrap();
    assert_eq!(no_pessoal["spaceId"], pessoal);
    assert_eq!(no_pessoal["state"], "available");
    assert_eq!(no_pessoal["windows"][0]["id"], "session");
    assert_eq!(no_pessoal["windows"][0]["percent"], 23.0);
    assert_eq!(no_pessoal["windows"].as_array().unwrap().len(), 1, "semana ausente fica de fora");
}

#[tokio::test]
async fn uso_codex_classifica_pela_duracao_e_descarta_janela_renovada() {
    let f = Fixture::new();
    let (cli, id) = codex_conectado(&f, "f6a-codex").await;
    bot_do_space(&f, "bot-codex", crate::fork::spaces::PERSONAL);
    let mut cfg = f.hub.store.bot("bot-codex").unwrap().unwrap().config;
    cfg.fork_editor.engine_account_id = Some(id.clone());
    f.hub.store.save_bot(&cfg).unwrap();
    let longe = 4_102_444_800_i64; // 2100
    let ler = |rates: Value| {
        let (f, cli, id) = (&f, &cli, id.clone());
        async move {
            std::fs::write(cli.parent().unwrap().join("rates"), rates.to_string()).unwrap();
            usage_cache::clear(&id);
            usage(&f.hub, id, None).await.unwrap()
        }
    };
    let janelas = |v: &Value| -> Vec<(String, f64, bool)> {
        v["windows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| (w["id"].as_str().unwrap().to_owned(), w["percent"].as_f64().unwrap(), w["nearLimit"] == true))
            .collect()
    };

    // Conta só semanal: a `primary` dura 10080 min e é a SEMANA, nunca a sessão.
    let v = ler(json!({"rateLimits":{"primary":{"usedPercent":95,"windowDurationMins":10080,"resetsAt":longe}}})).await;
    assert_eq!(janelas(&v), [("week".to_owned(), 95.0, true)]);
    // Ordem trocada: a duração manda, não o nome do campo.
    let v = ler(json!({"rateLimits":{
        "primary":{"usedPercent":40,"windowDurationMins":10080,"resetsAt":longe},
        "secondary":{"usedPercent":91,"windowDurationMins":300,"resetsAt":longe}}}))
    .await;
    assert_eq!(janelas(&v), [("session".to_owned(), 91.0, true), ("week".to_owned(), 40.0, false)]);
    // Janela que já renovou não marca uso antigo: a sessão vencida some e a semana segue.
    let v = ler(json!({"rateLimits":{
        "primary":{"usedPercent":97,"windowDurationMins":300,"resetsAt":1},
        "secondary":{"usedPercent":12,"windowDurationMins":10080,"resetsAt":longe}}}))
    .await;
    assert_eq!(janelas(&v), [("week".to_owned(), 12.0, false)]);
    // Tudo renovado = sem leitura (desconhecido), nunca 97% antigo nem 0%.
    let v = ler(json!({"rateLimits":{"primary":{"usedPercent":97,"windowDurationMins":300,"resetsAt":1}}})).await;
    assert_eq!(v["state"], "unavailable");
    assert_eq!(v["windows"].as_array().unwrap().len(), 0);
    // Mesmo bucket do painel global: `codex` de `rateLimitsByLimitId` vale mais que o balde de compatibilidade.
    let v = ler(json!({
        "rateLimits":{"primary":{"usedPercent":99,"windowDurationMins":300,"resetsAt":longe}},
        "rateLimitsByLimitId":{"codex":{"primary":{"usedPercent":12,"windowDurationMins":300,"resetsAt":longe}}}}))
    .await;
    assert_eq!(janelas(&v), [("session".to_owned(), 12.0, false)]);
    // Sem duração informada, o nome do campo é o último recurso.
    let v = ler(json!({"rateLimits":{"primary":{"usedPercent":30,"resetsAt":longe}}})).await;
    assert_eq!(janelas(&v), [("session".to_owned(), 30.0, false)]);
}

#[tokio::test]
async fn uso_uma_leitura_por_vez_com_validade_e_timeout() {
    let f = Fixture::new();
    let (cli, id) = codex_conectado(&f, "f6a-cache").await;
    bot_do_space(&f, "bot-cache", crate::fork::spaces::PERSONAL);
    let mut cfg = f.hub.store.bot("bot-cache").unwrap().unwrap().config;
    cfg.fork_editor.engine_account_id = Some(id.clone());
    f.hub.store.save_bot(&cfg).unwrap();
    let pasta = cli.parent().unwrap().to_path_buf();
    usage_cache::clear(&id);

    // Dez linhas pedem ao mesmo tempo: um processo só, todas com a mesma resposta.
    std::fs::write(pasta.join("rate-delay"), "0.4").unwrap();
    let todas = futures::future::join_all((0..10).map(|_| usage(&f.hub, id.clone(), None))).await;
    let primeira = todas[0].as_ref().unwrap().clone();
    assert_eq!(primeira["state"], "available");
    assert!(todas.iter().all(|r| r.as_ref().unwrap() == &primeira), "mesma resposta para todas");
    assert_eq!(leituras_do_cli(&cli), 1, "single-flight: uma leitura para dez pedidos");
    // Trocar de conversa ou de Space dentro da validade não sobe processo: sai do cache.
    for _ in 0..5 {
        assert_eq!(usage(&f.hub, id.clone(), None).await.unwrap(), primeira);
    }
    assert_eq!(leituras_do_cli(&cli), 1, "dentro da validade não lê de novo");
    // Vencida a validade, lê de novo (e uma só vez).
    usage_cache::clear(&id);
    std::fs::remove_file(pasta.join("rate-delay")).unwrap();
    usage(&f.hub, id.clone(), None).await.unwrap();
    assert_eq!(leituras_do_cli(&cli), 2);

    // CLI que não responde: o timeout corta, a conta fica "sem leitura" e a falha também é guardada
    // (não martela o CLI quebrado a cada linha).
    usage_cache::clear(&id);
    std::fs::write(pasta.join("rate-delay"), "60").unwrap();
    let antes = std::time::Instant::now();
    let travado = futures::future::join_all((0..3).map(|_| usage(&f.hub, id.clone(), None))).await;
    assert!(antes.elapsed() < Duration::from_secs(20), "o timeout corta a leitura: {:?}", antes.elapsed());
    assert!(travado.iter().all(|r| r.as_ref().unwrap()["state"] == "unavailable"));
    assert_eq!(leituras_do_cli(&cli), 3, "uma só tentativa para as três linhas");
    assert_eq!(usage(&f.hub, id.clone(), None).await.unwrap()["state"], "unavailable");
    assert_eq!(leituras_do_cli(&cli), 3, "falha recente não relê");
}
