//! Estado do navegador no Hub: um supervisor criado sob demanda e uma sessão Chromium por bot
//! (Hub, Space e bot), com limite, ociosidade, revogação e limpeza. Sem Chromium, Xvfb ou contenção, nada abre.

use super::supervisor::Scope;
use anyhow::Result;
use serde_json::Value;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[cfg(target_os = "linux")]
use super::{
    cdp::Cdp,
    pages,
    supervisor::{Supervisor, real::RealLauncher},
};
#[cfg(target_os = "linux")]
use anyhow::{Context, anyhow};
#[cfg(target_os = "linux")]
use std::path::PathBuf;

const NEGOTIATED_MAX: usize = 1024;
const PROBE_TTL: Duration = Duration::from_secs(60);
#[cfg(target_os = "linux")]
const LIMIT: usize = 3;
#[cfg(target_os = "linux")]
const IDLE: Duration = Duration::from_secs(300);
/// Período entre conferências de autoridade e de manutenção (revogada, caída, ociosa). É o período de checagem,
/// não um teto de latência: o corte de uma revogação ainda espera o próximo ciclo, a leitura do banco e o
/// agendador do runtime.
#[cfg(target_os = "linux")]
const SWEEP: Duration = Duration::from_millis(250);

/// A autoridade (vínculo de execução) de uma sessão ainda vale? Falso derruba a sessão na próxima varredura.
pub(super) type Authority = Arc<dyn Fn(&Scope, &str) -> bool + Send + Sync>;

/// Ponto de pausa dos testes: avisa que chegou e espera a liberação.
#[cfg(all(test, target_os = "linux"))]
type PauseHook = (Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>);

/// Resultado de uma chamada de página. A instância usada volta mesmo em erro, para o despacho poder
/// encerrar exatamente essa instância se a origem mudar durante a chamada.
pub(super) struct Outcome {
    pub instance: Option<u64>,
    pub result: Result<Value>,
}
impl Outcome {
    fn failed(instance: Option<u64>, error: anyhow::Error) -> Self {
        Self { instance, result: Err(error) }
    }
}

#[cfg(target_os = "linux")]
pub(super) type Pipe = tokio::net::UnixStream;
#[cfg(target_os = "linux")]
type Sessions = Arc<tokio::sync::Mutex<Option<Supervisor<RealLauncher>>>>;

#[derive(Default)]
pub(crate) struct State {
    #[cfg(target_os = "linux")]
    sessions: Sessions,
    probe: Mutex<Option<(Instant, Result<(), &'static str>)>>,
    /// Vínculos de execução que já negociaram o contrato v3 (`browser_status` v3 disponível): só eles veem
    /// e usam as ferramentas de página. Cheio, esvazia: cliente renegocia (falha fechada).
    negotiated: Mutex<HashSet<String>>,
    authority: Arc<Mutex<Option<Authority>>>,
    /// Registro de cortes do supervisor: consulta de instância e revogação sem pegar a trava dele.
    watch: Mutex<Option<super::supervisor::Watch>>,
    #[cfg(all(test, target_os = "linux"))]
    pauses: Mutex<std::collections::HashMap<&'static str, PauseHook>>,
}

#[cfg(target_os = "linux")]
fn executable(variable: &str, candidates: &[&str]) -> Option<PathBuf> {
    let from_env = std::env::var_os(variable).map(PathBuf::from);
    from_env.into_iter().chain(candidates.iter().map(PathBuf::from)).find(|path| path.is_absolute() && path.is_file())
}

/// Só Chromium de sistema (nunca Google Chrome) e Xvfb.
#[cfg(target_os = "linux")]
fn engine() -> Option<(PathBuf, PathBuf)> {
    Some((
        executable("MEXERICA_CHROMIUM", &["/usr/bin/chromium", "/usr/bin/chromium-browser"])?,
        executable("MEXERICA_XVFB", &["/usr/bin/Xvfb"])?,
    ))
}

#[cfg(target_os = "linux")]
fn probe() -> Result<(), &'static str> {
    if engine().is_none() {
        return Err("engine_missing");
    }
    // SAFETY: geteuid não recebe ponteiros nem altera memória.
    if unsafe { libc::geteuid() } == 0 {
        return Err("running_as_root");
    }
    match super::supervisor::real::privileged_group() {
        Ok(None) => {}
        Ok(Some(_)) => return Err("privileged_group"),
        Err(_) => return Err("containment_unavailable"),
    }
    super::bridge::ensure_namespaces_available().map_err(|_| "containment_unavailable")
}

