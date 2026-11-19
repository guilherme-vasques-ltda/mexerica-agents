//! Monitor do journal e prazo absoluto. Executado apenas pelo guard root.
use anyhow::{Result, ensure};
use std::{
    fs::File,
    io::Read,
    os::fd::{AsRawFd, FromRawFd},
};

pub enum Situacao {
    Liberado,
    Pendente(i64),
    Bloqueado,
    Ocupado,
}

fn novo(fd: libc::c_int) -> Result<File> {
    ensure!(fd >= 0, "monitor indisponível: {}", std::io::Error::last_os_error());
    // SAFETY: syscall retornou descritor novo, transferido ao File.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn prazo(fd: &File, instante: Option<i64>) -> Result<()> {
    let segundos = instante.unwrap_or(0);
    ensure!(segundos >= 0, "prazo inválido");
    let segundos = libc::time_t::try_from(segundos)?;
    let especificacao = libc::itimerspec {
        it_interval: libc::timespec { tv_sec: 0, tv_nsec: 0 },
        it_value: libc::timespec { tv_sec: segundos, tv_nsec: 0 },
    };
    ensure!(
        // SAFETY: descritor timerfd e ponteiro para estrutura inicializada.
        unsafe {
            libc::timerfd_settime(
                fd.as_raw_fd(),
                libc::TFD_TIMER_ABSTIME | libc::TFD_TIMER_CANCEL_ON_SET,
                &raw const especificacao,
                std::ptr::null_mut(),
            )
        } == 0,
        "não foi possível programar prazo"
    );
    Ok(())
}

/// Lê o timer do prazo. `true` = prazo vencido. Um ajuste do relógio de parede
/// (NTP, RTC no boot) cancela o timer com `ECANCELED` mesmo desarmado, por causa de
/// `TFD_TIMER_CANCEL_ON_SET`: não é falha do monitor, só exige reavaliar o journal
/// (a reavaliação confere o relógio sob a trava e bloqueia salto suspeito).
fn ler_prazo(timer: &mut File) -> Result<bool> {
    let mut contagem = [0u8; 8];
    expiracao(timer.read_exact(&mut contagem))
}

fn expiracao(leitura: std::io::Result<()>) -> Result<bool> {
    match leitura {
        Ok(()) => Ok(true),
        Err(e) if e.raw_os_error() == Some(libc::ECANCELED) || e.kind() == std::io::ErrorKind::WouldBlock => Ok(false),
        Err(e) => Err(anyhow::Error::new(e).context("leitura do prazo falhou")),
    }
}

