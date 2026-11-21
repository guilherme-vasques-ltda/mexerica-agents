//! Conversa só de texto da Open com o Codex isolado: um turno por mensagem, sem ferramentas.
use crate::LockExt;
use crate::fork::engine_accounts::{LaunchReference, Registry, State};
use crate::fork::engine_setup::process::Rpc;
use crate::hub::{BotStatus, Hub};
use crate::store::{BotConfig, EntryKind, Lane};
use anyhow::{Result, anyhow, bail, ensure};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};
use tokio::sync::watch;

const TURN_LIMIT: Duration = Duration::from_secs(600);
const MAX_REPLY: usize = 200_000;
/// Código JSON-RPC com que o Codex recusa retomar uma thread que não existe neste perfil.
const INVALID_REQUEST: i64 = -32600;
/// Uma resposta longa chega inteira no fim do item; a linha do protocolo pode passar bem dos 64 KiB do setup.
const LINE_LIMIT: usize = 4 * 1024 * 1024;
const INSTRUCTIONS: &str = "Nesta edição você conversa só por texto. Você não tem ferramentas, arquivos, \
internet nem acesso ao computador; se o pedido precisar disso, diga que não pode fazer por aqui.";
/// Itens que um turno só de texto pode produzir; qualquer outro encerra o turno.
const TEXT_ITEMS: [&str; 5] = ["userMessage", "agentMessage", "reasoning", "plan", "contextCompaction"];

struct Running {
    hub: Weak<Hub>,
    account: String,
    cancel: watch::Sender<bool>,
    done: watch::Receiver<()>,
}

static RUNNING: Mutex<Option<HashMap<String, Running>>> = Mutex::new(None);
static CLOSING: Mutex<Vec<Weak<Hub>>> = Mutex::new(Vec::new());

/// Reserva da conversa: um turno por bot, e nenhum depois do encerramento do Hub.
pub(crate) struct Ticket {
    bot: String,
    directory: PathBuf,
    cancel: watch::Receiver<bool>,
    done: watch::Sender<()>,
}

/// A conta Codex da Open que o bot usa: a escolhida nele ou, sem escolha, a conectada mais recente.
pub(crate) fn account_profile(hub: &Hub, cfg: &BotConfig) -> Result<(String, PathBuf)> {
    let registry = Registry::load(&hub.store)?;
    let mut connected = registry
        .accounts
        .iter()
        .filter(|a| crate::fork::engine_setup::is_open_account(a) && a.state == State::Connected);
    let account = match cfg.fork_editor.engine_account_id.as_deref() {
        Some(id) => connected.find(|a| a.id == id),
        None => connected.max_by_key(|a| a.verified_at),
    }
    .ok_or_else(|| anyhow!("open_chat_account_required"))?;
    let LaunchReference::CliProfile { directory, .. } = &account.launch else { bail!("open_chat_account_required") };
    Ok((account.id.clone(), directory.clone()))
}

/// Cerca das mutações de conta: recusa enquanto um turno da Open usa a conta e impede um turno novo até o fim do efeito.
pub(crate) fn with_idle_account<T>(account: &str, effect: impl FnOnce() -> Result<T>) -> Result<T> {
    let guard = RUNNING.locked();
    ensure!(
        !guard.iter().flat_map(HashMap::values).any(|r| r.account == account && r.done.has_changed().is_ok()),
        "account_busy: aguarde o turno terminar."
    );
    effect()
}

pub(crate) fn begin(hub: &Arc<Hub>, cfg: &BotConfig) -> Result<Ticket> {
    ensure!(crate::environment::Environment::is_open(), "open_chat_unavailable");
    ensure!(!cfg.is_group(), "open_chat_unavailable");
    let mut guard = RUNNING.locked();
    let (account, directory) = account_profile(hub, cfg)?;
    admit(&mut guard, hub, &cfg.id, account, directory)
}

