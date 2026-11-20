//! MEX-1291 (F3): matriz por motor com CLIs falsos. Roda no CI sem conta real, rede ou modelo.
//!
//! Cobre: detecção, conferência de login sem cota ("Testar"), terminal de login, turno simples por ACP com a
//! skill entregue pelo harness (F4) e fallback de cota (F6). O que depende de código que ainda não existe em
//! `main` fica `#[ignore]` com o motivo, e cada teste pendente já traz o roteiro que vai valer.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
use common::host::Host;
use serde_json::{Value, json};
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Como o Hub lança o motor pelo caminho real (`backends::launch_candidates`).
enum Lancamento {
    /// O próprio CLI, no modo ACP do `local` de `backends.rs`: argumentos fixos.
    Cli(&'static [&'static str]),
    /// Sem modo ACP próprio: o adaptador do registro, `npx -y <pacote>`.
    Npx(&'static str),
}

struct Motor {
    /// Id do motor no Hub.
    id: &'static str,
    /// Binário que o Hub procura.
    binario: &'static str,
    /// Modo do agente ACP falso que imita o motor.
    modo: &'static str,
    lancamento: Lancamento,
}

const MOTORES: [Motor; 9] = [
    Motor { id: "claude", binario: "claude", modo: "claude", lancamento: Lancamento::Npx("fake-claude-acp@1.0.0") },
    Motor { id: "codex", binario: "codex", modo: "codex", lancamento: Lancamento::Npx("fake-codex-acp@1.0.0") },
    Motor { id: "grok", binario: "grok", modo: "grok", lancamento: Lancamento::Cli(&["agent", "stdio"]) },
    Motor { id: "cursor", binario: "cursor-agent", modo: "standard", lancamento: Lancamento::Cli(&["acp"]) },
    Motor {
        id: "antigravity",
        binario: "agy",
        modo: "standard",
        lancamento: Lancamento::Npx("fake-antigravity-acp@1.0.0"),
    },
    Motor { id: "opencode", binario: "opencode", modo: "standard", lancamento: Lancamento::Cli(&["acp"]) },
    Motor { id: "devin", binario: "devin", modo: "standard", lancamento: Lancamento::Cli(&["acp"]) },
    Motor { id: "kimi", binario: "kimi", modo: "standard", lancamento: Lancamento::Cli(&["acp"]) },
    Motor { id: "cline", binario: "cline", modo: "cline", lancamento: Lancamento::Cli(&["--acp"]) },
];

/// Motores com comando de conferência já ligado em `backends.rs` (`signed_in`).
const COM_CONFERENCIA: [&str; 3] = ["claude", "codex", "cursor"];

/// Registro ACP do teste: o Hub só lê este arquivo (o gancho de teste desliga a busca na rede). Motores sem modo
/// ACP próprio entram pelo `npx` falso, um pacote por motor.
const REGISTRO: &str = r#"{"agents":[
  {"id":"claude-acp","name":"Claude","distribution":{"npx":{"package":"fake-claude-acp@1.0.0"}}},
  {"id":"codex-acp","name":"Codex","distribution":{"npx":{"package":"fake-codex-acp@1.0.0"}}},
  {"id":"antigravity-acp","name":"Antigravity","distribution":{"npx":{"package":"fake-antigravity-acp@1.0.0"}}}
]}"#;

fn aspas(p: &Path) -> String {
    p.display().to_string().replace('\'', "'\\''")
}

fn python3() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|d| d.join("python3"))
        .find(|p| p.is_file())
        .expect("python3 no PATH do teste")
}

/// Tudo que o Hub do teste enxerga fica numa pasta temporária: CLIs falsos, HOME, XDG, TMPDIR e o shell.
/// Nenhum CLI real, shell de login real ou pasta do usuário entra.
struct Falsos {
    raiz: PathBuf,
    /// O único diretório do PATH do Hub: os CLIs falsos.
    pasta: PathBuf,
}

impl Falsos {
    fn nova() -> Self {
        // O gancho MEXERICA_TESTE_ISOLADO só vale com debug_assertions: em `--release` o Hub consultaria
        // caminhos, rede e o helper reais, então o teste se recusa a rodar em vez de passar sem isolamento.
        // `black_box`: a checagem é de execução (o clippy recusa assert sobre constante e if-panic).
        assert!(
            std::hint::black_box(cfg!(debug_assertions)),
            "motores_e2e exige build com debug_assertions (não rode com --release)"
        );
        let raiz = std::env::temp_dir().join(format!("mexerica-motores-{}", uuid::Uuid::new_v4()));
        for sub in ["bin", "home", "config", "data", "cache", "state", "tmp", "npm-cache"] {
            std::fs::create_dir_all(raiz.join(sub)).unwrap();
        }
        let falsos = Self { pasta: raiz.join("bin"), raiz };
        // `$SHELL -ilc <comando>` é como o Hub descobre o PATH do usuário: este shell roda só o comando, sem perfil.
        Self::escrever(&falsos.raiz.join("shell-falso"), "#!/bin/sh\nexec /bin/sh -c \"$2\"\n".to_owned());
        // `npm config get cache` (o Hub pergunta antes de usar o npx) e `npm exec` (preparar o pacote): nada de rede.
        let cache = aspas(&falsos.raiz.join("npm-cache"));
        Self::escrever(
            &falsos.pasta.join("npm"),
            format!("#!/bin/sh\n[ \"$1 $2 $3\" = 'config get cache' ] && printf '%s\\n' '{cache}'\nexit 0\n"),
        );
        falsos
    }

    fn escrever(caminho: &Path, conteudo: String) {
        std::fs::write(caminho, conteudo).unwrap();
        std::fs::set_permissions(caminho, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Instala um CLI falso: o script só chama `fixtures/motor_cli.py` com o nome do binário.
    fn instalar(&self, binario: &str) {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/motor_cli.py");
        let script = format!(
            "#!/bin/sh\nexport MEXERICA_FALSO_CAMINHO=\"$0\"\nexec '{}' -X utf8 -u '{}' '{}' '{binario}' \"$@\"\n",
            aspas(&python3()),
            aspas(&fixture),
            aspas(&self.pasta),
        );
        Self::escrever(&self.pasta.join(binario), script);
    }

    fn instalar_todos(&self) {
        for m in &MOTORES {
            self.instalar(m.binario);
        }
        self.instalar("npx");
    }

    fn cenario(&self, binario: &str, cenario: &str) {
        std::fs::write(self.pasta.join(format!("scenario-{binario}")), cenario).unwrap();
    }

    /// Ambiente do Hub (o resto é limpo por `env_clear`): HOME, PATH, SHELL, XDG_* e TMPDIR temporários, mais o gancho que fixa a busca de CLIs.
    fn env(&self) -> Vec<(&'static str, OsString)> {
        let em = |sub: &str| self.raiz.join(sub).into_os_string();
        vec![
            ("HOME", em("home")),
            ("PATH", self.pasta.clone().into_os_string()),
            ("SHELL", em("shell-falso")),
            ("XDG_CONFIG_HOME", em("config")),
            ("XDG_DATA_HOME", em("data")),
            ("XDG_CACHE_HOME", em("cache")),
            ("XDG_STATE_HOME", em("state")),
            ("TMPDIR", em("tmp")),
            ("MEXERICA_TESTE_ISOLADO", "1".into()),
        ]
    }

    async fn hub(&self) -> Host {
        let host = Host::start_isolado(&self.env()).await;
        std::fs::write(host.home.join("registry.json"), REGISTRO).unwrap();
        host
    }
}

impl Drop for Falsos {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.raiz);
    }
}

