#![cfg(target_os = "linux")]
//! Provas da fatia 2: ferramenta do Hub por provedor falso local (ida e volta
//! pelo cliente MCP do Hub), permissão negada respeitada, cancelamento que
//! mata o processo, `finish()`/`Drop` sem zumbi nem tarefa solta, pasta de
//! trabalho privada, nenhuma chave em argv/saída/stderr e o MCP inerte: com
//! config MCP do Pi na pasta do motor nenhum servidor abre e a chamada só sai
//! pelo Hub. Os testes que abrem o motor seguem o padrão da fatia 1 (processo
//! à parte com ajudante fictício; sem Node reprovam sob `REQUIRED_ENV`).
use super::mcp_host::McpHost;
use super::tools::{ToolFuture, ToolOrigin, ToolRunner, ToolSpec};
use super::*;
use std::path::Path;
use std::time::Duration;

const SEGREDO: &str = "mexerica-teste-chave-da-fatia-dois";

/// Servidor MCP mínimo (stdio) que o Hub abre; devolve o pid do pai para a prova
/// de que quem o abriu foi o Hub e não o motor.
pub(super) const SERVIDOR_MCP: &str = r"
import readline from 'node:readline';
const rl = readline.createInterface({ input: process.stdin });
rl.on('line', (line) => {
  const m = JSON.parse(line);
  if (m.id === undefined) return;
  const reply = (result) => console.log(JSON.stringify({ jsonrpc: '2.0', id: m.id, result }));
  if (m.method === 'initialize') reply({ protocolVersion: '2025-06-18', capabilities: { tools: {} }, serverInfo: { name: 'fake', version: '1' } });
  else if (m.method === 'tools/list') reply({ tools: [{ name: 'eco', description: 'ecoa o texto', inputSchema: { type: 'object', properties: { texto: { type: 'string' } } } }] });
  else if (m.method === 'tools/call') reply({ content: [{ type: 'text', text: `eco:${m.params.arguments.texto} ppid:${process.ppid}` }], isError: false });
});
";

fn ferramenta(name: &str) -> ToolSpec {
    ToolSpec {
        name: name.to_owned(),
        description: "teste".to_owned(),
        input_schema: json!({"type": "object", "properties": {"texto": {"type": "string"}}}),
        origin: ToolOrigin::Hub,
    }
}

/// Executor de teste que registra as chamadas e responde com `reply`.
struct Registro {
    chamadas: std::sync::Mutex<Vec<(String, Value)>>,
    reply: ToolReplyFixed,
}
type ToolReplyFixed = Result<&'static str, &'static str>;
impl ToolRunner for Registro {
    fn call<'a>(&'a self, _call_id: &'a str, name: &'a str, input: Value) -> ToolFuture<'a> {
        self.chamadas.lock().unwrap().push((name.to_owned(), input));
        let reply = self.reply.map(str::to_owned).map_err(str::to_owned);
        Box::pin(async move { reply })
    }
}

struct Pendura;
impl ToolRunner for Pendura {
    fn call<'a>(&'a self, _call_id: &'a str, _name: &'a str, _input: Value) -> ToolFuture<'a> {
        Box::pin(std::future::pending())
    }
}

fn existe(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

async fn some(pid: u32) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while existe(pid) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("o processo {pid} continua na tabela (vivo ou zumbi)"));
}

fn filhos(pid: u32) -> Vec<String> {
    let mut all = Vec::new();
    for task in std::fs::read_dir(format!("/proc/{pid}/task")).unwrap().flatten() {
        let text = std::fs::read_to_string(task.path().join("children")).unwrap_or_default();
        all.extend(text.split_whitespace().map(str::to_owned));
    }
    all
}

fn novo_store() -> Arc<Store> {
    Arc::new(Store::open(Path::new(":memory:")).unwrap())
}

macro_rules! caso {
    ($nome:literal) => {{
        let Some(case) =
            crate::fork::motor_exec::test_support::case(concat!("fork::motor_mexerica::tests_tools::", $nome))
        else {
            return;
        };
        if !super::tests::require_motor() {
            return;
        }
        case
    }};
}