#[cfg(not(target_os = "linux"))]
fn probe() -> Result<(), &'static str> {
    Err("unsupported_platform")
}

impl State {
    pub(super) fn negotiate(&self, binding: &str) {
        if let Ok(mut set) = self.negotiated.lock() {
            if set.len() >= NEGOTIATED_MAX {
                set.clear();
            }
            set.insert(binding.to_owned());
        }
    }

    pub(crate) fn negotiated(&self, binding: &str) -> bool {
        self.negotiated.lock().is_ok_and(|set| set.contains(binding))
    }

    /// Regra que a varredura aplica a cada sessão a cada ciclo; só a primeira registrada vale.
    pub(super) fn watch_authority(&self, authority: &Authority) {
        if let Ok(mut slot) = self.authority.lock()
            && slot.is_none()
        {
            *slot = Some(Arc::clone(authority));
        }
    }

    /// Resultado da sonda de contenção, guardado por um minuto: a consulta de status não relança namespaces a cada vez.
    pub(super) fn availability(&self) -> Result<(), &'static str> {
        let Ok(mut cached) = self.probe.lock() else { return Err("containment_unavailable") };
        if let Some((at, result)) = *cached
            && at.elapsed() < PROBE_TTL
        {
            return result;
        }
        let result = probe();
        *cached = Some((Instant::now(), result));
        result
    }
}

#[cfg(target_os = "linux")]
impl State {
    /// Abre (ou reabre) a página na sessão da autoridade `owner`. O destino é validado antes de alocar
    /// qualquer sessão; sessão de outra autoridade ou cujo CDP caiu é descartada e recriada vazia. `valid` é
    /// conferida logo depois de a sessão existir e imediatamente antes da navegação: autoridade revogada
    /// nesse intervalo encerra a instância e nada é navegado.
    pub(super) async fn open(
        &self,
        scope: &Scope,
        owner: &str,
        hidden: Vec<PathBuf>,
        url: &str,
        valid: impl Fn() -> bool + Sync,
    ) -> Outcome {
        if let Err(error) = super::nav::validate_navigation_url(url).await {
            return Outcome::failed(None, error);
        }
        let (cdp, page, instance) = match self.session(scope, owner, hidden, &valid).await {
            Ok(handle) => handle,
            Err(error) => return Outcome::failed(None, error),
        };
        #[cfg(test)]
        self.pause_point("open").await;
        if !valid() {
            let _ = self.close_instance(scope, instance).await;
            return Outcome::failed(
                Some(instance),
                anyhow!("browser_authority_revoked: a autoridade da sessão foi revogada"),
            );
        }
        let result = match pages::open(&cdp, &page, url, &valid).await {
            Ok(value) => self.confirm(scope, owner, instance).await.map(|()| value),
            Err(error) => Err(error),
        };
        Outcome { instance: Some(instance), result }
    }

    /// Instância vigente (ou em arranque) do bot para o vínculo, sem esperar trava alguma. Quem chama captura
    /// isso ANTES de qualquer espera: ler e fechar valem só para essa instância.
    pub(super) fn watched_instance(&self, scope: &Scope, binding: &str) -> Option<u64> {
        let watch = self.watch.lock().ok()?.clone()?;
        let (owner, instance) = watch.entry(scope)?;
        owner.starts_with(&format!("{binding}#")).then_some(instance)
    }

