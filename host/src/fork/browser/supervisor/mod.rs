//! Supervisor interno ainda não conectado a ferramentas ou navegação remota.
#![allow(dead_code)]

use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use std::{
    collections::HashMap,
    fs,
    future::Future,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[cfg(target_os = "linux")]
pub(super) mod real;

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
    /// Falso quando o processo já caiu ou perdeu o canal de controle: a sessão não serve mais.
    fn alive(&self, _process: &Self::Process) -> bool {
        true
    }
    /// Corte do controle do processo sem trava e sem esperar o supervisor: depois dele nenhum comando sai.
    /// O encerramento dos processos vem depois, por `stop`.
    fn cutter(&self, _process: &Self::Process) -> Arc<dyn Fn() + Send + Sync> {
        Arc::new(|| {})
    }
}

/// Corte de uma instância que não depende da trava do supervisor: derruba o proxy (listener e túneis) e o
/// canal de controle. Idempotente.
pub(super) struct Cut {
    proxy: tokio::task::AbortHandle,
    control: Arc<dyn Fn() + Send + Sync>,
    fired: AtomicBool,
}

impl Cut {
    fn fire(&self) -> bool {
        if self.fired.swap(true, Ordering::AcqRel) {
            return false;
        }
        self.proxy.abort();
        (self.control)();
        true
    }
}

type Watched = (String, u64, Arc<Cut>);

/// Instâncias vivas e seus cortes, fora da trava do supervisor: a revogação age mesmo quando outra instância
/// está iniciando (a trava do supervisor fica presa durante o arranque e a autenticação).
#[derive(Clone, Default)]
pub(super) struct Watch(Arc<StdMutex<HashMap<Scope, Watched>>>);

impl Watch {
    fn insert(&self, scope: Scope, owner: String, instance: u64, cut: Arc<Cut>) {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(scope, (owner, instance, cut));
    }

    fn remove(&self, scope: &Scope, instance: u64) {
        let mut watched = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if watched.get(scope).is_some_and(|entry| entry.1 == instance) {
            watched.remove(scope);
        }
    }

    /// Corta toda instância cuja autoridade `invalid` reprova. Devolve quantas cortou agora.
    pub fn revoke(&self, invalid: impl Fn(&Scope, &str) -> bool) -> usize {
        let entries: Vec<_> = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|(scope, entry)| (scope.clone(), entry.0.clone(), Arc::clone(&entry.2)))
            .collect();
        entries
            .iter()
            .filter(|(scope, owner, cut)| !cut.fired.load(Ordering::Acquire) && invalid(scope, owner) && cut.fire())
            .count()
    }
}

struct Session<P> {
    process: P,
    _proxy: super::net_proxy::Proxy,
    profile: PathBuf,
    touched: Instant,
    /// Autoridade que abriu a sessão (vínculo de execução do Hub): outra autoridade nunca a reaproveita.
    owner: String,
    /// Identidade desta instância: resultado e limpeza só valem para a instância que os pediu.
    instance: u64,
    cut: Arc<Cut>,
}

