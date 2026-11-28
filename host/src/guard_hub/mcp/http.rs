//! Sessão única de transporte remoto. A ponte usa agora; o Hub pode reutilizar depois.
//! Nada é entregue sem a mediação fornecida pelo chamador; não autoriza efeitos.
use super::entrada::MAX_FRAME;
use crate::fork::conectores::mcp_version::{self, Erro, Perfil, Versao};
use anyhow::{Result, anyhow, bail};
use reqwest::{Client, Response, Url, header::HeaderMap};
use serde_json::{Value, json};
use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};

#[path = "http_eventos.rs"]
mod eventos;
use eventos::{Events, Legacy, Rate, SharedRate};

#[derive(Debug)]
pub(crate) struct PolicyDenied;
impl std::fmt::Display for PolicyDenied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Chamada MCP recusada: regra ou política indisponível")
    }
}
impl std::error::Error for PolicyDenied {}

pub(crate) struct Target {
    pub url: Url,
    pub headers: HeaderMap,
}

pub(crate) trait Mediation {
    fn begin(&mut self) {}
    fn request_frame(&mut self, _frame: &Value) {}
    fn session_id(&mut self, _id: &str) {}
    fn private_url(&mut self, _url: &Url) {}
    async fn target(&mut self, method: &str, stale: bool) -> Result<Target>;
    async fn observe(&mut self, frame: Value, method: &str) -> Result<Option<Value>>;
    /// Sem espera humana aqui: último CAS imediatamente antes de pollar send.
    fn before_send(&mut self, _frame: &Value, _method: &str) -> Result<()> {
        Ok(())
    }
    /// Admissão final no Hub, imediatamente antes de o POST existir: a revogação feita depois da decisão do alvo
    /// ainda barra o envio. Recusa com `PolicyDenied`.
    async fn admit(&mut self, _frame: &Value, _method: &str) -> Result<()> {
        Ok(())
    }
    fn correlated_result(&mut self, _frame: &Value, _method: &str) -> Result<()> {
        Ok(())
    }
    fn post_timeout(&self) -> Duration {
        Duration::from_secs(60)
    }
    fn exchange_timeout(&self) -> Duration {
        Duration::from_secs(90)
    }
}

pub(crate) struct Session<M> {
    mediation: M,
    client: Client,
    mcp_id: Option<String>,
    origin: Option<Url>,
    legacy: Option<Legacy>,
    negotiated: bool,
    // Versão aceita após mediação; separada da prontidão (ACK/exchange concluído).
    version: Option<Versao>,
    // Lida do frame original correlacionado antes da projeção; nunca texto remoto.
    candidate: Option<Result<Versao, Erro>>,
    reverse_endpoint: Option<Url>,
    budget: Option<Budget>,
    rate: SharedRate,
    failure: Failure,
}

/// Falha de um pedido não implica retry; Fatal e Incompatible exigem encerrar o transporte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    Recoverable,
    Denied,
    Fatal,
    /// Versão do initialize inválida/não suportada, já mediada: erro técnico fixo.
    Incompatible(Erro),
}

#[derive(Default)]
struct Budget {
    bytes: usize,
    frames: usize,
    reverse: usize,
}

impl Budget {
    fn charge(&mut self, bytes: usize, frames: usize, reverse: usize) -> Result<()> {
        self.bytes = self.bytes.saturating_add(bytes);
        self.frames += frames;
        self.reverse += reverse;
        if self.bytes > 256 * 1024 || self.frames > 64 || self.reverse > 8 {
            bail!("Orçamento do pedido MCP excedido");
        }
        Ok(())
    }
}

fn is_sse(response: &Response) -> bool {
    response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| t.split(';').next().is_some_and(|t| t.trim().eq_ignore_ascii_case("text/event-stream")))
}

fn same_origin(base: &Url, destination: &Url) -> Result<()> {
    if !matches!(destination.scheme(), "http" | "https")
        || base.origin() != destination.origin()
        || !destination.username().is_empty()
        || destination.password().is_some()
        || destination.fragment().is_some()
    {
        bail!("Origem do transporte MCP divergente; credenciais retidas");
    }
    Ok(())
}

