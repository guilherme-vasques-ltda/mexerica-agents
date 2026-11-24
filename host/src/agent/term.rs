// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Setup terminals: installing an agent's CLI or signing in to it runs in a
//! real PTY on this computer, streamed to the client that started it. The
//! host picks the command (from `backends::HARNESSES`); clients only type.

#[path = "../fork/terminal_login/pty.rs"]
mod login_pty;

use crate::agent::backends;
use crate::fork::autoridade::Autoridade;
use crate::hub::Hub;
use crate::{LockExt, api::devices::Caller};
use anyhow::{Result, anyhow, bail};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use portable_pty::{Child, MasterPty, PtyPair, PtySize, native_pty_system};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{Read as _, Write as _};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};

/// Output kept for a client that (re)attaches mid-run.
const SCROLLBACK: usize = 256 * 1024;
/// A finished terminal stays readable this long.
const LINGER: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Step {
    Install,
    /// Codync's Node.js, for agents that run on it (one terminal for every agent).
    Node,
    Login,
}

enum Input {
    Data(Vec<u8>),
    Resize(u16, u16),
    Kill,
    Scoped(std::sync::Weak<Hub>, Box<Autoridade>, Box<Self>),
}

impl Input {
    fn with_authority(self, effect: impl FnOnce(Self)) {
        match self {
            Self::Scoped(hub, authority, input) => {
                if let Some(hub) = hub.upgrade() {
                    if authority.com_saida(&hub, None, || effect(*input)).is_err() {
                        tracing::warn!(reason = "request_admission_ended", "terminal input denied");
                    }
                } else {
                    tracing::debug!(reason = "hub_unavailable", "terminal input discarded");
                }
            }
            input => effect(input),
        }
    }
}

/// A linha que o PTY executa (`comando`) e o que o cabeçalho do terminal mostra (`cabecalho`):
/// iguais, exceto no script de fornecedor, que só roda embrulhado na confirmação (GUI-145).
struct Linha {
    comando: String,
    cabecalho: String,
}

struct Output {
    scrollback: Vec<u8>,
    exit: Option<i32>,
}

pub struct Term {
    key: (String, Step),
    owner: Option<String>,
    pub(crate) login: Option<Arc<crate::fork::terminal_login::Login>>,
    input: mpsc::UnboundedSender<Input>,
    output: Mutex<Output>,
    /// `{"type":"output","data":<base64>}` and `{"type":"exit","code":n}`.
    events: broadcast::Sender<Value>,
}

impl Term {
    pub(crate) fn require_access(&self, caller: &Caller) -> Result<()> {
        if let Some(login) = &self.login {
            return login.require(caller);
        }
        crate::fork::engine_setup::require_legacy_setup(caller, &self.key.0)?;
        match caller {
            Caller::Local => Ok(()),
            Caller::Device { key, scopes, .. }
                if self.owner.as_deref() == Some(key) && scopes.contains(&crate::store::Scope::Control) =>
            {
                Ok(())
            }
            _ => bail!("not_allowed: este terminal pertence a outro chamador."),
        }
    }

    /// The scrollback so far plus a live feed, taken atomically so nothing is lost or doubled.
    pub fn attach(&self) -> (Vec<Value>, broadcast::Receiver<Value>) {
        let out = self.output.locked();
        let rx = self.events.subscribe();
        let mut head = vec![];
        if let Some(login) = &self.login {
            login.ready.notify_one();
        }
        if !out.scrollback.is_empty() {
            head.push(json!({"type": "output", "data": B64.encode(&out.scrollback)}));
        }
        if let Some(code) = out.exit {
            head.push(json!({"type": "exit", "code": code}));
        }
        (head, rx)
    }

