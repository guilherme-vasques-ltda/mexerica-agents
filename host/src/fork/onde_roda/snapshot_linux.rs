//! Guarda de processo Linux `x86_64`; sem fallback e sem dependência de namespaces.
use std::fs::OpenOptions;
use std::io;
use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd, RawFd};
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _, OpenOptionsExt as _};
use std::os::unix::process::CommandExt as _;
use std::process::Command;

fn statement(code: u16, k: u32) -> libc::sock_filter {
    libc::sock_filter { code, jt: 0, jf: 0, k }
}
fn jump(k: u32, jt: u8, jf: u8) -> libc::sock_filter {
    libc::sock_filter { code: 0x15, jt, jf, k } // BPF_JMP | BPF_JEQ | BPF_K
}

fn filter() -> Vec<libc::sock_filter> {
    let deny = 0x0005_0000 | u32::try_from(libc::EPERM).unwrap_or(1);
    let mut bpf = vec![
        statement(0x20, 4),           // seccomp_data.arch
        jump(0xc000_003e, 1, 0),      // AUDIT_ARCH_X86_64
        statement(0x06, 0x8000_0000), // KILL_PROCESS em ABI desconhecida
        statement(0x20, 0),           // seccomp_data.nr; x32 não pertence à allowlist
    ];
    // O_RDWR exige Landlock instalado: escrita só no objeto /dev/null conferido.
    // Não permitir openat2: flags ficam em ponteiro.
    for (nr, offset) in [(libc::SYS_open, 24), (libc::SYS_openat, 32)] {
        bpf.push(jump(u32::try_from(nr).unwrap_or(u32::MAX), 0, 5));
        bpf.push(statement(0x20, offset));
        bpf.push(statement(
            0x54,
            u32::try_from(
                libc::O_WRONLY
                    | libc::O_CREAT
                    | libc::O_TRUNC
                    | libc::O_APPEND
                    | (libc::O_TMPFILE & !libc::O_DIRECTORY),
            )
            .unwrap_or(u32::MAX),
        ));
        bpf.push(jump(0, 1, 0));
        bpf.push(statement(0x06, deny));
        bpf.push(statement(0x06, 0x7fff_0000)); // ALLOW
    }
    // argv/config próprios selecionam só builtins; execve é necessário ao exec inicial.
    // Não é sandbox contra exploração do Git/loader. Rede, descendentes, io_uring,
    // ptrace e mutações de filesystem não fazem parte desta lista.
    for nr in [
        libc::SYS_read,
        libc::SYS_write,
        libc::SYS_readv,
        libc::SYS_writev,
        libc::SYS_close,
        libc::SYS_fstat,
        libc::SYS_newfstatat,
        libc::SYS_stat,
        libc::SYS_lstat,
        libc::SYS_statx,
        libc::SYS_lseek,
        libc::SYS_pread64,
        libc::SYS_mmap,
        libc::SYS_mprotect,
        libc::SYS_munmap,
        libc::SYS_brk,
        libc::SYS_mremap,
        libc::SYS_madvise,
        libc::SYS_rt_sigaction,
        libc::SYS_rt_sigprocmask,
        libc::SYS_rt_sigreturn,
        libc::SYS_sigaltstack,
        libc::SYS_access,
        libc::SYS_faccessat,
        libc::SYS_faccessat2,
        libc::SYS_getcwd,
        libc::SYS_chdir,
        libc::SYS_readlink,
        libc::SYS_readlinkat,
        libc::SYS_getdents64,
        libc::SYS_fcntl,
        libc::SYS_dup,
        libc::SYS_dup2,
        libc::SYS_dup3,
        libc::SYS_poll,
        libc::SYS_ppoll,
        libc::SYS_select,
        libc::SYS_pselect6,
        libc::SYS_futex,
        libc::SYS_arch_prctl,
        libc::SYS_set_tid_address,
        libc::SYS_set_robust_list,
        libc::SYS_rseq,
        libc::SYS_getpid,
        libc::SYS_getppid,
        libc::SYS_gettid,
        libc::SYS_getuid,
        libc::SYS_geteuid,
        libc::SYS_getgid,
        libc::SYS_getegid,
        libc::SYS_getrandom,
        libc::SYS_clock_gettime,
        libc::SYS_gettimeofday,
        libc::SYS_time,
        libc::SYS_getrlimit,
        libc::SYS_prlimit64,
        libc::SYS_getrusage,
        libc::SYS_uname,
        libc::SYS_sysinfo,
        libc::SYS_sched_getaffinity,
        libc::SYS_execve,
        libc::SYS_exit,
        libc::SYS_exit_group,
    ] {
        bpf.push(jump(u32::try_from(nr).unwrap_or(u32::MAX), 0, 1));
        bpf.push(statement(0x06, 0x7fff_0000));
    }
    bpf.push(statement(0x06, deny));
    bpf
}

// UAPI mínima: WRITE_FILE existe desde ABI 1; nenhum direito de diretório.
const WRITE_FILE: u64 = 1 << 1;