async fn atualizar(host: &Host) -> Value {
    host.call("refreshBackends", json!({})).await
}

fn motor<'a>(lista: &'a Value, id: &str) -> &'a Value {
    lista["backends"].as_array().unwrap().iter().find(|b| b["id"] == id).unwrap_or_else(|| panic!("sem {id}"))
}

fn workspace(host: &Host, bot: &str) -> PathBuf {
    host.home.join("bots").join(bot).join("workspace")
}

async fn criar_bot(host: &Host, m: &Motor) -> String {
    let bot = json!({"name": m.id, "backend": m.id, "connectors": [], "notify": false});
    host.call("createBot", bot).await["bot"]["id"].as_str().unwrap().to_owned()
}

/// Prova que o Hub lançou este bot pelo caminho real do motor: o binário certo (pelo caminho do falso), com os
/// argumentos e o modo do motor. O falso grava isto na pasta de trabalho do bot ao virar o agente ACP.
fn confere_lancamento(host: &Host, falsos: &Falsos, m: &Motor, bot: &str) {
    let arquivo = workspace(host, bot).join("lancado-por.json");
    let lido = std::fs::read_to_string(arquivo);
    let lido = lido.unwrap_or_else(|e| panic!("{}: não foi lançado pelo falso: {e}", m.id));
    let lancado: Value = serde_json::from_str(&lido).unwrap();
    let (binario, args) = match m.lancamento {
        Lancamento::Cli(args) => (m.binario, json!(args)),
        Lancamento::Npx(pacote) => ("npx", json!(["-y", pacote])),
    };
    assert_eq!(lancado["binario"], binario, "{}: binário lançado: {lancado}", m.id);
    assert_eq!(lancado["args"], args, "{}: argumentos lançados: {lancado}", m.id);
    assert_eq!(lancado["modo"], m.modo, "{}: modo lançado: {lancado}", m.id);
    assert_eq!(
        lancado["caminho"],
        falsos.pasta.join(binario).display().to_string(),
        "{}: o Hub lançou outro executável: {lancado}",
        m.id
    );
}

// ---------------------------------------------------------------------------------------------------------
// 1. Detecção: instalado e não instalado
// ---------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn deteccao_acha_cada_motor_instalado() {
    let falsos = Falsos::nova();
    falsos.instalar_todos();
    let host = falsos.hub().await;
    let lista = atualizar(&host).await;
    for m in &MOTORES {
        let achado = motor(&lista, m.id);
        assert_eq!(achado["installed"], true, "{}: {achado}", m.id);
        assert_eq!(achado["path"], falsos.pasta.join(m.binario).display().to_string(), "{}: caminho do falso", m.id);
    }
}

#[tokio::test]
async fn deteccao_nao_acha_motor_sem_binario_e_acompanha_a_remocao() {
    let falsos = Falsos::nova();
    // Só Claude e Codex instalados; o resto não. O PATH do Hub é só a pasta dos falsos, então a ausência vale em
    // qualquer máquina e nenhum caso é pulado.
    falsos.instalar("claude");
    falsos.instalar("codex");
    let host = falsos.hub().await;
    let lista = atualizar(&host).await;
    assert_eq!(motor(&lista, "claude")["installed"], true);
    assert_eq!(motor(&lista, "codex")["installed"], true);
    let ausentes: Vec<&Motor> = MOTORES.iter().filter(|m| !["claude", "codex"].contains(&m.id)).collect();
    assert_eq!(ausentes.len(), 7);
    for m in ausentes {
        let achado = motor(&lista, m.id);
        assert_eq!(achado["installed"], false, "{}: {achado}", m.id);
        assert!(achado["path"].is_null(), "{}: {achado}", m.id);
    }
    // Desinstalar muda a resposta na próxima atualização.
    std::fs::remove_file(falsos.pasta.join("codex")).unwrap();
    let depois = atualizar(&host).await;
    assert_eq!(motor(&depois, "codex")["installed"], false);
    assert!(motor(&depois, "codex")["path"].is_null());
    assert_eq!(motor(&depois, "claude")["installed"], true);
    // O Cursor também se chama `agent`: o apelido conta como instalado, com o caminho do apelido.
    falsos.instalar("agent");
    let com_apelido = atualizar(&host).await;
    assert_eq!(motor(&com_apelido, "cursor")["installed"], true);
    assert_eq!(motor(&com_apelido, "cursor")["path"], falsos.pasta.join("agent").display().to_string());
}

// ---------------------------------------------------------------------------------------------------------
// 2. Conferência de login sem cota ("Testar")
// ---------------------------------------------------------------------------------------------------------

fn conferir(lista: &Value, binario_por_id: &[(&str, &str)], esperado: &Value, motivo: &str) {
    for (id, _) in binario_por_id {
        assert_eq!(&motor(lista, id)["signedIn"], esperado, "{id}: {motivo}");
    }
}

fn com_conferencia() -> Vec<(&'static str, &'static str)> {
    MOTORES.iter().filter(|m| COM_CONFERENCIA.contains(&m.id)).map(|m| (m.id, m.binario)).collect()
}

#[tokio::test]
async fn testar_logado_e_deslogado() {
    let falsos = Falsos::nova();
    falsos.instalar_todos();
    let host = falsos.hub().await;
    let motores = com_conferencia();
    for (_, binario) in &motores {
        falsos.cenario(binario, "logged_in");
    }
    conferir(&atualizar(&host).await, &motores, &json!(true), "logado");
    for (_, binario) in &motores {
        falsos.cenario(binario, "logged_out");
    }
    conferir(&atualizar(&host).await, &motores, &json!(false), "deslogado");
}

#[tokio::test]
async fn testar_resposta_desconhecida_nunca_vira_logado() {
    let falsos = Falsos::nova();
    falsos.instalar_todos();
    let host = falsos.hub().await;
    let motores = com_conferencia();
    for (_, binario) in &motores {
        falsos.cenario(binario, "unknown");
    }
    let lista = atualizar(&host).await;
    for (id, _) in &motores {
        assert_ne!(motor(&lista, id)["signedIn"], true, "{id}: resposta desconhecida não prova login");
    }
}