    fn push(&self, bytes: &[u8]) {
        if let Some(login) = &self.login {
            let Ok(bytes) = login.output(bytes) else {
                login.stop();
                let _ = self.input.send(Input::Kill);
                return;
            };
            if login.take_rejection_notice()
                && login.deliver(|| self.events.send(json!({"type":"notice","code":"login_code_rejected"}))).is_err()
            {
                let _ = self.input.send(Input::Kill);
            }
            if !bytes.is_empty()
                && login.deliver(|| self.events.send(json!({"type":"output","data":B64.encode(&bytes)}))).is_err()
            {
                let _ = self.input.send(Input::Kill);
            }
            return;
        }
        let mut out = self.output.locked();
        out.scrollback.extend_from_slice(bytes);
        if out.scrollback.len() > SCROLLBACK {
            let cut = out.scrollback.len() - SCROLLBACK;
            out.scrollback.drain(..cut);
        }
        let _ = self.events.send(json!({"type":"output","data":B64.encode(bytes)}));
    }
    pub(crate) fn disconnect_login(&self) {
        if let Some(login) = &self.login {
            login.stop();
            let _ = self.input.send(Input::Kill);
        }
    }
    fn finish(&self, code: i32) {
        self.output.locked().exit = Some(code);
        let _ = self.events.send(json!({"type": "exit", "code": code}));
    }
}

#[derive(Default)]
pub struct Terms(Mutex<HashMap<String, Arc<Term>>>);

impl Terms {
    pub fn busy(&self) -> bool {
        self.0.locked().values().any(|term| term.login.is_none() && term.output.locked().exit.is_none())
    }

    pub fn get(&self, id: &str) -> Option<Arc<Term>> {
        self.0.locked().get(id).cloned()
    }

    /// Terminal de prova sem processo; devolve o emissor dos eventos.
    #[cfg(test)]
    pub(crate) fn fixture(&self, id: &str, owner: Option<&str>) -> broadcast::Sender<Value> {
        let events = broadcast::channel(16).0;
        self.0.locked().insert(
            id.into(),
            Arc::new(Term {
                key: ("fixture-backend".into(), Step::Login),
                owner: owner.map(str::to_owned),
                login: None,
                input: mpsc::unbounded_channel().0,
                output: Mutex::new(Output { scrollback: Vec::new(), exit: None }),
                events: events.clone(),
            }),
        );
        events
    }

    pub(crate) fn authorized(&self, id: &str, caller: &Caller) -> Result<Option<Arc<Term>>> {
        let term = self.get(id);
        if let Some(term) = &term {
            term.require_access(caller)?;
        }
        Ok(term)
    }

    /// Starts `step` for `backend` (a sign-in `method` the agent advertised, or
    /// Codync's own command for it), or returns the one already running.
    pub async fn start(
        self: &Arc<Self>,
        caller: &Caller,
        backend: &str,
        step: Step,
        method: Option<&str>,
        cols: u16,
        rows: u16,
    ) -> Result<String> {
        backends::require_supported_execution(backend)?;
        anyhow::ensure!(step != Step::Login || matches!(caller, Caller::Local), "Use o terminal de login autorizado.");
        crate::fork::engine_setup::require_legacy_setup(caller, backend)?;
        let owner = match caller {
            Caller::Local => None,
            Caller::Device { key, scopes, .. } if scopes.contains(&crate::store::Scope::Control) => Some(key.clone()),
            _ => bail!("not_allowed: terminal exige autoridade de controle."),
        };
        let h = backends::harness(backend);
        let name = h.map_or(backend, |h| h.name);
        // A linha que roda e o que o cabeçalho do terminal mostra: iguais, exceto quando o
        // passo é um script de fornecedor, que só roda embrulhado na confirmação (GUI-145).
        let linha = match step {
            Step::Node => {
                let command =
                    format!("{} node", crate::shell::quote(&crate::service::current_exe()?.to_string_lossy()));
                Linha { comando: command.clone(), cabecalho: command }
            }
            Step::Install => {
                let h = h.ok_or_else(|| anyhow!("Mexerica downloads {name} by itself"))?;
                let command = h.install.and_then(backends::Install::command).ok_or_else(|| anyhow!("{}", h.setup))?;
                let linha = if h.install.is_some_and(backends::Install::is_script) {
                    crate::fork::instalacao_script::linha_com_confirmacao(&command)
                } else {
                    command.clone()
                };
                Linha { comando: linha, cabecalho: command }
            }
            // Some CLIs (codex) delete the current credentials the moment a new sign-in starts.
            Step::Login if backends::signed_in(backend) == Some(true) => bail!("{name} is already signed in"),
            Step::Login => {
                let command = if let Some(m) = method {
                    crate::agent::auth::terminal_command(backend, m)
                        .ok_or_else(|| anyhow!("That sign-in option is gone; check {name} again"))?
                } else {
                    backends::login_command(backend).await?
                };
                Linha { comando: command.clone(), cabecalho: command }
            }
        };
        self.spawn((backend.to_owned(), step), owner, &linha, cols, rows)
    }

