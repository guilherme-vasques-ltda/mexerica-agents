//! Contexto e guardas do terminal de LOGIN existente. Nenhum PTY ou terminal paralelo.
use super::engine_setup::access::{self, Authority};
use crate::{LockExt, agent::backends, api::devices::Caller, hub::Hub, store::Store};
use anyhow::{Result, anyhow, ensure};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::time::Instant;
pub(crate) mod conclusao;
pub(crate) mod entrega;

static PROFILE_MUTATION: Mutex<()> = Mutex::new(());

pub(crate) const TTL: Duration = Duration::from_secs(9 * 60);
pub(crate) struct Login {
    pub(crate) hub: Weak<Hub>,
    pub(crate) authority: Authority,
    pub(crate) backend: String,
    pub(crate) space: String,
    pub(crate) executable: PathBuf,
    pub(crate) args: Vec<String>,
    pub(crate) home: PathBuf,
    pub(crate) env: Vec<(String, String)>,
    pub(crate) deadline: Instant,
    pub(crate) attached: AtomicBool,
    pub(crate) ready: tokio::sync::Notify,
    pub(crate) cancelled: AtomicBool,
    stopped: tokio::sync::Notify,
    state: Mutex<InputState>,
    admission: Mutex<()>,
    promoted: AtomicBool,
    verified: AtomicBool,
    agent_mode: AtomicBool,
}
#[derive(Default, PartialEq, Eq)]
enum GoogleMethod {
    #[default]
    Unseen,
    Offered,
    Chosen,
}
#[derive(Default, PartialEq, Eq)]
enum Redaction {
    #[default]
    None,
    Submitted,
    Rejected,
}
#[derive(Default)]
struct InputState {
    output: Vec<u8>,
    code: Vec<u8>,
    challenge: bool,
    submitted: bool,
    redaction: Redaction,
    rejection_notice: bool,
    escape: u8,
    /// UTF-8 em curso no estado normal: continuações que faltam e a faixa válida do próximo byte.
    /// Só com isso um byte 0x80–0x9F é texto (`“` é E2 80 9C); fora disso é um C1 cru.
    utf8: Utf8Tail,
    google_method: GoogleMethod,
    /// Bytes aceitos por `enqueue` que ainda não foram escritos no PTY (canal + espera do escritor).
    pending: usize,
}
#[derive(Clone, Copy, Default)]
struct Utf8Tail {
    left: u8,
    lo: u8,
    hi: u8,
}
impl Utf8Tail {
    /// Início de uma sequência multibyte (C3..=F4); `None` para os demais bytes.
    fn lead(byte: u8) -> Option<Self> {
        let (left, lo, hi) = match byte {
            0xC3..=0xDF => (1, 0x80, 0xBF),
            0xE0 => (2, 0xA0, 0xBF),
            0xED => (2, 0x80, 0x9F),
            0xE1..=0xEF => (2, 0x80, 0xBF),
            0xF0 => (3, 0x90, 0xBF),
            0xF4 => (3, 0x80, 0x8F),
            0xF1..=0xF3 => (3, 0x80, 0xBF),
            _ => return None,
        };
        Some(Self { left, lo, hi })
    }
}

/// Teto de bytes aceitos e ainda não escritos no PTY. Passou disso, o envio é recusado (o login segue).
pub(crate) const PENDING_MAX: usize = 8 * 1024;
/// Maior envio que uma única entrada filtrada pode gerar (código de 2048 bytes + Enter).
const ONE_INPUT_MAX: usize = 2049;