#[tokio::test]
async fn testar_timeout_fica_sem_resposta() {
    let falsos = Falsos::nova();
    falsos.instalar_todos();
    let host = falsos.hub().await;
    let motores = com_conferencia();
    for (_, binario) in &motores {
        falsos.cenario(binario, "timeout");
    }
    // O Hub desiste da conferência em 15 s e não chuta: sem resposta, "não confirmado" (null).
    conferir(&atualizar(&host).await, &motores, &Value::Null, "timeout");
}

#[tokio::test]
async fn motores_sem_comando_de_conferencia_ficam_nao_confirmados() {
    let falsos = Falsos::nova();
    falsos.instalar_todos();
    let host = falsos.hub().await;
    let lista = atualizar(&host).await;
    for m in MOTORES.iter().filter(|m| ["devin", "kimi", "cline"].contains(&m.id)) {
        assert!(motor(&lista, m.id)["signedIn"].is_null(), "{}: sem comando provado, nunca diz logado", m.id);
    }
}

#[tokio::test]
#[ignore = "F3a: grok, agy e opencode ainda têm `signed_in: None` em backends.rs (comandos `models`/`auth list` a provar)"]
async fn testar_logado_e_deslogado_em_grok_agy_e_opencode() {
    let falsos = Falsos::nova();
    falsos.instalar_todos();
    let host = falsos.hub().await;
    let motores = [("grok", "grok"), ("antigravity", "agy"), ("opencode", "opencode")];
    conferir(&atualizar(&host).await, &motores, &json!(true), "logado");
    for (_, binario) in motores {
        falsos.cenario(binario, "logged_out");
    }
    conferir(&atualizar(&host).await, &motores, &json!(false), "deslogado");
}

#[tokio::test]
#[ignore = "F3a: hoje claude/cursor leem resposta desconhecida como deslogado e codex só olha o código de saída; o plano pede 'Não confirmada' (null)"]
async fn testar_resposta_desconhecida_fica_nao_confirmada() {
    let falsos = Falsos::nova();
    falsos.instalar_todos();
    let host = falsos.hub().await;
    let motores = com_conferencia();
    for (_, binario) in &motores {
        falsos.cenario(binario, "unknown");
    }
    conferir(&atualizar(&host).await, &motores, &Value::Null, "desconhecida");
}

// ---------------------------------------------------------------------------------------------------------
// 3. Terminal de login (§3.1 do plano). O terminal novo do PR #827 ainda não está em main: pendente.
// ---------------------------------------------------------------------------------------------------------

const MOTIVO_TERMINAL: &str =
    "terminal de login (§3.1, F3j/#827) ainda não existe em main; o `term.rs` atual abre shell";

/// Sessão do terminal de login do Hub. Quando o terminal existir, só estas funções mudam.
struct TerminoDeLogin;

fn abrir_login(_hub: &Host, _id: &str) -> TerminoDeLogin {
    panic!("{MOTIVO_TERMINAL}");
}

fn enviar_login(_terminal: &TerminoDeLogin, _texto: &str) {
    panic!("{MOTIVO_TERMINAL}");
}

fn ler_login(_terminal: &TerminoDeLogin) -> String {
    panic!("{MOTIVO_TERMINAL}");
}

fn login_fechou(_terminal: &TerminoDeLogin) -> bool {
    panic!("{MOTIVO_TERMINAL}");
}

#[tokio::test]
#[ignore = "terminal de login (§3.1, F3j/#827) ainda não existe em main"]
async fn login_pede_codigo_recebe_e_fecha_a_sessao() {
    let falsos = Falsos::nova();
    falsos.instalar_todos();
    let host = falsos.hub().await;
    for m in &MOTORES {
        let (id, binario) = (m.id, m.binario);
        falsos.cenario(binario, "logged_out");
        let terminal = abrir_login(&host, id);
        assert!(ler_login(&terminal).contains("digite o código"), "{id}");
        enviar_login(&terminal, "CODIGO-OK\n");
        assert!(ler_login(&terminal).contains("Login concluído"), "{id}");
        assert!(login_fechou(&terminal), "{id}: a sessão fecha quando o login termina");
        // O pai do CLI não é um shell: execução direta, argumentos fixos do Hub.
        // Sem como identificar o pai (nem /proc nem `ps`), o falso grava DESCONHECIDO e o teste falha: não presume.
        let pai = std::fs::read_to_string(falsos.pasta.join(format!("parent-{binario}"))).unwrap();
        let pai = pai.trim();
        assert_ne!(pai, "DESCONHECIDO", "{id}: não deu para identificar o processo pai; verificação inconclusiva");
        assert!(!["sh", "bash", "zsh", "dash", "fish"].contains(&pai), "{id}: pai {pai}");
    }
}

#[tokio::test]
#[ignore = "terminal de login (§3.1, F3j/#827) ainda não existe em main"]
async fn login_tentativas_de_escape_nao_chegam_a_shell() {
    let falsos = Falsos::nova();
    falsos.instalar_todos();
    let host = falsos.hub().await;
    let marca = falsos.pasta.join("PWNED");
    let marca = marca.display();
    let tentativas = [
        "!ls\n".to_owned(),
        "/shell\n".to_owned(),
        "\u{1a}".to_owned(),
        format!("$(touch {marca})\n"),
        format!("`touch {marca}`\n"),
        format!("; touch {marca}\n"),
    ];
    for m in &MOTORES {
        let (id, binario) = (m.id, m.binario);
        falsos.cenario(binario, "logged_out");
        let terminal = abrir_login(&host, id);
        for tentativa in &tentativas {
            enviar_login(&terminal, tentativa);
        }
        assert!(!falsos.pasta.join("PWNED").exists(), "{id}: algum comando rodou");
        // O CLI falso só recebe o texto como entrada; nada vira comando.
        let recebido = std::fs::read_to_string(falsos.pasta.join(format!("inputs-{binario}.log"))).unwrap_or_default();
        assert!(recebido.contains("!ls") && recebido.contains("/shell"), "{id}: entrada chega ao CLI como texto");
        assert!(!login_fechou(&terminal), "{id}: escape não encerra nem abre outra coisa");
        enviar_login(&terminal, "CODIGO-OK\n");
        assert!(login_fechou(&terminal), "{id}");
    }
}

// ---------------------------------------------------------------------------------------------------------
// 4. Turno simples por ACP respondendo "ok", com a skill entregue pelo harness (F4)
// ---------------------------------------------------------------------------------------------------------

