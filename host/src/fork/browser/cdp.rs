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

const ALLOWED_METHODS: &[&str] = &[
    "Browser.getVersion",
    "Browser.setDownloadBehavior",
    "Emulation.setScriptExecutionDisabled",
    "Fetch.continueRequest",
    "Fetch.continueWithAuth",
    "Fetch.enable",
    "Fetch.failRequest",
    "Page.captureScreenshot",
    "Page.createIsolatedWorld",
    "Page.enable",
    "Page.getFrameTree",
    "Runtime.enable",
    "Runtime.evaluate",
    "Runtime.runIfWaitingForDebugger",
    "Target.attachToTarget",
    "Target.closeTarget",
    "Target.getTargets",
    "Target.setAutoAttach",
];

/// Só HTTPS (e a página em branco inicial): redirecionamento ou subrecurso em `http:`, `file:`, `data:`,
/// `chrome:` e afins nunca sai do navegador, qualquer que seja o filtro de rede abaixo.
pub(crate) fn url_allowed(url: &str) -> bool {
    url == "about:blank" || url.starts_with("https://")
}

fn read_only_request(origin: Option<&reqwest::Url>, request: &Value) -> bool {
    if !matches!(request["method"].as_str(), Some("GET" | "HEAD"))
        || request.get("hasPostData").is_some_and(|value| value.as_bool() != Some(false))
        || request.get("postData").is_some()
        || request.get("postDataEntries").is_some()
    {
        return false;
    }
    let Some(origin) = origin else { return false };
    request["url"]
        .as_str()
        .and_then(|url| super::nav::parse_navigation_url(url).ok())
        .is_some_and(|url| reqwest::Url::parse(&url.url).is_ok_and(|url| url.origin() == origin.origin()))
}

fn closed_ok(result: &Result<Value>) -> bool {
    result.as_ref().is_ok_and(|value| value.get("success").and_then(Value::as_bool) == Some(true))
}
type Pending = HashMap<u64, (String, oneshot::Sender<Result<Value>>)>;