    /// Lê a página da instância `expected` da autoridade `owner`; nunca cria uma sessão. Outra instância (ainda
    /// que do mesmo vínculo) nunca é lida.
    pub(super) async fn read(
        &self,
        scope: &Scope,
        owner: &str,
        expected: Option<u64>,
        valid: impl Fn() -> bool + Sync,
    ) -> Outcome {
        let handle = {
            let mut guard = self.sessions.lock().await;
            let current = guard.as_ref().and_then(|supervisor| supervisor.owned_instance(scope, owner));
            let instance = match (current, expected) {
                (Some(current), Some(expected)) if current == expected => current,
                (_, Some(_)) => {
                    return Outcome::failed(
                        None,
                        anyhow!("browser_session_changed: a sessão foi encerrada ou trocada"),
                    );
                }
                (_, None) => {
                    return Outcome::failed(None, anyhow!("browser_no_page: abra uma página com browser_open antes"));
                }
            };
            let Some(supervisor) = guard.as_mut() else {
                return Outcome::failed(None, anyhow!("browser_session_closed"));
            };
            match supervisor.touch(scope, Instant::now()).and_then(|()| Self::handle(supervisor, scope)) {
                Ok((cdp, page)) => (cdp, page, instance),
                Err(error) => return Outcome::failed(Some(instance), error),
            }
        };
        let (cdp, page, instance) = handle;
        if !valid() {
            let _ = self.close_instance(scope, instance).await;
            return Outcome::failed(
                Some(instance),
                anyhow!("browser_authority_revoked: a autoridade da sessão foi revogada"),
            );
        }
        let result = match pages::read(&cdp, &page, &valid).await {
            Ok(value) => {
                #[cfg(test)]
                self.pause_point("read").await;
                self.confirm(scope, owner, instance).await.map(|()| value)
            }
            Err(error) => Err(error),
        };
        Outcome { instance: Some(instance), result }
    }

    /// Encerra a instância `expected` do vínculo `binding` (qualquer geração dele) e apaga o perfil. Uma
    /// instância sucessora, ou de outro vínculo, nunca é atingida; sem instância pretendida não há o que fechar.
    pub(super) async fn close(&self, scope: &Scope, binding: &str, expected: Option<u64>) -> Result<Option<u64>> {
        let Some(expected) = expected else { return Ok(None) };
        let mut guard = self.sessions.lock().await;
        let Some(supervisor) = guard.as_mut() else { return Ok(None) };
        let ours = supervisor.owner(scope).is_some_and(|owner| owner.starts_with(&format!("{binding}#")));
        if !ours || supervisor.instance(scope) != Some(expected) {
            return Ok(None);
        }
        supervisor.stop(scope)?;
        Ok(Some(expected))
    }

    /// Limpeza por instância: uma sessão sucessora do mesmo bot nunca é atingida.
    pub(super) async fn close_instance(&self, scope: &Scope, instance: u64) -> Result<bool> {
        match self.sessions.lock().await.as_mut() {
            Some(supervisor) => supervisor.stop_instance(scope, instance),
            None => Ok(false),
        }
    }

    async fn session(
        &self,
        scope: &Scope,
        owner: &str,
        hidden: Vec<PathBuf>,
        valid: &(impl Fn() -> bool + Sync),
    ) -> Result<(Cdp<Pipe>, String, u64)> {
        let mut guard = self.sessions.lock().await;
        if guard.is_none() {
            let (chromium, xvfb) =
                engine().ok_or_else(|| anyhow!("browser_engine_missing: Chromium ou Xvfb ausente"))?;
            let root = crate::service::data_dir().join("browser");
            #[cfg(test)]
            let host_binary = std::env::current_exe()
                .ok()
                .and_then(|path| path.parent()?.parent().map(|dir| dir.join("donnabot-host")));
            #[cfg(not(test))]
            let host_binary = None;
            let supervisor = Supervisor::new(
                RealLauncher { chromium, xvfb, host_binary, extra_hidden: Vec::new() },
                root,
                LIMIT,
                IDLE,
            )?;
            if let Ok(mut slot) = self.watch.lock() {
                *slot = Some(supervisor.watch());
            }
            tokio::spawn(cut_revoked(supervisor.watch(), Arc::downgrade(&self.sessions), Arc::clone(&self.authority)));
            tokio::spawn(sweep(self.sessions.clone()));
            *guard = Some(supervisor);
        }
        let supervisor = guard.as_mut().context("supervisor ausente")?;
        if !valid() {
            anyhow::bail!("browser_authority_revoked: a autoridade da sessão foi revogada");
        }
        supervisor.launcher_mut().extra_hidden = hidden;
        let instance = supervisor.ensure(scope.clone(), owner, Instant::now()).await?;
        if !valid() {
            supervisor.stop_instance(scope, instance)?;
            anyhow::bail!("browser_authority_revoked: a autoridade da sessão foi revogada");
        }
        let (cdp, page) = Self::handle(supervisor, scope)?;
        Ok((cdp, page, instance))
    }