pub(crate) fn canonical(id: &str) -> Option<&'static backends::Harness> {
    backends::HARNESSES.iter().find(|h| h.id == id || h.registry == Some(id))
}
pub(crate) fn slot(backend: &str, space: &str) -> String {
    format!("engine-login-profile:v1:{space}:{backend}")
}
pub(crate) fn fixed_args(backend: &str) -> Result<Vec<String>> {
    let h = canonical(backend).ok_or_else(|| anyhow!("Motor sem comando de login verificado."))?;
    // Lista fechada: cada fluxo tem matriz de escape por motor. Nunca usar h.login.
    let args: &[&str] = match h.id {
        "grok" => &["login", "--device-auth"],
        "cursor" => &["login"],
        "antigravity" => &[],
        _ => anyhow::bail!(
            "Terminal de login não oferecido: motor sem prova contra escape. Use chave no cofre ou código de aparelho."
        ),
    };
    Ok(args.iter().map(|arg| (*arg).to_owned()).collect())
}
impl Login {
    pub(crate) fn prepare(hub: &Arc<Hub>, caller: &Caller, backend: &str, space: &str) -> Result<Arc<Self>> {
        ensure!(cfg!(unix), "Terminal seguro de login ainda indisponível neste Hub.");
        let authority = access::resolve(hub, caller)?;
        hub.store.require_space(space)?;
        let h = canonical(backend).ok_or_else(|| anyhow!("Motor desconhecido."))?;
        ensure!(!matches!(h.id, "claude" | "codex"), "Use o login da conta em Contas de IA.");
        let args = fixed_args(h.id)?;
        let executable = h
            .bins
            .iter()
            .find_map(|bin| backends::which(bin))
            .ok_or_else(|| anyhow!("Motor não instalado neste Hub."))?;
        let home = temporary_profile(hub, h.id, space)?;
        let mut env = environment(&home);
        env.extend([
            ("NO_OPEN_BROWSER".into(), "1".into()),
            ("NO_BROWSER".into(), "true".into()),
            ("BROWSER".into(), "/usr/bin/false".into()),
            ("TERM".into(), "xterm-256color".into()),
        ]);
        Ok(Arc::new(Self {
            hub: Arc::downgrade(hub),
            authority,
            backend: h.id.into(),
            space: space.into(),
            executable,
            args,
            home,
            env,
            deadline: Instant::now() + TTL,
            attached: AtomicBool::new(false),
            ready: tokio::sync::Notify::new(),
            cancelled: AtomicBool::new(false),
            stopped: tokio::sync::Notify::new(),
            state: Mutex::default(),
            admission: Mutex::new(()),
            promoted: AtomicBool::new(false),
            verified: AtomicBool::new(false),
            agent_mode: AtomicBool::new(false),
        }))
    }
    #[cfg(test)]
    pub(crate) fn fixture(hub: &Arc<Hub>, caller: &Caller, backend: &str, executable: PathBuf) -> Result<Arc<Self>> {
        Self::fixture_in(hub, caller, backend, executable, crate::fork::spaces::PERSONAL)
    }
    #[cfg(test)]
    pub(crate) fn fixture_in(
        hub: &Arc<Hub>,
        caller: &Caller,
        backend: &str,
        executable: PathBuf,
        space: &str,
    ) -> Result<Arc<Self>> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hub.store.engine_setup_root, std::fs::Permissions::from_mode(0o700))?;
        }
        let authority = access::resolve(hub, caller)?;
        let args = fixed_args(backend)?;
        let home = temporary_profile(hub, backend, space)?;
        Ok(Arc::new(Self {
            hub: Arc::downgrade(hub),
            authority,
            backend: backend.into(),
            space: space.into(),
            executable,
            args,
            env: environment(&home),
            home,
            deadline: Instant::now() + TTL,
            attached: AtomicBool::new(false),
            ready: tokio::sync::Notify::new(),
            cancelled: AtomicBool::new(false),
            stopped: tokio::sync::Notify::new(),
            state: Mutex::default(),
            admission: Mutex::new(()),
            promoted: AtomicBool::new(false),
            verified: AtomicBool::new(false),
            agent_mode: AtomicBool::new(false),
        }))
    }
    pub(crate) fn current(&self) -> bool {
        !self.cancelled.load(Ordering::Acquire)
            && Instant::now() < self.deadline
            && self.hub.upgrade().is_some_and(|h| self.authority.current(&h))
    }
    pub(crate) fn stop(&self) {
        let _guard = self.admission.locked();
        self.cancelled.store(true, Ordering::Release);
        self.stopped.notify_waiters();
    }
    pub(crate) fn require(&self, caller: &Caller) -> Result<()> {
        let hub = self.hub.upgrade().ok_or_else(|| anyhow!("Login encerrado."))?;
        ensure!(
            self.current() && access::resolve(&hub, caller)? == self.authority,
            "Este terminal pertence a outro vínculo."
        );
        self.authority.require_current(&hub)
    }
    pub(crate) fn deliver<T>(&self, send: impl FnOnce() -> T) -> Result<T> {
        let _guard = self.admission.locked();
        ensure!(!self.cancelled.load(Ordering::Acquire), "Login cancelado.");
        let hub = self.hub.upgrade().ok_or_else(|| anyhow!("Login encerrado."))?;
        ensure!(Instant::now() < self.deadline, "Login expirado.");
        match &self.authority {
            Authority::Local => Ok(send()),
            Authority::Owner { transporte, .. } => transporte.com_saida(&hub, Some(&self.authority), send),
        }
    }
    /// Fronteira de entrega ao PTY: a marca de modo agente é posta pela leitura de saída sob o
    /// cadeado de estado, portanto conferir e escrever sob o mesmo cadeado é atômico em relação
    /// a ela — ou a escrita precede a marca, ou é recusada e nada chega ao agente.
    pub(crate) fn deliver_input<T>(&self, send: impl FnOnce() -> T) -> Result<Option<T>> {
        let _guard = self.admission.locked();
        ensure!(!self.cancelled.load(Ordering::Acquire), "Login cancelado.");
        let hub = self.hub.upgrade().ok_or_else(|| anyhow!("Login encerrado."))?;
        ensure!(Instant::now() < self.deadline, "Login expirado.");
        let _state = self.state.locked();
        if self.agent_mode() {
            return Ok(None);
        }
        match &self.authority {
            Authority::Local => Ok(Some(send())),
            Authority::Owner { transporte, .. } => Ok(Some(transporte.com_saida(&hub, Some(&self.authority), send)?)),
        }
    }
    pub(crate) async fn ended(&self) {
        while self.current() {
            tokio::select! {()=self.stopped.notified()=>break,()=tokio::time::sleep(Duration::from_millis(100))=>{}}
        }
    }
    pub(crate) fn claim_reader(&self) -> Result<()> {
        ensure!(
            !self.attached.swap(true, Ordering::AcqRel),
            "Terminal de login não permite replay. Inicie outro login."
        );
        Ok(())
    }
    /// Apenas apresentação ao titular, sem OSC/clipboard. Após envio de código suprime todo eco nativo.
    pub(crate) fn output(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        let mut state = self.state.locked();
        ensure!(state.output.len() + bytes.len() <= 256 * 1024, "Saída de login excedida.");
        if self.agent_mode() {
            return Ok(Vec::new());
        }
        let mut clean = Vec::new();
        for &byte in bytes {
            let mut again = true;
            while std::mem::take(&mut again) {
                // Estados: 0 normal, 1 ESC pendente, 2 em OSC, 3 em DCS/SOS/PM/APC,
                // 4 ESC em OSC, 5 ESC nas demais, 6 C2 pendente, 7 C2 em OSC, 8 C2 nas demais.
                match state.escape {
                    // ESC: OSC, DCS, SOS, PM e APC abrem uma string de controle que é descartada.
                    // Um recomeço (outro ESC, ou C2 de um C1) DESCARTA o ESC pendente: emiti-lo
                    // deixaria um ESC solto que, depois da string descartada, se juntaria ao texto
                    // seguinte (`ESC C2 9D … BEL ]52;…` viraria OSC 52). Outro byte: o ESC sai
                    // sozinho e o byte é reprocessado do estado normal.
                    1 => {
                        if matches!(byte, b']' | b'P' | b'X' | b'^' | b'_') {
                            state.escape = if byte == b']' { 2 } else { 3 };
                        } else if byte == 0xC2 {
                            state.escape = 6;
                        } else if (0x80..=0x9f).contains(&byte) {
                            // ESC e C1 cru: o ESC é descartado e o C1 é tratado do estado normal.
                            state.escape = 0;
                            again = true;
                        } else if byte != 27 {
                            clean.push(27);
                            state.escape = 0;
                            again = true;
                        }
                    }
                    // Dentro da string: só OSC termina em BEL; todas terminam em ST
                    // (ESC \ ou U+009C). DCS, SOS, PM e APC ignoram BEL.
                    2 | 3 => {
                        let tail = std::mem::take(&mut state.utf8);
                        if tail.left > 0 && (tail.lo..=tail.hi).contains(&byte) {
                            state.utf8 = Utf8Tail { left: tail.left - 1, lo: 0x80, hi: 0xBF };
                        } else {
                            match byte {
                                7 if state.escape == 2 => state.escape = 0,
                                0x9c => state.escape = 0,
                                27 => state.escape += 2,
                                0xC2 => state.escape += 5,
                                _ => state.utf8 = Utf8Tail::lead(byte).unwrap_or_default(),
                            }
                        }
                    }
                    4 | 5 => {
                        let base = state.escape - 2;
                        if byte == b'\\' {
                            state.escape = 0;
                        } else if matches!(byte, b']' | b'P' | b'X' | b'^' | b'_') {
                            state.escape = if byte == b']' { 2 } else { 3 };
                        } else {
                            state.escape = base;
                            again = matches!(byte, 7 | 27 | 0x9c | 0xC2);
                        }
                    }
                    // U+0080..U+009F chegam em UTF-8 como C2 xx: os C1 de string são descartados como o ESC.
                    // Recomeço depois de C2 (outro ESC ou outro C2) descarta o C2 pendente pelo mesmo
                    // motivo do ESC: C2 seguido de ESC ou C2 não é UTF-8 válido de texto.
                    6 => {
                        if matches!(byte, 0x90 | 0x98 | 0x9d | 0x9e | 0x9f) {
                            state.escape = if byte == 0x9d { 2 } else { 3 };
                        } else if byte == 27 {
                            state.escape = 1;
                        } else if byte == 0xC2 {
                        } else if (0x80..=0xBF).contains(&byte) {
                            // U+0080..U+00BF comuns (©, °, NBSP, outros C1 sem string): passam como antes.
                            clean.extend([0xC2, byte]);
                            state.escape = 0;
                        } else {
                            clean.push(0xC2);
                            state.escape = 0;
                            again = true;
                        }
                    }
                    7 | 8 => {
                        let base = state.escape - 5;
                        if byte == 0x9c {
                            state.escape = 0;
                        } else if matches!(byte, 0x90 | 0x98 | 0x9d | 0x9e | 0x9f) {
                            state.escape = if byte == 0x9d { 2 } else { 3 };
                        } else {
                            state.escape = base;
                            again = matches!(byte, 7 | 27 | 0xC2);
                        }
                    }
                    _ => {
                        let tail = std::mem::take(&mut state.utf8);
                        match byte {
                            27 => state.escape = 1,
                            0xC2 => state.escape = 6,
                            // Continuação válida de uma sequência UTF-8 em curso: é texto, não C1.
                            0x80..=0xBF if tail.left > 0 && (tail.lo..=tail.hi).contains(&byte) => {
                                clean.push(byte);
                                state.utf8 = Utf8Tail { left: tail.left - 1, lo: 0x80, hi: 0xBF };
                            }
                            // C1 cru (0x80–0x9F fora de UTF-8): o mesmo tratamento do C1 em UTF-8.
                            // Abrir string (DCS, SOS, OSC, PM, APC) descarta até o terminador; os demais caem sozinhos.
                            0x80..=0x9F => {
                                if matches!(byte, 0x90 | 0x98 | 0x9d | 0x9e | 0x9f) {
                                    state.escape = if byte == 0x9d { 2 } else { 3 };
                                }
                            }
                            _ => {
                                clean.push(byte);
                                state.utf8 = Utf8Tail::lead(byte).unwrap_or_default();
                            }
                        }
                    }
                }
            }
        }
        state.output.extend(&clean);
        let text = String::from_utf8_lossy(&state.output).into_owned();
        if state.google_method != GoogleMethod::Chosen
            && text.contains("Select login method")
            && text.contains("Google OAuth")
        {
            state.google_method = GoogleMethod::Offered;
        }
        let challenge = text.match_indices("https://").any(|(at, _)| {
            let suffix = &text[at..];
            suffix
                .find(|c: char| c.is_whitespace() || c == '\u{1b}')
                .is_some_and(|end| official_url(&self.backend, &suffix[..end]))
        });
        state.challenge |= challenge;
        let lower = text.to_lowercase();
        if ["agent mode", "agent ready", "how can i help", "what can i help", "ask anything"]
            .iter()
            .any(|marker| lower.contains(marker))
        {
            // O login pode ter dado certo: a entrada fecha, mas quem decide é a conferência do CLI.
            self.agent_mode.store(true, Ordering::Release);
            state.output.clear();
            return Ok(Vec::new());
        }
        if state.redaction != Redaction::None {
            let lower = text.to_lowercase();
            let rejected = state.redaction == Redaction::Submitted
                && ["invalid code", "incorrect code", "código inválido", "code expired"]
                    .iter()
                    .any(|s| lower.contains(s));
            let mut visible = Vec::new();
            if rejected {
                state.redaction = Redaction::Rejected;
                state.rejection_notice = true;
            }
            if state.redaction == Redaction::Rejected {
                if let Some(url) = text.match_indices("https://").find_map(|(at, _)| {
                    let suffix = &text[at..];
                    let end = suffix.find(|c: char| c.is_whitespace() || c == '\u{1b}')?;
                    official_url(&self.backend, &suffix[..end]).then_some(&suffix[..end])
                }) {
                    visible.extend_from_slice(url.as_bytes());
                    visible.extend_from_slice(b"\r\n");
                    state.output.clear();
                } else if rejected {
                    state.output.clear();
                }
            }
            return Ok(visible);
        }
        // Cursor e Grok concluem no navegador: depois do desafio não há código a enviar ao CLI.
        if challenge && matches!(self.backend.as_str(), "cursor" | "grok") {
            state.submitted = true;
        }
        Ok(clean)
    }
    pub(crate) fn agent_mode(&self) -> bool {
        self.agent_mode.load(Ordering::Acquire)
    }
    pub(crate) fn submitted(&self) -> bool {
        self.state.locked().submitted
    }
    pub(crate) fn take_rejection_notice(&self) -> bool {
        std::mem::take(&mut self.state.locked().rejection_notice)
    }
    pub(crate) fn progress(&self) -> bool {
        self.state.locked().submitted
    }
    /// Serializa a filtragem e o envio para que entradas concorrentes não ultrapassem o código.
    pub(crate) fn enqueue(&self, bytes: &[u8], send: impl FnOnce(Vec<u8>) -> Result<()>) -> Result<()> {
        let _guard = self.admission.locked();
        ensure!(!self.cancelled.load(Ordering::Acquire), "Login cancelado.");
        // Teto de pendentes ANTES de filtrar: recusar depois deixaria o estado do código adiantado.
        ensure!(
            self.state.locked().pending + ONE_INPUT_MAX <= PENDING_MAX,
            "Entrada de login excedida. Aguarde o terminal processar."
        );
        let filtered = self.input(bytes)?;
        if !filtered.is_empty() {
            let len = filtered.len();
            self.state.locked().pending += len;
            if let Err(error) = send(filtered) {
                self.release_pending(len);
                return Err(error);
            }
        }
        Ok(())
    }
    /// Bytes aceitos que o PTY já recebeu (ou que foram descartados): libera espaço no teto.
    pub(crate) fn release_pending(&self, bytes: usize) {
        let mut state = self.state.locked();
        state.pending = state.pending.saturating_sub(bytes);
    }
    #[cfg(test)]
    pub(crate) fn pending(&self) -> usize {
        self.state.locked().pending
    }
    /// Texto fica em RAM até Enter, sem eco no PTY. Sem Ctrl-C/Z/D, comandos, multiline ou escapes arbitrários.
    pub(crate) fn input(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        ensure!(bytes.len() <= 4096, "Entrada de login excedida.");
        let mut s = self.state.locked();
        ensure!(!s.submitted && !self.agent_mode(), "Aguarde a conferência do login.");
        if self.backend == "antigravity" && !s.challenge {
            ensure!(
                s.google_method == GoogleMethod::Offered && matches!(bytes, b"\r" | b"\n"),
                "Escolha Google OAuth com Enter no menu de login."
            );
            s.google_method = GoogleMethod::Chosen;
            return Ok(b"1\r".to_vec());
        }
        if matches!(bytes, b"\x1b[A" | b"\x1b[B" | b"\x1b[C" | b"\x1b[D") {
            ensure!(self.backend != "antigravity", "Cole somente o código.");
            return Ok(bytes.to_vec());
        }
        ensure!(!(s.code.is_empty() && bytes.first() == Some(&b'/')), "Comandos não são aceitos no login.");
        ensure!(!bytes.contains(&27), "Tecla indisponível no login.");
        ensure!(
            bytes.iter().all(|b| b.is_ascii_alphanumeric() || b"-_.~/#\r\n".contains(b) || *b == 127 || *b == 8),
            "Cole somente um código, sem comandos ou controles."
        );
        ensure!(
            bytes.iter().enumerate().all(|(i, b)| !matches!(*b, b'\r' | b'\n') || i + 1 == bytes.len()),
            "Cole somente um código."
        );
        let mut send = Vec::new();
        for (index, &byte) in bytes.iter().enumerate() {
            if byte == b'\r' || byte == b'\n' {
                ensure!(index + 1 == bytes.len(), "Cole somente um código.");
                if s.code.is_empty() {
                    send.push(b'\r');
                    if s.challenge {
                        s.submitted = true;
                        s.redaction = Redaction::Submitted;
                        s.output.clear();
                    }
                } else {
                    ensure!(s.challenge, "Aguarde um desafio de login válido.");
                    if !s.code.first().is_some_and(u8::is_ascii_alphanumeric) {
                        s.code.clear();
                        anyhow::bail!("Comandos não são aceitos no login.");
                    }
                    send.append(&mut s.code);
                    send.push(b'\r');
                    s.submitted = true;
                    s.redaction = Redaction::Submitted;
                    s.output.clear();
                }
            } else if byte == 127 || byte == 8 {
                s.code.pop();
            } else {
                ensure!(
                    s.challenge && (byte.is_ascii_alphanumeric() || b"-_.~/#".contains(&byte)) && s.code.len() < 2048,
                    "Cole somente o código do desafio."
                );
                s.code.push(byte);
            }
        }
        Ok(send)
    }
    pub(crate) async fn check(&self) -> Option<bool> {
        if !self.current() {
            return None;
        }
        let result = tokio::select! {
            result=check(&self.backend,&self.executable,&self.home,&self.env)=>result,
            ()=self.stopped.notified()=>None,
            ()=async{while self.current(){tokio::time::sleep(Duration::from_millis(250)).await;}}=>None,
        };
        self.verified.store(result == Some(true), Ordering::Release);
        result
    }
    pub(crate) fn complete(&self) -> Result<()> {
        let _guard = self.admission.locked();
        let _profiles = PROFILE_MUTATION.locked();
        ensure!(!self.cancelled.load(Ordering::Acquire) && Instant::now() < self.deadline, "Login encerrado.");
        ensure!(self.verified.load(Ordering::Acquire), "Login não confirmado.");
        ensure!(!self.promoted.load(Ordering::Acquire), "Login já promovido.");
        let hub = self.hub.upgrade().ok_or_else(|| anyhow!("Login encerrado."))?;
        validate_profile(&hub.store, &self.home)?;
        let old = hub
            .store
            .kv_read(&slot(&self.backend, &self.space))?
            .filter(|old| old.as_str() != self.home.to_str().unwrap_or_default());
        // A pasta nova só fica visível aos bots no commit com o vínculo ORIGINAL vigente.
        self.authority.commit_and_admit(
            &hub,
            |db| {
                db.execute("DELETE FROM kv WHERE k=?1", [attempt_key(&self.home)?])?;
                // A troca e o registro do perfil substituído são atômicos: uma queda não o esquece.
                if let Some(old) = &old
                    && let Ok(key) = replaced_key(Path::new(old))
                {
                    db.execute(
                        "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=?2",
                        rusqlite::params![key, old],
                    )?;
                }
                db.execute(
                    "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=?2",
                    rusqlite::params![slot(&self.backend, &self.space), self.home.to_string_lossy()],
                )?;
                db.execute(
                    "INSERT INTO kv(k,v) VALUES(?1,'1') ON CONFLICT(k) DO UPDATE SET v=CAST(v AS INTEGER)+1",
                    [format!("{}:generation", slot(&self.backend, &self.space))],
                )?;
                Ok(())
            },
            |()| self.promoted.store(true, Ordering::Release),
        )?;
        // Troca confirmada: a credencial da conta anterior não fica no disco. Falha só avisa.
        if let Some(old) = old {
            reap_replaced(&hub, Path::new(&old));
        }
        if self.backend != "antigravity" {
            conclusao::finish(&hub, &self.backend, &self.space, &self.authority)?;
        }
        Ok(())
    }
    /// Chamar somente depois de encerrar o processo e seus descendentes.
    pub(crate) fn discard(&self) -> Result<()> {
        let _guard = self.admission.locked();
        let _profiles = PROFILE_MUTATION.locked();
        if self.promoted.load(Ordering::Acquire) {
            return Ok(());
        }
        if !super::motor_exec::configured()
            && matches!(std::fs::symlink_metadata(&self.home), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
        {
            return Ok(());
        }
        let hub = self.hub.upgrade().ok_or_else(|| anyhow!("Hub encerrado."))?;
        // Uma promoção já persistida prevalece mesmo se a flag em memória não foi atualizada.
        if hub.store.kv_read(&slot(&self.backend, &self.space))?.as_deref() == self.home.to_str() {
            return Ok(());
        }
        let key = attempt_key(&self.home)?;
        let saved = hub.store.kv_read(&key)?.ok_or_else(|| anyhow!("Operação provisória não registrada."))?;
        let deadline = saved.strip_prefix("c:").unwrap_or(&saved);
        if !saved.starts_with("c:") {
            cancel_attempt(&self.home, deadline)?;
            hub.store.kv_set(&key, &format!("c:{deadline}"))?;
        }
        remove_attempt(&self.home, deadline, true)?;
        hub.store.kv_delete(&key)?;
        Ok(())
    }
}
impl Drop for Login {
    fn drop(&mut self) {
        if self.discard().is_err() {
            tracing::warn!(backend = self.backend, "Não foi possível apagar perfil provisório de login.");
        }
    }
}
pub(crate) fn official_url(backend: &str, raw: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(raw) else { return false };
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return false;
    }
    let hosts: &[&str] = match backend {
        "antigravity" => &["accounts.google.com"],
        "cursor" => &["cursor.com"],
        "grok" => &["auth.x.ai", "accounts.x.ai"],
        _ => &[],
    };
    hosts.contains(&url.host_str().unwrap_or_default())
        && !url.query_pairs().any(|(key, _)| matches!(key.as_ref(), "access_token" | "refresh_token" | "id_token"))
}
fn environment(home: &Path) -> Vec<(String, String)> {
    let mut env = Vec::new();
    if let Ok(value) = std::env::var("PATH") {
        env.push(("PATH".into(), value));
    }
    for key in ["EDITOR", "VISUAL", "PAGER"] {
        env.push((key.into(), "/usr/bin/false".into()));
    }
    for (key, path) in [
        ("HOME", home.to_path_buf()),
        ("USERPROFILE", home.to_path_buf()),
        ("XDG_CONFIG_HOME", home.join(".config")),
        ("XDG_DATA_HOME", home.join(".local/share")),
        ("XDG_CACHE_HOME", home.join(".cache")),
    ] {
        env.push((key.into(), path.to_string_lossy().into_owned()));
    }
    env
}
fn temporary_profile(hub: &Hub, backend: &str, space: &str) -> Result<PathBuf> {
    let id = format!(
        "ac_{}",
        super::bootstrap::codec::hash_bytes(format!("terminal-login/v1:{space}:{backend}").as_bytes())
    );
    let _profiles = PROFILE_MUTATION.locked();
    cleanup_expired(hub, &id)?;
    let deadline = (crate::store::now_ms() + i64::try_from(TTL.as_millis())?).to_string();
    let operation = uuid::Uuid::new_v4().to_string();
    let base = if super::motor_exec::configured() {
        super::motor_roots::accounts()
    } else {
        hub.store.engine_setup_root.join("accounts")
    };
    let expected = base.join(&id).join(&operation);
    // O registro precede a criação: uma queda nunca deixa perfil marcado sem registro.
    hub.store.kv_set(&attempt_key(&expected)?, &deadline)?;
    let home = if super::motor_exec::configured() {
        let raw = super::motor_roots::profile("login", &[id.as_ref(), deadline.as_ref(), operation.as_ref()])?;
        let home = PathBuf::from(String::from_utf8(raw)?.trim());
        ensure!(home == expected, "Perfil criado fora da operação.");
        let count = super::motor_roots::profile("avisar-login", &[id.as_ref()])
            .ok()
            .and_then(|out| String::from_utf8(out).ok()?.trim().parse::<usize>().ok());
        warn_unmarked(count);
        home
    } else {
        let root = hub.store.engine_setup_root.clone();
        private_dir(&root)?;
        let accounts = root.join("accounts");
        private_dir(&accounts)?;
        let account = accounts.join(id);
        private_dir(&account)?;
        #[cfg(unix)]
        warn_unmarked(super::motor_exec::login_cleanup::count_unmarked(&account).ok());
        let home = account.join(&operation);
        private_dir(&home)?;
        std::fs::write(home.join(".donnabot-profile"), [])?;
        let operation = home.file_name().ok_or_else(|| anyhow!("Operação de login inválida."))?.to_string_lossy();
        #[cfg(unix)]
        super::private_fs::create_atomic(
            &home.join(".donnabot-login-operation"),
            format!("{operation} {deadline}\n").as_bytes(),
        )?;
        #[cfg(not(unix))]
        anyhow::bail!("Terminal seguro indisponível.");
        home
    };
    Ok(home)
}
/// O aviso nunca derruba o login: contagem ilegível (por exemplo, ancestral que é link) só é registrada.
fn warn_unmarked(count: Option<usize>) {
    match count {
        Some(0) => {}
        Some(_) => tracing::warn!("Perfil anterior sem marcador foi preservado."),
        None => tracing::warn!("Não foi possível conferir perfis anteriores sem marcador."),
    }
}
pub(crate) fn attempt_key(path: &Path) -> Result<String> {
    let account =
        path.parent().and_then(Path::file_name).and_then(|s| s.to_str()).ok_or_else(|| anyhow!("Conta inválida."))?;
    let operation = path.file_name().and_then(|s| s.to_str()).ok_or_else(|| anyhow!("Operação inválida."))?;
    ensure!(
        account.len() == 67 && account.starts_with("ac_") && account[3..].bytes().all(|b| b.is_ascii_hexdigit()),
        "Conta inválida."
    );
    ensure!(uuid::Uuid::parse_str(operation).is_ok(), "Operação inválida.");
    Ok(format!("terminal-login-attempt:{account}:{operation}"))
}
fn replaced_key(path: &Path) -> Result<String> {
    Ok(attempt_key(path)?.replacen("terminal-login-attempt:", "terminal-login-replaced:", 1))
}
/// Apaga o perfil promovido que foi substituído, sem seguir links; se falhar, o registro fica para a varredura.
fn reap_replaced(hub: &Hub, path: &Path) {
    let Ok(key) = replaced_key(path) else { return };
    let removed = if super::motor_exec::configured() {
        super::motor_roots::profile("substituir-login", &[path.as_os_str()]).map(|_| ())
    } else {
        #[cfg(unix)]
        {
            if path.starts_with(hub.store.engine_setup_root.join("accounts")) {
                super::motor_exec::login_cleanup::remove_replaced(path)
            } else {
                Err(anyhow!("Perfil fora da raiz."))
            }
        }
        #[cfg(not(unix))]
        {
            Err(anyhow!("Terminal seguro indisponível."))
        }
    };
    // Já não existir também é sucesso: a pasta foi apagada por outro caminho.
    let gone = matches!(std::fs::symlink_metadata(path), Err(e) if e.kind() == std::io::ErrorKind::NotFound);
    if removed.is_ok() || gone {
        let _ = hub.store.kv_delete(&key);
    } else {
        tracing::warn!("Perfil substituído não pôde ser removido com segurança; nova tentativa na próxima varredura.");
    }
}
fn cancel_attempt(path: &Path, deadline: &str) -> Result<()> {
    if super::motor_exec::configured() {
        super::motor_roots::profile("cancelar-login", &[path.as_os_str(), deadline.as_ref()])?;
        Ok(())
    } else {
        #[cfg(unix)]
        {
            super::motor_exec::login_cleanup::cancel(path, deadline)
        }
        #[cfg(not(unix))]
        {
            anyhow::bail!("Terminal seguro indisponível.")
        }
    }
}
fn remove_attempt(path: &Path, deadline: &str, cancelled: bool) -> Result<()> {
    if super::motor_exec::configured() {
        let state = if cancelled { "cancelado" } else { "vencido" };
        super::motor_roots::profile("limpar-login", &[path.as_os_str(), deadline.as_ref(), state.as_ref()])?;
        Ok(())
    } else {
        #[cfg(unix)]
        {
            super::motor_exec::login_cleanup::remove(path, deadline, cancelled)
        }
        #[cfg(not(unix))]
        {
            anyhow::bail!("Terminal seguro indisponível.")
        }
    }
}
fn cleanup_expired(hub: &Hub, account: &str) -> Result<()> {
    let prefix = format!("terminal-login-attempt:{account}:");
    let base = if super::motor_exec::configured() {
        super::motor_roots::accounts()
    } else {
        hub.store.engine_setup_root.join("accounts")
    };
    for key in hub.store.kv_prefix(&format!("terminal-login-replaced:{account}:")) {
        if let Some(operation) = key.rsplit(':').next()
            && uuid::Uuid::parse_str(operation).is_ok()
        {
            reap_replaced(hub, &base.join(account).join(operation));
        }
    }
    for key in hub.store.kv_prefix(&prefix) {
        let operation = key.strip_prefix(&prefix).ok_or_else(|| anyhow!("Operação inválida."))?;
        ensure!(uuid::Uuid::parse_str(operation).is_ok(), "Operação inválida.");
        let saved = hub.store.kv_read(&key)?.ok_or_else(|| anyhow!("Operação ausente."))?;
        let cancelled = saved.starts_with("c:");
        let deadline = saved.strip_prefix("c:").unwrap_or(&saved);
        let expires: i64 = deadline.parse()?;
        if !cancelled && expires > crate::store::now_ms() {
            continue;
        }
        let path = base.join(account).join(operation);
        ensure!(attempt_key(&path)? == key, "Operação inválida.");
        // A promoção retira o registro na mesma transação que torna o perfil visível.
        if remove_attempt(&path, deadline, cancelled).is_ok() {
            hub.store.kv_delete(&key)?;
        } else {
            tracing::warn!("Perfil provisório vencido não pôde ser removido com segurança.");
        }
    }
    Ok(())
}
fn private_dir(path: &Path) -> Result<()> {
    let created = match std::fs::create_dir(path) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(e) => return Err(e.into()),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if created {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
        }
        let meta = std::fs::symlink_metadata(path)?;
        // SAFETY: consulta o UID efetivo; não altera identidade nem processo.
        let uid = unsafe { libc::geteuid() };
        ensure!(
            meta.is_dir()
                && !meta.file_type().is_symlink()
                && meta.uid() == uid
                && meta.permissions().mode().trailing_zeros() >= 6,
            "Pasta de login insegura."
        );
    }
    #[cfg(not(unix))]
    {
        let _ = created;
        let meta = std::fs::symlink_metadata(path)?;
        ensure!(meta.is_dir() && !meta.file_type().is_symlink(), "Pasta de login insegura.");
    }
    Ok(())
}
fn validate_profile(store: &Store, path: &Path) -> Result<PathBuf> {
    if super::motor_exec::configured() {
        super::motor_roots::profile("validar", &[path.as_os_str()])?;
    } else {
        ensure!(path.starts_with(store.engine_setup_root.join("accounts")), "Perfil de login inválido.");
        let relative = path.strip_prefix(&store.engine_setup_root)?;
        let parts = relative.components().collect::<Vec<_>>();
        ensure!(
            parts.len() == 3 && parts.iter().all(|c| matches!(c, std::path::Component::Normal(_))),
            "Perfil de login inválido."
        );
        let mut root = store.engine_setup_root.clone();
        private_dir(&root)?;
        for part in parts {
            root.push(part);
            private_dir(&root)?;
        }
        ensure!(path.join(".donnabot-profile").is_file(), "Perfil de login inválido.");
    }
    Ok(path.to_path_buf())
}
pub(crate) async fn status(hub: &Hub, backend: &str, space: &str) -> Result<Option<Value>> {
    let Some(h) = canonical(backend) else { return Ok(None) };
    if fixed_args(h.id).is_err() {
        return Ok(None);
    }
    hub.store.require_space(space)?;
    let home =
        hub.store.kv_read(&slot(h.id, space))?.map(|s| validate_profile(&hub.store, Path::new(&s))).transpose()?;
    if home.is_none() {
        return Ok(None);
    }
    let signed = if let Some(home) = &home {
        if let Some(bin) = h.bins.iter().find_map(|b| backends::which(b)) {
            check(h.id, &bin, home, &environment(home)).await
        } else {
            None
        }
    } else {
        None
    };
    let mut methods = Vec::new();
    super::conexao_motores::add_methods(h.id, &mut methods);
    let saved = crate::agent::auth::env(&hub.store, h.id)?.into_iter().map(|(k, _)| k).collect::<Vec<_>>();
    Ok(Some(
        json!({"signedIn":if h.id=="antigravity"{None}else{signed},"cliSignedIn":if h.id=="antigravity"{signed}else{None},"detail":if h.id=="antigravity"&&signed==Some(true){Some("Login do agy confirmado. O motor ACP usa outra credencial; integração pendente.")}else{None},"methods":methods,"savedEnv":saved,"login":fixed_args(h.id).is_ok()&&backends::login_available(h.id)}),
    ))
}
pub(crate) fn stamp(store: &Store, backend: &str, space: &str) -> Result<String> {
    let backend = canonical(backend).map_or(backend, |h| h.id);
    if matches!(backend, "claude" | "codex" | "antigravity") {
        return Ok(String::new());
    }
    let Some(path) = store.kv_read(&slot(backend, space))? else {
        return Ok(String::new());
    };
    Ok(format!("{}:{}", path, store.kv_read(&format!("{}:generation", slot(backend, space)))?.unwrap_or_default()))
}

