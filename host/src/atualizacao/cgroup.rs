//! Verificação de cgroup v2 preparado pelo administrador.
use anyhow::{Result, ensure};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::Path,
};

pub struct Limites {
    pub memoria: u64,
    pub processos: u64,
    pub cpu_quota: u64,
    pub cpu_periodo: u64,
}

fn abrir(base: &Path, nome: &str, escrita: bool) -> Result<File> {
    let file = OpenOptions::new()
        .read(!escrita)
        .write(escrita)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(base.join(nome))?;
    let m = file.metadata()?;
    ensure!(m.is_file() && m.uid() == 0 && m.gid() == 0 && m.mode() & 0o022 == 0, "controle de cgroup inseguro");
    Ok(file)
}
fn ler(base: &Path, nome: &str) -> Result<String> {
    let mut texto = String::new();
    abrir(base, nome, false)?.take(4097).read_to_string(&mut texto)?;
    ensure!(texto.len() <= 4096, "controle de cgroup grande demais");
    Ok(texto.trim().into())
}

/// Este caminho fixo precisa ter sido configurado por root, nunca pelo Hub.
/// A associação não habilita a publicação nem comprova os demais gates.
pub fn entrar(limites: &Limites) -> Result<()> {
    ensure!(
        limites.memoria > 0 && limites.processos > 0 && limites.cpu_quota > 0 && limites.cpu_periodo > 0,
        "limites de cgroup inválidos"
    );
    let base = Path::new("/sys/fs/cgroup/donnabot-atualizacao");
    for path in [Path::new("/sys"), Path::new("/sys/fs"), Path::new("/sys/fs/cgroup"), base] {
        let m = std::fs::symlink_metadata(path)?;
        ensure!(m.is_dir() && m.uid() == 0 && m.gid() == 0 && m.mode() & 0o022 == 0, "caminho de cgroup inseguro");
    }
    let dir = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open(base)?;
    // SAFETY: ponteiro válido para estrutura preenchida por fstatfs.
    let mut sistema: libc::statfs = unsafe { std::mem::zeroed() };
    ensure!(
        // SAFETY: descritor e ponteiro válidos.
        unsafe { libc::fstatfs(dir.as_raw_fd(), &raw mut sistema) } == 0
            && sistema.f_type.cast_unsigned() == 0x6367_7270,
        "cgroup v2 exigido"
    );
    ensure!(ler(base, "memory.max")? == limites.memoria.to_string(), "memory.max divergente");
    ensure!(ler(base, "pids.max")? == limites.processos.to_string(), "pids.max divergente");
    ensure!(ler(base, "cpu.max")? == format!("{} {}", limites.cpu_quota, limites.cpu_periodo), "cpu.max divergente");
    // SAFETY: getpid não altera estado.
    let pid = unsafe { libc::getpid() };
    // Uma única escrita: o kernel interpreta cada write(2) isoladamente e
    // recusa com EINVAL um "\n" enviado separado do PID.
    abrir(base, "cgroup.procs", true)?.write_all(format!("{pid}\n").as_bytes())?;
    let mut associacao = String::new();
    File::open("/proc/self/cgroup")?.take(4097).read_to_string(&mut associacao)?;
    ensure!(
        associacao.len() <= 4096 && associacao.lines().any(|l| l == "0::/donnabot-atualizacao"),
        "associação de cgroup não confirmada"
    );
    ensure!(
        ler(base, "memory.max")? == limites.memoria.to_string()
            && ler(base, "pids.max")? == limites.processos.to_string()
            && ler(base, "cpu.max")? == format!("{} {}", limites.cpu_quota, limites.cpu_periodo),
        "limites de cgroup mudaram"
    );
    Ok(())
}