async fn bytes(mut response: Response, rate: &SharedRate) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| anyhow!("Corpo MCP indisponível"))? {
        if body.len() + chunk.len() > MAX_FRAME {
            bail!("Corpo MCP acima de 64 KiB; conteúdo retido");
        }
        eventos::charge(rate, chunk.len(), 0, 0)?;
        body.extend_from_slice(&chunk);
    }
    if !body.is_empty() {
        eventos::charge(rate, 0, 1, 0)?;
    }
    Ok(body)
}

pub(crate) fn method(frame: &Value) -> Result<&str> {
    if !frame.is_object() || frame["jsonrpc"] != "2.0" {
        bail!("Pedido MCP inválido ou batch não admitido");
    }
    let method = frame["method"].as_str().ok_or_else(|| anyhow!("Método MCP obrigatório"))?;
    if !matches!(
        method,
        "initialize"
            | "ping"
            | "tools/list"
            | "tools/call"
            | "resources/list"
            | "resources/templates/list"
            | "resources/read"
            | "prompts/list"
            | "prompts/get"
            | "notifications/initialized"
    ) {
        bail!("Método MCP sem mediação disponível");
    }
    if method == "notifications/initialized" {
        if frame.get("id").is_some() {
            bail!("Notificação MCP inválida");
        }
    } else if !frame
        .get("id")
        .is_some_and(|id| id.is_i64() || id.is_u64() || id.as_str().is_some_and(|s| s.len() <= 128))
    {
        bail!("Pedido MCP exige ID inteiro ou texto de até 128 bytes");
    }
    if method == "initialize" && !frame["params"].is_object() {
        bail!("Parâmetros initialize inválidos");
    }
    Ok(method)
}

impl<M: Mediation> Session<M> {
    pub(crate) fn mediation_mut(&mut self) -> &mut M {
        &mut self.mediation
    }

    pub fn new(mediation: M) -> Result<Self> {
        // Não seguir redirects: nenhum destino derivado recebe Authorization por inferência.
        let client = Client::builder().redirect(reqwest::redirect::Policy::none()).build()?;
        Ok(Self {
            mediation,
            client,
            mcp_id: None,
            origin: None,
            legacy: None,
            negotiated: false,
            version: None,
            candidate: None,
            reverse_endpoint: None,
            budget: None,
            rate: Arc::new(Mutex::new(Rate::default())),
            failure: Failure::Recoverable,
        })
    }

    async fn target(&mut self, method: &str, stale: bool) -> Result<Target> {
        let target = self.mediation.target(method, stale).await.inspect_err(|_| {
            self.failure = Failure::Fatal;
        })?;
        same_origin(self.origin.as_ref().unwrap_or(&target.url), &target.url).inspect_err(|_| {
            self.failure = Failure::Fatal;
        })?;
        self.origin.get_or_insert_with(|| target.url.clone());
        Ok(target)
    }

    async fn retained(&mut self, text: &str) -> Result<()> {
        self.observe(json!({"method":"invalid","params":{"retained":text}}), "invalid").await?;
        Ok(())
    }

    async fn observe(&mut self, frame: Value, method: &str) -> Result<Option<Value>> {
        self.mediation.observe(frame, method).await.inspect_err(|_| {
            self.failure = Failure::Fatal;
        })
    }

    fn charge(&mut self, bytes: usize, frames: usize, reverse: usize) -> Result<()> {
        if let Some(budget) = &mut self.budget {
            budget.charge(bytes, frames, reverse).inspect_err(|_| {
                self.failure = Failure::Fatal;
            })?;
        }
        Ok(())
    }

    async fn discard_bytes(&mut self, body: &[u8]) -> Result<()> {
        let empty = body.iter().all(u8::is_ascii_whitespace);
        self.charge(body.len(), usize::from(!empty), 0)?;
        if empty {
            return Ok(());
        }
        let frame = serde_json::from_slice::<Value>(body)
            .unwrap_or_else(|_| json!({"method":"invalid","params":{"retainedBytes":body.len()}}));
        self.observe(frame, "invalid").await?;
        Ok(())
    }

    async fn discard(&mut self, response: Response) -> Result<()> {
        let body = bytes(response, &self.rate).await.inspect_err(|_| {
            self.failure = Failure::Fatal;
        })?;
        self.discard_bytes(&body).await
    }