/// O manual do harness, onde cada tipo de agente o recebe.
fn instrucoes(host: &Host, bot: &str) -> String {
    let nova = &host.requests(bot, "session/new")[0]["params"];
    let inicio = &host.requests(bot, "initialize")[0];
    [
        nova["_meta"]["systemPrompt"]["append"].as_str(),
        nova["systemPrompt"]["content"][0]["text"].as_str(),
        inicio["codexConfig"]["developer_instructions"].as_str(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("\n")
}

fn respondeu_ok(historico: &Value) -> bool {
    historico["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "agent" && e["data"]["final"] == true && e["data"]["text"] == "ok")
}

const MANUAL_DO_HARNESS: &str = "nunca use agendamento nativo";

/// Sem `command` no bot: o Hub escolhe o executável sozinho (`launch_commands` -> `launch_candidates`), e o PATH
/// isolado só tem os CLIs falsos. Cada motor tem de ser lançado pelo seu binário e modo.
#[tokio::test]
async fn turno_simples_responde_ok_pelo_lancamento_real_de_cada_motor() {
    let falsos = Falsos::nova();
    falsos.instalar_todos();
    let host = falsos.hub().await;
    for m in &MOTORES {
        let bot = criar_bot(&host, m).await;
        host.reply(&bot, "Responda ok").await;
        let historico = host.call("history", json!({"botId": bot})).await;
        assert!(respondeu_ok(&historico), "{}: {historico}", m.id);
        confere_lancamento(&host, &falsos, m, &bot);
        // O harness entrega as instruções (e as skills) pelo próprio canal, igual em qualquer motor.
        let manual = instrucoes(&host, &bot);
        assert!(manual.contains(MANUAL_DO_HARNESS), "{}: manual do harness ausente: {manual}", m.id);
    }
}

/// F4 — NÃO é prova. O isolamento real de skills por motor ainda não existe; este teste fica ignorado e sem
/// asserções para não parecer cobertura. Roteiro (a fazer na F4, MEX-1292):
/// - modelar, para cada motor, as fontes e configurações que ele realmente lê (ex.: `CLAUDE_CONFIG_DIR`,
///   `CODEX_HOME`, `~/.claude/skills`, `~/.agents/skills`, `.claude/skills` do projeto);
/// - plantar uma skill sentinela nativa em cada fonte e fazer o CLI falso registrar a leitura efetiva do
///   conteúdo do `SKILL.md`;
/// - plantar também a skill entregue pelo harness e provar que o motor a recebe;
/// - provar que nenhuma sentinela nativa foi lida por nenhum dos nove motores.
#[tokio::test]
#[ignore = "a definir na F4: isolamento real por motor ainda não existe"]
async fn turno_nao_le_skills_nativas_do_usuario_nem_do_projeto() {
    // TODO(MEX-1292): implementar o roteiro acima quando o isolamento por motor existir.
    unimplemented!("F4 (MEX-1292): isolamento real por motor ainda não existe");
}

// ---------------------------------------------------------------------------------------------------------
// 5. Fallback F6 (MEX-1294 fatia A e MEX-1391 fatia B) com cota simulada
// ---------------------------------------------------------------------------------------------------------

/// Uma conta Claude gerenciada: a pasta de configuração (`dir`) decide, no CLI falso, a identidade, o uso
/// (`uso-sessao`, `uso-semana`) e a cota do turno (`cota-esgotada`).
struct Conta {
    id: String,
    dir: PathBuf,
    json: Value,
}

/// A mesma impressão do registro de contas (`Registry::fingerprint`): sal, motor, espaço e identidade.
fn impressao(sal: &str, motor: &str, espaco: &str, identidade: &str) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    let mut hash = Sha256::new();
    for parte in [sal, motor, espaco, identidade] {
        hash.update(parte.as_bytes());
        hash.update([0]);
    }
    let mut id = String::from("ac_");
    for byte in hash.finalize() {
        let _ = write!(id, "{byte:02x}");
    }
    id
}

fn conta_gerenciada(host: &Host, falsos: &Falsos, sal: &str, letra: char, billing: &str) -> Conta {
    let identidade = format!("{letra}@example.invalid\0org");
    let id = impressao(sal, "claude", "account", &identidade);
    let quota = impressao(sal, "claude", "quota", &identidade);
    let dir = host.home.join("engine-setup").join("accounts").join(&id).join(letra.to_string());
    std::fs::create_dir_all(&dir).unwrap();
    let executavel = falsos.pasta.join("claude").display().to_string();
    let launch = json!({"kind":"cli_profile", "executable": executavel, "directory": dir});
    let proof = json!({"method":"claude_auth_status", "plan":"max"});
    let json = json!({"id":id,"backend":"claude","billing":billing,"state":"connected","launch":launch,"scope":"owner",
        "quotaIdentity":quota,"verifiedAt":1,"subscriptionProof":proof,"models":[{"id":"haiku","options":[]}],
        "managed":{"label":format!("Conta {letra}"),"revision":1,"generation":0,"enabled":true,
            "requestId":"fixture","principal":"fixture","profile":null,"setupRequest":null,
            "catalog":{"accountId":id,"principal":"fixture","launch":launch,"revision":1,"subscription":proof}}});
    Conta { id, dir, json }
}

fn registrar_contas(host: &Host, sal: &str, contas: &[&Conta]) {
    let lista: Vec<&Value> = contas.iter().map(|c| &c.json).collect();
    let registro = json!({"salt": sal, "accounts": lista});
    let db = rusqlite::Connection::open(host.home.join("codync.db")).unwrap();
    db.execute("INSERT OR REPLACE INTO kv(k,v) VALUES('fork.engine-accounts.v1',?1)", [registro.to_string()]).unwrap();
}

/// O Hub, os CLIs falsos e o sal do registro de contas. TODOS os motores falsos ficam instalados: uma tentativa de
/// outro motor deixaria registro em `launches.jsonl`.
struct Ambiente {
    falsos: Falsos,
    host: Host,
    sal: String,
}

async fn ambiente() -> Ambiente {
    let falsos = Falsos::nova();
    falsos.instalar_todos();
    falsos.instalar("claude-agent-acp");
    let host = falsos.hub().await;
    Ambiente { falsos, host, sal: uuid::Uuid::new_v4().to_string() }
}

impl Ambiente {
    fn conta(&self, letra: char) -> Conta {
        conta_gerenciada(&self.host, &self.falsos, &self.sal, letra, "subscription")
    }

    async fn bot_na(&self, conta: &Conta) -> String {
        let criar = json!({"name":"claude","backend":"claude","connectors":[],"notify":false,
            "model":"haiku","forkEditor":{"engineAccountId":conta.id}});
        self.host.call("createBot", criar).await["bot"]["id"].as_str().unwrap().to_owned()
    }

    /// A ordem de reserva só deste bot (a conta do bot é o passo 0). Cada reserva precisa de vínculo de verdade com o
    /// Space (um bot dele a usa): aqui um bot auxiliar por reserva faz esse papel.
    async fn reserva_do_bot(&self, bot: &str, reservas: &[&Conta]) -> Value {
        for conta in reservas {
            self.bot_na(conta).await;
        }
        let ids: Vec<&str> = reservas.iter().map(|c| c.id.as_str()).collect();
        self.host.call("donnaEngineFallbackSave", json!({"backend":"claude","botId":bot,"accounts":ids})).await
    }

    async fn enviar(&self, bot: &str) {
        let envio = json!({"botId": bot, "text": "Responda ok", "clientNonce": uuid::Uuid::new_v4().to_string(),
            "contentOrigin": common::host::typed_origin("Responda ok")});
        self.host.call("send", envio).await;
    }

    async fn estado(&self) -> Value {
        self.host.call("donnaEngineFallback", json!({})).await
    }
}

fn usos_de_cota(host: &Host) -> Vec<Value> {
    let texto = std::fs::read_to_string(host.home.join("cota-uso.jsonl")).unwrap_or_default();
    texto.lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

/// Quem foi chamado e o que aconteceu, na ordem: `[("a","cota_esgotada"),("b","ok")]`.
fn resumo(usos: &[Value]) -> Vec<(String, String)> {
    usos.iter()
        .map(|u| (u["conta"].as_str().unwrap().to_owned(), u["resultado"].as_str().unwrap().to_owned()))
        .collect()
}

fn par(conta: &str, resultado: &str) -> (String, String) {
    (conta.to_owned(), resultado.to_owned())
}

async fn esperar_aviso(host: &Host, bot: &str, trecho: &str) -> Value {
    let limite = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let historico = host.call("history", json!({"botId": bot})).await;
        if historico.to_string().contains(trecho) {
            return historico;
        }
        assert!(std::time::Instant::now() < limite, "sem aviso {trecho:?}: {historico}");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

/// O Hub não pode ter chamado nenhuma chave de API paga em nenhuma tentativa.
fn sem_api_paga(usos: &[Value]) {
    assert!(usos.iter().all(|u| u["chave_api_paga"] == false), "caiu em API paga: {usos:?}");
}

#[tokio::test]
async fn cota_esgotada_passa_para_a_outra_conta_do_mesmo_motor_com_aviso_e_volta() {
    let amb = ambiente().await;
    let (a, b) = (amb.conta('a'), amb.conta('b'));
    std::fs::write(a.dir.join("cota-esgotada"), "").unwrap();
    registrar_contas(&amb.host, &amb.sal, &[&a, &b]);
    let bot = amb.bot_na(&a).await;
    amb.reserva_do_bot(&bot, &[&b]).await;

    // Sem leitura de uso só valem esgotada e erro: a conta A é tentada, esgota, e o pedido sai na B.
    amb.host.reply(&bot, "Responda ok").await;
    let historico = amb.host.call("history", json!({"botId": bot})).await;
    assert!(respondeu_ok(&historico), "a segunda conta responde: {historico}");
    assert!(historico.to_string().contains("Passei para Conta b porque Conta a acabou"), "{historico}");
    let usos = usos_de_cota(&amb.host);
    assert_eq!(resumo(&usos), [par("a", "cota_esgotada"), par("b", "ok")], "ordem da lista do dono: {usos:?}");
    sem_api_paga(&usos);

    // A conta esgotada descansa, e a conversa roda na reserva até ela voltar.
    let estado = amb.estado().await;
    let descanso = estado["resting"].as_array().unwrap().iter().find(|r| r["engineAccountId"] == json!(a.id)).unwrap();
    assert_eq!(descanso["reason"], "esgotada", "{estado}");
    assert_eq!(estado["sessions"][0]["currentAccountId"], json!(b.id), "{estado}");
    assert_eq!(estado["switches"].as_array().unwrap().len(), 1, "um cartão por troca: {estado}");

    // Voltar agora encerra o descanso: o turno seguinte começa na primeira conta livre, com a linha de volta.
    std::fs::remove_file(a.dir.join("cota-esgotada")).unwrap();
    amb.host.call("donnaEngineFallbackAction", json!({"action":"backNow","engineAccountId":a.id})).await;
    amb.host.reply(&bot, "Responda ok").await;
    let historico = amb.host.call("history", json!({"botId": bot})).await;
    assert!(historico.to_string().contains("Voltei para Conta a"), "{historico}");
    let usos = usos_de_cota(&amb.host);
    assert_eq!(resumo(&usos).last(), Some(&par("a", "ok")), "{usos:?}");
    assert_eq!(amb.estado().await["switches"].as_array().unwrap().len(), 2, "a volta também é uma troca");
}

#[tokio::test]
#[allow(clippy::many_single_char_names)]
async fn perto_do_limite_troca_antes_do_envio_so_a_partir_de_90_na_sessao_ou_na_semana() {
    let amb = ambiente().await;
    let (c, d, e, f, g, h) =
        (amb.conta('c'), amb.conta('d'), amb.conta('e'), amb.conta('f'), amb.conta('g'), amb.conta('h'));
    std::fs::write(c.dir.join("uso-sessao"), "89").unwrap();
    std::fs::write(e.dir.join("uso-sessao"), "90").unwrap();
    std::fs::write(g.dir.join("uso-sessao"), "10").unwrap();
    std::fs::write(g.dir.join("uso-semana"), "90").unwrap();
    registrar_contas(&amb.host, &amb.sal, &[&c, &d, &e, &f, &g, &h]);

    // 89% não troca.
    let (bot_c, bot_e, bot_g) = (amb.bot_na(&c).await, amb.bot_na(&e).await, amb.bot_na(&g).await);
    amb.reserva_do_bot(&bot_c, &[&d]).await;
    amb.reserva_do_bot(&bot_e, &[&f]).await;
    amb.reserva_do_bot(&bot_g, &[&h]).await;
    amb.host.reply(&bot_c, "Responda ok").await;
    assert_eq!(resumo(&usos_de_cota(&amb.host)), [par("c", "ok")]);

    // 90% da sessão troca ANTES do envio: a conta E nem é chamada.
    amb.host.reply(&bot_e, "Responda ok").await;
    let historico = amb.host.call("history", json!({"botId": bot_e})).await;
    assert!(historico.to_string().contains("Conta e perto do limite; seguindo com Conta f."), "{historico}");
    // 90% da semana também.
    amb.host.reply(&bot_g, "Responda ok").await;
    let usos = usos_de_cota(&amb.host);
    assert_eq!(resumo(&usos), [par("c", "ok"), par("f", "ok"), par("h", "ok")], "{usos:?}");
    sem_api_paga(&usos);
}

/// Descanso por "perto do limite" vigente + leitura de uso indisponível: a conta continua parada (não volta a receber
/// envio a >= 90% só porque a leitura falhou).
#[tokio::test]
#[allow(clippy::cast_possible_truncation)]
async fn descanso_por_perto_do_limite_segue_valendo_sem_leitura_de_uso() {
    let amb = ambiente().await;
    let (x, y) = (amb.conta('x'), amb.conta('y'));
    registrar_contas(&amb.host, &amb.sal, &[&x, &y]);
    let bot = amb.bot_na(&x).await;
    amb.reserva_do_bot(&bot, &[&y]).await;
    let ate =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64 + 3_600_000;
    let estado = json!({"descanso": {x.id.clone(): {"ate": ate, "motivo": "perto_do_limite"}}});
    let db = rusqlite::Connection::open(amb.host.home.join("codync.db")).unwrap();
    // O estado do Hub é um registro só; aqui ele já tem as ordens salvas pela API.
    let atual: String = db.query_row("SELECT v FROM kv WHERE k='fork.fallback-contas.v1'", [], |r| r.get(0)).unwrap();
    let mut registro: Value = serde_json::from_str(&atual).unwrap();
    registro["descanso"] = estado["descanso"].clone();
    db.execute("UPDATE kv SET v=?1 WHERE k='fork.fallback-contas.v1'", [registro.to_string()]).unwrap();
    drop(db);
    amb.host.reply(&bot, "Responda ok").await;
    assert_eq!(resumo(&usos_de_cota(&amb.host)), [par("y", "ok")], "a conta parada não pode receber o envio");
}

#[tokio::test]
async fn sem_alternativa_para_e_so_segue_com_o_uso_extra_permitido() {
    let amb = ambiente().await;
    let (i, j) = (amb.conta('i'), amb.conta('j'));
    std::fs::write(i.dir.join("uso-sessao"), "95").unwrap();
    std::fs::write(j.dir.join("uso-sessao"), "95").unwrap();
    registrar_contas(&amb.host, &amb.sal, &[&i, &j]);
    let bot = amb.bot_na(&i).await;
    amb.reserva_do_bot(&bot, &[&j]).await;
    amb.enviar(&bot).await;
    let historico = esperar_aviso(&amb.host, &bot, "não há outra conta livre").await;
    assert!(!respondeu_ok(&historico), "{historico}");
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(usos_de_cota(&amb.host).is_empty(), "nada pode ser enviado sem conta livre");
    // O app mostra a hora em que volta e o botão: a conta está em descanso com horário.
    let estado = amb.estado().await;
    let parada = estado["resting"].as_array().unwrap().iter().find(|r| r["engineAccountId"] == json!(i.id)).unwrap();
    assert!(parada["restUntil"].as_i64().unwrap() > 0, "{estado}");

    // Permitir uso extra vale só até o reset informado; a mensagem guardada sai sozinha.
    amb.host.call("donnaEngineFallbackAction", json!({"action":"allowExtra","engineAccountId":i.id})).await;
    let limite = std::time::Instant::now() + std::time::Duration::from_secs(40);
    while usos_de_cota(&amb.host).is_empty() {
        assert!(std::time::Instant::now() < limite, "a mensagem guardada não saiu após permitir o uso extra");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert_eq!(resumo(&usos_de_cota(&amb.host)), [par("i", "ok")]);
}

#[tokio::test]
async fn erro_comum_nao_troca_de_conta() {
    let amb = ambiente().await;
    let (k, l) = (amb.conta('k'), amb.conta('l'));
    std::fs::write(k.dir.join("erro-comum"), "").unwrap();
    registrar_contas(&amb.host, &amb.sal, &[&k, &l]);
    let bot = amb.bot_na(&k).await;
    amb.reserva_do_bot(&bot, &[&l]).await;
    amb.enviar(&bot).await;
    esperar_aviso(&amb.host, &bot, "The agent failed").await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert_eq!(resumo(&usos_de_cota(&amb.host)), [par("k", "erro")], "erro qualquer falha como antes");
}

#[tokio::test]
async fn chave_de_api_e_conta_nao_comprovada_nunca_entram_na_troca() {
    let amb = ambiente().await;
    let (m, n) = (amb.conta('m'), amb.conta('n'));
    let api = conta_gerenciada(&amb.host, &amb.falsos, &amb.sal, 'p', "paid_api");
    std::fs::write(m.dir.join("cota-esgotada"), "").unwrap();
    registrar_contas(&amb.host, &amb.sal, &[&m, &n, &api]);
    let bot = amb.bot_na(&m).await;
    // Chave de API nem entra na lista.
    let recusa =
        amb.host.refused("donnaEngineFallbackSave", json!({"backend":"claude","botId":bot,"accounts":[api.id]})).await;
    assert!(recusa.contains("fallback_not_subscription"), "{recusa}");
    // Constar da lista não basta: sem um bot do Space usando a conta, ela não é reserva (vínculo independente da lista).
    let sem_vinculo =
        amb.host.refused("donnaEngineFallbackSave", json!({"backend":"claude","botId":bot,"accounts":[n.id]})).await;
    assert!(sem_vinculo.contains("account_wrong_space"), "{sem_vinculo}");
    // Uma reserva válida que deixa de ser comprovada depois da lista não é escolhida.
    amb.reserva_do_bot(&bot, &[&n]).await;
    let nao_comprovada = conta_gerenciada(&amb.host, &amb.falsos, &amb.sal, 'n', "unverified");
    registrar_contas(&amb.host, &amb.sal, &[&m, &nao_comprovada]);
    amb.enviar(&bot).await;
    esperar_aviso(&amb.host, &bot, "Limite da assinatura atingido").await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert_eq!(resumo(&usos_de_cota(&amb.host)), [par("m", "cota_esgotada")]);
}

#[tokio::test]
async fn sem_prova_da_conta_reserva_nada_e_enviado_e_o_destino_fica_bloqueado() {
    let amb = ambiente().await;
    let (o, p) = (amb.conta('o'), amb.conta('p'));
    std::fs::write(o.dir.join("cota-esgotada"), "").unwrap();
    // O CLI não confirma a identidade da reserva.
    std::fs::write(p.dir.join("identidade-negada"), "").unwrap();
    registrar_contas(&amb.host, &amb.sal, &[&o, &p]);
    let bot = amb.bot_na(&o).await;
    amb.reserva_do_bot(&bot, &[&p]).await;
    // A conta O esgota na primeira tentativa; o pedido volta à fila e a reserva P é recusada sem prova.
    amb.enviar(&bot).await;
    let historico = esperar_aviso(&amb.host, &bot, "Não passei para Conta p").await;
    assert!(historico.to_string().contains("o login da conta não foi confirmado"), "{historico}");
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(resumo(&usos_de_cota(&amb.host)).iter().all(|(conta, _)| conta != "p"), "a reserva sem prova recebeu algo");
    // O destino que falhou fica bloqueado por 1 h.
    let estado = amb.estado().await;
    let bloqueada = estado["resting"].as_array().unwrap().iter().find(|r| r["engineAccountId"] == json!(p.id)).unwrap();
    assert!(bloqueada["blockedUntil"].as_i64().unwrap() > 0, "{estado}");
    // Passada a hora de bloqueio, o destino volta a valer: com a prova refeita e boa, a troca acontece.
    std::fs::remove_file(p.dir.join("identidade-negada")).unwrap();
    alterar_estado_do_fallback(&amb.host, |e| {
        e["bloqueio"] = json!({p.id.clone(): 1});
    });
    let limite = std::time::Instant::now() + std::time::Duration::from_secs(40);
    while !resumo(&usos_de_cota(&amb.host)).iter().any(|(conta, res)| conta == "p" && res == "ok") {
        assert!(std::time::Instant::now() < limite, "o destino não voltou a valer depois do bloqueio: {estado}");
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
}

#[tokio::test]
async fn efeito_incerto_nao_repete_e_o_proximo_pedido_segue_na_outra_conta() {
    let amb = ambiente().await;
    let (q, r) = (amb.conta('q'), amb.conta('r'));
    std::fs::write(q.dir.join("cota-esgotada"), "").unwrap();
    std::fs::write(q.dir.join("ferramenta-antes-do-limite"), "").unwrap();
    registrar_contas(&amb.host, &amb.sal, &[&q, &r]);
    let bot = amb.bot_na(&q).await;
    amb.reserva_do_bot(&bot, &[&r]).await;
    amb.enviar(&bot).await;
    esperar_aviso(&amb.host, &bot, "confira antes de refazer").await;
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    // A ferramenta já tinha rodado: o pedido não é refeito às cegas na outra conta.
    assert_eq!(resumo(&usos_de_cota(&amb.host)), [par("q", "cota_esgotada")]);
    // O dono manda de novo: agora sai na reserva.
    amb.host.reply(&bot, "Responda ok").await;
    assert_eq!(resumo(&usos_de_cota(&amb.host)).last(), Some(&par("r", "ok")));
}

#[tokio::test]
async fn teto_travado_nao_deixa_a_reserva_enviar() {
    let amb = ambiente().await;
    let (s, t) = (amb.conta('s'), amb.conta('t'));
    std::fs::write(s.dir.join("cota-esgotada"), "").unwrap();
    registrar_contas(&amb.host, &amb.sal, &[&s, &t]);
    let bot = amb.bot_na(&s).await;
    amb.reserva_do_bot(&bot, &[&t]).await;
    amb.host.reply(&bot, "Responda ok").await;
    assert_eq!(resumo(&usos_de_cota(&amb.host)), [par("s", "cota_esgotada"), par("t", "ok")]);
    // Turno sem preço no ledger: o teto de dinheiro não pode ser conferido e trava o próximo envio, de qualquer conta.
    amb.host
        .call(
            "donnaCeilingSave",
            json!({"enabled":true,"money":[{"currency":"USD","limit":1.0}],"tokens":null,"warnPercent":80,
                "applyToConversations":true,"period":"month"}),
        )
        .await;
    amb.enviar(&bot).await;
    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
    assert_eq!(usos_de_cota(&amb.host).len(), 2, "o teto travado não pode ser contornado pela reserva");
}

/// Sobrescreve o estado do fallback no banco do Hub de teste (o registro é um JSON só), para montar situações que a
/// API recusa (ordem de bot de outro Space) ou que só o relógio produziria (bloqueio vencido).
fn alterar_estado_do_fallback(host: &Host, mudar: impl FnOnce(&mut Value)) {
    let db = rusqlite::Connection::open(host.home.join("codync.db")).unwrap();
    let atual: Option<String> =
        db.query_row("SELECT v FROM kv WHERE k='fork.fallback-contas.v1'", [], |r| r.get(0)).ok();
    let mut registro: Value = atual.map_or_else(|| json!({}), |raw| serde_json::from_str(&raw).unwrap());
    mudar(&mut registro);
    db.execute(
        "INSERT INTO kv(k,v) VALUES('fork.fallback-contas.v1',?1) ON CONFLICT(k) DO UPDATE SET v=?1",
        [registro.to_string()],
    )
    .unwrap();
}

/// P1 da revisão do #1017: um bot de OUTRO Space, mesmo com ordem de reserva gravada por baixo da API, segue como antes
/// (espera por limite D72): a conta esgota, o aviso de limite sai e NADA é reenviado nem passa para a reserva.
#[tokio::test]
async fn outro_space_segue_como_antes_sem_reenvio_em_laco() {
    let amb = ambiente().await;
    let (a, b) = (amb.conta('a'), amb.conta('b'));
    std::fs::write(a.dir.join("cota-esgotada"), "").unwrap();
    registrar_contas(&amb.host, &amb.sal, &[&a, &b]);
    let hello = amb.host.call("hello", json!({})).await;
    let hub = hello["hostId"].clone();
    let inicial = amb.host.call("donnaSpaces", json!({"hubId":hub})).await;
    let criado = amb
        .host
        .call(
            "donnaSpaceCreate",
            json!({"hubId":hub,"name":"Outro Space","expectedRevision":inicial["revision"],
                "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"OS"}}}),
        )
        .await;
    let work = criado["spaces"][1]["id"].clone();
    // Um bot do OUTRO Space, com a conta A, conversando nesse Space.
    let bot = amb
        .host
        .call(
            "createBot",
            json!({"hubId":hub,"spaceId":work,"name":"claude","backend":"claude","connectors":[],"notify":false,
                "model":"haiku","forkEditor":{"engineAccountId":a.id}}),
        )
        .await["bot"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let conversa = amb
        .host
        .call("donnaConversationCreate", json!({"hubId":hub,"spaceId":work,"botId":bot,"title":"Outro Space"}))
        .await["conversation"]
        .clone();
    // A revisão da conta registrada no teste é 1 (ler a lista de contas sondaria os CLIs de novo).
    let rev_conta = json!(1);
    let conversa = amb
        .host
        .call(
            "donnaConversationUpdate",
            json!({"hubId":hub,"spaceId":work,"conversationId":conversa["id"],"expectedRevision":conversa["revision"],
                "engineAccountId":a.id,"expectedAccountRevision":rev_conta,
                "engineSelection":{"backend":"claude","model":"haiku","modelOptions":{}}}),
        )
        .await["conversation"]
        .clone();
    // A API recusa a ordem fora do Pessoal; o estado é gravado direto para provar que o portão do limite basta.
    alterar_estado_do_fallback(&amb.host, |e| {
        e["ordensBot"] = json!({format!("{bot}|claude"): [b.id.clone()]});
    });
    amb.host
        .call(
            "donnaConversationSend",
            json!({"hubId":hub,"spaceId":work,"conversationId":conversa["id"],"expectedRevision":conversa["revision"],
                "text":"Responda ok","clientNonce":uuid::Uuid::new_v4().to_string()}),
        )
        .await;
    let limite = std::time::Instant::now() + std::time::Duration::from_secs(40);
    loop {
        let pagina = amb
            .host
            .call("donnaConversationHistory", json!({"hubId":hub,"spaceId":work,"conversationId":conversa["id"]}))
            .await;
        if pagina.to_string().contains("Limite da assinatura atingido") {
            break;
        }
        assert!(std::time::Instant::now() < limite, "sem o aviso de limite do comportamento antigo: {pagina}");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    tokio::time::sleep(std::time::Duration::from_secs(8)).await;
    assert_eq!(resumo(&usos_de_cota(&amb.host)), [par("a", "cota_esgotada")], "uma chamada só: sem laço e sem reserva");
    let estado = amb.estado().await;
    assert!(estado["sessions"].as_array().unwrap().is_empty(), "nenhuma troca em outro Space: {estado}");
    let recusa =
        amb.host.refused("donnaEngineFallbackSave", json!({"spaceId":work,"backend":"claude","accounts":[b.id]})).await;
    assert!(recusa.contains("fallback_space_unsupported"), "{recusa}");
}

/// (l): o cartão de aprovação aberto na conta que esgotou expira e nada dele migra para a conta nova; o que sai na
/// reserva nasce de novo, sem reaproveitar a aprovação.
#[tokio::test]
async fn aprovacao_aberta_na_conta_esgotada_expira_e_nao_migra_para_a_reserva() {
    let amb = ambiente().await;
    let (q, r) = (amb.conta('q'), amb.conta('r'));
    std::fs::write(q.dir.join("cota-esgotada"), "").unwrap();
    std::fs::write(q.dir.join("aprovacao-aberta"), "").unwrap();
    registrar_contas(&amb.host, &amb.sal, &[&q, &r]);
    let bot = amb.bot_na(&q).await;
    amb.reserva_do_bot(&bot, &[&r]).await;
    amb.enviar(&bot).await;
    let historico = esperar_aviso(&amb.host, &bot, "acabou").await;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let entradas = amb.host.call("history", json!({"botId": bot})).await;
    let cartoes: Vec<&Value> =
        entradas["entries"].as_array().unwrap().iter().filter(|e| e["kind"] == "permission").collect();
    assert!(!cartoes.is_empty(), "o cartão de aprovação existia: {historico}");
    assert!(cartoes.iter().all(|c| c["data"]["status"] == "expired"), "nenhum cartão sobrevive à troca: {cartoes:?}");
    // O que sai na reserva é um turno novo: a reserva nunca pede nem herda a aprovação do cartão expirado.
    amb.host.reply(&bot, "Responda ok").await;
    assert_eq!(resumo(&usos_de_cota(&amb.host)).last(), Some(&par("r", "ok")));
}

/// Reinício do Hub no meio da troca: a sobreposição e o descanso são duráveis; depois de reiniciar a conversa continua
/// na reserva e a conta esgotada não recebe nada.
#[tokio::test]
async fn reinicio_do_hub_no_meio_da_troca_conserva_a_reserva_e_o_descanso() {
    let mut amb = ambiente().await;
    let (a, b) = (amb.conta('a'), amb.conta('b'));
    std::fs::write(a.dir.join("cota-esgotada"), "").unwrap();
    registrar_contas(&amb.host, &amb.sal, &[&a, &b]);
    let bot = amb.bot_na(&a).await;
    amb.reserva_do_bot(&bot, &[&b]).await;
    amb.host.reply(&bot, "Responda ok").await;
    assert_eq!(resumo(&usos_de_cota(&amb.host)), [par("a", "cota_esgotada"), par("b", "ok")]);
    let env = amb.falsos.env();
    amb.host.restart_isolado(&env).await;
    let estado = amb.estado().await;
    assert_eq!(estado["sessions"][0]["currentAccountId"], json!(b.id), "a troca sobreviveu ao reinício: {estado}");
    assert!(estado["resting"].as_array().unwrap().iter().any(|r| r["engineAccountId"] == json!(a.id)));
    amb.host.reply(&bot, "Responda ok").await;
    let usos = usos_de_cota(&amb.host);
    assert_eq!(
        resumo(&usos),
        [par("a", "cota_esgotada"), par("b", "ok"), par("b", "ok")],
        "depois do reinício a conta esgotada não é tentada de novo: {usos:?}"
    );
}

/// Q5 (MEX-1294, fatia A): cota esgotada SEM conta reserva — o turno para com o aviso de
/// limite da assinatura e o registro prova que NENHUMA outra conta, modelo ou motor foi
/// chamado e que nenhuma chave de API paga entrou no ambiente.
#[tokio::test]
async fn cota_esgotada_para_o_turno_sem_trocar_de_executor() {
    // `ambiente` deixa TODOS os motores falsos (e o `npx`) instalados: uma tentativa de outro motor, do login global
    // ou de outro modelo ACP subiria e deixaria registro em `launches.jsonl`.
    let amb = ambiente().await;
    let (falsos, host) = (&amb.falsos, &amb.host);
    let conta_a = amb.conta('a');
    std::fs::write(conta_a.dir.join("cota-esgotada"), "").unwrap();
    registrar_contas(host, &amb.sal, &[&conta_a]);
    let bot = amb.bot_na(&conta_a).await;
    host.call(
        "send",
        json!({"botId":bot,"text":"Responda ok","clientNonce":uuid::Uuid::new_v4().to_string(),
            "contentOrigin":common::host::typed_origin("Responda ok")}),
    )
    .await;
    let historico = esperar_aviso(host, &bot, "Limite da assinatura atingido").await;
    assert!(!respondeu_ok(&historico), "conta esgotada não responde: {historico}");
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let usos = usos_de_cota(host);
    assert_eq!(usos.len(), 1, "uma tentativa só, na conta esgotada: {usos:?}");
    assert!(
        usos.iter().all(|u| u["conta"] == "a" && u["resultado"] == "cota_esgotada" && u["chave_api_paga"] == false),
        "{usos:?}"
    );
    let lancamentos: Vec<Value> = std::fs::read_to_string(falsos.pasta.join("launches.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lancamentos.len(), 1, "exatamente um lançamento, o da conta A: {lancamentos:?}");
    assert!(
        lancamentos.iter().all(|l| l["binario"] == "claude-agent-acp" && l["modo"] == "claude"),
        "só o adaptador da conta A pode ter subido: {lancamentos:?}"
    );
    for outro in ["codex", "grok", "cursor-agent", "agy", "opencode", "devin", "kimi", "cline", "npx"] {
        assert!(lancamentos.iter().all(|l| l["binario"] != outro), "outro motor subiu ({outro}): {lancamentos:?}");
    }
}
