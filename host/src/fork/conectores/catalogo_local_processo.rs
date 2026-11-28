use super::Artefato;
use crate::market::Connector;
use anyhow::{Context, Result};
use std::{io, process::Stdio};
use tempfile::TempDir;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

pub(crate) struct Processo {
    child: Child,
    grupo: i32,
    dir: TempDir,
}

impl Processo {
    pub(crate) async fn iniciar(
        conta: &Connector,
        spawn: impl FnOnce(&mut Command) -> Result<Child> + Send + 'static,
        #[cfg(test)] raiz: Option<&std::path::Path>,
    ) -> Result<Self> {
        crate::fork::motor_exec::refuse("MCP local do catálogo")?;
        let artefato = super::da_conta(conta).context("Artefato MCP não fixado")?;
        let (dir, command) = {
            #[cfg(test)]
            if let Some(raiz) = raiz {
                artefato.preparar_em(raiz).await?
            } else {
                Artefato::preparar(artefato.id()).await?
            }
            #[cfg(not(test))]
            Artefato::preparar(artefato.id()).await?
        };
        Self::lancar(dir, command, spawn).await
    }

    async fn lancar(
        dir: TempDir,
        mut command: Command,
        spawn: impl FnOnce(&mut Command) -> Result<Child> + Send + 'static,
    ) -> Result<Self> {
        let pai = i32::try_from(std::process::id()).context("PID do Hub inválido")?;
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).process_group(0).kill_on_drop(true);
        // SAFETY: entre fork e exec só há syscalls sem alocação; a thread criadora vive enquanto o Hub vive.
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::getppid() != pai {
                    return Err(io::Error::from_raw_os_error(libc::ECHILD));
                }
                Ok(())
            });
        }
        crate::fork::motor_exec::criador::criar(
            move || {
                let child = spawn(&mut command)?;
                let grupo = child.id().and_then(|id| i32::try_from(id).ok()).context("MCP do catálogo sem PID")?;
                Ok(Self { child, grupo, dir })
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

    pub(crate) fn home(&self) -> &std::path::Path {
        self.dir.path()
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
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        time::{Duration, timeout},
    };

    const FIXTURE: &[u8] = include_bytes!("../../../tests/fixtures/catalogo-stdio.sh");

    #[tokio::test]
    #[ignore = "executado como subprocesso pelo teste de morte do Hub"]
    async fn processo_catalogo_hub_fixture() -> Result<()> {
        let root = std::env::var_os("MEXERICA_FIXTURE_ROOT").context("diretório de teste ausente")?;
        let root = std::path::Path::new(&root);
        let artifact = super::super::da_ficha("fixture-stdio", "fixture-stdio.local").context("fixture")?;
        let (dir, command) = artifact.preparar_em(root).await?;
        let mut process = Processo::lancar(dir, command, |command| Ok(command.spawn()?)).await?;
        let (mut input, output) = process.pipes()?;
        input.write_all(b"fixture-home\n").await?;
        let mut ready = String::new();
        timeout(Duration::from_secs(2), BufReader::new(output).read_line(&mut ready)).await??;
        anyhow::ensure!(ready.trim_end() == process.home().to_string_lossy(), "processo de teste não respondeu");
        let record = format!("{}\n{}", process.grupo, ready);
        tokio::fs::write(root.join("starting"), record).await?;
        tokio::fs::rename(root.join("starting"), root.join("ready")).await?;
        std::future::pending::<()>().await;
        drop((process, input));
        Ok(())
    }

    #[tokio::test]
    async fn processo_catalogo_termina_quando_hub_morre_sem_drop() -> Result<()> {
        let root = tempfile::tempdir_in("/tmp")?;
        let artifacts = root.path().join("mcp-artifacts");
        tokio::fs::create_dir(&artifacts).await?;
        tokio::fs::write(artifacts.join("fixture-stdio"), FIXTURE).await?;
        let test = concat!(module_path!(), "::processo_catalogo_hub_fixture");
        let (_, test) = test.split_once("::").context("nome de teste inválido")?;
        let mut hub = Command::new(std::env::current_exe()?)
            .env_clear()
            .env("MEXERICA_FIXTURE_ROOT", root.path())
            .args(["--ignored", "--exact", test])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let record = timeout(Duration::from_secs(5), async {
            loop {
                match tokio::fs::read_to_string(root.path().join("ready")).await {
                    Ok(record) => break Ok(record),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                    Err(e) => break Err(e),
                }
            }
        })
        .await??;
        let (pid, home) = record.trim_end().split_once('\n').context("processo de teste sem identificação")?;
        let pid: u32 = pid.parse()?;
        let status = format!("/proc/{pid}/stat");
        anyhow::ensure!(tokio::fs::try_exists(&status).await?, "processo de teste morreu antes do Hub");
        hub.kill().await?;
        let stopped = timeout(Duration::from_secs(5), async {
            loop {
                match tokio::fs::read_to_string(&status).await {
                    Ok(stat) if stat.rsplit_once(") ").is_some_and(|(_, tail)| tail.starts_with("Z ")) => break Ok(()),
                    Ok(_) => tokio::time::sleep(Duration::from_millis(20)).await,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => break Ok(()),
                    Err(e) => break Err(e),
                }
            }
        })
        .await;
        tokio::fs::remove_dir_all(home).await?;
        stopped??;
        Ok(())
    }

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
        let mut process = Processo::lancar(dir, command, |command| Ok(command.spawn()?)).await?;
        let (mut input, output) = process.pipes()?;
        input.write_all(b"fixture-home\n").await?;
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
