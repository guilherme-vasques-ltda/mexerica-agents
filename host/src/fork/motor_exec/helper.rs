use anyhow::{Context, Result, bail, ensure};
use std::{
    ffi::CString,
    fs::OpenOptions,
    io::Read,
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt},
        process::CommandExt,
    },
    path::Path,
};

#[path = "background.rs"]
mod background;
#[path = "fd.rs"]
mod fd;
#[path = "login_cleanup.rs"]
mod login_cleanup;
#[path = "profiles.rs"]
mod profiles;

const CONFIG: &str = "/etc/donnabot/motor.conf";

struct Identity {
    uid: u32,
    gid: u32,
    hub: u32,
    work: u32,
}

fn validate(identity: &Identity, caller: u32) -> Result<()> {
    ensure!(caller == identity.hub, "chamador diferente do UID do Hub");
    ensure!(identity.uid != 0 && identity.gid != 0 && identity.work != 0, "alvo/grupo root recusado");
    ensure!(identity.uid != identity.hub, "alvo igual ao Hub");
    // SAFETY: getgid só consulta o grupo primário real do chamador.
    let caller_gid = unsafe { libc::getgid() };
    ensure!(
        identity.gid != caller_gid && identity.work != caller_gid,
        "grupo primário do motor ou de trabalho igual ao grupo primário do Hub"
    );
    Ok(())
}

fn read_config(path: &Path) -> Result<String> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .context("abrindo configuração root do motor")?;
    let meta = file.metadata()?;
    ensure!(
        meta.is_file() && meta.uid() == 0 && meta.mode() & 0o7777 == 0o644,
        "configuração deve ser arquivo root 0644"
    );
    ensure!(meta.len() <= 4096, "configuração muito grande");
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(text)
}

fn identity(text: &str) -> Result<Identity> {
    let mut user = None;
    let mut hub = None;
    let mut work = None;
    for line in text.lines().map(str::trim).filter(|s| !s.is_empty() && !s.starts_with('#')) {
        let (key, value) = line.split_once('=').context("configuração inválida")?;
        match key {
            "motor_user" if user.is_none() => user = Some(CString::new(value)?),
            "hub_uid" if hub.is_none() => hub = Some(value.parse()?),
            "work_gid" if work.is_none() => work = Some(value.parse()?),
            _ => bail!("campo desconhecido ou duplicado na configuração"),
        }
    }
    let user = user.context("motor_user ausente")?;
    // SAFETY: string terminada em NUL; ajudante é single-thread, copiamos os IDs antes de outra consulta.
    let entry = unsafe { libc::getpwnam(user.as_ptr()) };
    ensure!(!entry.is_null(), "usuário do motor inexistente");
    // SAFETY: getpwnam retornou entrada válida e nenhuma outra consulta NSS ocorreu.
    let (uid, gid) = unsafe { ((*entry).pw_uid, (*entry).pw_gid) };
    Ok(Identity { uid, gid, hub: hub.context("hub_uid ausente")?, work: work.context("work_gid ausente")? })
}

fn check(rc: i32, operation: &str) -> Result<()> {
    if rc == -1 {
        return Err(std::io::Error::last_os_error()).context(operation.to_owned());
    }
    Ok(())
}

#[repr(C)]
struct CapHeader {
    version: u32,
    pid: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CapData {
    effective: u32,
    permitted: u32,
    inheritable: u32,
}

fn switch(identity: &Identity) -> Result<()> {
    // SAFETY: chamadas sem ponteiros compartilhados; todos os retornos são conferidos.
    unsafe {
        check(libc::setgroups(1, &raw const identity.work), "setgroups")?;
        check(libc::setresgid(identity.gid, identity.gid, identity.gid), "setresgid")?;
        check(libc::setresuid(identity.uid, identity.uid, identity.uid), "setresuid")?;
        let (mut r, mut e, mut s) = (0, 0, 0);
        check(libc::getresuid(&raw mut r, &raw mut e, &raw mut s), "getresuid")?;
        ensure!([r, e, s] == [identity.uid; 3], "UIDs não conferem");
        check(libc::getresgid(&raw mut r, &raw mut e, &raw mut s), "getresgid")?;
        ensure!([r, e, s] == [identity.gid; 3], "GIDs não conferem");
        let header = CapHeader { version: 0x2008_0522, pid: 0 };
        let data = [CapData::default(); 2];
        ensure!(
            libc::syscall(libc::SYS_capset, &raw const header, data.as_ptr()) == 0,
            "capset: {}",
            std::io::Error::last_os_error()
        );
        check(
            libc::prctl(libc::PR_CAP_AMBIENT, libc::PR_CAP_AMBIENT_CLEAR_ALL, 0, 0, 0),
            "limpando capacidades ambient",
        )?;
        check(libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0), "no_new_privs")?;
        check(libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0), "pdeathsig")?;
    }
    Ok(())
}