    async fn reverse(&mut self, frame: &Value) -> Result<()> {
        let previous = self.failure;
        self.failure = Failure::Fatal;
        self.charge(0, 0, 1)?;
        eventos::charge(&self.rate, 0, 0, 1).inspect_err(|_| {
            self.failure = Failure::Fatal;
        })?;
        let id = &frame["id"];
        if !(id.is_i64() || id.is_u64() || id.as_str().is_some_and(|s| s.len() <= 128)) {
            bail!("ID de pedido reverso inválido");
        }
        let response = if frame["method"] == "ping" {
            json!({"jsonrpc":"2.0","id":id,"result":{}})
        } else {
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Pedido iniciado pelo servidor indisponível"}})
        };
        let target = self.target("ping", false).await?;
        let endpoint = self.reverse_endpoint.as_ref().unwrap_or(&target.url);
        same_origin(&target.url, endpoint)?;
        // Antes da versão aceita, sem header; depois, a aceita mesmo antes do ACK.
        let request = self
            .client
            .post(endpoint.clone())
            .headers(self.headers(&target.headers)?)
            .header("accept", "application/json, text/event-stream")
            .json(&response)
            .timeout(Duration::from_secs(10));
        let response = request.send().await.map_err(|_| anyhow!("Resposta de protocolo MCP indisponível"))?;
        let ok = response.status().is_success();
        self.discard(response).await?;
        if !ok {
            bail!("Resposta de protocolo MCP recusada");
        }
        self.failure = previous;
        Ok(())
    }

    async fn frame(&mut self, frame: Value, id: Option<&Value>, method: &str) -> Result<Option<Value>> {
        let correlated = frame.is_object()
            && frame["jsonrpc"] == "2.0"
            && frame.get("method").is_none()
            && id.is_some_and(|id| frame.get("id") == Some(id))
            && (frame.get("result").is_some() ^ frame.get("error").is_some());
        let kind = if correlated {
            method
        } else if super::notification_method(&frame).is_some() {
            "notification"
        } else if frame.get("method").is_some() && frame.get("id").is_some() {
            "server-request"
        } else {
            "invalid"
        };
        // Também observar erros, notificações, batch/ID forjado e pedidos reversos,
        // mas só a resposta do pedido real pode alcançar o consumidor.
        if correlated {
            self.mediation.correlated_result(&frame, method)?;
            if method == "initialize" && frame.get("result").is_some() {
                self.candidate = Some(Perfil::BridgeLegacy.resultado(&frame["result"]));
            }
        }
        let reverse = (kind == "server-request").then(|| frame.clone());
        let observed = self.observe(frame, kind).await?;
        if let Some(frame) = reverse {
            self.reverse(&frame).await?;
        }
        if correlated {
            let observed = observed.ok_or_else(|| anyhow!("Retorno MCP retido pela mediação"))?;
            if method == "initialize" {
                self.accept(&observed)?;
            }
            return Ok(Some(observed));
        }
        Ok(None)
    }

    /// Só depois de Guard/recibo/#125: aceita a versão se a projeção coincide com o candidato.
    fn accept(&mut self, observed: &Value) -> Result<()> {
        match self.candidate.take() {
            None => Ok(()), // Erro JSON-RPC remoto não negocia versão.
            Some(Ok(version)) if observed["result"]["protocolVersion"] == version.as_str() => {
                self.version = Some(version);
                Ok(())
            }
            Some(Ok(_)) => {
                self.failure = Failure::Incompatible(Erro::RespostaInvalida);
                bail!(Erro::RespostaInvalida.message())
            }
            Some(Err(error)) => {
                self.failure = Failure::Incompatible(error);
                bail!(error.message())
            }
        }
    }

    fn headers(&self, base: &HeaderMap) -> Result<HeaderMap> {
        mcp_version::headers(base, self.version, self.mcp_id.as_deref())
    }

