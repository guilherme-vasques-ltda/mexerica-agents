//! Adaptador ACP interno (fatia 2, MEX-1367): o bot com backend `mexerica`
//! fala ACP com este objeto DENTRO do Hub — nenhum processo intermediário.
//! Cada `session/*` vira uma chamada ao [`Engine`]; cada `tool_request` do
//! motor vira `session/request_permission` para o bot, que decide pelo
//! cartão de aprovação e pelas permissões que já existem, e só então o
//! [`McpHost`] executa. O streaming sai como `session/update`;
//! `session/cancel` mata o processo do motor. Um processo do motor por
//! sessão ACP (o `rpc.ts` é uma conversa por processo).

use super::historico::{self, History, TurnLog};
use super::tools::{ToolFuture, ToolRunner};
use super::{
    Engine, VaultSource,
    mcp_host::{McpHost, ToolClass},
};
use crate::LockExt;
use crate::agent::acp::{self, Incoming};
use anyhow::{Context, Result, anyhow, bail, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinSet;

/// Estado de uma sessão ACP: o motor (tomado pelo turno em curso), o
/// executor MCP do Hub e a configuração aplicada ao (re)nascer o processo.
struct Session {
    engine: tokio::sync::Mutex<Option<Engine>>,
    mcp: Option<Arc<McpHost>>,
    provider: Mutex<String>,
    model: Mutex<String>,
    system_prompt: Mutex<Option<String>>,
    /// Pid do motor tomado pelo turno em curso (o slot do `engine` fica vazio).
    pid: Mutex<Option<u32>>,
    cancel: watch::Sender<bool>,
    /// Há um turno em curso nesta sessão: um segundo prompt é recusado antes de tocar na conversa ou no processo.
    busy: std::sync::atomic::AtomicBool,
    /// A conversa é da sessão, não do processo (e sobrevive ao adaptador: ver
    /// o módulo historico). Cancelar, trocar de provedor ou cair matam o
    /// processo; o novo recebe a conversa de volta.
    history: Arc<History>,
}

/// O adaptador em si: atende pedidos ACP vindos do `stdin` lógico e emite
/// notificações/pedidos na fila `Incoming` que o bot consome.
pub(crate) struct Link {
    vault: VaultSource,
    /// O bot dono das sessões: só ele reabre a conversa de uma sessão (session/load), e só com a mesma identidade
    /// da sessão (`conversationIdentity` dos parâmetros: Space, conta, revisão).
    bot: String,
    extra_env: Vec<(String, String)>,
    /// Preenche no `attach`: fila `Incoming` que o bot consome.
    out: Mutex<Option<mpsc::UnboundedSender<Incoming>>>,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    /// Respostas do Hub a pedidos NOSSOS (permissão), por id `mxp-N`.
    outgoing: Mutex<HashMap<String, oneshot::Sender<Value>>>,
    /// Respostas do adaptador aos pedidos do bot (`Pending` do [`acp::Acp`]).
    pending: Mutex<acp::Pending>,
    next_out: AtomicU64,
    /// So as provas do ator com o motor real dispensam os servidores MCP: o executavel deles seria o binario de testes.
    #[cfg(test)]
    skip_mcp: std::sync::atomic::AtomicBool,
    stop: watch::Sender<bool>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

/// Só para provas: segura o fim da retomada para o cancelamento chegar no meio dela.
#[cfg(test)]
pub(super) static PAUSE_AFTER_RESTORE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Libera a marca de turno em curso quando o prompt termina, por qualquer caminho.
struct BusyGuard<'a>(&'a std::sync::atomic::AtomicBool);

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Menor credencial que dá para achar no texto sem estragá-lo: abaixo disto a conversa não é guardada.
const MIN_SECRET_LEN: usize = 8;

/// Provedor do modelo `provider/model` ou do nome (`gemini-*` → google,
/// `muse-*` → meta, `gpt-*`/`o*` → openai). Sem provedor reconhecido, falha fechada.
fn provider_of(model_id: &str) -> Result<(String, String)> {
    if let Some((provider, model)) = model_id.split_once('/') {
        ensure!(!provider.is_empty() && !model.is_empty(), "modelo do motor inválido: {model_id}");
        return Ok((provider.to_owned(), model.to_owned()));
    }
    let provider = if model_id.starts_with("gemini") {
        "google"
    } else if model_id.starts_with("muse") {
        "meta"
    } else if model_id.starts_with("gpt")
        || model_id.strip_prefix('o').is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
    {
        "openai"
    } else {
        bail!("o modelo do Motor Mexerica precisa vir como <provedor>/<modelo>: {model_id}")
    };
    Ok((provider.to_owned(), model_id.to_owned()))
}

/// Geração de uma credencial: impressão digital (só em memória, nunca registrada) do que o cofre guarda para
/// `provider`. Vazia quando o provedor não tem credencial.
pub(super) fn credential_generation(provider: &str, key: Option<&str>) -> String {
    let Some(key) = key.filter(|key| !key.is_empty()) else { return String::new() };
    let digest = Sha256::new()
        .chain_update(b"motor-mexerica/credential-generation\0")
        .chain_update(provider.as_bytes())
        .chain_update([0])
        .chain_update(key.as_bytes())
        .finalize();
    digest.iter().take(16).fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

/// UMA leitura do cofre por turno: dela saem as chaves que a proteção precisa conhecer E a geração de cada
/// credencial, então o que foi autorizado e o que foi protegido são a mesma identidade (não há segunda leitura
/// que possa trocar a conta depois da decisão).
struct Credentials {
    reads: Vec<(&'static str, Result<Option<String>>)>,
}

impl Credentials {
    fn read(vault: &VaultSource) -> Self {
        Self {
            reads: super::PROVIDERS
                .iter()
                .map(|provider| (*provider, vault(&super::credential_slot(provider))))
                .collect(),
        }
    }

    fn key(&self, provider: &str) -> Result<Option<&str>> {
        match self.reads.iter().find(|(name, _)| *name == provider) {
            None | Some((_, Ok(None))) => Ok(None),
            Some((_, Ok(Some(key)))) => Ok(Some(key.as_str())),
            Some((name, Err(error))) => bail!("lendo a credencial do provedor {name}: {error:#}"),
        }
    }

    /// Geração da credencial do provedor. Cofre ilegível: erro (nada é autorizado sem saber qual conta recebe).
    fn generation(&self, provider: &str) -> Result<String> {
        Ok(credential_generation(provider, self.key(provider)?))
    }

    /// Chaves do cofre de todos os provedores: saem do texto antes de a conversa ser guardada. A proteção não pode
    /// falhar calada: se uma leitura falha, ou uma chave é curta demais para ser achada no texto sem estragá-lo, a
    /// conversa não é guardada.
    fn known(&self) -> Result<Vec<String>> {
        let mut keys = Vec::new();
        for (provider, _) in &self.reads {
            let Some(key) = self.key(provider)?.filter(|key| !key.is_empty()) else { continue };
            ensure!(
                key.len() >= MIN_SECRET_LEN,
                "a credencial do provedor {provider} é curta demais para ser protegida"
            );
            keys.push(key.to_owned());
        }
        Ok(keys)
    }
}

/// O cofre que o processo do motor enxerga: só entrega a credencial se ela ainda é a da geração autorizada para esta
/// conversa. Se mudou no meio (rotação entre a decisão e a chamada ao provedor), a leitura é recusada e a chamada
/// falha, em vez de a conta nova receber o contexto que foi autorizado para a anterior.
pub(super) fn pinned(vault: VaultSource, provider: &str, generation: String) -> VaultSource {
    let provider = provider.to_owned();
    Arc::new(move |slot: &str| {
        let key = vault(slot)?;
        ensure!(
            credential_generation(&provider, key.as_deref()) == generation,
            "a credencial do provedor mudou depois de autorizada; a chamada foi recusada"
        );
        Ok(key)
    })
}

impl Link {
    /// Adaptador preso ao cofre do Hub. `extra_env` é só para provas
    /// (`MEXERICA_MOTOR_FAUX=1`); credencial jamais entra aí.
    pub(crate) fn new(vault: VaultSource, bot: &str, extra_env: &[(&str, &str)]) -> Arc<Self> {
        let (stop, _) = watch::channel(false);
        Arc::new(Self {
            vault,
            bot: bot.to_owned(),
            extra_env: extra_env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            out: Mutex::new(None),
            sessions: Mutex::new(HashMap::new()),
            outgoing: Mutex::new(HashMap::new()),
            pending: Mutex::new(acp::Pending::default()),
            next_out: AtomicU64::new(1),
            #[cfg(test)]
            skip_mcp: std::sync::atomic::AtomicBool::new(false),
            stop,
            task: Mutex::new(None),
        })
    }

    /// Dispensa os servidores MCP deste adaptador (so a prova do ator com o motor real).
    #[cfg(test)]
    pub(crate) fn skip_mcp_servers(&self) {
        self.skip_mcp.store(true, Ordering::Relaxed);
    }

    /// Liga o adaptador ao transporte: `read` recebe os quadros JSON-RPC do
    /// bot; `out` leva `session/update`, `session/request_permission` e o
    /// `Closed`; `pending` resolve os pedidos do bot. Chamado por `Acp::spawn_link`.
    pub(crate) fn attach(
        self: &Arc<Self>,
        read: tokio::io::DuplexStream,
        out: mpsc::UnboundedSender<Incoming>,
        pending: acp::Pending,
    ) {
        *self.out.locked() = Some(out);
        *self.pending.locked() = pending;
        let conn = self.clone();
        let mut stop = self.stop.subscribe();
        *self.task.locked() = Some(tokio::spawn(async move {
            conn.run(read, &mut stop).await;
        }));
    }

    /// Emite na fila que o bot consome; `None` = adaptador ainda não ligado.
    fn emit(&self, incoming: Incoming) {
        if let Some(out) = self.out.locked().as_ref() {
            let _ = out.send(incoming);
        }
    }

    async fn run(self: Arc<Self>, read: tokio::io::DuplexStream, stop: &mut watch::Receiver<bool>) {
        let mut lines = BufReader::new(read).lines();
        // Uma tarefa por pedido do bot: no fim, o que ainda anda é cancelado.
        let mut requests = JoinSet::new();
        loop {
            while requests.try_join_next().is_some() {}
            let line = tokio::select! {
                _ = stop.changed() => None,
                line = lines.next_line() => line.ok().flatten(),
            };
            let Some(line) = line else { break };
            let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
            match (msg["method"].as_str(), msg.get("id").cloned()) {
                // Pedido do bot (tem método e id): atende em tarefa própria —
                // um `session/prompt` longo não pode segurar `session/cancel`.
                (Some(method), Some(id)) if !id.is_null() => {
                    let method = method.to_owned();
                    let params = msg["params"].clone();
                    let conn = self.clone();
                    requests.spawn(async move {
                        let result = conn.call(&method, params).await;
                        let res = match result {
                            Ok(value) => Ok(value),
                            Err(error) => Err(json!({"code": -32603, "message": format!("{error:#}")})),
                        };
                        let pending = conn.pending.locked().clone();
                        if let Some(key) = id.as_i64()
                            && let Some(waiter) = pending.locked().remove(&key)
                        {
                            let _ = waiter.send(res);
                        }
                    });
                }
                (Some("session/cancel"), _) => {
                    if let Some(sid) = msg["params"]["sessionId"].as_str()
                        && let Some(session) = self.sessions.locked().get(sid)
                    {
                        session.cancel.send_replace(true);
                    }
                }
                // Resposta do bot a um pedido nosso (permissão): id texto `mxp-N`.
                (None, Some(id)) => {
                    if let Some(key) = id.as_str()
                        && let Some(waiter) = self.outgoing.locked().remove(key)
                    {
                        let result = match msg.get("error") {
                            Some(error) => {
                                json!({"outcome": {"outcome": "cancelled"}, "denied": error["message"].clone()})
                            }
                            None => msg["result"].clone(),
                        };
                        let _ = waiter.send(result);
                    }
                }
                _ => {}
            }
        }
        // Transporte morto ou parada pedida: cancela os turnos em andamento
        // (cada um mata o processo do motor e solta a ferramenta MCP) e espera
        // por eles, para o `kill`/`kill_and_wait` não deixar o motor gastando.
        for session in self.sessions.locked().values() {
            session.cancel.send_replace(true);
        }
        let drained = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while requests.join_next().await.is_some() {}
        })
        .await;
        if drained.is_err() {
            tracing::warn!("turnos do Motor Mexerica não terminaram no prazo; abortando");
        }
        requests.shutdown().await;
        // Fechar o adaptador inteiro também é atividade: o prazo sem uso de cada conversa conta daqui.
        for session in self.sessions.locked().values() {
            session.history.touch();
        }
        self.sessions.locked().clear();
        self.outgoing.locked().clear();
        self.emit(Incoming::Closed { stderr_tail: String::new() });
    }

    /// Encerra o adaptador: o laço morre, os processos do motor também.
    pub(crate) async fn shutdown(&self) {
        self.stop.send_replace(true);
        let task = self.task.locked().take();
        if let Some(task) = task {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(10), task).await;
        }
    }

    /// Pid de algum motor vivo (prova e diagnóstico).
    pub(crate) fn pid(&self) -> Option<u32> {
        self.sessions.locked().values().find_map(|s| {
            s.engine.try_lock().ok().and_then(|slot| slot.as_ref().and_then(Engine::pid)).or_else(|| *s.pid.locked())
        })
    }

    async fn call(self: &Arc<Self>, method: &str, params: Value) -> Result<Value> {
        match method {
            "initialize" => Ok(json!({
                "protocolVersion": 1,
                "agentCapabilities": {
                    "loadSession": true,
                    "sessionCapabilities": {"systemPrompt": {}, "close": {}},
                },
                "agentInfo": {"name": "mexerica-motor", "version": env!("CARGO_PKG_VERSION")},
            })),
            "session/new" => self.session_new(&params).await,
            "session/load" => self.session_load(&params).await,
            "session/set_model" => self.set_model(&params).await.map(|()| json!({})),
            "session/set_config_option" => Ok(json!({})),
            "session/prompt" => self.prompt(&params).await,
            // Fecha o processo e o MCP da sessão; a conversa fica guardada para o session/load.
            "session/close" => {
                if let Some(sid) = params["sessionId"].as_str()
                    && let Some(session) = self.sessions.locked().remove(sid)
                {
                    session.history.touch();
                }
                Ok(json!({}))
            }
            // ACPs que o motor não conhece: erro JSON-RPC padrão, nunca silêncio.
            _ => bail!("{method} não é suportado pelo Motor Mexerica"),
        }
    }

    /// O dono da conversa de uma sessão: o bot e a identidade com que ela abre (ausente = identidade vazia).
    fn owner_of(&self, params: &Value) -> String {
        super::conversation_owner(&self.bot, params["conversationIdentity"].as_str().unwrap_or_default())
    }

    async fn session_new(self: &Arc<Self>, params: &Value) -> Result<Value> {
        let sid = uuid::Uuid::new_v4().to_string();
        let session = self.open_session(params, historico::create(&self.owner_of(params), &sid)?).await?;
        self.sessions.locked().insert(sid.clone(), session);
        Ok(json!({"sessionId": sid}))
    }

    /// Retoma uma sessão deste bot depois que a conexão antiga foi fechada (troca de
    /// modelo ou de configuração no app): servidores MCP novos, conversa de volta. O
    /// modelo não volta: o bot o escolhe de novo (`session/set_model`).
    async fn session_load(self: &Arc<Self>, params: &Value) -> Result<Value> {
        let sid = params["sessionId"].as_str().context("session/load sem sessionId")?;
        let history = historico::reopen(&self.owner_of(params), sid)
            .context("conversa do Motor Mexerica desconhecida ou expirada")?;
        let session = self.open_session(params, history).await?;
        self.sessions.locked().insert(sid.to_owned(), session);
        Ok(json!({}))
    }

    async fn open_session(self: &Arc<Self>, params: &Value, history: Arc<History>) -> Result<Arc<Session>> {
        // Desligado (ou sem ajudante), nenhum servidor MCP abre: a mesma fronteira do Engine.
        super::ensure_runnable()?;
        let cwd = params["cwd"].as_str().map_or_else(|| PathBuf::from("/"), PathBuf::from);
        // Falha ao abrir os servidores recusa a sessão: sem isso o bot seguiria sem ferramentas e sem aviso.
        // Só a prova do ator com o motor real (`actor_extra_env` vazio fora dela) dispensa os servidores: o
        // executável deles seria o binário de testes, não um Hub.
        #[cfg(test)]
        let skip = self.skip_mcp.load(Ordering::Relaxed);
        #[cfg(not(test))]
        let skip = false;
        let servers = if skip { json!([]) } else { params["mcpServers"].clone() };
        let mcp = McpHost::start(&servers, &cwd)
            .await
            .map(|host| Some(Arc::new(host)))
            .context("abrindo os servidores MCP do Motor Mexerica")?;
        let (cancel, _) = watch::channel(false);
        Ok(Arc::new(Session {
            engine: tokio::sync::Mutex::new(None),
            mcp,
            provider: Mutex::new(String::new()),
            model: Mutex::new(String::new()),
            system_prompt: Mutex::new(params["systemPrompt"].as_str().map(str::to_owned)),
            pid: Mutex::new(None),
            cancel,
            busy: std::sync::atomic::AtomicBool::new(false),
            history,
        }))
    }

    /// Garante o processo do motor preso a `provider`, com prompt de sistema
    /// e ferramentas aplicados. Troca de provedor = processo novo.
    async fn ensure_engine(self: &Arc<Self>, session: &Session, provider: &str, creds: &Credentials) -> Result<()> {
        // A decisão de proteção e de identidade vem ANTES de qualquer retomada. Sem ela, nada da conversa antiga é
        // reaproveitado: a cópia é apagada e o processo vivo (que ainda guarda o contexto) é descartado.
        let mut slot = session.engine.lock().await;
        let generation = match creds.generation(provider) {
            Ok(generation) => generation,
            Err(error) => {
                session.history.drop_unprotected();
                *slot = None;
                return Err(error);
            }
        };
        let mut discard = false;
        if let Err(error) = creds.known() {
            tracing::warn!(error = format!("{error:#}"), "sem proteger a conversa não há cópia nem contexto vivo");
            session.history.drop_unprotected();
            discard = true;
        } else if !session.history.bind_credential(provider, &generation) {
            tracing::info!(provider, "a credencial do provedor mudou; a conversa recomeça");
            discard = true;
        }
        if discard {
            *slot = None;
        }
        let needs = match slot.as_mut() {
            None => true,
            Some(engine) => {
                let alive = engine.alive();
                if !alive {
                    // O processo caiu entre turnos (OOM, SIGKILL): sessão quebrada.
                    // Descarta o motor morto; este turno sobe outro (a conversa em
                    // memória do processo antigo volta do histórico da sessão).
                    tracing::warn!("o processo do Motor Mexerica caiu; reabrindo a sessão neste turno");
                }
                // Um processo preso a outra geração da credencial não serve mais.
                !alive || engine.provider() != provider || engine.credential != generation
            }
        };
        if !needs {
            // O recorte do Hub tirou voltas que o processo vivo ainda tem: ele recebe a cópia.
            // O processo sai do slot enquanto recebe a copia: se a retomada falhar ou for cancelada, ele morre com o
            // futuro e o proximo turno sobe outro, que recebe a conversa inteira.
            if session.history.take_resync()
                && let Some(mut engine) = slot.take()
            {
                engine.restore_history(&session.history.snapshot()).await?;
                *slot = Some(engine);
            }
            return Ok(());
        }
        *slot = None; // Drop mata o processo e o cofre do provedor antigo.
        let extra: Vec<(&str, &str)> = self.extra_env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let mut engine =
            Engine::spawn_with(pinned(self.vault.clone(), provider, generation.clone()), provider, &extra)?;
        engine.credential = generation;
        let system_prompt = session.system_prompt.locked().clone();
        if let Some(prompt) = system_prompt {
            engine.set_system_prompt(&prompt).await?;
        }
        if let Some(mcp) = &session.mcp {
            engine.set_tools(mcp.specs()).await?;
        }
        let history = session.history.snapshot();
        if !history.is_empty() {
            engine.restore_history(&history).await?;
        }
        session.history.take_resync();
        #[cfg(test)]
        if PAUSE_AFTER_RESTORE.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        }
        *slot = Some(engine);
        Ok(())
    }

    async fn set_model(self: &Arc<Self>, params: &Value) -> Result<()> {
        let sid = params["sessionId"].as_str().context("session/set_model sem sessionId")?;
        let model_id = params["modelId"].as_str().context("session/set_model sem modelId")?;
        let (provider, model) = provider_of(model_id)?;
        let session = self.sessions.locked().get(sid).cloned().ok_or_else(|| anyhow!("sessão desconhecida: {sid}"))?;
        ensure!(!session.busy.swap(true, Ordering::AcqRel), "a sessão já tem um turno em curso");
        let _busy = BusyGuard(&session.busy);
        self.ensure_engine(&session, &provider, &Credentials::read(&self.vault)).await?;
        let mut slot = session.engine.lock().await;
        let engine = slot.as_mut().ok_or_else(|| anyhow!("o motor não subiu"))?;
        let result = engine.set_model(&provider, &model).await;
        if let Err(error) = result {
            // O slot nunca guarda um processo cuja conversa de comandos falhou:
            // o `Drop` encerra o grupo de processos e colhe o filho, e o próximo
            // comando sobe outro. Acontece antes de `BusyGuard` soltar a trava.
            *slot = None;
            return Err(error);
        }
        *session.provider.locked() = provider;
        *session.model.locked() = model;
        Ok(())
    }

    async fn prompt(self: &Arc<Self>, params: &Value) -> Result<Value> {
        let sid = params["sessionId"].as_str().context("session/prompt sem sessionId")?.to_owned();
        historico::sweep();
        let session = self.sessions.locked().get(&sid).cloned().ok_or_else(|| anyhow!("sessão desconhecida: {sid}"))?;
        ensure!(!session.busy.swap(true, Ordering::AcqRel), "a sessão já tem um turno em curso");
        let _busy = BusyGuard(&session.busy);
        let text = acp::content_text(&params["prompt"]);
        let runner = CardRunner { link: self.clone(), session: session.clone(), sid: sid.clone() };
        let mut retried = false;
        // Inscrito antes de subir e restaurar o processo: um cancelamento que chega durante a
        // retomada da conversa também para o turno.
        let mut cancel = session.cancel.subscribe();
        loop {
            let current_provider = session.provider.locked().clone();
            ensure!(!current_provider.is_empty(), "nenhum modelo escolhido (session/set_model antes do prompt)");
            // Uma leitura do cofre por turno: a mesma identidade decide a retomada, a proteção e a credencial servida.
            let creds = Credentials::read(&self.vault);
            let ready = tokio::select! {
                _ = cancel.changed() => false,
                result = self.ensure_engine(&session, &current_provider, &creds) => result.map(|()| true)?,
            };
            if !ready {
                return Ok(json!({"stopReason": "cancelled"}));
            }
            let mut owned = session.engine.lock().await.take().context("a sessão já tem um turno em curso")?;
            *session.pid.locked() = owned.pid();
            let provider = session.provider.locked().clone();
            let model = session.model.locked().clone();
            ensure!(!model.is_empty(), "nenhum modelo escolhido (session/set_model antes do prompt)");
            // Nada do turno foi entregue ao bot enquanto isto for falso.
            let delivered = Arc::new(std::sync::atomic::AtomicBool::new(false));
            // Mensagens que o motor terminou de emitir neste turno (limpas, dentro do teto): viram a
            // conversa da sessão.
            let known = creds.known();
            let protected = known.is_ok();
            let finished = Arc::new(Mutex::new(match known {
                Ok(known) => TurnLog::new(known),
                Err(error) => {
                    tracing::warn!(
                        error = format!("{error:#}"),
                        "sem proteger a conversa não há cópia; o turno não é guardado"
                    );
                    TurnLog::unprotected()
                }
            }));
            let conn = self.clone();
            let sid_events = sid.clone();
            let seen = delivered.clone();
            let collect = finished.clone();
            let mut on_event = move |event: &Value| {
                seen.store(true, Ordering::Relaxed);
                if event["type"] == "message_end" {
                    collect.locked().push(&event["message"]);
                }
                conn.forward_event(&sid_events, event);
            };
            let alive = tokio::select! {
                _ = cancel.changed() => {
                    // Cancelar o turno mata o processo do motor.
                    let _ = owned.kill().await;
                    *session.pid.locked() = None;
                    session.history.remember(finished.locked().take());
                    false
                }
                result = owned.prompt_with(&provider, &model, &text, &runner, &mut on_event) => {
                    if let Err(error) = result {
                        // Falha no meio do turno (processo morto, linha inválida ou
                        // acima do teto, erro do modelo): o stdout pode ter eventos
                        // velhos e o processo pode nem existir mais. O motor NÃO
                        // volta ao slot — o `Drop` mata e colhe; o próximo turno
                        // sobe um limpo.
                        *session.pid.locked() = None;
                        drop(owned);
                        session.history.remember(finished.locked().take());
                        // O motor já tinha morrido antes de o turno dar qualquer
                        // sinal (a queda entre turnos ainda não era visível ao
                        // `try_wait`): nada foi entregue e nenhuma ferramenta rodou,
                        // então um único reinício limpo é seguro.
                        if !retried && !delivered.load(Ordering::Relaxed) && super::engine_gone(&error) {
                            tracing::warn!("o Motor Mexerica tinha caído antes do turno; reabrindo e repetindo uma vez");
                            retried = true;
                            continue;
                        }
                        return Err(error);
                    }
                    session.history.remember(finished.locked().take());
                    true
                }
            };
            if alive {
                *session.pid.locked() = None;
                // O turno sem proteção não vira contexto: o processo que o viu não volta ao slot, e o próximo
                // turno começa de uma conversa que passou pela redação.
                if protected {
                    *session.engine.lock().await = Some(owned);
                } else {
                    drop(owned);
                }
            }
            return Ok(json!({"stopReason": if alive { "end_turn" } else { "cancelled" }}));
        }
    }

    /// Evento do motor → `session/update` ACP (streaming do turno).
    fn forward_event(&self, sid: &str, event: &Value) {
        if event["type"] != "message_update" {
            return;
        }
        let inner = &event["assistantMessageEvent"];
        let (update, text) = match inner["type"].as_str() {
            Some("text_delta") => ("agent_message_chunk", inner["delta"].as_str().unwrap_or_default()),
            Some("thinking_delta") => ("agent_thought_chunk", inner["delta"].as_str().unwrap_or_default()),
            _ => return,
        };
        if text.is_empty() {
            return;
        }
        self.emit(Incoming::Notification {
            method: "session/update".to_owned(),
            params: json!({
                "sessionId": sid,
                "update": {"sessionUpdate": update, "content": {"type": "text", "text": text}},
            }),
        });
    }

    /// Pedido agente→bot (`session/request_permission`) e espera da resposta.
    async fn ask(&self, method: &str, params: Value) -> Result<Value> {
        let id = format!("mxp-{}", self.next_out.fetch_add(1, Ordering::Relaxed));
        let (tx, rx) = oneshot::channel();
        self.outgoing.locked().insert(id.clone(), tx);
        let Some(out) = self.out.locked().clone() else { bail!("o adaptador já encerrou") };
        match out.send(Incoming::Request { id: json!(id), method: method.to_owned(), params }) {
            Ok(()) => rx.await.context("o adaptador encerrou durante a aprovação"),
            Err(_) => Err(anyhow!("o adaptador já encerrou")),
        }
    }
}