/// Reserva o turno sob `RUNNING`, a mesma trava em que a reconexão da conta abre a operação: uma conta em reconexão
/// não recebe turno, e uma conta com turno não começa a reconectar.
fn admit(
    guard: &mut Option<HashMap<String, Running>>,
    hub: &Arc<Hub>,
    bot: &str,
    account: String,
    directory: PathBuf,
) -> Result<Ticket> {
    ensure!(!CLOSING.locked().iter().any(|h| h.ptr_eq(&Arc::downgrade(hub))), "open_chat_unavailable");
    ensure!(!crate::fork::engine_setup::in_setup(&hub.store, &account), "open_chat_reconnecting");
    let running = guard.get_or_insert_with(HashMap::new);
    running.retain(|_, r| r.hub.strong_count() > 0 && r.done.has_changed().is_ok());
    ensure!(!running.contains_key(bot), "open_chat_busy");
    let (cancel_tx, cancel) = watch::channel(false);
    let (done, done_rx) = watch::channel(());
    running.insert(bot.to_owned(), Running { hub: Arc::downgrade(hub), account, cancel: cancel_tx, done: done_rx });
    Ok(Ticket { bot: bot.to_owned(), directory, cancel, done })
}

#[cfg(test)]
pub(crate) fn admit_for_test(hub: &Arc<Hub>, bot: &str, account: &str) -> Result<Ticket> {
    admit(&mut RUNNING.locked(), hub, bot, account.to_owned(), PathBuf::new())
}

pub(crate) fn stop(bot: &str) {
    if let Some(r) = RUNNING.locked().as_ref().and_then(|running| running.get(bot)) {
        let _ = r.cancel.send(true);
    }
}

/// Recusa turnos novos, interrompe os em curso e espera cada um encerrar o Codex.
pub(crate) async fn shutdown(hub: &Arc<Hub>) {
    let waiting: Vec<_> = {
        let guard = RUNNING.locked();
        CLOSING.locked().push(Arc::downgrade(hub));
        guard
            .as_ref()
            .into_iter()
            .flat_map(HashMap::values)
            .filter(|r| r.hub.ptr_eq(&Arc::downgrade(hub)))
            .map(|r| {
                let _ = r.cancel.send(true);
                r.done.clone()
            })
            .collect()
    };
    let _ = tokio::time::timeout(Duration::from_secs(5), async {
        for mut done in waiting {
            let _ = done.changed().await;
        }
    })
    .await;
}

fn thread_key(lane: &Lane) -> String {
    format!("open_chat.thread.{}.{}", lane.chat, lane.session.as_deref().unwrap_or("main"))
}

/// Roda o turno em segundo plano; a resposta chega como entradas normais da conversa.
pub(crate) fn run(
    hub: &Arc<Hub>,
    ticket: Ticket,
    cfg: &BotConfig,
    lane: Lane,
    turn: i64,
    entry_id: String,
    text: String,
) {
    let hub = hub.clone();
    let instructions = format!("{}\n\n{INSTRUCTIONS}", cfg.description.trim());
    tokio::spawn(async move {
        let Ticket { bot, directory, mut cancel, done } = ticket;
        let alive = || matches!(hub.store.bot(&bot), Ok(Some(row)) if !row.deleted);
        if !alive() {
            if let Some(running) = RUNNING.locked().as_mut() {
                running.remove(&bot);
            }
            return;
        }
        hub.set_runtime(&bot, |r| {
            r.status = BotStatus::Working;
            r.lane = Some(lane.clone());
            r.message = Some(entry_id.clone());
            r.started_at = Some(crate::store::now_ms());
        });
        hub.emit_bot(&bot);
        if let Some(entry) = hub.store.entry(&entry_id) {
            let mut data = entry.data;
            data["status"] = "sent".into();
            hub.set_entry(&entry_id, &data);
        }
        let mut reply = Stream::new(&hub, &lane, turn, &bot);
        let outcome = turn_in_profile(&hub, &directory, &lane, &instructions, &text, &mut cancel, &mut reply).await;
        reply.close();
        if alive() {
            if let Err(error) = outcome {
                let (code, text) = notice(&error.to_string());
                hub.add_entry(&lane, EntryKind::Notice, turn, &json!({"text": text, "code": code, "author": bot}));
            }
            hub.set_runtime(&bot, |r| {
                r.status = BotStatus::Idle;
                r.lane = None;
                r.message = None;
                r.started_at = None;
            });
            hub.emit_bot(&bot);
        }
        if let Some(running) = RUNNING.locked().as_mut() {
            running.remove(&bot);
        }
        drop(done);
    });
}