struct Shared<W> {
    writer: AsyncMutex<W>,
    pending: Mutex<Pending>,
    next_id: AtomicU64,
    events: Mutex<Option<broadcast::Sender<Value>>>,
    auth: Mutex<Option<mpsc::Sender<Value>>>,
    closed: std::sync::atomic::AtomicBool,
    page: Mutex<Option<String>>,
    read_only: Option<Mutex<Option<reqwest::Url>>>,
    /// Uma operação de página por vez: abrir e ler não se intercalam na mesma aba.
    operation: AsyncMutex<()>,
    /// Chamado uma vez quando o canal cai por falha (não pelo corte deliberado): liga a perda do controle
    /// ao corte do proxy da instância sem passar por trava alguma.
    on_close: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    /// Acorda a escrita em andamento quando o canal é cortado.
    close_signal: tokio::sync::Notify,
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
        Self::with_mode(reader, writer, false)
    }

    pub(crate) fn read_only<R: AsyncRead + Unpin + Send + 'static>(reader: R, writer: W) -> Self {
        Self::with_mode(reader, writer, true)
    }

    fn with_mode<R: AsyncRead + Unpin + Send + 'static>(reader: R, writer: W, read_only: bool) -> Self {
        let (events, _) = broadcast::channel(256);
        let shared = Arc::new(Shared {
            writer: AsyncMutex::new(writer),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            events: Mutex::new(Some(events)),
            auth: Mutex::new(None),
            closed: false.into(),
            page: Mutex::new(None),
            read_only: read_only.then(|| Mutex::new(None)),
            operation: AsyncMutex::new(()),
            on_close: Mutex::new(None),
            close_signal: tokio::sync::Notify::new(),
        });
        let client = Self { shared };
        tokio::spawn(client.clone().read_loop(reader));
        client
    }

    /// Serializa as operações de página desta instância: quem segura a guarda é o único a navegar ou ler a aba.
    pub(crate) async fn lock_page(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.shared.operation.lock().await
    }

    /// Registra o que fazer quando o canal cair por falha. Se já caiu, roda na hora.
    pub(crate) fn on_close(&self, hook: Arc<dyn Fn() + Send + Sync>) {
        if let Ok(mut slot) = self.shared.on_close.lock()
            && !self.shared.closed.load(Ordering::Acquire)
        {
            *slot = Some(hook);
            return;
        }
        hook();
    }

    pub(crate) fn events(&self) -> Result<broadcast::Receiver<Value>> {
        let events = self.shared.events.lock().map_err(|_| anyhow!("pipe CDP indisponível"))?;
        Ok(events.as_ref().ok_or_else(|| anyhow!("pipe CDP fechado"))?.subscribe())
    }

    pub(crate) async fn command(&self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        self.command_inner(None, method, params, timeout, false, None).await
    }

    pub(crate) async fn command_session(
        &self,
        session: &str,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value> {
        self.command_inner(Some(session), method, params, timeout, false, None).await
    }

    /// Comando de página que só sai se a autoridade ainda valer no instante do envio: a conferência roda
    /// com a trava do escritor já obtida, depois de qualquer espera.
    pub(crate) async fn command_session_checked(
        &self,
        session: &str,
        method: &str,
        params: Value,
        timeout: Duration,
        authorized: &(dyn Fn() -> bool + Sync),
    ) -> Result<Value> {
        self.command_inner(Some(session), method, params, timeout, false, Some(authorized)).await
    }

    /// Única navegação existente: o tipo `ValidatedNavigation` só nasce em `nav.rs`, depois de validar a URL no Hub.
    pub(super) async fn navigate(
        &self,
        session: &str,
        target: &super::nav::ValidatedNavigation,
        authorized: &(dyn Fn() -> bool + Sync),
    ) -> Result<Value> {
        if let Some(origin) = &self.shared.read_only {
            let mut origin = origin.lock().map_err(|_| anyhow!("browser_network_policy_unavailable"))?;
            if !authorized() {
                bail!("browser_authority_revoked");
            }
            *origin = Some(reqwest::Url::parse(&target.url)?);
        }
        self.command_inner(
            Some(session),
            "Page.navigate",
            json!({"url": target.url}),
            super::nav::NAV_TIMEOUT,
            true,
            Some(authorized),
        )
        .await
    }

    /// Único criador de alvo: mesma regra de `navigate`.
    pub(super) async fn create_target(&self, target: &super::nav::ValidatedNavigation) -> Result<Value> {
        self.command_inner(None, "Target.createTarget", json!({"url": target.url}), super::nav::NAV_TIMEOUT, true, None)
            .await
    }

    /// Sessão da página principal, fixada quando a autenticação do proxy a configurou.
    pub(crate) fn main_session(&self) -> Option<String> {
        self.shared.page.lock().ok().and_then(|page| page.clone())
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.shared.closed.load(Ordering::Acquire)
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
        self.command_inner(Some(session), method, params, timeout, true, None).await
    }

    /// Corta o canal de controle na hora: nenhum comando novo sai e os pendentes falham.
    pub(crate) fn sever(&self) {
        self.close_inner(false);
    }

    /// Comando cru de nível de navegador só para os controles positivos dos testes vivos.
    #[cfg(test)]
    pub(crate) async fn command_raw_for_tests(&self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        self.command_inner(None, method, params, timeout, true, None).await
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
        Ok(self.page_target().await?.1)
    }

    /// Alvo e sessão da página principal.
    async fn page_target(&self) -> Result<(String, String)> {
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
        let session = attached
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow!("sessão de página ausente no CDP"))?;
        Ok((target_id.to_owned(), session))
    }

    async fn command_inner(
        &self,
        session: Option<&str>,
        method: &str,
        params: Value,
        timeout: Duration,
        from_gate: bool,
        authorized: Option<&(dyn Fn() -> bool + Sync)>,
    ) -> Result<Value> {
        // Navegar/criar alvo só pelo gate de URL (nav.rs), nunca cru.
        if !from_gate {
            if matches!(method, "Page.navigate" | "Target.createTarget" | "Page.navigateToHistoryEntry") {
                bail!("browser_navigation_gate_required");
            }
            if !ALLOWED_METHODS.contains(&method) {
                bail!("browser_cdp_method_denied");
            }
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
            let cut = self.shared.close_signal.notified();
            tokio::pin!(cut);
            cut.as_mut().enable();
            if self.shared.closed.load(Ordering::Acquire) {
                bail!("pipe CDP fechado");
            }
            if let Some(authorized) = authorized
                && !authorized()
            {
                bail!("browser_authority_revoked: a autoridade da sessão foi revogada");
            }
            let mut writing = WriteGuard { client: self, complete: false };
            let sent = async {
                writer.write_all(&message).await?;
                writer.flush().await
            };
            // `biased`: se o corte e a escrita estiverem prontos ao mesmo tempo, o corte vence e nada é enviado.
            tokio::select! {
                biased;
                () = &mut cut => bail!("pipe CDP fechado"),
                result = sent => result.map_err(|_| anyhow!("falha ao escrever no pipe CDP"))?,
            }
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
        let (main_target, session) = self.page_target().await?;
        *self.shared.page.lock().map_err(|_| anyhow!("pipe CDP indisponível"))? = Some(session.clone());
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
                    // não fica pausado: falha (ou auth CancelAuth) sem token.
                    tracing::warn!(method = %event["method"], "evento Fetch de sessão desconhecida recusado sem credencial");
                    if let Some(request_id) =
                        event.pointer("/params/requestId").and_then(Value::as_str).map(str::to_owned)
                    {
                        let (method, params) = if event["method"] == "Fetch.authRequired" {
                            (
                                "Fetch.continueWithAuth",
                                json!({"requestId":request_id,"authChallengeResponse":{"response":"CancelAuth"}}),
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
                let token = Arc::clone(&token);
                let request = &event["params"]["request"];
                let permitted = match &client.shared.read_only {
                    Some(origin) => origin.lock().is_ok_and(|origin| read_only_request(origin.as_ref(), request)),
                    None => url_allowed(request["url"].as_str().unwrap_or_default()),
                };
                let (method, params) = if event["method"] == "Fetch.requestPaused" && !permitted {
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
        if self.shared.read_only.is_some() {
            self.disable_scripts(&session).await?;
        }
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
                let target_id = event.pointer("/params/targetInfo/targetId").and_then(Value::as_str).map(str::to_owned);
                if target_type == "page" && target_id.as_deref() != Some(main_target.as_str()) {
                    // Popup/janela nova nunca é mantido, nem com URL aparentemente permitida: o alvo nasce
                    // sem URL e o endereço só aparece depois. O alvo nasce pausado e o `window.open` do opener só
                    // continua quando ele é retomado; por isso a retomada só acontece com o filtro de rede ligado E
                    // com a execução de scripts do alvo desligada, e o alvo é fechado logo em seguida. Se qualquer
                    // um dos dois falhar, o alvo NUNCA é retomado: o canal CDP é encerrado e a sessão inteira é
                    // descartada. Sem o fechamento confirmado a contenção também não se sustenta.
                    let Some(popup_id) = target_id else {
                        tracing::warn!("popup sem identidade de alvo; canal CDP encerrado");
                        watcher.close();
                        break;
                    };
                    let filtered = watcher
                        .command_session(&new_session, "Fetch.enable", json!({"handleAuthRequests":true}), AUTH_TIMEOUT)
                        .await
                        .is_ok();
                    let silenced = filtered
                        && watcher
                            .command_session(
                                &new_session,
                                "Emulation.setScriptExecutionDisabled",
                                json!({"value":true}),
                                AUTH_TIMEOUT,
                            )
                            .await
                            .is_ok();
                    if !silenced {
                        tracing::warn!(
                            "popup sem filtro e sem scripts desligados; alvo não retomado e canal CDP encerrado"
                        );
                        watcher.close();
                        break;
                    }
                    let _ = watcher
                        .command_session(&new_session, "Runtime.runIfWaitingForDebugger", json!({}), AUTH_TIMEOUT)
                        .await;
                    let closed = closed_ok(
                        &watcher.command("Target.closeTarget", json!({"targetId":popup_id}), AUTH_TIMEOUT).await,
                    );
                    if !closed {
                        tracing::warn!("popup sem fechamento confirmado; canal CDP encerrado");
                        watcher.close();
                        break;
                    }
                    continue;
                }
                if known.lock().map(|mut set| set.insert(new_session.clone())).is_err() {
                    watcher.close();
                    break;
                }
                // OOPIF/filhos: auto-attach recursivo a partir da sessão de
                // página/iframe, para herdar a mesma regra.
                let covered = matches!(target_type.as_str(), "page" | "iframe");
                let strict = watcher.shared.read_only.is_some();
                let mut refused = strict && (!covered || watcher.disable_scripts(&new_session).await.is_err());
                refused |= covered
                    && watcher
                        .command_session(
                            &new_session,
                            "Target.setAutoAttach",
                            json!({"autoAttach":true,"waitForDebuggerOnStart":true,"flatten":true}),
                            AUTH_TIMEOUT,
                        )
                        .await
                        .is_err();
                // Fetch pode não existir em worker: falhar aqui não pode
                // derrubar a sessão inteira (uma página hostil registra um
                // worker e mataria o canal). O proxy segue como barreira.
                let fetch_refused = watcher
                    .command_session(&new_session, "Fetch.enable", json!({"handleAuthRequests":true}), AUTH_TIMEOUT)
                    .await
                    .is_err();
                if fetch_refused && !covered {
                    tracing::warn!(target_type, "Fetch.enable recusado no alvo anexado; sessão mantida sem Fetch");
                }
                refused |= fetch_refused && (covered || strict);
                if refused {
                    // Página ou iframe sem o filtro Fetch (ou sem auto-attach) fica só com o proxy: fecha o alvo.
                    tracing::warn!(target_type, "alvo sem Fetch ou auto-attach recursivo; fechado");
                    let closed = match &target_id {
                        Some(target_id) => closed_ok(
                            &watcher.command("Target.closeTarget", json!({"targetId":target_id}), AUTH_TIMEOUT).await,
                        ),
                        None => false,
                    };
                    known.lock().map(|mut set| set.remove(&new_session)).ok();
                    if !closed {
                        tracing::warn!(target_type, "alvo sem Fetch não pôde ser fechado; canal CDP encerrado");
                        watcher.close();
                        break;
                    }
                    continue;
                }
                let _ = watcher
                    .command_session(&new_session, "Runtime.runIfWaitingForDebugger", json!({}), AUTH_TIMEOUT)
                    .await;
            }
        });
        Ok(())
    }

    async fn disable_scripts(&self, session: &str) -> Result<()> {
        self.command_session(session, "Emulation.setScriptExecutionDisabled", json!({"value":true}), AUTH_TIMEOUT)
            .await?;
        Ok(())
    }

    fn close(&self) {
        self.close_inner(true);
    }

    /// `notify` falso é o corte deliberado (quem corta já cuida do resto); verdadeiro é a queda do canal.
    fn close_inner(&self, notify: bool) {
        self.shared.closed.store(true, Ordering::Release);
        self.shared.close_signal.notify_waiters();
        if let Ok(mut pending) = self.shared.pending.lock() {
            pending.clear();
        }
        if let Ok(mut auth) = self.shared.auth.lock() {
            auth.take();
        }
        if let Ok(mut events) = self.shared.events.lock() {
            events.take();
        }
        let hook = self.shared.on_close.lock().ok().and_then(|mut slot| slot.take());
        if notify && let Some(hook) = hook {
            hook();
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

    #[test]
    fn leitura_no_mac_so_permite_get_e_head_sem_corpo_na_origem_aberta() {
        let origin = reqwest::Url::parse("https://example.invalid/pagina").unwrap();
        for method in ["GET", "HEAD"] {
            assert!(read_only_request(Some(&origin), &json!({"url":"https://example.invalid/asset","method":method})));
        }
        for request in [
            json!({"url":"https://example.invalid/","method":"POST"}),
            json!({"url":"https://example.invalid/","method":"GET","postData":"data"}),
            json!({"url":"https://example.invalid/","method":"GET","hasPostData":true}),
            json!({"url":"https://example.invalid:444/","method":"GET"}),
            json!({"url":"https://other.example.invalid/","method":"GET"}),
            json!({"url":"https://example.invalid/","method":null}),
        ] {
            assert!(!read_only_request(Some(&origin), &request), "{request}");
        }
        assert!(!read_only_request(None, &json!({"url":"https://example.invalid/","method":"GET"})));
    }

    type PipeFixture = (
        Cdp<tokio::io::WriteHalf<DuplexStream>>,
        BufReader<tokio::io::ReadHalf<DuplexStream>>,
        tokio::io::WriteHalf<DuplexStream>,
    );

    fn pair() -> PipeFixture {
        pair_mode(false)
    }

    fn pair_mode(read_only: bool) -> PipeFixture {
        let (a, b) = tokio::io::duplex(16384);
        let (read, write) = tokio::io::split(a);
        let (browser_read, browser_write) = tokio::io::split(b);
        let client = if read_only { Cdp::read_only(read, write) } else { Cdp::new(read, write) };
        (client, BufReader::new(browser_read), browser_write)
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
            async move { c.command("Browser.getVersion", json!({}), Duration::from_secs(1)).await.unwrap() }
        });
        let b = tokio::spawn({
            let c = client.clone();
            async move { c.command("Target.getTargets", json!({}), Duration::from_secs(1)).await.unwrap() }
        });
        let first = receive(&mut reader).await;
        let second = receive(&mut reader).await;
        send(&mut writer, json!({"method":"Test.event","params":{}})).await;
        send(&mut writer, json!({"id":second["id"],"result":{"method":second["method"]}})).await;
        send(&mut writer, json!({"id":first["id"],"result":{"method":first["method"]}})).await;
        assert_eq!(a.await.unwrap()["method"], "Browser.getVersion");
        assert_eq!(b.await.unwrap()["method"], "Target.getTargets");
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
        // CancelAuth (sem token) e não fica pausado para sempre.
        send(
            &mut writer,
            json!({"sessionId":"s2","method":"Fetch.authRequired","params":{"requestId":"s2-req", "authChallenge":{"source":"Proxy"}}}),
        )
        .await;
        let unknown = receive(&mut reader).await;
        assert_eq!(unknown["method"], "Fetch.continueWithAuth");
        assert_eq!(unknown["sessionId"], "s2");
        assert_eq!(unknown["params"]["authChallengeResponse"], json!({"response":"CancelAuth"}));
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
                json!({"sessionId":"s1","method":"Fetch.requestPaused","params":{"requestId":format!("r{index}"),"request":{"url":"https://a.test/"}}}),
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
        for url in ["https://a.test/", "about:blank"] {
            assert!(url_allowed(url), "{url}");
        }
        for url in [
            "http://a.test/",
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
    async fn escrita_bloqueada_e_cortada_falha_na_hora_sem_esperar_o_prazo() {
        let (a, _peer) = tokio::io::duplex(64);
        let (read, write) = tokio::io::split(a);
        let client = Cdp::new(read, write);
        let big = "x".repeat(4096);
        let sender = tokio::spawn({
            let client = client.clone();
            async move { client.command("Browser.getVersion", json!({"p": big}), Duration::from_secs(30)).await }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        client.sever();
        let result = tokio::time::timeout(Duration::from_secs(2), sender)
            .await
            .expect("o corte tem de acordar a escrita presa em vez de esperar o prazo")
            .unwrap();
        assert!(result.is_err());
        assert!(client.is_closed());
    }

    #[tokio::test]
    async fn comando_nao_sai_se_a_autoridade_sumiu_enquanto_esperava_a_trava_do_escritor() {
        let (a, b) = tokio::io::duplex(64);
        let (read, write) = tokio::io::split(a);
        let client = Cdp::new(read, write);
        let (browser_read, mut browser_write) = tokio::io::split(b);
        let mut reader = BufReader::new(browser_read);
        let big = "x".repeat(2000);
        let first = tokio::spawn({
            let client = client.clone();
            async move { client.command("Browser.getVersion", json!({"p": big}), Duration::from_secs(30)).await }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        let allowed = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let second = tokio::spawn({
            let (client, allowed) = (client.clone(), Arc::clone(&allowed));
            async move {
                let check = move || allowed.load(Ordering::SeqCst);
                client
                    .command_session_checked("s1", "Page.getFrameTree", json!({}), Duration::from_secs(30), &check)
                    .await
            }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        allowed.store(false, Ordering::SeqCst);
        let sent = receive(&mut reader).await;
        send(&mut browser_write, json!({"id":sent["id"],"result":{}})).await;
        first.await.unwrap().unwrap();
        let error = second.await.unwrap().unwrap_err().to_string();
        assert!(error.starts_with("browser_authority_revoked"), "{error}");
        let extra = tokio::time::timeout(Duration::from_millis(300), receive(&mut reader)).await;
        assert!(extra.is_err(), "o comando sem autoridade chegou ao navegador");
    }

    #[tokio::test]
    async fn perda_do_canal_dispara_o_gancho_uma_vez_e_o_corte_deliberado_nao() {
        use std::sync::atomic::AtomicUsize;
        let counter = || Arc::new(AtomicUsize::new(0));
        let hook = |count: &Arc<AtomicUsize>| {
            let count = Arc::clone(count);
            Arc::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
            }) as Arc<dyn Fn() + Send + Sync>
        };
        let (client, reader, writer) = pair();
        let lost = counter();
        client.on_close(hook(&lost));
        drop((reader, writer));
        for _ in 0..50 {
            if client.is_closed() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(client.is_closed());
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(lost.load(Ordering::SeqCst), 1, "a queda do canal tem de avisar exatamente uma vez");
        let late = counter();
        client.on_close(hook(&late));
        assert_eq!(late.load(Ordering::SeqCst), 1, "gancho registrado depois da queda roda na hora");
        let (client, _reader, _writer) = pair();
        let deliberate = counter();
        client.on_close(hook(&deliberate));
        client.sever();
        assert!(client.is_closed());
        assert_eq!(deliberate.load(Ordering::SeqCst), 0, "o corte deliberado não é uma queda");
    }

    async fn conectado() -> PipeFixture {
        conectado_no_modo(false).await
    }

    async fn conectado_no_modo(read_only: bool) -> PipeFixture {
        let (client, mut reader, mut writer) = pair_mode(read_only);
        let prover = client.clone();
        let task = tokio::spawn(async move { prover.provide_proxy_credentials("segredo-teste".into()).await.unwrap() });
        let version = receive(&mut reader).await;
        send(&mut writer, json!({"id":version["id"],"result":{"product":"Chromium/teste"}})).await;
        let targets = receive(&mut reader).await;
        send(&mut writer, json!({"id":targets["id"],"result":{"targetInfos":[{"type":"page","targetId":"t1"}]}})).await;
        let attach = receive(&mut reader).await;
        send(&mut writer, json!({"id":attach["id"],"result":{"sessionId":"s1"}})).await;
        if read_only {
            let disable = receive(&mut reader).await;
            assert_eq!(disable["method"], "Emulation.setScriptExecutionDisabled");
            assert_eq!(disable["params"]["value"], true);
            send(&mut writer, json!({"id":disable["id"],"result":{}})).await;
        }
        for expected in ["Fetch.enable", "Browser.setDownloadBehavior", "Target.setAutoAttach"] {
            let command = receive(&mut reader).await;
            assert_eq!(command["method"], expected);
            send(&mut writer, json!({"id":command["id"],"result":{}})).await;
        }
        task.await.unwrap();
        (client, reader, writer)
    }

    #[tokio::test]
    async fn protocolo_de_leitura_limita_requisicoes_e_fecha_iframe_sem_protecao() {
        let (client, mut reader, mut writer) = conectado_no_modo(true).await;
        let navigation = super::super::nav::parse_navigation_url("https://example.invalid/").unwrap();
        let turn = tokio::spawn({
            let client = client.clone();
            async move { client.navigate("s1", &navigation, &|| true).await.unwrap() }
        });
        let go = receive(&mut reader).await;
        assert_eq!(go["method"], "Page.navigate");
        send(&mut writer, json!({"id":go["id"],"result":{}})).await;
        turn.await.unwrap();
        for (method, url, expected) in [
            ("GET", "https://example.invalid/asset", "Fetch.continueRequest"),
            ("POST", "https://example.invalid/form", "Fetch.failRequest"),
            ("GET", "https://other.example.invalid/", "Fetch.failRequest"),
        ] {
            send(&mut writer, json!({"sessionId":"s1","method":"Fetch.requestPaused","params":{"requestId":"r","request":{"url":url,"method":method}}})).await;
            let answer = receive(&mut reader).await;
            assert_eq!(answer["method"], expected);
            send(&mut writer, json!({"id":answer["id"],"result":{}})).await;
        }
        send(&mut writer, json!({"method":"Target.attachedToTarget","params":{"sessionId":"s2","targetInfo":{"targetId":"iframe","type":"iframe"}}})).await;
        let disable = receive(&mut reader).await;
        assert_eq!(disable["method"], "Emulation.setScriptExecutionDisabled");
        assert_eq!(disable["sessionId"], "s2");
        send(&mut writer, json!({"id":disable["id"],"error":{"code":-1}})).await;
        for expected in ["Target.setAutoAttach", "Fetch.enable", "Target.closeTarget"] {
            let command = receive(&mut reader).await;
            assert_eq!(command["method"], expected);
            send(&mut writer, json!({"id":command["id"],"result":{"success":true}})).await;
        }
        let probe =
            tokio::spawn(async move { client.command("Target.getTargets", json!({}), AUTH_TIMEOUT).await.unwrap() });
        let command = receive(&mut reader).await;
        assert_eq!(command["method"], "Target.getTargets", "o iframe recusado não pode ser retomado");
        send(&mut writer, json!({"id":command["id"],"result":{}})).await;
        probe.await.unwrap();
    }
    fn popup(url: &str, target: Option<&str>) -> Value {
        let mut info = json!({"type":"page","url":url});
        if let Some(target) = target {
            info["targetId"] = json!(target);
        }
        json!({"method":"Target.attachedToTarget","params":{"sessionId":"s9","targetInfo":info}})
    }
    async fn atende_popup(
        reader: &mut BufReader<tokio::io::ReadHalf<DuplexStream>>,
        writer: &mut tokio::io::WriteHalf<DuplexStream>,
        close: Option<Value>,
    ) {
        let filter = receive(reader).await;
        assert_eq!((filter["method"].as_str(), filter["sessionId"].as_str()), (Some("Fetch.enable"), Some("s9")));
        send(writer, json!({"id":filter["id"],"result":{}})).await;
        let silence = receive(reader).await;
        assert_eq!(
            (silence["method"].as_str(), silence["sessionId"].as_str(), &silence["params"]["value"]),
            (Some("Emulation.setScriptExecutionDisabled"), Some("s9"), &json!(true)),
            "scripts do popup desligados antes de qualquer retomada"
        );
        send(writer, json!({"id":silence["id"],"result":{}})).await;
        let resume = receive(reader).await;
        assert_eq!(resume["method"], "Runtime.runIfWaitingForDebugger");
        assert_eq!(resume["sessionId"], "s9");
        send(writer, json!({"id":resume["id"],"result":{}})).await;
        let closing = receive(reader).await;
        assert_eq!(closing["method"], "Target.closeTarget");
        assert_eq!(closing["params"]["targetId"], "t9");
        let mut response = close.unwrap_or_else(|| json!({"result":{"success":true}}));
        response["id"] = closing["id"].clone();
        send(writer, response).await;
    }
    #[tokio::test]
    async fn popup_de_qualquer_url_tem_filtro_e_scripts_desligados_antes_de_ser_retomado_e_e_fechado() {
        let (client, mut reader, mut writer) = conectado().await;
        for url in
            ["", "http://a.test/", "https://a.test/", "file:///etc/passwd", "data:text/html,x", "chrome://settings"]
        {
            send(&mut writer, popup(url, Some("t9"))).await;
            atende_popup(&mut reader, &mut writer, None).await;
        }
        let probe = tokio::spawn({
            let client = client.clone();
            async move { client.command("Target.getTargets", json!({}), Duration::from_secs(2)).await }
        });
        let next = receive(&mut reader).await;
        assert_eq!(next["method"], "Target.getTargets", "nenhum popup ficou aberto ou foi liberado sem filtro");
        send(&mut writer, json!({"id":next["id"],"result":{"targetInfos":[]}})).await;
        probe.await.unwrap().unwrap();
        assert!(!client.is_closed());
    }
    #[tokio::test]
    async fn popup_sem_filtro_ou_sem_scripts_desligados_nunca_e_retomado() {
        for failing in ["Fetch.enable", "Emulation.setScriptExecutionDisabled"] {
            let (client, mut reader, mut writer) = conectado().await;
            send(&mut writer, popup("https://a.test/", Some("t9"))).await;
            let filter = receive(&mut reader).await;
            assert_eq!(filter["method"], "Fetch.enable");
            if failing == "Fetch.enable" {
                send(&mut writer, json!({"id":filter["id"],"error":{"code":-32000,"message":"falhou"}})).await;
            } else {
                send(&mut writer, json!({"id":filter["id"],"result":{}})).await;
                let silence = receive(&mut reader).await;
                assert_eq!(silence["method"], "Emulation.setScriptExecutionDisabled");
                send(&mut writer, json!({"id":silence["id"],"error":{"code":-32000,"message":"falhou"}})).await;
            }
            for _ in 0..50 {
                if client.is_closed() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            assert!(client.is_closed(), "{failing}: sem proteção a sessão não pode continuar");
            let extra = tokio::time::timeout(Duration::from_millis(200), receive(&mut reader)).await;
            assert!(extra.is_err(), "{failing}: o popup foi retomado ou fechado sem a proteção");
        }
    }
    #[tokio::test]
    async fn popup_sem_fechamento_confirmado_ou_sem_identidade_encerra_o_canal() {
        for answer in [
            Some(json!({"error":{"code":-32000,"message":"falhou"}})),
            Some(json!({"result":{"success":false}})),
            Some(json!({"result":{}})),
            None,
        ] {
            let (client, mut reader, mut writer) = conectado().await;
            match answer {
                Some(answer) => {
                    send(&mut writer, popup("http://a.test/", Some("t9"))).await;
                    atende_popup(&mut reader, &mut writer, Some(answer)).await;
                }
                None => send(&mut writer, popup("http://a.test/", None)).await,
            }
            for _ in 0..50 {
                if client.is_closed() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            assert!(client.is_closed(), "sem fechamento confirmado a sessão não pode continuar");
        }
    }
    #[tokio::test]
    async fn pagina_principal_reanexada_nao_e_fechada() {
        let (client, mut reader, mut writer) = conectado().await;
        send(&mut writer, popup("about:blank", Some("t1"))).await;
        for expected in ["Target.setAutoAttach", "Fetch.enable", "Runtime.runIfWaitingForDebugger"] {
            let command = receive(&mut reader).await;
            assert_eq!(command["method"], expected);
            assert_eq!(command["sessionId"], "s9");
            send(&mut writer, json!({"id":command["id"],"result":{}})).await;
        }
        assert!(!client.is_closed());
    }
    #[tokio::test]
    async fn redirecionamento_e_subrecurso_nao_https_falham() {
        let (client, mut reader, mut writer) = conectado().await;
        for url in ["http://a.test/", "file:///etc/passwd", "data:text/html,x", "chrome://version", "ftp://a/"] {
            send(&mut writer, json!({"sessionId":"s1","method":"Fetch.requestPaused","params":{"requestId":"r","request":{"url":url}}})).await;
            let command = receive(&mut reader).await;
            assert_eq!(command["method"], "Fetch.failRequest", "{url}");
            assert_eq!(command["params"]["errorReason"], "BlockedByClient");
            send(&mut writer, json!({"id":command["id"],"result":{}})).await;
        }
        assert!(!client.is_closed());
    }
    #[tokio::test]
    async fn mensagem_gigante_ou_invalida_fecha_pipe() {
        for bytes in [vec![b'x'; MAX_MESSAGE + 1], b"{invalido}\0".to_vec()] {
            let (client, mut reader, mut writer) = pair();
            let task =
                tokio::spawn(
                    async move { client.command("Browser.getVersion", json!({}), Duration::from_secs(5)).await },
                );
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
            client
                .command("Browser.getVersion", json!({}), Duration::from_millis(10))
                .await
                .unwrap_err()
                .to_string()
                .contains("tempo")
        );
        drop(writer);
        drop(reader);
        assert!(tokio::time::timeout(Duration::from_secs(1), events.recv()).await.unwrap().is_err());
        assert!(
            client
                .command("Target.getTargets", json!({}), Duration::from_secs(1))
                .await
                .unwrap_err()
                .to_string()
                .contains("fechado")
        );
    }
}
