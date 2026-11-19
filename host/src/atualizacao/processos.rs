//! Encerramento e prova de ausência de privilégio do ajudante desativado.
//!
//! Ordem exigida pelo chamador: slot já fechado (0700, sem travessia pelo Hub)
//! e capabilities do inode já removidas. Depois disso, nenhuma execução nova do
//! inode recebe privilégio; resta encerrar quem já o executa ou ainda mantém
//! capabilities sob o UID do Hub. A varredura repete até duas passagens limpas
//! consecutivas para cobrir execuções que estavam em andamento na remoção.
use anyhow::{Context, Result, ensure};
use std::{
    fs::{self, File},
    io,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
    path::Path,
    time::Duration,
};

const MAX_PROCESSOS: usize = 32768;
const MAX_DESCRITORES: usize = 8192;
const MAX_PASSAGENS: usize = 8;
/// `CAP_SETGID` (6) e `CAP_SETUID` (7), as únicas concedidas ao ajudante.
const CAPS_AJUDANTE: u64 = (1 << 6) | (1 << 7);

fn desapareceu(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(libc::ENOENT | libc::ESRCH))
}
fn mesmo_inode(path: &Path, dev: u64, ino: u64) -> Result<bool> {
    match fs::metadata(path) {
        Ok(meta) => Ok(meta.dev() == dev && meta.ino() == ino),
        Err(e) if desapareceu(&e) => Ok(false),
        Err(_) => anyhow::bail!("não foi possível verificar referências a inode anterior"),
    }
}

/// Credenciais lidas de /proc/<pid>/status. `None` se o processo terminou.
struct Credenciais {
    real: u32,
    efetivo: u32,
    capacidades: bool,
}
fn credenciais(base: &Path) -> Result<Option<Credenciais>> {
    let texto = match fs::read_to_string(base.join("status")) {
        Ok(t) => t,
        Err(e) if desapareceu(&e) => return Ok(None),
        Err(_) => anyhow::bail!("não foi possível ler credenciais de processo"),
    };
    ensure!(texto.len() <= 65_536, "credenciais de processo inválidas");
    let mut uids = None;
    let mut capacidades = false;
    for linha in texto.lines() {
        if let Some(resto) = linha.strip_prefix("Uid:") {
            let campos: Vec<u32> = resto.split_whitespace().map(str::parse).collect::<Result<_, _>>()?;
            ensure!(campos.len() == 4, "linha Uid inválida");
            uids = Some((campos[0], campos[1]));
        }
        for prefixo in ["CapPrm:", "CapEff:", "CapAmb:"] {
            if let Some(resto) = linha.strip_prefix(prefixo) {
                // Só as capabilities que o ajudante concede (CAP_SETGID=6, CAP_SETUID=7):
                // o gerenciador `systemd --user` do Hub tem CAP_WAKE_ALARM e não pode ser alvo.
                capacidades |=
                    u64::from_str_radix(resto.trim(), 16).context("capacidades inválidas")? & CAPS_AJUDANTE != 0;
            }
        }
    }
    let (real, efetivo) = uids.context("credenciais ausentes")?;
    Ok(Some(Credenciais { real, efetivo, capacidades }))
}

/// PIDs que executam o inode ou mantêm capabilities sob o UID do Hub.
/// Descritor aberto para o inode falha fechado: não é encerrado automaticamente.
fn varrer(alvo: &fs::Metadata, uid_hub: u32) -> Result<Vec<u32>> {
    let proprio = std::process::id();
    let mut processos = 0usize;
    let mut ofensores = Vec::new();
    for entrada in fs::read_dir("/proc")? {
        let entrada = entrada.context("falha ao enumerar processos")?;
        let Some(pid) = entrada.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        if pid == proprio {
            continue;
        }
        processos += 1;
        ensure!(processos <= MAX_PROCESSOS, "limite de processos excedido");
        let base = entrada.path();
        let executa = mesmo_inode(&base.join("exe"), alvo.dev(), alvo.ino())?;
        let privilegiado = credenciais(&base)?
            .is_some_and(|c| c.efetivo != 0 && (c.real == uid_hub || c.efetivo == uid_hub) && c.capacidades);
        if executa || privilegiado {
            ofensores.push(pid);
        }
        let descritores = match fs::read_dir(base.join("fd")) {
            Ok(entries) => entries,
            Err(e) if desapareceu(&e) => continue,
            Err(_) => anyhow::bail!("não foi possível verificar descritores de processo"),
        };
        let mut quantidade = 0usize;
        for descritor in descritores {
            let descritor = match descritor {
                Ok(d) => d,
                Err(e) if desapareceu(&e) => continue,
                Err(_) => anyhow::bail!("não foi possível enumerar descritores"),
            };
            quantidade += 1;
            ensure!(quantidade <= MAX_DESCRITORES, "limite de descritores excedido");
            ensure!(
                !mesmo_inode(&descritor.path(), alvo.dev(), alvo.ino())?,
                "descritor do ajudante anterior permanece aberto"
            );
        }
    }
    Ok(ofensores)
}

