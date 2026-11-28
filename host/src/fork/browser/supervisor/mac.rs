use std::{net::SocketAddr, path::Path};

fn arguments(profile: &Path, proxy: SocketAddr) -> Vec<String> {
    [format!("--user-data-dir={}", profile.display()), format!("--proxy-server=http://{proxy}")]
        .into_iter()
        .chain(
            [
                "--headless",
                "--remote-debugging-pipe",
                "--no-first-run",
                "--no-default-browser-check",
                "--use-mock-keychain",
                "--disable-extensions",
                "--deny-permission-prompts",
                "--disable-sync",
                "--disable-background-networking",
                "--disable-component-update",
                "--disable-crash-reporter",
                "--disable-breakpad",
                "--proxy-bypass-list=<-loopback>",
                "--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE 127.0.0.1",
                "--disable-quic",
                "--force-webrtc-ip-handling-policy=disable_non_proxied_udp",
                "--no-pings",
                "about:blank",
            ]
            .map(str::to_owned),
        )
        .collect()
}

#[cfg(target_os = "macos")]
pub(in crate::fork::browser) use launcher::{Processes, RealLauncher, engine};

#[cfg(target_os = "macos")]
mod launcher {
    use super::{Path, SocketAddr, arguments};
    use crate::fork::browser::cdp::Cdp;
    use crate::fork::browser::supervisor::Launcher;
    use anyhow::{Context, Result, bail, ensure};
    use std::{
        os::{
            fd::{AsRawFd, FromRawFd, OwnedFd},
            unix::{fs::PermissionsExt, net::UnixStream, process::CommandExt},
        },
        path::PathBuf,
        process::{Child, Command, Stdio},
        sync::Arc,
    };

    pub(in crate::fork::browser) fn engine() -> Option<PathBuf> {
        [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
        ]
        .map(PathBuf::from)
        .into_iter()
        .find(|path| path.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
    }

    pub(in crate::fork::browser) struct RealLauncher {
        pub chromium: PathBuf,
    }

    pub(in crate::fork::browser) struct Processes {
        child: Option<Child>,
        cdp: Cdp<tokio::net::UnixStream>,
    }

    impl Processes {
        pub(in crate::fork::browser) fn cdp(&self) -> Option<Cdp<tokio::net::UnixStream>> {
            (!self.cdp.is_closed()).then(|| self.cdp.clone())
        }

        fn stop(&mut self) -> Result<()> {
            self.cdp.sever();
            let Some(child) = self.child.as_mut() else { return Ok(()) };
            let pid = i32::try_from(child.id()).context("PID de navegador inválido")?;
            // SAFETY: o grupo de processos é criado pelo launcher para esta instância.
            if unsafe { libc::kill(-pid, libc::SIGKILL) } < 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) {
                    return Err(error.into());
                }
            }
            child.wait()?;
            self.child = None;
            Ok(())
        }
    }

    impl Drop for Processes {
        fn drop(&mut self) {
            let _ = self.stop();
        }
    }

    fn dup_high(fd: &UnixStream) -> std::io::Result<OwnedFd> {
        // SAFETY: o fd está aberto; o retorno não negativo pertence exclusivamente ao chamador.
        let copy = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 10) };
        if copy < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: copy é um descritor novo e ainda sem dono.
        Ok(unsafe { OwnedFd::from_raw_fd(copy) })
    }

    impl Launcher for RealLauncher {
        type Process = Processes;

        fn start(&mut self, _profile: &Path, _proxy_socket: &Path) -> Result<Processes> {
            bail!("navegador local exige proxy TCP autenticado")
        }

        fn start_tcp(&mut self, profile: &Path, proxy: SocketAddr) -> Result<Processes> {
            ensure!(proxy.ip().is_loopback() && proxy.port() != 0, "proxy local inválido");
            let (hub_write, browser_read) = UnixStream::pair()?;
            let (hub_read, browser_write) = UnixStream::pair()?;
            let read_fd = dup_high(&browser_read)?;
            let write_fd = dup_high(&browser_write)?;
            hub_read.set_nonblocking(true)?;
            hub_write.set_nonblocking(true)?;
            let hub_read = tokio::net::UnixStream::from_std(hub_read)?;
            let hub_write = tokio::net::UnixStream::from_std(hub_write)?;
            let mut command = Command::new(&self.chromium);
            command
                .args(arguments(profile, proxy))
                .env_clear()
                .env("HOME", profile)
                .env("TMPDIR", profile)
                .current_dir(profile)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0);
            // SAFETY: dup2 é async-signal-safe e os dois fd de origem não colidem com os destinos.
            unsafe {
                command.pre_exec(move || {
                    if libc::dup2(read_fd.as_raw_fd(), 3) < 0 || libc::dup2(write_fd.as_raw_fd(), 4) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            let child = command.spawn().context("falha ao iniciar o navegador local")?;
            Ok(Processes { child: Some(child), cdp: Cdp::read_only(hub_read, hub_write) })
        }

        async fn authenticate(&mut self, process: &mut Processes, token: String) -> Result<()> {
            process.cdp.provide_proxy_credentials(token).await
        }

        fn stop(&mut self, process: &mut Processes) -> Result<()> {
            process.stop()
        }

        fn alive(&self, process: &Processes) -> bool {
            !process.cdp.is_closed()
        }

        fn cutter(&self, process: &Processes) -> Arc<dyn Fn() + Send + Sync> {
            let cdp = process.cdp.clone();
            Arc::new(move || cdp.sever())
        }

        fn watch_loss(&self, process: &Processes, lost: Arc<dyn Fn() + Send + Sync>) {
            process.cdp.on_close(lost);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chromium_local_usa_perfil_proprio_pipe_e_proxy_local() {
        let profile = Path::new("/fixture/profile");
        let proxy = "127.0.0.1:1234".parse().unwrap();
        let args = arguments(profile, proxy);
        assert!(args.contains(&"--user-data-dir=/fixture/profile".into()));
        assert!(args.contains(&"--headless".into()));
        assert!(args.contains(&"--remote-debugging-pipe".into()));
        assert!(args.contains(&"--proxy-server=http://127.0.0.1:1234".into()));
        assert!(args.contains(&"--proxy-bypass-list=<-loopback>".into()));
        assert!(!args.iter().any(|arg| arg.contains("debugging-port") || arg.contains("no-sandbox")));
        assert_eq!(args.last().map(String::as_str), Some("about:blank"));
    }
}
