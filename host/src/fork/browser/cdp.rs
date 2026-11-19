//! Transporte CDP por pipe. Ainda não habilita navegação no produto.
#![allow(dead_code)]

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    sync::{Mutex as AsyncMutex, broadcast, mpsc, oneshot},
};

const MAX_MESSAGE: usize = 8 * 1024 * 1024;
const AUTH_TIMEOUT: Duration = Duration::from_secs(10);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);

pub(crate) fn url_allowed(url: &str) -> bool {
    url == "about:blank" || url.starts_with("http://") || url.starts_with("https://")
}
type Pending = HashMap<u64, (String, oneshot::Sender<Result<Value>>)>;

struct Shared<W> {
    writer: AsyncMutex<W>,
    pending: Mutex<Pending>,
    next_id: AtomicU64,
    events: Mutex<Option<broadcast::Sender<Value>>>,
    auth: Mutex<Option<mpsc::Sender<Value>>>,
    closed: std::sync::atomic::AtomicBool,
}

pub(crate) struct Cdp<W> {
    shared: Arc<Shared<W>>,
}
impl<W> Clone for Cdp<W> {
    fn clone(&self) -> Self {
        Self { shared: self.shared.clone() }
    }
}

impl<W: AsyncWrite + Unpin + Send + 'static> Cdp<W> {
    pub(crate) fn new<R: AsyncRead + Unpin + Send + 'static>(reader: R, writer: W) -> Self {
        let (events, _) = broadcast::channel(256);
        let shared = Arc::new(Shared {
            writer: AsyncMutex::new(writer),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            events: Mutex::new(Some(events)),
            auth: Mutex::new(None),
            closed: false.into(),
        });
        let client = Self { shared };
        tokio::spawn(client.clone().read_loop(reader));
        client
    }

    pub(crate) fn events(&self) -> Result<broadcast::Receiver<Value>> {
        let events = self.shared.events.lock().map_err(|_| anyhow!("pipe CDP indisponível"))?;
        Ok(events.as_ref().ok_or_else(|| anyhow!("pipe CDP fechado"))?.subscribe())
    }

    pub(crate) async fn command(&self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        self.command_inner(None, method, params, timeout, false).await
    }

    pub(crate) async fn command_session(
        &self,
        session: &str,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value> {
        self.command_inner(Some(session), method, params, timeout, false).await
    }

    /// Caminho do gate (`nav.rs`): único autorizado a navegar/criar alvo,
    /// depois de validar a URL no Hub.
    pub(crate) async fn command_from_gate(&self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        self.command_inner(None, method, params, timeout, true).await
    }

    /// Idem `command_from_gate`, com sessão de página.
    pub(crate) async fn command_session_from_gate(
        &self,
        session: &str,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value> {
        self.command_inner(Some(session), method, params, timeout, true).await
    }

    /// Bypass cru só para os testes vivos provarem as camadas abaixo do
    /// gate; não existe no binário de produção.
    #[cfg(test)]
    pub(crate) async fn command_session_raw_for_tests(
        &self,
        session: &str,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value> {
        self.command_inner(Some(session), method, params, timeout, true).await
    }

    async fn get_version_tolerant(&self) -> Result<Value> {
        // Partida a frio do Chromium pode passar de 10 s: timeout maior e 1 retry.
        if let Ok(version) = self.command("Browser.getVersion", json!({}), HANDSHAKE_TIMEOUT).await {
            return Ok(version);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
        self.command("Browser.getVersion", json!({}), HANDSHAKE_TIMEOUT).await
    }

    /// O pipe é do navegador: comandos de página exigem sessão de um alvo.
    pub(crate) async fn page_session(&self) -> Result<String> {
        // Aperto de mão: prova que o pipe responde antes de qualquer outro comando.
        let version = self.get_version_tolerant().await?;
        tracing::info!(cdp_product = %version.get("product").and_then(|value| value.as_str()).unwrap_or("?"), "aperto de mão CDP ok");
        let targets = self.command("Target.getTargets", json!({}), AUTH_TIMEOUT).await?;
        let target_id = targets
            .get("targetInfos")
            .and_then(Value::as_array)
            .and_then(|list| list.iter().find(|target| target.get("type").and_then(Value::as_str) == Some("page")))
            .and_then(|target| target.get("targetId"))
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("alvo de página ausente no CDP"))?;
        let attached = self
            .command("Target.attachToTarget", json!({"targetId": target_id, "flatten": true}), AUTH_TIMEOUT)
            .await?;
        attached
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow!("sessão de página ausente no CDP"))
    }

    async fn command_inner(
        &self,
        session: Option<&str>,
        method: &str,
        params: Value,
        timeout: Duration,
        from_gate: bool,
    ) -> Result<Value> {
        // Navegar/criar alvo só pelo gate de URL (nav.rs), nunca cru.
        if !from_gate && matches!(method, "Page.navigate" | "Target.createTarget" | "Page.navigateToHistoryEntry") {
            bail!("browser_navigation_gate_required");
        }
        if self.shared.closed.load(Ordering::Acquire) {
            bail!("pipe CDP fechado");
        }
        let id = self.shared.next_id.fetch_add(1, Ordering::Relaxed);
        let mut payload = json!({"id": id, "method": method, "params": params});
        if let Some(session) = session {
            payload["sessionId"] = json!(session);
        }
        let mut message = serde_json::to_vec(&payload).map_err(|_| anyhow!("comando CDP inválido: {method}"))?;
        if message.len() > MAX_MESSAGE {
            bail!("comando CDP excede o limite");
        }
        message.push(0);
        tracing::debug!(method, bytes = message.len(), "CDP tx");
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.shared.pending.lock().map_err(|_| anyhow!("pipe CDP indisponível"))?;
            if self.shared.closed.load(Ordering::Acquire) {
                bail!("pipe CDP fechado");
            }
            pending.insert(id, (method.to_owned(), tx));
        }
        let guard = PendingGuard { pending: &self.shared.pending, id };
        let result = tokio::time::timeout(timeout, async {
            let mut writer = self.shared.writer.lock().await;
            if self.shared.closed.load(Ordering::Acquire) {
                bail!("pipe CDP fechado");
            }
            let mut writing = WriteGuard { client: self, complete: false };
            writer.write_all(&message).await.map_err(|_| anyhow!("falha ao escrever no pipe CDP"))?;
            writer.flush().await.map_err(|_| anyhow!("falha ao escrever no pipe CDP"))?;
            writing.complete = true;
            drop(writer);
            rx.await.map_err(|_| anyhow!("pipe CDP fechado"))?
        })
        .await
        .map_err(|_| anyhow!("tempo esgotado no CDP: {method}"))?;
        drop(guard);
        result
    }

    pub(crate) async fn provide_proxy_credentials(&self, token: String) -> Result<()> {
        let session = self.page_session().await?;
        let (tx, mut rx) = mpsc::channel::<Value>(256);
        {
            let mut auth = self.shared.auth.lock().map_err(|_| anyhow!("pipe CDP indisponível"))?;
            if auth.is_some() {
                bail!("autenticação de proxy já configurada");
            }
            *auth = Some(tx);
        }
        let client = self.clone();
        let auth_session = session.clone();
        let token: Arc<str> = token.into();
        let known: Arc<Mutex<std::collections::HashSet<String>>> =
            Arc::new(Mutex::new([auth_session.clone()].into_iter().collect()));
        let known_auth = Arc::clone(&known);
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                // Fail-closed: Fetch sem sessionId ou de sessão desconhecida
                // nunca recebe credencial; encerra o canal.
                let Some(event_session) = event.get("sessionId").and_then(Value::as_str).map(str::to_owned) else {
                    client.close();
                    break;
                };
                let is_known = known_auth.lock().is_ok_and(|set| set.contains(&event_session));
                if !is_known {
                    // Sessão desconhecida nunca recebe credencial; o pedido
                    // não fica pausado: falha (ou auth Default) sem token.
                    tracing::warn!(method = %event["method"], "evento Fetch de sessão desconhecida recusado sem credencial");
                    if let Some(request_id) =
                        event.pointer("/params/requestId").and_then(Value::as_str).map(str::to_owned)
                    {
                        let (method, params) = if event["method"] == "Fetch.authRequired" {
                            (
                                "Fetch.continueWithAuth",
                                json!({"requestId":request_id,"authChallengeResponse":{"response":"Default"}}),
                            )
                        } else {
                            ("Fetch.failRequest", json!({"requestId":request_id,"errorReason":"BlockedByClient"}))
                        };
                        let client = client.clone();
                        tokio::spawn(async move {
                            let _ = client.command_session(&event_session, method, params, AUTH_TIMEOUT).await;
                        });
                    }
                    continue;
                }
                let Some(request_id) = event.pointer("/params/requestId").and_then(Value::as_str).map(str::to_owned)
                else {
                    client.close();
                    break;
                };
                let url = event.pointer("/params/request/url").and_then(Value::as_str).unwrap_or_default().to_owned();
                let token = Arc::clone(&token);
                let (method, params) = if event["method"] == "Fetch.requestPaused" && !url_allowed(&url) {
                    ("Fetch.failRequest", json!({"requestId":request_id,"errorReason":"BlockedByClient"}))
                } else if event["method"] == "Fetch.authRequired" {
                    let response =
                        if event.pointer("/params/authChallenge/source").and_then(Value::as_str) == Some("Proxy") {
                            json!({"response":"ProvideCredentials", "username":"mx", "password": token.as_ref()})
                        } else {
                            json!({"response":"Default"})
                        };
                    ("Fetch.continueWithAuth", json!({"requestId":request_id,"authChallengeResponse":response}))
                } else {
                    ("Fetch.continueRequest", json!({"requestId":request_id}))
                };
                let client = client.clone();
                // Cada evento em sua tarefa: um continue lento não trava os demais
                // nem o laço de leitura, que segue drenando respostas.
                tokio::spawn(async move {
                    if client.command_session(&event_session, method, params, AUTH_TIMEOUT).await.is_err() {
                        client.close();
                    }
                });
            }
        });
        if let Err(error) =
            self.command_session(&session, "Fetch.enable", json!({"handleAuthRequests":true}), AUTH_TIMEOUT).await
        {
            self.close();
            return Err(error);
        }
        if let Err(error) = self.command("Browser.setDownloadBehavior", json!({"behavior":"deny"}), AUTH_TIMEOUT).await
        {
            self.close();
            return Err(error);
        }
        // Auto-attach: popups/janelas novas recebem a mesma regra por sessão
        // (Fetch com auth) e ficam pausadas até o Hub configurar a sessão.
        let mut attach_events = self.events()?;
        if let Err(error) = self
            .command(
                "Target.setAutoAttach",
                json!({"autoAttach":true,"waitForDebuggerOnStart":true,"flatten":true}),
                AUTH_TIMEOUT,
            )
            .await
        {
            self.close();
            return Err(error);
        }
        let watcher = self.clone();
        tokio::spawn(async move {
            loop {
                let event = match attach_events.recv().await {
                    Ok(event) => event,
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        // Nunca sair calado: sem watcher, popups ficam pausados.
                        tracing::warn!(skipped, "watcher CDP atrasado; fechando o canal CDP");
                        watcher.close();
                        break;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                if event["method"] == "Target.detachedFromTarget" {
                    if let Some(sid) = event.pointer("/params/sessionId").and_then(Value::as_str)
                        && let Ok(mut set) = known.lock()
                    {
                        set.remove(sid);
                    }
                    continue;
                }
                if event["method"] != "Target.attachedToTarget" {
                    continue;
                }
                let Some(new_session) = event.pointer("/params/sessionId").and_then(Value::as_str).map(str::to_owned)
                else {
                    watcher.close();
                    break;
                };
                let target_type =
                    event.pointer("/params/targetInfo/type").and_then(Value::as_str).unwrap_or_default().to_owned();
                let target_url =
                    event.pointer("/params/targetInfo/url").and_then(Value::as_str).unwrap_or_default().to_owned();
                let target_id = event.pointer("/params/targetInfo/targetId").and_then(Value::as_str).map(str::to_owned);
                if target_type == "page" && !target_url.is_empty() && !url_allowed(&target_url) {
                    if let Some(target_id) = target_id {
                        let _ =
                            watcher.command("Target.closeTarget", json!({"targetId":target_id}), AUTH_TIMEOUT).await;
                    }
                    let _ = watcher
                        .command_session(&new_session, "Runtime.runIfWaitingForDebugger", json!({}), AUTH_TIMEOUT)
                        .await;
                    continue;
                }
                if known.lock().map(|mut set| set.insert(new_session.clone())).is_err() {
                    watcher.close();
                    break;
                }
                // OOPIF/filhos: auto-attach recursivo a partir da sessão de
                // página/iframe, para herdar a mesma regra.
                if matches!(target_type.as_str(), "page" | "iframe")
                    && watcher
                        .command_session(
                            &new_session,
                            "Target.setAutoAttach",
                            json!({"autoAttach":true,"waitForDebuggerOnStart":true,"flatten":true}),
                            AUTH_TIMEOUT,
                        )
                        .await
                        .is_err()
                {
                    tracing::warn!(target_type, "auto-attach recursivo recusado na sessão anexada");
                }
                // Fetch pode não existir em worker: falhar aqui não pode
                // derrubar a sessão inteira (uma página hostil registra um
                // worker e mataria o canal). O proxy segue como barreira.
                if watcher
                    .command_session(&new_session, "Fetch.enable", json!({"handleAuthRequests":true}), AUTH_TIMEOUT)
                    .await
                    .is_err()
                {
                    tracing::warn!(target_type, "Fetch.enable recusado no alvo anexado; sessão mantida sem Fetch");
                }
                let _ = watcher
                    .command_session(&new_session, "Runtime.runIfWaitingForDebugger", json!({}), AUTH_TIMEOUT)
                    .await;
            }
        });
        Ok(())
    }

    fn close(&self) {
        self.shared.closed.store(true, Ordering::Release);
        if let Ok(mut pending) = self.shared.pending.lock() {
            pending.clear();
        }
        if let Ok(mut auth) = self.shared.auth.lock() {
            auth.take();
        }
        if let Ok(mut events) = self.shared.events.lock() {
            events.take();
        }
    }

    async fn read_loop(self, reader: impl AsyncRead + Unpin) {
        let mut reader = BufReader::new(reader);
        let result: Result<()> = async {
            loop {
                let mut message = Vec::new();
                loop {
                    let buf = reader.fill_buf().await.map_err(|_| anyhow!("falha ao ler pipe CDP"))?;
                    if buf.is_empty() {
                        bail!("pipe CDP fechado");
                    }
                    let end = buf.iter().position(|b| *b == 0);
                    let count = end.unwrap_or(buf.len());
                    if message.len() + count > MAX_MESSAGE {
                        bail!("mensagem CDP excede o limite");
                    }
                    message.extend_from_slice(&buf[..count]);
                    reader.consume(count + usize::from(end.is_some()));
                    if end.is_some() {
                        break;
                    }
                }
                let value: Value = serde_json::from_slice(&message).map_err(|_| anyhow!("mensagem CDP inválida"))?;
                if let Some(id) = value.get("id").and_then(Value::as_u64) {
                    tracing::debug!(id, bytes = message.len(), "CDP rx resposta");
                    let tx = self.shared.pending.lock().map_err(|_| anyhow!("pipe CDP indisponível"))?.remove(&id);
                    if let Some((method, sender)) = tx {
                        let response = if value.get("error").is_some() {
                            Err(anyhow!("comando CDP recusado: {method}"))
                        } else {
                            Ok(value.get("result").cloned().unwrap_or(Value::Null))
                        };
                        let _ = sender.send(response);
                    }
                } else if let Some(method) = value.get("method").and_then(Value::as_str) {
                    tracing::debug!(method, bytes = message.len(), "CDP rx evento");
                    if matches!(method, "Fetch.authRequired" | "Fetch.requestPaused") {
                        let auth = self.shared.auth.lock().map_err(|_| anyhow!("pipe CDP indisponível"))?.clone();
                        if let Some(auth) = auth
                            && auth.try_send(value.clone()).is_err()
                        {
                            // Fila cheia descarta o evento, nunca derruba o leitor.
                            tracing::warn!(method, "fila de eventos CDP cheia; evento descartado");
                        }
                    }
                    if let Some(events) =
                        self.shared.events.lock().map_err(|_| anyhow!("pipe CDP indisponível"))?.as_ref()
                    {
                        let _ = events.send(value);
                    }
                } else {
                    bail!("mensagem CDP inválida");
                }
            }
        }
        .await;
        let _ = result;
        self.close();
    }
}