/// Executor do turno no caminho do bot: `tool_request` → cartão de
/// aprovação existente (`session/request_permission`) → `McpHost` → saída.
struct CardRunner {
    link: Arc<Link>,
    session: Arc<Session>,
    sid: String,
}

impl ToolRunner for CardRunner {
    fn call<'a>(&'a self, _motor_call_id: &'a str, name: &'a str, input: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            // O `toolCallId` que o bot grava e correlaciona é do Hub: o `callId`
            // do motor é dado do motor e nunca vira chave de entrada do bot.
            let call_id = format!("mxt-{}", uuid::Uuid::new_v4());
            // Identidade pelo que o PRÓPRIO Hub abriu (`McpHost`), nunca pelo
            // nome ou por campo vindo do motor. O bot confere o servidor contra
            // as definições internas exatas (`tool_origin`), como nos outros motores.
            let class = self.session.mcp.as_ref().and_then(|mcp| mcp.classify(name));
            let meta = class.as_ref().map(|c| json!({"mexericaTool": {"server": c.server, "tool": c.tool}}));
            let with_meta = |mut value: Value| {
                if let Some(meta) = &meta {
                    value["_meta"] = meta.clone();
                }
                value
            };
            let notify = |update: Value| {
                self.link.emit(Incoming::Notification {
                    method: "session/update".to_owned(),
                    params: json!({"sessionId": self.sid, "update": update}),
                });
            };
            notify(with_meta(json!({
                "sessionUpdate": "tool_call",
                "toolCallId": call_id,
                "title": name,
                "kind": "other",
                "status": "in_progress",
                "rawInput": input,
            })));
            // Mesma política dos outros motores (`engine_permissions::mcp_released`,
            // hoje nada): o que não está liberado pede ao bot (cartão e autoridade).
            let allowed = if class.as_ref().is_some_and(ToolClass::released) {
                true
            } else {
                let outcome = self
                    .link
                    .ask(
                        "session/request_permission",
                        json!({
                            "sessionId": self.sid,
                            "toolCall": with_meta(json!({
                                "toolCallId": call_id,
                                "title": name,
                                "kind": "other",
                                "status": "in_progress",
                                "rawInput": input,
                            })),
                            "options": [
                                {"optionId": "allow", "name": "Permitir", "kind": "allow_once"},
                                {"optionId": "deny", "name": "Negar", "kind": "reject_once"},
                            ],
                        }),
                    )
                    .await;
                outcome
                    .ok()
                    .and_then(|result| result["outcome"]["optionId"].as_str().map(str::to_owned))
                    .is_some_and(|option| option == "allow")
            };
            let output = if allowed {
                match &self.session.mcp {
                    Some(mcp) => mcp.call(&call_id, name, input).await,
                    None => Err("esta sessão não tem ferramentas".to_owned()),
                }
            } else {
                Err("permissão negada".to_owned())
            };
            // O mesmo teto que vai ao motor: a entrada gravada pelo bot não passa dele.
            notify(json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": call_id,
                "status": if output.is_ok() { "completed" } else { "failed" },
                "rawOutput": super::tools::cap(output.clone().unwrap_or_else(|error| error)),
            }));
            output
        })
    }
}