    fn spawn(
        self: &Arc<Self>,
        key: (String, Step),
        owner: Option<String>,
        linha: &Linha,
        cols: u16,
        rows: u16,
    ) -> Result<String> {
        self.spawn_with(key, owner, linha, cols, rows, None)
    }
    fn spawn_with(
        self: &Arc<Self>,
        key: (String, Step),
        owner: Option<String>,
        linha: &Linha,
        cols: u16,
        rows: u16,
        login: Option<Arc<crate::fork::terminal_login::Login>>,
    ) -> Result<String> {
        let mut map = self.0.locked();
        if let Some((id, _)) = map.iter().find(|(_, t)| {
            t.key == key
                && t.owner == owner
                && t.output.locked().exit.is_none()
                && match (&t.login, &login) {
                    (Some(a), Some(b)) => a.authority == b.authority && a.space == b.space,
                    (None, None) => true,
                    _ => false,
                }
        }) {
            return Ok(id.clone());
        }
        let PtyPair { master, slave } = native_pty_system().openpty(size(cols, rows))?;
        let builder = if let Some(login) = &login {
            let hub = login.hub.upgrade().ok_or_else(|| anyhow!("Login encerrado."))?;
            login.authority.require_current(&hub)?;
            let mut builder = crate::fork::motor_exec::pty(&login.executable, &login.home)?;
            builder.env_clear();
            builder.args(&login.args);
            for (k, v) in &login.env {
                builder.env(k, v);
            }
            builder.cwd(crate::fork::motor_exec::hub_cwd(&login.home));
            #[cfg(unix)]
            builder.umask(Some(0o077));
            builder
        } else {
            let shell = crate::shell::line(&linha.comando);
            let cwd = dirs::home_dir().unwrap_or_else(|| "/".into());
            let mut builder = crate::fork::motor_exec::pty(shell.program, &cwd)?;
            if crate::fork::motor_exec::configured() {
                builder.env_clear();
                for (key, value) in super::environment::inherited(std::env::vars_os()) {
                    builder.env(key, value);
                }
            }
            builder.args(&shell.args);
            for (k, v) in &shell.env {
                builder.env(k, v);
            }
            builder.env("TERM", "xterm-256color");
            builder.env("COLORTERM", "truecolor");
            builder.cwd(crate::fork::motor_exec::hub_cwd(&cwd));
            builder
        };
        let child = if let Some(login) = &login {
            login.deliver(|| slave.spawn_command(builder))??
        } else {
            slave.spawn_command(builder)?
        };
        // Only the child may hold the terminal side, or the output never ends.
        drop(slave);
        let (tx, rx) = mpsc::unbounded_channel();
        let term = Arc::new(Term {
            key,
            owner,
            login,
            input: tx,
            output: Mutex::new(Output { scrollback: vec![], exit: None }),
            events: broadcast::channel(256).0,
        });
        if term.login.is_none() {
            term.push(format!("\x1b[2m$ {}\x1b[0m\r\n", linha.cabecalho).as_bytes());
        }
        let id = uuid::Uuid::new_v4().to_string();
        map.insert(id.clone(), term.clone());
        tracing::info!(backend = term.key.0, step = ?term.key.1, term = %id, "setup terminal started");
        if term.login.is_some() {
            tokio::spawn(login_pty::run(self.clone(), id.clone(), term, master, child, rx));
        } else {
            tokio::spawn(run(self.clone(), id.clone(), term, master, child, rx));
        }
        Ok(id)
    }

    #[cfg(test)]
    pub fn write(&self, id: &str, data: &str) -> Result<()> {
        let bytes = B64.decode(data).map_err(|_| anyhow!("Entrada de terminal inválida."))?;
        let term = self.get(id).ok_or_else(|| anyhow!("that terminal is gone"))?;
        if let Some(login) = &term.login {
            return login.enqueue(&bytes, |bytes| {
                term.input.send(Input::Data(bytes)).map_err(|_| anyhow!("that terminal is gone"))
            });
        }
        self.send(id, Input::Data(bytes))
    }

