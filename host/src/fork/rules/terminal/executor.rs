use super::Decisao;
use crate::{
    LockExt,
    fork::sandbox::{
        contrato::{Limites, Saida},
        processo::{Graca, Lancamento, rodar_guardado},
    },
};
use anyhow::{Context as _, Result, ensure};
use std::{
    ffi::CString,
    fs::{File, OpenOptions},
    os::{fd::AsRawFd as _, unix::fs::OpenOptionsExt as _},
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};
use tokio::{process::Command, time::Instant};
use uuid::Uuid;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "linux")]
const OPEN_FLAGS: libc::c_int = libc::O_PATH | libc::O_NOFOLLOW;
#[cfg(target_os = "macos")]
const OPEN_FLAGS: libc::c_int = libc::O_NONBLOCK | libc::O_NOFOLLOW;
const APPROVAL_TTL: Duration = Duration::from_secs(120);
const LIMITS: Limites =
    Limites { tempo: Duration::from_secs(120), saida_bytes: 40 * 1024, carencia: Duration::from_millis(100) };

#[derive(Clone, Copy)]
pub(crate) enum Consent {
    Disabled,
    OwnerAcceptedExecutionWithoutIsolation,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Request {
    id: Uuid,
    bot: String,
    turn: i64,
    argv: Vec<String>,
    cwd: PathBuf,
    expires_at: Instant,
}

enum State {
    Idle,
    Pending(Request),
    Approved(Request),
    Revoked,
}

/// A autoridade do Hub deve revogar esta cerca em toda perda de autoridade, consentimento
/// ou mudança de política do turno. A revogação e a criação do filho usam a mesma trava.
pub(crate) struct Gate {
    bot: String,
    turn: i64,
    state: Mutex<State>,
}

impl Gate {
    pub(crate) fn opaque(bot: String, turn: i64, decision: Decisao, consent: Consent) -> Result<Self> {
        ensure!(matches!(consent, Consent::OwnerAcceptedExecutionWithoutIsolation), "comandos locais desligados");
        ensure!(!bot.is_empty() && turn > 0 && decision == Decisao::Perguntar, "comando sem autorização");
        Ok(Self { bot, turn, state: Mutex::new(State::Idle) })
    }

    pub(crate) fn prepare(&self, argv: Vec<String>, cwd: PathBuf) -> Result<Prepared<'_>> {
        ensure!(
            !argv.is_empty()
                && argv.len() <= 64
                && argv.iter().all(|part| !part.chars().any(char::is_control))
                && argv.iter().map(String::len).sum::<usize>() <= 4096,
            "argumentos de comando inválidos"
        );
        let program = Path::new(&argv[0]);
        ensure!(program.is_absolute() && cwd.is_absolute(), "executável e pasta devem ter caminhos absolutos");
        let executable =
            OpenOptions::new().read(true).custom_flags(OPEN_FLAGS).open(program).context("abrir executável")?;
        ensure!(
            executable.metadata().context("ler metadados do executável")?.is_file(),
            "executável não é arquivo regular"
        );
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(OPEN_FLAGS | libc::O_DIRECTORY)
            .open(&cwd)
            .context("abrir pasta de trabalho")?;
        let mut state = self.state.locked();
        ensure!(
            match &*state {
                State::Idle => true,
                State::Pending(request) | State::Approved(request) => Instant::now() >= request.expires_at,
                State::Revoked => false,
            },
            "turno revogado ou com pedido de comando em aberto"
        );
        let request = Request {
            id: Uuid::new_v4(),
            bot: self.bot.clone(),
            turn: self.turn,
            argv,
            cwd,
            expires_at: Instant::now() + APPROVAL_TTL,
        };
        *state = State::Pending(request.clone());
        Ok(Prepared { gate: self, request, executable, directory })
    }

