use crate::{Cli, Sub, environment::Environment};
use anyhow::Result;
use clap::{Parser, Subcommand};
use serde_json::Value;

pub(crate) const SOCKET_PEER: std::net::SocketAddr =
    std::net::SocketAddr::V4(std::net::SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, 0));

#[derive(Parser)]
#[command(name = "mexerica-open-hub", version)]
struct OpenCli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Serve {
        #[arg(long)]
        port: u16,
        #[arg(long)]
        bind: String,
        #[arg(long)]
        only_this_mac: bool,
    },
}

pub(crate) fn cli() -> Cli {
    let Command::Serve { port, bind, only_this_mac } = OpenCli::parse().command;
    Cli { cmd: Some(Sub::Serve { port, bind: Some(bind), only_this_mac, web_dir: None }) }
}

pub(crate) fn require_executor() -> std::io::Result<()> {
    if Environment::is_open() {
        return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "Mexerica Open has no isolated executor"));
    }
    Ok(())
}

pub(crate) fn check_request(method: &str, body: &Value) -> Result<()> {
    if !Environment::is_open() || open_allows(method, body) {
        return Ok(());
    }
    Err(crate::api::devices::Forbidden.into())
}

/// Lista fechada do que a Open aceita: todo método fora dela, inclusive os de conexões de IA, é recusado.
pub(crate) fn open_allows(method: &str, body: &Value) -> bool {
    match method {
        "hello"
        | crate::fork::workspace::METHOD
        | "refreshBackends"
        | "teamSettings"
        | "usage"
        | "history"
        | "markRead"
        | "donnaSpaces"
        | "donnaSpaceCreate"
        | "donnaSpaceUpdate"
        | "donnaSpaceReorder"
        | "donnaOwnerPresentation"
        | "donnaEngineAccounts"
        | "donnaEngineAccess"
        | "donnaOnboardingStatus"
        | "donnaOnboardingDetect"
        | "donnaBotEditor"
        | "donnaBotEditorSave"
        | "donnaConversations"
        | "donnaConversationHistory"
        | "donnaConversationRead"
        | "stop"
        | "deleteBot"
        | "reorderBots" => true,
        "createBot" | "updateBot" => body.get("cwd").is_none_or(|cwd| cwd.as_str() == Some("")),
        // Só texto, na conversa principal do bot: sem anexos e sem sessão extra.
        "send" => ["attachments", "sessionId"].iter().all(|key| body.get(key).is_none_or(Value::is_null)),
        _ if crate::fork::engine_setup::open_login_method(method) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::open_allows;
    use serde_json::json;

    #[test]
    fn open_recusa_os_metodos_de_planos_de_codigo_e_de_chaves() {
        for method in crate::fork::planos_codigo::METHODS.into_iter().chain([
            "engineGoKeySave",
            "setAgentEnv",
            "agentAuthenticate",
        ]) {
            assert!(!open_allows(method, &json!({})), "{method} não pode existir na Open");
        }
        assert!(open_allows("hello", &json!({})));
        assert!(open_allows("donnaOnboardingStatus", &json!({})));
        assert!(open_allows("donnaOnboardingDetect", &json!({})));
        assert!(!open_allows("donnaOnboardingSave", &json!({})));
        assert!(open_allows("createBot", &json!({})));
        assert!(!open_allows("createBot", &json!({"cwd": "/tmp"})));
        assert!(open_allows("send", &json!({"text": "oi"})));
        assert!(open_allows("send", &json!({"attachments": null, "sessionId": null})));
        assert!(!open_allows("send", &json!({"attachments": ["arquivo"]})));
        assert!(!open_allows("send", &json!({"sessionId": "extra"})));
    }
}

#[cfg(unix)]
pub(crate) use unix::serve;

#[cfg(unix)]
mod unix {
    use super::{Environment, Result, SOCKET_PEER};
    use crate::fork::private_fs::{self, Directory};
    use crate::hub::Hub;
    use crate::service::mac_local::LOOPBACK;
    use anyhow::{Context, ensure};
    use axum::{Extension, extract::ConnectInfo};
    use std::fs;
    use std::future::IntoFuture;
    use std::io;
    use std::net::Ipv4Addr;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use tokio::net::{TcpListener, UnixListener};
    use tokio::sync::oneshot;

    const SYSTEM_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";
    const MARKER: &str = ".open-profile-v1";
    const PROFILE_FORMAT: &[u8] = b"mexerica-open-v1\n";

    struct Profile {
        data: PathBuf,
        socket: PathBuf,
    }

    impl Profile {
        fn from_environment() -> Result<Self> {
            for name in ["MEXERICA_HOME", "DONNABOT_HOME", "CODYNC_HOME"] {
                ensure!(std::env::var_os(name).is_none(), "Mexerica Open does not accept data overrides");
            }
            ensure!(std::env::var_os("PATH").is_some_and(|path| path == SYSTEM_PATH), "invalid Mexerica Open PATH");
            let path = |key| std::env::var_os(key).map(PathBuf::from).with_context(|| format!("{key} is required"));
            let home = path("HOME")?;
            let tmp = path("TMPDIR")?;
            let socket = path("OPEN_HUB_SOCKET")?;
            let cwd = std::env::current_dir()?;
            Self::validate(&home, &tmp, &socket, &cwd)
        }

        fn validate(home: &Path, tmp: &Path, socket: &Path, cwd: &Path) -> Result<Self> {
            let root = home.parent().context("missing Mexerica Open profile")?;
            ensure!(
                root.file_name().is_some_and(|name| name == Environment::Open.data_folder())
                    && home == root.join("home")
                    && cwd == home
                    && tmp == root.join("tmp"),
                "invalid Mexerica Open profile layout"
            );
            let session = session_directory(socket, tmp)?;
            for dir in [root, home, tmp, session] {
                private_directory(dir)?;
            }
            match fs::symlink_metadata(socket) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => (),
                Err(error) => return Err(error.into()),
                Ok(_) => anyhow::bail!("Mexerica Open socket already exists"),
            }
            Ok(Self { data: home.join(Environment::Open.data_folder()), socket: socket.to_path_buf() })
        }

        fn reserve_data(&self) -> Result<fs::File> {
            match fs::symlink_metadata(&self.data) {
                Ok(_) => private_directory(&self.data)?,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    Directory::open(&self.data, true)?;
                }
                Err(error) => return Err(error.into()),
            }
            validate_data(&self.data)?;
            let lock = crate::service::lock_host_at(&self.data.join("host.lock"))?;
            let marker = self.data.join(MARKER);
            if let Some(contents) = private_fs::read(&marker)? {
                ensure!(contents == PROFILE_FORMAT, "invalid Mexerica Open data marker");
            } else {
                for entry in fs::read_dir(&self.data)? {
                    ensure!(entry?.file_name() == "host.lock", "refusing data from another Mexerica installation");
                }
                private_fs::create_atomic(&marker, PROFILE_FORMAT)?;
            }
            Ok(lock)
        }

        fn open_store(&self) -> Result<crate::store::Store> {
            use crate::store::{Store, personal_database};
            let path = self.data.join("codync.db");
            if let Some(database) = personal_database::capture(&path)? {
                ensure!(!database.evidence().personal, "personal database unavailable in Mexerica Open");
                return Store::open_personal(database);
            }
            Store::open_created(personal_database::create_exclusive(&path)?)
        }
    }

    fn session_directory<'a>(socket: &'a Path, tmp: &Path) -> Result<&'a Path> {
        let session = socket.parent().context("missing Mexerica Open session directory")?;
        ensure!(
            socket.as_os_str() == session.join("s").as_os_str()
                && socket.as_os_str().as_bytes().len() <= 103
                && session.parent() == Some(tmp)
                && session.file_name().is_some_and(|name| name.as_bytes().starts_with(b"h-")),
            "invalid Mexerica Open socket layout"
        );
        Ok(session)
    }

    fn private_directory(path: &Path) -> Result<()> {
        ensure!(path.is_absolute(), "relative Mexerica Open directory");
        let metadata = Directory::open(path, false)?.0.metadata()?;
        ensure!(path.canonicalize()?.as_os_str() == path, "noncanonical Mexerica Open directory");
        ensure!(metadata.mode() & 0o7777 == 0o700, "Mexerica Open directories require mode 0700");
        Ok(())
    }

    /// Pasta de ajudantes que o Codex recria a cada início em `.codex/tmp`; fica com links quando ele é morto à força.
    fn codex_helpers(path: &Path) -> bool {
        fn name(path: Option<&Path>) -> &str {
            path.and_then(Path::file_name).and_then(|name| name.to_str()).unwrap_or_default()
        }
        let folder = name(Some(path));
        (folder == "arg0" || folder.strip_prefix("arg0-").is_some_and(|os| os.bytes().all(|b| b.is_ascii_lowercase())))
            && name(path.parent()) == "tmp"
            && name(path.parent().and_then(Path::parent)) == ".codex"
    }

    fn validate_data(root: &Path) -> Result<()> {
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let kind = entry.file_type()?;
                let metadata = entry.metadata()?;
                ensure!(metadata.uid() == private_fs::uid(), "wrong owner in Mexerica Open data");
                if kind.is_dir() && codex_helpers(&entry.path()) {
                    // O Codex recria a pasta a cada início e a limpa sozinha; deixada com links por um encerramento
                    // forçado, ela é inofensiva e a abertura só a tolera, sem apagar nada.
                } else if kind.is_dir() {
                    ensure!(metadata.mode() & 0o022 == 0, "writable directory in Mexerica Open data");
                    pending.push(entry.path());
                } else {
                    ensure!(
                        kind.is_file() && metadata.nlink() == 1 && metadata.mode() & 0o022 == 0,
                        "unsafe file in Mexerica Open data"
                    );
                }
            }
        }
        Ok(())
    }

    #[cfg(test)]
    #[test]
    fn startup_tolerates_codex_helper_links_and_still_refuses_any_other_link() {
        use std::os::unix::fs::{DirBuilderExt, symlink};
        let root = std::env::temp_dir().join(format!("open-data-{}", uuid::Uuid::new_v4()));
        let tmp = root.join("engine-setup/accounts/a1/h1/.codex/tmp");
        let helpers = tmp.join("arg0-macos/codex-sessao");
        fs::DirBuilder::new().recursive(true).mode(0o700).create(&helpers).unwrap();
        symlink("/usr/bin/true", helpers.join("apply_patch")).unwrap();
        validate_data(&root).unwrap();
        assert!(helpers.join("apply_patch").exists(), "a abertura só tolera a pasta de ajudantes e não apaga nada");
        symlink("/usr/bin/true", tmp.join("apply_patch")).unwrap();
        assert!(validate_data(&root).is_err(), "link fora da pasta de ajudantes continua recusado");
        let _ = fs::remove_dir_all(&root);
    }

    struct SocketFile {
        path: PathBuf,
        dev: u64,
        ino: u64,
    }

    impl SocketFile {
        fn bind(path: PathBuf) -> Result<(UnixListener, Self)> {
            let listener = UnixListener::bind(&path).context("binding the private Mexerica Open socket")?;
            let metadata = fs::symlink_metadata(&path)?;
            ensure!(
                metadata.file_type().is_socket() && metadata.uid() == private_fs::uid(),
                "unexpected socket identity"
            );
            let file = Self { path, dev: metadata.dev(), ino: metadata.ino() };
            fs::set_permissions(&file.path, fs::Permissions::from_mode(0o600))?;
            Ok((listener, file))
        }
    }

    impl Drop for SocketFile {
        fn drop(&mut self) {
            if fs::symlink_metadata(&self.path)
                .is_ok_and(|m| m.file_type().is_socket() && m.dev() == self.dev && m.ino() == self.ino)
            {
                let _ = fs::remove_file(&self.path);
            }
        }
    }

    fn watch_stdin() -> Result<oneshot::Receiver<()>> {
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: fstat só escreve na estrutura válida; o descritor é stdin da invocação.
        let result = unsafe { libc::fstat(libc::STDIN_FILENO, stat.as_mut_ptr()) };
        ensure!(result == 0, "Mexerica Open requires piped stdin");
        // SAFETY: fstat retornou sucesso.
        let kind = unsafe { stat.assume_init() }.st_mode & libc::S_IFMT;
        ensure!(matches!(kind, libc::S_IFIFO | libc::S_IFSOCK), "Mexerica Open requires piped stdin");
        let (tx, rx) = oneshot::channel();
        std::thread::Builder::new().name("open-stdin".into()).spawn(move || {
            let _ = io::copy(&mut io::stdin().lock(), &mut io::sink());
            let _ = tx.send(());
        })?;
        Ok(rx)
    }

    pub(crate) async fn serve(port: u16, bind: Option<&str>, only_this_mac: bool) -> Result<()> {
        ensure!(
            only_this_mac && bind == Some(LOOPBACK) && port == Environment::Open.port(),
            "Mexerica Open requires its fixed loopback configuration"
        );
        let mut eof = watch_stdin()?;
        let profile = Profile::from_environment()?;
        // SAFETY: processo Open próprio, antes de criar qualquer arquivo ou filho.
        unsafe { libc::umask(0o077) };
        let mut stop = std::pin::pin!(async {
            tokio::select! {
                _ = &mut eof => (),
                () = crate::shutdown_signal() => (),
            }
        });
        let tcp = tokio::select! {
            biased;
            () = &mut stop => return Ok(()),
            result = TcpListener::bind((Ipv4Addr::LOCALHOST, port)) => result.context("binding the dedicated Mexerica Open port")?,
        };
        let (unix, socket_file) = SocketFile::bind(profile.socket.clone())?;
        let _lock = profile.reserve_data()?;
        crate::configure_logging();
        let hub = tokio::select! {
            biased;
            () = &mut stop => return Ok(()),
            result = initialize(&profile, port) => result?,
        };
        let socket_router = crate::api::open_socket_router(hub.clone()).layer(Extension(ConnectInfo(SOCKET_PEER)));
        let result = tokio::select! {
            biased;
            () = &mut stop => Ok(()),
            result = axum::serve(unix, socket_router).into_future() => result,
            result = axum::serve(tcp, axum::Router::new()).into_future() => result,
        };
        crate::fork::engine_setup::shutdown_open(&hub).await;
        crate::fork::open_chat::shutdown(&hub).await;
        hub.shutdown().await;
        crate::agent::acp::stopped().await;
        drop(socket_file);
        result?;
        std::process::exit(0)
    }

    async fn initialize(profile: &Profile, port: u16) -> Result<Arc<Hub>> {
        tokio::task::spawn_blocking(crate::service::capture_binary_identity).await??;
        crate::agent::harness::install_docs().await;
        crate::chat::context::load_harness_context().await;
        let store = profile.open_store()?;
        let host_id = crate::store_host_id(&store)?;
        crate::chat::manage::recover(&store);
        crate::fork::open_chat_fila::recover(&store);
        let identity = crate::remote::identity::Identity::load_or_create(&profile.data)?;
        let token = crate::new_token();
        let hub = Hub::new(store, host_id, identity, token, port);
        hub.start()?;
        crate::write_token_file(&hub.token)?;
        Ok(hub)
    }
}
