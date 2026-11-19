//! Entrega da entrada ao PTY de login: escrita sem bloqueio e descarte da fila ao entrar no modo agente.
use super::Login;
#[cfg(unix)]
use portable_pty::MasterPty;

/// Escreve o que o PTY aceitar sem esperar: nunca bloqueia (a escrita do PTY de login é `O_NONBLOCK`).
/// `Ok(n)` são os bytes escritos; PTY cheio devolve o que coube, e o resto fica para a próxima tentativa.
pub(crate) fn write_nonblocking(writer: &mut dyn std::io::Write, bytes: &[u8]) -> std::io::Result<usize> {
    let mut done = 0;
    while done < bytes.len() {
        match writer.write(&bytes[done..]) {
            Ok(0) => return Err(std::io::Error::other("Escrita de login incompleta.")),
            Ok(n) => done += n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(done)
}

/// Entrega ao PTY a entrada pendente. Conferir o modo agente e escrever continuam atômicos
/// (`deliver_input`), e a escrita sob o cadeado não espera: o leitor, o prazo e o encerramento
/// nunca ficam presos atrás de um PTY cheio. No modo agente a entrada pendente é descartada.
pub(crate) fn flush_backlog(
    login: &Login,
    writer: &mut dyn std::io::Write,
    backlog: &mut Vec<u8>,
) -> std::io::Result<()> {
    if backlog.is_empty() {
        return Ok(());
    }
    let written = match login.deliver_input(|| write_nonblocking(writer, backlog)) {
        Ok(Some(result)) => result?,
        Ok(None) => backlog.len(),
        Err(error) => return Err(std::io::Error::other(error)),
    };
    backlog.drain(..written);
    login.release_pending(written);
    Ok(())
}

/// Fila de entrada do PTY (lado do CLI), para descartar o que ainda não foi lido ao entrar no modo agente.
///
/// Limite que sobra: o que o CLI já LEU antes de o Hub ler o marcador de modo agente (os milissegundos
/// entre o CLI trocar de modo e a leitura da saída) já está no processo do agente e não volta. Isso é
/// no máximo o que passa pelo filtro de entrada: setas, Enter vazio ou uma linha `[A-Za-z0-9-_.~/#]`
/// de até 2048 caracteres sem `/` inicial; nunca comando nem texto livre. O descarte também não é
/// atômico com as leituras do CLI: um byte lido durante o descarte escapa pelo mesmo motivo.
#[cfg(unix)]
pub(crate) struct InputQueue {
    master: std::os::fd::OwnedFd,
    slave: Option<std::path::PathBuf>,
}
#[cfg(unix)]
impl InputQueue {
    pub(crate) fn new(master: &dyn MasterPty) -> Option<Self> {
        use std::os::fd::BorrowedFd;
        let fd = master.as_raw_fd()?;
        // SAFETY: fd vivo do PTY próprio, emprestado só para duplicá-lo; a cópia mantém o PTY aberto.
        let master_fd = unsafe { BorrowedFd::borrow_raw(fd) }.try_clone_to_owned().ok()?;
        Some(Self { master: master_fd, slave: master.tty_name() })
    }
    /// `tcflush(TCIFLUSH)` no lado do CLI (bytes já entregues ao terminal e não lidos) e `TCOFLUSH` no
    /// mestre (bytes escritos e ainda em trânsito até o terminal do CLI).
    pub(crate) fn discard(&self) -> std::io::Result<()> {
        use std::os::fd::AsRawFd;
        // SAFETY: fd do PTY próprio, vivo enquanto `self` existir.
        let master = unsafe { libc::tcflush(self.master.as_raw_fd(), libc::TCOFLUSH) };
        let master = if master < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) };
        let slave = match &self.slave {
            Some(path) => {
                use std::os::unix::fs::OpenOptionsExt;
                let file = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK)
                    .open(path)?;
                // SAFETY: descritor aberto acima, fechado ao sair do escopo.
                if unsafe { libc::tcflush(file.as_raw_fd(), libc::TCIFLUSH) } < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            }
            None => Err(std::io::Error::other("PTY sem nome de terminal.")),
        };
        master.and(slave)
    }
}