#[cfg(unix)]
async fn turn_in_profile(
    hub: &Hub,
    directory: &std::path::Path,
    lane: &Lane,
    instructions: &str,
    text: &str,
    cancel: &mut watch::Receiver<bool>,
    reply: &mut Stream,
) -> Result<()> {
    let launch = tokio::select! {
        launch = crate::fork::open_codex::app_server(directory, true) => launch?,
        _ = cancel.wait_for(|stop| *stop) => bail!("open_chat_cancelled"),
    };
    let bot = reply.bot.clone();
    // A exclusão do bot grava sob a mesma trava do banco: ou o processo nasce antes dela (e é cancelado e esperado),
    // ou o lançamento já vê o bot apagado.
    let child = {
        let db = hub.store.db.locked();
        ensure!(
            !*cancel.borrow() && matches!(crate::store::Store::bot_on(&db, &bot), Ok(Some(row)) if !row.deleted),
            "open_chat_cancelled"
        );
        crate::fork::motor_exec::start_open_codex(launch).map_err(|_| anyhow!("open_codex_unavailable"))?
    };
    let mut rpc = Rpc::from_child(child)?;
    let saved = hub.store.kv_read(&thread_key(lane))?;
    let keep = |thread: &str| {
        if saved.as_deref() != Some(thread)
            && let Err(error) = hub.store.kv_set_if_bot(&bot, &thread_key(lane), thread)
        {
            tracing::warn!(%error, "conversa da Open não guardou a thread do Codex");
        }
    };
    let outcome = drive(&mut rpc, saved.as_deref(), instructions, text, cancel, keep, |delta| reply.push(delta)).await;
    match outcome {
        Ok(_) => {
            let _ = rpc.finish().await;
            Ok(())
        }
        Err(error) => {
            rpc.terminate().await;
            Err(error)
        }
    }
}

#[cfg(not(unix))]
async fn turn_in_profile(
    _: &Hub,
    _: &std::path::Path,
    _: &Lane,
    _: &str,
    _: &str,
    _: &mut watch::Receiver<bool>,
    _: &mut Stream,
) -> Result<()> {
    bail!("open_codex_platform")
}

const NOTICE_CODES: [&str; 7] = [
    "open_chat_cancelled",
    "open_chat_too_long",
    "open_chat_tool_refused",
    "open_chat_timeout",
    "open_chat_policy",
    "open_codex_unavailable",
    "open_codex_platform",
];

/// O código conhecido do erro e o texto do aviso; a janela traduz pelo código.
fn notice(message: &str) -> (&'static str, &'static str) {
    let first = message.split(':').next().unwrap_or_default();
    let code = NOTICE_CODES.into_iter().find(|code| *code == first).unwrap_or("open_chat_failed");
    let text = match code {
        "open_chat_cancelled" => "Resposta interrompida.",
        "open_chat_tool_refused" => {
            "O Codex tentou usar uma ferramenta, e a Open só conversa por texto. Resposta encerrada."
        }
        "open_chat_timeout" => "O Codex demorou demais para responder. Tente de novo.",
        "open_chat_too_long" => "A resposta passou do tamanho máximo e foi encerrada.",
        "open_chat_policy" => {
            "A configuração do Codex neste Mac não deixa a conversa ficar só em texto e sem aprovações. Resposta cancelada."
        }
        "open_codex_unavailable" | "open_codex_platform" => {
            "Não deu para abrir o Codex no perfil da Open. Confira a conta em Contas de IA."
        }
        _ => "O Codex não conseguiu responder agora. Tente de novo.",
    };
    (code, text)
}

/// Texto da resposta numa entrada do agente, gravado aos poucos.
struct Stream {
    hub: Arc<Hub>,
    lane: Lane,
    turn: i64,
    bot: String,
    entry: Option<String>,
    text: String,
    flushed: Instant,
}

impl Stream {
    fn new(hub: &Arc<Hub>, lane: &Lane, turn: i64, bot: &str) -> Self {
        Self {
            hub: hub.clone(),
            lane: lane.clone(),
            turn,
            bot: bot.to_owned(),
            entry: None,
            text: String::new(),
            flushed: Instant::now(),
        }
    }
    fn data(&self, last: bool) -> Value {
        json!({"text": self.text, "final": last, "author": self.bot})
    }
    /// Falso quando a resposta passou do limite: o turno é interrompido em vez de seguir gerando.
    fn push(&mut self, delta: &str) -> bool {
        if self.text.len() + delta.len() > MAX_REPLY {
            return false;
        }
        if delta.is_empty() {
            return true;
        }
        self.text.push_str(delta);
        match &self.entry {
            None => {
                self.entry =
                    self.hub.add_entry(&self.lane, EntryKind::Agent, self.turn, &self.data(false)).map(|e| e.id);
            }
            Some(id) if self.flushed.elapsed() >= Duration::from_millis(300) => {
                self.hub.set_entry(id, &self.data(false));
                self.flushed = Instant::now();
            }
            Some(_) => {}
        }
        true
    }
    fn close(&mut self) {
        if let Some(id) = &self.entry {
            self.hub.set_entry(id, &self.data(true));
        }
    }
}