    async fn body_bytes(&mut self, body: &[u8], id: Option<&Value>, method: &str, ack: bool) -> Result<Option<Value>> {
        if body.iter().all(u8::is_ascii_whitespace) {
            self.charge(body.len(), 0, 0)?;
            return Ok(None);
        }
        let Ok(frame) = serde_json::from_slice::<Value>(body) else {
            // ACK textual não é resultado nem erro: reter/observar e aguardar o GET.
            self.discard_bytes(body).await?;
            if ack {
                return Ok(None);
            }
            bail!("Corpo MCP inválido; conteúdo retido");
        };
        self.charge(body.len(), 1, 0)?;
        if frame.is_array() {
            self.frame(frame, None, "invalid").await?;
            bail!("Batch MCP remoto não admitido");
        }
        self.frame(frame, id, method).await
    }

    async fn body(&mut self, response: Response, id: Option<&Value>, method: &str) -> Result<Option<Value>> {
        let Ok(body) = bytes(response, &self.rate).await else {
            self.failure = Failure::Fatal;
            self.retained("limite ou falha de leitura HTTP").await?;
            bail!("Corpo MCP retido");
        };
        self.body_bytes(&body, id, method, id.is_none()).await
    }

    async fn stream(&mut self, mut events: Events, id: Option<&Value>, method: &str) -> Result<Option<Value>> {
        while let Some(event) = events.next().await.inspect_err(|_| {
            self.failure = Failure::Fatal;
        })? {
            if let Some(reply) = self.event(&event, id, method).await? {
                return Ok(Some(reply));
            }
        }
        Ok(None)
    }

    async fn event(&mut self, event: &str, id: Option<&Value>, method: &str) -> Result<Option<Value>> {
        let text = eventos::text(event);
        self.charge(event.len(), usize::from(!text.is_empty()), 0)?;
        if text.is_empty() {
            return Ok(None);
        }
        match serde_json::from_str::<Value>(&text) {
            Ok(frame) if !frame.is_array() => self.frame(frame, id, method).await,
            _ => {
                self.retained("frame SSE inválido ou batch não admitido").await?;
                bail!("Frame SSE retido");
            }
        }
    }

    async fn open_legacy(&mut self, target: &Target) -> Result<()> {
        self.failure = Failure::Fatal;
        let response = self
            .client
            .get(target.url.clone())
            .headers(self.headers(&target.headers)?)
            .header("accept", "text/event-stream")
            .send()
            .await
            .map_err(|_| anyhow!("SSE legado indisponível"))?;
        if !response.status().is_success() || !is_sse(&response) {
            self.body(response, None, "invalid").await?;
            bail!("SSE legado recusado; sem redirecionamento");
        }
        let mut events = Events::new(response, self.rate.clone());
        while let Some(event) = events.next().await? {
            if eventos::endpoint(&event) {
                self.charge(event.len(), 1, 0)?;
                // Endpoint é dado externo retido, nunca mensagem de instrução.
                self.retained("endpoint SSE recebido").await?;
                let endpoint =
                    target.url.join(eventos::text(&event).trim()).map_err(|_| anyhow!("Endpoint SSE inválido"))?;
                same_origin(&target.url, &endpoint)?;
                self.mediation.private_url(&endpoint);
                self.reverse_endpoint = Some(endpoint.clone());
                self.legacy = Some(Legacy::start(endpoint, events));
                return Ok(());
            }
            self.event(&event, None, "invalid").await?;
        }
        bail!("SSE terminou antes de declarar endpoint");
    }

    pub fn has_legacy(&self) -> bool {
        self.legacy.is_some()
    }

    pub fn close(&mut self) {
        self.legacy = None;
    }

    // Somente recv: cancelar este futuro não remove nem perde uma mensagem do canal.
    pub async fn next_legacy(&mut self) -> Result<String> {
        let received = match self.legacy.as_mut() {
            Some(legacy) => legacy.receiver.recv().await,
            None => std::future::pending().await,
        };
        self.failure = Failure::Fatal;
        received.ok_or_else(|| anyhow!("SSE legado encerrado"))?
    }

    pub async fn idle(&mut self, event: String) -> Result<()> {
        self.budget = None;
        self.failure = Failure::Fatal;
        tokio::time::timeout(Duration::from_secs(30), self.event(&event, None, "invalid"))
            .await
            .map_err(|_| anyhow!("Prazo da mediação idle excedido"))??;
        Ok(())
    }

