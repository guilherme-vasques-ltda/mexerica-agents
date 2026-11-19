//! Supervisor interno ainda não conectado a ferramentas ou navegação remota.
#![allow(dead_code)]

use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use std::{
    collections::HashMap,
    fs,
    future::Future,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[cfg(target_os = "linux")]
mod real;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct Scope {
    pub hub: String,
    pub space: String,
    pub bot: String,
}

/// O processo retornado pertence ao supervisor e deve ser encerrado por `stop`.
pub(super) trait Launcher {
    type Process;
    fn start(&mut self, profile: &Path, proxy_socket: &Path) -> Result<Self::Process>;
    /// Entrega a credencial só pelo canal de controle, nunca por argv ou ambiente.
    fn authenticate(&mut self, process: &mut Self::Process, token: String) -> impl Future<Output = Result<()>>;
    fn stop(&mut self, process: &mut Self::Process) -> Result<()>;
}

struct Session<P> {
    process: P,
    _proxy: super::net_proxy::Proxy,
    profile: PathBuf,
    touched: Instant,
}

pub(super) struct Supervisor<L: Launcher> {
    launcher: L,
    root: PathBuf,
    // Trava exclusiva impede que outra instância remova perfis ativos.
    _lock: fs::File,
    limit: usize,
    idle: Duration,
    sessions: HashMap<Scope, Session<L::Process>>,
    /// Somente testes Linux: destino local exato liberado no filtro.
    #[cfg(all(test, target_os = "linux"))]
    test_allowed: std::collections::HashSet<std::net::SocketAddr>,
}

impl<L: Launcher> Supervisor<L> {
    pub fn new(launcher: L, root: PathBuf, limit: usize, idle: Duration) -> Result<Self> {
        ensure!(limit > 0 && !idle.is_zero(), "limite e ociosidade devem ser positivos");
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700).recursive(true).create(&root)?;
            let metadata = fs::symlink_metadata(&root)?;
            // SAFETY: geteuid não altera memória nem recebe ponteiros.
            let uid = unsafe { libc::geteuid() };
            ensure!(
                metadata.is_dir()
                    && !metadata.file_type().is_symlink()
                    && metadata.permissions().mode() & 0o777 == 0o700
                    && metadata.uid() == uid,
                "diretório do supervisor inseguro"
            );
        }
        #[cfg(not(unix))]
        anyhow::bail!("supervisor de navegador indisponível nesta plataforma");
        let lock =
            fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(root.join("lock"))?;
        lock.try_lock_exclusive().context("supervisor de navegador já está em execução")?;
        // Só diretórios de sessão criados aqui são removidos após queda.
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().starts_with("session-") {
                let ty = entry.file_type()?;
                if ty.is_dir() {
                    fs::remove_dir_all(entry.path())?;
                } else {
                    fs::remove_file(entry.path())?;
                }
            }
        }
        Ok(Self {
            launcher,
            root,
            _lock: lock,
            limit,
            idle,
            sessions: HashMap::new(),
            #[cfg(all(test, target_os = "linux"))]
            test_allowed: std::collections::HashSet::new(),
        })
    }

    #[cfg(all(test, target_os = "linux"))]
    pub fn allow_test_destination(&mut self, address: std::net::SocketAddr) {
        self.test_allowed.insert(address);
    }

    pub async fn start(&mut self, scope: Scope, now: Instant) -> Result<()> {
        self.reap_idle(now)?;
        ensure!(!self.sessions.contains_key(&scope), "já existe sessão para este bot");
        ensure!(self.sessions.len() < self.limit, "limite de sessões atingido");
        let profile = self.root.join(format!("session-{}", uuid::Uuid::new_v4()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new().mode(0o700).create(&profile)?;
        }
        let socket = profile.join("proxy.sock");
        #[cfg(all(test, target_os = "linux"))]
        let proxy_result = if self.test_allowed.is_empty() {
            super::net_proxy::Proxy::start_unix(&socket).await
        } else {
            super::net_proxy::Proxy::start_unix_test(&socket, self.test_allowed.clone()).await
        };
        #[cfg(all(not(test), target_os = "linux"))]
        let proxy_result = super::net_proxy::Proxy::start_unix(&socket).await;
        #[cfg(not(target_os = "linux"))]
        let proxy_result = super::net_proxy::Proxy::start().await;
        let proxy = match proxy_result {
            Ok(proxy) => proxy,
            Err(error) => {
                fs::remove_dir_all(&profile)?;
                return Err(error.into());
            }
        };
        match self.launcher.start(&profile, &socket) {
            Ok(mut process) => {
                let token = proxy.token().to_owned();
                if let Err(error) = self.launcher.authenticate(&mut process, token).await {
                    let _ = self.launcher.stop(&mut process);
                    fs::remove_dir_all(&profile).context("falha ao limpar perfil após erro de autenticação")?;
                    return Err(error);
                }
                self.sessions.insert(scope, Session { process, _proxy: proxy, profile, touched: now });
                Ok(())
            }
            Err(error) => {
                fs::remove_dir_all(&profile).context("falha ao limpar perfil após erro de início")?;
                Err(error)
            }
        }
    }

    pub fn touch(&mut self, scope: &Scope, now: Instant) -> Result<()> {
        let session = self.sessions.get_mut(scope).context("sessão de navegador ausente")?;
        session.touched = now;
        Ok(())
    }

    pub fn stop(&mut self, scope: &Scope) -> Result<()> {
        let session = self.sessions.get_mut(scope).context("sessão de navegador ausente")?;
        self.launcher.stop(&mut session.process)?;
        fs::remove_dir_all(&session.profile).context("falha ao limpar perfil do navegador")?;
        self.sessions.remove(scope);
        Ok(())
    }

    pub fn reap_idle(&mut self, now: Instant) -> Result<()> {
        let expired: Vec<_> = self
            .sessions
            .iter()
            .filter(|(_, session)| now.saturating_duration_since(session.touched) >= self.idle)
            .map(|(scope, _)| scope.clone())
            .collect();
        let mut errors = Vec::new();
        for scope in expired {
            if let Err(error) = self.stop(&scope) {
                errors.push(format!("{error:#}"));
            }
        }
        ensure!(errors.is_empty(), "falhas ao encerrar sessões ociosas: {}", errors.join("; "));
        Ok(())
    }
}

impl<L: Launcher> Drop for Supervisor<L> {
    fn drop(&mut self) {
        let scopes: Vec<_> = self.sessions.keys().cloned().collect();
        for scope in scopes {
            if let Err(error) = self.stop(&scope) {
                tracing::error!(%error, "falha ao encerrar navegador");
            }
        }
    }
}

#[cfg(test)]
mod tests;
