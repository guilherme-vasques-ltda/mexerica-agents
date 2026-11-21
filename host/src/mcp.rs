// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Built-in stdio MCP servers: `chat` sends the user messages, reacts to messages and changes the bot's name and instructions; `team` discovers and asks other bots; `manage`
//! creates bots and changes any bot's setup, and `computer` operates the desktop, when enabled. Calls go through authenticated loopback
//! HTTP to the running host, which owns delegation and screen control.

use anyhow::Result;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

mod remote;
pub use remote::serve_remote;

pub const PROTOCOL_VERSION: &str = crate::fork::conectores::mcp_version::PREFERIDA.as_str();

#[derive(Clone, Copy)]
pub enum Server {
    Chat,
    Connectors,
    Routines,
    Computer,
    Computers,
    Team,
    Manage,
    Memory,
    Tasks,
}

const INSTRUCTIONS: &str = "Operate this computer's apps like a person would, in the background where you can: \
find the window (list_windows, list_apps, launch_app), look at it (get_window_state: its controls and a screenshot), \
act on a control by its element_token, then look again. Use pixel coordinates only for what has no control. \
Never type a password yourself: use type_login with a saved login (request_login asks the user for one; request_login_code asks for a two-factor code, typed with field \"code\"). \
Referências {{senha:...}} ainda são inertes: type_login recusa a marca {{senha: em login, mesmo malformada. O caminho protegido e a coleta privada estão indisponíveis; não contorne a recusa. \
Don't approve payments: ask the user to do that from their phone.";

