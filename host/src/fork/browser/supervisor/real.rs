use super::Launcher;
use anyhow::{Context, Result, anyhow, bail, ensure};
use std::{
    io::{BufRead, BufReader},
    net::Shutdown,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{net::UnixStream, process::CommandExt},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

pub(super) struct RealLauncher {
    pub chromium: PathBuf,
    pub xvfb: PathBuf,
    /// Binário com o modo `__browser-bridge`; em testes é o binário real, não a bancada.
    pub host_binary: Option<PathBuf>,
}

pub(super) struct Processes {
    chromium: Child,
    xvfb: Child,
    cdp: Option<super::super::cdp::Cdp<tokio::net::UnixStream>>,
}

impl Processes {
    pub(super) fn cdp(&self) -> Option<super::super::cdp::Cdp<tokio::net::UnixStream>> {
        self.cdp.clone()
    }
}

fn high_fd(fd: i32) -> Result<OwnedFd> {
    // SAFETY: fcntl duplica um descritor válido; o novo descritor pertence a OwnedFd.
    let copy = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 10) };
    if copy < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: copy é um descritor novo e válido, ainda sem dono Rust.
    Ok(unsafe { OwnedFd::from_raw_fd(copy) })
}

fn parent_death(parent: u32) -> std::io::Result<()> {
    // SAFETY: prctl e getppid não recebem ponteiros; mata o filho se o Hub cair.
    if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: getppid não recebe ponteiros nem altera memória.
    if u32::try_from(unsafe { libc::getppid() }).ok() != Some(parent) {
        return Err(std::io::Error::other("Hub terminou durante a inicialização"));
    }
    Ok(())
}

fn stop_child(child: &mut Child) -> Result<()> {
    if child.try_wait()?.is_none() {
        let pid = i32::try_from(child.id()).context("PID inválido")?;
        // SAFETY: kill só recebe o PID de grupo criado por este processo.
        let result = unsafe { libc::kill(-pid, libc::SIGKILL) };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error.into());
            }
        }
        child.wait()?;
    }
    Ok(())
}

fn child_environment(command: &mut Command, profile: &Path) {
    command.env_clear();
    if let Some(path) = std::env::var_os("PATH") {
        command.env("PATH", path);
    }
    command
        .env("HOME", profile)
        .env("XDG_CONFIG_HOME", profile.join("config"))
        .env("XDG_CACHE_HOME", profile.join("cache"))
        .env("XDG_DATA_HOME", profile.join("data"))
        .env("LANG", "C.UTF-8");
}

fn network_flags() -> Vec<String> {
    [
        "--proxy-bypass-list=<-loopback>",
        "--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE 127.0.0.1",
        "--disable-quic",
        "--force-webrtc-ip-handling-policy=disable_non_proxied_udp",
        "--disable-features=AsyncDns",
        "--no-pings",
        "--disable-background-networking",
        "--disable-sync",
        "--disable-component-update",
        "--disable-client-side-phishing-detection",
        "--safebrowsing-disable-auto-update",
    ]
    .into_iter()
    .map(ToString::to_string)
    .collect()
}

fn chromium_args(chromium: &Path, profile: &Path) -> Vec<String> {
    let mut args = vec![
        chromium.display().to_string(),
        format!("--user-data-dir={}", profile.display()),
        "--remote-debugging-pipe".into(),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
    ];
    args.extend(network_flags());
    args.push("about:blank".into());
    args
}

impl Launcher for RealLauncher {
    type Process = Processes;

