//! CLI stdio fina sobre a sessão HTTP/SSE compartilhada e os RPCs Guard existentes.
use super::{
    http::{Failure, Mediation, Session, Target},
    stdio::{Frame, Frames},
};
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use zeroize::Zeroizing;

struct Bridge {
    http: crate::fork::bridge_http::BridgeHttp,
    token: Zeroizing<String>,
    connector: String,
    context: String,
    id: String,
    acquired: bool,
    request: String,
    begin: bool,
    sequence: u64,
    params: Value,
}

impl Bridge {
    async fn call(&self, action: &str, extra: Value) -> Result<Value> {
        let mut body = json!({"connector":self.connector,"context":self.context,
            "transport":"http","bridgeId":self.id,"requestId":self.request,"requestSeq":self.sequence});
        body.as_object_mut()
            .ok_or_else(|| anyhow!("Corpo Guard inválido"))?
            .extend(extra.as_object().ok_or_else(|| anyhow!("Parâmetros Guard inválidos"))?.clone());
        let response = self
            .http
            .post(action)
            .bearer_auth(self.token.as_str())
            .json(&body)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|_| anyhow!("Hub indisponível; conteúdo retido"))?;
        if !response.status().is_success() {
            bail!("Hub recusou mediação; conteúdo retido");
        }
        response.json().await.map_err(|_| anyhow!("Recibo Guard inválido; conteúdo retido"))
    }
}

impl Mediation for Bridge {
    fn request_frame(&mut self, frame: &Value) {
        self.params = frame["params"].clone();
    }
    fn begin(&mut self) {
        self.sequence = self.sequence.saturating_add(1);
        self.request = uuid::Uuid::new_v4().to_string();
        self.begin = true;
    }
    async fn target(&mut self, method: &str, stale: bool) -> Result<Target> {
        let target = self
            .call(
                "donnaGuardRuntime",
                json!({"method":method,"stale":stale,"refreshLease":self.acquired,"beginRequest":self.begin,"params":self.params}),
            )
            .await?;
        if target["ruleDenied"] == true {
            return Err(super::http::PolicyDenied.into());
        }
        self.acquired = true;
        self.begin = false;
        let url = reqwest::Url::parse(target["url"].as_str().ok_or_else(|| anyhow!("Alvo MCP ausente"))?)
            .map_err(|_| anyhow!("Alvo MCP inválido"))?;
        let mut headers = reqwest::header::HeaderMap::new();
        for (key, value) in target["headers"].as_object().ok_or_else(|| anyhow!("Headers MCP ausentes"))? {
            headers.insert(
                reqwest::header::HeaderName::from_bytes(key.as_bytes())
                    .map_err(|_| anyhow!("Nome de header MCP inválido"))?,
                reqwest::header::HeaderValue::from_str(value.as_str().ok_or_else(|| anyhow!("Header MCP inválido"))?)
                    .map_err(|_| anyhow!("Valor de header MCP inválido"))?,
            );
        }
        let streamable_only = target["streamableOnly"].as_bool().ok_or_else(|| anyhow!("Perfil HTTP MCP ausente"))?;
        Ok(Target { url, headers, streamable_only })
    }

    async fn observe(&mut self, frame: Value, method: &str) -> Result<Option<Value>> {
        let result =
            self.call("donnaGuardObserve", json!({"frame":frame,"method":method,"params":self.params})).await?;
        Ok(result.get("message").filter(|m| m.is_object()).cloned())
    }

    async fn admit(&mut self, _frame: &Value, method: &str) -> Result<()> {
        if method != "tools/call" {
            return Ok(());
        }
        let result =
            self.call("donnaGuardRuntime", json!({"method":method,"validateOnly":true,"params":self.params})).await?;
        if result["ruleDenied"] == true {
            return Err(super::http::PolicyDenied.into());
        }
        if result["valid"] != true {
            bail!("Admissão final recusada; pedido não enviado");
        }
        Ok(())
    }
}

async fn write(output: &mut (impl AsyncWrite + Unpin), frame: &Value) -> Result<()> {
    let mut bytes = serde_json::to_vec(frame)?;
    bytes.push(b'\n');
    tokio::time::timeout(Duration::from_secs(5), async {
        output.write_all(&bytes).await?;
        output.flush().await
    })
    .await
    .map_err(|_| anyhow!("Saída MCP bloqueada"))??;
    Ok(())
}

fn failure(id: &Value, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32603,"message":message}})
}

enum Work {
    Request(Value),
    Event(String),
}