/// Serves MCP on stdio until the agent closes it.
pub async fn serve(bot: String, port: u16, server: Server) -> Result<()> {
    let token = crate::fork::ponte_credencial::ler_token()?;
    let token: Arc<str> = token.trim().into();
    let bot: Arc<str> = bot.into();
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    // Requests run side by side (an ask_bot can take minutes); their replies share stdout.
    let (replies, mut outbox) = mpsc::unbounded_channel::<Value>();
    tokio::spawn(async move {
        let mut out = tokio::io::stdout();
        while let Some(msg) = outbox.recv().await {
            let mut line = serde_json::to_vec(&msg)?;
            line.push(b'\n');
            out.write_all(&line).await?;
            out.flush().await?;
        }
        anyhow::Ok(())
    });
    let (name, instructions, mut available_tools) = match server {
        Server::Connectors => (
            "codync-connectors",
            crate::market::requests::INSTRUCTIONS,
            if std::env::var(crate::guard_hub::tool_origin::SESSION_SLOT_ENV)
                .is_ok_and(|s| s.starts_with("conversation:"))
            {
                crate::market::requests::context_tools()
            } else {
                crate::market::requests::tools()
            },
        ),
        // Listed by the host: they depend on this computer's driver.
        Server::Computer => ("codync-computer", INSTRUCTIONS, Value::Null),
        Server::Computers => {
            ("donnabot-computers", crate::computers::tools::INSTRUCTIONS, crate::computers::tools::tools())
        }
        Server::Routines => (
            "codync-routines",
            crate::routines::INSTRUCTIONS,
            crate::routines::tools_for(std::env::var(crate::guard_hub::tool_origin::SESSION_SLOT_ENV).ok().as_deref()),
        ),
        Server::Chat => {
            let mut tools = crate::chat::outbox::tools();
            // G1b: o cartão de escolha só é anunciado na conversa capturada; o Hub confere de novo.
            let genui = captured_conversation();
            if let Some(list) = tools.as_array_mut() {
                list.extend(crate::chat::profile::tools());
                list.push(crate::chat::reactions::tool());
                list.push(crate::fork::owner_question::tool());
                if genui {
                    list.push(crate::fork::genui::contract::descriptor());
                }
                list.retain(crate::fork::bot_editor::authorization::retain_tool);
            }
            let instructions = if genui {
                CHAT_GENUI_INSTRUCTIONS.as_str()
            } else {
                crate::fork::owner_question::CHAT_INSTRUCTIONS.as_str()
            };
            ("codync-chat", instructions, tools)
        }
        Server::Team => ("codync-team", crate::chat::team::INSTRUCTIONS, crate::chat::team::tools()),
        Server::Manage => ("codync-manage", crate::chat::manage::INSTRUCTIONS, crate::chat::manage::tools()),
        Server::Memory => ("codync-memory", crate::chat::memory::INSTRUCTIONS, json!([])),
        Server::Tasks => ("codync-tasks", crate::fork::tarefas::INSTRUCTIONS, crate::fork::tarefas::tools()),
    };
    add_instruction_guide(server, &mut available_tools);
    // Envelope G1b: nonce desta ponte + id JSON-RPC do motor; o modelo controla só `arguments`.
    let nonce: Arc<str> = uuid::Uuid::new_v4().to_string().into();
    // O catálogo `memory` muda uma vez: quando o navegador v3 é negociado pelo vínculo. O cliente é avisado uma vez.
    let announced = Arc::new(std::sync::atomic::AtomicBool::new(false));
    // Requests still running, by id: a cancelled one stops, and its host call with it (a team change's card expires).
    let mut running = std::collections::HashMap::<String, tokio::task::AbortHandle>::new();
    while let Some(line) = lines.next_line().await? {
        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(id) = msg.get("id").filter(|id| !id.is_null()).cloned() else {
            if msg["method"] == "notifications/cancelled"
                && let Some(request) = running.remove(&msg["params"]["requestId"].to_string())
            {
                request.abort();
            }
            continue; // other notifications (`notifications/initialized`…)
        };
        running.retain(|_, request| !request.is_finished());
        let key = id.to_string();
        let (replies, token, bot, available_tools) =
            (replies.clone(), token.clone(), bot.clone(), available_tools.clone());
        let announced = Arc::clone(&announced);
        let invocation = format!("{nonce}:{key}");
        let request = tokio::spawn(async move {
            let params = &msg["params"];
            let reply = match msg["method"].as_str().unwrap_or_default() {
                "initialize" => {
                    // C6: na extra selada, a orientação de escolha só com o sim atual do Hub.
                    let instructions = if sealed_session(server) && session_catalog(port, &token, &bot).await {
                        CHAT_GENUI_INSTRUCTIONS.as_str()
                    } else {
                        instructions
                    };
                    crate::fork::conectores::mcp_version::Perfil::Builtin
                        .oferta(&params["protocolVersion"])
                        .map(|version| {
                            json!({
                                "protocolVersion": version.as_str(),
                                "capabilities": {"tools": {"listChanged": matches!(server, Server::Memory)}},
                                "serverInfo": {"name": name, "version": env!("CARGO_PKG_VERSION")},
                                "instructions": instructions,
                            })
                        })
                        .map_err(crate::fork::conectores::mcp_version::Erro::json)
                }
                "ping" => Ok(json!({})),
                "tools/list" => match server {
                    Server::Computer => Ok(computer_tools(port, &token).await),
                    Server::Memory => memory_tools(port, &token, &bot).await,
                    Server::Chat if sealed_session(server) => {
                        Ok(session_tools(available_tools, session_catalog(port, &token, &bot).await))
                    }
                    _ => Ok(json!({"tools": available_tools})),
                },
                "tools/call" if params["name"] == "ajuda_instrucoes" && instruction_guide(server).is_some() => {
                    let guide = format!("{instructions}\n{}", instruction_guide(server).unwrap_or_default());
                    Ok(json!({"content":[{"type":"text","text":guide}]}))
                }
                "tools/call" => Ok(call_invoked(port, &token, &bot, params, server, &invocation).await),
                method => Err(json!({"code": -32601, "message": format!("unknown method {method}")})),
            };
            let negotiated = matches!(server, Server::Memory)
                && msg["method"] == "tools/call"
                && params["name"] == crate::fork::browser::STATUS_TOOL
                && reply.as_ref().is_ok_and(browser_negotiated);
            let _ = replies.send(match reply {
                Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
                Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
            });
            if negotiated && !announced.swap(true, std::sync::atomic::Ordering::SeqCst) {
                let _ = replies.send(json!({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"}));
            }
        });
        running.insert(key, request.abort_handle());
    }
    // The agent is gone: replies still pending have no one to read them.
    Ok(())
}

/// A resposta de `browser_status` diz que o navegador v3 está disponível: a partir daí o catálogo do Hub inclui as
/// ferramentas de página e o cliente precisa listar de novo.
fn browser_negotiated(result: &Value) -> bool {
    result["isError"] != true
        && result["content"][0]["text"]
            .as_str()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .is_some_and(|status| status["version"] == 3 && status["status"] == "available")
}

fn instruction_guide(server: Server) -> Option<&'static str> {
    match server {
        Server::Connectors => Some(crate::market::requests::INSTRUCTIONS_GUIDE),
        Server::Routines => Some(crate::routines::INSTRUCTIONS_GUIDE),
        _ => None,
    }
}

fn add_instruction_guide(server: Server, available_tools: &mut Value) {
    if instruction_guide(server).is_some()
        && let Some(tools) = available_tools.as_array_mut()
    {
        tools.push(json!({"name":"ajuda_instrucoes", "description":"Leia o restante das instruções deste servidor antes de usar suas ferramentas.", "inputSchema":{"type":"object","properties":{},"additionalProperties":false}}));
    }
}

/// Anúncio depende do motor efetivo revalidado no Hub. Recusa do Hub (vínculo revogado, chamada vencida) fecha a
/// lista; falha da própria ponte (Hub fora do ar, resposta ilegível) ou resposta fora do formato é erro JSON-RPC,
/// para o cliente manter a lista que já tinha em vez de ler o erro como "sem ferramentas".
async fn memory_tools(port: u16, token: &str, bot: &str) -> Result<Value, Value> {
    let unavailable =
        |why: &str| json!({"code": -32603, "message": format!("catálogo de ferramentas indisponível: {why}")});
    let params = json!({"name":"__tools","arguments":{}});
    let result = send(port, token, bot, &params, Server::Memory, None)
        .await
        .map_err(|_| unavailable("a ponte não alcançou o Hub"))?;
    if result["isError"] == true {
        return Ok(json!({"tools": []}));
    }
    let tools = result["content"][0]["text"]
        .as_str()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|v| v.get("tools").cloned())
        .filter(Value::is_array)
        .ok_or_else(|| unavailable("resposta fora do formato"))?;
    Ok(json!({"tools":tools}))
}

/// The `computer` tools the host offers; none when it can't be reached.
async fn computer_tools(port: u16, token: &str) -> Value {
    let res = crate::http()
        .post(format!("http://127.0.0.1:{port}/api/computerTools"))
        .bearer_auth(token)
        .json(&json!({}))
        .timeout(Duration::from_secs(30))
        .send()
        .await;
    match res {
        Ok(r) if r.status().is_success() => r.json::<Value>().await.unwrap_or_else(|_| json!({"tools": []})),
        _ => json!({"tools": []}),
    }
}

static CHAT_GENUI_INSTRUCTIONS: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    format!("{} {}", crate::fork::owner_question::CHAT_INSTRUCTIONS.as_str(), crate::fork::genui::flow::INSTRUCTIONS)
});