    pub(crate) fn remote_input(&self, hub: &Arc<Hub>, caller: &Caller, id: &str, data: &str) -> Result<()> {
        let term = self.get(id).ok_or_else(|| anyhow!("that terminal is gone"))?;
        term.require_access(caller)?;
        let bytes = B64.decode(data).map_err(|_| anyhow!("Entrada de terminal inválida."))?;
        if let Some(login) = &term.login {
            return login.enqueue(&bytes, |bytes| {
                term.input.send(Input::Data(bytes)).map_err(|_| anyhow!("that terminal is gone"))
            });
        }
        Self::send_scoped(hub, caller, &term, Input::Data(bytes));
        Ok(())
    }

    pub(crate) fn remote_resize(&self, hub: &Arc<Hub>, caller: &Caller, id: &str, cols: u16, rows: u16) -> Result<()> {
        let term = self.get(id).ok_or_else(|| anyhow!("that terminal is gone"))?;
        term.require_access(caller)?;
        if self.get(id).is_some_and(|t| t.login.is_some()) {
            anyhow::ensure!((20..=240).contains(&cols) && (4..=100).contains(&rows), "Dimensões de login inválidas.");
        }
        Self::send_scoped(hub, caller, &term, Input::Resize(cols, rows));
        Ok(())
    }

    fn send_scoped(hub: &Arc<Hub>, caller: &Caller, term: &Term, input: Input) {
        let input = if let Some(authority) = Autoridade::de_requisicao(caller).filter(|_| term.login.is_none()) {
            Input::Scoped(Arc::downgrade(hub), Box::new(authority.clone()), Box::new(input))
        } else {
            input
        };
        let _ = term.input.send(input);
    }

    pub fn close(&self, id: &str) {
        if let Some(t) = self.0.locked().remove(id) {
            if let Some(login) = &t.login {
                login.stop();
            }
            let _ = t.input.send(Input::Kill);
        }
    }

    #[cfg(test)]
    fn send(&self, id: &str, input: Input) -> Result<()> {
        let t = self.get(id).ok_or_else(|| anyhow!("that terminal is gone"))?;
        // Input after exit has nowhere to go; not an error for the typist.
        let _ = t.input.send(input);
        Ok(())
    }
}

/// Windows' pseudo console asks where the cursor is (`ESC[6n`) before the program starts,
/// and waits for the answer. The host answers it (the line under the `$ command` header) and
/// keeps the question from clients, which would answer it a second time into the program.
const CURSOR_QUERY: &[u8] = b"\x1b[6n";
const CURSOR_REPLY: &[u8] = b"\x1b[2;1R";

/// `chunk` without its cursor query, if it has one.
fn without_cursor_query(chunk: &[u8]) -> Option<Vec<u8>> {
    let at = chunk.windows(CURSOR_QUERY.len()).position(|w| w == CURSOR_QUERY)?;
    Some([&chunk[..at], &chunk[at + CURSOR_QUERY.len()..]].concat())
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize { rows: rows.max(4), cols: cols.max(20), pixel_width: 0, pixel_height: 0 }
}

