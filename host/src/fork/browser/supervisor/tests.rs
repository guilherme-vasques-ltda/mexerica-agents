use super::*;
use std::{cell::RefCell, rc::Rc, sync::Mutex};

#[derive(Default)]
struct Fake {
    events: Rc<RefCell<Vec<String>>>,
    fail: bool,
    fail_stop: bool,
    dead: Rc<RefCell<Vec<PathBuf>>>,
    cut_log: Arc<Mutex<Vec<PathBuf>>>,
    /// Arranque lento: avisa que chegou na autenticação e espera a liberação.
    hold: Option<(Rc<tokio::sync::Notify>, Rc<tokio::sync::Notify>)>,
}
impl Launcher for Fake {
    type Process = PathBuf;
    fn start(&mut self, profile: &Path, proxy: &Path) -> Result<PathBuf> {
        self.events.borrow_mut().push(format!("start {}", proxy.display()));
        ensure!(!self.fail, "falha simulada");
        Ok(profile.to_owned())
    }
    #[allow(clippy::unused_async, clippy::unused_async_trait_impl)]
    async fn authenticate(&mut self, _process: &mut PathBuf, token: String) -> Result<()> {
        assert_ne!(token, "");
        if let Some((started, release)) = self.hold.take() {
            started.notify_one();
            release.notified().await;
        }
        self.events.borrow_mut().push("auth".into());
        Ok(())
    }
    fn alive(&self, process: &PathBuf) -> bool {
        !self.dead.borrow().contains(process) && !self.cut_log.lock().is_ok_and(|cut| cut.contains(process))
    }
    fn cutter(&self, process: &PathBuf) -> Arc<dyn Fn() + Send + Sync> {
        let (log, process) = (self.cut_log.clone(), process.clone());
        Arc::new(move || {
            if let Ok(mut cut) = log.lock() {
                cut.push(process.clone());
            }
        })
    }
    fn stop(&mut self, process: &mut PathBuf) -> Result<()> {
        self.events.borrow_mut().push(format!("stop {}", process.display()));
        ensure!(!self.fail_stop, "falha simulada ao parar");
        Ok(())
    }
}
fn scope(bot: &str) -> Scope {
    Scope { hub: "hub".into(), space: "space".into(), bot: bot.into() }
}
fn root() -> PathBuf {
    std::env::temp_dir().join(format!("mex-supervisor-test-{}", uuid::Uuid::new_v4()))
}