// Um exchange ou uma observação idle; um único slot de entrada, sem fila de efeitos.
// A operação escolhida fica viva enquanto select lê stdin: nenhum frame observado se perde.
pub(super) async fn relay<M: Mediation>(
    input: impl AsyncRead + Unpin,
    mut output: impl AsyncWrite + Unpin,
    session: &mut Session<M>,
) -> Result<()> {
    let mut input = Frames::with_limit(input, 4 * 1024 * 1024);
    let mut queued = None;
    loop {
        let work = if let Some(frame) = queued.take() {
            Work::Request(frame)
        } else {
            tokio::select! {
                incoming = input.read(&[]) => match incoming? {
                    None => return Ok(()),
                    Some(Frame::Message(frame)) => Work::Request(frame),
                    Some(_) => {
                        write(&mut output, &failure(&Value::Null, "Entrada MCP inválida ou acima de 4 MiB; batch não admitido")).await?;
                        continue;
                    }
                },
                event = session.next_legacy(), if session.has_legacy() => match event {
                    Ok(event) => Work::Event(event),
                    Err(_) => return Ok(()),
                }
            }
        };
        let (id, can_queue) = match &work {
            Work::Event(_) => (None, true),
            Work::Request(frame) => {
                if frame.get("id").is_none() && frame["method"] != "notifications/initialized" {
                    continue; // Notificação sem pedido pendente nunca recebe erro id:null.
                }
                if super::http::method(frame).is_err() {
                    if let Some(id) = frame.get("id") {
                        write(
                            &mut output,
                            &failure(id, "Método, ID ou pedido MCP inválido; chamada sem mediação recusada"),
                        )
                        .await?;
                    }
                    continue;
                }
                (frame.get("id").cloned(), frame.get("id").is_none())
            }
        };
        let operation = async {
            match work {
                Work::Request(frame) => session.exchange(frame).await,
                Work::Event(event) => session.idle(event).await.map(|()| None).map_err(|_| Failure::Fatal),
            }
        };
        tokio::pin!(operation);
        loop {
            tokio::select! {
                incoming = input.read(&[]) => {
                    match incoming? {
                        None => return Ok(()),
                        Some(Frame::Message(next)) if next["method"] == "notifications/cancelled"
                            && (id.as_ref().is_some_and(|id| Some(id) == next["params"].get("requestId"))
                                || queued.as_ref().is_some_and(|frame: &Value| frame.get("id") == next["params"].get("requestId"))) => {
                            return Ok(());
                        }
                        Some(Frame::Message(next)) if next.get("id").is_none() => {}
                        Some(Frame::Message(next)) if can_queue && queued.is_none()
                            && super::http::method(&next).is_ok() => {
                            queued = Some(next);
                        }
                        Some(Frame::Message(next)) => {
                            write(&mut output, &failure(&next["id"], "Um pedido MCP já está pendente; ID repetido ou fila não admitidos")).await?;
                        }
                        Some(_) => {
                            write(&mut output, &failure(&Value::Null, "Entrada MCP inválida; fila não admitida")).await?;
                        }
                    }
                }
                result = &mut operation => {
                    match result {
                        Ok(Some(reply)) => write(&mut output, &reply).await?,
                        Ok(None) => {}
                        Err(disposition) => {
                            if let Some(id) = &id {
                                let message = match disposition {
                                    // Recibo/#125 já gravados; só o erro técnico fixo, sem valor remoto.
                                    Failure::Incompatible(error) => error.message(),
                                    Failure::Denied => "Chamada MCP recusada: regra ou política indisponível",
                                    _ => "Retorno MCP retido; confira o efeito antes de nova tentativa. A ponte não repete o pedido.",
                                };
                                write(&mut output, &failure(id, message)).await?;
                            }
                            if !matches!(disposition, Failure::Recoverable | Failure::Denied) {
                                return Ok(());
                            }
                        }
                    }
                    break;
                }
            }
        }
    }
}

pub async fn serve(connector: String, port: u16, context: String) -> Result<()> {
    let token = Zeroizing::new(if std::env::var_os(crate::fork::ponte_credencial::SETUP_ENV).is_some() {
        // R9: esta ponte de descoberta é iniciada pelo Hub, sem sessão de bot.
        tokio::fs::read_to_string(crate::service::data_dir().join("token")).await?
    } else {
        crate::fork::ponte_credencial::ler_token()?
    });
    let bridge = Bridge {
        http: crate::fork::bridge_http::BridgeHttp::new(port)?,
        token: Zeroizing::new(token.trim().to_owned()),
        connector,
        context,
        id: uuid::Uuid::new_v4().to_string(),
        acquired: false,
        request: uuid::Uuid::new_v4().to_string(),
        begin: true,
        sequence: 0,
        params: Value::Null,
    };
    // Clone só dos identificadores: liberar mesmo se EOF cancelar uma aquisição em voo.
    let release = Bridge {
        http: bridge.http.clone(),
        token: bridge.token.clone(),
        connector: bridge.connector.clone(),
        context: bridge.context.clone(),
        id: bridge.id.clone(),
        acquired: false,
        request: uuid::Uuid::new_v4().to_string(),
        begin: true,
        sequence: 0,
        params: Value::Null,
    };
    let mut session = Session::new(bridge)?;
    let result = relay(tokio::io::stdin(), tokio::io::stdout(), &mut session).await;
    session.close();
    let _ = release.call("donnaGuardRelease", json!({})).await;
    result
}

#[cfg(test)]
#[path = "ponte_remota_context_tests.rs"]
mod context_tests;
