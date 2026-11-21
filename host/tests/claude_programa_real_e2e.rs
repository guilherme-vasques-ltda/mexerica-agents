//! Prova opt-in com o binário oficial do Claude Code, sem conta, sem login e sem rede externa: uma API falsa
//! de mensagens responde em `127.0.0.1` no formato documentado de `/v1/messages` (fixture
//! `tests/fixtures/api_anthropic_falsa.py`), o programa real aponta para ela por `ANTHROPIC_BASE_URL` com chave
//! falsa e a ponte do Hub (`__claude-ponte`) o conduz de verdade, pela entrada/saída padrão, como faz com o Hub.
//!
//! Prova, na ordem: (1) os argumentos que a ponte monta são aceitos pelo programa real e ele roda no modo sem
//! tela; (2) ferramenta que exige aprovação gera cartão (`mcp__mexerica_ponte__permissao`), negada não roda e
//! aprovada roda; leitura no espaço de trabalho no modo padrão não gera cartão; (3) os eventos `stream-json` do
//! programa real viram as atualizações que a ponte traduz; (4) `auth status` com chave falsa devolve
//! `loggedIn: true`, o formato que a ponte exige, e sem chave e sem login a sessão é recusada.
//!
//! Só roda quando `MEXERICA_CLAUDE_REAL` aponta para o executável do programa; sem a variável, cada teste
//! termina sem fazer nada. Nada toca na conta de quem roda: `CLAUDE_CONFIG_DIR` e `HOME` são pastas temporárias.
//!
//! Isolamento de rede: sem privilégio não há bloqueio de rede no nível do sistema operacional. O isolamento
//! desta prova é o ambiente limpo (o subprocesso não herda credenciais nem proxies de quem roda, só a lista
//! explícita abaixo) e o programa apontado para o servidor falso em `127.0.0.1`; o teste do contador de
//! chamadas confirma que o programa falou de verdade com esse servidor.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::timeout;

/// Prazo de cada linha do protocolo: um turno do programa real pode levar alguns segundos.
const PRAZO: Duration = Duration::from_secs(120);

/// Caminho absoluto do programa oficial em `MEXERICA_CLAUDE_REAL`. Sem a variável, a prova fica desligada;
/// com a variável definida, um caminho que não é um executável válido é erro explícito: a prova não pode
/// terminar em silêncio e valer como sucesso sem ter rodado o programa.
fn programa_real() -> Option<PathBuf> {
    let p = std::path::absolute(PathBuf::from(std::env::var_os("MEXERICA_CLAUDE_REAL")?))
        .expect("MEXERICA_CLAUDE_REAL não é um caminho utilizável");
    let executavel = p.is_file() && std::fs::metadata(&p).is_ok_and(|m| m.permissions().mode() & 0o111 != 0);
    assert!(
        executavel,
        "MEXERICA_CLAUDE_REAL aponta para {}, que não é um executável válido; \
         corrija o caminho ou remova a variável para desligar a prova",
        p.display()
    );
    Some(p)
}

fn python3() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|d| d.join("python3"))
        .find(|p| p.is_file())
        .expect("python3 no PATH do teste")
}

/// Pastas temporárias da prova: perfil e `HOME` isolados, espaço de trabalho do programa e alvos das ferramentas.
struct Cenario {
    _dir: tempfile::TempDir,
    perfil: PathBuf,
    home: PathBuf,
    cwd: PathBuf,
    toque: PathBuf,
    gravado: PathBuf,
    ler: PathBuf,
}

fn cenario() -> Cenario {
    let dir = tempfile::tempdir().unwrap();
    let perfil = dir.path().join("perfil");
    let home = dir.path().join("home");
    let cwd = dir.path().join("trabalho");
    for p in [&perfil, &home, &cwd] {
        std::fs::create_dir_all(p).unwrap();
    }
    let toque = cwd.join("tocado-pela-ferramenta");
    let gravado = cwd.join("gravado-pela-ferramenta");
    let ler = cwd.join("leia.md");
    std::fs::write(&ler, "conteudo do arquivo a ler\n").unwrap();
    Cenario { _dir: dir, perfil, home, cwd, toque, gravado, ler }
}

