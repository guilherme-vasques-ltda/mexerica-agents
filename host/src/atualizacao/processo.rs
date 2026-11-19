//! Normalização antes de carregar política ou estado. Apenas Linux.
use anyhow::{Result, ensure};

fn limite(recurso: libc::__rlimit_resource_t, valor: libc::rlim_t) -> Result<()> {
    let limites = libc::rlimit { rlim_cur: valor, rlim_max: valor };
    // SAFETY: ponteiro válido e valores definidos pela política fixa do wrapper.
    ensure!(unsafe { libc::setrlimit(recurso, &raw const limites) } == 0, "limite não pôde ser restabelecido");
    Ok(())
}

/// Chamar em processo monothread antes de rede, acesso a arquivos ou operações.
pub fn normalizar() -> Result<()> {
    // SAFETY: consultas/ajustes do processo corrente, sem ponteiros inválidos.
    unsafe { libc::umask(0o077) };
    ensure!(
        // SAFETY: close_range fecha toda a tabela herdada; nenhum descritor é reutilizado.
        unsafe { libc::syscall(libc::SYS_close_range, 0u32, u32::MAX, 0u32) } == 0,
        "não foi possível fechar descritores"
    );
    // SAFETY: caminho fixo e flags válidas; nenhuma dependência de PATH.
    let null = unsafe { libc::open(c"/dev/null".as_ptr(), libc::O_RDWR | libc::O_CLOEXEC | libc::O_NOFOLLOW) };
    ensure!(null == 0, "não foi possível abrir /dev/null");
    ensure!(
        // SAFETY: duplica o descritor recém-aberto para saídas fixas.
        unsafe { libc::dup2(0, 1) } == 1 && unsafe { libc::dup2(0, 2) } == 2,
        "não foi possível fechar saídas herdadas"
    );
    // SAFETY: processo ainda monothread; não usa o ambiente depois deste ponto.
    ensure!(unsafe { libc::clearenv() } == 0, "não foi possível descartar ambiente");
    // SAFETY: estruturas sigset_t/sigaction são inicializadas antes do uso.
    unsafe {
        let mut mascarados: libc::sigset_t = std::mem::zeroed();
        ensure!(libc::sigemptyset(&raw mut mascarados) == 0, "sigemptyset falhou");
        for sinal in [
            libc::SIGHUP,
            libc::SIGINT,
            libc::SIGQUIT,
            libc::SIGTERM,
            libc::SIGPIPE,
            libc::SIGALRM,
            libc::SIGUSR1,
            libc::SIGUSR2,
            libc::SIGTSTP,
            libc::SIGTTIN,
            libc::SIGTTOU,
        ] {
            ensure!(libc::sigaddset(&raw mut mascarados, sinal) == 0, "sigaddset falhou");
        }
        ensure!(
            libc::sigprocmask(libc::SIG_SETMASK, &raw const mascarados, std::ptr::null_mut()) == 0,
            "máscara de sinais inválida"
        );
        for sinal in 1..=64 {
            if sinal == libc::SIGKILL || sinal == libc::SIGSTOP || sinal == 32 || sinal == 33 {
                continue;
            }
            let mut acao: libc::sigaction = std::mem::zeroed();
            acao.sa_sigaction = libc::SIG_DFL;
            ensure!(
                libc::sigemptyset(&raw mut acao.sa_mask) == 0
                    && libc::sigaction(sinal, &raw const acao, std::ptr::null_mut()) == 0,
                "disposição de sinal inválida"
            );
        }
    }
    limite(libc::RLIMIT_CORE, 0)?;
    limite(libc::RLIMIT_NOFILE, 1024)?;
    limite(libc::RLIMIT_FSIZE, 2 * 1024 * 1024 * 1024)?;
    limite(libc::RLIMIT_AS, 2 * 1024 * 1024 * 1024)?;
    limite(libc::RLIMIT_CPU, 600)?;
    Ok(())
}
