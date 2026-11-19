//! Interface fechada; o único conteúdo de entrada do chamador é o pedido da
//! verificação de partida, por pipe, limitado e com prazo (ver `ler_verificacao`).
use std::ffi::OsString;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operacao {
    Atualizar,
    Reverter,
    Aceitar,
}
pub fn interpretar(args: &[OsString]) -> Option<Operacao> {
    match args {
        [] => Some(Operacao::Atualizar),
        [arg] if arg == "--reverter" => Some(Operacao::Reverter),
        [arg] if arg == "--aceitar" => Some(Operacao::Aceitar),
        _ => None,
    }
}
/// Captura uma solicitação opcional antes de fechar os descritores herdados.
/// Apenas pipe, mensagem limitada e prazo curto; sem entrada equivale a aceite direto.
#[cfg(target_os = "linux")]
pub fn ler_verificacao() -> anyhow::Result<Option<Vec<u8>>> {
    ler_verificacao_fd(0)
}

#[cfg(target_os = "linux")]
fn ler_verificacao_fd(entrada: libc::c_int) -> anyhow::Result<Option<Vec<u8>>> {
    use anyhow::{Context, ensure};
    use std::time::{Duration, Instant};
    // SAFETY: estrutura de saída simples, inicializada antes de fstat.
    let mut meta: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: consulta descritor de entrada e estrutura inicializada.
    ensure!(unsafe { libc::fstat(entrada, &raw mut meta) } == 0, "entrada indisponível");
    if meta.st_mode & libc::S_IFMT != libc::S_IFIFO {
        return Ok(None);
    }
    // SAFETY: apenas altera flags do descritor próprio de entrada.
    let flags = unsafe { libc::fcntl(entrada, libc::F_GETFL) };
    ensure!(flags >= 0, "entrada inválida");
    // SAFETY: modifica somente as flags do descritor de entrada verificado.
    let ajustado = unsafe { libc::fcntl(entrada, libc::F_SETFL, flags | libc::O_NONBLOCK) };
    ensure!(ajustado == 0, "entrada inválida");
    let limite = Instant::now() + Duration::from_secs(3);
    let mut dados = Vec::new();
    loop {
        let restante = limite.saturating_duration_since(Instant::now());
        ensure!(!restante.is_zero(), "prazo de entrada excedido");
        let mut fd = libc::pollfd { fd: entrada, events: libc::POLLIN | libc::POLLHUP, revents: 0 };
        let ms = i32::try_from(restante.as_millis()).unwrap_or(3000).max(1);
        // SAFETY: estrutura pollfd válida e tempo limitado.
        let pronto = unsafe { libc::poll(&raw mut fd, 1, ms) };
        ensure!(pronto > 0 && fd.revents & (libc::POLLERR | libc::POLLNVAL) == 0, "entrada indisponível");
        let mut buffer = [0u8; 1025];
        // SAFETY: buffer próprio válido e leitura limitada.
        let lidos = unsafe { libc::read(entrada, buffer.as_mut_ptr().cast(), buffer.len()) };
        if lidos == 0 {
            break;
        }
        if lidos < 0 {
            let erro = std::io::Error::last_os_error();
            if erro.kind() == std::io::ErrorKind::WouldBlock || erro.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(erro).context("lendo verificação");
        }
        dados.extend_from_slice(&buffer[..usize::try_from(lidos)?]);
        ensure!(dados.len() <= 1024, "verificação grande demais");
    }
    if dados.is_empty() {
        return Ok(None);
    }
    Ok(Some(dados))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    #[test]
    fn leitura_por_pipe_limita_a_entrada_e_distingue_ausencia() -> anyhow::Result<()> {
        use std::{
            fs::File,
            io::Write,
            os::fd::{AsRawFd, FromRawFd},
        };
        fn ler(dados: &[u8]) -> anyhow::Result<Option<Vec<u8>>> {
            let mut fds = [0; 2];
            // SAFETY: array válido recebe os dois descritores exclusivos do pipe.
            anyhow::ensure!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } == 0, "pipe indisponível");
            // SAFETY: os dois descritores recém-criados passam a ter um dono cada.
            let leitura = unsafe { File::from_raw_fd(fds[0]) };
            // SAFETY: segundo descritor exclusivo criado acima.
            let mut escrita = unsafe { File::from_raw_fd(fds[1]) };
            escrita.write_all(dados)?;
            drop(escrita);
            ler_verificacao_fd(leitura.as_raw_fd())
        }
        assert_eq!(ler(b"{}")?, Some(b"{}".to_vec()));
        assert_eq!(ler(b"")?, None);
        assert!(ler(&vec![b'x'; 1025]).is_err());
        Ok(())
    }

    #[test]
    fn apenas_formas_permitidas() {
        assert_eq!(interpretar(&[]), Some(Operacao::Atualizar));
        assert_eq!(interpretar(&["--reverter".into()]), Some(Operacao::Reverter));
        assert_eq!(interpretar(&["--aceitar".into()]), Some(Operacao::Aceitar));
        assert_eq!(interpretar(&["".into()]), None);
        assert_eq!(interpretar(&["--aceitar".into(), "x".into()]), None);
        assert_eq!(interpretar(&["--help".into()]), None);
    }
}