#[tokio::test]
async fn limita_isola_expira_e_limpa() -> Result<()> {
    let root = root();
    let fake = Fake::default();
    let events = fake.events.clone();
    let now = Instant::now();
    let mut supervisor = Supervisor::new(fake, root.clone(), 2, Duration::from_secs(10))?;
    supervisor.start(scope("a"), now).await?;
    supervisor.start(scope("b"), now).await?;
    assert_ne!(supervisor.sessions[&scope("a")].profile, supervisor.sessions[&scope("b")].profile);
    assert!(supervisor.start(scope("a"), now).await.is_err());
    assert!(supervisor.start(scope("c"), now).await.is_err());
    supervisor.touch(&scope("b"), now + Duration::from_secs(5))?;
    supervisor.reap_idle(now + Duration::from_secs(10))?;
    assert!(!supervisor.sessions.contains_key(&scope("a")));
    assert!(supervisor.sessions.contains_key(&scope("b")));
    drop(supervisor);
    assert_eq!(events.borrow().len(), 6);
    assert_eq!(fs::read_dir(&root)?.count(), 1);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[tokio::test]
async fn sessao_pertence_a_autoridade_que_abriu_e_so_ela_reaproveita() -> Result<()> {
    let root = root();
    let fake = Fake::default();
    let (events, dead) = (fake.events.clone(), fake.dead.clone());
    let now = Instant::now();
    let mut supervisor = Supervisor::new(fake, root.clone(), 3, Duration::from_secs(10))?;
    let first = supervisor.ensure(scope("a"), "vinculo-1", now).await?;
    assert_eq!(supervisor.ensure(scope("a"), "vinculo-1", now).await?, first, "mesma autoridade reaproveita");
    assert_eq!(supervisor.owned_instance(&scope("a"), "vinculo-1"), Some(first));
    assert_eq!(supervisor.owned_instance(&scope("a"), "vinculo-2"), None, "outra autoridade não enxerga a sessão");
    let old_profile = supervisor.sessions[&scope("a")].profile.clone();
    let second = supervisor.ensure(scope("a"), "vinculo-2", now).await?;
    assert_ne!(first, second, "troca de autoridade descarta a sessão e abre outra vazia");
    assert!(!old_profile.exists(), "o perfil da autoridade anterior é apagado");
    assert!(events.borrow().iter().any(|event| event.starts_with("stop ")));
    assert_eq!(supervisor.owned_instance(&scope("a"), "vinculo-1"), None);
    assert!(!supervisor.stop_instance(&scope("a"), first)?, "limpeza da instância antiga não atinge a sucessora");
    assert!(supervisor.contains(&scope("a")));
    let profile = supervisor.sessions[&scope("a")].profile.clone();
    dead.borrow_mut().push(profile);
    let third = supervisor.ensure(scope("a"), "vinculo-2", now).await?;
    assert_ne!(second, third, "processo caído é recriado vazio");
    supervisor.ensure(scope("b"), "vinculo-3", now).await?;
    assert_eq!(supervisor.revoke(|_, owner| owner == "vinculo-3")?, 1);
    assert!(!supervisor.contains(&scope("b")) && supervisor.contains(&scope("a")), "revoga só a autoridade reprovada");
    assert!(supervisor.stop_instance(&scope("a"), third)?);
    assert!(!supervisor.contains(&scope("a")));
    drop(supervisor);
    fs::remove_dir_all(root)?;
    Ok(())
}

/// Raiz curta: o caminho do socket do proxy precisa caber nos 108 bytes de um endereço Unix para o teste conectar.
fn short_root() -> PathBuf {
    PathBuf::from("/tmp").join(format!("mxs-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]))
}

async fn aceita_conexao(socket: &Path) -> bool {
    tokio::net::UnixStream::connect(socket).await.is_ok()
}

async fn deixa_de_aceitar(socket: &Path) -> bool {
    for _ in 0..200 {
        if !aceita_conexao(socket).await {
            return true;
        }
        tokio::task::yield_now().await;
    }
    false
}

#[tokio::test]
async fn revogacao_corta_proxy_e_controle_sem_esperar_o_arranque_de_outra_instancia() -> Result<()> {
    let root = short_root();
    let fake = Fake::default();
    let cuts = fake.cut_log.clone();
    let mut supervisor = Supervisor::new(fake, root.clone(), 3, Duration::from_secs(60))?;
    let now = Instant::now();
    supervisor.ensure(scope("a"), "vinculo-a", now).await?;
    let profile = supervisor.sessions[&scope("a")].profile.clone();
    let socket = profile.join("proxy.sock");
    assert!(aceita_conexao(&socket).await, "controle positivo: o proxy da sessão a aceita conexões");
    let watch = supervisor.watch();
    let (started, release) = (Rc::new(tokio::sync::Notify::new()), Rc::new(tokio::sync::Notify::new()));
    supervisor.launcher_mut().hold = Some((started.clone(), release.clone()));
    let starting = supervisor.ensure(scope("b"), "vinculo-b", now);
    let revoking = async {
        started.notified().await;
        assert_eq!(watch.revoke(|scope, owner| scope.bot == "a" && owner == "vinculo-a"), 1);
        assert_eq!(
            watch.revoke(|_, _| true),
            0,
            "idempotente: a já cortada não conta e b, ainda em arranque, não está registrada"
        );
        release.notify_one();
    };
    let (started_b, ()) = tokio::join!(starting, revoking);
    started_b?;
    assert!(deixa_de_aceitar(&socket).await, "o proxy revogado continuou aceitando conexões");
    assert_eq!(
        *cuts.lock().map_err(|_| anyhow::anyhow!("log"))?,
        std::slice::from_ref(&profile),
        "só a sessão a foi cortada pelo controle"
    );
    assert!(supervisor.contains(&scope("a")), "a limpeza dos processos ainda não rodou");
    assert_eq!(supervisor.reap_dead()?, 1, "a manutenção encerra a sessão cortada");
    assert!(!supervisor.contains(&scope("a")) && !profile.exists());
    assert!(supervisor.contains(&scope("b")));
    drop(supervisor);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn falha_ao_parar_ou_apagar_perfil_nao_mantem_o_proxy_e_a_limpeza_roda_depois() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: geteuid não recebe ponteiros nem altera memória.
    let as_root = unsafe { libc::geteuid() } == 0;
    for failure in ["processo", "perfil"] {
        if failure == "perfil" && as_root {
            continue;
        }
        let root = short_root();
        let fake = Fake::default();
        let events = fake.events.clone();
        let mut supervisor = Supervisor::new(fake, root.clone(), 1, Duration::from_secs(60))?;
        let now = Instant::now();
        supervisor.ensure(scope("a"), "vinculo", now).await?;
        let profile = supervisor.sessions[&scope("a")].profile.clone();
        let socket = profile.join("proxy.sock");
        let locked = profile.join("preso");
        assert!(aceita_conexao(&socket).await, "{failure}: controle positivo, o proxy aceita conexões");
        if failure == "processo" {
            supervisor.launcher_mut().fail_stop = true;
        } else {
            fs::create_dir(&locked)?;
            fs::write(locked.join("arquivo"), b"x")?;
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o500))?;
        }
        let error = format!("{:#}", supervisor.stop(&scope("a")).expect_err("a limpeza falha de propósito"));
        assert!(
            error.contains(if failure == "processo" { "falha simulada ao parar" } else { "falha ao limpar perfil" }),
            "{failure}: diagnóstico perdido: {error}"
        );
        assert!(deixa_de_aceitar(&socket).await, "{failure}: o proxy continuou ativo depois da falha de limpeza");
        assert!(!supervisor.contains(&scope("a")), "{failure}: a sessão falha não pode mais ser servida");
        assert_eq!(supervisor.retired_len(), 1, "{failure}: a limpeza pendente se perdeu");
        assert!(
            events.borrow().iter().any(|event| event.starts_with("stop ")),
            "{failure}: o processo não foi tentado"
        );
        assert!(supervisor.start(scope("b"), now).await.is_err(), "{failure}: a sessão pendente ocupa a vaga");
        if failure == "processo" {
            supervisor.launcher_mut().fail_stop = false;
        } else {
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o700))?;
        }
        assert_eq!(supervisor.retry_retired()?, 1, "{failure}");
        assert_eq!(supervisor.retired_len(), 0);
        assert!(!profile.exists(), "{failure}: o perfil sobrou");
        supervisor.start(scope("b"), now).await?;
        drop(supervisor);
        fs::remove_dir_all(root)?;
    }
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn sessao_sem_canal_de_controle_e_retirada_e_o_processo_morre_sem_nova_chamada() -> Result<()> {
    struct Real {
        dead: Rc<RefCell<bool>>,
    }
    impl Launcher for Real {
        type Process = std::process::Child;
        fn start(&mut self, _profile: &Path, _proxy: &Path) -> Result<std::process::Child> {
            Ok(std::process::Command::new("sleep").arg("300").spawn()?)
        }
        #[allow(clippy::unused_async, clippy::unused_async_trait_impl)]
        async fn authenticate(&mut self, _process: &mut std::process::Child, _token: String) -> Result<()> {
            Ok(())
        }
        fn stop(&mut self, process: &mut std::process::Child) -> Result<()> {
            process.kill()?;
            process.wait()?;
            Ok(())
        }
        fn alive(&self, _process: &std::process::Child) -> bool {
            !*self.dead.borrow()
        }
    }
    let root = root();
    let dead = Rc::new(RefCell::new(false));
    let mut supervisor = Supervisor::new(Real { dead: dead.clone() }, root.clone(), 2, Duration::from_secs(60))?;
    let now = Instant::now();
    supervisor.ensure(scope("a"), "vinculo", now).await?;
    let pid = supervisor.sessions[&scope("a")].process.id();
    let profile = supervisor.sessions[&scope("a")].profile.clone();
    let socket = profile.join("proxy.sock");
    assert_eq!(supervisor.reap_dead()?, 0, "sessão viva não é tocada");
    assert!(supervisor.contains(&scope("a")) && profile.exists());
    let running = |pid: u32| Path::new(&format!("/proc/{pid}")).exists();
    assert!(running(pid));
    *dead.borrow_mut() = true;
    assert_eq!(supervisor.reap_dead()?, 1);
    assert!(!supervisor.contains(&scope("a")), "instância continuou no supervisor");
    assert!(!running(pid), "o processo do navegador continuou vivo");
    assert!(!profile.exists() && !socket.exists(), "sobrou perfil ou proxy");
    drop(supervisor);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[tokio::test]
async fn limpa_restos_e_falha_de_inicio_e_bloqueia_segunda_instancia() -> Result<()> {
    let root = root();
    let supervisor = Supervisor::new(Fake::default(), root.clone(), 1, Duration::from_secs(1))?;
    assert!(Supervisor::new(Fake::default(), root.clone(), 1, Duration::from_secs(1)).is_err());
    fs::create_dir(root.join("session-resto"))?;
    drop(supervisor);
    let mut supervisor =
        Supervisor::new(Fake { fail: true, ..Fake::default() }, root.clone(), 1, Duration::from_secs(1))?;
    assert!(!root.join("session-resto").exists());
    assert!(supervisor.start(scope("a"), Instant::now()).await.is_err());
    assert_eq!(fs::read_dir(&root)?.count(), 1);
    drop(supervisor);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "requer Chromium, Xvfb e sandbox Linux disponível"]
async fn inicia_chromium_real() -> Result<()> {
    let root = root();
    let host_binary = std::env::current_exe()?.parent().and_then(Path::parent).map(|dir| dir.join("donnabot-host"));
    let mut supervisor = Supervisor::new(
        real::RealLauncher {
            chromium: "/usr/bin/chromium".into(),
            xvfb: "/usr/bin/Xvfb".into(),
            host_binary,
            extra_hidden: Vec::new(),
        },
        root.clone(),
        1,
        Duration::from_secs(30),
    )?;
    supervisor.start(scope("a"), Instant::now()).await?;
    supervisor.stop(&scope("a"))?;
    drop(supervisor);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[tokio::test]
async fn expiracao_tenta_todas_as_sessoes_mesmo_com_erro() -> Result<()> {
    let root = root();
    let fake = Fake { fail_stop: true, ..Fake::default() };
    let events = fake.events.clone();
    let now = Instant::now();
    let mut supervisor = Supervisor::new(fake, root.clone(), 2, Duration::from_secs(1))?;
    for bot in ["a", "b"] {
        supervisor.start(scope(bot), now).await?;
    }
    let error = supervisor.reap_idle(now + Duration::from_secs(1)).unwrap_err().to_string();
    assert_eq!(error.matches("falha simulada ao parar").count(), 2);
    assert_eq!(events.borrow().iter().filter(|event| event.starts_with("stop ")).count(), 2);
    supervisor.launcher.fail_stop = false;
    drop(supervisor);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn recusa_diretorio_inseguro() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let root = root();
    fs::create_dir(&root)?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755))?;
    assert!(Supervisor::new(Fake::default(), root.clone(), 1, Duration::from_secs(1)).is_err());
    fs::remove_dir_all(root)?;
    Ok(())
}
