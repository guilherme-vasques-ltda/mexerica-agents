#![cfg(unix)]
//! Provas do adaptador ACP interno (fatia 2, MEX-1367): um turno pelo ACP do
//! bot com backend `mexerica` e provedor falso local, chamando uma ferramenta
//! do Hub — a `tool_request` vira `session/request_permission` e volta como
//! `tool_result` (ida e volta pelo cartão de aprovação); permissão negada;
//! `session/cancel` que mata o processo do motor; e nenhuma chave em argv.
//! Os testes que abrem o motor seguem o padrão da fatia 1 (processo à parte
//! com ajudante fictício; sem Node reprovam sob `REQUIRED_ENV`).

use super::link::Link;
use super::mcp_host::McpHost;
use super::tools::ToolRunner;
use super::*;
use crate::agent::acp::{Acp, Answer, Incoming};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

macro_rules! caso {
    ($nome:literal) => {{
        let Some(case) =
            crate::fork::motor_exec::test_support::case(concat!("fork::motor_mexerica::tests_link::", $nome))
        else {
            return;
        };
        if !super::tests::require_motor() {
            return;
        }
        case
    }};
}

const SEGREDO: &str = "mexerica-teste-chave-do-adaptador";
const SERVIDOR_MCP: &str = tests_tools::SERVIDOR_MCP;

/// O mesmo servidor, mas anotando cada `tools/call` num arquivo: prova de que
/// o cartão negado nunca deixa a chamada chegar ao servidor.
fn servidor_marcado(marca: &Path) -> String {
    format!(
        r"
import readline from 'node:readline';
import fs from 'node:fs';
const rl = readline.createInterface({{ input: process.stdin }});
rl.on('line', (line) => {{
  const m = JSON.parse(line);
  if (m.id === undefined) return;
  const reply = (result) => console.log(JSON.stringify({{ jsonrpc: '2.0', id: m.id, result }}));
  if (m.method === 'initialize') reply({{ protocolVersion: '2025-06-18', capabilities: {{ tools: {{}} }}, serverInfo: {{ name: 'fake', version: '1' }} }});
  else if (m.method === 'tools/list') reply({{ tools: [{{ name: 'eco', description: 'ecoa o texto', inputSchema: {{ type: 'object', properties: {{ texto: {{ type: 'string' }} }} }} }}] }});
  else if (m.method === 'tools/call') {{ fs.writeFileSync('{}', 'call'); reply({{ content: [{{ type: 'text', text: `eco:${{m.params.arguments.texto}}` }}], isError: false }}); }}
}});
",
        marca.display()
    )
}

fn novo_store() -> Arc<Store> {
    Arc::new(Store::open(Path::new(":memory:")).unwrap())
}

fn acp_motor(store: Arc<Store>) -> (Arc<Acp>, mpsc::UnboundedReceiver<Incoming>) {
    acp_motor_de(store, "bot-de-prova")
}

/// O adaptador de um bot: cada conexão nova do mesmo bot é um adaptador novo.
fn acp_motor_de(store: Arc<Store>, bot: &str) -> (Arc<Acp>, mpsc::UnboundedReceiver<Incoming>) {
    let link = Link::new(store_vault(store), bot, &[(FAUX_ENV, "1")]);
    Acp::spawn_link(link)
}

/// Trata um item da fila `Incoming` como o bot faz: responde a permissão com
/// `allow`/`deny` e acumula os `session/update`.
async fn tratar(acp: &Arc<Acp>, inc: Incoming, allow: bool, updates: &mut Vec<Value>, permissions: &mut usize) {
    match inc {
        Incoming::Request { id, method, params } if method == "session/request_permission" => {
            *permissions += 1;
            // Guarda o pedido (só `toolCall`) para as provas de identidade e de id.
            updates.push(json!({"sessionUpdate": "_pedido_de_permissao", "toolCall": params["toolCall"].clone()}));
            let option = if allow { "allow" } else { "deny" };
            acp.respond(id, json!({"outcome": {"outcome": "selected", "optionId": option}})).await.unwrap();
        }
        Incoming::Notification { method, params } if method == "session/update" => {
            updates.push(params["update"].clone());
        }
        Incoming::Closed { .. } => panic!("o adaptador encerrou durante o turno"),
        _ => {}
    }
}

/// Consome a fila `Incoming` como o bot faz: responde a permissão com
/// `allow`/`deny` e acumula os `session/update`. Devolve a resposta do
/// `session/prompt`, os updates e quantas permissões foram pedidas.
///
/// Determinístico: o adaptador enfileira cada `session/update` ANTES de
/// responder ao `session/prompt`, mas o `select!` pode ver a resposta pronta
/// antes de ver a fila. Por isso, depois da resposta, a fila é drenada até
/// esvaziar — o mesmo que o bot faz antes de fechar o turno (`bot/mod.rs`).
async fn dirigir(
    acp: &Arc<Acp>,
    answer: Answer,
    rx: &mut mpsc::UnboundedReceiver<Incoming>,
    allow: bool,
) -> (Value, Vec<Value>, usize) {
    let fut = answer.response();
    tokio::pin!(fut);
    let mut result = None;
    let mut updates = Vec::new();
    let mut permissions = 0;
    while result.is_none() {
        tokio::select! {
            r = &mut fut => result = Some(r.unwrap()),
            inc = rx.recv() => match inc {
                Some(inc) => tratar(acp, inc, allow, &mut updates, &mut permissions).await,
                None => panic!("a fila do adaptador fechou durante o turno"),
            },
        }
    }
    while let Ok(inc) = rx.try_recv() {
        tratar(acp, inc, allow, &mut updates, &mut permissions).await;
    }
    (result.unwrap(), updates, permissions)
}

async fn sessao(acp: &Arc<Acp>, case: &crate::fork::motor_exec::test_support::Case) -> String {
    sessao_com(acp, case, SERVIDOR_MCP).await
}

async fn sessao_com(acp: &Arc<Acp>, case: &crate::fork::motor_exec::test_support::Case, servidor: &str) -> String {
    let script = case.dir.join("servidor.mjs");
    std::fs::write(&script, servidor).unwrap();
    let res = acp
        .request(
            "session/new",
            json!({
                "cwd": case.dir,
                "mcpServers": [{"name": "fake", "command": node_path().unwrap(), "args": [script], "env": []}],
                "systemPrompt": "Você é um bot da Mexerica.",
            }),
        )
        .await
        .unwrap();
    let sid = res["sessionId"].as_str().unwrap().to_owned();
    acp.request("session/set_model", json!({"sessionId": sid, "modelId": "faux/faux-1"})).await.unwrap();
    sid
}

