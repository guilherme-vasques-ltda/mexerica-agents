//! Operações em árvores de outra conta rodam com a identidade de arquivos dela.
//! fsuid, fsgid e grupos são por thread no Linux: a thread dedicada morre com eles,
//! e nenhum caminho trocado ou link alcança o que essa conta não alcançaria.
//! O trabalho passado não cria processos nem threads: exec devolveria as credenciais de root.
use anyhow::{Context, Result, ensure};

#[cfg(not(target_pointer_width = "64"))]
compile_error!("identidade de arquivos exige Linux 64 bits: SYS_setgroups e c_long com IDs de 32 bits");

const FS_CAPABILITIES: u64 = 0b1_1111 | 1 << 32;

/// Grupos da conta como o login os inicializa; o grupo root é recusado por `as_user`.
pub(super) fn user_groups(name: &std::ffi::OsStr, gid: u32) -> Result<Vec<u32>> {
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(name.as_bytes())?;
    let mut groups = vec![0u32; 64];
    loop {
        let mut count = libc::c_int::try_from(groups.len())?;
        // SAFETY: nome terminado em NUL; buffer vivo com a capacidade informada em count.
        let rc = unsafe { libc::getgrouplist(name.as_ptr(), gid, groups.as_mut_ptr(), &raw mut count) };
        let count = usize::try_from(count)?;
        if rc >= 0 {
            ensure!(count <= groups.len(), "getgrouplist devolveu tamanho inválido");
            groups.truncate(count);
            return Ok(groups);
        }
        ensure!(count > groups.len() && count <= 65536, "getgrouplist falhou");
        groups.resize(count, 0);
    }
}

pub(super) fn as_user<T: Send>(
    uid: u32,
    gid: u32,
    groups: &[u32],
    work: impl FnOnce() -> Result<T> + Send,
) -> Result<T> {
    ensure!(uid != 0 && gid != 0, "identidade de arquivos não pode ser root");
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .spawn_scoped(scope, || {
                let mut all = vec![gid];
                all.extend(groups.iter().copied().filter(|g| *g != gid));
                all[1..].sort_unstable();
                all.dedup();
                ensure!(!all.contains(&0), "grupo root recusado na identidade de arquivos");
                // SAFETY: syscalls diretas afetam só esta thread; buffer vivo com o tamanho informado.
                let rc =
                    unsafe { libc::syscall(libc::SYS_setgroups, libc::c_long::try_from(all.len())?, all.as_ptr()) };
                ensure!(rc == 0, "setgroups: {}", std::io::Error::last_os_error());
                // SAFETY: setfsgid/setfsuid por thread; -1 apenas consulta o valor vigente.
                let (group_now, user_now) = unsafe {
                    libc::syscall(libc::SYS_setfsgid, libc::c_long::from(gid));
                    libc::syscall(libc::SYS_setfsuid, libc::c_long::from(uid));
                    let query: libc::c_long = -1;
                    (libc::syscall(libc::SYS_setfsgid, query), libc::syscall(libc::SYS_setfsuid, query))
                };
                ensure!(
                    group_now == libc::c_long::from(gid) && user_now == libc::c_long::from(uid),
                    "identidade de arquivos não aplicada"
                );
                let status = std::fs::read_to_string("/proc/thread-self/status")?;
                let effective =
                    status.lines().find_map(|line| line.strip_prefix("CapEff:")).context("CapEff ausente")?.trim();
                let effective = u64::from_str_radix(effective, 16)?;
                ensure!(effective & FS_CAPABILITIES == 0, "capacidades de arquivo continuam ativas");
                work()
            })?
            .join()
            .map_err(|_| anyhow::anyhow!("thread de identidade de arquivos falhou"))?
    })
}