/// Onde o node está no `PATH` de quem roda o teste: instalações do programa oficial são script de node.
fn pasta_do_node() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .find(|pasta| pasta.join("node").is_file())
        .expect("node no PATH de quem roda o teste")
}

/// Ambiente explícito do subprocesso da prova, sempre com `env_clear` antes: nada do ambiente de quem roda
/// passa — nem credencial, nem proxy, nem telemetria ligada. `chave` define a chave falsa; `base`, quando
/// presente, aponta para a API falsa em `127.0.0.1`. Só a chave sem base serve ao `auth status`, que informa
/// estado local e não faz pedido; sem chave e sem base não existe login.
fn ambiente(c: &Cenario, chave: bool, base: Option<u16>) -> Vec<(String, String)> {
    let mut vars = vec![
        ("PATH".to_owned(), format!("{}:/usr/local/bin:/usr/bin:/bin", pasta_do_node().display())),
        ("HOME".to_owned(), c.home.display().to_string()),
        ("CLAUDE_CONFIG_DIR".to_owned(), c.perfil.display().to_string()),
        ("NO_PROXY".to_owned(), "127.0.0.1,localhost".to_owned()),
        // Tráfego que o programa documenta como não essencial: telemetria, relatório de erro, atualização e
        // chamadas de modelo fora do turno ficam desligados na prova.
        ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".to_owned(), "1".to_owned()),
        ("DISABLE_AUTOUPDATER".to_owned(), "1".to_owned()),
        ("DISABLE_BUG_COMMAND".to_owned(), "1".to_owned()),
        ("DISABLE_COST_WARNINGS".to_owned(), "1".to_owned()),
        ("DISABLE_ERROR_REPORTING".to_owned(), "1".to_owned()),
        ("DISABLE_NON_ESSENTIAL_MODEL_CALLS".to_owned(), "1".to_owned()),
        ("DISABLE_TELEMETRY".to_owned(), "1".to_owned()),
        ("CLAUDE_CODE_DISABLE_TERMINAL_TITLE".to_owned(), "1".to_owned()),
    ];
    if chave {
        vars.push(("ANTHROPIC_API_KEY".to_owned(), "sk-teste-falso".to_owned()));
    }
    if let Some(porta) = base {
        vars.push(("ANTHROPIC_BASE_URL".to_owned(), format!("http://127.0.0.1:{porta}")));
    }
    vars
}

/// API falsa de mensagens em `127.0.0.1` na porta que o fixture escolher.
struct ApiFalsa {
    _processo: Child,
    porta: u16,
}

async fn api_falsa(c: &Cenario) -> ApiFalsa {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/api_anthropic_falsa.py");
    let mut cmd = Command::new(python3());
    cmd.arg(&fixture)
        .arg("0")
        .env("API_FALSA_TOQUE", &c.toque)
        .env("API_FALSA_ARQUIVO", &c.gravado)
        .env("API_FALSA_LER", &c.ler)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut processo = cmd.spawn().expect("a API falsa subiu");
    let mut linha = String::new();
    timeout(PRAZO, BufReader::new(processo.stdout.take().unwrap()).read_line(&mut linha))
        .await
        .expect("a API falsa anunciou a porta")
        .unwrap();
    let porta = u16::try_from(
        serde_json::from_str::<Value>(&linha).expect("porta da API falsa em JSON")["porta"]
            .as_u64()
            .expect("porta numérica"),
    )
    .expect("porta válida");
    ApiFalsa { _processo: processo, porta }
}