// Uma escrita cancelada pode ter enviado apenas parte do JSON; não reutilize o pipe.
struct WriteGuard<'a, W: AsyncWrite + Unpin + Send + 'static> {
    client: &'a Cdp<W>,
    complete: bool,
}
impl<W: AsyncWrite + Unpin + Send + 'static> Drop for WriteGuard<'_, W> {
    fn drop(&mut self) {
        if !self.complete {
            self.client.close();
        }
    }
}

struct PendingGuard<'a> {
    pending: &'a Mutex<Pending>,
    id: u64,
}
impl Drop for PendingGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(&self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};

    fn pair() -> (
        Cdp<tokio::io::WriteHalf<DuplexStream>>,
        BufReader<tokio::io::ReadHalf<DuplexStream>>,
        tokio::io::WriteHalf<DuplexStream>,
    ) {
        let (a, b) = tokio::io::duplex(16384);
        let (read, write) = tokio::io::split(a);
        let (browser_read, browser_write) = tokio::io::split(b);
        (Cdp::new(read, write), BufReader::new(browser_read), browser_write)
    }
    async fn receive(reader: &mut BufReader<tokio::io::ReadHalf<DuplexStream>>) -> Value {
        let mut bytes = Vec::new();
        reader.read_until(0, &mut bytes).await.unwrap();
        bytes.pop();
        serde_json::from_slice(&bytes).unwrap()
    }
    async fn send(writer: &mut tokio::io::WriteHalf<DuplexStream>, message: Value) {
        let mut bytes = serde_json::to_vec(&message).unwrap();
        bytes.push(0);
        writer.write_all(&bytes).await.unwrap();
    }
    #[tokio::test]
    async fn respostas_fora_de_ordem_e_evento() {
        let (client, mut reader, mut writer) = pair();
        let mut events = client.events().unwrap();
        let a = tokio::spawn({
            let c = client.clone();
            async move { c.command("A", json!({}), Duration::from_secs(1)).await.unwrap() }
        });
        let b = tokio::spawn({
            let c = client.clone();
            async move { c.command("B", json!({}), Duration::from_secs(1)).await.unwrap() }
        });
        let first = receive(&mut reader).await;
        let second = receive(&mut reader).await;
        send(&mut writer, json!({"method":"Test.event","params":{}})).await;
        send(&mut writer, json!({"id":second["id"],"result":{"method":second["method"]}})).await;
        send(&mut writer, json!({"id":first["id"],"result":{"method":first["method"]}})).await;
        assert_eq!(a.await.unwrap()["method"], "A");
        assert_eq!(b.await.unwrap()["method"], "B");
        assert_eq!(events.recv().await.unwrap()["method"], "Test.event");
    }
    #[tokio::test]
    async fn autentica_apenas_proxy_e_continua_pedidos() {
        let (client, mut reader, mut writer) = pair();
        let task = tokio::spawn(async move {
            client.provide_proxy_credentials("segredo-teste".into()).await.unwrap();
            client
        });
        let version = receive(&mut reader).await;
        assert_eq!(version["method"], "Browser.getVersion");
        send(&mut writer, json!({"id":version["id"],"result":{"product":"Chromium/teste"}})).await;
        let targets = receive(&mut reader).await;
        assert_eq!(targets["method"], "Target.getTargets");
        send(&mut writer, json!({"id":targets["id"],"result":{"targetInfos":[{"type":"page","targetId":"t1"}]}})).await;
        let attach = receive(&mut reader).await;
        assert_eq!(attach["method"], "Target.attachToTarget");
        assert_eq!(attach["params"]["flatten"], true);
        send(&mut writer, json!({"id":attach["id"],"result":{"sessionId":"s1"}})).await;
        let enable = receive(&mut reader).await;
        assert_eq!(enable["method"], "Fetch.enable");
        assert_eq!(enable["sessionId"], "s1");
        assert_eq!(enable["params"]["handleAuthRequests"], true);
        send(&mut writer, json!({"id":enable["id"],"result":{}})).await;
        let download = receive(&mut reader).await;
        assert_eq!(download["method"], "Browser.setDownloadBehavior");
        assert_eq!(download["params"]["behavior"], "deny");
        send(&mut writer, json!({"id":download["id"],"result":{}})).await;
        let auto = receive(&mut reader).await;
        assert_eq!(auto["method"], "Target.setAutoAttach");
        assert_eq!(auto["params"]["waitForDebuggerOnStart"], true);
        send(&mut writer, json!({"id":auto["id"],"result":{}})).await;
        let _client = task.await.unwrap();
        // Evento de outra sessão não pode receber a credencial: recebe
        // auth Default (sem token) e não fica pausado para sempre.
        send(
            &mut writer,
            json!({"sessionId":"s2","method":"Fetch.authRequired","params":{"requestId":"s2-req", "authChallenge":{"source":"Proxy"}}}),
        )
        .await;
        let unknown = receive(&mut reader).await;
        assert_eq!(unknown["method"], "Fetch.continueWithAuth");
        assert_eq!(unknown["sessionId"], "s2");
        assert_eq!(unknown["params"]["authChallengeResponse"], json!({"response":"Default"}));
        send(&mut writer, json!({"id":unknown["id"],"result":{}})).await;
        for (method, source, expected) in [
            ("Fetch.authRequired", "Proxy", "Fetch.continueWithAuth"),
            ("Fetch.authRequired", "Server", "Fetch.continueWithAuth"),
            ("Fetch.requestPaused", "", "Fetch.failRequest"),
        ] {
            let event_url = if method == "Fetch.requestPaused" { "file:///etc/passwd" } else { "https://a.test/" };
            send(&mut writer, json!({"sessionId":"s1","method":method,"params":{"requestId":"r","request":{"url":event_url}, "authChallenge":{"source":source}}}))
                .await;
            let command = receive(&mut reader).await;
            assert_eq!(command["method"], expected);
            assert_eq!(command["sessionId"], "s1");
            assert_eq!(command["params"]["requestId"], "r");
            if source == "Proxy" {
                assert_eq!(
                    command["params"]["authChallengeResponse"],
                    json!({"response":"ProvideCredentials","username":"mx","password":"segredo-teste"})
                );
            }
            if source == "Server" {
                assert_eq!(command["params"]["authChallengeResponse"], json!({"response":"Default"}));
            }
            send(&mut writer, json!({"id":command["id"],"result":{}})).await;
        }
    }
    #[tokio::test]
    async fn rajada_de_pausados_nao_trava_get_targets() {
        let (client, mut reader, mut writer) = pair();
        let prover = client.clone();
        let task = tokio::spawn(async move {
            prover.provide_proxy_credentials("segredo-teste".into()).await.unwrap();
        });
        let version = receive(&mut reader).await;
        assert_eq!(version["method"], "Browser.getVersion");
        send(&mut writer, json!({"id":version["id"],"result":{"product":"Chromium/teste"}})).await;
        let targets = receive(&mut reader).await;
        send(&mut writer, json!({"id":targets["id"],"result":{"targetInfos":[{"type":"page","targetId":"t1"}]}})).await;
        let attach = receive(&mut reader).await;
        send(&mut writer, json!({"id":attach["id"],"result":{"sessionId":"s1"}})).await;
        let enable = receive(&mut reader).await;
        send(&mut writer, json!({"id":enable["id"],"result":{}})).await;
        let download = receive(&mut reader).await;
        send(&mut writer, json!({"id":download["id"],"result":{}})).await;
        let auto = receive(&mut reader).await;
        assert_eq!(auto["method"], "Target.setAutoAttach");
        send(&mut writer, json!({"id":auto["id"],"result":{}})).await;
        task.await.unwrap();
        // Rajada de pausados: o navegador falso só responde aos continues no fim,
        // e mesmo assim comandos novos precisam fluir pelo mesmo pipe.
        for index in 0..3 {
            send(
                &mut writer,
                json!({"sessionId":"s1","method":"Fetch.requestPaused","params":{"requestId":format!("r{index}"),"request":{"url":"http://a.test/"}}}),
            )
            .await;
        }
        let mut continues = Vec::new();
        for _ in 0..3 {
            continues.push(receive(&mut reader).await);
        }
        assert!(continues.iter().all(|command| command["method"] == "Fetch.continueRequest"));
        assert!(continues.iter().all(|command| command["sessionId"] == "s1"));
        for command in &continues {
            send(&mut writer, json!({"id":command["id"],"result":{}})).await;
        }
        let client2 = client.clone();
        let pending = tokio::spawn(async move {
            client2.command("Target.getTargets", json!({}), Duration::from_secs(2)).await.unwrap()
        });
        let command = receive(&mut reader).await;
        assert_eq!(command["method"], "Target.getTargets");
        send(&mut writer, json!({"id":command["id"],"result":{"targetInfos":[]}})).await;
        assert_eq!(pending.await.unwrap()["targetInfos"].as_array().unwrap().len(), 0);
    }
    #[tokio::test]
    async fn navegar_cru_e_recusado_fora_do_gate() {
        let (client, _reader, _writer) = pair();
        for method in ["Page.navigate", "Target.createTarget", "Page.navigateToHistoryEntry"] {
            let error = if method == "Target.createTarget" {
                client.command(method, json!({}), Duration::from_secs(1)).await.unwrap_err()
            } else {
                client.command_session("s1", method, json!({}), Duration::from_secs(1)).await.unwrap_err()
            };
            assert!(error.to_string().contains("browser_navigation_gate_required"), "{method}: {error}");
        }
    }
    #[test]
    fn esquemas_permitidos() {
        for url in ["http://a.test/", "https://a.test/", "about:blank"] {
            assert!(url_allowed(url), "{url}");
        }
        for url in [
            "file:///etc/passwd",
            "chrome://settings",
            "devtools://x",
            "view-source:http://a/",
            "data:text/html,x",
            "blob:http://a/1",
            "ftp://a/",
            "chrome-extension://a/",
        ] {
            assert!(!url_allowed(url), "{url}");
        }
    }
    #[tokio::test]
    async fn mensagem_gigante_ou_invalida_fecha_pipe() {
        for bytes in [vec![b'x'; MAX_MESSAGE + 1], b"{invalido}\0".to_vec()] {
            let (client, mut reader, mut writer) = pair();
            let task = tokio::spawn(async move { client.command("A", json!({}), Duration::from_secs(5)).await });
            let _ = receive(&mut reader).await;
            let _ = writer.write_all(&bytes).await;
            assert!(task.await.unwrap().is_err());
        }
    }
    #[tokio::test]
    async fn fechamento_e_timeout() {
        let (client, reader, writer) = pair();
        let mut events = client.events().unwrap();
        assert!(
            client.command("A", json!({}), Duration::from_millis(10)).await.unwrap_err().to_string().contains("tempo")
        );
        drop(writer);
        drop(reader);
        assert!(tokio::time::timeout(Duration::from_secs(1), events.recv()).await.unwrap().is_err());
        assert!(
            client.command("B", json!({}), Duration::from_secs(1)).await.unwrap_err().to_string().contains("fechado")
        );
    }
}