/// C6: ponte `chat` de uma extra com selo do Hub. Só decide se PERGUNTA ao Hub; o prefixo e o env não anunciam.
fn sealed_session(server: Server) -> bool {
    matches!(server, Server::Chat)
        && std::env::var(crate::guard_hub::tool_origin::SESSION_SLOT_ENV).is_ok_and(|s| s.starts_with("session:"))
        && std::env::var(crate::fork::sessoes::contexto_ferramenta::SEAL_ENV).is_ok()
}

/// Catálogo autenticado da extra, a cada `initialize`/`tools/list` (sem guardar o sim). Recusa, erro ou Hub
/// indisponível: `false`.
pub(crate) async fn session_catalog(port: u16, token: &str, bot: &str) -> bool {
    let params = json!({"name": crate::fork::sessoes::contexto_ferramenta::CATALOG, "arguments": {}});
    let result = call(port, token, bot, &params, Server::Chat).await;
    result["isError"] != true
        && result["content"][0]["text"]
            .as_str()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .is_some_and(|v| v["available"] == true)
}

pub(crate) fn session_tools(mut tools: Value, available: bool) -> Value {
    if available && let Some(list) = tools.as_array_mut() {
        list.push(crate::fork::genui::contract::descriptor());
    }
    json!({"tools": tools})
}

/// Ponte de uma conversa capturada: slot `conversation:` e selo de ferramenta do Hub.
fn captured_conversation() -> bool {
    std::env::var(crate::guard_hub::tool_origin::SESSION_SLOT_ENV).is_ok_and(|s| s.starts_with("conversation:"))
        && std::env::var(crate::fork::spaces::tool_context::BINDING_ENV).is_ok()
}