async fn run(
    terms: Arc<Terms>,
    id: String,
    term: Arc<Term>,
    master: Box<dyn MasterPty + Send>,
    mut child: Box<dyn Child + Send + Sync>,
    mut rx: mpsc::UnboundedReceiver<Input>,
) {
    let mut killer = child.clone_killer();
    let group = crate::fork::motor_exec::Group::pty(child.process_id());
    let streams = master.try_clone_reader().and_then(|reader| Ok((reader, master.take_writer()?)));
    let (mut reader, mut writer) = match streams {
        Ok(streams) => streams,
        Err(error) => {
            tracing::warn!(error = format!("{error:#}"), "setup terminal has no streams");
            if group.is_none() {
                let _ = killer.kill();
            }
            term.finish(-1);
            return;
        }
    };
    let mut pump = {
        let term = term.clone();
        tokio::task::spawn_blocking(move || {
            let mut buf = vec![0u8; 16 * 1024];
            let mut answered = !cfg!(windows);
            // EOF or an error once the child and its children are gone.
            while let Ok(n @ 1..) = reader.read(&mut buf) {
                match without_cursor_query(&buf[..n]).filter(|_| !answered) {
                    Some(rest) => {
                        answered = true;
                        let _ = term.input.send(Input::Data(CURSOR_REPLY.to_vec()));
                        term.push(&rest);
                    }
                    None => term.push(&buf[..n]),
                }
            }
        })
    };
    let mut wait = tokio::task::spawn_blocking(move || child.wait());
    let status = loop {
        tokio::select! {
            status = &mut wait => break status,
            input = rx.recv() => match input {
                // Keystrokes are small; a PTY takes them without blocking.
                Some(Input::Data(bytes)) => {
                    if let Err(error) = writer.write_all(&bytes) {
                        tracing::info!(%error, "setup terminal write failed");
                    }
                }
                Some(Input::Resize(cols, rows)) => {
                    let _ = master.resize(size(cols, rows));
                }
                Some(Input::Scoped(hub, authority, input)) => match *input {
                    Input::Data(bytes) => {
                        if hub.upgrade().is_some_and(|hub| authority.com_saida(&hub, None, || ()).is_ok()) {
                            let _ = writer.write_all(&bytes);
                        } else {
                            tracing::warn!(reason = "request_admission_ended", "terminal input denied");
                        }
                    }
                    input => Input::Scoped(hub, authority, Box::new(input)).with_authority(|input| {
                        if let Input::Resize(cols, rows) = input {
                            let _ = master.resize(size(cols, rows));
                        }
                    }),
                },
                Some(Input::Kill) | None => {
                    if let Some(group) = &group { group.terminate(); } else { let _ = killer.kill(); }
                }
            },
        }
    };
    drop(group);
    // Let the last output drain; a background grandchild can hold the PTY open forever, and
    // Windows' pseudo console keeps it open until the master side closes.
    let _ = tokio::time::timeout(Duration::from_secs(1), &mut pump).await;
    drop((writer, master));
    let code = match status {
        Ok(Ok(status)) => i32::try_from(status.exit_code()).unwrap_or(-1),
        _ => -1,
    };
    tracing::info!(term = %id, code, "setup terminal finished");
    term.finish(code);
    tokio::time::sleep(LINGER).await;
    terms.0.locked().remove(&id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_out_the_cursor_query() {
        assert_eq!(without_cursor_query(b"a\x1b[6nb").as_deref(), Some(&b"ab"[..]));
        assert_eq!(without_cursor_query(b"plain"), None);
    }

    /// GUI-145: a confirmação do script do fornecedor no PTY de verdade — "s" executa o
    /// comando embrulhado; resposta fora da lista cancela sem executar nada.
    #[cfg(unix)]
    #[tokio::test]
    async fn confirmacao_no_pty_so_executa_com_linha_sim() {
        let terms = Arc::new(Terms::default());
        for (resposta, esperado, marcador) in [("s\n", 0i64, Some("pronto-executado")), ("x\n", 130, None)] {
            // A saída do comando difere do literal do comando: o cabeçalho sempre mostra o
            // comando inteiro, então o marcador só pode vir da execução de verdade.
            let comando = "printf 'pronto-%s' executado";
            let id = terms
                .spawn(
                    ("confirm".into(), Step::Login),
                    None,
                    &Linha {
                        comando: crate::fork::instalacao_script::linha_com_confirmacao(comando),
                        cabecalho: comando.to_owned(),
                    },
                    80,
                    24,
                )
                .unwrap();
            let term = terms.get(&id).unwrap();
            let (_, mut rx) = term.attach();
            terms.write(&id, &B64.encode(resposta)).unwrap();
            let code = tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    let v = rx.recv().await.unwrap();
                    if v["type"] == "exit" {
                        return v["code"].as_i64().unwrap();
                    }
                }
            })
            .await
            .unwrap();
            let (head, _) = term.attach();
            let text = String::from_utf8(
                head.iter().flat_map(|v| B64.decode(v["data"].as_str().unwrap_or_default()).unwrap()).collect(),
            )
            .unwrap();
            assert_eq!(code, esperado, "{text}");
            assert_eq!(marcador.is_some_and(|m| text.contains(m)), marcador.is_some(), "{text}");
            if marcador.is_none() {
                assert!(text.contains("Cancelado: nada foi executado."), "{text}");
            }
        }
    }

    #[tokio::test]
    async fn runs_in_a_pty_and_takes_input() {
        let terms = Arc::new(Terms::default());
        // `call` expands `%x%` again once `set /p` has read it.
        let (command, enter) = if cfg!(windows) {
            ("echo tty& set /p x=& call echo got-%x%", "\r")
        } else {
            ("[ -t 0 ] && echo tty; read x; echo got-$x", "\n")
        };
        let linha = Linha { comando: command.into(), cabecalho: command.into() };
        let id = terms.spawn(("test".into(), Step::Login), None, &linha, 80, 24).unwrap();
        let term = terms.get(&id).unwrap();
        let (_, mut rx) = term.attach();
        terms.write(&id, &B64.encode(format!("hi{enter}"))).unwrap();
        let code = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let v = rx.recv().await.unwrap();
                if v["type"] == "exit" {
                    return v["code"].as_i64().unwrap();
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(code, 0);
        let (head, _) = term.attach();
        let text = String::from_utf8(B64.decode(head[0]["data"].as_str().unwrap()).unwrap()).unwrap();
        assert!(text.contains("tty") && text.contains("got-hi"), "{text}");
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod authority_tests {
    use super::*;
    use crate::{
        api::{dispatch, term_events},
        hub::Hub,
        remote::identity::Identity,
        store::{Scope, Store},
    };

    fn device(key: &str) -> Caller {
        Caller::Device { key: key.into(), scopes: vec![Scope::Control], autoridade: None }
    }
    fn terminal(hub: &Hub, id: &str, backend: &str, owner: Option<&str>) -> mpsc::UnboundedReceiver<Input> {
        let (input, rx) = mpsc::unbounded_channel();
        hub.terms.0.locked().insert(
            id.into(),
            Arc::new(Term {
                key: (backend.into(), Step::Login),
                owner: owner.map(str::to_owned),
                login: None,
                input,
                output: Mutex::new(Output { scrollback: b"fixture-private-output".to_vec(), exit: None }),
                events: broadcast::channel(4).0,
            }),
        );
        rx
    }

    #[tokio::test]
    async fn resize_remoto_confere_a_admissao_ao_consumir() {
        use crate::{
            fork::workspace::{self, Motivo},
            store::{Device, DeviceSource, now_ms},
        };
        let root = tempfile::tempdir().unwrap();
        let hub = Hub::new(
            Store::open(std::path::Path::new(":memory:")).unwrap(),
            "fixture".into(),
            Identity::load_or_create(root.path()).unwrap(),
            "fixture".into(),
            0,
        );
        let key = "fixture-pty-device";
        hub.store
            .put_device(&Device {
                key: key.into(),
                name: "Fixture".into(),
                platform: "ios".into(),
                source: DeviceSource::Local,
                grant_id: None,
                scopes: vec![Scope::Control],
                lease_until: None,
                created_at: now_ms(),
                last_seen_at: None,
            })
            .unwrap();
        let (_, mut caller) = crate::fork::autoridade::autenticar(&hub, key).unwrap();
        let Caller::Device { autoridade: Some(original), .. } = &mut caller else { panic!("aparelho validado") };
        *original =
            Arc::new(original.para_requisicao(hub.workspace.atual(), original.geracao_capturada().unwrap()).unwrap());
        let term = "fixture-pty";
        let mut input = terminal(&hub, term, "fixture-backend", Some(key));
        let mut writes = 0;
        for changed in [false, true] {
            hub.terms.remote_resize(&hub, &caller, term, 80, 24).unwrap();
            let action = input.try_recv().unwrap();
            if changed {
                workspace::bump(&hub, Motivo::RoleChanged).unwrap();
            }
            action.with_authority(|event| {
                assert!(matches!(event, Input::Resize(80, 24)));
                writes += 1;
            });
            assert_eq!(writes, 1);
        }
    }
    #[tokio::test]
    async fn provedores_legados_exigem_local_em_setup_controles_e_saida() {
        let root = std::env::temp_dir().join(format!("d164-term-auth-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let hub = Hub::new(
            Store::open(std::path::Path::new(":memory:")).unwrap(),
            "fixture".into(),
            Identity::load_or_create(&root).unwrap(),
            "fixture".into(),
            0,
        );
        for backend in ["claude", "codex", "claude-acp", "claude-agent-acp", "codex-acp"] {
            let id = format!("term-{backend}");
            let mut input = terminal(&hub, &id, backend, None);
            for caller in [device("alice"), Caller::Pairing { key: "pairing".into() }] {
                for step in ["install", "login"] {
                    assert!(
                        dispatch(&hub, &caller, "agentSetup", json!({"backend":backend,"step":step}))
                            .await
                            .unwrap_err()
                            .to_string()
                            .starts_with("remote_setup_unavailable")
                    );
                }
                assert!(
                    dispatch(&hub, &caller, "agentAuthenticate", json!({"backend":backend,"method":"fixture"}))
                        .await
                        .unwrap_err()
                        .to_string()
                        .starts_with("remote_setup_unavailable")
                );
                assert!(term_events(&hub, &id, &caller).is_err());
                for method in ["termInput", "termResize", "termClose"] {
                    assert!(
                        dispatch(&hub, &caller, method, json!({"term":id,"data":"ZmFrZQ==","cols":100,"rows":24}))
                            .await
                            .is_err()
                    );
                }
            }
            assert!(term_events(&hub, &id, &Caller::Local).unwrap().is_some());
            dispatch(&hub, &Caller::Local, "termInput", json!({"term":id,"data":B64.encode("fixture-input")}))
                .await
                .unwrap();
            assert!(matches!(input.try_recv().unwrap(),Input::Data(bytes) if bytes == b"fixture-input"));
            dispatch(&hub, &Caller::Local, "termResize", json!({"term":id,"cols":100,"rows":24})).await.unwrap();
            assert!(matches!(input.try_recv().unwrap(), Input::Resize(100, 24)));
            dispatch(&hub, &Caller::Local, "termClose", json!({"term":id})).await.unwrap();
            assert!(matches!(input.try_recv().unwrap(), Input::Kill));
        }
        let mut input = terminal(&hub, "other", "other-backend", Some("alice"));
        let alice = device("alice");
        let bob = device("bob");
        assert!(term_events(&hub, "other", &alice).unwrap().is_some());
        assert!(term_events(&hub, "other", &bob).is_err());
        for method in ["termInput", "termResize", "termClose"] {
            assert!(
                dispatch(&hub, &bob, method, json!({"term":"other","data":"ZmFrZQ==","cols":100,"rows":24}))
                    .await
                    .is_err()
            );
        }
        dispatch(&hub, &alice, "termInput", json!({"term":"other","data":B64.encode("allowed")})).await.unwrap();
        assert!(matches!(input.try_recv().unwrap(),Input::Data(bytes) if bytes == b"allowed"));
        dispatch(&hub, &alice, "termResize", json!({"term":"other","cols":100,"rows":24})).await.unwrap();
        assert!(matches!(input.try_recv().unwrap(), Input::Resize(100, 24)));
        dispatch(&hub, &alice, "termClose", json!({"term":"other"})).await.unwrap();
        assert!(matches!(input.try_recv().unwrap(), Input::Kill));
        std::fs::remove_dir_all(root).unwrap();
    }

    /// GUI-145: instalar um motor por script do fornecedor mostra o comando e só executa
    /// com confirmação no terminal; recusar não baixa nem executa nada. O Cursor serve de
    /// motor mantido com instalação por script (o Amp saiu da oferta desta versão).
    #[cfg(unix)]
    #[tokio::test]
    async fn instalacao_por_script_pede_confirmacao_no_terminal() {
        let root = std::env::temp_dir().join(format!("d164-term-confirm-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let hub = Hub::new(
            Store::open(std::path::Path::new(":memory:")).unwrap(),
            "fixture".into(),
            Identity::load_or_create(&root).unwrap(),
            "fixture".into(),
            0,
        );
        let id = dispatch(
            &hub,
            &Caller::Local,
            "agentSetup",
            json!({"backend":"cursor","step":"install","cols":80,"rows":24}),
        )
        .await
        .unwrap()["term"]
            .as_str()
            .unwrap()
            .to_owned();
        let term = hub.terms.get(&id).unwrap();
        let (_, mut rx) = term.attach();
        dispatch(&hub, &Caller::Local, "termInput", json!({"term":id,"data":B64.encode("n\n")})).await.unwrap();
        let codigo = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let v = rx.recv().await.unwrap();
                if v["type"] == "exit" {
                    return v["code"].as_i64().unwrap();
                }
            }
        })
        .await
        .unwrap();
        let (head, _) = term.attach();
        let text = String::from_utf8(
            head.iter().flat_map(|v| B64.decode(v["data"].as_str().unwrap_or_default()).unwrap()).collect(),
        )
        .unwrap();
        assert_eq!(codigo, 130, "recusar não é sucesso de instalação: {text}");
        // O terminal mostra o comando exato e a pergunta antes de qualquer download.
        assert!(text.contains("curl https://cursor.com/install -fsS | bash"), "{text}");
        assert!(text.contains("Rodar agora? [s/N]"), "{text}");
        assert!(text.contains("Cancelado: nada foi executado."), "{text}");
        dispatch(&hub, &Caller::Local, "termClose", json!({"term":id})).await.unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(all(test, unix))]
mod cancellation_tests {
    use super::*;
    #[tokio::test]
    async fn motor_fechar_pty_legado_encerra_filho_e_neto() {
        let Some(case) = crate::fork::motor_exec::test_support::case(
            "agent::term::cancellation_tests::motor_fechar_pty_legado_encerra_filho_e_neto",
        ) else {
            return;
        };
        let fixture = case.fixture("terminal-auth", true);
        let citado = crate::shell::quote(&fixture.to_string_lossy());
        let terms = Arc::new(Terms::default());
        let id = terms
            .spawn(
                ("terminal-auth".into(), Step::Login),
                None,
                &Linha { comando: citado.clone(), cabecalho: citado.clone() },
                80,
                24,
            )
            .unwrap();
        let term = terms.get(&id).unwrap();
        let (_, mut rx) = term.attach();
        case.started(&fixture).await;
        terms.close(&id);
        tokio::time::timeout(Duration::from_secs(8), async {
            while let Ok(value) = rx.recv().await {
                if value["type"] == "exit" {
                    return;
                }
            }
            panic!("PTY fechou sem encerrar a tarefa");
        })
        .await
        .unwrap();
        case.stopped(&fixture).await;
    }
}

#[cfg(all(test, unix))]
mod linger_tests {
    use super::*;
    #[tokio::test]
    async fn motor_linger_nao_repete_kill_group() {
        let Some(case) = crate::fork::motor_exec::test_support::case(
            "agent::term::linger_tests::motor_linger_nao_repete_kill_group",
        ) else {
            return;
        };
        let fixture = case.fixture("linger", true);
        let citado = crate::shell::quote(&fixture.to_string_lossy());
        let terms = Arc::new(Terms::default());
        let id = terms
            .spawn(
                ("linger".into(), Step::Login),
                None,
                &Linha { comando: citado.clone(), cabecalho: citado.clone() },
                80,
                24,
            )
            .unwrap();
        let term = terms.get(&id).unwrap();
        let (_, mut rx) = term.attach();
        case.started(&fixture).await;
        terms.close(&id);
        tokio::time::timeout(Duration::from_secs(8), async {
            while let Ok(value) = rx.recv().await {
                if value["type"] == "exit" {
                    return;
                }
            }
            panic!("PTY não anunciou saída");
        })
        .await
        .unwrap();
        case.stopped(&fixture).await;
        let helper = std::env::var_os("DONNABOT_MOTOR_TEST_HELPER").unwrap();
        let calls = format!("{}.calls", std::path::PathBuf::from(helper).display());
        let before = std::fs::read_to_string(&calls).unwrap();
        tokio::time::pause();
        tokio::time::advance(LINGER + Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        assert!(terms.get(&id).is_none(), "retenção terminou");
        assert_eq!(std::fs::read_to_string(calls).unwrap(), before, "nenhum kill após retenção");
    }
}