#[tokio::test]
async fn turno_com_ferramenta_do_hub_ida_e_volta_pelo_cartao() {
    let case = caso!("turno_com_ferramenta_do_hub_ida_e_volta_pelo_cartao");
    let store = novo_store();
    crate::market::vault::write(&store, &credential_slot("faux"), SEGREDO).unwrap();
    let (acp, mut rx) = acp_motor(store);
    acp.request("initialize", json!({})).await.unwrap();
    let sid = sessao(&acp, &case).await;

    let answer = acp
        .send(
            "session/prompt",
            json!({"sessionId": sid, "prompt": [{"type": "text", "text": r#"/ferramenta mcp__fake__eco {"texto":"oi"}"#}]}),
        )
        .await
        .unwrap();
    let (result, updates, permissions) = dirigir(&acp, answer, &mut rx, true).await;
    assert_eq!(result["stopReason"], "end_turn", "{result}");
    assert_eq!(permissions, 1, "a tool_request tinha de virar um pedido de permissão");
    assert!(
        updates.iter().any(|u| u["sessionUpdate"] == "tool_call" && u["status"] == "in_progress"),
        "a chamada não apareceu na conversa: {updates:?}"
    );
    assert!(
        updates.iter().any(|u| u["sessionUpdate"] == "tool_call_update" && u["status"] == "completed"),
        "o resultado não apareceu na conversa: {updates:?}"
    );
    let texto: String = updates
        .iter()
        .filter(|u| u["sessionUpdate"] == "agent_message_chunk")
        .filter_map(|u| u["content"]["text"].as_str().map(str::to_owned))
        .collect();
    assert!(texto.contains("ferramenta respondeu: eco:oi"), "{texto}");
    // O `toolCallId` é do Hub (`mxt-…`), nunca o `callId` escolhido pelo motor
    // (`call-N-…`), e é o mesmo no pedido de permissão e nos updates.
    let ids: std::collections::HashSet<&str> = updates
        .iter()
        .filter(|u| {
            matches!(u["sessionUpdate"].as_str(), Some("tool_call" | "tool_call_update" | "_pedido_de_permissao"))
        })
        .filter_map(|u| u["toolCallId"].as_str().or_else(|| u["toolCall"]["toolCallId"].as_str()))
        .collect();
    assert_eq!(ids.len(), 1, "uma chamada, um id: {ids:?}");
    assert!(ids.iter().all(|id| id.starts_with("mxt-")), "id do Hub esperado: {ids:?}");

    // A chave só existe no cofre: nunca em argv do motor.
    let pid = acp.pid().unwrap();
    let cmdline = std::fs::read_to_string(format!("/proc/{pid}/cmdline")).unwrap();
    assert!(!cmdline.contains(SEGREDO));
    acp.kill_and_wait().await;
    // O reap do grupo é em segundo plano: espera o pid sair da tabela.
    tokio::time::timeout(Duration::from_secs(10), async {
        while Path::new(&format!("/proc/{pid}")).exists() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("o motor ficou vivo depois do kill");
}

#[tokio::test]
async fn permissao_negada_volta_como_erro_da_ferramenta() {
    let case = caso!("permissao_negada_volta_como_erro_da_ferramenta");
    let marca = case.dir.join("tools-call");
    // O servidor real anota cada tools/call: cartão negado → o arquivo não existe.
    let servidor = servidor_marcado(&marca);
    let (acp, mut rx) = acp_motor(novo_store());
    acp.request("initialize", json!({})).await.unwrap();
    let sid = sessao_com(&acp, &case, &servidor).await;

    let answer = acp
        .send(
            "session/prompt",
            json!({"sessionId": sid, "prompt": [{"type": "text", "text": "/ferramenta mcp__fake__eco {}"}]}),
        )
        .await
        .unwrap();
    let (result, updates, permissions) = dirigir(&acp, answer, &mut rx, false).await;
    assert_eq!(result["stopReason"], "end_turn", "{result}");
    assert_eq!(permissions, 1);
    assert!(
        updates.iter().any(|u| u["sessionUpdate"] == "tool_call_update" && u["status"] == "failed"),
        "a negação não virou falha da ferramenta: {updates:?}"
    );
    let texto: String = updates
        .iter()
        .filter(|u| u["sessionUpdate"] == "agent_message_chunk")
        .filter_map(|u| u["content"]["text"].as_str().map(str::to_owned))
        .collect();
    assert!(texto.contains("ferramenta falhou"), "{texto}");
    assert!(!marca.exists(), "tools/call chegou ao servidor com o cartão negado");
    acp.kill_and_wait().await;
}

#[tokio::test]
async fn cancelar_o_turno_mata_o_processo_do_motor() {
    let case = caso!("cancelar_o_turno_mata_o_processo_do_motor");
    let (acp, mut rx) = acp_motor(novo_store());
    acp.request("initialize", json!({})).await.unwrap();
    let sid = sessao(&acp, &case).await;

    let answer = acp
        .send(
            "session/prompt",
            json!({"sessionId": sid, "prompt": [{"type": "text", "text": "/ferramenta mcp__fake__eco {}"}]}),
        )
        .await
        .unwrap();
    // Espera o pedido de permissão chegar (o turno fica pendurado nele).
    let fut = answer.response();
    tokio::pin!(fut);
    let mut result = None;
    let mut cancelled = false;
    let deadline = Instant::now() + Duration::from_secs(30);
    while result.is_none() {
        assert!(Instant::now() < deadline, "o turno não terminou depois do cancelamento");
        tokio::select! {
            r = &mut fut => result = Some(r.unwrap()),
            inc = rx.recv() => {
                if let Some(Incoming::Request { method, .. }) = &inc
                    && method == "session/request_permission"
                    && !cancelled
                {
                    cancelled = true;
                    let pid = acp.pid().unwrap();
                    // Em vez de responder: cancela. O processo do motor morre.
                    acp.notify("session/cancel", json!({"sessionId": sid})).await.unwrap();
                    let prazo = Instant::now() + Duration::from_secs(10);
                    while Path::new(&format!("/proc/{pid}")).exists() {
                        assert!(Instant::now() < prazo, "o motor sobreviveu ao cancelamento");
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                }
            }
        }
    }
    assert!(cancelled, "nenhum pedido de permissão chegou");
    assert_eq!(result.unwrap()["stopReason"], "cancelled");
    acp.kill_and_wait().await;
}

// ---------------------------------------------------------------------------
// Ciclo de vida: processo do motor que cai, flag desligada, MCP que falha.
// ---------------------------------------------------------------------------

/// Espera o processo morrer de fato (some de `/proc` ou vira zumbi).
async fn esperar_morto(pid: u32) {
    let prazo = Instant::now() + Duration::from_secs(10);
    loop {
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Err(_) => return,
            Ok(stat) if stat.rsplit_once(") ").is_some_and(|(_, resto)| resto.starts_with('Z')) => return,
            Ok(_) => {}
        }
        assert!(Instant::now() < prazo, "o processo {pid} não morreu");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// SIGKILL no motor (OOM, falha do SDK) e espera ele morrer.
async fn matar_o_motor(pid: u32) {
    // SAFETY: `kill` com pid de um filho nosso e um sinal válido.
    assert_eq!(unsafe { libc::kill(i32::try_from(pid).unwrap(), libc::SIGKILL) }, 0);
    esperar_morto(pid).await;
}

fn texto_do_turno(updates: &[Value]) -> String {
    updates
        .iter()
        .filter(|u| u["sessionUpdate"] == "agent_message_chunk")
        .filter_map(|u| u["content"]["text"].as_str().map(str::to_owned))
        .collect()
}

async fn turno_de_texto(
    acp: &Arc<Acp>,
    rx: &mut mpsc::UnboundedReceiver<Incoming>,
    sid: &str,
    texto: &str,
) -> (Value, Vec<Value>) {
    let answer = acp
        .send("session/prompt", json!({"sessionId": sid, "prompt": [{"type": "text", "text": texto}]}))
        .await
        .unwrap();
    let (result, updates, _) = dirigir(acp, answer, rx, true).await;
    (result, updates)
}

/// Espera o pedido de permissão do turno (o turno fica pendurado nele).
async fn esperar_pedido(rx: &mut mpsc::UnboundedReceiver<Incoming>) -> Value {
    let prazo = Duration::from_secs(30);
    tokio::time::timeout(prazo, async {
        loop {
            match rx.recv().await {
                Some(Incoming::Request { id, method, .. }) if method == "session/request_permission" => return id,
                Some(Incoming::Closed { .. }) | None => panic!("o adaptador encerrou antes do pedido"),
                _ => {}
            }
        }
    })
    .await
    .expect("o pedido de permissão não chegou")
}

#[tokio::test]
async fn motor_que_cai_e_reaberto_no_proximo_turno() {
    let case = caso!("motor_que_cai_e_reaberto_no_proximo_turno");
    let store = novo_store();
    crate::market::vault::write(&store, &credential_slot("faux"), SEGREDO).unwrap();
    let (acp, mut rx) = acp_motor(store);
    acp.request("initialize", json!({})).await.unwrap();
    let sid = sessao(&acp, &case).await;

    let (result, updates) = turno_de_texto(&acp, &mut rx, &sid, "primeiro").await;
    assert_eq!(result["stopReason"], "end_turn", "{result}");
    assert!(texto_do_turno(&updates).contains("eco: primeiro"));

    // Queda ENTRE turnos (SIGKILL logo antes do prompt): o adaptador descarta o motor
    // morto e sobe outro, sem erro para o bot — logo após o SIGKILL o `try_wait` ainda
    // pode ver o processo vivo, e o turno sem nenhum evento entregue é repetido uma vez
    // (antes: "encerrou a saída" e depois "Broken pipe" para sempre).
    let antigo = acp.pid().unwrap();
    matar_o_motor(antigo).await;
    let (result, updates) = turno_de_texto(&acp, &mut rx, &sid, "segundo").await;
    assert_eq!(result["stopReason"], "end_turn", "{result}");
    assert!(texto_do_turno(&updates).contains("eco: segundo"), "{updates:?}");
    let novo = acp.pid().unwrap();
    assert_ne!(novo, antigo, "o motor morto tinha de ser trocado por outro processo");

    // Queda NO MEIO do turno (a ferramenta espera o cartão): o turno falha e o
    // motor morto não volta ao slot; o turno seguinte sobe um limpo.
    let answer = acp
        .send(
            "session/prompt",
            json!({"sessionId": sid, "prompt": [{"type": "text", "text": "/ferramenta mcp__fake__eco {}"}]}),
        )
        .await
        .unwrap();
    let pedido = esperar_pedido(&mut rx).await;
    let no_meio = acp.pid().unwrap();
    matar_o_motor(no_meio).await;
    acp.respond(pedido, json!({"outcome": {"outcome": "selected", "optionId": "deny"}})).await.unwrap();
    assert!(answer.response().await.is_err(), "o turno com o motor morto tinha de falhar");

    let (result, updates) = turno_de_texto(&acp, &mut rx, &sid, "terceiro").await;
    assert_eq!(result["stopReason"], "end_turn", "{result}");
    assert!(texto_do_turno(&updates).contains("eco: terceiro"), "{updates:?}");
    assert_ne!(acp.pid().unwrap(), no_meio);
    acp.kill_and_wait().await;
}

/// Pergunta ao provedor falso o que ele recebeu: o texto de cada mensagem de
/// usuário da conversa, na ordem (`/historico`).
async fn historico_visto(acp: &Arc<Acp>, rx: &mut mpsc::UnboundedReceiver<Incoming>, sid: &str) -> String {
    let (result, updates) = turno_de_texto(acp, rx, sid, "/historico").await;
    assert_eq!(result["stopReason"], "end_turn", "{result}");
    texto_do_turno(&updates)
}

/// A conversa é do Hub, não do processo do motor. Cancelar o turno
/// mata o processo, e o próximo turno tem de seguir a mesma conversa.
#[tokio::test]
async fn conversa_sobrevive_ao_cancelamento_do_turno() {
    let case = caso!("conversa_sobrevive_ao_cancelamento_do_turno");
    let (acp, mut rx) = acp_motor(novo_store());
    acp.request("initialize", json!({})).await.unwrap();
    let sid = sessao(&acp, &case).await;
    turno_de_texto(&acp, &mut rx, &sid, "meu codigo é 42").await;

    let answer = acp
        .send(
            "session/prompt",
            json!({"sessionId": sid, "prompt": [{"type": "text", "text": "/ferramenta mcp__fake__eco {}"}]}),
        )
        .await
        .unwrap();
    let _pedido = esperar_pedido(&mut rx).await;
    let antigo = acp.pid().unwrap();
    acp.notify("session/cancel", json!({"sessionId": sid})).await.unwrap();
    assert_eq!(answer.response().await.unwrap()["stopReason"], "cancelled");
    esperar_morto(antigo).await;

    let texto = historico_visto(&acp, &mut rx, &sid).await;
    assert_eq!(texto, "historico: meu codigo é 42 | /ferramenta mcp__fake__eco {} | /historico");
    assert_ne!(acp.pid().unwrap(), antigo, "o turno cancelado tinha de ter matado o processo");
    acp.kill_and_wait().await;
}

/// Trocar de provedor sobe outro processo, e a conversa vai junto.
#[tokio::test]
async fn conversa_sobrevive_a_troca_de_provedor() {
    let case = caso!("conversa_sobrevive_a_troca_de_provedor");
    let store = novo_store();
    crate::market::vault::write(&store, &credential_slot("faux2"), SEGREDO).unwrap();
    let (acp, mut rx) = acp_motor(store);
    acp.request("initialize", json!({})).await.unwrap();
    let sid = sessao(&acp, &case).await;
    turno_de_texto(&acp, &mut rx, &sid, "primeira pergunta").await;
    let antigo = acp.pid().unwrap();

    acp.request("session/set_model", json!({"sessionId": sid, "modelId": "faux2/faux-1"})).await.unwrap();
    let texto = historico_visto(&acp, &mut rx, &sid).await;
    assert_eq!(texto, "historico: primeira pergunta | /historico");
    assert_ne!(acp.pid().unwrap(), antigo, "o provedor novo tinha de subir outro processo");
    acp.kill_and_wait().await;
}

/// `session/set_model` recusado pelo motor derruba o processo em vez de
/// deixar no slot um processo com a conversa de comandos dessincronizada.
#[tokio::test]
async fn set_model_que_falha_descarta_o_processo_e_a_proxima_tentativa_abre_outro() {
    let case = caso!("set_model_que_falha_descarta_o_processo_e_a_proxima_tentativa_abre_outro");
    let store = novo_store();
    crate::market::vault::write(&store, &credential_slot("faux"), SEGREDO).unwrap();
    let (acp, mut rx) = acp_motor(store);
    acp.request("initialize", json!({})).await.unwrap();
    let sid = sessao(&acp, &case).await;
    let antigo = acp.pid().expect("o set_model inicial tinha de ter subido um motor");

    // Modelo inexistente: o motor recusa e o erro sobe ao bot.
    let recusado =
        acp.request("session/set_model", json!({"sessionId": sid, "modelId": "faux/modelo-que-nao-existe"})).await;
    assert!(recusado.is_err(), "o motor tinha de recusar o modelo inexistente");
    esperar_morto(antigo).await;
    assert!(acp.pid().is_none(), "o processo recusado tinha de sair do slot");

    // A próxima tentativa sobe um processo NOVO e conversa com ele.
    acp.request("session/set_model", json!({"sessionId": sid, "modelId": "faux/faux-1"})).await.unwrap();
    let novo = acp.pid().unwrap();
    assert_ne!(novo, antigo, "o set_model seguinte tinha de subir outro processo");
    let (result, _) = turno_de_texto(&acp, &mut rx, &sid, "depois da recusa").await;
    assert_eq!(result["stopReason"], "end_turn", "{result}");
    acp.kill_and_wait().await;
}

/// O processo cai (OOM, SIGKILL) entre dois turnos e no meio de um.
#[tokio::test]
async fn conversa_sobrevive_a_queda_do_motor() {
    let case = caso!("conversa_sobrevive_a_queda_do_motor");
    let (acp, mut rx) = acp_motor(novo_store());
    acp.request("initialize", json!({})).await.unwrap();
    let sid = sessao(&acp, &case).await;
    turno_de_texto(&acp, &mut rx, &sid, "um").await;
    matar_o_motor(acp.pid().unwrap()).await;
    turno_de_texto(&acp, &mut rx, &sid, "dois").await;

    let answer = acp
        .send(
            "session/prompt",
            json!({"sessionId": sid, "prompt": [{"type": "text", "text": "/ferramenta mcp__fake__eco {}"}]}),
        )
        .await
        .unwrap();
    let pedido = esperar_pedido(&mut rx).await;
    matar_o_motor(acp.pid().unwrap()).await;
    acp.respond(pedido, json!({"outcome": {"outcome": "selected", "optionId": "deny"}})).await.unwrap();
    assert!(answer.response().await.is_err(), "o turno com o motor morto tinha de falhar");

    let texto = historico_visto(&acp, &mut rx, &sid).await;
    assert_eq!(texto, "historico: um | dois | /ferramenta mcp__fake__eco {} | /historico");
    acp.kill_and_wait().await;
}

/// Trocar de modelo no app FECHA a conexão e o bot abre outra, com o
/// mesmo id de sessão (session/load). A conversa tem de estar lá, no motor novo, mesmo em outro provedor.
#[tokio::test]
async fn conversa_volta_por_session_load_quando_a_conexao_fecha() {
    let case = caso!("conversa_volta_por_session_load_quando_a_conexao_fecha");
    let store = novo_store();
    crate::market::vault::write(&store, &credential_slot("faux2"), SEGREDO).unwrap();
    let (antiga, mut rx) = acp_motor_de(store.clone(), "bot-load");
    let init = antiga.request("initialize", json!({})).await.unwrap();
    assert_eq!(init["agentCapabilities"]["loadSession"], true, "sem isso o bot nunca pede session/load");
    let sid = sessao(&antiga, &case).await;
    turno_de_texto(&antiga, &mut rx, &sid, "meu codigo é 42").await;
    antiga.kill_and_wait().await;

    let params =
        json!({"sessionId": sid, "cwd": case.dir, "mcpServers": [], "systemPrompt": "Você é um bot da Mexerica."});
    let (intrusa, _rx) = acp_motor_de(store.clone(), "outro-bot");
    assert!(intrusa.request("session/load", params.clone()).await.is_err(), "outro bot não reabre a conversa");
    intrusa.kill_and_wait().await;

    let (nova, mut rx) = acp_motor_de(store, "bot-load");
    nova.request("session/load", params).await.unwrap();
    nova.request("session/set_model", json!({"sessionId": sid, "modelId": "faux2/faux-1"})).await.unwrap();
    let texto = historico_visto(&nova, &mut rx, &sid).await;
    assert_eq!(texto, "historico: meu codigo é 42 | /historico");
    nova.kill_and_wait().await;
}

/// A credencial de um provedor muda DE VERDADE no cofre. A conversa que já passou por ele recomeça sem
/// cópia (não dá para provar que a chave nova é da mesma conta); a outra conversa do mesmo bot, que só passou por
/// outro provedor, segue intacta.
#[tokio::test]
async fn credencial_trocada_no_cofre_recomeca_so_a_conversa_do_provedor_afetado() {
    let case = caso!("credencial_trocada_no_cofre_recomeca_so_a_conversa_do_provedor_afetado");
    let store = novo_store();
    crate::market::vault::write(&store, &credential_slot("faux"), SEGREDO).unwrap();
    crate::market::vault::write(&store, &credential_slot("faux2"), SEGREDO).unwrap();
    let (acp, mut rx) = acp_motor(store.clone());
    acp.request("initialize", json!({})).await.unwrap();
    let a = sessao(&acp, &case).await;
    let b = sessao(&acp, &case).await;
    acp.request("session/set_model", json!({"sessionId": b, "modelId": "faux2/faux-1"})).await.unwrap();
    turno_de_texto(&acp, &mut rx, &a, "conversa A, provedor faux").await;
    turno_de_texto(&acp, &mut rx, &b, "conversa B, provedor faux2").await;
    assert_eq!(historico_visto(&acp, &mut rx, &a).await, "historico: conversa A, provedor faux | /historico");

    crate::market::vault::write(&store, &credential_slot("faux"), "mexerica-teste-chave-nova-do-faux").unwrap();
    assert_eq!(
        historico_visto(&acp, &mut rx, &a).await,
        "historico: /historico",
        "a credencial do provedor mudou: a conversa A recomeça, no processo vivo também"
    );
    assert_eq!(
        historico_visto(&acp, &mut rx, &b).await,
        "historico: conversa B, provedor faux2 | /historico",
        "a conversa B nunca passou pelo provedor trocado e fica"
    );
    // O recomeço vale também depois de uma queda: a cópia guardada de A foi apagada, não só o processo.
    matar_o_motor(acp.pid().unwrap()).await;
    assert_eq!(historico_visto(&acp, &mut rx, &a).await, "historico: /historico | /historico");
    acp.kill_and_wait().await;
}

/// Sem como proteger a conversa (chave curta demais para achar no texto), ela não é guardada.
#[tokio::test]
async fn chave_curta_demais_para_proteger_nao_guarda_a_conversa() {
    let case = caso!("chave_curta_demais_para_proteger_nao_guarda_a_conversa");
    let store = novo_store();
    crate::market::vault::write(&store, &credential_slot("faux"), "curta").unwrap();
    let (acp, mut rx) = acp_motor(store);
    acp.request("initialize", json!({})).await.unwrap();
    let sid = sessao(&acp, &case).await;
    turno_de_texto(&acp, &mut rx, &sid, "esta conversa não pode ser guardada").await;
    assert!(acp.pid().is_none(), "o processo que viu o turno sem proteção não volta ao slot");
    let texto = historico_visto(&acp, &mut rx, &sid).await;
    assert_eq!(texto, "historico: /historico", "sem proteção nada ficou guardado para o processo novo");
    acp.kill_and_wait().await;
}

/// O cofre falhou ao ler uma chave que a proteção precisa; a conversa não é guardada (o erro não vira "sem chaves").
#[tokio::test]
async fn cofre_ilegivel_para_proteger_nao_guarda_a_conversa() {
    let case = caso!("cofre_ilegivel_para_proteger_nao_guarda_a_conversa");
    let store = novo_store();
    let ok = store_vault(store);
    let google = credential_slot("google");
    let vault: VaultSource =
        Arc::new(move |slot: &str| if slot == google { anyhow::bail!("cofre indisponível") } else { ok(slot) });
    let (acp, mut rx) = Acp::spawn_link(Link::new(vault, "bot-de-prova", &[(FAUX_ENV, "1")]));
    acp.request("initialize", json!({})).await.unwrap();
    let sid = sessao(&acp, &case).await;
    turno_de_texto(&acp, &mut rx, &sid, "esta conversa não pode ser guardada").await;
    assert!(acp.pid().is_none(), "o processo que viu o turno sem proteção não volta ao slot");
    let texto = historico_visto(&acp, &mut rx, &sid).await;
    assert_eq!(texto, "historico: /historico", "sem ler o cofre nada ficou guardado para o processo novo");
    acp.kill_and_wait().await;
}

/// A chave de um endereço próprio REGISTRADO que não é o provedor desta conversa também sai do texto antes de a
/// conversa ser guardada: a leitura de proteção cobre todos os endereços do cofre, não só o desta sessão.
#[tokio::test]
async fn chave_de_outro_endereco_proprio_sai_do_texto_guardado() {
    let case = caso!("chave_de_outro_endereco_proprio_sai_do_texto_guardado");
    let store = novo_store();
    crate::market::vault::write(&store, &credential_slot("faux"), SEGREDO).unwrap();
    let chave = "chave-do-endereco-proprio-registrado-que-nao-e-este-provedor";
    let registro = json!({"key": chave, "baseUrl": "https://exemplo.invalido/v1",
        "formato": "chat-completions", "local": false, "nome": "Serviço registrado"});
    crate::market::vault::write(&store, &credential_slot("proprio-abcdef12"), &registro.to_string()).unwrap();
    super::proprio::incluir(&store, "proprio-abcdef12").unwrap();
    let fonte = store.clone();
    let link = Link::new(store_vault(store), "bot-de-prova", &[(FAUX_ENV, "1")]);
    link.use_proprios(Arc::new(move || super::proprio::ids(&fonte)));
    let (acp, mut rx) = Acp::spawn_link(link);
    acp.request("initialize", json!({})).await.unwrap();
    let sid = sessao(&acp, &case).await;
    turno_de_texto(&acp, &mut rx, &sid, &format!("minha outra chave é {chave}")).await;
    // O motor vivo viu o texto cru; o que prova a redação é a CÓPIA guardada, que volta no próximo processo.
    matar_o_motor(acp.pid().unwrap()).await;
    let texto = historico_visto(&acp, &mut rx, &sid).await;
    assert_eq!(texto, "historico: minha outra chave é [removido] | /historico");
    acp.kill_and_wait().await;
}

/// ESCOLHER um endereço próprio com a conversa vazia não o marca como recebedor: a conversa que nascer depois
/// em outro provedor não segue para ele — só um turno lá entrega a conversa.
#[tokio::test]
async fn escolher_endereco_proprio_com_conversa_vazia_nao_o_marca_recebedor() {
    let case = caso!("escolher_endereco_proprio_com_conversa_vazia_nao_o_marca_recebedor");
    let pedaco = |delta: Value, fim: Value| {
        format!(
            "data: {}\n\n",
            json!({"id": "c1", "object": "chat.completion.chunk", "created": 1, "model": "modelo-local",
                "choices": [{"index": 0, "delta": delta, "finish_reason": fim}]})
        )
    };
    let corpo = pedaco(json!({"role": "assistant", "content": "resposta do endereço"}), Value::Null)
        + &pedaco(json!({}), json!("stop"))
        + "data: [DONE]\n\n";
    let (base, pedidos) = tests_chaves::servico_local(move |_| (200, corpo.clone(), "text/event-stream")).await;
    let store = novo_store();
    crate::market::vault::write(&store, &credential_slot("faux"), SEGREDO).unwrap();
    let registro = json!({"key": "chave-do-endereco-proprio-2", "baseUrl": format!("{base}/v1"),
        "formato": "chat-completions", "local": true, "nome": "Serviço local"});
    crate::market::vault::write(&store, &credential_slot("proprio-abcdef12"), &registro.to_string()).unwrap();
    super::proprio::incluir(&store, "proprio-abcdef12").unwrap();
    let fonte = store.clone();
    let link = Link::new(store_vault(store), "bot-de-prova", &[(FAUX_ENV, "1")]);
    link.use_proprios(Arc::new(move || super::proprio::ids(&fonte)));
    let (acp, mut rx) = Acp::spawn_link(link);
    acp.request("initialize", json!({})).await.unwrap();
    let params = json!({"cwd": case.dir, "mcpServers": []});
    let sid = acp.request("session/new", params.clone()).await.unwrap()["sessionId"].as_str().unwrap().to_owned();

    // Só ESCOLHER o endereço, com a conversa vazia.
    acp.request("session/set_model", json!({"sessionId": sid, "modelId": "proprio-abcdef12/modelo-local"}))
        .await
        .unwrap();

    // A conversa nasce em outro provedor.
    acp.request("session/set_model", json!({"sessionId": sid, "modelId": "faux/faux-1"})).await.unwrap();
    let marca = "CONVERSA-QUE-NAO-PODE-SAIR-88";
    let answer = acp
        .send("session/prompt", json!({"sessionId": sid, "prompt": [{"type": "text", "text": marca}]}))
        .await
        .unwrap();
    dirigir(&acp, answer, &mut rx, true).await;

    // Voltar ao endereço escolhido antes e conversar lá: a conversa do outro provedor não pode ir junto.
    acp.request("session/set_model", json!({"sessionId": sid, "modelId": "proprio-abcdef12/modelo-local"}))
        .await
        .unwrap();
    let answer = acp
        .send("session/prompt", json!({"sessionId": sid, "prompt": [{"type": "text", "text": "oi"}]}))
        .await
        .unwrap();
    let (result, _, _) = dirigir(&acp, answer, &mut rx, true).await;
    assert_eq!(result["stopReason"], "end_turn", "{result}");
    let recebidos = pedidos.lock().unwrap().clone();
    let conversa = recebidos.iter().find(|p| p.starts_with("POST ")).expect("o endereço próprio não recebeu o turno");
    assert!(conversa.contains("\"oi\""), "{conversa}");
    assert!(!conversa.contains(marca), "a escolha com a conversa vazia marcou o endereço como recebedor: {conversa}");
    acp.kill_and_wait().await;
}

/// O índice dos endereços está CORROMPIDO no cofre: índice ausente é lista vazia, mas índice ilegível é erro —
/// a conversa não é guardada sem saber quais chaves a proteção precisa mascarar.
#[tokio::test]
async fn indice_de_enderecos_corrompido_nao_guarda_a_conversa() {
    let case = caso!("indice_de_enderecos_corrompido_nao_guarda_a_conversa");
    let store = novo_store();
    crate::market::vault::write(&store, &credential_slot("faux"), SEGREDO).unwrap();
    store.kv_set(super::proprio::INDICE, "não é json").unwrap();
    let fonte = store.clone();
    let link = Link::new(store_vault(store), "bot-de-prova", &[(FAUX_ENV, "1")]);
    link.use_proprios(Arc::new(move || super::proprio::ids(&fonte)));
    let (acp, mut rx) = Acp::spawn_link(link);
    acp.request("initialize", json!({})).await.unwrap();
    let sid = sessao(&acp, &case).await;
    turno_de_texto(&acp, &mut rx, &sid, "esta conversa não pode ser guardada").await;
    assert!(acp.pid().is_none(), "o processo que viu o turno sem proteção não volta ao slot");
    let texto = historico_visto(&acp, &mut rx, &sid).await;
    assert_eq!(texto, "historico: /historico", "índice corrompido não pode virar \"sem endereços\"");
    acp.kill_and_wait().await;
}

/// Sem conseguir listar os endereços próprios, a proteção não presume "sem chaves": a conversa não é guardada,
/// como em qualquer leitura do cofre que falha.
#[tokio::test]
async fn falha_ao_listar_os_enderecos_proprios_nao_guarda_a_conversa() {
    let case = caso!("falha_ao_listar_os_enderecos_proprios_nao_guarda_a_conversa");
    let store = novo_store();
    crate::market::vault::write(&store, &credential_slot("faux"), SEGREDO).unwrap();
    let link = Link::new(store_vault(store), "bot-de-prova", &[(FAUX_ENV, "1")]);
    link.use_proprios(Arc::new(|| anyhow::bail!("índice de endereços ilegível na prova")));
    let (acp, mut rx) = Acp::spawn_link(link);
    acp.request("initialize", json!({})).await.unwrap();
    let sid = sessao(&acp, &case).await;
    turno_de_texto(&acp, &mut rx, &sid, "esta conversa não pode ser guardada").await;
    assert!(acp.pid().is_none(), "o processo que viu o turno sem proteção não volta ao slot");
    let texto = historico_visto(&acp, &mut rx, &sid).await;
    assert_eq!(texto, "historico: /historico", "sem a lista de endereços nada ficou guardado");
    acp.kill_and_wait().await;
}

/// A credencial servida ao motor é a da geração autorizada para a conversa: se o cofre muda no meio (rotação entre a
/// decisão e a chamada ao provedor), a leitura é recusada em vez de a conta nova receber o contexto da anterior.
#[test]
fn credencial_servida_e_presa_a_geracao_autorizada() {
    let key = Arc::new(std::sync::Mutex::new(Some("chave-autorizada-1".to_owned())));
    let source = key.clone();
    let vault: VaultSource = Arc::new(move |_| Ok(source.lock().unwrap().clone()));
    let pinned = link::pinned(vault, "google", link::credential_generation("google", Some("chave-autorizada-1")));
    assert_eq!(pinned("slot").unwrap().as_deref(), Some("chave-autorizada-1"));
    *key.lock().unwrap() = Some("chave-rotacionada-2".to_owned());
    assert!(pinned("slot").is_err(), "a chave rotacionada depois da autorização não é entregue");
    *key.lock().unwrap() = None;
    assert!(pinned("slot").is_err(), "a credencial removida também não");
    *key.lock().unwrap() = Some("chave-autorizada-1".to_owned());
    assert!(pinned("slot").is_ok(), "a mesma geração volta a valer");
}

/// A proteção falha DEPOIS de já haver conversa guardada e processo vivo: a decisão vem antes de restaurar, então nem a
/// cópia antiga nem o contexto do processo vivo chegam ao turno, e nada do turno sem proteção é guardado.
#[tokio::test]
async fn cofre_que_falha_depois_de_guardar_descarta_a_copia_e_o_motor_vivo() {
    let case = caso!("cofre_que_falha_depois_de_guardar_descarta_a_copia_e_o_motor_vivo");
    let ok = store_vault(novo_store());
    let broken = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = broken.clone();
    let google = credential_slot("google");
    let vault: VaultSource = Arc::new(move |slot: &str| {
        if flag.load(Ordering::Relaxed) && slot == google {
            anyhow::bail!("cofre indisponível");
        }
        ok(slot)
    });
    let (acp, mut rx) = Acp::spawn_link(Link::new(vault, "bot-de-prova", &[(FAUX_ENV, "1")]));
    acp.request("initialize", json!({})).await.unwrap();
    let sid = sessao(&acp, &case).await;
    turno_de_texto(&acp, &mut rx, &sid, "conversa que já estava guardada").await;
    let antes = historico_visto(&acp, &mut rx, &sid).await;
    assert_eq!(antes, "historico: conversa que já estava guardada | /historico", "com proteção a conversa segue");

    broken.store(true, Ordering::Relaxed);
    let sem_protecao = historico_visto(&acp, &mut rx, &sid).await;
    assert_eq!(sem_protecao, "historico: /historico", "sem proteção nem a cópia nem o contexto vivo reaparecem");

    broken.store(false, Ordering::Relaxed);
    let depois = historico_visto(&acp, &mut rx, &sid).await;
    assert_eq!(depois, "historico: /historico", "o turno sem proteção não vira contexto quando o cofre volta");
    acp.kill_and_wait().await;
}

/// O cancelamento que chega enquanto o processo novo recebe a conversa também para o turno.
#[tokio::test]
async fn cancelar_durante_a_retomada_da_conversa_para_o_turno() {
    let case = caso!("cancelar_durante_a_retomada_da_conversa_para_o_turno");
    let (acp, mut rx) = acp_motor(novo_store());
    acp.request("initialize", json!({})).await.unwrap();
    let sid = sessao(&acp, &case).await;
    turno_de_texto(&acp, &mut rx, &sid, "primeiro").await;
    matar_o_motor(acp.pid().unwrap()).await;

    link::PAUSE_AFTER_RESTORE.store(true, Ordering::Relaxed);
    let answer = acp
        .send("session/prompt", json!({"sessionId": sid, "prompt": [{"type": "text", "text": "segundo"}]}))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let second = acp
        .send("session/prompt", json!({"sessionId": sid, "prompt": [{"type": "text", "text": "intruso"}]}))
        .await
        .unwrap();
    let refused =
        tokio::time::timeout(Duration::from_secs(10), second.response()).await.expect("o segundo prompt ficou preso");
    assert!(
        refused.unwrap_err().to_string().contains("turno em curso"),
        "um segundo prompt na mesma sessão é recusado antes de tocar na conversa"
    );
    let model = acp.send("session/set_model", json!({"sessionId": sid, "modelId": "faux/faux-1"})).await.unwrap();
    let refused =
        tokio::time::timeout(Duration::from_secs(10), model.response()).await.expect("o set_model ficou preso");
    assert!(
        refused.unwrap_err().to_string().contains("turno em curso"),
        "trocar o modelo no meio de um turno também é recusado"
    );
    acp.notify("session/cancel", json!({"sessionId": sid})).await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(10), answer.response())
        .await
        .expect("o cancelamento durante a retomada não parou o turno")
        .unwrap();
    link::PAUSE_AFTER_RESTORE.store(false, Ordering::Relaxed);
    assert_eq!(result["stopReason"], "cancelled");
    assert!(acp.pid().is_none(), "o processo novo, ainda na retomada, tem de morrer com o cancelamento");

    let texto = historico_visto(&acp, &mut rx, &sid).await;
    assert_eq!(texto, "historico: primeiro | /historico", "o turno cancelado antes de começar não entra na conversa");
    acp.kill_and_wait().await;
}

#[tokio::test]
async fn kill_and_wait_cancela_o_turno_em_andamento() {
    let case = caso!("kill_and_wait_cancela_o_turno_em_andamento");
    let (acp, mut rx) = acp_motor(novo_store());
    acp.request("initialize", json!({})).await.unwrap();
    let sid = sessao(&acp, &case).await;
    let answer = acp
        .send(
            "session/prompt",
            json!({"sessionId": sid, "prompt": [{"type": "text", "text": "/ferramenta mcp__fake__eco {}"}]}),
        )
        .await
        .unwrap();
    // O turno está pendurado no cartão, com o motor vivo.
    let _pedido = esperar_pedido(&mut rx).await;
    let pid = acp.pid().unwrap();
    assert!(Path::new(&format!("/proc/{pid}")).exists());
    // `kill_and_wait` cancela o turno: o motor não sobrevive gastando o provedor.
    acp.kill_and_wait().await;
    esperar_morto(pid).await;
    // Ninguém fica pendurado: a resposta do prompt resolve (cancelado ou erro).
    let _ = tokio::time::timeout(Duration::from_secs(10), answer.response())
        .await
        .expect("o prompt ficou pendurado depois do kill_and_wait");
}

#[tokio::test]
async fn flag_desligada_nao_abre_servidor_mcp_na_sessao() {
    // Sob MEXERICA_MOTOR=1 externo o caso não se aplica (a flag vale por processo).
    if enabled() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("mex-flag-off-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let marca = dir.join("servidor-abriu");
    let script = dir.join("servidor.mjs");
    std::fs::write(&script, format!("import fs from 'node:fs'; fs.writeFileSync('{}', 'abriu');", marca.display()))
        .unwrap();
    let (acp, _rx) = acp_motor(novo_store());
    acp.request("initialize", json!({})).await.unwrap();
    let error = acp
        .request(
            "session/new",
            json!({
                "cwd": dir,
                "mcpServers": [{"name": "fake", "command": node_path().unwrap_or_else(|| "node".into()), "args": [script], "env": []}],
            }),
        )
        .await
        .expect_err("com a flag desligada a sessão não pode abrir");
    assert!(format!("{error:#}").contains("desligado"), "{error:#}");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!marca.exists(), "um servidor MCP foi aberto com a flag desligada");
    acp.kill_and_wait().await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn falha_ao_abrir_o_mcp_recusa_a_sessao_em_vez_de_calar() {
    let case = caso!("falha_ao_abrir_o_mcp_recusa_a_sessao_em_vez_de_calar");
    let (acp, _rx) = acp_motor(novo_store());
    acp.request("initialize", json!({})).await.unwrap();
    let error = acp
        .request(
            "session/new",
            json!({
                "cwd": case.dir,
                // URL remota: o Hub do motor só abre servidor stdio, e isso tem de ser dito.
                "mcpServers": [{"name": "remoto", "url": "http://127.0.0.1:9/mcp"}],
            }),
        )
        .await
        .expect_err("a falha do McpHost não pode virar uma sessão sem ferramentas");
    assert!(format!("{error:#}").contains("servidores MCP"), "{error:#}");
    acp.kill_and_wait().await;
}

// ---------------------------------------------------------------------------
// Política de permissão idêntica à dos outros motores (decisão da Geral).
// ---------------------------------------------------------------------------

/// Servidor com uma leitura, uma escrita e uma saída enorme.
const SERVIDOR_LER_GRAVAR: &str = r"
import readline from 'node:readline';
const rl = readline.createInterface({ input: process.stdin });
const tools = [
  { name: 'ler', description: 'lê', inputSchema: { type: 'object' }, annotations: { readOnlyHint: true } },
  { name: 'gravar', description: 'grava', inputSchema: { type: 'object' }, annotations: { readOnlyHint: false } },
  { name: 'grande', description: 'saída enorme', inputSchema: { type: 'object' } },
];
rl.on('line', (line) => {
  const m = JSON.parse(line);
  if (m.id === undefined) return;
  const reply = (result) => console.log(JSON.stringify({ jsonrpc: '2.0', id: m.id, result }));
  if (m.method === 'initialize') reply({ protocolVersion: '2025-06-18', capabilities: { tools: {} }, serverInfo: { name: 'fake', version: '1' } });
  else if (m.method === 'tools/list') reply({ tools });
  else if (m.method === 'tools/call') {
    const text = m.params.name === 'grande' ? 'x'.repeat(400000) : m.params.name;
    reply({ content: [{ type: 'text', text }], isError: false });
  }
});
";

/// Um 'executável do Hub' de mentira: o teste não pode abrir `<exe> mcp <nome>`
/// com o binário de teste, então o ajudante entrega o servidor de mentira.
fn exe_do_hub_falso(dir: &Path, script: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let exe = dir.join("hub-falso");
    std::fs::write(&exe, format!("#!/bin/sh\nexec '{}' '{}'\n", node_path().unwrap().display(), script.display()))
        .unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    exe
}

/// Roda UMA chamada pelo executor do cartão, respondendo ao pedido de
/// permissão como o bot (`allow`/`deny`). Devolve o resultado, os updates e
/// quantos pedidos de permissão chegaram.
async fn chamar(
    link: &Arc<Link>,
    runner: &dyn ToolRunner,
    rx: &mut mpsc::UnboundedReceiver<Incoming>,
    name: &str,
    allow: bool,
) -> (Result<String, String>, Vec<Value>, usize) {
    let call = runner.call("call-escolhido-pelo-motor", name, json!({}));
    tokio::pin!(call);
    let mut updates = Vec::new();
    let mut permissions = 0;
    let result = loop {
        tokio::select! {
            result = &mut call => break result,
            Some(inc) = rx.recv() => match inc {
                Incoming::Request { id, method, params } if method == "session/request_permission" => {
                    permissions += 1;
                    updates.push(json!({"sessionUpdate": "_pedido_de_permissao", "toolCall": params["toolCall"].clone()}));
                    let option = if allow { "allow" } else { "deny" };
                    link.answer_for_tests(id.as_str().unwrap(), json!({"outcome": {"outcome": "selected", "optionId": option}}));
                }
                Incoming::Notification { method, params } if method == "session/update" => {
                    updates.push(params["update"].clone());
                }
                _ => {}
            },
        }
    };
    while let Ok(inc) = rx.try_recv() {
        if let Incoming::Notification { method, params } = inc
            && method == "session/update"
        {
            updates.push(params["update"].clone());
        }
    }
    (result, updates, permissions)
}

#[tokio::test]
async fn toda_ferramenta_mcp_pede_cartao_como_nos_motores_acp() {
    let case = caso!("toda_ferramenta_mcp_pede_cartao_como_nos_motores_acp");
    let script = case.dir.join("ler-gravar.mjs");
    std::fs::write(&script, SERVIDOR_LER_GRAVAR).unwrap();
    let hub = exe_do_hub_falso(&case.dir, &script);
    let definicoes = json!([
        {"name": "chat", "command": hub, "args": ["mcp", "chat"], "env": []},
        {"name": "computers", "command": hub, "args": ["mcp", "computers"], "env": []},
        {"name": "conector", "command": node_path().unwrap(), "args": [script], "env": []},
    ]);
    let mcp = McpHost::start_as(&definicoes, &case.dir, Some(&hub)).await.unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let link = Link::new(store_vault(novo_store()), "bot-de-prova", &[]);
    let runner = link.test_runner(mcp, tx);

    // Paridade com os motores ACP (`tests_paridade`): hoje o Hub não libera
    // NENHUMA ferramenta MCP sem cartão, nem leitura do Hub. Todas pedem.
    for nome in
        ["mcp__chat__ler", "mcp__chat__gravar", "mcp__computers__ler", "mcp__conector__ler", "mcp__conector__gravar"]
    {
        let (result, updates, permissions) = chamar(&link, &*runner, &mut rx, nome, true).await;
        assert!(result.is_ok(), "{nome}: {result:?}");
        assert_eq!(permissions, 1, "{nome} tinha de abrir o cartão: {updates:?}");
        let chamada = updates.iter().find(|u| u["sessionUpdate"] == "tool_call").expect("sem tool_call");
        // A identidade sai do que o Hub abriu; o id é do Hub, não do motor.
        let servidor = nome.split("__").nth(1).unwrap();
        assert_eq!(chamada["_meta"]["mexericaTool"]["server"], servidor);
        assert!(chamada["toolCallId"].as_str().unwrap().starts_with("mxt-"), "{chamada}");
        let pedido = updates.iter().find(|u| u["sessionUpdate"] == "_pedido_de_permissao").unwrap();
        assert_eq!(pedido["toolCall"]["_meta"]["mexericaTool"]["server"], servidor);
    }

    // Cartão negado em escrita: a ferramenta não roda.
    let (result, _, permissions) = chamar(&link, &*runner, &mut rx, "mcp__chat__gravar", false).await;
    assert_eq!((result, permissions), (Err("permissão negada".to_owned()), 1));
}

#[tokio::test]
async fn saida_enorme_e_cortada_tambem_na_entrada_gravada_pelo_bot() {
    let case = caso!("saida_enorme_e_cortada_tambem_na_entrada_gravada_pelo_bot");
    let script = case.dir.join("ler-gravar.mjs");
    std::fs::write(&script, SERVIDOR_LER_GRAVAR).unwrap();
    let definicoes = json!([{"name": "conector", "command": node_path().unwrap(), "args": [script], "env": []}]);
    let mcp = McpHost::start_as(&definicoes, &case.dir, None).await.unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let link = Link::new(store_vault(novo_store()), "bot-de-prova", &[]);
    let runner = link.test_runner(mcp, tx);
    let (result, updates, _) = chamar(&link, &*runner, &mut rx, "mcp__conector__grande", true).await;
    assert_eq!(result.unwrap().len(), 400_000, "o executor devolve a saída inteira; o teto é de quem publica");
    let fim = updates.iter().find(|u| u["sessionUpdate"] == "tool_call_update").expect("sem tool_call_update");
    let bruto = fim["rawOutput"].as_str().unwrap();
    assert!(bruto.len() <= tools::MAX_OUTPUT + 64, "rawOutput sem teto: {} bytes", bruto.len());
    assert!(bruto.ends_with("[saída truncada pelo Hub]"));
}

#[tokio::test]
async fn definicao_curada_ou_com_outro_executavel_sai_como_conector() {
    let case = caso!("definicao_curada_ou_com_outro_executavel_sai_como_conector");
    let script = case.dir.join("ler-gravar.mjs");
    std::fs::write(&script, SERVIDOR_LER_GRAVAR).unwrap();
    let hub = exe_do_hub_falso(&case.dir, &script);
    let node = node_path().unwrap();
    let definicoes = json!([
        // Conector curado do Hub: `<exe> mcp curated`, nome fora de HUB_SERVERS.
        {"name": "donna-curado-x", "command": hub, "args": ["mcp", "curated"], "env": []},
        // Nome de servidor interno, mas executável de terceiro: conector.
        {"name": "memory", "command": node, "args": [script], "env": []},
        // A definição exata do Hub: interno.
        {"name": "chat", "command": hub, "args": ["mcp", "chat"], "env": []},
    ]);
    let mcp = McpHost::start_as(&definicoes, &case.dir, Some(&hub)).await.unwrap();
    let classe = |nome: &str| mcp.classify(nome).unwrap_or_else(|| panic!("sem rota para {nome}"));
    assert!(!classe("mcp__donna-curado-x__ler").hub);
    assert!(!classe("mcp__memory__ler").hub);
    assert!(classe("mcp__chat__ler").hub);
    assert!(!classe("mcp__memory__ler").released() && !classe("mcp__donna-curado-x__ler").released());
    assert!(!classe("mcp__chat__ler").released() && !classe("mcp__chat__gravar").released());
    assert!(mcp.classify("mcp__chat__nao_existe").is_none());
}

#[tokio::test]
async fn conversa_nao_segue_para_um_endereco_proprio_que_nao_a_recebeu() {
    let case = caso!("conversa_nao_segue_para_um_endereco_proprio_que_nao_a_recebeu");
    let pedaco = |delta: Value, fim: Value| {
        format!(
            "data: {}\n\n",
            json!({"id": "c1", "object": "chat.completion.chunk", "created": 1, "model": "modelo-local",
                "choices": [{"index": 0, "delta": delta, "finish_reason": fim}]})
        )
    };
    let corpo = pedaco(json!({"role": "assistant", "content": "resposta do endereço"}), Value::Null)
        + &pedaco(json!({}), json!("stop"))
        + "data: [DONE]\n\n";
    let (base, pedidos) = tests_chaves::servico_local(move |_| (200, corpo.clone(), "text/event-stream")).await;
    let store = novo_store();
    crate::market::vault::write(&store, &credential_slot("faux"), SEGREDO).unwrap();
    let registro = json!({"key": "chave-do-endereco-proprio-1", "baseUrl": format!("{base}/v1"),
        "formato": "chat-completions", "local": true, "nome": "Serviço local"});
    crate::market::vault::write(&store, &credential_slot("proprio-abcdef12"), &registro.to_string()).unwrap();
    let (acp, mut rx) = acp_motor(store);
    acp.request("initialize", json!({})).await.unwrap();
    let params = json!({"cwd": case.dir, "mcpServers": []});
    let sid = acp.request("session/new", params).await.unwrap()["sessionId"].as_str().unwrap().to_owned();
    acp.request("session/set_model", json!({"sessionId": sid, "modelId": "faux/faux-1"})).await.unwrap();
    let marca = "CONVERSA-QUE-NAO-PODE-SAIR-77";
    let answer = acp
        .send("session/prompt", json!({"sessionId": sid, "prompt": [{"type": "text", "text": marca}]}))
        .await
        .unwrap();
    dirigir(&acp, answer, &mut rx, true).await;

    // Trocar para um endereço próprio que nunca recebeu esta conversa: ela recomeça lá, sem a cópia.
    acp.request("session/set_model", json!({"sessionId": sid, "modelId": "proprio-abcdef12/modelo-local"}))
        .await
        .unwrap();
    let answer = acp
        .send("session/prompt", json!({"sessionId": sid, "prompt": [{"type": "text", "text": "oi"}]}))
        .await
        .unwrap();
    let (result, updates, _) = dirigir(&acp, answer, &mut rx, true).await;
    assert_eq!(result["stopReason"], "end_turn", "{result}");
    let texto: String = updates
        .iter()
        .filter(|u| u["sessionUpdate"] == "agent_message_chunk")
        .filter_map(|u| u["content"]["text"].as_str().map(str::to_owned))
        .collect();
    assert!(texto.contains("resposta do endereço"), "{texto}");
    let recebidos = pedidos.lock().unwrap().clone();
    let conversa =
        recebidos.iter().find(|p| p.starts_with("POST ")).expect("o endereço próprio não recebeu a conversa");
    assert!(conversa.contains("\"oi\""), "{conversa}");
    assert!(!recebidos.iter().any(|p| p.contains(marca)), "a conversa anterior seguiu para o endereço próprio");
    acp.kill_and_wait().await;
}

// ---------------------------------------------------------------------------
// Catálogo do navegador: o Hub (cliente MCP do motor) negocia a v3 e relista.
// ---------------------------------------------------------------------------

/// Servidor `memory` de mentira: `browser_open` só aparece na lista depois de `browser_status` v3 ser chamado.
const SERVIDOR_MEMORY_NAVEGADOR: &str = r"
import readline from 'node:readline';
const rl = readline.createInterface({ input: process.stdin });
let negociado = false;
const status = { name: 'browser_status', description: 'status', inputSchema: { type: 'object' } };
const abrir = { name: 'browser_open', description: 'abre', inputSchema: { type: 'object' } };
rl.on('line', (line) => {
  const m = JSON.parse(line);
  if (m.id === undefined) return;
  const reply = (result) => console.log(JSON.stringify({ jsonrpc: '2.0', id: m.id, result }));
  if (m.method === 'initialize') reply({ protocolVersion: '2025-06-18', capabilities: { tools: {} }, serverInfo: { name: 'fake', version: '1' } });
  else if (m.method === 'tools/list') reply({ tools: negociado ? [status, abrir] : [status] });
  else if (m.method === 'tools/call' && m.params.name === 'browser_status') {
    const v3 = m.params.arguments.version === 3;
    negociado = negociado || v3;
    reply({ content: [{ type: 'text', text: JSON.stringify({ version: v3 ? 3 : 2, status: v3 ? 'available' : 'unavailable' }) }], isError: false });
  } else if (m.method === 'tools/call') reply({ content: [{ type: 'text', text: 'aberto:' + m.params.name }], isError: false });
});
";

#[tokio::test]
async fn hub_negocia_o_navegador_v3_e_relista_as_ferramentas_para_o_motor() {
    let case = caso!("hub_negocia_o_navegador_v3_e_relista_as_ferramentas_para_o_motor");
    let script = case.dir.join("memory-navegador.mjs");
    std::fs::write(&script, SERVIDOR_MEMORY_NAVEGADOR).unwrap();
    let hub = exe_do_hub_falso(&case.dir, &script);
    let definicoes = json!([{"name": "memory", "command": hub, "args": ["mcp", "memory"], "env": []}]);
    let mcp = McpHost::start_as(&definicoes, &case.dir, Some(&hub)).await.unwrap();
    let nomes = |mcp: &McpHost| mcp.specs().into_iter().map(|spec| spec.name).collect::<Vec<_>>();
    assert_eq!(nomes(&mcp), ["mcp__memory__browser_status"], "controle positivo: sem negociação só o status aparece");
    assert!(mcp.classify("mcp__memory__browser_open").is_none());
    assert!(mcp.refresh_browser().await.unwrap(), "a negociação v3 muda o conjunto de ferramentas");
    assert_eq!(nomes(&mcp), ["mcp__memory__browser_status", "mcp__memory__browser_open"]);
    let classe = mcp.classify("mcp__memory__browser_open").expect("a ferramenta nova precisa de rota");
    assert!(classe.hub && classe.tool == "browser_open", "continua sendo ferramenta interna do Hub");
    let saida = mcp.call("c1", "mcp__memory__browser_open", json!({})).await;
    assert_eq!(saida.unwrap(), "aberto:browser_open");
    assert!(!mcp.refresh_browser().await.unwrap(), "relistar de novo sem mudança não pede set_tools");
}

#[tokio::test]
async fn servidor_de_terceiro_chamado_memory_nunca_e_negociado_nem_relistado() {
    let case = caso!("servidor_de_terceiro_chamado_memory_nunca_e_negociado_nem_relistado");
    let script = case.dir.join("memory-navegador.mjs");
    std::fs::write(&script, SERVIDOR_MEMORY_NAVEGADOR).unwrap();
    let hub = exe_do_hub_falso(&case.dir, &script);
    let definicoes = json!([{"name": "memory", "command": node_path().unwrap(), "args": [script], "env": []}]);
    let mcp = McpHost::start_as(&definicoes, &case.dir, Some(&hub)).await.unwrap();
    assert!(!mcp.refresh_browser().await.unwrap(), "só o servidor interno do Hub é negociado");
    assert_eq!(mcp.specs().len(), 1);
    assert!(mcp.classify("mcp__memory__browser_open").is_none());
}

/// Servidor `memory` de mentira cujo `browser_status` nunca responde (a negociação v3 fica pendurada).
const SERVIDOR_MEMORY_PENDURADO: &str = r"
import readline from 'node:readline';
const rl = readline.createInterface({ input: process.stdin });
const status = { name: 'browser_status', description: 'status', inputSchema: { type: 'object' } };
rl.on('line', (line) => {
  const m = JSON.parse(line);
  if (m.id === undefined) return;
  const reply = (result) => console.log(JSON.stringify({ jsonrpc: '2.0', id: m.id, result }));
  if (m.method === 'initialize') reply({ protocolVersion: '2025-06-18', capabilities: { tools: {} }, serverInfo: { name: 'fake', version: '1' } });
  else if (m.method === 'tools/list') reply({ tools: [status] });
});
";

#[tokio::test]
async fn cancelar_durante_a_negociacao_do_navegador_nao_inicia_o_turno() {
    let case = caso!("cancelar_durante_a_negociacao_do_navegador_nao_inicia_o_turno");
    let script = case.dir.join("memory-pendurado.mjs");
    std::fs::write(&script, SERVIDOR_MEMORY_PENDURADO).unwrap();
    let hub = exe_do_hub_falso(&case.dir, &script);
    let link = Link::new(store_vault(novo_store()), "bot-de-prova", &[(FAUX_ENV, "1")]);
    link.set_hub_exe_for_tests(hub.clone());
    let (acp, _rx) = Acp::spawn_link(link);
    acp.request("initialize", json!({})).await.unwrap();
    let res = acp
        .request(
            "session/new",
            json!({
                "cwd": case.dir,
                "mcpServers": [{"name": "memory", "command": hub, "args": ["mcp", "memory"], "env": []}],
                "systemPrompt": "Você é um bot da Mexerica.",
            }),
        )
        .await
        .unwrap();
    let sid = res["sessionId"].as_str().unwrap().to_owned();
    acp.request("session/set_model", json!({"sessionId": sid, "modelId": "faux/faux-1"})).await.unwrap();
    let started = Instant::now();
    let answer = acp
        .send("session/prompt", json!({"sessionId": sid, "prompt": [{"type": "text", "text": "oi"}]}))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    acp.notify("session/cancel", json!({"sessionId": sid})).await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), answer.response())
        .await
        .expect("o cancelamento tem de interromper a negociação do navegador, não esperar o prazo dela")
        .unwrap();
    assert_eq!(result["stopReason"], "cancelled");
    assert!(started.elapsed() < Duration::from_secs(5));
    acp.kill_and_wait().await;
}

/// Servidor `memory` de mentira cuja lista passa de 256 ferramentas depois da negociação v3.
const SERVIDOR_MEMORY_ENORME: &str = r"
import readline from 'node:readline';
const rl = readline.createInterface({ input: process.stdin });
let negociado = false;
const status = { name: 'browser_status', description: 'status', inputSchema: { type: 'object' } };
const antiga = { name: 'ler_memoria', description: 'lê', inputSchema: { type: 'object' } };
const muitas = Array.from({ length: 300 }, (_, i) => ({ name: 'extra_' + i, description: 'x', inputSchema: { type: 'object' } }));
rl.on('line', (line) => {
  const m = JSON.parse(line);
  if (m.id === undefined) return;
  const reply = (result) => console.log(JSON.stringify({ jsonrpc: '2.0', id: m.id, result }));
  if (m.method === 'initialize') reply({ protocolVersion: '2025-06-18', capabilities: { tools: {} }, serverInfo: { name: 'fake', version: '1' } });
  else if (m.method === 'tools/list') reply({ tools: negociado ? [status, antiga, ...muitas] : [status, antiga] });
  else if (m.method === 'tools/call' && m.params.name === 'browser_status') {
    negociado = m.params.arguments.version === 3;
    reply({ content: [{ type: 'text', text: JSON.stringify({ version: 3, status: 'available' }) }], isError: false });
  } else if (m.method === 'tools/call') reply({ content: [{ type: 'text', text: 'ok:' + m.params.name }], isError: false });
});
";

#[tokio::test]
async fn relistagem_que_estoura_o_limite_preserva_o_catalogo_que_funcionava() {
    let case = caso!("relistagem_que_estoura_o_limite_preserva_o_catalogo_que_funcionava");
    let script = case.dir.join("memory-enorme.mjs");
    std::fs::write(&script, SERVIDOR_MEMORY_ENORME).unwrap();
    let hub = exe_do_hub_falso(&case.dir, &script);
    let definicoes = json!([{"name": "memory", "command": hub, "args": ["mcp", "memory"], "env": []}]);
    let mcp = McpHost::start_as(&definicoes, &case.dir, Some(&hub)).await.unwrap();
    let antes: Vec<String> = mcp.specs().into_iter().map(|spec| spec.name).collect();
    assert_eq!(antes, ["mcp__memory__browser_status", "mcp__memory__ler_memoria"]);
    assert!(mcp.refresh_browser().await.is_err(), "a lista nova não cabe no limite do motor");
    let depois: Vec<String> = mcp.specs().into_iter().map(|spec| spec.name).collect();
    assert_eq!(depois, antes, "o catálogo anterior foi trocado por uma lista parcial");
    let saida = mcp.call("c1", "mcp__memory__ler_memoria", json!({})).await;
    assert_eq!(saida.unwrap(), "ok:ler_memoria", "a ferramenta antiga perdeu a rota");
}

/// Servidor `memory` de mentira que, depois da negociação v3, lista páginas sem fim (`por_pagina` ferramentas
/// com descrição de `descricao` bytes cada) e anota uma linha em `marca` a cada `tools/list` desse tipo.
fn servidor_memory_paginado(marca: &Path, por_pagina: usize, descricao: usize) -> String {
    format!(
        r"
import readline from 'node:readline';
import fs from 'node:fs';
const rl = readline.createInterface({{ input: process.stdin }});
let negociado = false;
const status = {{ name: 'browser_status', description: 'status', inputSchema: {{ type: 'object' }} }};
const antiga = {{ name: 'ler_memoria', description: 'lê', inputSchema: {{ type: 'object' }} }};
const pagina = () => Array.from({{ length: {por_pagina} }}, (_, i) => ({{ name: 'extra_' + i, description: 'x'.repeat({descricao}), inputSchema: {{ type: 'object' }} }}));
rl.on('line', (line) => {{
  const m = JSON.parse(line);
  if (m.id === undefined) return;
  const reply = (result) => console.log(JSON.stringify({{ jsonrpc: '2.0', id: m.id, result }}));
  if (m.method === 'initialize') reply({{ protocolVersion: '2025-06-18', capabilities: {{ tools: {{}} }}, serverInfo: {{ name: 'fake', version: '1' }} }});
  else if (m.method === 'tools/list') {{
    if (!negociado) return reply({{ tools: [status, antiga] }});
    fs.appendFileSync('{}', 'p\n');
    reply({{ tools: pagina().map((t, i) => ({{ ...t, name: 'extra_' + fs.readFileSync('{}', 'utf8').length + '_' + i }})), nextCursor: 'mais' }});
  }} else if (m.method === 'tools/call' && m.params.name === 'browser_status') {{
    negociado = m.params.arguments.version === 3;
    reply({{ content: [{{ type: 'text', text: JSON.stringify({{ version: 3, status: 'available' }}) }}], isError: false }});
  }} else if (m.method === 'tools/call') reply({{ content: [{{ type: 'text', text: 'ok:' + m.params.name }}], isError: false }});
}});
",
        marca.display(),
        marca.display()
    )
}

async fn memory_com_servidor(case: &crate::fork::motor_exec::test_support::Case, script: &str) -> McpHost {
    let arquivo = case.dir.join("memory-falso.mjs");
    std::fs::write(&arquivo, script).unwrap();
    let hub = exe_do_hub_falso(&case.dir, &arquivo);
    let definicoes = json!([{"name": "memory", "command": hub, "args": ["mcp", "memory"], "env": []}]);
    McpHost::start_as(&definicoes, &case.dir, Some(&hub)).await.unwrap()
}

#[tokio::test]
async fn listagem_para_de_pedir_paginas_ao_atingir_o_limite_de_ferramentas() {
    let case = caso!("listagem_para_de_pedir_paginas_ao_atingir_o_limite_de_ferramentas");
    let marca = case.dir.join("paginas.txt");
    let mcp = memory_com_servidor(&case, &servidor_memory_paginado(&marca, 100, 1)).await;
    let antes: Vec<String> = mcp.specs().into_iter().map(|spec| spec.name).collect();
    assert!(mcp.refresh_browser().await.is_err(), "páginas sem fim não cabem no limite do motor");
    let pedidas = std::fs::read_to_string(&marca).unwrap().lines().count();
    assert!(pedidas <= 3, "o Hub pediu {pedidas} páginas: a leitura tem de parar quando passa de 256 ferramentas");
    let depois: Vec<String> = mcp.specs().into_iter().map(|spec| spec.name).collect();
    assert_eq!(depois, antes);
}

#[tokio::test]
async fn listagem_para_de_pedir_paginas_ao_atingir_o_teto_de_bytes() {
    let case = caso!("listagem_para_de_pedir_paginas_ao_atingir_o_teto_de_bytes");
    let marca = case.dir.join("paginas.txt");
    let mcp = memory_com_servidor(&case, &servidor_memory_paginado(&marca, 2, 1_500_000)).await;
    assert!(mcp.refresh_browser().await.is_err(), "descrições gigantes não cabem no teto de bytes");
    let pedidas = std::fs::read_to_string(&marca).unwrap().lines().count();
    assert!(pedidas <= 2, "o Hub pediu {pedidas} páginas: a leitura tem de parar no teto de bytes");
}

/// Servidor `memory` de mentira cuja relistagem depois da negociação v3 vem malformada (sem a lista de ferramentas).
const SERVIDOR_MEMORY_MALFORMADO: &str = r"
import readline from 'node:readline';
const rl = readline.createInterface({ input: process.stdin });
let negociado = false;
const status = { name: 'browser_status', description: 'status', inputSchema: { type: 'object' } };
const antiga = { name: 'ler_memoria', description: 'lê', inputSchema: { type: 'object' } };
rl.on('line', (line) => {
  const m = JSON.parse(line);
  if (m.id === undefined) return;
  const reply = (result) => console.log(JSON.stringify({ jsonrpc: '2.0', id: m.id, result }));
  if (m.method === 'initialize') reply({ protocolVersion: '2025-06-18', capabilities: { tools: {} }, serverInfo: { name: 'fake', version: '1' } });
  else if (m.method === 'tools/list') reply(negociado ? { items: [] } : { tools: [status, antiga] });
  else if (m.method === 'tools/call' && m.params.name === 'browser_status') {
    negociado = m.params.arguments.version === 3;
    reply({ content: [{ type: 'text', text: JSON.stringify({ version: 3, status: 'available' }) }], isError: false });
  } else if (m.method === 'tools/call') reply({ content: [{ type: 'text', text: 'ok:' + m.params.name }], isError: false });
});
";

#[tokio::test]
async fn pagina_malformada_e_erro_e_preserva_o_catalogo_anterior() {
    let case = caso!("pagina_malformada_e_erro_e_preserva_o_catalogo_anterior");
    let mcp = memory_com_servidor(&case, SERVIDOR_MEMORY_MALFORMADO).await;
    let antes: Vec<String> = mcp.specs().into_iter().map(|spec| spec.name).collect();
    assert_eq!(antes, ["mcp__memory__browser_status", "mcp__memory__ler_memoria"]);
    assert!(mcp.refresh_browser().await.is_err(), "página sem a lista de ferramentas virou lista vazia válida");
    let depois: Vec<String> = mcp.specs().into_iter().map(|spec| spec.name).collect();
    assert_eq!(depois, antes, "a ferramenta de memória foi retirada em silêncio");
    let saida = mcp.call("c1", "mcp__memory__ler_memoria", json!({})).await;
    assert_eq!(saida.unwrap(), "ok:ler_memoria");
}

#[tokio::test]
async fn parada_que_chega_durante_o_preparo_do_motor_nao_envia_o_prompt() {
    let case = caso!("parada_que_chega_durante_o_preparo_do_motor_nao_envia_o_prompt");
    let (acp, mut rx) = acp_motor(novo_store());
    acp.request("initialize", json!({})).await.unwrap();
    let sid = sessao(&acp, &case).await;
    // O motor cai entre turnos: o prompt sobe outro, e a parada chega enquanto ele ainda está subindo.
    matar_o_motor(acp.pid().unwrap()).await;
    let answer = acp
        .send(
            "session/prompt",
            json!({"sessionId": sid, "prompt": [{"type": "text", "text": "não deve chegar ao provedor"}]}),
        )
        .await
        .unwrap();
    acp.notify("session/cancel", json!({"sessionId": sid})).await.unwrap();
    let (result, updates, permissions) = dirigir(&acp, answer, &mut rx, true).await;
    assert_eq!(result["stopReason"], "cancelled", "{result}");
    assert_eq!(permissions, 0);
    assert!(texto_do_turno(&updates).is_empty(), "o prompt chegou ao motor depois da parada: {updates:?}");
    acp.kill_and_wait().await;
}

/// Servidor `memory` de mentira cuja consulta `browser_status` v3 falha, mas cuja listagem ofereceria as
/// ferramentas de página: sem a confirmação da negociação, a relistagem não pode publicar essa lista.
const SERVIDOR_MEMORY_STATUS_FALHA: &str = r"
import readline from 'node:readline';
const rl = readline.createInterface({ input: process.stdin });
let negociado = false;
const status = { name: 'browser_status', description: 'status', inputSchema: { type: 'object' } };
const abrir = { name: 'browser_open', description: 'abre', inputSchema: { type: 'object' } };
rl.on('line', (line) => {
  const m = JSON.parse(line);
  if (m.id === undefined) return;
  const reply = (result) => console.log(JSON.stringify({ jsonrpc: '2.0', id: m.id, result }));
  if (m.method === 'initialize') reply({ protocolVersion: '2025-06-18', capabilities: { tools: {} }, serverInfo: { name: 'fake', version: '1' } });
  else if (m.method === 'tools/list') reply({ tools: negociado ? [status, abrir] : [status] });
  else console.log(JSON.stringify({ jsonrpc: '2.0', id: m.id, error: { code: -32000, message: 'sem navegador' } }));
});
";

#[tokio::test]
async fn falha_da_consulta_v3_nao_publica_as_ferramentas_de_pagina() {
    let case = caso!("falha_da_consulta_v3_nao_publica_as_ferramentas_de_pagina");
    let mcp = memory_com_servidor(&case, SERVIDOR_MEMORY_STATUS_FALHA).await;
    let antes: Vec<String> = mcp.specs().into_iter().map(|spec| spec.name).collect();
    assert_eq!(antes, ["mcp__memory__browser_status"]);
    assert!(mcp.refresh_browser().await.is_err(), "a consulta que falhou virou relistagem normal");
    let depois: Vec<String> = mcp.specs().into_iter().map(|spec| spec.name).collect();
    assert_eq!(depois, antes, "a listagem de uma ponte sem negociação confirmada foi publicada");
    assert!(mcp.classify("mcp__memory__browser_open").is_none());
}
const MARCA_SISTEMA: &str = "MARCA-DO-SISTEMA-7Q";
const MARCA_USUARIO: &str = "MARCA-DO-USUARIO-3K";

/// O que o provedor falso recebeu no pedido (`/pedido`): textos de sistema e de usuário.
fn pedido_recebido(texto: &str) -> (String, Vec<String>) {
    let pedido: Value =
        serde_json::from_str(texto.strip_prefix("pedido: ").unwrap_or_else(|| panic!("{texto}"))).unwrap();
    let textos = |campo: &str| -> Vec<String> {
        pedido[campo].as_array().unwrap().iter().map(|t| t.as_str().unwrap().to_owned()).collect()
    };
    (textos("sistema").join("\n"), textos("usuario"))
}

#[test]
fn texto_de_sistema_vale_como_texto_ou_objeto_do_hub_e_recusa_o_resto() {
    use super::link::system_text;
    assert_eq!(system_text(&Value::Null).unwrap(), None);
    assert_eq!(system_text(&json!("só texto")).unwrap().as_deref(), Some("só texto"));
    assert_eq!(system_text(&crate::agent::bot::system_prompt("do Hub")).unwrap().as_deref(), Some("do Hub"));
    let dois = json!({"mode": "append", "content": [{"type": "text", "text": "a"}, {"type": "text", "text": "b"}]});
    assert_eq!(system_text(&dois).unwrap().as_deref(), Some("a\n\nb"));
    for invalido in [
        json!({"mode": "replace", "content": [{"type": "text", "text": "a"}]}),
        json!({"mode": "append"}),
        json!({"mode": "append", "content": [{"type": "image", "data": "x"}]}),
        json!({"mode": "append", "content": [{"type": "text"}]}),
        json!(42),
        json!(["a"]),
    ] {
        assert!(system_text(&invalido).is_err(), "aceitou {invalido}");
    }
}

#[tokio::test]
async fn instrucoes_no_formato_do_hub_chegam_ao_primeiro_pedido_uma_vez() {
    let case = caso!("instrucoes_no_formato_do_hub_chegam_ao_primeiro_pedido_uma_vez");
    let store = novo_store();
    crate::market::vault::write(&store, &credential_slot("faux"), SEGREDO).unwrap();
    let (acp, mut rx) = acp_motor(store);
    acp.request("initialize", json!({})).await.unwrap();
    // O mesmo objeto que o ator do bot manda no session/new (`launch::system_prompt`).
    let params =
        json!({"cwd": case.dir, "mcpServers": [], "systemPrompt": crate::agent::bot::system_prompt(MARCA_SISTEMA)});
    let sid = acp.request("session/new", params).await.unwrap()["sessionId"].as_str().unwrap().to_owned();
    acp.request("session/set_model", json!({"sessionId": sid, "modelId": "faux/faux-1"})).await.unwrap();
    let prompt = json!({"sessionId": sid, "prompt": [{"type": "text", "text": format!("/pedido {MARCA_USUARIO}")}]});
    let answer = acp.send("session/prompt", prompt).await.unwrap();
    let (result, updates, _) = dirigir(&acp, answer, &mut rx, true).await;
    assert_eq!(result["stopReason"], "end_turn", "{result}");
    let texto: String = updates
        .iter()
        .filter(|u| u["sessionUpdate"] == "agent_message_chunk")
        .filter_map(|u| u["content"]["text"].as_str().map(str::to_owned))
        .collect();
    let (sistema, usuario) = pedido_recebido(&texto);
    assert_eq!(sistema.matches(MARCA_SISTEMA).count(), 1, "o texto de sistema não chegou uma vez: {sistema:?}");
    assert!(!sistema.contains(MARCA_USUARIO), "a mensagem foi parar no texto de sistema: {sistema:?}");
    assert_eq!(usuario.concat().matches(MARCA_USUARIO).count(), 1, "{usuario:?}");
    assert!(!usuario.concat().contains(MARCA_SISTEMA), "o texto de sistema foi parar na mensagem: {usuario:?}");
    acp.kill_and_wait().await;
}

#[tokio::test]
async fn systemprompt_invalido_recusa_a_sessao() {
    let case = caso!("systemprompt_invalido_recusa_a_sessao");
    let (acp, _rx) = acp_motor(novo_store());
    acp.request("initialize", json!({})).await.unwrap();
    let params =
        json!({"cwd": case.dir, "mcpServers": [], "systemPrompt": {"mode": "append", "content": [{"type": "image"}]}});
    assert!(acp.request("session/new", params).await.is_err(), "objeto inválido não pode abrir sessão sem instruções");
    acp.kill_and_wait().await;
}

/// Servidor `memory` de mentira que responde `browser_status` com `isError` (o formato da ponte real) e cuja
/// listagem ofereceria as ferramentas de página: falha também aqui não pode virar relistagem.
const SERVIDOR_MEMORY_STATUS_ERRO_FERRAMENTA: &str = r"
import readline from 'node:readline';
const rl = readline.createInterface({ input: process.stdin });
let negociado = false;
const status = { name: 'browser_status', description: 'status', inputSchema: { type: 'object' } };
const abrir = { name: 'browser_open', description: 'abre', inputSchema: { type: 'object' } };
rl.on('line', (line) => {
  const m = JSON.parse(line);
  if (m.id === undefined) return;
  const reply = (result) => console.log(JSON.stringify({ jsonrpc: '2.0', id: m.id, result }));
  if (m.method === 'initialize') reply({ protocolVersion: '2025-06-18', capabilities: { tools: {} }, serverInfo: { name: 'fake', version: '1' } });
  else if (m.method === 'tools/list') reply({ tools: negociado ? [status, abrir] : [status] });
  else reply({ content: [{ type: 'text', text: 'sem navegador' }], isError: true });
});
";

#[tokio::test]
async fn browser_status_com_iserror_tambem_interrompe_a_relistagem() {
    let case = caso!("browser_status_com_iserror_tambem_interrompe_a_relistagem");
    let mcp = memory_com_servidor(&case, SERVIDOR_MEMORY_STATUS_ERRO_FERRAMENTA).await;
    let antes: Vec<String> = mcp.specs().into_iter().map(|spec| spec.name).collect();
    assert_eq!(antes, ["mcp__memory__browser_status"]);
    assert!(mcp.refresh_browser().await.is_err(), "falha entregue como isError virou relistagem normal");
    let depois: Vec<String> = mcp.specs().into_iter().map(|spec| spec.name).collect();
    assert_eq!(depois, antes);
    assert!(mcp.classify("mcp__memory__browser_open").is_none());
}