#[repr(C)]
struct RulesetAttr {
    handled_access_fs: u64,
}

#[repr(C, packed)]
struct PathRule {
    allowed_access: u64,
    parent_fd: i32,
}

struct Prepared {
    ruleset: OwnedFd,
    bpf: Vec<libc::sock_filter>,
}

impl Prepared {
    fn new() -> io::Result<Self> {
        let null = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open("/dev/null")?;
        let metadata = null.metadata()?;
        if !metadata.file_type().is_char_device() || metadata.rdev() != libc::makedev(1, 3) {
            return Err(io::Error::from_raw_os_error(libc::ENODEV));
        }
        let attr = RulesetAttr { handled_access_fs: WRITE_FILE };
        let rule = PathRule { allowed_access: WRITE_FILE, parent_fd: null.as_raw_fd() };
        // SAFETY: atributos locais válidos; syscalls e FDs checados antes de transferir posse.
        // Tudo no pai, inclusive alocações; a regra retém o objeto exato após fechar null.
        let ruleset = unsafe {
            let abi = libc::syscall(
                libc::SYS_landlock_create_ruleset,
                std::ptr::null::<RulesetAttr>(),
                0_usize,
                1_u32, // LANDLOCK_CREATE_RULESET_VERSION
            );
            if abi < 0 {
                return Err(io::Error::last_os_error());
            }
            if abi < 1 {
                return Err(io::Error::from_raw_os_error(libc::ENOSYS));
            }
            let fd = libc::syscall(libc::SYS_landlock_create_ruleset, &raw const attr, size_of::<RulesetAttr>(), 0_u32);
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            let fd = i32::try_from(fd).map_err(|_| io::Error::from_raw_os_error(libc::EOVERFLOW))?;
            let ruleset = OwnedFd::from_raw_fd(fd);
            if libc::syscall(libc::SYS_landlock_add_rule, fd, 1_u32, &raw const rule, 0_u32) != 0 {
                return Err(io::Error::last_os_error());
            }
            ruleset
        };
        Ok(Self { ruleset, bpf: filter() })
    }

    fn install(&self) -> io::Result<()> {
        install(&self.bpf, self.ruleset.as_raw_fd())
    }
}

