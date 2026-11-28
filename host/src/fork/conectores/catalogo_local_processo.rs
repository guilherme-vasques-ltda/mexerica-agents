use super::Artefato;
use anyhow::{Context, Result};
use std::{io, process::Stdio};
use tempfile::TempDir;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

pub(crate) struct Processo {
    child: Child,
    grupo: i32,
    _dir: TempDir,
}

impl Processo {
    pub(crate) async fn iniciar(artefato: &Artefato) -> Result<Self> {
        crate::fork::motor_exec::refuse("MCP local do catálogo")?;
        let (dir, command) = Artefato::preparar(artefato.id()).await?;
        Self::lancar(dir, command).await
    }

    async fn lancar(dir: TempDir, mut command: Command) -> Result<Self> {
        let pai = std::process::id();
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).process_group(0).kill_on_drop(true);
        // SAFETY: entre fork e exec só há syscalls sem alocação; a thread criadora vive enquanto o Hub vive.
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::getppid() as u32 != pai {
                    return Err(io::Error::from_raw_os_error(libc::ECHILD));
                }
                Ok(())
            });
        }
        crate::fork::motor_exec::criador::criar(
            move || {
                let child = command.spawn().context("Não foi possível iniciar o MCP do catálogo")?;
                let grupo = child.id().and_then(|id| i32::try_from(id).ok()).context("MCP do catálogo sem PID")?;
                Ok(Self { child, grupo, _dir: dir })
            },
            drop,
        )
        .await?
    }

    pub(crate) fn pipes(&mut self) -> Result<(ChildStdin, ChildStdout)> {
        let entrada = self.child.stdin.take().context("Entrada MCP indisponível")?;
        let saida = self.child.stdout.take().context("Saída MCP indisponível")?;
        Ok((entrada, saida))
    }
}

impl Drop for Processo {
    fn drop(&mut self) {
        // SAFETY: este spawn criou o grupo, e não espera nem recolhe o líder antes de sinalizá-lo.
        unsafe { libc::killpg(self.grupo, libc::SIGKILL) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncBufReadExt, BufReader},
        time::{Duration, timeout},
    };

    const FIXTURE: &[u8] = include_bytes!("../../../tests/fixtures/catalogo-stdio.sh");

    #[tokio::test]
    async fn processo_do_catalogo_recebe_ambiente_privado_e_termina_no_drop() -> Result<()> {
        let root = tempfile::tempdir_in("/tmp")?;
        let artifacts = root.path().join("mcp-artifacts");
        tokio::fs::create_dir(&artifacts).await?;
        let source = artifacts.join("fixture-stdio");
        tokio::fs::write(&source, FIXTURE).await?;
        let artifact = super::super::da_ficha("fixture-stdio", "fixture-stdio.local").context("fixture")?;
        let (dir, command) = artifact.preparar_em(root.path()).await?;
        let home = dir.path().to_string_lossy().into_owned();
        let mut process = Processo::lancar(dir, command).await?;
        let (input, output) = process.pipes()?;
        let mut output = BufReader::new(output);
        let mut ready = String::new();
        timeout(Duration::from_secs(2), output.read_line(&mut ready)).await??;
        assert_eq!(ready.trim_end(), home);
        drop(process);
        assert_eq!(timeout(Duration::from_secs(2), output.read_line(&mut String::new())).await??, 0);
        drop(input);
        assert!(!std::path::Path::new(&home).exists());
        tokio::fs::write(&source, b"modificado").await?;
        assert!(artifact.preparar_em(root.path()).await.is_err());
        Ok(())
    }
}
