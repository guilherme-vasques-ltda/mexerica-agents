use super::*;
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
struct Fake {
    events: Rc<RefCell<Vec<String>>>,
    fail: bool,
    fail_stop: bool,
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
        self.events.borrow_mut().push("auth".into());
        Ok(())
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
    let mut supervisor = Supervisor::new(
        real::RealLauncher { chromium: "/usr/bin/chromium".into(), xvfb: "/usr/bin/Xvfb".into(), host_binary: None },
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