    fn start(&mut self, profile: &Path, proxy_socket: &Path) -> Result<Processes> {
        // SAFETY: geteuid não recebe ponteiros nem altera memória.
        ensure!(unsafe { libc::geteuid() } != 0, "Chromium indisponível: sandbox não funciona como root");
        super::super::bridge::ensure_namespaces_available()?;
        ensure!(proxy_socket.starts_with(profile), "socket do proxy fora do perfil da sessão");
        let parent = std::process::id();
        let mut xvfb_command = Command::new(&self.xvfb);
        child_environment(&mut xvfb_command, profile);
        xvfb_command
            .args(["-displayfd", "1", "-screen", "0", "1280x720x24", "-nolisten", "tcp"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0);
        // SAFETY: pre_exec usa somente syscalls seguras após fork.
        unsafe {
            xvfb_command.pre_exec(move || parent_death(parent));
        }
        let mut xvfb = xvfb_command.spawn().context("Xvfb ausente ou impossível de iniciar")?;
        let result = (|| -> Result<(Child, super::super::cdp::Cdp<tokio::net::UnixStream>)> {
            let stdout = xvfb.stdout.take().context("saída do Xvfb ausente")?;
            let mut poll = libc::pollfd { fd: stdout.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            // SAFETY: poll aponta para um único pollfd válido durante a chamada.
            ensure!(unsafe { libc::poll(&raw mut poll, 1, 5000) } > 0, "Xvfb não ficou pronto");
            let mut display = String::new();
            BufReader::new(stdout).read_line(&mut display)?;
            let display: u32 = display.trim().parse().context("Xvfb não informou display válido")?;
            let (hub_write, browser_read) = UnixStream::pair()?;
            let (hub_read, browser_write) = UnixStream::pair()?;
            hub_write.shutdown(Shutdown::Read)?;
            hub_read.shutdown(Shutdown::Write)?;
            let read_fd = high_fd(browser_read.as_raw_fd())?;
            let write_fd = high_fd(browser_write.as_raw_fd())?;
            // SAFETY: geteuid/getegid não recebem ponteiros.
            let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
            let bridge_binary = match &self.host_binary {
                Some(path) => path.clone(),
                None => std::env::current_exe()?,
            };
            let mut hide_paths: Vec<PathBuf> = Vec::new();
            if let Some(home) = std::env::var_os("HOME") {
                hide_paths.push(PathBuf::from(home));
            }
            // Diretório de dados real do Hub (MEXERICA_HOME ou padrão), não um
            // palpite a partir do perfil; e os caminhos fixos de serviço.
            hide_paths.push(crate::service::data_dir());
            if let Some(root) = profile.parent() {
                hide_paths.push(root.to_path_buf());
            }
            hide_paths.push(PathBuf::from("/var/lib/donnabot"));
            hide_paths.push(PathBuf::from("/etc/donnabot"));
            hide_paths.push(PathBuf::from(format!("/run/user/{uid}")));
            hide_paths.push(PathBuf::from("/tmp"));
            let script = super::super::bridge::launch_script(
                &bridge_binary,
                profile,
                &chromium_args(&self.chromium, profile),
                uid,
                gid,
                &hide_paths,
                display,
            )?;
            let script_path = profile.join("launch.sh");
            std::fs::write(&script_path, script)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o700))?;
            }
            let mut command = Command::new("unshare");
            child_environment(&mut command, profile);
            command
                .args(["-Urmn", "--map-root-user", "/bin/sh"])
                .arg(&script_path)
                .env("DISPLAY", format!(":{display}"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0);
            // SAFETY: após fork, apenas syscalls async-signal-safe antes do exec.
            unsafe {
                command.pre_exec(move || {
                    parent_death(parent)?;
                    if libc::dup2(read_fd.as_raw_fd(), 3) < 0 || libc::dup2(write_fd.as_raw_fd(), 4) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            let mut chromium =
                command.spawn().context("contenção indisponível: falha ao iniciar namespaces do navegador")?;
            thread::sleep(Duration::from_millis(500));
            if let Some(status) = chromium.try_wait()? {
                bail!(
                    "Chromium terminou ao iniciar ({status}); confira se o sandbox e os namespaces estão disponíveis"
                );
            }
            hub_write.set_nonblocking(true)?;
            hub_read.set_nonblocking(true)?;
            let tokio_write = tokio::net::UnixStream::from_std(hub_write)?;
            let tokio_read = tokio::net::UnixStream::from_std(hub_read)?;
            let cdp = super::super::cdp::Cdp::new(tokio_read, tokio_write);
            Ok((chromium, cdp))
        })();
        match result {
            Ok((chromium, cdp)) => Ok(Processes { chromium, xvfb, cdp: Some(cdp) }),
            Err(error) => {
                stop_child(&mut xvfb).map_err(|cleanup| anyhow!("{error:#}; falha ao encerrar Xvfb: {cleanup:#}"))?;
                Err(error)
            }
        }
    }

    async fn authenticate(&mut self, process: &mut Processes, token: String) -> Result<()> {
        let cdp = process.cdp.clone().context("canal CDP ausente")?;
        cdp.provide_proxy_credentials(token).await
    }

    fn stop(&mut self, processes: &mut Processes) -> Result<()> {
        processes.cdp.take();
        let chromium = stop_child(&mut processes.chromium);
        let xvfb = stop_child(&mut processes.xvfb);
        chromium.and(xvfb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filho_nao_herda_segredo() -> Result<()> {
        const SECRET: &str = "MEX_TEST_SECRET";
        if std::env::var_os(SECRET).is_none() {
            let status = Command::new(std::env::current_exe()?)
                .args(["--exact", "fork::browser::supervisor::real::tests::filho_nao_herda_segredo"])
                .env(SECRET, "segredo-simulado")
                .status()?;
            assert!(status.success());
        } else {
            let mut command = Command::new("/bin/sh");
            child_environment(&mut command, Path::new("/tmp"));
            let status = command.args(["-c", "test -z \"${MEX_TEST_SECRET+x}\" && test \"$HOME\" = /tmp"]).status()?;
            assert!(status.success());
        }
        Ok(())
    }

    fn ns_of(pid: u32, kind: &str) -> String {
        std::fs::read_link(format!("/proc/{pid}/ns/{kind}")).map(|path| path.display().to_string()).unwrap_or_default()
    }

    fn session_pids(profile: &Path) -> Vec<u32> {
        let needle = profile.to_string_lossy().to_string();
        let mut pids = Vec::new();
        for entry in std::fs::read_dir("/proc").unwrap() {
            let Ok(entry) = entry else { continue };
            let Ok(name) = entry.file_name().into_string() else { continue };
            let Ok(pid) = name.parse::<u32>() else { continue };
            if let Ok(cmdline) = std::fs::read(entry.path().join("cmdline"))
                && String::from_utf8_lossy(&cmdline).contains(&needle)
            {
                pids.push(pid);
            }
        }
        pids
    }

    fn chromium_main_pid(profile: &Path) -> Result<u32> {
        session_pids(profile)
            .into_iter()
            .find(|pid| {
                // O wrapper (unshare/sh) também tem --user-data-dir no cmdline;
                // o principal de verdade é quem executa o binário chromium.
                let exe_ok = std::fs::read_link(format!("/proc/{pid}/exe")).is_ok_and(|exe| {
                    let name = exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    name == "chromium" || name == "chrome" || name.starts_with("chromium")
                });
                exe_ok
                    && std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|cmdline| {
                        let text = String::from_utf8_lossy(&cmdline);
                        text.contains("--user-data-dir=") && !text.contains("--type=")
                    })
            })
            .context("processo principal do Chromium não encontrado")
    }

    fn assert_no_sandbox_disabled(profile: &Path) -> Result<()> {
        let pids = session_pids(profile);
        assert!(!pids.is_empty(), "nenhum processo da sessão");
        let mut renderer_ok = false;
        let main = chromium_main_pid(profile)?;
        for pid in &pids {
            let cmdline = String::from_utf8_lossy(&std::fs::read(format!("/proc/{pid}/cmdline"))?).into_owned();
            assert!(!cmdline.contains("--no-sandbox"), "--no-sandbox presente");
            if cmdline.contains("--type=renderer")
                && (ns_of(*pid, "user") != ns_of(main, "user") || ns_of(*pid, "pid") != ns_of(main, "pid"))
            {
                renderer_ok = true;
            }
        }
        assert!(renderer_ok, "renderer sem namespace próprio");
        Ok(())
    }

    fn bridge_pid(profile: &Path) -> Result<u32> {
        session_pids(profile)
            .into_iter()
            .find(|pid| {
                std::fs::read(format!("/proc/{pid}/cmdline"))
                    .is_ok_and(|cmdline| String::from_utf8_lossy(&cmdline).contains("__browser-bridge"))
            })
            .context("ponte não encontrada")
    }

    fn direct_probes_fail(profile: &Path, server_addr: std::net::SocketAddr) -> Result<()> {
        let pid = bridge_pid(profile)?;
        for probe in [
            format!("timeout 3 bash -c 'cat </dev/null >/dev/tcp/{}/{}'", server_addr.ip(), server_addr.port()),
            "timeout 3 bash -c 'cat </dev/null >/dev/tcp/192.0.2.1/80'".to_string(),
            "timeout 3 bash -c 'echo x >/dev/udp/192.0.2.1/53'".to_string(),
            "timeout 3 bash -c 'echo x >/dev/udp/239.255.255.250/1900'".to_string(),
        ] {
            let status = Command::new("nsenter")
                .args(["-t", &pid.to_string(), "-U", "-n", "--preserve-credentials", "bash", "-lc", &probe])
                .status()?;
            assert!(!status.success(), "sonda direta teve sucesso: {probe}");
        }
        Ok(())
    }

    fn connect_via_bridge_counts(profile: &Path, token: &str, server_addr: std::net::SocketAddr) -> Result<()> {
        let pid = bridge_pid(profile)?;
        let port = std::fs::read_to_string(profile.join("bridge.port"))?;
        let script = format!(
            "import socket,sys,base64; token=sys.stdin.read().strip(); s=socket.create_connection(('127.0.0.1',{port})); s.sendall(b'CONNECT {server} HTTP/1.1\\r\\nProxy-Authorization: Basic ' + base64.b64encode(('mx:'+token).encode()) + b'\\r\\n\\r\\n'); print(s.recv(200).decode(errors='ignore')); s.close()",
            port = port.trim(),
            server = server_addr
        );
        let mut child = Command::new("nsenter")
            .args(["-t", &pid.to_string(), "-U", "-n", "--preserve-credentials", "python3", "-c", &script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        {
            use std::io::Write;
            child.stdin.take().context("stdin da sonda")?.write_all(token.as_bytes())?;
        }
        let output = child.wait_with_output()?;
        // CONNECT autenticado ao destino de teste libera o túnel (o TLS nem precisa fechar).
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains("200"), "CONNECT pela ponte falhou: {text}");
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requer Chromium, Xvfb, namespaces e Instabox Linux"]
    async fn g1_ponte_token_e_controle_positivo() -> Result<()> {
        use std::{
            collections::HashSet,
            net::Ipv4Addr,
            sync::{
                Arc,
                atomic::{AtomicUsize, Ordering},
            },
        };
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let server_addr = listener.local_addr()?;
        let hits = Arc::new(AtomicUsize::new(0));
        let server_hits = Arc::clone(&hits);
        let server = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                server_hits.fetch_add(1, Ordering::SeqCst);
                let mut request = [0u8; 2048];
                let _ = stream.read(&mut request).await;
                let _ = stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\nCache-Control: no-store\r\nConnection: close\r\n\r\nMEX_G1_OK")
                    .await;
            }
        });
        let profile = std::env::temp_dir().join(format!("mex-g1-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&profile)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o700))?;
        }
        let socket = profile.join("proxy.sock");
        let mut allowed = HashSet::new();
        allowed.insert(server_addr);
        let mut proxy = super::super::super::net_proxy::Proxy::start_unix_test(&socket, allowed).await?;
        let token = proxy.token().to_owned();
        let bridge_binary = std::env::current_exe()?
            .parent()
            .and_then(Path::parent)
            .map(|dir| dir.join("donnabot-host"))
            .context("binário do Hub não encontrado para a ponte")?;
        ensure!(bridge_binary.exists(), "binário do Hub ausente: {}", bridge_binary.display());
        let mut launcher = RealLauncher {
            chromium: "/usr/bin/chromium".into(),
            xvfb: "/usr/bin/Xvfb".into(),
            host_binary: Some(bridge_binary),
        };
        let mut processes = launcher.start(&profile, &socket)?;
        let cdp_pre = processes.cdp().context("CDP ausente")?;
        let session = cdp_pre.page_session().await?;
        cdp_pre.command_session(&session, "Page.enable", serde_json::json!({}), Duration::from_secs(5)).await?;
        let url_pre = format!("https://127.0.0.1:{}/sem-token", server_addr.port());
        let _ = cdp_pre
            .command_session_raw_for_tests(
                &session,
                "Page.navigate",
                serde_json::json!({"url": url_pre}),
                Duration::from_secs(5),
            )
            .await;
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert_eq!(hits.load(Ordering::SeqCst), 0, "sem token a origem foi alcançada");
        assert!(proxy.auth_failures() > 0, "proxy não contou desafio 407 antes do token");
        // Contenção pelo kernel: namespaces e interfaces do Chromium diferem dos do Hub.
        let main_pid = chromium_main_pid(&profile)?;
        assert_ne!(ns_of(main_pid, "net"), ns_of(std::process::id(), "net"), "netns igual ao do Hub");
        assert_ne!(ns_of(main_pid, "user"), ns_of(std::process::id(), "user"), "userns igual ao do Hub");
        let dev = std::fs::read_to_string(format!("/proc/{main_pid}/net/dev"))?;
        let ifaces: Vec<&str> = dev.lines().skip(2).filter_map(|line| line.split(':').next()).map(str::trim).collect();
        assert_eq!(ifaces, ["lo"], "interfaces fora do esperado: {ifaces:?}");
        let route = std::fs::read_to_string(format!("/proc/{main_pid}/net/route"))?;
        assert!(route.lines().count() <= 1, "rota presente no netns");
        assert_no_sandbox_disabled(&profile)?;
        let ponte = bridge_pid(&profile)?;
        // Os fds 3/4 da ponte podem ser sockets/epoll próprios: o vazamento seria
        // apontar para os mesmos inodes do canal CDP do Chromium. A produção usa
        // socketpair (socket:[...]); pipe:[...] vale para a sondagem manual.
        let pipes_cdp: Vec<String> = [3, 4]
            .iter()
            .filter_map(|fd| std::fs::read_link(format!("/proc/{main_pid}/fd/{fd}")).ok())
            .map(|alvo| alvo.display().to_string())
            .filter(|alvo| alvo.starts_with("pipe:") || alvo.starts_with("socket:"))
            .collect();
        assert_eq!(pipes_cdp.len(), 2, "fds 3/4 do Chromium não são o pipe CDP: {pipes_cdp:?}");
        for entrada in std::fs::read_dir(format!("/proc/{ponte}/fd"))? {
            let entrada = entrada?;
            if let Ok(alvo) = std::fs::read_link(entrada.path()) {
                assert!(
                    !pipes_cdp.contains(&alvo.display().to_string()),
                    "ponte herdou fd do pipe CDP: {} -> {alvo:?}",
                    entrada.path().display()
                );
            }
        }
        direct_probes_fail(&profile, server_addr)?;
        launcher.authenticate(&mut processes, token.clone()).await?;
        // O token não pode aparecer na linha de comando de nenhum processo da sessão.
        for entry in std::fs::read_dir("/proc")? {
            let Ok(entry) = entry else { continue };
            let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else { continue };
            if cmdline.windows(profile.to_string_lossy().len()).any(|w| w == profile.to_string_lossy().as_bytes()) {
                assert!(!String::from_utf8_lossy(&cmdline).contains(&token), "token vazou para argv");
            }
        }
        let cdp = processes.cdp().context("CDP ausente")?;
        cdp.command_session(&session, "Page.enable", serde_json::json!({}), Duration::from_secs(5)).await?;
        let url = format!("https://127.0.0.1:{}/positivo", server_addr.port());
        cdp.command_session_raw_for_tests(
            &session,
            "Page.navigate",
            serde_json::json!({"url": url}),
            Duration::from_secs(5),
        )
        .await?;
        let mut reached = false;
        for _ in 0..100 {
            if hits.load(Ordering::SeqCst) > 0 {
                reached = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(reached, "destino de teste não foi alcançado pela ponte e pelo proxy autenticado");
        let hits_connect_antes = hits.load(Ordering::SeqCst);
        connect_via_bridge_counts(&profile, &token, server_addr)?;
        assert!(hits.load(Ordering::SeqCst) > hits_connect_antes, "CONNECT pela ponte não tocou o destino de teste");
        let before = hits.load(Ordering::SeqCst);
        proxy.stop().await;
        let url2 = format!("https://127.0.0.1:{}/sem-proxy", server_addr.port());
        let _ = cdp
            .command_session_raw_for_tests(
                &session,
                "Page.navigate",
                serde_json::json!({"url": url2}),
                Duration::from_secs(5),
            )
            .await;
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert_eq!(hits.load(Ordering::SeqCst), before, "navegação continuou sem o proxy");
        launcher.stop(&mut processes)?;
        server.abort();
        std::fs::remove_dir_all(profile)?;
        Ok(())
    }

    /// Lê o texto da página após navegar cru (sem gate, sem Fetch): mede só
    /// a camada de montagem/PID. Espera até `marker` aparecer ou esgotar.
    async fn texto_apos_navegar(
        cdp: &super::super::super::cdp::Cdp<tokio::net::UnixStream>,
        session: &str,
        url: &str,
        marker: &str,
        espera: Duration,
    ) -> Result<String> {
        let _ = cdp
            .command_session_raw_for_tests(
                session,
                "Page.navigate",
                serde_json::json!({"url": url}),
                Duration::from_secs(5),
            )
            .await;
        let inicio = std::time::Instant::now();
        let mut text = String::new();
        while inicio.elapsed() < espera {
            if let Ok(evaluated) = cdp
                .command_session(
                    session,
                    "Runtime.evaluate",
                    serde_json::json!({"expression": "document.body ? document.body.innerText : ''", "returnByValue": true}),
                    Duration::from_secs(5),
                )
                .await
            {
                text = evaluated
                    .pointer("/result/value")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                if text.contains(marker) {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Ok(text)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requer Chromium, Xvfb, namespaces e Instabox Linux"]
    async fn g1_negativo_file_nao_le_arquivos_do_hub() -> Result<()> {
        use std::collections::HashSet;
        let profile = std::env::temp_dir().join(format!("mex-g1-neg-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&profile)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o700))?;
        }
        // Controle positivo: arquivo VISÍVEL dentro do perfil da sessão.
        std::fs::write(profile.join("visivel.html"), "MEX_VISIVEL_OK")?;
        // Canários escondidos: irmão do perfil (sob o pai escondido) e /dev/shm.
        let canary_path = std::env::temp_dir().join(format!("mex-canario-{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&canary_path, "MEX_CANARIO_HUB_SECRETO")?;
        let shm_canary = PathBuf::from(format!("/dev/shm/mex-canario-{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&shm_canary, "MEX_CANARIO_SHM_SECRETO")?;
        let socket = profile.join("proxy.sock");
        let mut proxy = super::super::super::net_proxy::Proxy::start_unix_test(&socket, HashSet::new()).await?;
        let bridge_binary = std::env::current_exe()?
            .parent()
            .and_then(Path::parent)
            .map(|dir| dir.join("donnabot-host"))
            .context("binário do Hub não encontrado para a ponte")?;
        ensure!(bridge_binary.exists(), "binário do Hub ausente: {}", bridge_binary.display());
        let mut launcher = RealLauncher {
            chromium: "/usr/bin/chromium".into(),
            xvfb: "/usr/bin/Xvfb".into(),
            host_binary: Some(bridge_binary),
        };
        let mut processes = launcher.start(&profile, &socket)?;
        let result = async {
            // Sem authenticate(): nada de Fetch.enable. file: não usa proxy;
            // o que bloquear aqui é mount/PID namespace, e só ele.
            let cdp = processes.cdp().context("CDP ausente")?;
            let session = cdp.page_session().await?;
            cdp.command_session(&session, "Page.enable", serde_json::json!({}), Duration::from_secs(5)).await?;
            cdp.command_session(&session, "Runtime.enable", serde_json::json!({}), Duration::from_secs(5)).await?;
            // Mutação estrutural: sem mount/PID ns próprios, nem adianta ler.
            let main_pid = chromium_main_pid(&profile)?;
            assert_ne!(ns_of(main_pid, "mnt"), ns_of(std::process::id(), "mnt"), "Chromium no mount ns do Hub");
            assert_ne!(ns_of(main_pid, "pid"), ns_of(std::process::id(), "pid"), "Chromium no PID ns do Hub");
            assert_ne!(ns_of(main_pid, "net"), ns_of(std::process::id(), "net"), "Chromium no net ns do Hub");
            // (1) Controle positivo: o visível TEM de ser lido, senão o resto
            // passaria por navegação quebrada/página de erro.
            let visible_url = format!("file://{}/visivel.html", profile.display());
            let text =
                texto_apos_navegar(&cdp, &session, &visible_url, "MEX_VISIVEL_OK", Duration::from_secs(5)).await?;
            assert!(text.contains("MEX_VISIVEL_OK"), "controle positivo falhou (file: visível não lido): {text}");
            // (2) Canários escondidos NÃO podem ser lidos.
            for (url, marker) in [
                (format!("file://{}", canary_path.display()), "MEX_CANARIO_HUB_SECRETO"),
                (format!("file://{}", shm_canary.display()), "MEX_CANARIO_SHM_SECRETO"),
            ] {
                let text = texto_apos_navegar(&cdp, &session, &url, marker, Duration::from_secs(2)).await?;
                assert!(!text.contains(marker), "página leu o canário escondido {url}: {text}");
            }
            // (3) /proc do Hub: nem pela raiz dele, nem environ/cmdline.
            let hub = std::process::id();
            for (url, marker) in [
                (format!("file:///proc/{hub}/root{}", canary_path.display()), "MEX_CANARIO_HUB_SECRETO"),
                (format!("file:///proc/{hub}/environ"), "PATH="),
                (format!("file:///proc/{hub}/cmdline"), "donnabot_host"),
            ] {
                let text = texto_apos_navegar(&cdp, &session, &url, marker, Duration::from_secs(2)).await?;
                assert!(!text.contains(marker), "página vazou /proc do Hub via {url}: {text}");
            }
            // O gate do Hub recusa file: antes de qualquer Page.navigate.
            assert!(cdp.navigate_validated(&session, "file:///etc/hostname").await.is_err());
            Ok::<(), anyhow::Error>(())
        }
        .await;
        proxy.stop().await;
        let stopped = launcher.stop(&mut processes);
        let _ = std::fs::remove_file(&canary_path);
        let _ = std::fs::remove_file(&shm_canary);
        let _ = std::fs::remove_dir_all(&profile);
        result?;
        stopped?;
        Ok(())
    }

    #[test]
    fn argumentos_do_chromium_nao_contem_token() {
        let args = chromium_args(Path::new("/usr/bin/chromium"), Path::new("/perfil"));
        assert!(args.iter().all(|arg| !arg.contains("Proxy-Authorization") && !arg.contains("segredo")));
        assert!(args.iter().any(|arg| arg == "--remote-debugging-pipe"));
    }
}
