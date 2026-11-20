//! O grupo vive tanto quanto a operação: timeout/cancelamento também o encerra como motor.
use std::{
    io,
    ops::{Deref, DerefMut},
    path::PathBuf,
    process::{Output, Stdio},
    time::Duration,
};
use tokio::process::Command;

pub(crate) struct Group {
    pid: u32,
    helper: PathBuf,
}

impl Group {
    pub(crate) fn pty(pid: Option<u32>) -> Option<Self> {
        if super::configured() { pid.map(|pid| Self { pid, helper: super::helper_path() }) } else { None }
    }

    fn kill(&self) -> io::Result<()> {
        let status = std::process::Command::new(&self.helper)
            .args(["kill-group", &self.pid.to_string()])
            .env_clear()
            .status()?;
        if status.success() { Ok(()) } else { Err(io::Error::other("ajudante não encerrou o grupo do motor")) }
    }

    /// Também usado antes de esperar o PTY; erro nunca vira sinalização como Hub.
    pub(crate) fn terminate(&self) {
        if let Err(error) = self.kill() {
            tracing::warn!(%error, pid = self.pid, "falha ao encerrar motor pelo ajudante");
        }
    }
}
impl Drop for Group {
    fn drop(&mut self) {
        self.terminate();
    }
}

pub(crate) struct Child {
    // Primeiro: encerrar como motor antes de descartar o handle do filho.
    group: Option<Group>,
    inner: tokio::process::Child,
}
impl Deref for Child {
    type Target = tokio::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl DerefMut for Child {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
impl Child {
    pub(crate) async fn kill(&mut self) -> io::Result<()> {
        if let Some(group) = &self.group {
            group.kill()?;
            tokio::time::timeout(Duration::from_secs(5), self.inner.wait())
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "motor não saiu após kill pelo ajudante"))??;
            Ok(())
        } else {
            match self.inner.kill().await {
                Err(error) if error.kind() == io::ErrorKind::InvalidInput => Ok(()),
                result => result,
            }
        }
    }

    /// Para quem descarta o filho sem poder esperar (`Drop`): encerra o grupo agora
    /// e entrega o `wait` a uma tarefa, para o processo não ficar zumbi.
    pub(crate) fn terminate_and_reap(self) {
        let Self { group, mut inner } = self;
        if group.is_none()
            && let Err(error) = inner.start_kill()
        {
            tracing::debug!(%error, "motor já encerrado");
        }
        // O `Drop` do `Group` já chama `terminate` — sem kill duplo no ajudante.
        drop(group);
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn(async move {
                    let _ = tokio::time::timeout(Duration::from_secs(5), inner.wait()).await;
                });
            }
            // Sem runtime não há como esperar; o `tokio` colhe órfãos no próximo SIGCHLD.
            Err(_) => drop(inner),
        }
    }

    pub(crate) async fn wait_with_output(self) -> io::Result<Output> {
        let Self { group, inner } = self;
        let output = inner.wait_with_output().await;
        drop(group);
        output
    }
}

pub(crate) fn spawn(command: &mut Command) -> io::Result<Child> {
    crate::open_hub::require_executor()?;
    let helper = super::helper_path();
    let separate = command.as_std().get_program() == helper.as_os_str();
    if separate {
        // Nunca depender de kill_on_drop do Hub para sinalizar outro UID.
        command.kill_on_drop(false);
        #[cfg(unix)]
        command.process_group(0);
    }
    let inner = command.spawn()?;
    let group = if separate { inner.id().map(|pid| Group { pid, helper }) } else { None };
    Ok(Child { group, inner })
}

#[cfg(unix)]
pub(crate) fn start_open_codex(launch: crate::fork::open_codex::AppServer) -> io::Result<Child> {
    Ok(Child { group: None, inner: launch.spawn()? })
}

pub(crate) async fn output(command: &mut Command) -> io::Result<Output> {
    command.stdin(Stdio::null()).stdout(Stdio::piped());
    spawn(command)?.wait_with_output().await
}
