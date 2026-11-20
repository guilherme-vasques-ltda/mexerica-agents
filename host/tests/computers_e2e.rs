//! Planilha pedida no chat chega ao computador escolhido: Hub real (processo próprio) + executores de referência
//! descartáveis (processos próprios, pastas sintéticas). O modelo é roteirizado: a chamada da ferramenta sai do teste,
//! pela mesma API local que a ponte MCP usa; todo o resto (Hub, canal E2E, cadastro, executor, disco) é real.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// Os testes dependem dos desvios de Área de Trabalho e pasta pessoal que só existem em build de depuração: em release
// eles tocariam a Área de Trabalho de verdade, então a suíte nem compila.
#![cfg(debug_assertions)]

mod common;

use serde_json::{Value, json};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Hub {
    child: Child,
    base: String,
    token: String,
    home: PathBuf,
    work: PathBuf,
    bot: String,
}
impl Drop for Hub {
    fn drop(&mut self) {
        common::stop(&mut self.child);
        let _ = std::fs::remove_dir_all(&self.home);
        let _ = std::fs::remove_dir_all(&self.work);
    }
}
impl Hub {
    async fn call(&self, method: &str, body: Value) -> Value {
        let res = reqwest::Client::new()
            .post(format!("{}/api/{method}", self.base))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = res.status();
        let v: Value = res.json().await.unwrap();
        assert!(status.is_success(), "{method} failed: {v}");
        v
    }
    fn binding(&self) -> Option<String> {
        let db = rusqlite::Connection::open(self.home.join("codync.db")).ok()?;
        db.query_row(
            "SELECT id FROM donna_session_bindings WHERE bot=?1 AND revoked_at IS NULL ORDER BY rowid DESC LIMIT 1",
            [&self.bot],
            |r| r.get(0),
        )
        .ok()
    }
    /// A ferramenta como o bot a chama: (isError, texto).
    async fn tool(&self, name: &str, arguments: Value) -> (bool, String) {
        let v = self
            .call(
                "computersCall",
                json!({"botId":self.bot,"name":name,"arguments":arguments,"sessionBinding":self.binding()}),
            )
            .await;
        (v["isError"] == true, v["content"][0]["text"].as_str().unwrap_or_default().to_owned())
    }
    async fn computers(&self) -> Vec<Value> {
        self.call("computersList", json!({})).await["computers"].as_array().unwrap().clone()
    }
    async fn wait_online(&self, name: &str) -> String {
        let end = Instant::now() + Duration::from_secs(40);
        loop {
            if let Some(c) = self
                .computers()
                .await
                .iter()
                .find(|c| c["name"] == name && c["state"] == "online" && c["capabilities"]["planilha"] == true)
            {
                return c["id"].as_str().unwrap().to_owned();
            }
            assert!(Instant::now() < end, "{name} não ficou online: {:?}", self.computers().await);
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}

async fn start_hub() -> Hub {
    let home = std::env::temp_dir().join(format!("mexerica-computers-e2e-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&home).unwrap();
    let (child, base) = common::serve(&home).await;
    let token = std::fs::read_to_string(home.join("token")).unwrap().trim().to_owned();
    let work = std::env::temp_dir().join(format!("mexerica-computers-work-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&work).unwrap();
    let mut hub = Hub { child, base, token, home: home.clone(), work: work.clone(), bot: String::new() };
    let agent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fake_agent_hold.py");
    let cwd = work;
    let bot = hub
        .call("createBot", json!({"name":"Berga de teste","backend":"custom","command":common::python_agent(&agent),"cwd":cwd,"permission":"ask"}))
        .await["bot"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    hub.bot = bot;
    hub.call("computerBots", json!({"bots":[hub.bot]})).await;
    hub.call("send", json!({"botId":hub.bot,"text":"crie uma pasta X na minha Área de Trabalho e salve ali a planilha","clientNonce":"n1"})).await;
    let end = Instant::now() + Duration::from_secs(30);
    while hub.binding().is_none() {
        assert!(Instant::now() < end, "turno não começou");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    hub
}

/// Um computador descartável: pasta pessoal, Área de Trabalho e estado do executor sintéticos.
struct Pc {
    state: PathBuf,
    user_home: PathBuf,
    desktop: PathBuf,
    run: Option<Child>,
}
impl Drop for Pc {
    fn drop(&mut self) {
        if let Some(c) = self.run.as_mut() {
            common::stop(c);
        }
        let _ = std::fs::remove_dir_all(self.state.parent().unwrap());
    }
}
impl Pc {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!("mexerica-pc-{}", uuid::Uuid::new_v4()));
        let (state, user_home, desktop) = (base.join("state"), base.join("home"), base.join("home/Desktop"));
        std::fs::create_dir_all(&desktop).unwrap();
        let user_home = user_home.canonicalize().unwrap();
        let desktop = desktop.canonicalize().unwrap();
        Self { state, user_home, desktop, run: None }
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_donnabot-host"));
        c.arg("executor").args(args).arg("--home").arg(&self.state);
        c.env("MEXERICA_EXECUTOR_TEST_DESKTOP", &self.desktop).env("MEXERICA_EXECUTOR_TEST_HOME", &self.user_home);
        c.env("MEXERICA_EXECUTOR_TEST_PLATFORM", "macos");
        c
    }
    fn exec(&self, args: &[&str], stdin: &str) -> (bool, String) {
        let mut child =
            self.command(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        (
            out.status.success(),
            format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)),
        )
    }
    /// O Hub limita cadastros por origem (3 tentativas por minuto); os testes esperam a janela passar.
    async fn enroll_with(&self, hub: &Hub, code: &str, name: &str) -> (bool, String) {
        for _ in 0..12 {
            let (ok, out) = self.exec(&["enroll", "--hub", &hub.base, "--code", code, "--name", name], "");
            if !out.contains("rate_limited") && !out.contains("tentativas por minuto") {
                return (ok, out);
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
        panic!("limite de tentativas não liberou");
    }
    async fn enroll(&self, hub: &Hub, name: &str) -> (bool, String) {
        let code = hub.call("computerEnrollCode", json!({})).await["code"].as_str().unwrap().to_owned();
        self.enroll_with(hub, &code, name).await
    }
    fn authorize(&self) -> (bool, String) {
        self.exec(&["authorize", "--location", "desktop"], "AUTORIZAR\n")
    }
    fn start(&mut self) {
        self.start_with(None);
    }
    /// `crash`: ponto em que o executor se derruba sozinho (só em build de depuração), para provar a conferência pós-queda.
    fn start_with(&mut self, crash: Option<&str>) {
        let log = std::fs::File::options().create(true).append(true).open(self.state.join("run.log")).unwrap();
        let mut command = self.command(&["run"]);
        if let Some(point) = crash {
            command.env("MEXERICA_EXECUTOR_TEST_CRASH", point);
        }
        self.run = Some(command.stdin(Stdio::null()).stdout(log.try_clone().unwrap()).stderr(log).spawn().unwrap());
    }
    async fn wait_exit(&mut self) -> std::process::ExitStatus {
        let end = Instant::now() + Duration::from_secs(40);
        loop {
            if let Some(status) = self.run.as_mut().unwrap().try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < end, "o executor não caiu no ponto combinado: {}", self.log());
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    fn log(&self) -> String {
        std::fs::read_to_string(self.state.join("run.log")).unwrap_or_default()
    }
    async fn ready(hub: &Hub, name: &str) -> Self {
        let mut pc = Self::new();
        let (ok, out) = pc.enroll(hub, name).await;
        assert!(ok, "{out}");
        let (ok, out) = pc.authorize();
        assert!(ok, "{out}");
        pc.start();
        hub.wait_online(name).await;
        pc
    }
}

fn args(computer: Option<&str>, folder: Option<&str>, filename: &str) -> Value {
    let mut a = json!({"location":"desktop","filename":filename,"reason":"pedido do dono no chat",
        "sheets":[{"name":"Vendas","rows":[["Produto","Qtd","Preço"],["Caneta fictícia",12,3.5],["Caderno fictício",4,19.9],[null,true,"=1+1"]]}]});
    if let Some(c) = computer {
        a["computer"] = json!(c);
    }
    if let Some(f) = folder {
        a["folder"] = json!(f);
    }
    a
}
fn receipt(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|_| panic!("não é JSON de resultado: {text}"))
}
fn listing(dir: &Path) -> Vec<String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => panic!("não consegui listar {}: {e}", dir.display()),
    };
    let mut names: Vec<String> = entries.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

#[tokio::test]
async fn chat_request_creates_the_folder_and_the_spreadsheet_on_the_chosen_computer_and_never_overwrites() {
    let hub = start_hub().await;
    let pc = Pc::ready(&hub, "PC descartável A").await;

    let (error, text) = hub.tool("create_spreadsheet", args(None, Some("X"), "Vendas.xlsx")).await;
    assert!(!error, "{text}");
    let r = receipt(&text);
    assert_eq!((r["outcome"].clone(), r["decidedBy"].clone()), (json!("done"), json!("pasta liberada")));
    let file = pc.desktop.join("X").join("Vendas.xlsx");
    assert_eq!(r["receipt"]["path"], json!(file.display().to_string()));
    assert_eq!((r["receipt"]["folderCreated"].clone(), r["receipt"]["reopened"].clone()), (json!(true), json!(true)));
    let bytes = std::fs::read(&file).unwrap();
    assert_eq!(r["receipt"]["size"], json!(bytes.len()));
    if let Some(dir) = std::env::var_os("MEXERICA_E2E_EXPORT") {
        std::fs::write(Path::new(&dir).join("Vendas.xlsx"), &bytes).unwrap();
    }
    assert_eq!(listing(&pc.desktop), ["X"]);
    assert_eq!(listing(&pc.desktop.join("X")), ["Vendas.xlsx"]);
    assert!(pc.log().contains("criado e conferido"), "{}", pc.log());

    // Mesmo pedido outra vez: nada é sobrescrito, o arquivo fica intacto e o Hub não promete criação.
    let (error, text) = hub.tool("create_spreadsheet", args(None, Some("X"), "Vendas.xlsx")).await;
    assert!(error && text.starts_with("file_exists"), "{text}");
    assert!(!text.contains("sameContent"), "a colisão não lê o arquivo existente: {text}");
    assert_eq!(std::fs::read(&file).unwrap(), bytes);

    // Arquivo do dono com o mesmo nome: preservado byte a byte.
    std::fs::write(pc.desktop.join("X").join("Dono.xlsx"), b"trabalho do dono").unwrap();
    let (error, text) = hub.tool("create_spreadsheet", args(None, Some("X"), "Dono.xlsx")).await;
    assert!(error && text.contains("file_exists") && !text.contains("sameContent"), "{text}");
    assert_eq!(std::fs::read(pc.desktop.join("X").join("Dono.xlsx")).unwrap(), b"trabalho do dono");

    // O diário local chegou ao Hub com a cadeia íntegra.
    let rows = pc.exec(&["status"], "").1;
    assert!(
        rows.contains("já confirmados pelo Hub") && !rows.contains("(0 já confirmados"),
        "o Hub confirmou o diário: {rows}"
    );
}

#[tokio::test]
async fn two_computers_never_guess_and_a_disconnected_or_removed_one_gets_nothing() {
    let hub = start_hub().await;
    let mut a = Pc::ready(&hub, "PC descartável A").await;
    let b = Pc::ready(&hub, "PC descartável B").await;

    let (error, text) = hub.tool("create_spreadsheet", args(None, None, "Dois.xlsx")).await;
    assert!(error && text.contains("ambiguous_computer"), "{text}");
    assert!(listing(&a.desktop).is_empty() && listing(&b.desktop).is_empty());

    let (error, text) = hub.tool("create_spreadsheet", args(Some("PC descartável B"), None, "Dois.xlsx")).await;
    assert!(!error, "{text}");
    assert_eq!(listing(&b.desktop), ["Dois.xlsx"]);
    assert_eq!(listing(&a.desktop), Vec::<String>::new());

    let (error, text) = hub.tool("create_spreadsheet", args(Some("PC que não existe"), None, "Tres.xlsx")).await;
    assert!(error && text.contains("unknown_computer"), "{text}");

    // Desconectado: nada é enfileirado e não há troca silenciosa de computador.
    common::stop(a.run.as_mut().unwrap());
    let end = Instant::now() + Duration::from_secs(20);
    while hub.computers().await.iter().any(|c| c["name"] == "PC descartável A" && c["state"] == "online") {
        assert!(Instant::now() < end, "A continua online");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let (error, text) = hub.tool("create_spreadsheet", args(Some("PC descartável A"), None, "Offline.xlsx")).await;
    assert!(error && text.contains("nada foi executado"), "{text}");
    assert_eq!(listing(&a.desktop), Vec::<String>::new());

    // Revogação no Hub: o executor de B para sozinho e B some das escolhas.
    let id = hub.computers().await.iter().find(|c| c["name"] == "PC descartável B").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    hub.call("computerRemove", json!({"computerId":id})).await;
    let end = Instant::now() + Duration::from_secs(40);
    let mut b = b;
    loop {
        if let Some(status) = b.run.as_mut().unwrap().try_wait().unwrap() {
            assert!(!status.success());
            break;
        }
        assert!(Instant::now() < end, "executor de B não parou: {}", b.log());
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let (error, text) = hub.tool("create_spreadsheet", args(Some("PC descartável B"), None, "Revogado.xlsx")).await;
    assert!(error && text.contains("unknown_computer"), "{text}");
    assert_eq!(listing(&b.desktop), ["Dois.xlsx"]);
}

#[tokio::test]
async fn the_computer_enforces_its_own_limits_not_the_prompt() {
    let hub = start_hub().await;
    let pc = Pc::ready(&hub, "PC descartável A").await;

    // Caminho e nomes perigosos nunca chegam a ser aceitos pelo Hub; nada é criado.
    for (folder, file) in [
        (Some("../fora"), "a.xlsx"),
        (Some("a/b"), "a.xlsx"),
        (None, "../a.xlsx"),
        (None, "a.xlsx.exe"),
        (Some("AppData"), "../x.xlsx"),
    ] {
        let (error, text) = hub.tool("create_spreadsheet", args(None, folder, file)).await;
        assert!(error && text.contains("invalid_arguments"), "{folder:?} {file}: {text}");
    }
    assert_eq!(listing(&pc.desktop), Vec::<String>::new());
    // A pasta liberada trocada por OUTRA pasta comum no mesmo caminho (outro inode) não herda a liberação.
    let old = pc.desktop.with_file_name("Desktop.antiga");
    std::fs::rename(&pc.desktop, &old).unwrap();
    std::fs::create_dir(&pc.desktop).unwrap();
    let (error, text) = hub.tool("create_spreadsheet", args(None, Some("X"), "Troca.xlsx")).await;
    assert!(error && text.contains("location_changed"), "{text}");
    assert_eq!(listing(&pc.desktop), Vec::<String>::new(), "nada pode ter sido criado na pasta nova");
    std::fs::remove_dir(&pc.desktop).unwrap();
    std::fs::rename(&old, &pc.desktop).unwrap();

    // O dono tira a liberação no próprio PC; o Hub ainda tem a capacidade antiga por até 25 s e envia o pedido.
    let (ok, out) = pc.exec(&["unauthorize", "--location", "desktop"], "");
    assert!(ok, "{out}");
    let (error, text) = hub.tool("create_spreadsheet", args(None, Some("X"), "Vendas.xlsx")).await;
    assert!(error && text.contains("location_not_authorized"), "{text}");
    assert_eq!(listing(&pc.desktop), Vec::<String>::new(), "nada pode ter sido criado");
    // Depois do próximo batimento o Hub recusa antes de enfileirar.
    let end = Instant::now() + Duration::from_secs(40);
    loop {
        let (error, text) = hub.tool("create_spreadsheet", args(None, None, "Vendas.xlsx")).await;
        if error && text.contains("capability_missing") {
            break;
        }
        assert!(Instant::now() < end, "capacidade não foi retirada: {text}");
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

#[tokio::test]
async fn critical_places_cannot_be_authorized_and_codes_are_single_use() {
    let hub = start_hub().await;
    let mut pc = Pc::new();
    let (ok, out) = pc.enroll(&hub, "PC descartável A").await;
    assert!(ok, "{out}");
    for critical in ["Library", "AppData/Roaming", ".ssh"] {
        let dir = pc.user_home.join(critical);
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = pc.command(&["authorize", "--location", "desktop"]);
        c.env("MEXERICA_EXECUTOR_TEST_DESKTOP", &dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = c.spawn().unwrap();
        child.stdin.take().unwrap().write_all(b"AUTORIZAR\n").unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            !out.status.success() && String::from_utf8_lossy(&out.stderr).contains("crítica"),
            "{critical}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    // Sem digitar AUTORIZAR nada é liberado.
    let (ok, _) = pc.exec(&["authorize", "--location", "desktop"], "sim\n");
    assert!(!ok);
    let (_, status) = pc.exec(&["status"], "");
    assert!(status.contains("Nada liberado"), "{status}");
    // Liberada de verdade, a pasta passa a valer; se o Desktop mudar de lugar o executor recusa.
    assert!(pc.authorize().0);
    pc.start();
    hub.wait_online("PC descartável A").await;
    let moved = pc.user_home.join("Library");
    // Mesma pasta liberada continua aceita; uma pasta crítica nunca recebe nada.
    let (error, text) = hub.tool("create_spreadsheet", args(None, None, "Ok.xlsx")).await;
    assert!(!error, "{text}");
    assert_eq!([listing(&moved), listing(&pc.user_home.join(".ssh"))], [Vec::<String>::new(), Vec::<String>::new()]);

    // Código errado ou repetido não cadastra.
    let other = Pc::new();
    let (ok, out) = other.enroll_with(&hub, "AAAA-BBBB-CCCC-DDDD", "Intruso").await;
    assert!(!ok && out.contains("HTTP 403"), "{out}");
    let code = hub.call("computerEnrollCode", json!({})).await["code"].as_str().unwrap().to_owned();
    let (ok, out) = other.enroll_with(&hub, &code, "Segundo").await;
    assert!(ok, "{out}");
    let third = Pc::new();
    let (ok, _) = third.enroll_with(&hub, &code, "Terceiro").await;
    assert!(!ok, "o código é de uso único");
    assert_eq!(hub.computers().await.len(), 2);
}

fn job_id(text: &str) -> String {
    let at = text.find("\"jobId\":\"").unwrap_or_else(|| panic!("sem jobId: {text}")) + 9;
    text[at..].split('"').next().unwrap().to_owned()
}

/// Espera o trabalho sair de "desconhecido/em andamento" e devolve o resultado final.
async fn settled(hub: &Hub, id: &str) -> (bool, String) {
    for _ in 0..40 {
        let (error, text) = hub.tool("computer_job_status", json!({"jobId":id,"waitSeconds":2})).await;
        if !text.starts_with("unknown_outcome") && !text.contains("\"outcome\":\"running\"") {
            return (error, text);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    panic!("o trabalho {id} não se resolveu");
}

/// Pedido com o executor derrubado em \`crash\`; depois o executor volta (outro processo) e reconcilia.
/// Devolve: resposta da chamada original, repetição feita enquanto o resultado era incerto, e o que havia no disco na queda.
async fn crash_and_return(
    hub: &Hub,
    pc: &mut Pc,
    crash: &str,
    filename: &str,
) -> ((bool, String), (bool, String), Option<Vec<u8>>) {
    pc.start_with(Some(crash));
    hub.wait_online("PC descartável A").await;
    let request = args(None, Some("X"), filename);
    let first = hub.tool("create_spreadsheet", request.clone());
    let after = async {
        let status = pc.wait_exit().await;
        assert_eq!(status.code(), Some(86), "{}", pc.log());
        let on_disk = std::fs::read(pc.desktop.join("X").join(filename)).ok();
        // Resultado incerto: repetir o mesmo pedido é barrado (computador offline ou pedido pendente), não grava de novo.
        let repeat = hub.tool("create_spreadsheet", request).await;
        pc.start_with(None);
        (repeat, on_disk)
    };
    let (first, (repeat, on_disk)) = tokio::join!(first, after);
    (first, repeat, on_disk)
}

#[tokio::test]
async fn a_crash_after_writing_is_settled_by_checking_the_same_attempt_and_never_by_repeating_it() {
    let hub = start_hub().await;
    let mut pc = Pc::new();
    let (ok, out) = pc.enroll(&hub, "PC descartável A").await;
    assert!(ok, "{out}");
    let (ok, out) = pc.authorize();
    assert!(ok, "{out}");
    let x = pc.desktop.join("X");
    let file = |name: &str| x.join(name);

    // 1) Caiu logo depois de gravar, antes de guardar o resultado: o disco decide, não a repetição.
    let (first, repeat, on_disk) = crash_and_return(&hub, &mut pc, "after_write", "Vendas.xlsx").await;
    let written = on_disk.expect("o arquivo já estava gravado na queda");
    assert!(first.0 && first.1.starts_with("unknown_outcome"), "{first:?}");
    assert!(repeat.0 && (repeat.1.contains("offline") || repeat.1.contains("duplicate_pending")), "{repeat:?}");
    let (error, text) = settled(&hub, &job_id(&first.1)).await;
    assert!(!error, "{text}");
    let r = receipt(&text);
    assert_eq!((r["outcome"].clone(), r["decidedBy"].clone()), (json!("done"), json!("pasta liberada")));
    assert_eq!(r["receipt"]["path"], json!(file("Vendas.xlsx").display().to_string()));
    assert_eq!((r["receipt"]["created"].clone(), r["receipt"]["reopened"].clone()), (json!(true), json!(true)));
    assert_eq!(std::fs::read(file("Vendas.xlsx")).unwrap(), written, "o arquivo não pode ser regravado");
    assert_eq!(listing(&pc.desktop.join("X")), ["Vendas.xlsx"]);
    assert!(pc.exec(&["status"], "").1.contains("recovered"), "o diário local registra a conferência");

    // 2) Caiu depois de guardar o resultado e antes de entregá-lo: o mesmo resultado é reenviado.
    hub.wait_online("PC descartável A").await;
    common::stop(pc.run.as_mut().unwrap());
    let (first, repeat, on_disk) = crash_and_return(&hub, &mut pc, "after_outbox", "Entrega.xlsx").await;
    assert!(on_disk.is_some());
    assert!(first.0 && first.1.starts_with("unknown_outcome"), "{first:?}");
    assert!(repeat.0 && (repeat.1.contains("offline") || repeat.1.contains("duplicate_pending")), "{repeat:?}");
    let (error, text) = settled(&hub, &job_id(&first.1)).await;
    assert!(!error, "{text}");
    assert_eq!(receipt(&text)["outcome"], json!("done"));
    assert_eq!(listing(&pc.desktop.join("X")), ["Entrega.xlsx", "Vendas.xlsx"]);

    // 3) Caiu antes de gravar: o computador prova que nada foi criado e só então repetir é permitido.
    hub.wait_online("PC descartável A").await;
    common::stop(pc.run.as_mut().unwrap());
    let (first, _, on_disk) = crash_and_return(&hub, &mut pc, "before_write", "Antes.xlsx").await;
    assert!(on_disk.is_none());
    assert!(first.0 && first.1.starts_with("unknown_outcome"), "{first:?}");
    let (error, text) = settled(&hub, &job_id(&first.1)).await;
    assert!(error && text.starts_with("not_written"), "{text}");
    assert_eq!(listing(&pc.desktop.join("X")), ["Entrega.xlsx", "Vendas.xlsx"]);
    hub.wait_online("PC descartável A").await;
    let (error, text) = hub.tool("create_spreadsheet", args(None, Some("X"), "Antes.xlsx")).await;
    assert!(!error, "{text}");
    assert_eq!(listing(&pc.desktop.join("X")), ["Antes.xlsx", "Entrega.xlsx", "Vendas.xlsx"]);

    // 4) Caiu depois de marcar "gravando" e antes de criar: disco vazio não prova nada. Continua desconhecido e barrado.
    hub.wait_online("PC descartável A").await;
    common::stop(pc.run.as_mut().unwrap());
    let (first, _, on_disk) = crash_and_return(&hub, &mut pc, "before_create", "Duvida.xlsx").await;
    assert!(on_disk.is_none());
    assert!(first.0 && first.1.starts_with("unknown_outcome"), "{first:?}");
    hub.wait_online("PC descartável A").await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let (error, text) = hub.tool("computer_job_status", json!({"jobId":job_id(&first.1),"waitSeconds":1})).await;
    assert!(
        error && text.contains("\"outcome\":\"unknown_outcome\""),
        "sem prova, o resultado não vira falha nem sucesso: {text}"
    );
    assert!(text.contains("executor_restarted"), "{text}");
    let (error, text) = hub.tool("create_spreadsheet", args(None, Some("X"), "Duvida.xlsx")).await;
    assert!(error && text.contains("duplicate_pending"), "{text}");
    assert_eq!(listing(&pc.desktop.join("X")), ["Antes.xlsx", "Entrega.xlsx", "Vendas.xlsx"]);

    // O pedido que já teve resultado incerto não se repete nesta conversa: a consulta mostra o recibo, o arquivo fica intacto.
    let (error, text) = hub.tool("create_spreadsheet", args(None, Some("X"), "Vendas.xlsx")).await;
    assert!(error && text.contains("duplicate_pending"), "{text}");
    assert_eq!(std::fs::read(file("Vendas.xlsx")).unwrap(), written);
}

#[tokio::test]
async fn revoking_from_the_computer_removes_it_and_allows_enrolling_again() {
    let hub = start_hub().await;
    let mut pc = Pc::ready(&hub, "PC descartável A").await;
    let (error, text) = hub.tool("create_spreadsheet", args(None, Some("X"), "Antes.xlsx")).await;
    assert!(!error, "{text}");

    let (ok, out) = pc.exec(&["revoke"], "");
    assert!(ok, "{out}");
    let end = Instant::now() + Duration::from_secs(40);
    while hub.computers().await.iter().any(|c| c["name"] == "PC descartável A") {
        assert!(Instant::now() < end, "o Hub ainda lista o computador revogado");
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    pc.wait_exit().await;
    pc.run = None;
    assert!(!pc.state.join("hub.json").exists() && !pc.state.join("identity.key").exists());
    let (error, text) =
        hub.tool("create_spreadsheet", args(Some("PC descartável A"), Some("X"), "Revogado.xlsx")).await;
    assert!(error && text.contains("no_computers"), "{text}");

    // Mesmo estado local: o recadastro funciona e começa limpo.
    let (ok, out) = pc.enroll(&hub, "PC descartável A2").await;
    assert!(ok, "{out}");
    let (ok, out) = pc.authorize();
    assert!(ok, "{out}");
    pc.start();
    hub.wait_online("PC descartável A2").await;
    let (error, text) = hub.tool("create_spreadsheet", args(None, Some("X"), "Depois.xlsx")).await;
    assert!(!error, "{text}");
    assert_eq!(listing(&pc.desktop.join("X")), ["Antes.xlsx", "Depois.xlsx"]);
}