fn install(bpf: &[libc::sock_filter], ruleset: RawFd) -> io::Result<()> {
    // SAFETY: só syscalls e memória previamente alocada; válido também em pre_exec.
    unsafe {
        if libc::syscall(libc::SYS_close_range, 3_u32, u32::MAX, 4_u32) != 0 {
            return Err(io::Error::last_os_error()); // CLOSE_RANGE_CLOEXEC
        }
        for (resource, value) in [
            (libc::RLIMIT_AS, 512 * 1024 * 1024),
            (libc::RLIMIT_CPU, 2),
            (libc::RLIMIT_CORE, 0),
            (libc::RLIMIT_NOFILE, 128),
        ] {
            let limit = libc::rlimit { rlim_cur: value, rlim_max: value };
            if libc::setrlimit(resource, &raw const limit) != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
            return Err(io::Error::last_os_error());
        }
        // Sem esta restrição, a variante BPF que admite O_RDWR nunca é instalada.
        if libc::syscall(libc::SYS_landlock_restrict_self, ruleset, 0_u32) != 0 {
            return Err(io::Error::last_os_error());
        }
        let program = libc::sock_fprog {
            len: u16::try_from(bpf.len()).map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?,
            filter: bpf.as_ptr().cast_mut(),
        };
        if libc::prctl(libc::PR_SET_SECCOMP, 2, &raw const program) != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

pub(super) fn command(command: &mut Command) -> io::Result<()> {
    let prepared = Prepared::new()?; // FDs, attrs e BPF preparados antes do fork
    // SAFETY: install só usa syscalls e memória já alocada; nenhum lock ou allocator.
    unsafe {
        command.pre_exec(move || prepared.install());
    }
    Ok(())
}

pub(super) fn nonblocking(fd: RawFd) -> io::Result<()> {
    // SAFETY: fcntl opera somente no descritor vivo do pipe de propriedade do chamador.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    // Filtro real em filho descartável; depois do fork não há alocação/lock Rust.
    // Não executar a bateria libtest dentro da guarda (ela cria threads).
    #[test]
    fn guarda_real_diretorio_leitura_tmpfile_rede_e_descendentes() {
        let mut fixture = super::super::odb::Private::create().unwrap();
        let paths = [fixture.git.join("config"), fixture.git.join("objects/probe")];
        for path in &paths {
            std::fs::write(path, b"sentinela-inalterada").unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
            // Controle sem guarda: DAC permite O_RDWR ao mesmo UID.
            drop(OpenOptions::new().read(true).write(true).open(path).unwrap());
        }
        let names = paths.each_ref().map(|path| std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap());
        let prepared = Prepared::new().expect("preparar Landlock e seccomp sem fallback");
        // SAFETY: filho só instala guarda, faz syscalls de prova e _exit; pai faz reap.
        unsafe {
            let pid = libc::fork();
            assert!(pid >= 0);
            if pid == 0 {
                if prepared.install().is_err() {
                    libc::_exit(120);
                }
                let null = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR | libc::O_CLOEXEC);
                if null < 0 {
                    libc::_exit(7);
                }
                let mut byte = [0_u8];
                if libc::write(null, b"x".as_ptr().cast(), 1) != 1 || libc::read(null, byte.as_mut_ptr().cast(), 1) != 0
                {
                    libc::_exit(8);
                }
                libc::close(null);
                for name in &names {
                    let fd = libc::open(name.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC);
                    if fd < 0 {
                        libc::_exit(9);
                    }
                    // FD somente leitura não permite write nem mmap compartilhado gravável.
                    if libc::write(fd, b"x".as_ptr().cast(), 1) != -1 || *libc::__errno_location() != libc::EBADF {
                        libc::_exit(10);
                    }
                    if libc::mmap(std::ptr::null_mut(), 4096, libc::PROT_WRITE, libc::MAP_SHARED, fd, 0)
                        != libc::MAP_FAILED
                        || *libc::__errno_location() != libc::EACCES
                    {
                        libc::_exit(11);
                    }
                    libc::close(fd);
                    // BPF admite estes flags; EACCES é a negação WRITE_FILE do Landlock.
                    if libc::open(name.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) >= 0
                        || *libc::__errno_location() != libc::EACCES
                    {
                        libc::_exit(12);
                    }
                }
                let fd = libc::open(c"/tmp".as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY);
                if fd < 0 {
                    libc::_exit(1);
                }
                libc::close(fd);
                if libc::open(c"/tmp".as_ptr(), libc::O_TMPFILE | libc::O_RDWR, 0o600) >= 0
                    || *libc::__errno_location() != libc::EPERM
                {
                    libc::_exit(2);
                }
                // Bit específico de temporário também recusa sem O_RDWR.
                if libc::open(c"/tmp".as_ptr(), libc::O_TMPFILE, 0o600) >= 0 || *libc::__errno_location() != libc::EPERM
                {
                    libc::_exit(3);
                }
                if libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) >= 0 || *libc::__errno_location() != libc::EPERM {
                    libc::_exit(4);
                }
                if libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) >= 0 || *libc::__errno_location() != libc::EPERM {
                    libc::_exit(5);
                }
                let descendant = libc::syscall(libc::SYS_fork);
                if descendant == 0 {
                    libc::_exit(121);
                }
                if descendant > 0 || *libc::__errno_location() != libc::EPERM {
                    libc::_exit(6);
                }
                libc::_exit(0);
            }
            let mut status = 0;
            assert_eq!(libc::waitpid(pid, &raw mut status, 0), pid);
            assert!(libc::WIFEXITED(status));
            assert_eq!(libc::WEXITSTATUS(status), 0, "guarda não comprovada; sem fallback");
        }
        for path in &paths {
            assert_eq!(std::fs::read(path).unwrap(), b"sentinela-inalterada");
        }
        fixture.cleanup().unwrap();
    }

    fn evaluate(arch: u32, syscall: u32, flags: u32) -> u32 {
        let program = filter();
        let mut pc = 0;
        let mut a = 0;
        loop {
            let instruction = &program[pc];
            pc += 1;
            match instruction.code {
                0x20 => {
                    a = match instruction.k {
                        0 => syscall,
                        4 => arch,
                        24 | 32 => flags,
                        _ => panic!("offset"),
                    }
                }
                0x54 => a &= instruction.k,
                0x15 => pc += usize::from(if a == instruction.k { instruction.jt } else { instruction.jf }),
                0x06 => return instruction.k,
                _ => panic!("instrução"),
            }
        }
    }

    #[test]
    fn bpf_recusa_abi_x32_arch_e_flags_sem_confundir_directory() {
        let arch = 0xc000_003e;
        let open = u32::try_from(libc::SYS_openat).unwrap();
        assert_eq!(evaluate(arch, open, u32::try_from(libc::O_DIRECTORY).unwrap()), 0x7fff_0000);
        // BPF permite O_RDWR; negação por objeto é exercitada no teste da guarda real.
        assert_eq!(evaluate(arch, open, u32::try_from(libc::O_RDWR).unwrap()), 0x7fff_0000);
        for flags in [libc::O_WRONLY, libc::O_CREAT, libc::O_TRUNC, libc::O_APPEND, libc::O_TMPFILE] {
            assert_eq!(evaluate(arch, open, u32::try_from(flags).unwrap()) & 0xffff_0000, 0x0005_0000);
        }
        assert_eq!(evaluate(0, open, 0), 0x8000_0000);
        assert_eq!(evaluate(arch, open | 0x4000_0000, 0) & 0xffff_0000, 0x0005_0000);
    }
}
