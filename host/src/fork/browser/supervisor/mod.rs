//! Supervisor interno ainda não conectado a ferramentas ou navegação remota.
#![allow(dead_code)]

use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use std::{
    collections::HashMap,
    fs,
    future::Future,
    path::{Path, PathBuf},
    sync::{Arc, Mutex as StdMutex, PoisonError, Weak},
    time::{Duration, Instant},
};

#[cfg(any(target_os = "macos", all(test, target_os = "linux")))]
pub(super) mod mac;
#[cfg(any(target_os = "macos", all(test, target_os = "linux")))]
pub(super) mod mac_policy;
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
    #[cfg(target_os = "macos")]
    fn start_tcp(&mut self, profile: &Path, _proxy: std::net::SocketAddr) -> Result<Self::Process> {
        self.start(profile, &profile.join("proxy.sock"))
    }
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
    /// Registra o aviso a disparar quando o controle do processo cair por falha: a perda corta o proxy da
    /// instância sem depender de trava alguma.
    fn watch_loss(&self, _process: &Self::Process, _lost: Arc<dyn Fn() + Send + Sync>) {}
}

/// Corte de uma instância que não depende da trava do supervisor: derruba o proxy (listener e túneis) e o
/// canal de controle. Os efeitos rodam sob a trava do próprio corte: quem chegar depois só volta quando o corte
/// já está efetivado, então nenhuma limpeza começa antes dele.
pub(super) struct Cut {
    proxy: tokio::task::AbortHandle,
    control: Arc<dyn Fn() + Send + Sync>,
    done: StdMutex<bool>,
}

impl Cut {
    fn new(proxy: tokio::task::AbortHandle, control: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { proxy, control, done: StdMutex::new(false) }
    }

    /// Devolve verdadeiro só para quem executou os efeitos.
    fn fire(&self) -> bool {
        let mut done = self.done.lock().unwrap_or_else(PoisonError::into_inner);
        if *done {
            return false;
        }
        self.proxy.abort();
        (self.control)();
        *done = true;
        true
    }

