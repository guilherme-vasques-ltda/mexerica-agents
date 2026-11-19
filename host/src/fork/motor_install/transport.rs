//! Protocolo limitado do instalador: stdout de terceiros nunca vira resposta do Hub.
use anyhow::{Context, Result, ensure};
use serde::{Serialize, de::DeserializeOwned};
use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const LIMIT: usize = 64 * 1024;

async fn read(mut pipe: impl tokio::io::AsyncRead + Unpin) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    (&mut pipe).take((LIMIT + 1) as u64).read_to_end(&mut bytes).await?;
    ensure!(bytes.len() <= LIMIT, "resposta do instalador excedeu o limite");
    Ok(bytes)
}

/// O chamador prepara o ajudante e o ambiente. O guard de grupo cobre cancelamento/erro.
pub(super) async fn exchange<T: DeserializeOwned>(
    mut command: tokio::process::Command,
    request: &impl Serialize,
) -> Result<T> {
    let bytes = serde_json::to_vec(request)?;
    ensure!(bytes.len() <= LIMIT, "pedido de instalação excedeu o limite");
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = crate::fork::motor_exec::spawn(&mut command)?;
    let mut stdin = child.stdin.take().context("stdin do instalador ausente")?;
    let stdout = child.stdout.take().context("stdout do instalador ausente")?;
    let stderr = child.stderr.take().context("stderr do instalador ausente")?;
    let send = async {
        stdin.write_all(&bytes).await?;
        stdin.shutdown().await?;
        drop(stdin);
        Ok::<_, anyhow::Error>(())
    };
    let ((), stdout, stderr, status) =
        tokio::try_join!(send, read(stdout), read(stderr), async { child.wait().await.map_err(anyhow::Error::from) })?;
    ensure!(status.success(), "instalador do motor: {}", String::from_utf8_lossy(&stderr).trim());
    serde_json::from_slice(&stdout).context("resposta inválida do instalador do motor")
}

pub(super) fn request<T: DeserializeOwned>() -> Result<T> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::io::stdin().take((LIMIT + 1) as u64).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= LIMIT, "pedido de instalação excedeu o limite");
    serde_json::from_slice(&bytes).context("pedido de instalação inválido")
}

/// Preserva um descritor CLOEXEC para o resultado; filhos só herdam stdout descartável.
#[cfg(target_os = "linux")]
pub(super) fn response_pipe() -> Result<std::fs::File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let sink = std::fs::OpenOptions::new().write(true).open("/dev/null")?;
    // SAFETY: duplica stdout como descritor próprio CLOEXEC; dup2 só troca stdout do worker.
    let fd = unsafe { libc::fcntl(libc::STDOUT_FILENO, libc::F_DUPFD_CLOEXEC, 3) };
    ensure!(fd >= 0, "duplicando saída do instalador: {}", std::io::Error::last_os_error());
    // SAFETY: fd acabou de ser criado e tem um único proprietário.
    let response = unsafe { std::fs::File::from_raw_fd(fd) };
    // SAFETY: descritores válidos; o arquivo sink permanece aberto durante dup2.
    ensure!(unsafe { libc::dup2(sink.as_raw_fd(), libc::STDOUT_FILENO) } >= 0, "isolando saída de terceiros");
    Ok(response)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn protocolo_recusa_falha_json_invalido_e_saida_excessiva() {
        for script in [
            "cat >/dev/null; printf '{\"path\":\"/forjado\"}'; exit 1",
            "cat >/dev/null; printf 'saida de terceiro'",
            "cat >/dev/null; head -c 65537 /dev/zero",
        ] {
            let mut command = tokio::process::Command::new("/bin/sh");
            command.args(["-c", script]);
            assert!(exchange::<serde_json::Value>(command, &serde_json::json!({})).await.is_err());
        }
        let mut command = tokio::process::Command::new("/bin/sh");
        command.args(["-c", "cat"]);
        let expected = serde_json::json!({"path": "/raiz/instalada"});
        assert_eq!(exchange::<serde_json::Value>(command, &expected).await.unwrap(), expected);
    }
}