#[tokio::test]
async fn ferramenta_do_hub_vai_e_volta_pelo_cliente_mcp_do_hub() {
    let case = caso!("ferramenta_do_hub_vai_e_volta_pelo_cliente_mcp_do_hub");
    let script = case.dir.join("servidor.mjs");
    std::fs::write(&script, SERVIDOR_MCP).unwrap();
    let node = node_path().unwrap();
    let definicoes = json!([{"name": "fake", "command": node, "args": [script], "env": []}]);
    let mcp = McpHost::start(&definicoes, &case.dir).await.unwrap();
    assert_eq!(mcp.specs().len(), 1);
    assert_eq!(mcp.specs()[0].name, "mcp__fake__eco");

    let store = novo_store();
    crate::market::vault::write(&store, &credential_slot("faux"), SEGREDO).unwrap();
    let mut engine = Engine::spawn(store, "faux", &[(FAUX_ENV, "1")]).unwrap();
    engine.set_system_prompt("Você é um bot da Mexerica.").await.unwrap();
    engine.set_tools(&mcp.specs()).await.unwrap();
    let turn = engine
        .prompt_with(
            "faux",
            "faux-1",
            &serde_json::json!({}),
            SEM_CACHE,
            r#"/ferramenta mcp__fake__eco {"texto":"oi"}"#,
            &mcp,
            &mut |_| {},
        )
        .await
        .unwrap();
    assert!(turn.text.contains("ferramenta respondeu: eco:oi"), "{}", turn.text);
    // Quem abriu o servidor MCP foi o Hub (este processo), nunca o motor.
    assert!(turn.text.contains(&format!("ppid:{}", std::process::id())), "{}", turn.text);
    let motor = engine.pid().unwrap();
    assert!(filhos(motor).is_empty(), "o motor abriu processo filho: {:?}", filhos(motor));
    // A chave só existe no cofre: nem argv, nem eventos, nem stderr.
    let cmdline = std::fs::read_to_string(format!("/proc/{motor}/cmdline")).unwrap();
    assert!(!cmdline.contains(SEGREDO));
    assert!(!serde_json::to_string(&turn.events).unwrap().contains(SEGREDO));
    let (stderr, _) = engine.finish().await.unwrap();
    assert!(!String::from_utf8_lossy(&stderr).contains(SEGREDO));
}

#[tokio::test]
async fn permissao_negada_volta_ao_modelo_e_nada_executa() {
    let _case = caso!("permissao_negada_volta_ao_modelo_e_nada_executa");
    let mut engine = Engine::spawn(novo_store(), "faux", &[(FAUX_ENV, "1")]).unwrap();
    engine.set_tools(&[ferramenta("list_bots")]).await.unwrap();
    let negada = Registro { chamadas: std::sync::Mutex::default(), reply: Err("permissão negada pelo dono") };
    let turn = engine
        .prompt_with(
            "faux",
            "faux-1",
            &serde_json::json!({}),
            SEM_CACHE,
            "/ferramenta list_bots {}",
            &negada,
            &mut |_| {},
        )
        .await
        .unwrap();
    assert!(turn.text.contains("ferramenta falhou: permissão negada pelo dono"), "{}", turn.text);
    assert_eq!(negada.chamadas.lock().unwrap().len(), 1);
    engine.finish().await.unwrap();
}

#[tokio::test]
async fn cancelar_o_turno_mata_o_processo() {
    let _case = caso!("cancelar_o_turno_mata_o_processo");
    let store = novo_store();
    let mut engine = Engine::spawn(store.clone(), "faux", &[(FAUX_ENV, "1")]).unwrap();
    engine.set_tools(&[ferramenta("list_bots")]).await.unwrap();
    let pid = engine.pid().unwrap();
    // O executor nunca responde; o turno é cancelado largando o futuro.
    let mut ignora = |_: &Value| {};
    let limites = serde_json::json!({});
    let turno =
        engine.prompt_with("faux", "faux-1", &limites, SEM_CACHE, "/ferramenta list_bots {}", &Pendura, &mut ignora);
    assert!(tokio::time::timeout(Duration::from_millis(800), turno).await.is_err());
    assert!(existe(pid), "o motor tinha de estar vivo antes do cancelamento");
    engine.kill().await.unwrap();
    some(pid).await;
    // A tarefa do cofre também terminou: só este teste segura o Store.
    assert_eq!(Arc::strong_count(&store), 1, "tarefa do cofre ficou solta");
}

