//! Guardas e ciclo do PTY de login, isolados do terminal herdado.
use super::*;
use crate::fork::terminal_login::entrega;

impl Terms {
    pub(crate) async fn start_login(
        self: &Arc<Self>,
        hub: &Arc<crate::hub::Hub>,
        caller: &Caller,
        backend: &str,
        space: &str,
        cols: u16,
        rows: u16,
    ) -> Result<String> {
        let authority = crate::fork::engine_setup::access::resolve(hub, caller)?;
        let _mutation = crate::fork::engine_accounts::MUTATION.lock().await;
        authority.require_current(hub)?;
        anyhow::ensure!((20..=240).contains(&cols) && (4..=100).contains(&rows), "Dimensões de login inválidas.");
        let canonical =
            crate::fork::terminal_login::canonical(backend).ok_or_else(|| anyhow!("Motor desconhecido."))?.id;
        anyhow::ensure!(
            !self.0.locked().values().any(|t| t
                .login
                .as_ref()
                .is_some_and(|l| l.backend == canonical && l.space == space)
                && t.output.locked().exit.is_none()),
            "Já há um login ativo nesta conta e Space."
        );
        let login = crate::fork::terminal_login::Login::prepare(hub, caller, backend, space)?;
        let owner = Some(caller.device_key().to_owned());
        self.spawn_with(
            (login.backend.clone(), Step::Login),
            owner,
            &Linha { comando: String::new(), cabecalho: String::new() },
            cols,
            rows,
            Some(login),
        )
    }
    #[cfg(test)]
    pub(crate) fn fixture_login(
        self: &Arc<Self>,
        hub: &Arc<crate::hub::Hub>,
        caller: &Caller,
        backend: &str,
        executable: std::path::PathBuf,
    ) -> Result<String> {
        let login = crate::fork::terminal_login::Login::fixture(hub, caller, backend, executable)?;
        self.spawn_with(
            (backend.into(), Step::Login),
            Some(caller.device_key().into()),
            &Linha { comando: String::new(), cabecalho: String::new() },
            80,
            24,
            Some(login),
        )
    }
    #[cfg(test)]
    pub(crate) fn fixture_login_timeout(
        self: &Arc<Self>,
        hub: &Arc<crate::hub::Hub>,
        caller: &Caller,
        executable: std::path::PathBuf,
        ttl: Duration,
    ) -> Result<String> {
        self.fixture_login_backend_timeout(hub, caller, "antigravity", executable, ttl)
    }
    #[cfg(test)]
    pub(crate) fn fixture_login_backend_timeout(
        self: &Arc<Self>,
        hub: &Arc<crate::hub::Hub>,
        caller: &Caller,
        backend: &str,
        executable: std::path::PathBuf,
        ttl: Duration,
    ) -> Result<String> {
        let mut login = crate::fork::terminal_login::Login::fixture(hub, caller, backend, executable)?;
        Arc::get_mut(&mut login).expect("fixture sem outras referências").deadline = tokio::time::Instant::now() + ttl;
        self.spawn_with(
            (backend.into(), Step::Login),
            Some(caller.device_key().into()),
            &Linha { comando: String::new(), cabecalho: String::new() },
            80,
            24,
            Some(login),
        )
    }
}

