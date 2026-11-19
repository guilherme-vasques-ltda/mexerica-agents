//! Contrato de ferramentas entre o Hub e o motor (combinado com a frente Marketplace).
//!
//! - Hub→motor `set_tools {tools:[{name, description, inputSchema, origin}]}`:
//!   substitui a lista inteira e vai antes do prompt.
//! - motor→Hub `tool_request {callId, name, input}`: o motor NÃO executa nada.
//! - Hub→motor `tool_result {callId, ok, output|error}`, inclusive permissão negada.
//!
//! O executor ([`ToolRunner`]) é do Hub e roda pelo caminho de ferramentas/MCP
//! que já existe para os outros motores, com as permissões e aprovações dele.

use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;

/// Quem oferece a ferramenta ao usuário (aparece na descrição que o modelo vê).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ToolOrigin {
    Hub,
    Connector { account_label: Option<String> },
}

#[derive(Clone, Debug)]
pub(crate) struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub origin: ToolOrigin,
}

impl ToolSpec {
    /// Forma do fio: `{name, description, inputSchema, origin:{kind, accountLabel?}}`.
    pub(crate) fn wire(&self) -> Value {
        let origin = match &self.origin {
            ToolOrigin::Hub => json!({"kind": "hub"}),
            ToolOrigin::Connector { account_label: None } => json!({"kind": "connector"}),
            ToolOrigin::Connector { account_label: Some(label) } => {
                json!({"kind": "connector", "accountLabel": label})
            }
        };
        json!({"name": self.name, "description": self.description, "inputSchema": self.input_schema, "origin": origin})
    }
}

/// Resultado de uma chamada: `Ok(saída)` ou `Err(motivo)` (inclusive permissão negada).
pub(crate) type ToolReply = Result<String, String>;
pub(crate) type ToolFuture<'a> = Pin<Box<dyn Future<Output = ToolReply> + Send + 'a>>;

/// Executa uma chamada de ferramenta pelo caminho do Hub.
pub(crate) trait ToolRunner: Send + Sync {
    /// `call_id` é o id da chamada no turno (o executor pode correlacionar
    /// aprovação e saída); `name` é o nome declarado por `set_tools`.
    fn call<'a>(&'a self, call_id: &'a str, name: &'a str, input: Value) -> ToolFuture<'a>;
}

/// Sem executor: toda chamada é recusada (turno de conversa pura).
pub(crate) struct NoTools;

impl ToolRunner for NoTools {
    fn call<'a>(&'a self, _call_id: &'a str, _name: &'a str, _input: Value) -> ToolFuture<'a> {
        Box::pin(async { Err("este turno não tem ferramentas".to_owned()) })
    }
}

/// Teto da saída devolvida ao motor (o motor também trunca).
pub(crate) const MAX_OUTPUT: usize = 256 * 1024;

pub(crate) fn cap(mut text: String) -> String {
    if text.len() > MAX_OUTPUT {
        let mut end = MAX_OUTPUT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n[saída truncada pelo Hub]");
    }
    text
}