pub(super) fn run() -> Result<()> {
    // Capturar antes do NSS/troca; o PID pai é conferido após armar PDEATHSIG.
    // SAFETY: getters sem efeitos colaterais.
    let (caller, parent) = unsafe { (libc::getuid(), libc::getppid()) };
    let identity = identity(&read_config(Path::new(CONFIG))?)?;
    validate(&identity, caller)?;
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    switch(&identity)?;
    // SAFETY: getppid só consulta o pai atual.
    ensure!(unsafe { libc::getppid() } == parent, "Hub saiu durante a troca de usuário");
    match args.first().and_then(|a| a.to_str()) {
        Some("kill-group") if args.len() == 2 => {
            let pgid: i32 = args[1].to_str().context("grupo inválido")?.parse()?;
            ensure!(pgid > 1, "grupo inválido");
            kill(-pgid)
        }
        Some("cleanup") if args.len() == 1 => cleanup(identity.uid),
        Some("background") => background::run(&args[1..]),
        Some("conta-perfil") => profiles::run(&args[1..]),
        Some("read-file") if args.len() == 2 => read_file(Path::new(&args[1])),
        Some("--cwd") if args.len() >= 4 && args[2] == "--" => {
            let executable = Path::new(&args[3]);
            ensure!(executable.is_absolute(), "exec exige caminho absoluto");
            std::env::set_current_dir(&args[1]).context("cwd como motor")?;
            // SAFETY: só muda a máscara deste ajudante single-thread, após a troca.
            unsafe {
                libc::umask(if std::env::var_os("DONNABOT_MOTOR_PRIVATE").is_some() { 0o077 } else { 0o007 });
            }
            // Encaixe da F2b: aplicar Landlock aqui, após no_new_privs e antes do exec.
            Err(std::process::Command::new(executable).args(&args[4..]).exec()).context("exec do motor")
        }
        _ => bail!("uso: --cwd PASTA -- /caminho/programa [args] | kill-group PGID | cleanup"),
    }
}

fn kill(target: i32) -> Result<()> {
    // SAFETY: alvo validado; o kernel limita os sinais ao UID do motor sem capacidades.
    let rc = unsafe { libc::kill(target, libc::SIGKILL) };
    if rc == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        return Ok(());
    }
    check(rc, "kill do motor")
}

fn cleanup(uid: u32) -> Result<()> {
    kill(-1)?;
    // kill(-1) não inclui o próprio ajudante. Espera a saída (zumbis não executam).
    for _ in 0..100 {
        let mut alive = false;
        for entry in std::fs::read_dir("/proc")? {
            let entry = entry?;
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else { continue };
            if pid == std::process::id() {
                continue;
            }
            if let Ok(meta) = entry.metadata()
                && meta.uid() == uid
            {
                let status = match std::fs::read_to_string(entry.path().join("status")) {
                    Ok(status) => status,
                    Err(error) if matches!(error.raw_os_error(), Some(libc::ENOENT | libc::ESRCH)) => continue,
                    Err(error) => return Err(error.into()),
                };
                if !status.lines().any(|l| l.starts_with("State:\tZ") || l.starts_with("State:\tX")) {
                    alive = true;
                }
            }
        }
        if !alive {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        kill(-1)?;
    }
    bail!("processos do motor sobreviveram à limpeza")
}

fn read_file(path: &Path) -> Result<()> {
    use std::io::Write;
    const MAX: u64 = 100 * 1024 * 1024;
    let mut file = fd::Directory::open(path.parent().context("arquivo sem pai")?)?.file(
        path.file_name().context("arquivo sem nome")?,
        libc::O_RDONLY,
        0,
    )?;
    let before = file.metadata()?;
    ensure!(before.len() <= MAX, "arquivo excede 100 MiB");
    let mut output = std::io::stdout().lock();
    let count = std::io::copy(&mut (&mut file).take(MAX + 1), &mut output)?;
    let after = file.metadata()?;
    ensure!(
        count <= MAX
            && count == before.len()
            && after.len() == before.len()
            && after.modified()? == before.modified()?,
        "arquivo alterado durante leitura"
    );
    output.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recusa_fronteiras_de_identidade() {
        // SAFETY: getgid só consulta o grupo primário real do processo de teste.
        let caller_gid = unsafe { libc::getgid() };
        let gid = if caller_gid == 1002 { 1004 } else { 1002 };
        let work = if caller_gid == 1003 { 1005 } else { 1003 };
        let mut id = Identity { uid: 1002, gid, hub: 1001, work };
        assert!(validate(&id, 1004).is_err());
        assert!(validate(&id, 1001).is_ok());
        id.gid = caller_gid;
        assert!(validate(&id, 1001).is_err());
        id.gid = gid;
        id.work = caller_gid;
        assert!(validate(&id, 1001).is_err());
        id.work = work;
        id.uid = 0;
        assert!(validate(&id, 1001).is_err());
        id.uid = 1001;
        assert!(validate(&id, 1001).is_err());
    }
    #[test]
    fn recusa_configuracao_ausente_ou_insegura() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("motor-config-{}", std::process::id()));
        assert!(read_config(&path).is_err());
        std::fs::write(&path, "motor_user=nobody\nhub_uid=1\nwork_gid=2\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert!(read_config(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