pub(crate) fn runtime_env(store: &Store, backend: &str, space: &str) -> Result<Vec<(String, String)>> {
    let backend = canonical(backend).map_or(backend, |h| h.id);
    if matches!(backend, "claude" | "codex" | "antigravity") {
        return Ok(Vec::new());
    }
    let Some(path) = store.kv_read(&slot(backend, space))? else {
        return Ok(Vec::new());
    };
    Ok(environment(&validate_profile(store, Path::new(&path))?))
}
pub(crate) async fn check(backend: &str, bin: &Path, home: &Path, env: &[(String, String)]) -> Option<bool> {
    let h = canonical(backend)?;
    let spec =
        if backend == "antigravity" { backends::SignInCheck { args: "models", ok: agy_models } } else { h.signed_in? };
    let env: Vec<_> = env.iter().map(|(k, v)| (k.into(), v.into())).collect();
    backends::run_sign_in_check(spec, bin, Duration::from_secs(5), None, &env, Some(home)).await
}
fn agy_models(success: bool, out: &str) -> bool {
    success
        && out.lines().any(|line| {
            line.split_once('\t').is_some_and(|(id, label)| {
                !id.is_empty()
                    && id.bytes().all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
                    && !label.trim().is_empty()
            })
        })
}
#[cfg(test)]
mod p3_filter_tests;
#[cfg(test)]
mod tests;