fn eventos(fd: &mut File) -> Result<bool> {
    let mut buffer = [0u8; 8192];
    let quantidade = match fd.read(&mut buffer) {
        Ok(n) => n,
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    let mut pos = 0usize;
    let mut relevante = false;
    while pos < quantidade {
        let tamanho = size_of::<libc::inotify_event>();
        ensure!(quantidade - pos >= tamanho, "evento incompleto");
        // SAFETY: comprimento conferido; leitura não alinhada de estrutura POD.
        let evento = unsafe { std::ptr::read_unaligned(buffer[pos..].as_ptr().cast::<libc::inotify_event>()) };
        let fim = pos
            .checked_add(tamanho)
            .and_then(|v| v.checked_add(evento.len as usize))
            .ok_or_else(|| anyhow::anyhow!("evento grande demais"))?;
        ensure!(fim <= quantidade, "evento incompleto");
        ensure!(
            evento.mask & (libc::IN_Q_OVERFLOW | libc::IN_IGNORED | libc::IN_DELETE_SELF | libc::IN_MOVE_SELF) == 0,
            "monitor do estado perdido"
        );
        let nome = &buffer[pos + tamanho..fim];
        let nome = nome.split(|b| *b == 0).next().unwrap_or_default();
        if nome == b"journal" || nome == b"bloqueio" {
            relevante = true;
        }
        pos = fim;
    }
    Ok(relevante)
}

/// Aguarda mudanças atômicas do journal e acorda no prazo absoluto.
/// Se o monitor ou o relógio falhar, retorna o erro; o chamador decide a
/// contenção (parada e bloqueio) conforme a transação em andamento.
pub fn vigiar(mut avaliar: impl FnMut() -> Result<Situacao>, mut parar: impl FnMut() -> Result<()>) -> Result<()> {
    // SAFETY: inicializa descritores próprios, sem herdar entrada do chamador.
    let mut inotify = novo(unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) })?;
    let mascara = libc::IN_MOVED_TO
        | libc::IN_CREATE
        | libc::IN_DELETE
        | libc::IN_ATTRIB
        | libc::IN_CLOSE_WRITE
        | libc::IN_DELETE_SELF
        | libc::IN_MOVE_SELF;
    ensure!(
        // SAFETY: caminho fixo root-owned e descritor inotify válido.
        unsafe { libc::inotify_add_watch(inotify.as_raw_fd(), c"/var/lib/donnabot/atualizacao".as_ptr(), mascara) }
            >= 0,
        "não foi possível observar journal"
    );
    // SAFETY: cria timer realtime próprio com close-on-exec.
    let mut timer =
        novo(unsafe { libc::timerfd_create(libc::CLOCK_REALTIME, libc::TFD_CLOEXEC | libc::TFD_NONBLOCK) })?;
    let mut prazo_atual = None;
    let mut parou_por_contencao = false;
    loop {
        let ocupado = match avaliar()? {
            Situacao::Liberado => {
                prazo(&timer, None)?;
                prazo_atual = None;
                parou_por_contencao = false;
                false
            }
            Situacao::Pendente(instante) => {
                prazo(&timer, Some(instante))?;
                prazo_atual = Some(instante);
                parou_por_contencao = false;
                false
            }
            Situacao::Bloqueado => {
                prazo(&timer, None)?;
                prazo_atual = None;
                parar()?;
                false
            }
            Situacao::Ocupado => {
                if prazo_atual.is_none() && !parou_por_contencao {
                    parar()?;
                    parou_por_contencao = true;
                }
                true
            }
        };
        loop {
            let mut fds = [
                libc::pollfd { fd: inotify.as_raw_fd(), events: libc::POLLIN, revents: 0 },
                libc::pollfd { fd: timer.as_raw_fd(), events: libc::POLLIN, revents: 0 },
            ];
            // SAFETY: array válido de pollfd, aguardando somente descritores próprios.
            let pronto =
                unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, if ocupado { 100 } else { -1 }) };
            if pronto < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            if pronto == 0 && ocupado {
                break;
            }
            ensure!(
                pronto > 0 && fds.iter().all(|p| p.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) == 0),
                "monitor falhou"
            );
            if fds[1].revents & libc::POLLIN != 0 {
                if !ler_prazo(&mut timer)? {
                    // Relógio ajustado: reavalia e reprograma o prazo absoluto.
                    break;
                }
                // O aceite pode ter sido persistido pouco antes do prazo.
                // Só interrompe imediatamente se a janela ainda estiver
                // bloqueada ou a trava impedir a confirmação.
                match avaliar()? {
                    Situacao::Liberado | Situacao::Pendente(_) => (),
                    Situacao::Bloqueado | Situacao::Ocupado => parar()?,
                }
                break;
            }
            if fds[0].revents & libc::POLLIN != 0 && eventos(&mut inotify)? {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ajuste_do_relogio_reavalia_sem_derrubar_o_guard() -> Result<()> {
        let cancelado = std::io::Error::from_raw_os_error(libc::ECANCELED);
        assert!(!expiracao(Err(cancelado))?);
        assert!(!expiracao(Err(std::io::Error::from_raw_os_error(libc::EAGAIN)))?);
        assert!(expiracao(Ok(()))?);
        assert!(expiracao(Err(std::io::Error::from_raw_os_error(libc::EBADF))).is_err());
        // Timer real com as mesmas flags do monitor, ainda sem expirar: não é falha.
        // SAFETY: cria timer próprio com close-on-exec.
        let mut timer =
            novo(unsafe { libc::timerfd_create(libc::CLOCK_REALTIME, libc::TFD_CLOEXEC | libc::TFD_NONBLOCK) })?;
        prazo(&timer, Some(i64::from(i32::MAX)))?;
        assert!(!ler_prazo(&mut timer)?);
        prazo(&timer, Some(1))?;
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(ler_prazo(&mut timer)?);
        Ok(())
    }
}
