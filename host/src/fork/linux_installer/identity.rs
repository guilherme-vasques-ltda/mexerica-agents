//! Operações em árvores de outra conta rodam com a identidade de arquivos dela.
//! fsuid, fsgid e grupos são por thread no Linux: a thread dedicada morre com eles,
//! e nenhum caminho trocado ou link alcança o que essa conta não alcançaria.
use anyhow::{Context, Result, ensure};

const FS_CAPABILITIES: u64 = 0b1_1111;

pub(super) fn as_user<T: Send>(
    uid: u32,
    gid: u32,
    groups: &[u32],
    work: impl FnOnce() -> Result<T> + Send,
) -> Result<T> {
    ensure!(uid != 0 && gid != 0, "identidade de arquivos não pode ser root");
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let mut all = vec![gid];
                all.extend(groups.iter().copied().filter(|g| *g != gid));
                ensure!(!all.contains(&0), "grupo root recusado na identidade de arquivos");
                // SAFETY: syscalls diretas afetam só esta thread; buffer vivo com o tamanho informado.
                let rc =
                    unsafe { libc::syscall(libc::SYS_setgroups, libc::c_long::try_from(all.len())?, all.as_ptr()) };
                ensure!(rc == 0, "setgroups: {}", std::io::Error::last_os_error());
                // SAFETY: setfsgid/setfsuid por thread; -1 apenas consulta o valor vigente.
                let (group_now, user_now) = unsafe {
                    libc::syscall(libc::SYS_setfsgid, libc::c_long::from(gid));
                    libc::syscall(libc::SYS_setfsuid, libc::c_long::from(uid));
                    let query = libc::c_long::from(u32::MAX);
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
            })
            .join()
            .map_err(|_| anyhow::anyhow!("thread de identidade de arquivos falhou"))?
    })
}