pub(super) async fn run(
    terms: Arc<Terms>,
    id: String,
    term: Arc<Term>,
    master: Box<dyn MasterPty + Send>,
    mut child: Box<dyn Child + Send + Sync>,
    mut rx: mpsc::UnboundedReceiver<Input>,
) {
    struct Cleanup(Option<Arc<crate::fork::terminal_login::Login>>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            if let Some(login) = &self.0
                && login.discard().is_err()
            {
                tracing::warn!(backend = login.backend, "Falha ao apagar perfil provisório de login.");
            }
        }
    }
    let _cleanup = Cleanup(term.login.clone());
    let child_pid = child.process_id();
    let mut killer = child.clone_killer();
    let group = crate::fork::motor_exec::Group::pty(child.process_id());
    let streams = master.try_clone_reader().and_then(|reader| Ok((reader, master.take_writer()?)));
    let (mut reader, mut writer) = match streams {
        Ok(streams) => streams,
        Err(error) => {
            if term.login.is_none() {
                tracing::warn!(error = format!("{error:#}"), "setup terminal has no streams");
            } else {
                crate::fork::engine_setup::kill_setup_group(child_pid);
                terms.0.locked().remove(&id);
            }
            if let Some(group) = &group {
                group.terminate();
            } else {
                let _ = killer.kill();
            }
            let _ = child.wait();
            term.finish(-1);
            return;
        }
    };
    #[cfg(unix)]
    if term.login.is_some() {
        let Some(fd) = master.as_raw_fd() else {
            crate::fork::engine_setup::kill_setup_group(child_pid);
            let _ = killer.kill();
            let _ = child.wait();
            term.finish(-1);
            terms.0.locked().remove(&id);
            return;
        };
        // SAFETY: fd vivo do PTY próprio; O_NONBLOCK impede segurar a cerca em escrita bloqueante.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        // SAFETY: preserva as opções e muda só o modo de I/O do PTY próprio.
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            crate::fork::engine_setup::kill_setup_group(child_pid);
            let _ = killer.kill();
            let _ = child.wait();
            term.finish(-1);
            terms.0.locked().remove(&id);
            return;
        }
    }
    if let Some(login) = &term.login {
        let attached = loop {
            tokio::select! {
                () = login.ready.notified() => break true,
                () = login.ended() => break false,
                input = rx.recv() => match input {
                    Some(Input::Resize(cols, rows)) => { let _ = login.deliver(|| master.resize(size(cols, rows))); }
                    Some(Input::Data(_)) => {},
                    Some(Input::Kill) | None => break false,
                }
            }
        };
        if !attached {
            if let Some(group) = &group {
                group.terminate();
            } else {
                crate::fork::engine_setup::kill_setup_group(child_pid);
                let _ = killer.kill();
            }
            let _ = child.wait();
            term.finish(-1);
            terms.0.locked().remove(&id);
            return;
        }
    }
    #[cfg(unix)]
    let input_queue = term.login.as_ref().and_then(|_| {
        let queue = entrega::InputQueue::new(master.as_ref());
        if queue.is_none() {
            tracing::warn!("Não foi possível preparar o descarte da entrada pendente do login.");
        }
        queue
    });
    let mut pump = {
        let term = term.clone();
        let mut output_killer = child.clone_killer();
        tokio::task::spawn_blocking(move || {
            let mut buf = vec![0u8; 16 * 1024];
            let mut answered = !cfg!(windows);
            // EOF or an error once the child and its children are gone.
            loop {
                let n = match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(20));
                        continue;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                };
                match without_cursor_query(&buf[..n]).filter(|_| !answered) {
                    Some(rest) => {
                        answered = true;
                        let _ = term.input.send(Input::Data(CURSOR_REPLY.to_vec()));
                        term.push(&rest);
                    }
                    None => term.push(&buf[..n]),
                }
                if term.login.as_ref().is_some_and(|login| login.agent_mode()) {
                    // A fila de entrada do terminal é descartada ANTES de encerrar o CLI: o que já foi
                    // escrito no PTY antes da marca não chega ao agente. Limite no módulo `entrega::InputQueue`.
                    #[cfg(unix)]
                    if let Some(queue) = &input_queue
                        && let Err(error) = queue.discard()
                    {
                        tracing::warn!(%error, "Falha ao descartar a entrada pendente do login.");
                    }
                    terminate(None, output_killer.as_mut(), child_pid);
                    break;
                }
            }
        })
    };
    let mut wait = tokio::task::spawn_blocking(move || child.wait());
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let mut next_check = tokio::time::Instant::now() + Duration::from_secs(3);
    let mut check_delay = Duration::from_secs(3);
    let mut verified = false;
    let mut watching_submitted = false;
    // Entrada aceita e ainda não escrita (o PTY estava cheio). O teto está em `Login::enqueue`.
    let mut backlog: Vec<u8> = Vec::new();
    let status = loop {
        tokio::select! {
            status = &mut wait => {
                if let Some(login) = &term.login {
                    verified = login.current() && login.check().await == Some(true);
                }
                break status;
            }
            _ = tick.tick(), if term.login.is_some() => {
                let Some(login) = term.login.as_ref() else { continue };
                if !login.current() {
                    terminate(group.as_ref(), killer.as_mut(), child_pid);
                    break wait.await;
                }
                if login.submitted() && !watching_submitted {
                    watching_submitted = true;
                    next_check = tokio::time::Instant::now();
                    check_delay = Duration::from_secs(1);
                }
                if tokio::time::Instant::now() >= next_check {
                    next_check = tokio::time::Instant::now() + check_delay;
                    check_delay = if login.submitted() { Duration::from_secs(1) } else { (check_delay * 2).min(Duration::from_secs(60)) };
                    if login.check().await == Some(true) {
                        verified = true;
                        terminate(group.as_ref(), killer.as_mut(), child_pid);
                        break wait.await;
                    }
                    if login.progress() {
                        let _ = login.deliver(|| term.events.send(json!({"type":"notice","code":"login_waiting"})));
                    }
                }
            }
            // PTY cheio: tenta o resto da entrada de novo sem segurar nada entre as tentativas.
            () = tokio::time::sleep(Duration::from_millis(20)), if !backlog.is_empty() => {
                if let Some(login) = &term.login
                    && let Err(error) = entrega::flush_backlog(login, writer.as_mut(), &mut backlog)
                {
                    login.stop();
                    let _ = term.input.send(Input::Kill);
                    tracing::info!(%error, "login terminal write failed");
                }
            }
            input = rx.recv() => match input {
                Some(Input::Data(bytes)) => {
                    // No modo agente a entrada está fechada: a fronteira confere a marca e
                    // escreve sob o mesmo cadeado que a leitura usa para marcá-la, sem janela
                    // entre a conferência e a escrita. Recusa vira descarte silencioso.
                    let result = if let Some(login) = &term.login {
                        backlog.extend_from_slice(&bytes);
                        entrega::flush_backlog(login, writer.as_mut(), &mut backlog)
                    } else {
                        writer.write_all(&bytes)
                    };
                    if result.is_ok() && term.login.as_ref().is_some_and(|login| login.submitted()) {
                        let login = term.login.as_ref().expect("login existente");
                        if login.check().await == Some(true) {
                            verified = true;
                            terminate(group.as_ref(), killer.as_mut(), child_pid);
                            break wait.await;
                        }
                        watching_submitted = true;
                        next_check = tokio::time::Instant::now() + Duration::from_secs(1);
                        check_delay = Duration::from_secs(1);
                    }
                    if let Err(error) = result {
                        if let Some(login) = &term.login {
                            login.stop();
                            let _ = term.input.send(Input::Kill);
                        } else {
                            tracing::info!(%error, "setup terminal write failed");
                        }
                    }
                }
                Some(Input::Resize(cols, rows)) => {
                    if let Some(login) = &term.login {
                        let _ = login.deliver(|| master.resize(size(cols, rows)));
                    } else {
                        let _ = master.resize(size(cols, rows));
                    }
                }
                Some(Input::Kill) | None => {
                    if let Some(login) = &term.login {
                        login.stop();
                        terminate(group.as_ref(), killer.as_mut(), child_pid);
                    } else if let Some(group) = &group {
                        group.terminate();
                    } else {
                        let _ = killer.kill();
                    }
                }
            }
        }
    };
    // Mesmo se o CLI sair sozinho, nenhum descendente do login conserva o PTY vivo.
    if term.login.is_some() && group.is_none() {
        crate::fork::engine_setup::kill_setup_group(child_pid);
    }
    drop(group);
    // Let the last output drain; a background grandchild can hold the PTY open forever, and
    // Windows' pseudo console keeps it open until the master side closes.
    let _ = tokio::time::timeout(Duration::from_secs(1), &mut pump).await;
    drop((writer, master));
    let mut code = match status {
        Ok(Ok(status)) => i32::try_from(status.exit_code()).unwrap_or(-1),
        _ => -1,
    };
    if let Some(login) = &term.login {
        verified = verified && login.current() && login.complete().is_ok();
        code = if verified { 0 } else { -1 };
        if verified {
            let _ = term.events.send(json!({"type":"verified","cliOnly":login.backend=="antigravity"}));
        }
    }
    tracing::info!(term = %id, code, "setup terminal finished");
    term.finish(code);
    if term.login.is_none() {
        tokio::time::sleep(LINGER).await;
    }
    terms.0.locked().remove(&id);
}

/// Somente o grupo/PID criado para este login.
fn terminate(
    group: Option<&crate::fork::motor_exec::Group>,
    killer: &mut (dyn portable_pty::ChildKiller + Send + Sync),
    child_pid: Option<u32>,
) {
    if let Some(group) = group {
        group.terminate();
    } else {
        crate::fork::engine_setup::kill_setup_group(child_pid);
        let _ = killer.kill();
    }
}