/// Encerra por pidfd somente o processo ainda ofensor depois de fixado o pidfd,
/// evitando sinalizar um PID reciclado.
fn encerrar(pid: u32, alvo: &fs::Metadata, uid_hub: u32) -> Result<()> {
    let pid_c = libc::pid_t::try_from(pid)?;
    // SAFETY: pidfd_open sem flags para PID numérico; retorno conferido.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid_c, 0) };
    if fd < 0 {
        let erro = io::Error::last_os_error();
        if desapareceu(&erro) {
            return Ok(());
        }
        anyhow::bail!("não foi possível fixar processo a encerrar");
    }
    // SAFETY: syscall retornou descritor novo e válido.
    let pidfd = unsafe { File::from_raw_fd(i32::try_from(fd)?) };
    let base = Path::new("/proc").join(pid.to_string());
    let executa = mesmo_inode(&base.join("exe"), alvo.dev(), alvo.ino())?;
    let privilegiado = credenciais(&base)?
        .is_some_and(|c| c.efetivo != 0 && (c.real == uid_hub || c.efetivo == uid_hub) && c.capacidades);
    if !(executa || privilegiado) {
        return Ok(());
    }
    // SAFETY: sinal fixo enviado pelo pidfd fixado acima.
    let enviado = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            pidfd.as_raw_fd(),
            libc::SIGKILL,
            std::ptr::null::<libc::siginfo_t>(),
            0u32,
        )
    };
    if enviado != 0 {
        let erro = io::Error::last_os_error();
        ensure!(desapareceu(&erro), "não foi possível encerrar processo privilegiado");
        return Ok(());
    }
    let mut poll = libc::pollfd { fd: pidfd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
    // SAFETY: estrutura válida; aguarda término do processo sinalizado.
    let pronto = unsafe { libc::poll(&raw mut poll, 1, 2000) };
    ensure!(pronto == 1, "processo privilegiado não terminou");
    Ok(())
}

/// Exige que o slot já esteja fechado ao Hub e as capabilities zeradas.
/// Encerra processos que ainda executam o inode ou mantêm capabilities sob o
/// UID do Hub e só retorna após duas varreduras limpas consecutivas.
pub fn verificar_sem_referencias(arquivo: &File, uid_hub: u32) -> Result<()> {
    ensure!(uid_hub != 0, "UID do Hub inválido");
    let alvo = arquivo.metadata()?;
    ensure!(alvo.is_file() && alvo.uid() == 0 && alvo.nlink() == 1, "inode anterior inseguro");
    // SAFETY: consulta de atributo em descritor válido e nome fixo.
    let caps =
        unsafe { libc::fgetxattr(arquivo.as_raw_fd(), c"security.capability".as_ptr(), std::ptr::null_mut(), 0) };
    ensure!(
        caps == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::ENODATA),
        "capabilities anteriores não foram removidas"
    );
    let proc = File::open("/proc")?;
    // SAFETY: memória válida para fstatfs e descritor de diretório aberto.
    let mut sistema: libc::statfs = unsafe { std::mem::zeroed() };
    ensure!(
        // SAFETY: descritor e ponteiro válidos.
        unsafe { libc::fstatfs(proc.as_raw_fd(), &raw mut sistema) } == 0 && sistema.f_type.cast_unsigned() == 0x9fa0,
        "procfs não confiável"
    );
    let mut limpas = 0usize;
    for _ in 0..MAX_PASSAGENS {
        let ofensores = varrer(&alvo, uid_hub)?;
        if ofensores.is_empty() {
            limpas += 1;
            if limpas >= 2 {
                return Ok(());
            }
        } else {
            limpas = 0;
            for pid in ofensores {
                encerrar(pid, &alvo, uid_hub)?;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    anyhow::bail!("processos privilegiados persistem após encerramento")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn so_setuid_ou_setgid_contam_como_privilegio_do_ajudante() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("mx365-proc-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir)?;
        let status = |caps: &str| {
            format!("Name:\tx\nUid:\t1000\t1000\t1000\t1000\nCapPrm:\t{caps}\nCapEff:\t{caps}\nCapAmb:\t{caps}\n")
        };
        // CAP_WAKE_ALARM (35), como o `systemd --user`: não é alvo.
        fs::write(dir.join("status"), status("0000000800000000"))?;
        assert!(!credenciais(&dir)?.context("processo")?.capacidades);
        fs::write(dir.join("status"), status("00000000000000c0"))?;
        assert!(credenciais(&dir)?.context("processo")?.capacidades);
        fs::write(dir.join("status"), status("0000000000000080"))?;
        assert!(credenciais(&dir)?.context("processo")?.capacidades);
        fs::remove_dir_all(dir)?;
        Ok(())
    }
}