/// Contador de chamadas que o servidor falso recebeu em `/v1/messages`. Um GET cru pela rede, sem cliente
/// HTTP, para não depender de proxy nem de credencial de quem roda.
async fn chamadas_da_api_falsa(porta: u16) -> u64 {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let mut conexao = tokio::net::TcpStream::connect(("127.0.0.1", porta)).await.unwrap();
    conexao.write_all(b"GET /__chamadas HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").await.unwrap();
    let mut bruto = Vec::new();
    conexao.read_to_end(&mut bruto).await.unwrap();
    let texto = String::from_utf8_lossy(&bruto);
    let corpo = texto.split_once("\r\n\r\n").map_or("", |(_, corpo)| corpo);
    let v: Value = serde_json::from_str(corpo).unwrap_or_else(|_| panic!("contador em JSON: {texto}"));
    v["chamadas"].as_u64().unwrap_or_else(|| panic!("resposta sem contador de chamadas: {texto}"))
}

/// Lado do Hub: fala ACP com a ponte (executável do Hub no modo `__claude-ponte`) pela entrada/saída padrão.
struct Ponte {
    _processo: Child,
    escrita: ChildStdin,
    leitura: Lines<BufReader<ChildStdout>>,
    proximo: i64,
    notas: Vec<Value>,
    permissoes: Vec<Value>,
    aprovar: bool,
}

