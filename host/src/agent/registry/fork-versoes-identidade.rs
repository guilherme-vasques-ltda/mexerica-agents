use uuid::Uuid;

pub(super) enum Processo {
    Vivo(u64),
    Ausente,
    Incerto,
}

#[cfg(target_os = "linux")]
pub(super) fn boot() -> Option<Uuid> {
    Uuid::parse_str(std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?.trim()).ok()
}

#[cfg(target_os = "macos")]
pub(super) fn boot() -> Option<Uuid> {
    use std::ffi::CStr;

    let mut valor = [0u8; 37];
    let mut tamanho = valor.len();
    // O UUID da sessão não muda quando um ajuste do relógio altera kern.boottime.
    // SAFETY: o buffer e seu tamanho são válidos; a chamada só consulta o sysctl.
    let resultado = unsafe {
        libc::sysctlbyname(
            c"kern.bootsessionuuid".as_ptr(),
            valor.as_mut_ptr().cast(),
            &mut tamanho,
            std::ptr::null_mut(),
            0,
        )
    };
    if resultado != 0 || tamanho != valor.len() {
        return None;
    }
    Uuid::parse_str(CStr::from_bytes_with_nul(&valor).ok()?.to_str().ok()?).ok()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(super) fn boot() -> Option<Uuid> {
    None
}

#[cfg(unix)]
pub(super) fn processo(pid: u32) -> Processo {
    let Ok(pid) = libc::pid_t::try_from(pid) else { return Processo::Incerto };
    if pid <= 0 {
        return Processo::Incerto;
    }
    // SAFETY: sinal 0 só consulta a existência deste PID, nunca envia um sinal.
    if unsafe { libc::kill(pid, 0) } == -1 {
        return match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ESRCH) => Processo::Ausente,
            _ => Processo::Incerto,
        };
    }
    inicio(pid).map_or(Processo::Incerto, Processo::Vivo)
}

#[cfg(target_os = "linux")]
fn inicio(pid: libc::pid_t) -> Option<u64> {
    let stat = std::fs::read(format!("/proc/{pid}/stat")).ok()?;
    let fim_nome = stat.iter().rposition(|&b| b == b')')?;
    // A coluna 22 se refere ao início do processo em ticks desde o boot.
    let campo = stat[fim_nome + 1..].split(u8::is_ascii_whitespace).filter(|v| !v.is_empty()).nth(19)?;
    std::str::from_utf8(campo).ok()?.parse().ok()
}

#[cfg(target_os = "macos")]
fn inicio(pid: libc::pid_t) -> Option<u64> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let tamanho = libc::c_int::try_from(size_of::<libc::proc_bsdinfo>()).ok()?;
    // SAFETY: proc_pidinfo preenche um proc_bsdinfo quando retorna o tamanho completo.
    let info = unsafe {
        if libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, info.as_mut_ptr().cast(), tamanho) != tamanho {
            return None;
        }
        info.assume_init()
    };
    info.pbi_start_tvsec.checked_mul(1_000_000)?.checked_add(info.pbi_start_tvusec)
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn inicio(_: libc::pid_t) -> Option<u64> {
    None
}

#[cfg(not(any(unix, windows)))]
pub(super) fn processo(_: u32) -> Processo {
    Processo::Incerto
}

#[cfg(windows)]
mod win {
    use super::{Processo, Uuid};
    use std::ffi::c_void;
    use std::os::windows::io::{FromRawHandle, OwnedHandle};

    #[repr(C)]
    struct BootEnvironment {
        id: [u8; 16],
        firmware: u32,
        flags: u64,
    }

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtQuerySystemInformation(classe: u32, buffer: *mut c_void, tamanho: u32, escrito: *mut u32) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(acesso: u32, herdar: i32, pid: u32) -> *mut c_void;
        fn WaitForSingleObject(handle: *mut c_void, milissegundos: u32) -> u32;
        fn GetProcessTimes(
            processo: *mut c_void,
            inicio: *mut [u32; 2],
            saida: *mut [u32; 2],
            kernel: *mut [u32; 2],
            usuario: *mut [u32; 2],
        ) -> i32;
    }

    pub(in super::super) fn boot() -> Option<Uuid> {
        const SYSTEM_BOOT_ENVIRONMENT_INFORMATION: u32 = 90;
        let mut ambiente = BootEnvironment { id: [0; 16], firmware: 0, flags: 0 };
        let mut escrito = 0;
        let tamanho = u32::try_from(size_of::<BootEnvironment>()).ok()?;
        // SAFETY: o buffer C tem o tamanho declarado para a consulta.
        let status = unsafe {
            NtQuerySystemInformation(
                SYSTEM_BOOT_ENVIRONMENT_INFORMATION,
                (&raw mut ambiente).cast(),
                tamanho,
                &mut escrito,
            )
        };
        let id = Uuid::from_bytes(ambiente.id);
        (status == 0 && escrito >= 16 && !id.is_nil()).then_some(id)
    }

    pub(in super::super) fn processo(pid: u32) -> Processo {
        const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
        const SYNCHRONIZE: u32 = 0x0010_0000;
        const WAIT_OBJECT_0: u32 = 0;
        const WAIT_TIMEOUT: u32 = 258;
        if pid == 0 {
            return Processo::Incerto;
        }
        // SAFETY: solicita apenas consulta/espera e impede herança do handle.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, 0, pid) };
        if handle.is_null() {
            return Processo::Incerto;
        }
        // SAFETY: OpenProcess retornou um handle novo e válido.
        let _handle = unsafe { OwnedHandle::from_raw_handle(handle) };
        // SAFETY: o handle continua aberto; espera de zero milissegundos não bloqueia.
        match unsafe { WaitForSingleObject(handle, 0) } {
            WAIT_OBJECT_0 => return Processo::Ausente,
            WAIT_TIMEOUT => (),
            _ => return Processo::Incerto,
        }
        let (mut inicio, mut saida, mut kernel, mut usuario) = ([0; 2], [0; 2], [0; 2], [0; 2]);
        // SAFETY: o handle continua aberto e cada buffer tem o tamanho e alinhamento de um FILETIME.
        if unsafe { GetProcessTimes(handle, &mut inicio, &mut saida, &mut kernel, &mut usuario) } == 0 {
            return Processo::Incerto;
        }
        Processo::Vivo(u64::from(inicio[1]) << 32 | u64::from(inicio[0]))
    }

    #[cfg(test)]
    #[test]
    fn reserva_windows_so_confirma_saida_com_handle_sinalizado() {
        assert!(matches!(processo(std::process::id()), Processo::Vivo(_)));
        let mut filho = std::process::Command::new("cmd.exe").args(["/D", "/C", "exit", "0"]).spawn().unwrap();
        assert!(filho.wait().unwrap().success());
        assert!(matches!(processo(filho.id()), Processo::Ausente));
        assert!(matches!(processo(0), Processo::Incerto));
    }
}

#[cfg(windows)]
pub(super) use win::{boot, processo};
