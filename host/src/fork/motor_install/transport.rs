//! Protocolo limitado do instalador: stdout de terceiros nunca vira resposta do Hub.
use anyhow::{Context, Result, ensure};
use serde::{Serialize, de::DeserializeOwned};
use std::io::BufRead;
use std::process::{ExitStatus, Stdio};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const LIMIT: usize = 64 * 1024;

async fn read(mut pipe: impl tokio::io::AsyncRead + Unpin) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    (&mut pipe).take((LIMIT + 1) as u64).read_to_end(&mut bytes).await?;
    ensure!(bytes.len() <= LIMIT, "resposta do instalador excedeu o limite");
    Ok(bytes)
}

fn verificar_saida(status: ExitStatus) -> Result<()> {
    if !status.success() {
        match status.code() {
            Some(code) => anyhow::bail!("instalador do motor: codigo={code}, categoria=falha_de_saida"),
            None => anyhow::bail!("instalador do motor: codigo=ausente, categoria=encerramento_sem_codigo"),
        }
    }
    Ok(())
}

/// O chamador prepara o ajudante e o ambiente. O guard de grupo cobre cancelamento/erro.
pub(super) async fn exchange<T: DeserializeOwned>(
    mut command: tokio::process::Command,
    request: &impl Serialize,
) -> Result<T> {
    let bytes = serde_json::to_vec(request)?;
    ensure!(bytes.len() <= LIMIT, "pedido de instalação excedeu o limite");
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
    let mut child = crate::fork::motor_exec::spawn(&mut command)?;
    let mut stdin = child.stdin.take().context("stdin do instalador ausente")?;
    let stdout = child.stdout.take().context("stdout do instalador ausente")?;
    let send = async {
        stdin.write_all(&bytes).await?;
        stdin.shutdown().await?;
        drop(stdin);
        Ok::<_, anyhow::Error>(())
    };
    let ((), stdout, status) =
        tokio::try_join!(send, read(stdout), async { child.wait().await.map_err(anyhow::Error::from) })?;
    // A recusa nasce no instalador, onde o motivo é conhecido; sem isto ela se perde atrás
    // do código de saída.
    if let Ok(super::Reply::Recusa { operacao, categoria }) = serde_json::from_slice(&stdout) {
        anyhow::bail!("instalador do motor: operação={operacao}, categoria={categoria}");
    }
    verificar_saida(status)?;
    serde_json::from_slice(&stdout).context("resposta inválida do instalador do motor")
}

pub(super) fn request<T: DeserializeOwned>(pipe: &mut impl BufRead) -> Result<T> {
    let mut bytes = Vec::new();
    std::io::Read::take(pipe, (LIMIT + 1) as u64).read_until(b'\n', &mut bytes)?;
    ensure!(bytes.len() <= LIMIT, "pedido de instalação excedeu o limite");
    serde_json::from_slice(&bytes).context("pedido de instalação inválido")
}

#[cfg(target_os = "linux")]
pub(super) fn request_pipe() -> Result<std::io::BufReader<std::fs::File>> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let sink = std::fs::File::open("/dev/null")?;
    // SAFETY: mantém o canal de controle privado; processos de terceiros só recebem /dev/null.
    let fd = unsafe { libc::fcntl(libc::STDIN_FILENO, libc::F_DUPFD_CLOEXEC, 3) };
    ensure!(fd >= 0, "duplicando entrada do instalador: {}", std::io::Error::last_os_error());
    // SAFETY: o descritor novo pertence exclusivamente ao File.
    let pipe = unsafe { std::fs::File::from_raw_fd(fd) };
    // SAFETY: descritores válidos; sink permanece aberto durante dup2.
    ensure!(unsafe { libc::dup2(sink.as_raw_fd(), libc::STDIN_FILENO) } >= 0, "isolando entrada do instalador");
    Ok(std::io::BufReader::new(pipe))
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

    #[tokio::test]
    async fn erro_do_instalador_informa_codigo_e_categoria_sem_stderr() {
        let comando = || {
            let mut command = tokio::process::Command::new("/bin/sh");
            command.args(["-c", "cat >/dev/null; printf '/privado/sentinela-stderr' >&2; exit 23"]);
            command
        };
        let pedido = super::super::Request::Node;
        let erro = exchange::<serde_json::Value>(comando(), &pedido).await.unwrap_err();
        let mut recusa = tokio::process::Command::new("/bin/sh");
        recusa.args([
            "-c",
            r#"cat >/dev/null; printf '{"result":"recusa","operacao":"substituicao","categoria":"versao_protegida"}'; exit 2"#,
        ]);
        let erro_recusa = exchange::<serde_json::Value>(recusa, &pedido).await.unwrap_err();
        let texto = format!("{erro:#}");
        assert!(texto.contains("codigo=23, categoria=falha_de_saida"), "{texto}");
        assert!(!texto.contains("sentinela-stderr"), "{texto}");
        let texto = format!("{erro_recusa:#}");
        assert!(texto.contains("operação=substituicao, categoria=versao_protegida"), "{texto}");
    }
}