#[tokio::test]
async fn finish_colhe_o_filho_e_encerra_a_tarefa_do_cofre() {
    let _case = caso!("finish_colhe_o_filho_e_encerra_a_tarefa_do_cofre");
    let store = novo_store();
    let mut engine = Engine::spawn(store.clone(), "faux", &[(FAUX_ENV, "1")]).unwrap();
    engine.prompt("faux", "faux-1", "oi").await.unwrap();
    let pid = engine.pid().unwrap();
    let (_, status) = engine.finish().await.unwrap();
    assert_eq!(status["status"], 0, "o motor devia sair sozinho ao fechar o stdin");
    assert!(!existe(pid), "o filho ficou zumbi depois de finish()");
    assert_eq!(Arc::strong_count(&store), 1, "tarefa do cofre ficou solta");
}

#[tokio::test]
async fn engine_descartado_sem_finish_nao_vaza_processo_nem_tarefa() {
    let _case = caso!("engine_descartado_sem_finish_nao_vaza_processo_nem_tarefa");
    let store = novo_store();
    let engine = Engine::spawn(store.clone(), "faux", &[(FAUX_ENV, "1")]).unwrap();
    let pid = engine.pid().unwrap();
    drop(engine);
    some(pid).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while Arc::strong_count(&store) > 1 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("a tarefa do cofre segurou o Store depois do Drop");
}

#[tokio::test]
async fn pasta_de_trabalho_e_a_privada_do_motor_nao_o_tmp() {
    let _case = caso!("pasta_de_trabalho_e_a_privada_do_motor_nao_o_tmp");
    let mut engine = Engine::spawn(novo_store(), "faux", &[(FAUX_ENV, "1")]).unwrap();
    engine.prompt("faux", "faux-1", "oi").await.unwrap();
    let cwd = std::fs::read_link(format!("/proc/{}/cwd", engine.pid().unwrap())).unwrap();
    assert_ne!(cwd, std::env::temp_dir(), "o motor roda no tmp compartilhado");
    assert_eq!(cwd.file_name().and_then(|n| n.to_str()), Some("work"), "{}", cwd.display());
    let home = cwd.parent().unwrap();
    assert!(home.join("config").is_dir() && home.join("state").is_dir(), "{} não é a pasta privada", home.display());
    engine.finish().await.unwrap();
}