/// Um turno no app-server: retoma ou cria a thread, envia o texto e devolve o id da thread. Recusa qualquer pedido
/// do servidor e qualquer item que não seja texto, interrompendo o turno.
pub(crate) async fn drive(
    rpc: &mut Rpc,
    thread: Option<&str>,
    instructions: &str,
    text: &str,
    cancel: &mut watch::Receiver<bool>,
    keep_thread: impl FnOnce(&str),
    mut on_delta: impl FnMut(&str) -> bool,
) -> Result<String> {
    rpc.set_line_limit(LINE_LIMIT);
    let flow = async {
        rpc.initialize().await?;
        let session = json!({"sandbox": "read-only", "approvalPolicy": "never", "developerInstructions": instructions});
        let resumed = match thread.filter(|id| valid_id(id)) {
            Some(id) => {
                let mut params = session.clone();
                params["threadId"] = id.into();
                params["excludeTurns"] = true.into();
                match rpc.request(2, "thread/resume", params).await? {
                    Ok(value) => Some(value),
                    Err(error) if error["code"] == INVALID_REQUEST => None,
                    Err(_) => bail!("open_chat_failed"),
                }
            }
            None => None,
        };
        let started = match resumed {
            Some(value) => value,
            None => rpc.call(3, "thread/start", session).await?,
        };
        if started["approvalPolicy"] != "never" || started["sandbox"]["type"] != "readOnly" {
            tracing::warn!(approval = %started["approvalPolicy"], sandbox = %started["sandbox"]["type"], "conversa da Open recusada pela política do Codex");
            bail!("open_chat_policy");
        }
        let thread = started["thread"]["id"]
            .as_str()
            .filter(|id| valid_id(id))
            .ok_or_else(|| anyhow!("open_chat_failed"))?
            .to_owned();
        keep_thread(&thread);
        let turn = rpc
            .call(
                4,
                "turn/start",
                json!({"threadId": thread, "input": [{"type": "text", "text": text, "text_elements": []}]}),
            )
            .await?;
        let turn_id = turn["turn"]["id"].as_str().unwrap_or_default().to_owned();
        loop {
            let message = rpc.next().await?;
            if message.get("id").is_some() && message.get("method").is_some() {
                tracing::warn!(method = %message["method"], "pedido do Codex recusado na conversa só de texto");
                rpc.write(json!({"id": message["id"], "error": {"code": -32601, "message": "text only"}})).await?;
                interrupt(rpc, &thread, &turn_id).await;
                bail!("open_chat_tool_refused");
            }
            let params = &message["params"];
            match message["method"].as_str().unwrap_or_default() {
                "item/started" | "item/completed"
                    if !TEXT_ITEMS.contains(&params["item"]["type"].as_str().unwrap_or_default()) =>
                {
                    tracing::warn!(item = %params["item"]["type"], "item do Codex fora do texto encerrou a conversa");
                    interrupt(rpc, &thread, &turn_id).await;
                    bail!("open_chat_tool_refused");
                }
                "item/agentMessage/delta" if !on_delta(params["delta"].as_str().unwrap_or_default()) => {
                    interrupt(rpc, &thread, &turn_id).await;
                    bail!("open_chat_too_long");
                }
                "turn/completed" => {
                    ensure!(params["turn"]["status"] == "completed", "open_chat_failed");
                    return Ok(thread);
                }
                _ => {}
            }
        }
    };
    tokio::select! {
        result = tokio::time::timeout(TURN_LIMIT, flow) => result.map_err(|_| anyhow!("open_chat_timeout"))?,
        _ = cancel.wait_for(|stop| *stop) => bail!("open_chat_cancelled"),
    }
}

async fn interrupt(rpc: &mut Rpc, thread: &str, turn: &str) {
    let _ =
        rpc.write(json!({"id": 9, "method": "turn/interrupt", "params": {"threadId": thread, "turnId": turn}})).await;
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 100 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[cfg(all(test, unix))]
#[path = "open_chat_tests.rs"]
mod tests;
