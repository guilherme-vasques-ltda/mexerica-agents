#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Prova com um programa falso que imita o Grok Build: nenhuma conta, rede ou assinatura reais.
use super::*;
use crate::{
    LockExt,
    api::devices::Caller,
    fork::engine_accounts::{Billing, State},
    hub::Hub,
    store::Store,
};
use serde_json::{Value, json};

const TOKEN: &str = "FIXTURE-TOKEN-SECRET";
const WAIT: Duration = Duration::from_secs(60);

struct Fixture {
    hub: Arc<Hub>,
    root: PathBuf,
    control: PathBuf,
}
impl Fixture {
    fn new(style: &str) -> Self {
        use crate::remote::identity::Identity;
        let store = Store::open(Path::new(":memory:")).unwrap();
        let root = store.engine_setup_root.clone();
        std::fs::create_dir_all(&root).unwrap();
        let identity = Identity::load_or_create(&root).unwrap();
        let hub = Hub::new(store, "fixture-host".into(), identity, "fixture-token".into(), 0);
        let control = root.join("bin").join("grok");
        std::fs::create_dir_all(&control).unwrap();
        let program = control.join("grok");
        std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/grok_cli.py"), &program).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        std::fs::write(control.join("style"), style).unwrap();
        Self { hub, root, control }
    }
    fn decide(&self, answer: &str) {
        std::fs::write(self.control.join(answer), "").unwrap();
    }
    async fn create(&self, request: &str) -> Value {
        super::super::call(
            &self.hub,
            &Caller::Local,
            "donnaEngineAccountCreate",
            json!({"backend":"grok","kind":"cli","label":"Fictícia","clientRequestId":request}),
        )
        .await
        .unwrap()
    }
    async fn start(&self, account: &Value, action: &str, request: &str) -> Value {
        super::super::call(
            &self.hub,
            &Caller::Local,
            "donnaEngineSetupStart",
            json!({"engineAccountId":account["id"],"expectedRevision":account["revision"],"action":action,"clientRequestId":request}),
        )
        .await
        .unwrap()
    }
    fn read(&self, op: &Value, kind: Value) -> Value {
        let mut body = json!({"operationId":op["operationId"],"generation":op["generation"]});
        let Value::Object(fields) = kind else { panic!("ação inválida") };
        for (key, value) in fields {
            body[&key] = value;
        }
        super::super::action(&self.hub, super::super::parse(body).unwrap()).unwrap()
    }
    async fn phase(&self, op: &Value, target: &str) -> Value {
        tokio::time::timeout(WAIT, async {
            loop {
                let value = self.read(op, json!({"action":"read"}));
                if value["phase"] == target {
                    return value;
                }
                assert!(
                    target != "awaiting_authorization"
                        || !["failed", "expired", "cancelled"].iter().any(|p| value["phase"] == *p),
                    "fase inesperada: {value}"
                );
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("operação não chegou a {target}"))
    }
    fn account(&self, id: &Value) -> Value {
        Registry::load(&self.hub.store).unwrap().dto_for(&self.hub.store, id.as_str().unwrap()).unwrap()
    }
    fn home(&self, id: &Value) -> PathBuf {
        let registry = Registry::load(&self.hub.store).unwrap();
        profile_home(registry.account(id.as_str().unwrap()).unwrap()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn screen(chunks: &[&[u8]]) -> Option<Challenge> {
    let mut screen = Screen::default();
    for chunk in chunks {
        screen.push(chunk).unwrap();
    }
    screen.challenge().unwrap_or(None)
}

#[test]
fn endereco_e_codigo_saem_so_de_linhas_completas_e_so_de_sites_da_xai() {
    let plain =
        screen(&[b"Open this page:\r\n  https://auth.x.ai/device\r\nThen enter this code: ABCD-EFGH\r\n"]).unwrap();
    assert_eq!(
        (plain.url.as_str(), plain.display_domain.as_str(), plain.code.as_deref()),
        ("https://auth.x.ai/device", "auth.x.ai", Some("ABCD-EFGH"))
    );
    let confirm = screen(&[b"Confirm this code in your browser: WXYZ-2468\nhttps://accounts.x.ai/device\n"]).unwrap();
    assert_eq!(confirm.code.as_deref(), Some("WXYZ-2468"));
    let complete = screen(&[b"Open https://auth.x.ai/device?user_code=QRST-1357 to continue.\n"]).unwrap();
    assert_eq!(complete.code.as_deref(), Some("QRST-1357"));
    let ansi = screen(&[
        b"\x1b]52;c;SEGREDO\x07\x1b[1mhttps://auth.x.ai/de",
        b"vice\x1b[0m\n\x1b[36mThen enter this code:\x1b[0m ABCD-",
        b"EFGH\x1b[0m\n",
    ])
    .unwrap();
    assert_eq!((ansi.url.as_str(), ansi.code.as_deref()), ("https://auth.x.ai/device", Some("ABCD-EFGH")));
    assert!(!format!("{}{:?}", ansi.url, ansi.code).contains("SEGREDO"));

    // Linha ainda incompleta: nada sai, para nunca mostrar um código cortado.
    assert!(screen(&[b"https://auth.x.ai/device\nThen enter this code: ABCD-"]).is_none());
    assert!(screen(&[b"https://auth.x.ai/dev"]).is_none());
    // Fora da xAI, com porta, usuário, fragmento, parâmetro estranho ou código fora do formato: recusa.
    for url in [
        "https://evil.example.invalid/device",
        "https://auth.x.ai.evil.example.invalid/device",
        "http://auth.x.ai/device",
        "https://user@auth.x.ai/device",
        "https://auth.x.ai:8443/device",
        "https://auth.x.ai/device#parte",
        "https://auth.x.ai/device?access_token=x",
    ] {
        let text = format!("{url}\nThen enter this code: ABCD-EFGH\n");
        assert!(screen(&[text.as_bytes()]).is_none(), "{url}");
    }
    for code in ["abcd-efgh", "AB", "ABCD EFGH $(id)", "ABCD;EFGH"] {
        let text = format!("https://auth.x.ai/device\nThen enter this code: {code}\n");
        assert!(screen(&[text.as_bytes()]).is_none(), "{code}");
    }
    let mut flood = Screen::default();
    assert!(flood.push(&vec![b'a'; 40 * 1024]).is_err());
}

#[tokio::test]
async fn aprovado_mostra_endereco_e_codigo_confirma_com_grok_models_e_nao_guarda_token() {
    let f = Fixture::new("plain");
    let account = f.create("grok-ok").await;
    assert_eq!(account["backend"], "grok");
    assert_eq!(account["brand"], "xAI");
    let op = f.start(&account, "login", "grok-login").await;
    let waiting = f.phase(&op, "awaiting_authorization").await;
    assert_eq!(waiting["challenge"]["kind"], "device_code");
    assert_eq!(waiting["challenge"]["url"], "https://auth.x.ai/device");
    assert_eq!(waiting["challenge"]["code"], "ABCD-EFGH");
    assert_eq!(f.account(&account["id"])["state"], "unverified", "antes de autorizar não há conta conectada");

    f.decide("approve");
    let done = f.phase(&op, "connected").await;
    let connected = f.account(&account["id"]);
    assert_eq!((connected["state"].as_str(), connected["billing"].as_str()), (Some("connected"), Some("subscription")));
    assert!(done["challenge"].is_null());

    // A credencial fica só no perfil isolado da conta, e o Hub nunca a lê nem a replica.
    let home = f.home(&account["id"]);
    let auth = home.join(".grok/auth.json");
    assert!(std::fs::read_to_string(&auth).unwrap().contains(TOKEN));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&auth).unwrap().permissions().mode() & 0o777, 0o600);
    }
    let db = f.hub.store.db.locked();
    let mut query = db.prepare("SELECT v FROM kv").unwrap();
    for stored in query.query_map([], |row| row.get::<_, String>(0)).unwrap() {
        assert!(!stored.unwrap().contains(TOKEN), "token no estado do Hub");
    }
    drop(query);
    drop(db);
    assert!(!connected.to_string().contains(TOKEN) && !done.to_string().contains(TOKEN));

    // Um bot que usa a conta recebe o perfil dela e nenhuma chave de API.
    let bot: crate::store::BotConfig = serde_json::from_value(
        json!({"id":"bot-grok","name":"Fixture","backend":"grok","forkEditor":{"engineAccountId":account["id"]}}),
    )
    .unwrap();
    f.hub.store.save_bot(&bot).unwrap();
    let launch = managed::resolve_for_bot(&f.hub.store, &bot).unwrap().unwrap();
    let env = |key: &str| launch.env.iter().find(|(name, _)| name == key).map(|(_, value)| value.clone());
    assert_eq!(env("GROK_HOME"), Some(home.join(".grok").to_string_lossy().into_owned()));
    assert_eq!(env("HOME"), Some(home.to_string_lossy().into_owned()));
    assert!(env("XAI_API_KEY").is_none());
}

#[tokio::test]
async fn recusado_expirado_e_formatos_do_programa() {
    let f = Fixture::new("confirm");
    let account = f.create("grok-deny").await;
    let op = f.start(&account, "login", "grok-deny-login").await;
    assert_eq!(f.phase(&op, "awaiting_authorization").await["challenge"]["code"], "WXYZ-2468");
    f.decide("deny");
    let failed = f.phase(&op, "failed").await;
    assert_eq!(failed["errorCode"], "auth_failed");
    assert_ne!(f.account(&account["id"])["state"], "connected");

    let again = f.account(&account["id"]);
    std::fs::remove_file(f.control.join("deny")).unwrap();
    std::fs::write(f.control.join("style"), "complete").unwrap();
    let op = f.start(&again, "login", "grok-expire-login").await;
    assert_eq!(f.phase(&op, "awaiting_authorization").await["challenge"]["code"], "QRST-1357");
    f.decide("expire");
    let expired = f.phase(&op, "expired").await;
    assert_eq!(expired["errorCode"], "setup_expired");
    assert!(expired["challenge"].is_null());
    assert_ne!(f.account(&account["id"])["state"], "connected");
}

#[tokio::test]
async fn endereco_de_outro_site_nunca_chega_a_tela() {
    let f = Fixture::new("foreign");
    let account = f.create("grok-foreign").await;
    let op = f.start(&account, "login", "grok-foreign-login").await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let value = f.read(&op, json!({"action":"read"}));
    assert_eq!(value["phase"], "preparing");
    assert!(value["challenge"].is_null());
    let _ = f.read(&op, json!({"action":"cancel"}));
}

#[cfg(target_os = "linux")]
fn alive(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
        stat.rsplit_once(')').and_then(|(_, rest)| rest.trim_start().chars().next()).is_some_and(|state| state != 'Z')
    })
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn cancelar_encerra_o_programa_e_nao_deixa_login() {
    let f = Fixture::new("ansi");
    let account = f.create("grok-cancel").await;
    let op = f.start(&account, "login", "grok-cancel-login").await;
    let waiting = f.phase(&op, "awaiting_authorization").await;
    assert_eq!(waiting["challenge"]["url"], "https://auth.x.ai/device");
    assert_eq!(waiting["challenge"]["code"], "ABCD-EFGH");
    assert!(!waiting.to_string().contains("SENTINEL"));
    let pid: i32 = std::fs::read_to_string(f.control.join("login-pids")).unwrap().trim().parse().unwrap();
    assert!(alive(pid));
    let generations = f.root.join("accounts").join(account["id"].as_str().unwrap());
    let count = || std::fs::read_dir(&generations).unwrap().count();
    assert_eq!(count(), 2, "a pasta original e a de preparação do login");
    let cancelled = f.read(&op, json!({"action":"cancel"}));
    assert_eq!(cancelled["phase"], "cancelled");
    assert!(cancelled["challenge"].is_null());
    tokio::time::timeout(Duration::from_secs(15), async {
        while alive(pid) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("o programa de login continuou vivo depois de cancelar");
    tokio::time::timeout(Duration::from_secs(15), async {
        while count() != 1 {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("a pasta de preparação do login cancelado continuou no disco");
    f.decide("approve");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_ne!(f.account(&account["id"])["state"], "connected");
    assert!(!f.home(&account["id"]).join(".grok/auth.json").exists());
    for entry in std::fs::read_dir(&generations).unwrap() {
        assert!(!entry.unwrap().path().join(".grok/auth.json").exists(), "credencial deixada pelo login cancelado");
    }
}

#[tokio::test]
async fn desconectar_para_o_trabalho_apaga_o_login_e_recusa_se_nao_parar() {
    let f = Fixture::new("plain");
    let account = f.create("grok-disconnect").await;
    let op = f.start(&account, "login", "grok-disconnect-login").await;
    f.phase(&op, "awaiting_authorization").await;
    f.decide("approve");
    f.phase(&op, "connected").await;
    let connected = f.account(&account["id"]);
    let home = f.home(&account["id"]);
    assert!(home.join(".grok/auth.json").is_file());

    let bot: crate::store::BotConfig = serde_json::from_value(
        json!({"id":"bot-grok","name":"Fixture","backend":"grok","forkEditor":{"engineAccountId":account["id"]}}),
    )
    .unwrap();
    f.hub.store.save_bot(&bot).unwrap();
    f.hub.set_runtime("bot-grok", |r| r.status = BotStatus::Working);
    let mut commands = f.hub.test_bot_commands("bot-grok");
    let request = json!({"engineAccountId":account["id"],"expectedRevision":connected["revision"]});

    // Trabalho que não para: a conta continua conectada e o login, no lugar.
    let refused = super::super::call(&f.hub, &Caller::Local, "donnaEngineAccountDisconnect", request.clone()).await;
    assert!(refused.unwrap_err().to_string().starts_with("account_busy"));
    assert!(matches!(commands.try_recv(), Ok(crate::agent::bot::Cmd::Stop)));
    assert!(home.join(".grok/auth.json").is_file());
    assert_eq!(f.account(&account["id"])["state"], "connected");

    // Trabalho que obedece ao Parar: a conta desconecta e o login local some.
    let hub = f.hub.clone();
    tokio::spawn(async move {
        while let Some(command) = commands.recv().await {
            if matches!(command, crate::agent::bot::Cmd::Stop) {
                hub.set_runtime("bot-grok", |r| r.status = BotStatus::default());
            }
        }
    });
    let done = super::super::call(&f.hub, &Caller::Local, "donnaEngineAccountDisconnect", request).await.unwrap();
    assert_eq!(done["state"], "disconnected");
    assert!(!home.join(".grok/auth.json").exists(), "o login do programa foi apagado");
    assert_eq!(std::fs::read_dir(home.join(".grok")).unwrap().count(), 0, "a pasta fica vazia e no lugar");
    assert_eq!(f.account(&account["id"])["models"], json!([]));

    // A conta desconectada entra de novo, e Remover leva a pasta inteira.
    let again = f.start(&done, "login", "grok-reconnect").await;
    f.phase(&again, "awaiting_authorization").await;
    f.decide("approve");
    f.phase(&again, "connected").await;
    let renewed = f.home(&account["id"]);
    assert!(renewed.join(".grok/auth.json").is_file());
    let current = f.account(&account["id"]);
    let removed = super::super::call(
        &f.hub,
        &Caller::Local,
        "donnaEngineAccountDisconnect",
        json!({"engineAccountId":account["id"],"expectedRevision":current["revision"]}),
    )
    .await
    .unwrap();
    let gone = super::super::call(
        &f.hub,
        &Caller::Local,
        "donnaEngineAccountRemove",
        json!({"engineAccountId":account["id"],"expectedRevision":removed["revision"]}),
    )
    .await;
    assert!(gone.unwrap_err().to_string().starts_with("account_in_use"), "bot ainda vinculado");
    f.hub.store.delete_bot("bot-grok").ok();
}

#[tokio::test]
async fn novo_login_apaga_o_perfil_do_login_anterior() {
    let f = Fixture::new("plain");
    let account = f.create("grok-relogin").await;
    let op = f.start(&account, "login", "grok-relogin-1").await;
    f.phase(&op, "awaiting_authorization").await;
    f.decide("approve");
    f.phase(&op, "connected").await;
    let first = f.home(&account["id"]);
    let op = f.start(&f.account(&account["id"]), "login", "grok-relogin-2").await;
    f.phase(&op, "connected").await;
    let second = f.home(&account["id"]);
    assert_ne!(first, second);
    assert!(!first.exists() && second.join(".grok/auth.json").is_file());
}

#[tokio::test]
async fn grok_so_entra_por_login_e_sem_programa_a_tela_recebe_o_motivo() {
    let f = Fixture::new("plain");
    let api = super::super::call(
        &f.hub,
        &Caller::Local,
        "donnaEngineAccountCreate",
        json!({"backend":"grok","kind":"api","label":"Chave","clientRequestId":"grok-api"}),
    )
    .await;
    assert!(api.unwrap_err().to_string().starts_with("invalid_field"));

    std::fs::remove_file(f.control.join("grok")).unwrap();
    if crate::agent::backends::which("grok").is_some() {
        return;
    }
    let account = f.create("grok-missing").await;
    let install = f.start(&account, "install", "grok-install").await;
    let failed = tokio::time::timeout(WAIT, async {
        loop {
            let value = f.read(&install, json!({"action":"read"}));
            if value["phase"] == "failed" {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(failed["errorCode"], "grok_not_installed");
    assert_eq!(f.account(&account["id"])["state"], "unverified");
    // Pedir o login direto, sem passar pela preparação, dá o mesmo motivo.
    let login = f.start(&f.account(&account["id"]), "login", "grok-login-missing").await;
    let failed = tokio::time::timeout(WAIT, async {
        loop {
            let value = f.read(&login, json!({"action":"read"}));
            if value["phase"] == "failed" {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(failed["errorCode"], "grok_not_installed");
}

#[test]
fn leitura_firme_da_saida_do_programa() {
    // Sequências de string (DCS, APC, PM, SOS) e escapes de um byte extra não vazam texto.
    let hidden =
        screen(&[b"\x1bPsegredo\x1b\\https://auth.x.ai/device\n\x1b_apc\x07\x1b(BThen enter this code: ABCD-EFGH\n"])
            .unwrap();
    assert_eq!(hidden.code.as_deref(), Some("ABCD-EFGH"));
    // Código no texto diferente do da URL: não mostra nada.
    assert!(
        screen(&[b"Open https://auth.x.ai/device?user_code=AAAA-1111\nThen enter this code: BBBB-2222\n"]).is_none()
    );
    assert_eq!(
        screen(&[b"Open https://auth.x.ai/device?user_code=AAAA-1111\nThen enter this code: AAAA-1111\n"])
            .unwrap()
            .code
            .as_deref(),
        Some("AAAA-1111")
    );
    // Consulta com valor inválido na URL, mesmo com um código bom no texto: recusa a URL inteira.
    assert!(screen(&[b"https://auth.x.ai/device?user_code=abc.def\nThen enter this code: ABCD-EFGH\n"]).is_none());
    // Duas chaves na consulta, ou valor fora do formato: recusa.
    assert!(screen(&[b"https://auth.x.ai/device?user_code=AAAA-1111&x=1\n"]).is_none());
    assert!(screen(&[b"https://auth.x.ai/device?user_code=abcd\n"]).is_none());
    // Sem rótulo, só uma linha inteira no formato do código; siglas e versões soltas no texto não valem.
    assert_eq!(screen(&[b"https://auth.x.ai/device\nABCD-EFGH\n"]).unwrap().code.as_deref(), Some("ABCD-EFGH"));
    assert!(screen(&[b"https://auth.x.ai/device\nPress CTRL-C to quit. UTF-8 SHA-256 2026-11\n"]).is_none());
    // Saída sem fim, mesmo sem texto, estoura o limite.
    let mut flood = Screen::default();
    let mut result = Ok(());
    for _ in 0..400 {
        result = flood.push(&[0x1b, b']', b'x', b'x', b'x', b'x', b'x', b'x'].repeat(100));
        if result.is_err() {
            break;
        }
    }
    assert!(result.is_err());
}

#[test]
fn so_o_login_de_sessao_da_xai_vira_assinatura() {
    let account = format!("ac_{}", "a".repeat(64));
    let observe = |output: &str, success: bool| {
        let directory = PathBuf::from(format!("/hub/accounts/{account}/perfil/.grok"));
        let mut observation = probe::Observation::cli(Backend::Grok, Path::new("/fixture/grok"));
        observation.launch = LaunchReference::CliProfile { executable: "/fixture/grok".into(), directory };
        probe::grok_status(&mut observation, success, output);
        observation
    };
    let session = observe("You are logged in with grok.com.\ngrok-4.7\n", true);
    assert_eq!((session.state, session.billing), (State::Connected, Billing::Subscription));
    assert_eq!(session.identity.as_deref(), Some(format!("grok-profile:{account}").as_str()));
    assert!(session.subscription_proof.is_some());
    for output in [
        "You are logged in with an API key.\n",
        "You are logged in with XAI_API_KEY.\n",
        "You are logged in with grok.com via token\n",
        "You are logged in with the web.\n",
    ] {
        let other = observe(output, true);
        assert_eq!((other.state, other.billing), (State::Unverified, Billing::Unverified), "{output}");
        assert!(other.subscription_proof.is_none());
    }
    assert_eq!(observe("You are not authenticated.\n", false).state, State::Disconnected);
    assert_eq!(observe("", false).state, State::Unavailable);
}

#[tokio::test]
async fn apagar_so_aceita_o_formato_exato_do_perfil() {
    let f = Fixture::new("plain");
    let account = f.create("grok-shape").await;
    let id = account["id"].as_str().unwrap();
    let accounts = f.root.join("accounts").join(id);
    let outside = f.root.join("fora");
    std::fs::create_dir_all(outside.join(".grok")).unwrap();
    std::fs::write(outside.join(".grok/auth.json"), TOKEN).unwrap();
    // Pasta fora da conta, ou com o formato errado, nunca é apagada.
    for home in [outside.clone(), accounts.clone(), accounts.join("uuid").join("extra")] {
        std::fs::create_dir_all(home.join(".grok")).ok();
        assert!(wipe(&f.hub.store, id, &home).is_err(), "{home:?}");
        assert!(erase(&f.hub.store, id, &home).is_err(), "{home:?}");
    }
    assert!(outside.join(".grok/auth.json").is_file());
    #[cfg(unix)]
    {
        let link = accounts.join("link");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        assert!(wipe(&f.hub.store, id, &link).is_err());
        assert!(outside.join(".grok/auth.json").is_file());
    }
    // Pasta da geração que é link para fora da raiz, mesmo sem .grok lá dentro: recusa e não cria nada.
    #[cfg(unix)]
    {
        let bare = f.root.join("sem-grok");
        std::fs::create_dir_all(&bare).unwrap();
        let generation = accounts.join(uuid::Uuid::new_v4().to_string());
        std::os::unix::fs::symlink(&bare, &generation).unwrap();
        assert!(wipe(&f.hub.store, id, &generation).is_err());
        assert!(erase(&f.hub.store, id, &generation).is_err());
        assert_eq!(std::fs::read_dir(&bare).unwrap().count(), 0, "nada foi criado fora da raiz");
    }
    // Pasta que já não existe: nada a apagar.
    assert!(wipe(&f.hub.store, id, &accounts.join("sumiu")).is_ok());
}

#[tokio::test]
async fn remover_apaga_o_login_antes_e_a_pasta_depois_e_recusa_com_bot_vinculado() {
    let f = Fixture::new("plain");
    let account = f.create("grok-remove").await;
    let op = f.start(&account, "login", "grok-remove-login").await;
    f.phase(&op, "awaiting_authorization").await;
    f.decide("approve");
    f.phase(&op, "connected").await;
    let home = f.home(&account["id"]);
    let current = f.account(&account["id"]);
    let bot: crate::store::BotConfig = serde_json::from_value(
        json!({"id":"bot-grok","name":"Fixture","backend":"grok","forkEditor":{"engineAccountId":account["id"]}}),
    )
    .unwrap();
    f.hub.store.save_bot(&bot).unwrap();
    let request = json!({"engineAccountId":account["id"],"expectedRevision":current["revision"]});

    // Com bot vinculado a conta fica como estava, login incluído.
    let refused = super::super::call(&f.hub, &Caller::Local, "donnaEngineAccountRemove", request.clone()).await;
    assert!(refused.unwrap_err().to_string().starts_with("account_in_use"));
    assert!(home.join(".grok/auth.json").is_file());
    assert_eq!(f.account(&account["id"])["state"], "connected");

    f.hub.store.delete_bot("bot-grok").unwrap();
    super::super::call(&f.hub, &Caller::Local, "donnaEngineAccountRemove", request).await.unwrap();
    assert!(!home.exists(), "o login e a pasta saíram");
    assert!(Registry::load(&f.hub.store).unwrap().account(account["id"].as_str().unwrap()).is_err());
}

#[test]
fn o_grok_nao_usa_adaptador_de_outro_motor() {
    assert_eq!(managed::adapter_names("grok"), [] as [&str; 0]);
    assert_eq!(managed::adapter_names("codex"), ["codex-acp"]);
    assert!(managed::adapter_names("claude").contains(&"claude-agent-acp"));
}

#[test]
fn retorno_de_carro_separa_linhas_e_mudanca_depois_do_desafio_e_recusada() {
    let crlf = screen(&[b"https://auth.x.ai/device\rThen enter this code: ABCD-EFGH\r"]).unwrap();
    assert_eq!(crlf.code.as_deref(), Some("ABCD-EFGH"));
    // O que já foi mostrado não pode mudar depois: texto diferente do código da URL derruba o login.
    let mut later = Screen::default();
    later.push(b"Open https://auth.x.ai/device?user_code=AAAA-1111 now\n").unwrap();
    assert_eq!(later.challenge().unwrap().unwrap().code.as_deref(), Some("AAAA-1111"));
    later.push(b"Then enter this code: BBBB-2222\n").unwrap();
    assert!(later.challenge().is_err());
}

#[tokio::test]
async fn programa_sem_nome_de_grok_nao_serve_e_login_antigo_que_nao_sai_trava_o_novo() {
    let f = Fixture::new("plain");
    std::fs::write(f.control.join("version"), "").unwrap();
    let account = f.create("grok-unnamed").await;
    let op = f.start(&account, "login", "grok-unnamed-login").await;
    let failed = tokio::time::timeout(WAIT, async {
        loop {
            let value = f.read(&op, json!({"action":"read"}));
            if value["phase"] == "failed" {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(failed["errorCode"], "grok_not_installed");
    std::fs::remove_file(f.control.join("version")).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let first = f.start(&f.account(&account["id"]), "login", "grok-stuck-1").await;
        f.phase(&first, "awaiting_authorization").await;
        f.decide("approve");
        f.phase(&first, "connected").await;
        let home = f.home(&account["id"]);
        let locked = home.join(".grok");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
        let blocked = std::fs::remove_file(locked.join("auth.json")).is_err();
        if blocked {
            let second = f.start(&f.account(&account["id"]), "login", "grok-stuck-2").await;
            let failed = tokio::time::timeout(WAIT, async {
                loop {
                    let value = f.read(&second, json!({"action":"read"}));
                    if ["failed", "connected"].iter().any(|phase| value["phase"] == *phase) {
                        return value;
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            })
            .await
            .unwrap();
            assert_eq!(failed["errorCode"], "credential_not_erased");
            assert_eq!(f.home(&account["id"]), home, "a conta continua apontando para o login anterior");
        }
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
}