    /// Só recebe a resposta autenticada do dono à apresentação deste pedido exato.
    pub(crate) fn approve(&self, response: &Request) -> Result<()> {
        let mut state = self.state.locked();
        ensure!(
            matches!(&*state, State::Pending(request) if request == response && Instant::now() < request.expires_at),
            "pedido de aprovação inválido"
        );
        *state = State::Approved(response.clone());
        Ok(())
    }

    pub(crate) fn deny(&self, response: &Request) {
        let mut state = self.state.locked();
        if matches!(&*state, State::Pending(r) if r == response) {
            *state = State::Idle;
        }
    }

    pub(crate) fn revoke(&self) {
        *self.state.locked() = State::Revoked;
    }

    fn start<T>(&self, request: &Request, spawn: impl FnOnce() -> Result<T>) -> Result<T> {
        let mut state = self.state.locked();
        ensure!(
            matches!(&*state, State::Approved(approved) if approved == request && Instant::now() < approved.expires_at),
            "comando não aprovado ou aprovação expirada"
        );
        *state = State::Idle;
        spawn()
    }
}

pub(crate) struct Prepared<'a> {
    gate: &'a Gate,
    request: Request,
    executable: File,
    directory: File,
}

impl Prepared<'_> {
    pub(crate) fn request(&self) -> &Request {
        &self.request
    }

    pub(crate) async fn execute(self) -> Result<Saida> {
        let strings = self.request.argv.iter().map(|arg| CString::new(arg.as_str())).collect::<Result<Vec<_>, _>>()?;
        #[cfg(target_os = "macos")]
        let identity = macos::Identity::capture(&self)?;
        #[cfg(target_os = "macos")]
        let child_identity = identity.clone();
        #[cfg(target_os = "linux")]
        let executable = self.executable.as_raw_fd();
        let directory = self.directory.as_raw_fd();
        let mut command = Command::new("/dev/null");
        command.env_clear();
        // SAFETY: os descritores ficam vivos até o spawn. No filho, só se usa a pilha,
        // memória já alocada e chamadas de sistema sem locks nem alocações.
        unsafe {
            command.pre_exec(move || {
                #[cfg(target_os = "linux")]
                {
                    let one: libc::c_ulong = 1;
                    let zero: libc::c_ulong = 0;
                    if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, one, zero, zero, zero) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                if libc::fchdir(directory) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                let mut argv = [std::ptr::null(); 65];
                for (slot, arg) in argv.iter_mut().zip(&strings) {
                    *slot = arg.as_ptr();
                }
                let env = [c"PATH=/usr/bin:/bin".as_ptr(), std::ptr::null()];
                #[cfg(target_os = "linux")]
                {
                    libc::fexecve(executable, argv.as_ptr(), env.as_ptr());
                    let error = std::io::Error::last_os_error();
                    // No Linux, fexecve devolve ENOENT se um script precisar passar
                    // seu descritor ao interpretador e ele estiver marcado CLOEXEC.
                    if error.raw_os_error() != Some(libc::ENOENT) {
                        return Err(error);
                    }
                    if libc::fcntl(executable, libc::F_SETFD, 0) == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    libc::fexecve(executable, argv.as_ptr(), env.as_ptr());
                }
                #[cfg(target_os = "macos")]
                {
                    child_identity.recheck()?;
                    libc::execve(argv[0], argv.as_ptr(), env.as_ptr());
                }
                Err(std::io::Error::last_os_error())
            });
        }
        rodar_guardado(Lancamento { command, manter: Vec::new(), graca: Graca::Grupo }, LIMITS, |command| {
            self.gate.start(&self.request, || {
                #[cfg(target_os = "macos")]
                identity.recheck()?;
                command.spawn().context("não foi possível iniciar o comando")
            })
        })
        .await
    }
}

impl Drop for Prepared<'_> {
    fn drop(&mut self) {
        let mut state = self.gate.state.locked();
        if matches!(&*state, State::Pending(r) | State::Approved(r) if r == &self.request) {
            *state = State::Idle;
        }
    }
}

#[cfg(test)]
pub(in crate::fork) mod tests;