pub(super) struct Supervisor<L: Launcher> {
    launcher: L,
    root: PathBuf,
    // Trava exclusiva impede que outra instância remova perfis ativos.
    _lock: fs::File,
    limit: usize,
    idle: Duration,
    sessions: HashMap<Scope, Session<L::Process>>,
    /// Sessões já cortadas e fora de uso cuja limpeza (processo ou perfil) falhou: a limpeza é tentada de novo.
    retired: Vec<Session<L::Process>>,
    watch: Watch,
    next_instance: u64,
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
            retired: Vec::new(),
            watch: Watch::default(),
            next_instance: 1,
            #[cfg(all(test, target_os = "linux"))]
            test_allowed: std::collections::HashSet::new(),
        })
    }

    #[cfg(all(test, target_os = "linux"))]
    pub fn allow_test_destination(&mut self, address: std::net::SocketAddr) {
        self.test_allowed.insert(address);
    }

    pub async fn start(&mut self, scope: Scope, now: Instant) -> Result<()> {
        self.start_owned(scope, String::new(), now).await.map(|_| ())
    }

    /// Sessão da autoridade `owner`, reaproveitada só se for dela e ainda viva; outra autoridade
    /// ou um processo caído a encerra e abre uma sessão nova e vazia. Devolve a instância vigente.
    pub async fn ensure(&mut self, scope: Scope, owner: &str, now: Instant) -> Result<u64> {
        if let Some(session) = self.sessions.get(&scope) {
            if session.owner == owner && self.launcher.alive(&session.process) {
                let instance = session.instance;
                self.touch(&scope, now)?;
                return Ok(instance);
            }
            self.stop(&scope)?;
        }
        self.start_owned(scope, owner.to_owned(), now).await
    }

    /// Instância da sessão do bot somente se pertencer a `owner`.
    pub fn owned_instance(&self, scope: &Scope, owner: &str) -> Option<u64> {
        self.sessions.get(scope).filter(|session| session.owner == owner).map(|session| session.instance)
    }

    pub fn instance(&self, scope: &Scope) -> Option<u64> {
        self.sessions.get(scope).map(|session| session.instance)
    }

    /// Encerra a sessão só se ainda for a instância pedida (uma sucessora nunca é atingida).
    pub fn stop_instance(&mut self, scope: &Scope, instance: u64) -> Result<bool> {
        if self.instance(scope) != Some(instance) {
            return Ok(false);
        }
        self.stop(scope)?;
        Ok(true)
    }

    /// Registro de cortes, para quem precisa revogar sem pegar a trava do supervisor.
    pub fn watch(&self) -> Watch {
        self.watch.clone()
    }

    pub fn retired_len(&self) -> usize {
        self.retired.len()
    }

    pub fn profile(&self, scope: &Scope) -> Option<&Path> {
        self.sessions.get(scope).map(|session| session.profile.as_path())
    }

    /// Retira do supervisor, sem depender de nova chamada, toda sessão cujo navegador perdeu o canal de
    /// controle (contenção que falhou, Chromium que caiu): encerra navegador e proxy e apaga o perfil.
    pub fn reap_dead(&mut self) -> Result<usize> {
        let dead: Vec<_> = self
            .sessions
            .iter()
            .filter(|(_, session)| !self.launcher.alive(&session.process))
            .map(|(scope, _)| scope.clone())
            .collect();
        let mut errors = Vec::new();
        for scope in &dead {
            if let Err(error) = self.stop(scope) {
                errors.push(format!("{error:#}"));
            }
        }
        ensure!(errors.is_empty(), "falhas ao encerrar sessões caídas: {}", errors.join("; "));
        Ok(dead.len())
    }

    /// Encerra, com limpeza, toda sessão cuja autoridade `invalid` reprova (vínculo revogado, preferência desligada).
    pub fn revoke(&mut self, invalid: impl Fn(&Scope, &str) -> bool) -> Result<usize> {
        let revoked: Vec<_> = self
            .sessions
            .iter()
            .filter(|(scope, session)| invalid(scope, &session.owner))
            .map(|(scope, _)| scope.clone())
            .collect();
        let mut errors = Vec::new();
        for scope in &revoked {
            if let Err(error) = self.stop(scope) {
                errors.push(format!("{error:#}"));
            }
        }
        ensure!(errors.is_empty(), "falhas ao encerrar sessões revogadas: {}", errors.join("; "));
        Ok(revoked.len())
    }

    async fn start_owned(&mut self, scope: Scope, owner: String, now: Instant) -> Result<u64> {
        self.reap_idle(now)?;
        if let Err(error) = self.retry_retired() {
            tracing::warn!(%error, "limpeza pendente de sessões de navegador");
        }
        ensure!(!self.sessions.contains_key(&scope), "já existe sessão para este bot");
        ensure!(self.sessions.len() + self.retired.len() < self.limit, "limite de sessões atingido");
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
                let instance = self.next_instance;
                self.next_instance += 1;
                let cut = Arc::new(Cut {
                    proxy: proxy.abort_handle(),
                    control: self.launcher.cutter(&process),
                    fired: AtomicBool::new(false),
                });
                self.watch.insert(scope.clone(), owner.clone(), instance, Arc::clone(&cut));
                self.sessions
                    .insert(scope, Session { process, _proxy: proxy, profile, touched: now, owner, instance, cut });
                Ok(instance)
            }
            Err(error) => {
                fs::remove_dir_all(&profile).context("falha ao limpar perfil após erro de início")?;
                Err(error)
            }
        }
    }

    pub fn contains(&self, scope: &Scope) -> bool {
        self.sessions.contains_key(scope)
    }

    pub fn process(&self, scope: &Scope) -> Option<&L::Process> {
        self.sessions.get(scope).map(|session| &session.process)
    }

    pub fn launcher_mut(&mut self) -> &mut L {
        &mut self.launcher
    }

    pub fn touch(&mut self, scope: &Scope, now: Instant) -> Result<()> {
        let session = self.sessions.get_mut(scope).context("sessão de navegador ausente")?;
        session.touched = now;
        Ok(())
    }

    /// Retira a sessão do uso e corta proxy e controle ANTES de qualquer limpeza: falha ao parar o processo ou
    /// apagar o perfil não mantém o proxy ativo. Se a limpeza falhar, a sessão fica em `retired` para nova tentativa.
    pub fn stop(&mut self, scope: &Scope) -> Result<()> {
        let mut session = self.sessions.remove(scope).context("sessão de navegador ausente")?;
        self.watch.remove(scope, session.instance);
        session.cut.fire();
        let result = Self::release(&mut self.launcher, &mut session);
        if result.is_err() {
            self.retired.push(session);
        }
        result
    }

    /// Para o processo e apaga o perfil; tenta os dois mesmo que um falhe. A mensagem não leva credencial.
    fn release(launcher: &mut L, session: &mut Session<L::Process>) -> Result<()> {
        let stopped = launcher.stop(&mut session.process);
        let removed = match fs::remove_dir_all(&session.profile) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                Err(anyhow::Error::from(error).context("falha ao limpar perfil do navegador"))
            }
            _ => Ok(()),
        };
        match (stopped, removed) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(stop), Err(remove)) => Err(stop.context(format!("{remove:#}"))),
        }
    }

    /// Tenta de novo a limpeza das sessões que falharam ao encerrar.
    pub fn retry_retired(&mut self) -> Result<usize> {
        let pending = std::mem::take(&mut self.retired);
        let total = pending.len();
        let mut errors = Vec::new();
        for mut session in pending {
            if let Err(error) = Self::release(&mut self.launcher, &mut session) {
                errors.push(format!("{error:#}"));
                self.retired.push(session);
            }
        }
        ensure!(errors.is_empty(), "limpeza pendente de sessões de navegador: {}", errors.join("; "));
        Ok(total)
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
        if let Err(error) = self.retry_retired() {
            tracing::error!(%error, "falha ao limpar sessão de navegador pendente");
        }
    }
}

#[cfg(test)]
mod tests;