/// `tools/call` com identidade de invocação no envelope. Componentes e criação
/// de rotinas guardam recibo; um reenvio de transporte não duplica seus efeitos.
async fn call_invoked(port: u16, token: &str, bot: &str, params: &Value, server: Server, invocation: &str) -> Value {
    let choice = matches!(server, Server::Chat) && params["name"] == crate::fork::genui::flow::TOOL;
    let creation = matches!(server, Server::Routines)
        && params["name"] == "save_routine"
        && params["arguments"].get("id").is_none();
    if !choice && !creation {
        return call(port, token, bot, params, server).await;
    }
    match send(port, token, bot, params, server, Some(invocation)).await {
        Err(_) => send(port, token, bot, params, server, Some(invocation)).await.unwrap_or_else(|error| error),
        Ok(result) => result,
    }
}

/// One tool call through the host. Failures are tool errors the agent can read, not protocol errors.
async fn call(port: u16, token: &str, bot: &str, params: &Value, server: Server) -> Value {
    send(port, token, bot, params, server, None).await.unwrap_or_else(|error| error)
}

/// Montagem pura: só `browser_status` dispensa o metadado D62 no servidor memory.
/// Campos enviados pelo motor nessa consulta permanecem sujeitos ao decoder fechado do Hub.
fn memory_arguments(server: Server, params: &Value, capability: Option<&str>) -> Value {
    let mut args = params.get("arguments").filter(|a| a.is_object()).cloned().unwrap_or_else(|| json!({}));
    if matches!(server, Server::Memory) && params["name"] != "browser_status" {
        if let Some(capability) = capability {
            args[crate::fork::memory_auto::transport::ARG] = capability.into();
        } else if let Some(args) = args.as_object_mut() {
            args.remove(crate::fork::memory_auto::transport::ARG);
        }
    }
    args
}

/// `Err` só quando o Hub não respondeu (transporte); recusa do Hub volta como `Ok` com `isError`.
async fn send(
    port: u16,
    token: &str,
    bot: &str,
    params: &Value,
    server: Server,
    invocation: Option<&str>,
) -> Result<Value, Value> {
    let memory_transport = if matches!(server, Server::Memory) && params["name"] != "browser_status" {
        std::env::var(crate::fork::memory_auto::transport::ENV).ok()
    } else {
        None
    };
    let mut body = json!({
        "botId": bot,
        "name": params["name"],
        "arguments": memory_arguments(server, params, memory_transport.as_deref()),
    });
    if let Ok(space) = std::env::var("DONNABOT_SPACE_ID") {
        body["spaceId"] = space.into();
    }
    if let Ok(slot) = std::env::var(crate::guard_hub::tool_origin::SESSION_SLOT_ENV) {
        body["sessionSlot"] = slot.into();
    }
    if let Ok(binding) = std::env::var(crate::fork::sessoes::bindings::ENV) {
        body["sessionBinding"] = binding.into();
    }
    if let Ok(binding) = std::env::var(crate::fork::spaces::tool_context::BINDING_ENV) {
        body["toolBinding"] = binding.into();
    }
    if let Ok(seal) = std::env::var(crate::fork::sessoes::contexto_ferramenta::SEAL_ENV) {
        body[crate::fork::sessoes::contexto_ferramenta::BODY] = seal.into();
    }
    // Nunca copiado de `params`: só a ponte gera a invocação.
    if let Some(invocation) = invocation {
        body[crate::fork::genui::flow::INVOCATION] = invocation.into();
    }
    let (method, timeout) = match server {
        Server::Connectors => ("connectorCall", Duration::from_secs(90)),
        Server::Routines => ("routineCall", Duration::from_secs(30)),
        Server::Computer => ("computerCall", crate::screen::fork_permission::WAIT + Duration::from_secs(60)),
        Server::Computers => ("computersCall", Duration::from_secs(265)),
        Server::Chat => ("chatCall", Duration::from_secs(if params["name"] == "send_file" { 70 } else { 30 })),
        Server::Team => ("teamCall", crate::chat::team::ASK_TIMEOUT + Duration::from_secs(30)),
        Server::Manage => ("manageCall", crate::chat::manage::APPROVAL_TIMEOUT + Duration::from_secs(30)),
        Server::Memory => ("memoryCall", Duration::from_secs(120)),
        Server::Tasks => ("taskCall", Duration::from_secs(30)),
    };
    let res = crate::http()
        .post(format!("http://127.0.0.1:{port}/api/{method}"))
        .bearer_auth(token)
        .json(&body)
        .timeout(timeout)
        .send()
        .await;
    let error = match res {
        Ok(r) if r.status() == reqwest::StatusCode::UNAUTHORIZED => {
            "chamada vencida: credencial da ponte inválida ou revogada".to_owned()
        }
        Ok(r) => {
            let ok = r.status().is_success();
            match r.json::<Value>().await {
                Ok(v) if ok => {
                    return Ok(match server {
                        Server::Computer | Server::Computers if v["isError"] == true => {
                            json!({"content": v["content"], "isError": true})
                        }
                        Server::Computer | Server::Computers => json!({"content": v["content"]}),
                        Server::Chat
                        | Server::Team
                        | Server::Manage
                        | Server::Memory
                        | Server::Routines
                        | Server::Connectors
                        | Server::Tasks => {
                            json!({"content": [{"type": "text", "text": v.to_string()}]})
                        }
                    });
                }
                Ok(v) => v["error"].as_str().unwrap_or("the Mexerica host refused the call").to_owned(),
                Err(e) => return Err(failure(&format!("bad response from the Mexerica host: {e}"))),
            }
        }
        Err(e) => return Err(failure(&format!("can't reach the Mexerica host: {e}"))),
    };
    Ok(failure(&error))
}