#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::par::Par;
    use super::*;
    use std::io::{Error, ErrorKind, Write};

    /// Escritor falso: aceita `room` bytes por vez e depois responde `WouldBlock` (PTY cheio).
    struct Cheio {
        room: usize,
        gravado: Vec<u8>,
        eventos: Vec<ErrorKind>,
    }
    impl Write for Cheio {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if let Some(kind) = self.eventos.pop() {
                return Err(Error::from(kind));
            }
            if self.room == 0 {
                return Err(Error::from(ErrorKind::WouldBlock));
            }
            let n = buf.len().min(self.room).min(3);
            self.room -= n;
            self.gravado.extend_from_slice(&buf[..n]);
            Ok(n)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct Zero;
    impl Write for Zero {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Ok(0)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn escrita_sem_bloqueio_devolve_o_que_coube_e_guarda_o_resto() {
        let mut w = Cheio { room: 7, gravado: vec![], eventos: vec![] };
        assert_eq!(write_nonblocking(&mut w, b"0123456789").unwrap(), 7);
        assert_eq!(w.gravado, b"0123456");
        // Interrupted é repetido; WouldBlock sem nada escrito é zero, não erro.
        let mut w = Cheio { room: 0, gravado: vec![], eventos: vec![ErrorKind::Interrupted] };
        assert_eq!(write_nonblocking(&mut w, b"abc").unwrap(), 0);
        let mut w = Cheio { room: 9, gravado: vec![], eventos: vec![ErrorKind::BrokenPipe] };
        assert!(write_nonblocking(&mut w, b"abc").is_err());
        // Escrita de zero bytes é falha, nunca laço infinito.
        assert!(write_nonblocking(&mut Zero, b"abc").is_err());
    }

    #[test]
    fn descartar_a_fila_tira_do_escravo_os_bytes_ja_escritos_e_ainda_nao_lidos() {
        for _ in 0..20 {
            let par = Par::new();
            let queue = InputQueue::new(par.master.as_ref()).expect("fila de entrada");
            let mut writer = par.master.take_writer().unwrap();
            // O que o dono mandou antes da marca: o token e o Enter já estão na fila do terminal do CLI.
            writer.write_all(b"TOKEN-DO-DONO\r").unwrap();
            par.esperar_legiveis(14);
            queue.discard().unwrap();
            assert_eq!(par.legiveis(), 0, "bytes escritos antes da marca ainda chegariam ao agente");
            // Em trânsito (ainda não entregues ao terminal do CLI) também são descartados.
            writer.write_all(b"MAIS-UM\r").unwrap();
            queue.discard().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(30));
            assert_eq!(par.legiveis(), 0, "bytes em trânsito chegaram ao agente");
        }
    }
}

/// PTY real para os testes: o mestre do Hub (sempre `O_NONBLOCK`) e o escravo, em modo cru, que o teste lê.
#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub(crate) mod par {
    use super::*;
    /// PTY real: o mestre do Hub e o escravo que o teste lê. Sem filho.
    pub(crate) struct Par {
        pub(crate) master: Box<dyn MasterPty + Send>,
        slave: std::fs::File,
    }
    impl Par {
        pub(crate) fn new() -> Self {
            use std::os::fd::AsRawFd;
            use std::os::unix::fs::OpenOptionsExt;
            let pair = portable_pty::native_pty_system()
                .openpty(portable_pty::PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
                .unwrap();
            let path = pair.master.tty_name().expect("nome do escravo");
            let slave = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK)
                .open(path)
                .unwrap();
            // Modo cru: sem linha canônica, o byte fica legível assim que chega, como num CLI de TUI.
            // SAFETY: termios zerado e preenchido pelo próprio tcgetattr no descritor aberto acima.
            unsafe {
                let mut t: libc::termios = std::mem::zeroed();
                assert_eq!(libc::tcgetattr(slave.as_raw_fd(), &raw mut t), 0);
                libc::cfmakeraw(&raw mut t);
                assert_eq!(libc::tcsetattr(slave.as_raw_fd(), libc::TCSANOW, &raw const t), 0);
            }
            drop(pair.slave);
            // Como no `run`: a escrita do mestre nunca espera.
            // SAFETY: fd vivo do mestre próprio.
            unsafe {
                let fd = pair.master.as_raw_fd().expect("fd do mestre");
                let flags = libc::fcntl(fd, libc::F_GETFL);
                assert!(flags >= 0 && libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) >= 0);
            }
            Self { master: pair.master, slave }
        }
        pub(crate) fn legiveis(&self) -> i32 {
            use std::os::fd::AsRawFd;
            let mut n = 0i32;
            // SAFETY: FIONREAD só escreve um int no endereço dado.
            assert_eq!(unsafe { libc::ioctl(self.slave.as_raw_fd(), libc::FIONREAD, &mut n) }, 0);
            n
        }
        pub(crate) fn esperar_legiveis(&self, quantos: i32) {
            let limite = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while self.legiveis() < quantos {
                assert!(std::time::Instant::now() < limite, "bytes não chegaram ao escravo");
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }
}
