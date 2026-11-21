//! Servidor MCP local da ponte, só para o `--permission-prompt-tool` documentado do modo sem tela: o programa
//! oficial pergunta aqui antes de usar uma ferramenta que pede aprovação, e a ponte leva a pergunta ao Hub como
//! `session/request_permission`. Escuta só em `127.0.0.1`, numa porta livre, e exige um segredo aleatório por
//! processo no cabeçalho; sem ele, nada é respondido. O segredo vive só na memória da ponte e no arquivo de
//! configuração 0600 que o programa lê; não é credencial de conta.

use super::opcoes::FERRAMENTA_PERMISSAO;
use anyhow::Result;
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Decide um pedido `{tool_name, input, tool_use_id}` e devolve `{behavior, ...}` no formato documentado.
pub(super) type Decisor = Arc<dyn Fn(Value) -> Pin<Box<dyn Future<Output = Value> + Send>> + Send + Sync>;

pub(super) struct Servidor {
    pub url: String,
    pub segredo: String,
    tarefa: tokio::task::JoinHandle<()>,
}

impl Drop for Servidor {
    fn drop(&mut self) {
        self.tarefa.abort();
    }
}

struct Estado {
    segredo: String,
    decisor: Decisor,
}

fn iguais(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0_u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub(super) async fn iniciar(decisor: Decisor) -> Result<Servidor> {
    let segredo = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    let estado = Arc::new(Estado { segredo: segredo.clone(), decisor });
    let app = Router::new().route("/mcp", post(pedido).get(sem_fluxo).delete(sem_fluxo)).with_state(estado);
    let ouvinte = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let url = format!("http://127.0.0.1:{}/mcp", ouvinte.local_addr()?.port());
    let tarefa = tokio::spawn(async move {
        let _ = axum::serve(ouvinte, app).await;
    });
    Ok(Servidor { url, segredo, tarefa })
}

async fn sem_fluxo() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}

fn resposta(id: &Value, corpo: Value) -> Response {
    let mut msg = json!({"jsonrpc": "2.0", "id": id});
    if corpo.get("code").is_some() && corpo.get("message").is_some() {
        msg["error"] = corpo;
    } else {
        msg["result"] = corpo;
    }
    ([(header::CONTENT_TYPE, "application/json")], msg.to_string()).into_response()
}

async fn pedido(State(estado): State<Arc<Estado>>, cabecalhos: HeaderMap, corpo: Bytes) -> Response {
    let esperado = format!("Bearer {}", estado.segredo);
    let autorizado = cabecalhos.get(header::AUTHORIZATION).is_some_and(|v| iguais(v.as_bytes(), esperado.as_bytes()));
    if !autorizado {
        tracing::warn!("pedido de permissão sem o segredo recusado pela ponte do Claude");
        return StatusCode::UNAUTHORIZED.into_response();
    }
    // Só o programa local chama aqui, e ele não manda `Origin`; pedido vindo de página de navegador é recusado.
    if cabecalhos.contains_key(header::ORIGIN) {
        tracing::warn!("pedido de permissão com Origin recusado pela ponte do Claude");
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(msg) = serde_json::from_slice::<Value>(&corpo) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if !msg.is_object() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some(id) = msg.get("id").filter(|id| !id.is_null()).cloned() else {
        return StatusCode::ACCEPTED.into_response();
    };
    let params = &msg["params"];
    match msg["method"].as_str().unwrap_or_default() {
        "initialize" => {
            // Só as revisões do transporte HTTP que este servidor implementa; outra pedida recebe a mais nova delas.
            let versao = params["protocolVersion"]
                .as_str()
                .filter(|v| ["2025-06-18", "2025-03-26"].contains(v))
                .unwrap_or("2025-06-18");
            resposta(
                &id,
                json!({"protocolVersion": versao, "capabilities": {"tools": {}},
                    "serverInfo": {"name": "mexerica-ponte", "version": env!("CARGO_PKG_VERSION")}}),
            )
        }
        "ping" => resposta(&id, json!({})),
        "tools/list" => resposta(
            &id,
            // A aprovação devolve a entrada inteira; o limite documentado da ferramenta sobe para entradas grandes
            // (um `Write` de dezenas de KB) não virarem resposta inválida.
            json!({"tools": [{"name": FERRAMENTA_PERMISSAO,
                "description": "Pergunta ao dono se uma ferramenta pode rodar. Uso interno do Mexerica.",
                "_meta": {"anthropic/maxResultSizeChars": 500_000},
                "inputSchema": {"type": "object", "properties": {"tool_name": {"type": "string"},
                    "input": {"type": "object"}, "tool_use_id": {"type": "string"}}, "required": ["tool_name", "input"]}}]}),
        ),
        "tools/call" if params["name"] == FERRAMENTA_PERMISSAO => {
            let decisao = (estado.decisor)(params["arguments"].clone()).await;
            resposta(&id, json!({"content": [{"type": "text", "text": decisao.to_string()}]}))
        }
        _ => resposta(&id, json!({"code": -32601, "message": "método não suportado"})),
    }
}