    /// O resultado só vale se a mesma instância, da mesma autoridade, ainda for a vigente.
    async fn confirm(&self, scope: &Scope, owner: &str, instance: u64) -> Result<()> {
        let guard = self.sessions.lock().await;
        let current = guard.as_ref().and_then(|supervisor| supervisor.owned_instance(scope, owner));
        anyhow::ensure!(current == Some(instance), "browser_session_changed: a sessão foi encerrada ou trocada");
        Ok(())
    }

    fn handle(supervisor: &Supervisor<RealLauncher>, scope: &Scope) -> Result<(Cdp<Pipe>, String)> {
        let cdp = supervisor
            .process(scope)
            .and_then(super::supervisor::real::Processes::cdp)
            .ok_or_else(|| anyhow!("browser_session_closed"))?;
        let page = cdp.main_session().ok_or_else(|| anyhow!("browser_session_closed"))?;
        Ok((cdp, page))
    }

    #[cfg(test)]
    async fn pause_point(&self, name: &str) {
        let hook = self.pauses.lock().ok().and_then(|pauses| pauses.get(name).cloned());
        if let Some((reached, proceed)) = hook {
            reached.notify_one();
            proceed.notified().await;
        }
    }
}

/// Corte de revogação: sem pegar a trava do supervisor (que o arranque de outra instância pode segurar),
/// derruba proxy e controle de toda instância cuja autoridade foi revogada (vínculo revogado ou trocado,
/// troca de conta ou de Space, bot apagado, preferência fora de Integrado). Os processos são encerrados
/// depois, pela manutenção. Termina quando o Hub solta o estado.
#[cfg(target_os = "linux")]
async fn cut_revoked(
    watch: super::supervisor::Watch,
    sessions: std::sync::Weak<tokio::sync::Mutex<Option<Supervisor<RealLauncher>>>>,
    authority: Arc<Mutex<Option<Authority>>>,
) {
    loop {
        tokio::time::sleep(SWEEP).await;
        if sessions.strong_count() == 0 {
            return;
        }
        let check = authority.lock().ok().and_then(|slot| slot.clone());
        if let Some(check) = check {
            watch.revoke(|scope, owner| !check(scope, owner));
        }
    }
}

/// Manutenção: encerra (processos, proxy e perfil), sem esperar nova chamada, as sessões cujo navegador perdeu o
/// canal de controle ou foi cortado, as que ficaram ociosas e as que falharam ao encerrar antes.
/// Termina quando o Hub solta o estado.
#[cfg(target_os = "linux")]
async fn sweep(sessions: Sessions) {
    loop {
        tokio::time::sleep(SWEEP).await;
        if Arc::strong_count(&sessions) == 1 {
            return;
        }
        let mut guard = sessions.lock().await;
        let Some(supervisor) = guard.as_mut() else { continue };
        if let Err(error) = supervisor.reap_dead() {
            tracing::warn!(%error, "falha ao encerrar sessões de navegador sem canal de controle");
        }
        if let Err(error) = supervisor.reap_idle(Instant::now()) {
            tracing::warn!(%error, "falha ao expirar sessões ociosas do navegador");
        }
        if let Err(error) = supervisor.retry_retired() {
            tracing::warn!(%error, "limpeza pendente de sessões de navegador");
        }
    }
}

#[cfg(not(target_os = "linux"))]
impl State {
    pub(super) async fn open(
        &self,
        _scope: &Scope,
        _owner: &str,
        _hidden: Vec<std::path::PathBuf>,
        _url: &str,
        _valid: impl Fn() -> bool + Sync,
    ) -> Outcome {
        Outcome::failed(
            None,
            anyhow::anyhow!("browser_engine_missing: o navegador integrado só roda em servidor Linux"),
        )
    }
    pub(super) fn watched_instance(&self, _scope: &Scope, _binding: &str) -> Option<u64> {
        None
    }
    pub(super) async fn read(
        &self,
        _scope: &Scope,
        _owner: &str,
        _expected: Option<u64>,
        _valid: impl Fn() -> bool + Sync,
    ) -> Outcome {
        Outcome::failed(None, anyhow::anyhow!("browser_no_page: abra uma página com browser_open antes"))
    }
    pub(super) async fn close(&self, _scope: &Scope, _binding: &str, _expected: Option<u64>) -> Result<Option<u64>> {
        Ok(None)
    }
    pub(super) async fn close_instance(&self, _scope: &Scope, _instance: u64) -> Result<bool> {
        Ok(false)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn scope(bot: &str) -> Scope {
        Scope { hub: "hub".into(), space: "space".into(), bot: bot.into() }
    }

    fn done(outcome: Outcome) -> Result<(Value, u64)> {
        let instance = outcome.instance.context("instância ausente")?;
        Ok((outcome.result?, instance))
    }

    fn authority(flag: &Arc<AtomicBool>) -> Authority {
        let flag = Arc::clone(flag);
        Arc::new(move |_, _| flag.load(Ordering::SeqCst))
    }

    fn valid(flag: &Arc<AtomicBool>) -> impl Fn() -> bool + Send + 'static {
        let flag = Arc::clone(flag);
        move || flag.load(Ordering::SeqCst)
    }