#[tokio::test]
async fn config_mcp_do_pi_na_pasta_do_motor_nao_abre_servidor_e_a_chamada_so_sai_pelo_hub() {
    let case = caso!("config_mcp_do_pi_na_pasta_do_motor_nao_abre_servidor_e_a_chamada_so_sai_pelo_hub");
    // "Servidor" remoto: qualquer conexão do motor a ele fica registrada.
    let escuta = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let porta = escuta.local_addr().unwrap().port();
    let conexoes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let contador = conexoes.clone();
    tokio::spawn(async move {
        while escuta.accept().await.is_ok() {
            contador.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    });
    let sentinela = case.dir.join("mcp-subiu");

    let mut engine = Engine::spawn(novo_store(), "faux", &[(FAUX_ENV, "1")]).unwrap();
    let motor = engine.pid().unwrap();
    engine.set_tools(&[ferramenta("list_bots")]).await.unwrap(); // o ajudante já fez exec do Node (cwd final)
    // Config MCP do Pi/Claude em todos os lugares onde um cliente nativo procuraria.
    let cwd = std::fs::read_link(format!("/proc/{motor}/cwd")).unwrap();
    let home = cwd.parent().unwrap().to_owned();
    // Só escreve dentro da pasta privada do motor: nunca no checkout nem no /tmp.
    assert_eq!(cwd.file_name().and_then(|n| n.to_str()), Some("work"), "{}", cwd.display());
    assert!(home.join("config").is_dir() && home.join("state").is_dir(), "{} não é a pasta privada", home.display());
    let servidores = json!({"mcpServers": {
        "local": {"command": "/bin/sh", "args": ["-c", format!("touch {}", sentinela.display())]},
        "remoto": {"url": format!("http://127.0.0.1:{porta}/mcp")},
    }});
    for pasta in [&cwd, &home] {
        for arquivo in [".mcp.json", ".pi/mcp.json", ".pi/agent/mcp.json", ".pi/settings.json", "config/pi/mcp.json"] {
            let alvo = pasta.join(arquivo);
            std::fs::create_dir_all(alvo.parent().unwrap()).unwrap();
            std::fs::write(alvo, servidores.to_string()).unwrap();
        }
    }

    let hub = Registro { chamadas: std::sync::Mutex::default(), reply: Ok("pelo hub") };
    let turn = engine
        .prompt_with("faux", "faux-1", &serde_json::json!({}), SEM_CACHE, "/ferramenta list_bots {}", &hub, &mut |_| {})
        .await
        .unwrap();
    assert!(turn.text.contains("ferramenta respondeu: pelo hub"), "{}", turn.text);
    assert_eq!(hub.chamadas.lock().unwrap().len(), 1, "a chamada tinha de sair pelo Hub");
    assert!(filhos(motor).is_empty(), "o motor abriu servidor MCP: {:?}", filhos(motor));
    assert!(!sentinela.exists(), "o servidor MCP local foi executado");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(conexoes.load(std::sync::atomic::Ordering::SeqCst), 0, "o motor conectou na URL do MCP");
    engine.finish().await.unwrap();
}

/// "Motor" mínimo de fixture (não é o rpc.ts): pede uma ferramenta que NUNCA
/// foi declarada em `set_tools` e depois encerra o turno com o resultado que
/// o Hub mandou. Serve para provar que o guarda do Hub é o que segura a
/// chamada — mutação que troca `self.tools.contains` por `true` derruba este teste.
const MOTOR_FANTASMA: &str = r"
const rl = require('readline').createInterface({ input: process.stdin });
rl.on('line', (line) => {
  const m = JSON.parse(line);
  if (m.id !== undefined) {
    console.log(JSON.stringify({ type: 'response', id: m.id, success: true, result: { disposition: 'started' } }));
    if (m.type === 'prompt') console.log(JSON.stringify({ type: 'tool_request', callId: 'c1', name: 'ferramenta_fantasma', input: {} }));
  } else if (m.type === 'tool_result') {
    console.log(JSON.stringify({ type: 'message_end', message: { role: 'assistant', stopReason: 'end', content: [{ type: 'text', text: 'ferramenta falhou: ' + (m.error || 'sem erro') }] } }));
    console.log(JSON.stringify({ type: 'agent_end' }));
  }
});
";

#[tokio::test]
async fn ferramenta_nao_declarada_nao_chega_ao_executor() {
    let case = caso!("ferramenta_nao_declarada_nao_chega_ao_executor");
    let entry = case.dir.join("motor-fantasma.cjs");
    std::fs::write(&entry, MOTOR_FANTASMA).unwrap();
    // SAFETY: cada prova abre o motor num processo próprio (caso!); aqui só este
    // teste lê MEXERICA_MOTOR_ENTRY.
    unsafe { std::env::set_var(ENTRY_ENV, &entry) };
    let mut engine = Engine::spawn(novo_store(), "faux", &[]).unwrap();
    engine.set_tools(&[ferramenta("list_bots")]).await.unwrap();
    let registro = Registro { chamadas: std::sync::Mutex::default(), reply: Ok("ok") };
    let turn = engine
        .prompt_with("faux", "faux-1", &serde_json::json!({}), SEM_CACHE, "oi", &registro, &mut |_| {})
        .await
        .unwrap();
    assert!(registro.chamadas.lock().unwrap().is_empty(), "nome fora da lista declarada chegou ao executor");
    assert!(turn.text.contains("ferramenta falhou: ferramenta não declarada"), "{}", turn.text);
    engine.finish().await.unwrap();
}

/// Um motor que para de ler a entrada: a escrita do histórico não volta e a restauração prenderia a sessão. O prazo cobre
/// envio e resposta, e o erro sobe para quem chamou descartar o processo.
#[tokio::test]
async fn motor_que_nao_le_a_entrada_nao_segura_a_restauracao_para_sempre() {
    let case = caso!("motor_que_nao_le_a_entrada_nao_segura_a_restauracao_para_sempre");
    let entry = case.dir.join("motor-parado.cjs");
    std::fs::write(&entry, "setInterval(() => {}, 1000);").unwrap();
    // SAFETY: cada prova abre o motor num processo próprio (caso!); aqui só este teste lê MEXERICA_MOTOR_ENTRY.
    unsafe { std::env::set_var(ENTRY_ENV, &entry) };
    SETUP_DEADLINE_MS.store(500, std::sync::atomic::Ordering::Relaxed);
    let mut engine = Engine::spawn(novo_store(), "faux", &[]).unwrap();
    let message = json!({"role": "user", "content": [{"type": "text", "text": "x".repeat(200_000)}], "timestamp": 1});
    let messages = vec![message; 10];
    let started = std::time::Instant::now();
    let error = tokio::time::timeout(Duration::from_secs(20), engine.restore_history(&messages))
        .await
        .expect("a restauração prendeu para sempre: o prazo não cobre a escrita")
        .unwrap_err();
    assert!(format!("{error:#}").contains("não respondeu a tempo"), "{error:#}");
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[tokio::test]
async fn drop_aborta_o_cofre_mesmo_com_o_fd3_preso_num_fileneto() {
    let case = caso!("drop_aborta_o_cofre_mesmo_com_o_fd3_preso_num_fileneto");
    // O filho sai na hora, mas deixa um neto segurando o fd 3 (o socket do
    // cofre): sem o abort do `Drop`, a tarefa do cofre espera o EOF para
    // sempre e segura o `Arc<Store>`.
    let entry = case.dir.join("motor-neto.cjs");
    std::fs::write(
        &entry,
        r"
const { spawn } = require('node:child_process');
const g = spawn('sleep', ['30'], { detached: true, stdio: ['ignore', 'ignore', 'ignore', 3] });
g.unref();
process.stdout.write(JSON.stringify({ pid: g.pid }) + '\n');
process.exit(0);
",
    )
    .unwrap();
    // SAFETY: cada prova abre o motor num processo próprio (caso!); aqui só este
    // teste lê MEXERICA_MOTOR_ENTRY.
    unsafe { std::env::set_var(ENTRY_ENV, &entry) };
    let store = novo_store();
    let mut engine = Engine::spawn(store.clone(), "faux", &[]).unwrap();
    let linha = engine.next_line().await.unwrap();
    let neto = linha["pid"].as_i64().unwrap();
    drop(engine);
    tokio::time::timeout(Duration::from_secs(5), async {
        while Arc::strong_count(&store) > 1 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("o Drop do Engine não abortou a tarefa do cofre (fd 3 preso no neto)");
    let _ = std::process::Command::new("/bin/kill").args(["-KILL", &neto.to_string()]).status();
}

#[tokio::test]
async fn cancelar_o_turno_avisa_o_servidor_mcp_com_notifications_cancelled() {
    let case = caso!("cancelar_o_turno_avisa_o_servidor_mcp_com_notifications_cancelled");
    let marca = case.dir.join("cancelou");
    let script = case.dir.join("servidor.mjs");
    std::fs::write(
        &script,
        format!(
            r"
import readline from 'node:readline';
import fs from 'node:fs';
const rl = readline.createInterface({{ input: process.stdin }});
rl.on('line', (line) => {{
  const m = JSON.parse(line);
  if (m.method === 'notifications/cancelled') {{ fs.writeFileSync('{}', 'cancelled'); return; }}
  if (m.id === undefined) return;
  const reply = (result) => console.log(JSON.stringify({{ jsonrpc: '2.0', id: m.id, result }}));
  if (m.method === 'initialize') reply({{ protocolVersion: '2025-06-18', capabilities: {{ tools: {{}} }}, serverInfo: {{ name: 'fake', version: '1' }} }});
  else if (m.method === 'tools/list') reply({{ tools: [{{ name: 'eco', description: 'ecoa', inputSchema: {{ type: 'object' }} }}] }});
  else if (m.method === 'tools/call') setTimeout(() => reply({{ content: [{{ type: 'text', text: 'eco' }}], isError: false }}), 30000);
}});
",
            marca.display()
        ),
    )
    .unwrap();
    let node = node_path().unwrap();
    let definicoes = json!([{"name": "fake", "command": node, "args": [script], "env": []}]);
    let mcp = McpHost::start(&definicoes, &case.dir).await.unwrap();
    let mut engine = Engine::spawn(novo_store(), "faux", &[(FAUX_ENV, "1")]).unwrap();
    engine.set_tools(&mcp.specs()).await.unwrap();
    let pid = engine.pid().unwrap();
    // O tools/call fica em andamento; largar o futuro do turno tem de mandar
    // notifications/cancelled ao servidor (P2-4).
    let mut ignora = |_: &Value| {};
    let limites = serde_json::json!({});
    let turno =
        engine.prompt_with("faux", "faux-1", &limites, SEM_CACHE, "/ferramenta mcp__fake__eco {}", &mcp, &mut ignora);
    // O timeout larga o futuro do turno: é o cancelamento.
    assert!(tokio::time::timeout(Duration::from_millis(1500), turno).await.is_err());
    tokio::time::timeout(Duration::from_secs(5), async {
        while !marca.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("o servidor MCP não recebeu notifications/cancelled");
    engine.kill().await.unwrap();
    some(pid).await;
}