/// Ganchos só para as provas (`tests_link.rs`): o executor do cartão sobre um
/// `McpHost` pronto, sem processo do motor, e a resposta do bot ao pedido.
#[cfg(test)]
impl Link {
    pub(super) fn test_runner(
        self: &Arc<Self>,
        mcp: McpHost,
        out: mpsc::UnboundedSender<Incoming>,
    ) -> Box<dyn ToolRunner> {
        *self.out.locked() = Some(out);
        let (cancel, _) = watch::channel(false);
        let session = Arc::new(Session {
            engine: tokio::sync::Mutex::new(None),
            mcp: Some(Arc::new(mcp)),
            provider: Mutex::new(String::new()),
            model: Mutex::new(String::new()),
            system_prompt: Mutex::new(None),
            pid: Mutex::new(None),
            cancel,
            busy: std::sync::atomic::AtomicBool::new(false),
            history: Arc::default(),
        });
        Box::new(CardRunner { link: self.clone(), session, sid: "sessao-de-prova".to_owned() })
    }

    /// Entrega ao adaptador a resposta do bot a um pedido nosso (`mxp-N`).
    pub(super) fn answer_for_tests(&self, id: &str, result: Value) {
        if let Some(waiter) = self.outgoing.locked().remove(id) {
            let _ = waiter.send(result);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::provider_of;

    #[test]
    fn provedor_do_modelo_nao_confunde_nome_que_comeca_com_o() {
        assert_eq!(provider_of("o3").unwrap(), ("openai".to_owned(), "o3".to_owned()));
        assert_eq!(provider_of("o4-mini").unwrap().0, "openai");
        assert_eq!(provider_of("gpt-5").unwrap().0, "openai");
        assert_eq!(provider_of("gemini-3").unwrap().0, "google");
        assert_eq!(provider_of("google/gemini-3").unwrap().0, "google");
        assert_eq!(provider_of("muse-spark-1.3").unwrap().0, "meta");
        assert_eq!(provider_of("meta/muse-wonder-1.0").unwrap(), ("meta".to_owned(), "muse-wonder-1.0".to_owned()));
        for nome in ["opus-5", "o", "outro-modelo", "claude-sonnet-5-5", "llama-4"] {
            assert!(provider_of(nome).is_err(), "{nome} não é de nenhum provedor do motor");
        }
    }
}