    fn key(binding: &str) -> String {
        format!("{binding}#1")
    }

    async fn read_as(state: &State, scope: &Scope, binding: &str, live: &Arc<AtomicBool>) -> Outcome {
        let expected = state.watched_instance(scope, binding);
        state.read(scope, &key(binding), expected, valid(live)).await
    }

    async fn close_as(state: &State, scope: &Scope, binding: &str) -> Result<Option<u64>> {
        let expected = state.watched_instance(scope, binding);
        state.close(scope, binding, expected).await
    }

    async fn profile_of(state: &State, scope: &Scope) -> Option<PathBuf> {
        state.sessions.lock().await.as_ref()?.profile(scope).map(PathBuf::from)
    }

    async fn instance_of(state: &State, scope: &Scope) -> Option<u64> {
        state.sessions.lock().await.as_ref()?.instance(scope)
    }

    async fn cdp_of(state: &State, scope: &Scope) -> Option<Cdp<Pipe>> {
        let guard = state.sessions.lock().await;
        State::handle(guard.as_ref()?, scope).ok().map(|(cdp, _)| cdp)
    }

    fn pause(state: &State, name: &'static str) -> (Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>) {
        let hook = (Arc::new(tokio::sync::Notify::new()), Arc::new(tokio::sync::Notify::new()));
        state.pauses.lock().expect("pausas").insert(name, hook.clone());
        hook
    }

