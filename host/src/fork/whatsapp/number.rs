//! R2: o número de cada bot. Pareamento (código ou QR), um `sync --follow` por bot
//! conectado (com reinício e espera crescente) e desconectar/apagar.
//!
//! Estado durável: `donna_whatsapp_numbers` (conectado ou "precisa conectar de novo").
//! Estado vivo: só em memória (pareamento em curso, processo, conectado/caiu).
//! Nenhum lock daqui atravessa `await`; ordem: banco → `bots`.
use super::wacli::{self, Event, Method};
use crate::{LockExt, hub::Hub};
use anyhow::{Result, bail, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Child,
    sync::{Notify, broadcast, oneshot, watch},
};

/// O canal de pareamento do `WhatsApp` fecha antes; isto só evita processo esquecido.
const PAIRING_LIMIT: Duration = Duration::from_secs(300);
const FIRST_CODE: Duration = Duration::from_secs(20);
const BACKOFF_MAX: Duration = Duration::from_secs(300);

#[derive(Clone)]
pub(crate) struct State(Arc<Inner>);

struct Inner {
    root: Mutex<PathBuf>,
    /// Ajuste interno; `None` = variável `DONNABOT_WACLI` ou `whatsapp/bin/wacli` (R7).
    binary: Mutex<Option<PathBuf>>,
    bots: Mutex<HashMap<String, Live>>,
    changed: watch::Sender<u64>,
    closed: AtomicBool,
    runs: AtomicU64,
    /// R3: receptor de avisos e sinais de acordar dos leitores.
    inbox: Arc<super::inbox::Shared>,
    /// MX-18: acorda a drenagem da fila de saída.
    outbox: Mutex<Option<Arc<Notify>>>,
    #[cfg(test)]
    record_failures: Mutex<usize>,
    /// Testes: provedor de transcrição falso, sem rede.
    #[cfg(test)]
    fake_transcriber: Mutex<Option<FakeTranscriber>>,
    #[cfg(test)]
    after_download: Mutex<Option<AfterDownload>>,
}

#[cfg(test)]
pub(super) type FakeTranscriber = Arc<dyn Fn(&Hub, crate::voice::Provider, &[u8]) -> Result<String> + Send + Sync>;
/// Testes: roda logo depois do download da voz, antes da nova conferência.
#[cfg(test)]
pub(super) type AfterDownload = Arc<dyn Fn(&Hub) + Send + Sync>;

impl Default for State {
    fn default() -> Self {
        Self(Arc::new(Inner {
            root: Mutex::new(crate::service::data_dir().join("whatsapp")),
            binary: Mutex::default(),
            bots: Mutex::default(),
            changed: watch::Sender::new(0),
            closed: AtomicBool::new(false),
            runs: AtomicU64::new(0),
            inbox: Arc::default(),
            outbox: Mutex::default(),
            #[cfg(test)]
            record_failures: Mutex::default(),
            #[cfg(test)]
            fake_transcriber: Mutex::default(),
            #[cfg(test)]
            after_download: Mutex::default(),
        }))
    }
}

#[derive(Default)]
struct Live {
    pairing: Option<Pairing>,
    sync: Option<Follow>,
    /// R8: o `sync` fica parado enquanto o wacli apaga uma conversa (trava do store).
    hold: bool,
}

impl Live {
    fn idle(&self) -> bool {
        self.sync.is_none() && self.pairing.as_ref().is_none_or(|p| p.stop.is_none())
    }
}

struct Pairing {
    run: u64,
    method: Method,
    /// Código de 8 letras ou texto do QR. Só sai para o dono, pela leitura.
    secret: Option<String>,
    outcome: Option<Outcome>,
    /// `Some` enquanto o `wacli auth` está de pé.
    stop: Option<oneshot::Sender<()>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Connected,
    Expired,
    Refused,
    Cancelled,
}

struct Follow {
    run: u64,
    link: Link,
    stop: watch::Sender<bool>,
    /// Sobe a cada `connected`: a confirmação do número vale só para esta conexão.
    epoch: u64,
    /// Número confirmado pelo wacli nesta conexão; some quando ela cai.
    phone: Option<String>,
}

