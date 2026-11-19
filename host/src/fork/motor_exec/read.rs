//! Leitura delegada: pipe limitado, sem abrir o caminho com o UID do Hub.
use anyhow::{Context, Result, ensure};
use std::{
    io::Read,
    path::Path,
    process::{Child, ChildStdout, Command, Stdio},
};
pub(crate) struct Input {
    child: Child,
    stdout: ChildStdout,
    finished: bool,
}
impl Input {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let mut command = Command::new(super::helper_path());
        command
            .arg("read-file")
            .arg(path)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn()?;
        let stdout = child.stdout.take().context("stdout do ajudante ausente")?;
        Ok(Self { child, stdout, finished: false })
    }
    pub(crate) fn finish(&mut self) -> Result<()> {
        let status = self.child.wait()?;
        self.finished = true;
        ensure!(status.success(), "leitura recusada com os direitos do motor");
        Ok(())
    }
}
impl Read for Input {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.stdout.read(bytes)
    }
}
impl Drop for Input {
    fn drop(&mut self) {
        if !self.finished {
            let _ = super::kill_group(self.child.id());
            let _ = self.child.wait();
        }
    }
}