    async fn legacy_io<F, T>(
        &mut self,
        task: F,
        id: Option<&Value>,
        method: &str,
        held: &mut Option<Value>,
    ) -> Result<T>
    where
        F: Future<Output = Result<T>>,
    {
        tokio::pin!(task);
        loop {
            tokio::select! {
                result = &mut task => return result,
                event = self.next_legacy() => {
                    let event = event?;
                    let reply = self.event(&event, if held.is_none() { id } else { None }, method).await?;
                    if held.is_none() { *held = reply; }
                }
            }
        }
    }

    async fn legacy(
        &mut self,
        target: Target,
        frame: &Value,
        id: Option<&Value>,
        method: &str,
    ) -> Result<Option<Value>> {
        // Falha após POST legado pode deixar stream/correlação incertos: encerra, sem retry.
        self.failure = Failure::Fatal;
        let endpoint = self.legacy.as_ref().ok_or_else(|| anyhow!("Sessão SSE ausente"))?.endpoint.clone();
        same_origin(&target.url, &endpoint)?;
        let request = self
            .client
            .post(endpoint)
            .headers(self.headers(&target.headers)?)
            .header("accept", "application/json, text/event-stream")
            .json(frame)
            .timeout(self.mediation.post_timeout());
        let mut held = None;
        let send = request.send();
        tokio::pin!(send);
        self.mediation.admit(frame, method).await?;
        self.mediation.before_send(frame, method)?;
        // Pollar o mesmo futuro imediatamente após o CAS, antes de observar GET.
        // Mesmo se o GET ficar pronto primeiro depois, nenhum futuro/frame se perde.
        let first = futures::poll!(&mut send);
        let response = self
            .legacy_io(
                async move {
                    match first {
                        std::task::Poll::Ready(result) => result,
                        std::task::Poll::Pending => send.await,
                    }
                    .map_err(|_| anyhow!("POST SSE indisponível; não repetir automaticamente"))
                },
                id,
                method,
                &mut held,
            )
            .await?;
        let ok = response.status().is_success();
        if is_sse(&response) {
            let mut events = Events::new(response, self.rate.clone());
            loop {
                let event = self.legacy_io(events.next(), if ok { id } else { None }, method, &mut held).await?;
                let Some(event) = event else { break };
                let reply = self.event(&event, if ok && held.is_none() { id } else { None }, method).await?;
                if held.is_none() {
                    held = reply;
                }
                if held.is_some() {
                    break;
                }
            }
        } else {
            let rate = self.rate.clone();
            let body = self.legacy_io(bytes(response, &rate), if ok { id } else { None }, method, &mut held).await?;
            if ok {
                let reply = self.body_bytes(&body, if held.is_none() { id } else { None }, method, true).await?;
                if held.is_none() {
                    held = reply;
                }
            } else {
                self.discard_bytes(&body).await?;
            }
        }
        if !ok {
            bail!("POST SSE recusado; conteúdo retido, sem repetição");
        }
        if held.is_some() || id.is_none() {
            return Ok(held);
        }
        loop {
            let event = self.next_legacy().await?;
            if let Some(reply) = self.event(&event, id, method).await? {
                return Ok(Some(reply));
            }
        }
    }