/// De qual sessão veio uma confirmação do número: run do `sync`, conexão e conta.
#[derive(Clone)]
pub(super) struct Confirm {
    pub run: u64,
    pub epoch: u64,
    pub account: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Link {
    Starting,
    Connected,
    Disconnected,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Row {
    Linked,
    LoggedOut,
}

impl State {
    pub(super) fn root(&self) -> PathBuf {
        self.0.root.locked().clone()
    }

    pub(super) fn closing(&self) -> bool {
        self.0.closed.load(Ordering::SeqCst)
    }

    pub(super) fn inbox(&self) -> &Arc<super::inbox::Shared> {
        &self.0.inbox
    }

    pub(super) fn binary(&self) -> Option<PathBuf> {
        wacli::binary(&self.root(), self.0.binary.locked().as_deref())
    }

    #[cfg(test)]
    pub(super) fn set_fake_transcriber(&self, fake: FakeTranscriber) {
        *self.0.fake_transcriber.locked() = Some(fake);
    }

    #[cfg(test)]
    pub(super) fn fake_transcriber(&self) -> Option<FakeTranscriber> {
        self.0.fake_transcriber.locked().clone()
    }

    #[cfg(test)]
    pub(super) fn set_after_download(&self, hook: AfterDownload) {
        *self.0.after_download.locked() = Some(hook);
    }

    #[cfg(test)]
    pub(super) fn after_download(&self) -> Option<AfterDownload> {
        self.0.after_download.locked().clone()
    }

    #[cfg(test)]
    pub(super) fn set_paths(&self, root: PathBuf, binary: Option<PathBuf>) {
        *self.0.root.locked() = root;
        *self.0.binary.locked() = binary;
    }

    fn dir(&self, bot: &str) -> Result<PathBuf> {
        wacli::bot_dir(&self.root(), bot)
    }

    pub(super) fn notify(&self, events: &broadcast::Sender<Value>, bot: &str) {
        self.0.changed.send_modify(|v| *v += 1);
        self.wake_outbox();
        // Só o aviso; QR e código ficam na leitura do dono.
        let _ = events.send(json!({"type": "whatsappChanged", "botId": bot}));
    }

    pub(super) fn set_outbox_notify(&self, notify: Arc<Notify>) {
        *self.0.outbox.locked() = Some(notify);
    }

    #[cfg(test)]
    pub(super) fn fail_next_records(&self, n: usize) {
        *self.0.record_failures.locked() = n;
    }

    #[cfg(test)]
    pub(super) fn take_record_failure(&self) -> bool {
        let mut left = self.0.record_failures.locked();
        if *left == 0 {
            return false;
        }
        *left -= 1;
        true
    }

    pub(super) fn wake_outbox(&self) {
        if let Some(notify) = self.0.outbox.locked().as_ref() {
            notify.notify_one();
        }
    }

    /// R4c: a encarnação de saída (run do `sync` e conexão) só com o número conectado agora.
    pub(super) fn connection(&self, bot: &str) -> Option<(u64, u64)> {
        let bots = self.0.bots.locked();
        let sync = bots.get(bot)?.sync.as_ref()?;
        (sync.link == Link::Connected).then_some((sync.run, sync.epoch))
    }

    /// Estado do número para a leitura do dono.
    pub(super) fn view(&self, bot: &str, row: Option<Row>, allowed: bool, installed: bool) -> Value {
        let bots = self.0.bots.locked();
        let live = bots.get(bot);
        let pairing = live.and_then(|l| l.pairing.as_ref());
        let pairing_active = pairing.is_some_and(|p| p.stop.is_some());
        let state = match row {
            Some(Row::Linked) => match live.and_then(|l| l.sync.as_ref()).map(|s| s.link) {
                Some(Link::Connected) => "connected",
                Some(Link::Disconnected) => "disconnected",
                None if !installed => "not_installed",
                None if !allowed => "stopped",
                Some(Link::Starting) | None => "connecting",
            },
            Some(Row::LoggedOut) if !pairing_active => "needs_reconnect",
            _ if pairing_active => "pairing",
            _ if !installed => "not_installed",
            _ => "none",
        };
        let mut view = json!({"state": state});
        // Telefone só com a sessão conectada e confirmada nesta conexão.
        if state == "connected"
            && let Some(phone) = live.and_then(|l| l.sync.as_ref()).and_then(|s| s.phone.as_ref())
        {
            view["phone"] = json!(phone);
        }
        if let Some(p) = pairing {
            let method = if p.method == Method::Code { "code" } else { "qr" };
            let result = match p.outcome {
                None => "waiting",
                Some(Outcome::Connected) => "connected",
                Some(Outcome::Expired) => "expired",
                Some(Outcome::Refused) => "refused",
                Some(Outcome::Cancelled) => "cancelled",
            };
            let mut pairing = json!({"method": method, "result": result});
            if p.outcome.is_none()
                && let Some(secret) = &p.secret
            {
                pairing[method] = json!(secret);
            }
            view["pairing"] = pairing;
        }
        view
    }

    /// Para tudo (Hub parando) e espera os processos saírem.
    pub(crate) async fn shutdown(&self) {
        self.0.closed.store(true, Ordering::SeqCst);
        self.wake_outbox();
        let bots: Vec<String> = self.0.bots.locked().keys().cloned().collect();
        for bot in &bots {
            self.stop(bot);
        }
        for bot in &bots {
            self.wait_idle(bot).await;
        }
        self.0.inbox.shutdown().await;
    }

    /// Pede para parar pareamento e sync do bot, sem esperar.
    fn stop(&self, bot: &str) {
        let mut bots = self.0.bots.locked();
        let Some(live) = bots.get_mut(bot) else { return };
        if let Some(stop) = live.pairing.as_mut().and_then(|p| p.stop.take()) {
            let _ = stop.send(());
        }
        if let Some(sync) = &live.sync {
            let _ = sync.stop.send(true);
        }
    }

    #[cfg(all(test, unix))]
    pub(super) fn erase_waiting(&self, bot: &str) -> bool {
        self.0
            .bots
            .locked()
            .get(bot)
            .is_some_and(|live| live.hold && live.sync.as_ref().is_some_and(|sync| *sync.stop.borrow()))
    }

    async fn wait_idle(&self, bot: &str) {
        let mut rx = self.0.changed.subscribe();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while !self.0.bots.locked().get(bot).is_none_or(Live::idle) {
            if tokio::time::timeout_at(deadline, rx.changed()).await.is_err() {
                tracing::warn!("whatsapp: o wacli não parou a tempo");
                return;
            }
        }
    }
}

// MARK: banco

pub(super) fn rows(c: &Connection) -> Result<HashMap<String, Row>> {
    let mut stmt = c.prepare("SELECT bot_id,state FROM donna_whatsapp_numbers")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut map = HashMap::new();
    for row in rows {
        let (bot, state) = row?;
        map.insert(bot, if state == "linked" { Row::Linked } else { Row::LoggedOut });
    }
    Ok(map)
}

/// Cada pareamento é uma conta nova (contrato de plugins §3): o ID não se repete. A conta
/// anterior do bot (se havia) sai do registro com as concessões dela.
fn link(c: &Connection, bot: &str) -> Result<()> {
    forget(c, bot)?;
    // Conta nova: a fila do número anterior não vale mais e o ritmo recomeça.
    super::outbox::cancel_pending(c, Some(bot), "number_changed")?;
    c.execute("DELETE FROM donna_whatsapp_pace WHERE bot_id=?1", [bot])?;
    c.execute(
        "INSERT INTO donna_whatsapp_numbers(bot_id,account_id,state,linked_at,phone) VALUES(?1,?2,'linked',?3,NULL)
         ON CONFLICT(bot_id) DO UPDATE SET account_id=excluded.account_id,state='linked',linked_at=excluded.linked_at,
         phone=NULL",
        params![bot, uuid::Uuid::new_v4().to_string(), crate::store::now_ms()],
    )?;
    Ok(())
}

/// Tira do registro a conta do número do bot (concessões e recurso juntos) e o cursor.
/// Não apaga a linha do número; quem chama decide.
pub(super) fn forget(c: &Connection, bot: &str) -> Result<()> {
    let account: Option<String> =
        c.query_row("SELECT account_id FROM donna_whatsapp_numbers WHERE bot_id=?1", [bot], |r| r.get(0)).optional()?;
    if let Some(account) = account {
        crate::fork::plugins::whatsapp_grants::unlink(c, &account)?;
    }
    // Sem conta não há "Responder": o que esperava sai da fila.
    super::outbox::cancel_pending(c, Some(bot), "number_gone")?;
    c.execute("DELETE FROM donna_whatsapp_pace WHERE bot_id=?1", [bot])?;
    c.execute("DELETE FROM donna_whatsapp_marked WHERE bot_id=?1", [bot])?;
    c.execute("DELETE FROM donna_whatsapp_cursors WHERE bot_id=?1", [bot])?;
    c.execute("DELETE FROM donna_whatsapp_call_pending WHERE bot_id=?1", [bot])?;
    Ok(())
}

fn row(c: &Connection, bot: &str) -> Result<Option<Row>> {
    let state: Option<String> =
        c.query_row("SELECT state FROM donna_whatsapp_numbers WHERE bot_id=?1", [bot], |r| r.get(0)).optional()?;
    Ok(state.map(|s| if s == "linked" { Row::Linked } else { Row::LoggedOut }))
}

/// Bot que ainda existe na tabela (vivo ou não). Apagado ou ausente = pode limpar.
fn bot_exists(c: &Connection, bot: &str) -> Result<bool> {
    let deleted: Option<bool> = c.query_row("SELECT deleted FROM bots WHERE id=?1", [bot], |r| r.get(0)).optional()?;
    Ok(deleted == Some(false))
}

// MARK: pareamento

/// Inicia (ou renova) o pareamento e espera o primeiro código/QR.
pub(super) async fn pair(hub: &Arc<Hub>, bot: &str, method: Method, phone: Option<String>) -> Result<()> {
    let state = &hub.whatsapp;
    ensure!(!state.0.closed.load(Ordering::SeqCst), "whatsapp_stopping: o Hub está parando.");
    {
        let policy = super::store::policy(&hub.store, &hub.host_id)?;
        ensure!(policy.enabled, "plugin_disabled: ligue o plugin WhatsApp primeiro.");
        ensure!(policy.agents.iter().any(|id| id == bot), "bot_not_found: bot não encontrado.");
        ensure!(
            policy.allowed.iter().any(|id| id == bot),
            "not_allowed: no modo Só o Berga, só o Berga usa o WhatsApp."
        );
        let c = hub.store.db.locked();
        ensure!(row(&c, bot)? != Some(Row::Linked), "already_connected: desconecte o número primeiro.");
    }
    let Some(binary) = state.binary() else {
        bail!("wacli_not_installed: o wacli não está instalado neste computador.");
    };
    let dir = state.dir(bot)?;
    // Renovar = cancelar o anterior e começar outro.
    state.stop(bot);
    state.wait_idle(bot).await;
    wacli::ensure_dir(&dir)?;
    let mut child = wacli::auth(&binary, &dir, method, phone.as_deref())
        .spawn()
        .map_err(|_| anyhow::anyhow!("wacli_failed: não deu para abrir o wacli."))?;
    let stderr = child.stderr.take();
    let run = state.0.runs.fetch_add(1, Ordering::SeqCst);
    let (stop_tx, stop_rx) = oneshot::channel();
    state.0.bots.locked().entry(bot.to_owned()).or_default().pairing =
        Some(Pairing { run, method, secret: None, outcome: None, stop: Some(stop_tx) });
    state.notify(&hub.events, bot);
    // D368: o vínculo é gravado depois, quando o celular confirma; vale a origem deste pedido.
    let origem = crate::fork::donna_fixa::pedido::atual();
    tokio::spawn(drive_pairing(hub.clone(), bot.to_owned(), run, child, stderr, stop_rx, binary, dir, origem));
    let mut rx = state.0.changed.subscribe();
    let deadline = tokio::time::Instant::now() + FIRST_CODE;
    loop {
        let ready = state
            .0
            .bots
            .locked()
            .get(bot)
            .and_then(|l| l.pairing.as_ref())
            .is_none_or(|p| p.run != run || p.secret.is_some() || p.outcome.is_some());
        if ready || tokio::time::timeout_at(deadline, rx.changed()).await.is_err() {
            return Ok(());
        }
    }
}

pub(super) fn cancel(hub: &Arc<Hub>, bot: &str) {
    let stop = hub.whatsapp.0.bots.locked().get_mut(bot).and_then(|l| l.pairing.as_mut()).and_then(|p| p.stop.take());
    if let Some(stop) = stop {
        let _ = stop.send(());
    }
}

#[allow(clippy::too_many_arguments, reason = "Tudo pertence a um único processo de pareamento.")]
async fn drive_pairing(
    hub: Arc<Hub>,
    bot: String,
    run: u64,
    mut child: Child,
    stderr: Option<tokio::process::ChildStderr>,
    mut stop: oneshot::Receiver<()>,
    binary: PathBuf,
    dir: PathBuf,
    origem: Option<crate::fork::donna_fixa::pedido::Origem>,
) {
    let state = &hub.whatsapp;
    let update = |f: &dyn Fn(&mut Pairing)| {
        if let Some(p) = state.0.bots.locked().get_mut(&bot).and_then(|l| l.pairing.as_mut())
            && p.run == run
        {
            f(p);
        }
        state.notify(&hub.events, &bot);
    };
    let (mut linked, mut timed_out, mut forced) = (false, false, None);
    let limit = tokio::time::sleep(PAIRING_LIMIT);
    tokio::pin!(limit);
    if let Some(stderr) = stderr {
        let mut lines = BufReader::new(stderr).lines();
        loop {
            tokio::select! {
                _ = &mut stop => { forced = Some(Outcome::Cancelled); break }
                () = &mut limit => { forced = Some(Outcome::Expired); break }
                line = lines.next_line() => match line {
                    Ok(Some(line)) => match wacli::event(&line) {
                        Event::Qr(code) | Event::PairCode(code) => update(&|p| p.secret = Some(code.clone())),
                        Event::Connected if !linked => {
                            linked = record_link(&hub, &bot, origem.as_ref());
                            if linked {
                                update(&|p| p.outcome = Some(Outcome::Connected));
                            }
                        }
                        Event::TimedOut => timed_out = true,
                        _ => {}
                    },
                    _ => break,
                },
            }
        }
    }
    let success = if forced.is_some() {
        kill(&mut child).await;
        false
    } else {
        matches!(child.wait().await, Ok(s) if s.success())
    };
    if !linked && forced.is_none() && success && wacli::session(&binary, &dir).await.authenticated {
        linked = record_link(&hub, &bot, origem.as_ref());
    }
    let outcome = match forced {
        _ if linked => Outcome::Connected,
        Some(o) => o,
        None if timed_out => Outcome::Expired,
        None => Outcome::Refused,
    };
    update(&|p| {
        p.outcome = Some(outcome);
        p.secret = None;
        p.stop = None;
    });
    reconcile(&hub);
}

fn record_link(hub: &Hub, bot: &str, origem: Option<&crate::fork::donna_fixa::pedido::Origem>) -> bool {
    // O bot pode ter sido apagado durante o pareamento. D368: o vínculo vale só se a autoridade
    // de quem pediu o pareamento ainda vale agora, na mesma transação.
    let ok = crate::fork::donna_fixa::pedido::gravar_com(&hub.store, origem, |tx| {
        anyhow::ensure!(bot_exists(tx, bot)?, "bot_not_found");
        link(tx, bot)
    })
    .is_ok();
    if !ok {
        tracing::warn!("whatsapp: pareamento concluído para bot que não existe mais");
        return false;
    }
    // Conta nova no registro e concessões a partir das opções do bot.
    if let Err(error) = super::store::refresh_grants(&hub.store, &hub.host_id) {
        tracing::warn!(error = %error, "whatsapp: não deu para atualizar as concessões");
    }
    true
}

/// O número confirmado pelo wacli depois de conectar (nunca o digitado no pareamento).
/// A sessão (`Confirm`) é capturada antes do `await`; nenhuma trava atravessa o `await`.
/// Nunca vai para log.
async fn record_phone(hub: Arc<Hub>, bot: String, confirm: Confirm, binary: PathBuf, dir: PathBuf) {
    let phone = wacli::session(&binary, &dir).await.phone;
    apply_phone(&hub, &bot, &confirm, phone);
}

/// Grava o número só se a confirmação ainda é da sessão vigente: mesmo run do `sync`,
/// mesma conexão (`epoch`), ainda conectada, e a mesma conta do pareamento. Resultado de
/// run, conexão ou pareamento antigo é descartado (`false`). Ordem: banco → `bots`.
pub(super) fn apply_phone(hub: &Hub, bot: &str, confirm: &Confirm, phone: Option<String>) -> bool {
    let c = hub.store.db.locked();
    let mut bots = hub.whatsapp.0.bots.locked();
    let Some(follow) = bots.get_mut(bot).and_then(|l| l.sync.as_mut()) else { return false };
    if follow.run != confirm.run || follow.epoch != confirm.epoch || follow.link != Link::Connected {
        return false;
    }
    let Ok(changed) = c.execute(
        "UPDATE donna_whatsapp_numbers SET phone=?3 WHERE bot_id=?1 AND account_id=?2 AND state='linked'",
        params![bot, confirm.account, phone],
    ) else {
        return false;
    };
    if changed != 1 {
        return false;
    }
    follow.phone = phone;
    drop(bots);
    drop(c);
    hub.whatsapp.notify(&hub.events, bot);
    true
}

/// A conta do pareamento atual do bot, para prender a confirmação a ela.
fn current_account(hub: &Hub, bot: &str) -> Option<String> {
    hub.store
        .db
        .locked()
        .query_row("SELECT account_id FROM donna_whatsapp_numbers WHERE bot_id=?1 AND state='linked'", [bot], |r| {
            r.get(0)
        })
        .optional()
        .ok()
        .flatten()
}

#[cfg(test)]
impl State {
    /// Run e conexão vigentes do `sync` do bot (para os testes de confirmação atrasada).
    pub(super) fn current(&self, bot: &str) -> Option<(u64, u64)> {
        self.0.bots.locked().get(bot).and_then(|l| l.sync.as_ref()).map(|s| (s.run, s.epoch))
    }
}

async fn kill(child: &mut Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id().and_then(|p| i32::try_from(p).ok()) {
        // SAFETY: o grupo é o do próprio wacli (`process_group(0)` no spawn).
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    let _ = child.start_kill();
    let _ = child.wait().await;
}

// MARK: supervisor

/// Liga ou desliga o `sync --follow` de cada bot conforme o banco: número conectado,
/// plugin ligado, bot vivo e não pausado, wacli instalado. Pareamento fora da regra cai.
pub(crate) fn reconcile(hub: &Arc<Hub>) {
    let state = &hub.whatsapp;
    if state.0.closed.load(Ordering::SeqCst) {
        return;
    }
    let (allowed, linked) = match super::store::policy(&hub.store, &hub.host_id)
        .and_then(|p| Ok((p.allowed, rows(&hub.store.db.locked())?)))
    {
        Ok(v) => v,
        Err(error) => {
            tracing::warn!(error = %error, "whatsapp: não deu para ler o estado dos números");
            return;
        }
    };
    let allowed: HashSet<String> = allowed.into_iter().collect();
    let binary = state.binary();
    let want: Vec<String> = linked
        .into_iter()
        .filter(|(bot, row)| *row == Row::Linked && allowed.contains(bot) && binary.is_some())
        .map(|(bot, _)| bot)
        .collect();
    let mut start = Vec::new();
    {
        let mut bots = state.0.bots.locked();
        for (bot, live) in bots.iter_mut() {
            if !allowed.contains(bot)
                && let Some(stop) = live.pairing.as_mut().and_then(|p| p.stop.take())
            {
                let _ = stop.send(());
            }
            if let Some(sync) = &live.sync
                && !want.contains(bot)
            {
                let _ = sync.stop.send(true);
            }
        }
        if let Some(binary) = &binary {
            for bot in want {
                let live = bots.entry(bot.clone()).or_default();
                if live.idle()
                    && !live.hold
                    && let Ok(dir) = wacli::bot_dir(&state.root(), &bot)
                {
                    let run = state.0.runs.fetch_add(1, Ordering::SeqCst);
                    let (stop, rx) = watch::channel(false);
                    live.sync = Some(Follow { run, link: Link::Starting, stop, epoch: 0, phone: None });
                    start.push((bot, run, rx, dir, binary.clone()));
                }
            }
        }
    }
    for (bot, run, rx, dir, binary) in start {
        state.notify(&hub.events, &bot);
        tokio::spawn(supervise(hub.clone(), bot, run, rx, binary, dir));
    }
}

async fn supervise(
    hub: Arc<Hub>,
    bot: String,
    run: u64,
    mut stop: watch::Receiver<bool>,
    binary: PathBuf,
    dir: PathBuf,
) {
    let state = &hub.whatsapp;
    // Devolve a conexão nova (`epoch`) quando passa a conectado. Fora de "conectado" o
    // número confirmado some, na memória e no banco, até nova confirmação.
    let set_link = |link: Link| -> u64 {
        if link != Link::Connected {
            let _ =
                hub.store.db.locked().execute("UPDATE donna_whatsapp_numbers SET phone=NULL WHERE bot_id=?1", [&bot]);
        }
        let mut epoch = 0;
        if let Some(s) = state.0.bots.locked().get_mut(&bot).and_then(|l| l.sync.as_mut())
            && s.run == run
        {
            if link == Link::Connected && s.link != Link::Connected {
                s.epoch += 1;
            }
            if link != Link::Connected {
                s.phone = None;
            }
            s.link = link;
            epoch = s.epoch;
        }
        state.notify(&hub.events, &bot);
        epoch
    };
    let mut delay = Duration::from_secs(1);
    let _ = wacli::ensure_dir(&dir);
    wacli::reap(&dir).await;
    // R3: segredo novo do aviso e um leitor do wacli.db enquanto este sync existir.
    let (hook, wake, voice) = state.inbox().register(&bot).await;
    let (reader_stop, reader_rx) = watch::channel(false);
    let reader = tokio::spawn(super::inbox::follow(hub.clone(), bot.clone(), dir.clone(), reader_rx.clone(), wake));
    // R3b: a fila de transcrição do bot, à parte, para não segurar a entrada de texto.
    let voices = tokio::spawn(super::voz::follow(hub.clone(), bot.clone(), dir.clone(), reader_rx, voice));
    loop {
        if *stop.borrow() {
            break;
        }
        let mut logged_out = false;
        let mut stopped = false;
        if let Ok(mut child) = wacli::sync(&binary, &dir, hook.as_ref()).spawn() {
            wacli::write_pid(&dir, child.id());
            set_link(Link::Starting);
            if let Some(stderr) = child.stderr.take() {
                let mut lines = BufReader::new(stderr).lines();
                loop {
                    tokio::select! {
                        changed = stop.changed() => if changed.is_err() || *stop.borrow() { stopped = true; break },
                        line = lines.next_line() => match line {
                            Ok(Some(line)) => match wacli::event(&line) {
                                Event::Connected => {
                                    delay = Duration::from_secs(1);
                                    let epoch = set_link(Link::Connected);
                                    if let Some(account) = current_account(&hub, &bot) {
                                        let confirm = Confirm { run, epoch, account };
                                        tokio::spawn(record_phone(hub.clone(), bot.clone(), confirm, binary.clone(), dir.clone()));
                                    }
                                }
                                Event::Stale => {
                                    set_link(Link::Disconnected);
                                }
                                Event::LoggedOut => logged_out = true,
                                _ => {}
                            },
                            _ => break,
                        },
                    }
                }
            }
            if stopped {
                kill(&mut child).await;
            } else {
                let _ = child.wait().await;
            }
            let _ = std::fs::remove_file(dir.join("wacli.pid"));
        }
        if logged_out {
            // O dono tirou o aparelho no celular (ou o WhatsApp tirou): conectar de novo.
            let c = hub.store.db.locked();
            let _ =
                c.execute("UPDATE donna_whatsapp_numbers SET state='logged_out',phone=NULL WHERE bot_id=?1", [&bot]);
            break;
        }
        if stopped {
            break;
        }
        set_link(Link::Disconnected);
        tokio::select! {
            () = tokio::time::sleep(delay) => {}
            _ = stop.changed() => break,
        }
        delay = (delay * 2).min(BACKOFF_MAX);
    }
    // O leitor sai antes de o sync constar como parado: desconectar não vê entrega depois.
    let _ = reader_stop.send(true);
    let _ = reader.await;
    let _ = voices.await;
    state.inbox().unregister(&bot);
    {
        let mut bots = state.0.bots.locked();
        if let Some(live) = bots.get_mut(&bot)
            && live.sync.as_ref().is_some_and(|s| s.run == run)
        {
            live.sync = None;
        }
    }
    state.notify(&hub.events, &bot);
    // Um pedido de ligar pode ter chegado enquanto este parava.
    reconcile(&hub);
}

// MARK: apagar uma conversa (R8)

/// O que aconteceu com a cópia do wacli ao apagar uma conversa.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WacliErase {
    Done,
    /// O wacli não tem esta conversa (ou nenhuma pasta): nada a apagar lá.
    Skipped,
    Failed,
}

impl WacliErase {
    pub(super) fn json(self) -> Value {
        match self {
            Self::Done => json!({"wacli": "done"}),
            Self::Skipped => json!({"wacli": "skipped"}),
            Self::Failed => json!({
                "wacli": "failed",
                "warning": "Não foi possível confirmar que toda a conversa foi apagada. Tente novamente."
            }),
        }
    }
}

/// O wacli tem a conversa? Leitura só leitura do `wacli.db`, como a caixa.
fn wacli_has_chat(dir: &Path, chat: &str) -> bool {
    let db = dir.join("wacli.db");
    if !db.is_file() {
        return false;
    }
    let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;
    Connection::open_with_flags(&db, flags)
        .and_then(|c| {
            c.query_row(
                "SELECT EXISTS(SELECT 1 FROM chats WHERE jid=?1) OR EXISTS(SELECT 1 FROM messages WHERE chat_jid=?1)",
                [chat],
                |r| r.get(0),
            )
        })
        .unwrap_or(true)
}

/// Apaga a conversa da cópia do wacli pelo comando oficial: para o `sync` do bot (o
/// comando precisa da trava do store), roda `chats cleanup --jid` e religa. O Hub nunca
/// escreve no `wacli.db`.
pub(super) async fn erase_chat(hub: &Arc<Hub>, bot: &str, chat: &str) -> WacliErase {
    let state = &hub.whatsapp;
    let Ok(dir) = state.dir(bot) else { return WacliErase::Skipped };
    if !wacli_has_chat(&dir, chat) {
        return WacliErase::Skipped;
    }
    let Some(binary) = state.binary() else { return WacliErase::Failed };
    let mut changes = state.0.changed.subscribe();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let acquired = {
            let mut bots = state.0.bots.locked();
            let live = bots.entry(bot.to_owned()).or_default();
            // Não interrompe um pareamento; outro apagar deve terminar primeiro.
            if live.pairing.as_ref().is_some_and(|p| p.stop.is_some()) {
                return WacliErase::Failed;
            }
            if live.hold {
                false
            } else {
                live.hold = true;
                if let Some(sync) = &live.sync {
                    let _ = sync.stop.send(true);
                }
                true
            }
        };
        if acquired {
            break;
        }
        let wait_result = tokio::time::timeout_at(deadline, changes.changed()).await;
        if !matches!(wait_result, Ok(Ok(()))) {
            return WacliErase::Failed;
        }
    }
    state.wait_idle(bot).await;
    let idle = state.0.bots.locked().get(bot).is_some_and(Live::idle);
    // CF-27: um lote lido antes do pedido pode ter gravado a conversa depois do primeiro
    // DELETE. Com o leitor parado e `hold` ligado, nada mais entra: apaga de novo.
    let erased = if idle { super::messages::erase_chat(&hub.store.db.locked(), bot, chat) } else { Ok(0) };
    let erased = erased.inspect_err(|e| tracing::warn!("whatsapp: falha ao reapagar a conversa: {e:#}"));
    if erased.as_ref().is_ok_and(|n| *n > 0) {
        state.notify(&hub.events, bot);
    }
    let done = idle && erased.is_ok() && wacli::cleanup_chat(&binary, &dir, chat).await;
    if let Some(live) = state.0.bots.locked().get_mut(bot) {
        live.hold = false;
    }
    // Acorda também outro apagar, mesmo que reconcile não inicie um novo sync.
    state.notify(&hub.events, bot);
    reconcile(hub);
    if done { WacliErase::Done } else { WacliErase::Failed }
}

// MARK: desconectar e apagar

#[derive(Clone, Copy)]
pub(super) enum Logout {
    Done,
    Failed,
    Skipped,
}

impl Logout {
    pub(super) fn json(self) -> Value {
        match self {
            Self::Done => json!({"logout": "done"}),
            Self::Skipped => json!({"logout": "skipped"}),
            Self::Failed => json!({
                "logout": "failed",
                "warning": "Não deu para avisar o WhatsApp. No celular do número, abra Aparelhos conectados e remova este aparelho."
            }),
        }
    }
}

/// "Desconectar" (`erase=false`) e "Desconectar e apagar" (`erase=true`).
pub(super) async fn disconnect(hub: &Arc<Hub>, bot: &str, erase: bool) -> Result<Logout> {
    let state = &hub.whatsapp;
    let dir = state.dir(bot)?;
    // D368: a autoridade original do pedido do dono é reconferida nesta transação (`pedido::gravar`).
    let previous = crate::fork::donna_fixa::pedido::gravar(&hub.store, |tx| {
        ensure!(bot_exists(tx, bot)?, "bot_not_found: bot não encontrado.");
        let previous = row(tx, bot)?;
        // Conta e concessões saem do registro junto com o número (D247).
        forget(tx, bot)?;
        tx.execute("DELETE FROM donna_whatsapp_numbers WHERE bot_id=?1", [bot])?;
        // "Desconectar" mantém a caixa; "Desconectar e apagar" apaga tudo do bot.
        if erase {
            super::messages::erase(tx, bot)?;
        }
        Ok(previous)
    })?;
    let logout = release(state, bot, &dir, previous != Some(Row::LoggedOut), erase).await;
    if let Some(live) = state.0.bots.locked().get_mut(bot) {
        live.pairing = None;
    }
    state.notify(&hub.events, bot);
    logout
}

async fn release(state: &State, bot: &str, dir: &Path, try_logout: bool, erase: bool) -> Result<Logout> {
    state.stop(bot);
    state.wait_idle(bot).await;
    let logout = match state.binary() {
        Some(binary) if try_logout && dir.join("session.db").is_file() => {
            if wacli::logout(&binary, dir).await {
                Logout::Done
            } else {
                Logout::Failed
            }
        }
        _ if try_logout && dir.join("session.db").is_file() => Logout::Failed,
        _ => Logout::Skipped,
    };
    wacli::remove_session(dir);
    if erase {
        wacli::remove_dir(dir)?;
    }
    Ok(logout)
}

/// O bot foi apagado (a linha do número já saiu na mesma transação): faz o mesmo
/// que "Desconectar e apagar". Roda em segundo plano depois do commit.
pub(crate) fn bot_deleted(hub: &Hub, bot: &str) {
    let (state, events, bot) = (hub.whatsapp.clone(), hub.events.clone(), bot.to_owned());
    let Ok(dir) = state.dir(&bot) else { return };
    // Bot sem nada de WhatsApp: nenhum trabalho e nenhum aviso.
    if !dir.exists() && !state.0.bots.locked().contains_key(&bot) {
        return;
    }
    let Ok(runtime) = tokio::runtime::Handle::try_current() else { return };
    runtime.spawn(async move {
        if let Err(error) = release(&state, &bot, &dir, true, true).await {
            tracing::warn!(error = %error, "whatsapp: não deu para apagar a pasta de um bot apagado");
        }
        state.0.bots.locked().remove(&bot);
        state.notify(&events, &bot);
    });
}

/// Início do Hub: mata wacli que sobrou de uma queda, apaga pastas de bots que
/// não existem mais e sobe os processos dos números conectados.
pub(crate) fn start(hub: &Arc<Hub>) {
    let hub = hub.clone();
    tokio::spawn(async move {
        let root = hub.whatsapp.root();
        let names: Vec<String> = std::fs::read_dir(&root)
            .map(|d| d.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect())
            .unwrap_or_default();
        for bot in names {
            let Ok(dir) = wacli::bot_dir(&root, &bot) else { continue };
            if !dir.is_dir() {
                continue;
            }
            wacli::reap(&dir).await;
            let gone = bot_exists(&hub.store.db.locked(), &bot).is_ok_and(|exists| !exists);
            if gone && let Err(error) = release(&hub.whatsapp, &bot, &dir, true, true).await {
                tracing::warn!(error = %error, "whatsapp: pasta de bot apagado ficou para trás");
            }
        }
        // Prazo de guarda das caixas, independente de conexão (agora e de hora em hora).
        tokio::spawn(super::inbox::sweep(hub.clone()));
        // Números conectados antes do R3 ganham a conta no registro aqui.
        if let Err(error) = super::store::refresh_grants(&hub.store, &hub.host_id) {
            tracing::warn!(error = %error, "whatsapp: não deu para atualizar as concessões");
        }
        super::outbox::start(&hub);
        reconcile(&hub);
    });
}

#[path = "card_source.rs"]
mod card_source;
pub(crate) use card_source::{
    WhatsappCardSource, capture_card_source_on, close_draft, with_card_cleanup_on, with_card_source_on,
};