    fn is_fired(&self) -> bool {
        *self.done.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

type Watched = (String, u64, Arc<Cut>);

/// Instâncias vivas ou em arranque e seus cortes, fora da trava do supervisor: a revogação age mesmo quando
/// outra instância (ou esta) está iniciando, porque a trava do supervisor fica presa durante o arranque.
#[derive(Clone, Default)]
pub(super) struct Watch(Arc<StdMutex<HashMap<Scope, Watched>>>);

impl Watch {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Scope, Watched>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn insert(&self, scope: Scope, owner: String, instance: u64, cut: Arc<Cut>) {
        self.lock().insert(scope, (owner, instance, cut));
    }

    fn remove(&self, scope: &Scope, instance: u64) {
        let mut watched = self.lock();
        if watched.get(scope).is_some_and(|entry| entry.1 == instance) {
            watched.remove(scope);
        }
    }

    /// Autoridade e instância registradas para o escopo, sem esperar trava alguma.
    pub fn entry(&self, scope: &Scope) -> Option<(String, u64)> {
        self.lock().get(scope).map(|entry| (entry.0.clone(), entry.1))
    }

    pub fn visible(&self, scope: &Scope) -> Option<(String, u64)> {
        let (owner, instance, cut) = self.lock().get(scope).cloned()?;
        (!cut.is_fired()).then_some((owner, instance))
    }

    #[cfg(target_os = "macos")]
    pub fn has_live_sessions(&self) -> bool {
        self.lock().values().any(|entry| !entry.2.is_fired())
    }

    /// Corta toda instância cuja autoridade `invalid` reprova. Devolve quantas cortou agora.
    pub fn revoke(&self, invalid: impl Fn(&Scope, &str) -> bool) -> usize {
        let entries: Vec<_> =
            self.lock().iter().map(|(scope, entry)| (scope.clone(), entry.0.clone(), Arc::clone(&entry.2))).collect();
        entries.iter().filter(|(scope, owner, cut)| !cut.is_fired() && invalid(scope, owner) && cut.fire()).count()
    }
}

/// Arranque em andamento: se o futuro for cancelado ou falhar antes de a sessão existir, o corte dispara, o
/// registro some e o perfil é apagado (o processo morre porque `Launcher::Process` encerra o que lançou ao cair).
struct Starting<P> {
    session: Option<Session<P>>,
    watch: Watch,
    scope: Scope,
}

impl<P> Drop for Starting<P> {
    fn drop(&mut self) {
        if let Some(session) = self.session.take() {
            self.watch.remove(&self.scope, session.instance);
            session.cut.fire();
            // Os processos morrem primeiro (o `Drop` da sessão); só depois o perfil é apagado, para o navegador
            // não escrever nele enquanto a remoção corre.
            let profile = session.profile.clone();
            drop(session);
            let _ = fs::remove_dir_all(&profile);
        }
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

    /// Autoridade da sessão vigente do bot, qualquer que seja.
    pub fn owner(&self, scope: &Scope) -> Option<&str> {
        self.sessions.get(scope).map(|session| session.owner.as_str())
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
        #[cfg(not(target_os = "macos"))]
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
        #[cfg(not(target_os = "macos"))]
        let started = self.launcher.start(&profile, &socket);
        #[cfg(target_os = "macos")]
        let started = match proxy.addr() {
            super::net_proxy::Endpoint::Tcp(addr) => self.launcher.start_tcp(&profile, *addr),
            _ => Err(anyhow::anyhow!("proxy local indisponível")),
        };
        let process = match started {
            Ok(process) => process,
            Err(error) => {
                fs::remove_dir_all(&profile).context("falha ao limpar perfil após erro de início")?;
                return Err(error);
            }
        };
        let instance = self.next_instance;
        self.next_instance += 1;
        let cut = Arc::new(Cut::new(proxy.abort_handle(), self.launcher.cutter(&process)));
        // A instância entra no registro de cortes ANTES da autenticação: revogar durante o arranque corta o proxy
        // e o controle na hora, sem esperar o arranque terminar.
        self.watch.insert(scope.clone(), owner.clone(), instance, Arc::clone(&cut));
        let lost: Weak<Cut> = Arc::downgrade(&cut);
        self.launcher.watch_loss(
            &process,
            Arc::new(move || {
                if let Some(cut) = lost.upgrade() {
                    cut.fire();
                }
            }),
        );
        let token = proxy.token().to_owned();
        let mut starting = Starting {
            session: Some(Session {
                process,
                _proxy: proxy,
                profile,
                touched: now,
                owner,
                instance,
                cut: Arc::clone(&cut),
            }),
            watch: self.watch.clone(),
            scope: scope.clone(),
        };
        let authenticated = match starting.session.as_mut() {
            Some(session) => self.launcher.authenticate(&mut session.process, token).await,
            None => Err(anyhow::anyhow!("arranque sem sessão")),
        };
        let outcome = authenticated.and_then(|()| {
            ensure!(!cut.is_fired(), "browser_authority_revoked: a instância foi cortada durante o arranque");
            Ok(())
        });
        let Some(mut session) = starting.session.take() else { anyhow::bail!("arranque sem sessão") };
        if let Err(error) = outcome {
            self.watch.remove(&scope, instance);
            session.cut.fire();
            if let Err(cleanup) = Self::release(&mut self.launcher, &mut session) {
                tracing::warn!(error = %cleanup, "limpeza pendente depois de falha de arranque");
                self.retired.push(session);
            }
            return Err(error);
        }
        self.sessions.insert(scope, session);
        Ok(instance)
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

    /// Só apaga o perfil depois de parar o processo. A mensagem não leva credencial.
    fn release(launcher: &mut L, session: &mut Session<L::Process>) -> Result<()> {
        launcher.stop(&mut session.process)?;
        match fs::remove_dir_all(&session.profile) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                Err(anyhow::Error::from(error).context("falha ao limpar perfil do navegador"))
            }
            _ => Ok(()),
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