    async fn exchange_inner(&mut self, mut frame: Value) -> Result<Option<Value>> {
        let method = method(&frame)?.to_owned();
        if !self.negotiated && method != "initialize" {
            bail!("Initialize MCP obrigatório");
        }
        if self.negotiated && method == "initialize" {
            bail!("Sessão MCP já inicializada");
        }
        self.mediation.request_frame(&frame);
        let client_id = frame.get("id").cloned();
        let wire_id = client_id.as_ref().map(|_| json!(uuid::Uuid::new_v4().to_string()));
        if let Some(id) = &wire_id {
            frame["id"] = id.clone();
        }
        if method == "initialize" {
            let offer = match Perfil::BridgeLegacy.oferta(&frame["params"]["protocolVersion"]) {
                Ok(offer) => offer,
                // Pedido local inválido: -32602 sem rede, sem fabricar versão.
                Err(error) => return Ok(Some(json!({"jsonrpc":"2.0","id":client_id,"error":error.json()}))),
            };
            frame["params"]["protocolVersion"] = json!(offer.as_str());
            frame["params"]["capabilities"] = json!({});
        }
        let mut target = self.target(&method, false).await?;
        let reply = if self.legacy.is_some() {
            self.legacy(target, &frame, wire_id.as_ref(), &method).await?
        } else {
            let mut refresh = false;
            loop {
                let request = self
                    .client
                    .post(target.url.clone())
                    .headers(self.headers(&target.headers)?)
                    .header("accept", "application/json, text/event-stream")
                    .json(&frame)
                    .timeout(self.mediation.post_timeout());
                self.mediation.admit(&frame, &method).await?;
                self.mediation.before_send(&frame, &method)?;
                let response =
                    request.send().await.map_err(|_| anyhow!("HTTP MCP indisponível; não repetir automaticamente"))?;
                let status = response.status();
                if method == "initialize" && !refresh && status.as_u16() == 401 {
                    self.discard(response).await?;
                    target = self.target(&method, true).await?;
                    refresh = true;
                    continue;
                }
                // Só 404/405 no initialize inicial abrem o SSE legado, uma vez; 400 nunca.
                if method == "initialize" && matches!(status.as_u16(), 404 | 405) {
                    self.discard(response).await?;
                    target = self.target(&method, false).await?;
                    self.open_legacy(&target).await?;
                    target = self.target(&method, false).await?;
                    break self.legacy(target, &frame, wire_id.as_ref(), &method).await?;
                }
                if !status.is_success() {
                    self.discard(response).await?;
                    bail!("HTTP MCP recusado; conteúdo retido, sem repetição ou redirect");
                }
                if let Some(session) = response.headers().get("mcp-session-id") {
                    let session = session
                        .to_str()
                        .ok()
                        .filter(|s| s.len() <= 256)
                        .ok_or_else(|| anyhow!("Identificador de sessão MCP inválido"))?;
                    self.mediation.session_id(session);
                    self.mcp_id = Some(session.to_owned());
                }
                break if is_sse(&response) {
                    self.stream(Events::new(response, self.rate.clone()), wire_id.as_ref(), &method).await?
                } else {
                    self.body(response, wire_id.as_ref(), &method).await?
                };
            }
        };
        if reply.is_some() {
            // O GET pode ter sido observado antes de o ACK do POST terminar.
            // Revalidar o runtime evita entregar a projeção retida após revogação.
            self.target(&method, false).await.map_err(|error| {
                if error.is::<PolicyDenied>() {
                    anyhow!("Resultado retido após alteração da política; confira o efeito")
                } else {
                    error
                }
            })?;
        }
        if client_id.is_some() && reply.is_none() {
            bail!("MCP terminou sem resposta correlacionada");
        }
        if method == "initialize" && reply.as_ref().is_some_and(|r| r.get("result").is_some()) {
            if self.version.is_none() {
                self.failure = Failure::Fatal;
                bail!("Versão MCP não aceita pela mediação");
            }
            self.negotiated = true;
        }
        Ok(reply.map(|mut reply| {
            reply["id"] = client_id.unwrap_or(Value::Null);
            reply
        }))
    }

    pub async fn exchange(&mut self, frame: Value) -> std::result::Result<Option<Value>, Failure> {
        self.mediation.begin();
        self.failure = Failure::Recoverable;
        self.budget = Some(Budget::default());
        let result = tokio::time::timeout(self.mediation.exchange_timeout(), async {
            let result = self.exchange_inner(frame).await;
            let failure = self.failure;
            self.budget = None;
            if result.is_err() && !self.negotiated {
                // ACK/mediação/revalidação falhou: nada de reaproveitar candidato ou versão.
                self.version = None;
                self.candidate = None;
            }
            if let Ok(reply) = result {
                Ok(reply)
            } else if result.as_ref().err().is_some_and(anyhow::Error::is::<PolicyDenied>) {
                Err(Failure::Denied)
            } else {
                self.retained("falha de transporte ou mediação; retorno não liberado")
                    .await
                    .map_err(|_| Failure::Fatal)?;
                Err(failure)
            }
        })
        .await;
        self.budget = None;
        result.unwrap_or(Err(Failure::Fatal))
    }
}

#[cfg(test)]
#[path = "http_tests.rs"]
mod tests;