/// Abre a ponte com o programa real. `com_chave`: aponta o programa para a API falsa com chave falsa; sem ela,
/// nem chave nem login existem, e a ponte tem de recusar a sessão. O Hub herda só a lista explícita de
/// `ambiente`, e o programa oficial herda o ambiente do Hub.
fn ponte(programa: &Path, c: &Cenario, porta: u16, com_chave: bool) -> Ponte {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_donnabot-host"));
    cmd.arg("__claude-ponte")
        .arg(programa)
        .env_clear()
        .envs(ambiente(c, com_chave, com_chave.then_some(porta)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    let mut processo = cmd.spawn().expect("a ponte abriu");
    let escrita = processo.stdin.take().unwrap();
    let leitura = BufReader::new(processo.stdout.take().unwrap()).lines();
    Ponte {
        _processo: processo,
        escrita,
        leitura,
        proximo: 1,
        notas: Vec::new(),
        permissoes: Vec::new(),
        aprovar: true,
    }
}

/// As opções que o Hub manda hoje para uma conversa Claude, na forma que a ponte aceita.
fn opcoes_da_conversa() -> Value {
    json!({"settings": {"permissions": {"defaultMode": "default", "allow": ["Read(//docs/**)"],
            "deny": ["Edit(//mem/**)"]}, "includeGitInstructions": false},
        "settingSources": [], "strictMcpConfig": true,
        "disallowedTools": ["CronCreate", "CronDelete", "CronList", "ScheduleWakeup", "RemoteTrigger",
            "Workflow", "ListAgents", "ReportFindings"],
        "permissionMode": "default", "allowDangerouslySkipPermissions": false})
}

/// Versão do programa oficial, com o mesmo ambiente limpo do resto da prova: a saída tem de se apresentar
/// como «Claude Code» e vai para o log, para registrar qual versão foi provada.
async fn conferir_versao(programa: &Path, c: &Cenario) {
    let saida = Command::new(programa)
        .arg("--version")
        .env_clear()
        .envs(ambiente(c, false, None))
        .stdin(Stdio::null())
        .output()
        .await
        .unwrap();
    let texto = String::from_utf8_lossy(&saida.stdout).trim().to_string();
    assert!(texto.contains("Claude Code"), "a saída de --version não se apresenta como Claude Code: {texto:?}");
    eprintln!("versão do programa oficial na prova: {texto}");
}

impl Ponte {
    async fn escrever(&mut self, msg: &Value) {
        self.escrita.write_all(format!("{msg}\n").as_bytes()).await.unwrap();
    }

    /// Pede e espera a resposta, coletando notificações e cartões de permissão no caminho.
    async fn pedir(&mut self, metodo: &str, params: Value) -> Value {
        let id = self.proximo;
        self.proximo += 1;
        self.escrever(&json!({"jsonrpc": "2.0", "id": id, "method": metodo, "params": params})).await;
        loop {
            let linha = timeout(PRAZO, self.leitura.next_line())
                .await
                .expect("a ponte respondeu a tempo")
                .unwrap()
                .expect("a ponte segue aberta");
            let msg: Value = serde_json::from_str(&linha).unwrap();
            match (msg["method"].as_str(), msg.get("id")) {
                (None, Some(i)) if *i == id => return msg,
                (Some("session/request_permission"), Some(i)) => {
                    let ordem = self.notas.len();
                    self.permissoes.push(json!({"ordem": ordem, "params": msg["params"].clone()}));
                    let escolha = if self.aprovar { "allow-once" } else { "reject" };
                    let i = i.clone();
                    self.escrever(&json!({"jsonrpc": "2.0", "id": i,
                        "result": {"outcome": {"outcome": "selected", "optionId": escolha}}}))
                        .await;
                }
                (Some(_), None) => {
                    self.notas.push(msg["params"]["update"].clone());
                }
                _ => {}
            }
        }
    }

    async fn nova(&mut self, cwd: &Path) -> String {
        let r = self
            .pedir(
                "session/new",
                json!({"cwd": cwd, "mcpServers": [],
                    "_meta": {"systemPrompt": {"type": "preset", "preset": "claude_code", "append": "Você é o Berga."},
                        "claudeCode": {"options": opcoes_da_conversa()}}}),
            )
            .await;
        r["result"]["sessionId"].as_str().unwrap_or_else(|| panic!("sessão criada: {r}")).to_owned()
    }

    async fn falar(&mut self, sessao: &str, texto: &str) -> Value {
        self.pedir("session/prompt", json!({"sessionId": sessao, "prompt": [{"type": "text", "text": texto}]})).await
    }

    fn texto(&self, tipo: &str) -> String {
        self.notas.iter().filter(|n| n["sessionUpdate"] == tipo).filter_map(|n| n["content"]["text"].as_str()).collect()
    }

    fn de(&self, tipo: &str) -> Vec<&Value> {
        self.notas.iter().filter(|n| n["sessionUpdate"] == tipo).collect()
    }
}

#[tokio::test]
#[ignore = "prova opt-in: defina MEXERICA_CLAUDE_REAL com o caminho do claude oficial"]
async fn argumentos_da_ponte_sao_aceitos_pelo_programa_real() {
    let Some(programa) = programa_real() else { return };
    let c = cenario();
    let api = api_falsa(&c).await;
    let mut p = ponte(&programa, &c, api.porta, true);
    let init = p.pedir("initialize", json!({"protocolVersion": 1})).await["result"].clone();
    assert_eq!(init["agentInfo"]["name"], "mexerica-claude-ponte", "{init}");
    let sessao = p.nova(&c.cwd).await;
    let r = p.falar(&sessao, "[[texto]]").await;
    // Um programa real que rejeitasse uma das opções sairia com erro de uso: o turno só termina se ele aceitou
    // todas as opções montadas pela ponte e rodou no modo sem tela até o fim.
    assert_eq!(r["result"]["stopReason"], "end_turn", "{r}");
    assert_eq!(p.texto("agent_message_chunk"), "resposta da API falsa sem ferramenta");
    assert_eq!(r["result"]["usage"]["inputTokens"], 10, "{r}");
    assert_eq!(r["result"]["usage"]["outputTokens"], 5, "{r}");
    assert!(p.de("usage_update").iter().any(|u| u["used"].is_u64()), "ocupação de contexto traduzida: {:?}", p.notas);
    assert_eq!(p.permissoes.len(), 0, "texto puro não gera cartão");
}

#[tokio::test]
#[ignore = "prova opt-in: defina MEXERICA_CLAUDE_REAL com o caminho do claude oficial"]
async fn bash_do_programa_real_so_roda_aprovado_pela_ponte() {
    let Some(programa) = programa_real() else { return };
    for aprovar in [true, false] {
        let c = cenario();
        let api = api_falsa(&c).await;
        let mut p = ponte(&programa, &c, api.porta, true);
        p.aprovar = aprovar;
        let sessao = p.nova(&c.cwd).await;
        let r = p.falar(&sessao, "[[bash]]").await;
        assert_eq!(r["result"]["stopReason"], "end_turn", "{r}");
        assert_eq!(p.permissoes.len(), 1, "um cartão de permissão: {r} {:?}", p.permissoes);
        let pedido = &p.permissoes[0];
        let chamada = &pedido["params"]["toolCall"];
        assert_eq!(chamada["_meta"]["claudeCode"]["toolName"], "Bash", "{chamada}");
        assert_eq!(chamada["kind"], "execute", "{chamada}");
        let comando = chamada["rawInput"]["command"].as_str().expect("comando no cartão");
        assert!(comando.contains("touch"), "{chamada}");
        assert!(comando.contains(c.toque.file_name().unwrap().to_string_lossy().as_ref()), "{chamada}");
        assert!(chamada["toolCallId"].as_str().is_some_and(|id| !id.is_empty()), "{chamada}");
        let opcoes: Vec<_> =
            pedido["params"]["options"].as_array().unwrap().iter().map(|o| o["optionId"].clone()).collect();
        assert_eq!(opcoes, vec![json!("allow-once"), json!("reject")], "{pedido}");
        // A chamada é anunciada pelo programa antes de o cartão chegar ao dono.
        let anuncio = p
            .notas
            .iter()
            .position(|n| n["sessionUpdate"] == "tool_call" && n["toolCallId"] == chamada["toolCallId"])
            .expect("a chamada foi anunciada");
        assert!(anuncio < usize::try_from(pedido["ordem"].as_u64().unwrap()).unwrap(), "{pedido}");
        // Negado, o arquivo não existe; aprovado, existe: a ponte decide, não o programa.
        assert_eq!(c.toque.is_file(), aprovar, "a ferramenta só roda aprovada");
        let fim = p
            .de("tool_call_update")
            .into_iter()
            .rev()
            .find(|u| u["toolCallId"] == chamada["toolCallId"] && u["status"].is_string())
            .expect("a chamada foi fechada");
        assert_eq!(fim["status"], if aprovar { json!("completed") } else { json!("failed") }, "{fim}");
        assert!(p.texto("agent_message_chunk").contains("resultado recebido"), "{:?}", p.notas);
    }
}

#[tokio::test]
#[ignore = "prova opt-in: defina MEXERICA_CLAUDE_REAL com o caminho do claude oficial"]
async fn write_do_programa_real_grava_so_aprovado_pela_ponte() {
    let Some(programa) = programa_real() else { return };
    for aprovar in [true, false] {
        let c = cenario();
        let api = api_falsa(&c).await;
        let mut p = ponte(&programa, &c, api.porta, true);
        p.aprovar = aprovar;
        let sessao = p.nova(&c.cwd).await;
        let r = p.falar(&sessao, "[[write]]").await;
        assert_eq!(r["result"]["stopReason"], "end_turn", "{r}");
        assert_eq!(p.permissoes.len(), 1, "um cartão de permissão: {r} {:?}", p.permissoes);
        let chamada = &p.permissoes[0]["params"]["toolCall"];
        assert_eq!(chamada["_meta"]["claudeCode"]["toolName"], "Write", "{chamada}");
        assert_eq!(chamada["kind"], "edit", "{chamada}");
        assert_eq!(chamada["rawInput"]["file_path"], c.gravado.display().to_string(), "{chamada}");
        assert_eq!(c.gravado.is_file(), aprovar, "o arquivo só é gravado aprovado");
        if aprovar {
            let gravado = std::fs::read_to_string(&c.gravado).unwrap();
            assert_eq!(gravado, "conteúdo de teste da API falsa\n", "{gravado}");
        }
    }
}

#[tokio::test]
#[ignore = "prova opt-in: defina MEXERICA_CLAUDE_REAL com o caminho do claude oficial"]
async fn read_no_workspace_do_programa_real_nao_gera_cartao() {
    let Some(programa) = programa_real() else { return };
    let c = cenario();
    let api = api_falsa(&c).await;
    let mut p = ponte(&programa, &c, api.porta, true);
    let sessao = p.nova(&c.cwd).await;
    let r = p.falar(&sessao, "[[read]]").await;
    assert_eq!(r["result"]["stopReason"], "end_turn", "{r}");
    assert!(p.permissoes.is_empty(), "leitura no espaço de trabalho não pede aprovação: {:?}", p.permissoes);
    assert_eq!(p.de("tool_call").len(), 1, "uma chamada de leitura: {:?}", p.notas);
    let fim = p.de("tool_call_update").into_iter().rev().find(|u| u["status"].is_string()).unwrap();
    assert_eq!(fim["status"], "completed", "{fim}");
    // O conteúdo lido aparece no resultado da chamada, não na mensagem final do programa.
    assert!(fim["content"][0]["content"]["text"].as_str().unwrap().contains("conteudo do arquivo a ler"), "{fim}");
}

#[tokio::test]
#[ignore = "prova opt-in: defina MEXERICA_CLAUDE_REAL com o caminho do claude oficial"]
async fn versao_do_programa_oficial_e_conferida() {
    let Some(programa) = programa_real() else { return };
    let c = cenario();
    conferir_versao(&programa, &c).await;
}

#[tokio::test]
#[ignore = "prova opt-in: defina MEXERICA_CLAUDE_REAL com o caminho do claude oficial"]
async fn auth_status_sem_login_com_chave_falsa_devolve_logado_verdadeiro() {
    let Some(programa) = programa_real() else { return };
    let c = cenario();
    // A chave é falsa e não aponta para nada: nenhum pedido sai; o programa só informa o estado local.
    let saida = Command::new(&programa)
        .args(["auth", "status"])
        .env_clear()
        .envs(ambiente(&c, true, None))
        .stdin(Stdio::null())
        .output()
        .await
        .unwrap();
    let texto = String::from_utf8_lossy(&saida.stdout);
    let v: Value = serde_json::from_str(texto.trim()).unwrap_or_else(|_| panic!("auth status em JSON: {texto}"));
    assert_eq!(v["loggedIn"], json!(true), "a ponte exige loggedIn verdadeiro: {v}");
    assert_eq!(v["authMethod"], "api_key", "{v}");
}

#[tokio::test]
#[ignore = "prova opt-in: defina MEXERICA_CLAUDE_REAL com o caminho do claude oficial"]
async fn api_falsa_recebe_as_chamadas_do_programa_real() {
    let Some(programa) = programa_real() else { return };
    let c = cenario();
    let api = api_falsa(&c).await;
    let mut p = ponte(&programa, &c, api.porta, true);
    let sessao = p.nova(&c.cwd).await;
    let r = p.falar(&sessao, "[[texto]]").await;
    assert_eq!(r["result"]["stopReason"], "end_turn", "{r}");
    // Sem privilégio não há bloqueio de rede no sistema operacional: o isolamento é o ambiente limpo mais o
    // servidor falso, e esta conferência é a prova de que o programa falou de verdade com esse servidor.
    let chamadas = chamadas_da_api_falsa(api.porta).await;
    assert!(chamadas >= 1, "o programa não falou com o servidor falso: {chamadas} chamadas em /v1/messages");
}

#[tokio::test]
#[ignore = "prova opt-in: defina MEXERICA_CLAUDE_REAL com o caminho do claude oficial"]
async fn ponte_recusa_sessao_do_programa_real_sem_login_nem_chave() {
    let Some(programa) = programa_real() else { return };
    let c = cenario();
    let mut p = ponte(&programa, &c, 0, false);
    let r = p.pedir("session/new", json!({"cwd": c.cwd, "mcpServers": []})).await;
    assert_eq!(r["error"]["code"], -32000, "{r}");
    assert!(
        r["error"]["message"].as_str().unwrap().contains("Authentication required"),
        "falha fechada sem login: {r}"
    );
}