    /// Processos (Chromium, Xvfb, ponte) que ainda carregam o perfil na linha de comando ou no ambiente.
    fn processes_of(profile: &std::path::Path) -> Vec<u32> {
        let needle = profile.display().to_string().into_bytes();
        std::fs::read_dir("/proc")
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
                let mentions = |file: &str| {
                    std::fs::read(entry.path().join(file))
                        .is_ok_and(|bytes| bytes.windows(needle.len()).any(|w| w == needle))
                };
                (mentions("cmdline") || mentions("environ")).then_some(pid)
            })
            .collect()
    }

    async fn until(mut condition: impl AsyncFnMut() -> bool) -> bool {
        for _ in 0..60 {
            if condition().await {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        false
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requer Chromium, Xvfb, namespaces e internet (Linux)"]
    async fn abre_e_le_pagina_publica_pelo_supervisor_real() -> Result<()> {
        let state = State::default();
        let live = Arc::new(AtomicBool::new(true));
        let s = scope("bot");
        assert!(read_as(&state, &s, "a", &live).await.result.is_err(), "ler sem abrir não cria sessão");
        let (opened, instance) =
            done(state.open(&s, &key("a"), Vec::new(), "https://example.com/", valid(&live)).await)?;
        assert_eq!(opened["status"], "opened");
        assert!(opened["page"]["url"].as_str().unwrap_or_default().starts_with("https://example.com"), "{opened}");
        let (read, same) = done(read_as(&state, &s, "a", &live).await)?;
        assert_eq!(same, instance);
        assert_eq!(read["page"]["title"], "Example Domain");
        assert!(read["page"]["text"].as_str().unwrap_or_default().contains("documentation examples"), "{read}");
        assert_eq!(read["untrustedContent"], true);
        for url in ["https://localhost/", "https://127.0.0.1/", "https://2852039166/", "file:///etc/hostname"] {
            assert!(state.open(&s, &key("a"), Vec::new(), url, valid(&live)).await.result.is_err(), "{url}");
        }
        let (again, _) = done(read_as(&state, &s, "a", &live).await)?;
        assert_eq!(again["page"]["title"], "Example Domain", "recusa não mexe na página");
        let (_, reused) = done(state.open(&s, &key("a"), Vec::new(), "https://example.com/", valid(&live)).await)?;
        assert_eq!(reused, instance, "mesma autoridade reaproveita");
        assert!(read_as(&state, &s, "b", &live).await.result.is_err(), "outra autoridade não lê a página alheia");
        assert_eq!(close_as(&state, &s, "b").await?, None, "outra autoridade não fecha a sessão alheia");
        let (_, replaced) = done(state.open(&s, &key("b"), Vec::new(), "https://example.com/", valid(&live)).await)?;
        assert_ne!(replaced, instance, "outra autoridade ganha sessão nova e vazia");
        assert!(!state.close_instance(&s, instance).await?, "limpeza da instância antiga não atinge a sucessora");
        assert!(read_as(&state, &s, "b", &live).await.result.is_ok());
        assert!(close_as(&state, &s, "b").await?.is_some());
        assert_eq!(close_as(&state, &s, "b").await?, None);
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requer Chromium, Xvfb, namespaces e internet (Linux)"]
    async fn revogacao_derruba_pagina_ativa_e_abertura_em_andamento_sem_nova_chamada() -> Result<()> {
        let state = Arc::new(State::default());
        let live = Arc::new(AtomicBool::new(true));
        state.watch_authority(&authority(&live));
        let s = scope("bot");
        done(state.open(&s, &key("a"), Vec::new(), "https://example.com/", valid(&live)).await)?;
        let profile = profile_of(&state, &s).await.context("perfil")?;
        assert!(!processes_of(&profile).is_empty(), "controle positivo: o navegador está rodando");
        live.store(false, Ordering::SeqCst);
        assert!(
            until(async || instance_of(&state, &s).await.is_none()).await,
            "página ativa continuou aberta sem nova chamada"
        );
        assert!(
            until(async || !profile.exists() && processes_of(&profile).is_empty()).await,
            "sobrou perfil ou processo da sessão revogada"
        );

        for wait_for_sweep in [true, false] {
            live.store(true, Ordering::SeqCst);
            let (reached, proceed) = pause(&state, "open");
            let task = tokio::spawn({
                let (state, live, s) = (Arc::clone(&state), Arc::clone(&live), s.clone());
                async move { state.open(&s, &key("a"), Vec::new(), "https://example.com/", valid(&live)).await }
            });
            reached.notified().await;
            let profile = profile_of(&state, &s).await.context("perfil da abertura em andamento")?;
            live.store(false, Ordering::SeqCst);
            if wait_for_sweep {
                assert!(
                    until(async || instance_of(&state, &s).await.is_none()).await,
                    "abertura em andamento sobreviveu à revogação"
                );
            }
            proceed.notify_one();
            let outcome = task.await?;
            outcome.result.expect_err("a abertura revogada não pode devolver página");
            assert!(instance_of(&state, &s).await.is_none(), "a instância revogada continuou no supervisor");
            assert!(
                until(async || !profile.exists() && processes_of(&profile).is_empty()).await,
                "sobrou processo da abertura revogada"
            );
        }
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requer Chromium, Xvfb, namespaces e internet (Linux)"]
    async fn abertura_vencida_nao_derruba_a_sessao_do_vinculo_sucessor() -> Result<()> {
        let state = State::default();
        let live = Arc::new(AtomicBool::new(true));
        let s = scope("bot");
        let (_, current) = done(state.open(&s, &key("novo"), Vec::new(), "https://example.com/", valid(&live)).await)?;
        let revoked = Arc::new(AtomicBool::new(false));
        let outcome = state.open(&s, &key("antigo"), Vec::new(), "https://example.com/", valid(&revoked)).await;
        outcome.result.expect_err("a abertura sem autoridade não pode seguir");
        assert_eq!(instance_of(&state, &s).await, Some(current), "a sessão do vínculo vigente foi derrubada");
        assert!(read_as(&state, &s, "novo", &live).await.result.is_ok());
        assert!(close_as(&state, &s, "novo").await?.is_some());
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requer Chromium, Xvfb, namespaces e internet (Linux)"]
    async fn falha_de_contencao_encerra_chromium_e_proxy_sem_nova_chamada() -> Result<()> {
        let state = State::default();
        let live = Arc::new(AtomicBool::new(true));
        state.watch_authority(&authority(&live));
        let s = scope("bot");
        let (_, instance) = done(state.open(&s, &key("a"), Vec::new(), "https://example.com/", valid(&live)).await)?;
        let profile = profile_of(&state, &s).await.context("perfil")?;
        let socket = profile.join("proxy.sock");
        assert!(socket.exists() && !processes_of(&profile).is_empty(), "controle positivo: proxy e navegador de pé");
        cdp_of(&state, &s).await.context("cdp")?.sever();
        assert!(
            until(async || instance_of(&state, &s).await.is_none()).await,
            "sessão sem canal de controle continuou no supervisor"
        );
        assert!(until(async || processes_of(&profile).is_empty()).await, "sobrou Chromium, Xvfb ou ponte");
        assert!(!socket.exists() && !profile.exists(), "sobrou o proxy ou o perfil");
        let (_, next) = done(state.open(&s, &key("a"), Vec::new(), "https://example.com/", valid(&live)).await)?;
        assert_ne!(next, instance, "a próxima abertura começa numa instância nova");
        assert!(close_as(&state, &s, "a").await?.is_some());
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requer Chromium, Xvfb, namespaces e internet (Linux)"]
    async fn leitura_e_fechamento_ficam_presos_a_instancia_pretendida_antes_da_fila() -> Result<()> {
        let state = State::default();
        let live = Arc::new(AtomicBool::new(true));
        let s = scope("bot");
        let (_, first) = done(state.open(&s, &key("a"), Vec::new(), "https://example.com/", valid(&live)).await)?;
        let expected = state.watched_instance(&s, "a");
        assert_eq!(expected, Some(first), "a instância pretendida é lida sem esperar trava alguma");
        assert_eq!(state.watched_instance(&s, "outro"), None, "outro vínculo não enxerga a instância");
        assert_eq!(close_as(&state, &s, "a").await?, Some(first));
        let (_, second) = done(state.open(&s, &key("a"), Vec::new(), "https://example.com/", valid(&live)).await)?;
        let late = state.read(&s, &key("a"), expected, valid(&live)).await;
        let error = late.result.expect_err("a leitura pedida para a instância antiga não pode ler a nova").to_string();
        assert!(error.starts_with("browser_session_changed"), "{error}");
        assert_eq!(state.close(&s, "a", expected).await?, None, "o fechamento antigo não atinge a substituta");
        assert_eq!(instance_of(&state, &s).await, Some(second), "a substituta foi derrubada");
        assert_eq!(state.close(&s, "a", None).await?, None, "sem instância pretendida não há o que fechar");
        let none = state.read(&s, &key("a"), None, valid(&live)).await;
        let error = none.result.expect_err("sem instância pretendida não há o que ler").to_string();
        assert!(error.starts_with("browser_no_page"), "{error}");
        assert_eq!(close_as(&state, &s, "a").await?, Some(second));
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requer Chromium, Xvfb, namespaces e internet (Linux)"]
    async fn leitura_concorrente_com_fechar_e_reabrir_descarta_o_resultado_velho() -> Result<()> {
        let state = Arc::new(State::default());
        let live = Arc::new(AtomicBool::new(true));
        let s = scope("bot");
        let (_, first) = done(state.open(&s, &key("a"), Vec::new(), "https://example.com/", valid(&live)).await)?;
        let (reached, proceed) = pause(&state, "read");
        let reading = tokio::spawn({
            let (state, live, s) = (Arc::clone(&state), Arc::clone(&live), s.clone());
            async move { read_as(&state, &s, "a", &live).await }
        });
        reached.notified().await;
        assert_eq!(close_as(&state, &s, "a").await?, Some(first));
        let (_, second) = done(state.open(&s, &key("a"), Vec::new(), "https://example.com/", valid(&live)).await)?;
        assert_ne!(first, second);
        state.pauses.lock().expect("pausas").clear();
        proceed.notify_one();
        let outcome = reading.await?;
        assert_eq!(outcome.instance, Some(first));
        let error = outcome.result.expect_err("o texto da instância antiga não pode ser entregue").to_string();
        assert!(error.starts_with("browser_session_changed"), "{error}");
        assert_eq!(instance_of(&state, &s).await, Some(second), "a sessão sucessora não foi atingida");
        assert!(!state.close_instance(&s, first).await?);
        assert!(read_as(&state, &s, "a", &live).await.result.is_ok());
        assert!(close_as(&state, &s, "a").await?.is_some());
        Ok(())
    }
}
