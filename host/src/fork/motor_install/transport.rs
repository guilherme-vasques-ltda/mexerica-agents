//! Protocolo limitado do instalador: stdout de terceiros nunca vira resposta do Hub.
use anyhow::{Context, Result, ensure};
use serde::{Serialize, de::DeserializeOwned};
use std::io::BufRead;
use std::process::{ExitStatus, Stdio};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

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
    verificar_saida(status)?;
    serde_json::from_slice(&stdout).context("resposta inválida do instalador do motor")
}

pub(super) fn request<T: DeserializeOwned>(pipe: &mut impl BufRead) -> Result<T> {
    let mut bytes = Vec::new();
    std::io::Read::take(pipe, (LIMIT + 1) as u64).read_until(b'\n', &mut bytes)?;
    ensure!(bytes.len() <= LIMIT, "pedido de instalação excedeu o limite");
    serde_json::from_slice(&bytes).context("pedido de instalação inválido")
}

pub(super) async fn exchange_registry(
    mut command: tokio::process::Command,
    request: &super::Request,
    revalidar: impl Future<Output = Result<super::ProtecaoRegistro>>,
) -> Result<super::Reply> {
    let bytes = serde_json::to_vec(request)?;
    ensure!(bytes.len() < LIMIT, "pedido de instalação excedeu o limite");
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
    let mut protecao = None;
    let mut child = crate::fork::motor_exec::spawn(&mut command)?;
    let stdin = child.stdin.take().context("stdin do instalador ausente")?;
    let stdout = child.stdout.take().context("stdout do instalador ausente")?;
    let exchange = async {
        let mut stdin = stdin;
        stdin.write_all(&bytes).await?;
        stdin.write_all(b"\n").await?;
        let mut stdout = tokio::io::BufReader::new(stdout);
        let mut primeira = Vec::new();
        (&mut stdout).take((LIMIT + 1) as u64).read_until(b'\n', &mut primeira).await?;
        ensure!(primeira.len() <= LIMIT, "resposta do instalador excedeu o limite");
        if matches!(serde_json::from_slice(&primeira), Ok(super::Reply::Revalidar)) {
            let confirmacao = protecao.insert(revalidar.await?);
            let bytes = serde_json::to_vec(&confirmacao.protecao)?;
            ensure!(bytes.len() < LIMIT, "confirmação de uso excedeu o limite");
            stdin.write_all(&bytes).await?;
            stdin.write_all(b"\n").await?;
            stdin.shutdown().await?;
            return read(stdout).await;
        }
        Ok(primeira)
    };
    let (resposta, status) = match tokio::time::timeout(std::time::Duration::from_secs(25 * 60), async {
        tokio::join!(exchange, child.wait())
    })
    .await
    {
        Ok(result) => result,
        Err(error) => {
            child.kill().await.context("encerrando instalador do motor")?;
            return Err(error).context("instalação do motor excedeu o prazo");
        }
    };
    let resposta = resposta?;
    // A recusa nasce no instalador, onde o motivo é conhecido; sem isto ela se perde atrás
    // do código de saída.
    if let Ok(super::Reply::Recusa { operacao, categoria }) = serde_json::from_slice(&resposta) {
        anyhow::bail!("instalador do motor: operação={operacao}, categoria={categoria}");
    }
    verificar_saida(status?)?;
    serde_json::from_slice(&resposta).context("resposta inválida do instalador do motor")
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
        let pedido = super::super::Request::Registry { agent: serde_json::json!({}), protecao: None };
        let erro = exchange::<serde_json::Value>(comando(), &pedido).await.unwrap_err();
        let mut recusa = tokio::process::Command::new("/bin/sh");
        recusa.args([
            "-c",
            r#"read -r pedido; printf '{"result":"recusa","operacao":"substituicao","categoria":"versao_protegida"}'; exit 2"#,
        ]);
        let erro_recusa = exchange_registry(recusa, &pedido, std::future::pending()).await.unwrap_err();
        let mut registro = tokio::process::Command::new("/bin/sh");
        registro.args(["-c", "read -r pedido; printf '/privado/sentinela-stderr' >&2; exit 23"]);
        let erro_registro = exchange_registry(registro, &pedido, std::future::pending()).await.unwrap_err();
        for erro in [erro, erro_registro] {
            let texto = format!("{erro:#}");
            assert!(texto.contains("codigo=23, categoria=falha_de_saida"), "{texto}");
            assert!(!texto.contains("sentinela-stderr"), "{texto}");
        }
        let texto = format!("{erro_recusa:#}");
        assert!(texto.contains("operação=substituicao, categoria=versao_protegida"), "{texto}");
    }

    #[tokio::test]
    async fn marcador_criado_durante_download_bloqueia_substituicao_no_worker() {
        let raiz = tempfile::tempdir().unwrap();
        let versao = raiz.path().join("registry-in-use/agente/1");
        std::fs::create_dir_all(&versao).unwrap();
        let reserva = versao.join("reserva.propria");
        std::fs::write(&reserva, "").unwrap();
        let executavel = raiz.path().join("executavel");
        std::fs::write(&executavel, "original").unwrap();
        let mut command = tokio::process::Command::new("/bin/sh");
        command.current_dir(raiz.path()).args([
            "-c",
            r#"
            read -r pedido && touch baixando || exit 2
            while [ ! -f continuar ]; do sleep 0.01; done
            printf '{"result":"revalidar"}\n'
            read -r lista || exit 3
            if [ "$lista" = '["1"]' ]; then
                printf '{"result":"status","code":1}'
            else
                printf 'substituído' > executavel
                printf '{"result":"status","code":0}'
            fi
        "#,
        ]);
        let pedido = super::super::Request::Registry {
            agent: serde_json::json!({}),
            protecao: Some(crate::agent::registry::Protecao { em_uso: Vec::new(), registradas: vec!["1".into()] }),
        };
        let publicar = exchange_registry(
            command,
            &pedido,
            crate::agent::registry::protecao_motor(raiz.path(), "agente".into(), reserva),
        );
        let novo_marcador = async {
            while !raiz.path().join("baixando").is_file() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            std::fs::write(versao.join("boot.-"), "").unwrap();
            std::fs::write(raiz.path().join("continuar"), "").unwrap();
        };
        let (resposta, ()) =
            tokio::time::timeout(std::time::Duration::from_secs(5), async { tokio::join!(publicar, novo_marcador) })
                .await
                .unwrap();
        assert!(matches!(resposta.unwrap(), super::super::Reply::Status { code: 1 }));
        assert_eq!(std::fs::read_to_string(executavel).unwrap(), "original");
    }
}
