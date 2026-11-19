//! Pipes não bloqueantes, prazo/orçamento únicos, encerramento e reap síncronos.
use super::{linux, odb::Private, snapshot::Budget};
use anyhow::{Context as _, Result, ensure};
use std::io::{ErrorKind, Read as _, Write as _};
use std::os::fd::AsRawFd as _;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};

pub(super) struct Process<'a, 'b> {
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    error: ChildStderr,
    budget: &'a Budget<'b>,
    error_eof: bool,
    reaped: bool,
}

impl<'a, 'b> Process<'a, 'b> {
    pub fn start(private: &Private, args: &[&str], budget: &'a Budget<'b>) -> Result<Self> {
        private.require_sealed()?;
        let home = private.root.join("home");
        let mut command = Command::new("/usr/bin/git");
        command
            .arg("--git-dir")
            .arg(&private.git)
            .args(args)
            .current_dir(&private.root)
            .env_clear()
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", &home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_PAGER", "cat")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("LC_ALL", "C")
            .env("GIT_OBJECT_DIRECTORY", private.git.join("objects"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        Self::guarded(command, budget)
    }

    fn guarded(mut command: Command, budget: &'a Budget<'b>) -> Result<Self> {
        budget.launch()?;
        linux::command(&mut command).context("snapshot_git_landlock_guard")?;
        let mut child = command.spawn().context("snapshot_git_spawn_guard")?;
        // Stdio::piped garante os três campos. Montar o guard antes de qualquer fcntl.
        let pipes = (child.stdin.take(), child.stdout.take(), child.stderr.take());
        let (Some(input), Some(output), Some(error)) = pipes else {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("snapshot_git_pipe");
        };
        let value = Self { child, input: Some(input), output, error, budget, error_eof: false, reaped: false };
        linux::nonblocking(value.output.as_raw_fd())?;
        linux::nonblocking(value.error.as_raw_fd())?;
        if let Some(pipe) = &value.input {
            linux::nonblocking(pipe.as_raw_fd())?;
        }
        Ok(value)
    }

    #[cfg(test)]
    pub(super) fn probe(private: &Private, script: &str, budget: &'a Budget<'b>) -> Result<Self> {
        // Só teste do supervisor; nenhuma seleção de executável/script na API de produção.
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", script])
            .current_dir(&private.root)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        Self::guarded(command, budget)
    }

    #[cfg(test)]
    pub(super) fn pid(&self) -> u32 {
        self.child.id()
    }

    fn stderr(&mut self) -> Result<()> {
        let mut bytes = [0_u8; 4096];
        loop {
            self.budget.check()?;
            match self.error.read(&mut bytes) {
                Ok(0) => {
                    self.error_eof = true;
                    return Ok(());
                }
                Ok(n) => self.budget.stderr(n)?,
                Err(e) if e.kind() == ErrorKind::WouldBlock => return Ok(()),
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(e.into()),
            }
        }
    }

    fn pause(&self, writing: bool) -> Result<()> {
        self.budget.check()?;
        let mut descriptors = [
            libc::pollfd { fd: self.output.as_raw_fd(), events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: self.error.as_raw_fd(), events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: -1, events: libc::POLLOUT, revents: 0 },
        ];
        if writing && let Some(input) = &self.input {
            descriptors[2].fd = input.as_raw_fd();
        }
        // SAFETY: poll recebe array válido; espera no máximo 10 ms e nunca herda FDs.
        let result = unsafe { libc::poll(descriptors.as_mut_ptr(), 3, 10) };
        if result < 0 && std::io::Error::last_os_error().kind() != ErrorKind::Interrupted {
            return Err(std::io::Error::last_os_error().into());
        }
        self.budget.check()
    }

    /// Só para o pedido cat-file (OID completo + LF, no máximo 65 bytes).
    pub fn request(&mut self, oid: &str) -> Result<()> {
        ensure!(super::super::oid(oid), "snapshot_git_request");
        let request = format!("{oid}\n");
        let mut bytes = request.as_bytes();
        while !bytes.is_empty() {
            self.budget.check()?;
            self.stderr()?;
            match self.input.as_mut().context("snapshot_git_stdin")?.write(bytes) {
                Ok(0) => anyhow::bail!("snapshot_git_stdin"),
                Ok(n) => bytes = &bytes[n..],
                Err(e) if e.kind() == ErrorKind::WouldBlock => self.pause(true)?,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    pub fn exact(&mut self, mut bytes: &mut [u8]) -> Result<()> {
        while !bytes.is_empty() {
            self.budget.check()?;
            self.stderr()?;
            match self.output.read(bytes) {
                Ok(0) => anyhow::bail!("snapshot_git_eof"),
                Ok(n) => {
                    self.budget.stdout(n)?;
                    bytes = &mut bytes[n..];
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => self.pause(false)?,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    pub fn line(&mut self) -> Result<String> {
        let mut bytes = Vec::with_capacity(256);
        for _ in 0..256 {
            let mut byte = [0];
            self.exact(&mut byte)?;
            if byte[0] == b'\n' {
                return Ok(String::from_utf8(bytes)?);
            }
            bytes.push(byte[0]);
        }
        anyhow::bail!("snapshot_git_header_limit")
    }

    /// Intercala escrita e leitura: hash-object pode bufferizar stdout até EOF.
    /// Teto local é calculado pela contagem de OIDs antes da primeira alocação.
    pub fn finish(mut self, input: &[u8], limit: usize) -> Result<Vec<u8>> {
        let mut offset = 0;
        let mut output = Vec::new();
        let mut eof = false;
        loop {
            self.budget.check()?;
            self.stderr()?;
            if offset == input.len() {
                self.input.take();
            }
            if let Some(pipe) = &mut self.input {
                match pipe.write(&input[offset..]) {
                    Ok(0) => anyhow::bail!("snapshot_git_stdin"),
                    Ok(n) => offset += n,
                    Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {}
                    Err(e) => return Err(e.into()),
                }
            }
            let mut bytes = [0_u8; 4096];
            if !eof {
                match self.output.read(&mut bytes) {
                    Ok(0) => eof = true,
                    Ok(n) => {
                        self.budget.stdout(n)?;
                        ensure!(
                            output.len().checked_add(n).is_some_and(|size| size <= limit),
                            "snapshot_git_extra_output"
                        );
                        output.try_reserve(n)?;
                        output.extend_from_slice(&bytes[..n]);
                    }
                    Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {}
                    Err(e) => return Err(e.into()),
                }
            }
            if eof
                && self.error_eof
                && let Some(status) = self.child.try_wait()?
            {
                self.reaped = true;
                ensure!(status.success() && offset == input.len(), "snapshot_git_exit");
                self.budget.check()?;
                return Ok(output);
            }
            self.pause(offset < input.len())?;
        }
    }
}

impl Drop for Process<'_, '_> {
    fn drop(&mut self) {
        if !self.reaped {
            // Síncrono: cancelar ou falhar nunca deixa leitor/filho solto na cópia.
            self.input.take();
            let _ = self.child.kill();
            if let Err(error) = self.child.wait() {
                tracing::warn!(%error, "snapshot_git_reap");
            }
        }
    }
}