fn failure(error: &str) -> Value {
    json!({"content": [{"type": "text", "text": error}], "isError": true})
}

/// O fork medeia stdio antes de qualquer retorno externo chegar ao motor.
pub async fn serve_local(connector: String, port: u16, context: String) -> Result<()> {
    crate::guard_hub::mcp::stdio::serve(connector, port, context).await
}

#[cfg(test)]
#[path = "guard_hub/mcp/internal_tests.rs"]
mod tests;

#[cfg(test)]
mod catalogo_do_navegador_tests {
    use super::*;

    fn answer(status: &Value, is_error: bool) -> Value {
        json!({"content": [{"type": "text", "text": status.to_string()}], "isError": is_error})
    }

    #[test]
    fn so_a_resposta_v3_disponivel_avisa_o_cliente_para_listar_de_novo() {
        assert!(browser_negotiated(&answer(&json!({"version": 3, "status": "available"}), false)));
        for refusal in [
            answer(&json!({"version": 3, "status": "unavailable"}), false),
            answer(&json!({"version": 2, "status": "available"}), false),
            answer(&json!({"version": 3, "status": "available"}), true),
            json!({"content": [{"type": "text", "text": "isto não é JSON"}], "isError": false}),
            json!({"isError": false}),
            Value::Null,
        ] {
            assert!(!browser_negotiated(&refusal), "{refusal}");
        }
    }

    async fn hub_que_responde(status: u16, body: Value) -> (u16, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let status = axum::http::StatusCode::from_u16(status).unwrap();
        let router = axum::Router::new()
            .route("/api/{method}", axum::routing::post(move || async move { (status, axum::Json(body)) }));
        (port, tokio::spawn(async move { axum::serve(listener, router).await.unwrap() }))
    }

    #[tokio::test]
    async fn falha_da_ponte_vira_erro_e_so_a_recusa_do_hub_fecha_a_lista() {
        let (port, hub) = hub_que_responde(200, json!({"tools": []})).await;
        let vazia = memory_tools(port, "token", "bot").await;
        hub.abort();
        assert_eq!(vazia.unwrap(), json!({"tools": []}), "lista vazia legítima continua valendo");

        for (status, corpo) in [(500, json!({"error": "recusado"})), (401, json!({"error": "vencida"}))] {
            let (port, hub) = hub_que_responde(status, corpo.clone()).await;
            let result = memory_tools(port, "token", "bot").await;
            hub.abort();
            assert_eq!(result.unwrap(), json!({"tools": []}), "{status}: a recusa do Hub fecha a lista");
        }

        for (status, corpo) in [(200, json!({"nao_e_lista": true})), (200, json!({"tools": "x"}))] {
            let (port, hub) = hub_que_responde(status, corpo.clone()).await;
            let result = memory_tools(port, "token", "bot").await;
            hub.abort();
            let error = result.expect_err(&format!("{status} {corpo} virou lista vazia válida"));
            assert_eq!(error["code"], -32603);
        }

        let inalcancavel = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = inalcancavel.local_addr().unwrap().port();
        drop(inalcancavel);
        assert!(memory_tools(port, "token", "bot").await.is_err(), "ponte fora do ar virou lista vazia válida");
    }
}
