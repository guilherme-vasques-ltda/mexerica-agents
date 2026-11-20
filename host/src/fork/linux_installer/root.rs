use super::{Migration, RootAction, migration};
use anyhow::{Context, Result, bail, ensure};
use std::{
    ffi::{CStr, OsStr, OsString},
    fs::{self, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Component, Path, PathBuf},
    process::{Command, Output},
};

pub(super) const BASE: &str = "/var/lib/donnabot";
pub(super) use crate::fork::motor_roots::{MOTOR, WORK};
pub(super) const BIN: &str = "/opt/donnabot/bin";
pub(super) const CONFIG: &str = "/etc/donnabot/motor.conf";
const MOTOR_USER: &str = "donnabot-motor";
const WORK_GROUP: &str = "donnabot-trabalho";

pub(super) struct User {
    pub uid: u32,
    pub gid: u32,
    pub name: OsString,
    pub home: PathBuf,
    shell: PathBuf,
}

pub(super) fn user(uid: u32) -> Result<User> {
    // SAFETY: etapa administrativa monothread; copia os campos antes da próxima chamada NSS.
    let entry = unsafe { libc::getpwuid(uid) };
    ensure!(!entry.is_null(), "usuário {uid} inexistente");
    // SAFETY: entrada não nula, campos C válidos mantidos pelo NSS.
    unsafe {
        use std::os::unix::ffi::OsStrExt;
        Ok(User {
            uid,
            gid: (*entry).pw_gid,
            name: OsStr::from_bytes(CStr::from_ptr((*entry).pw_name).to_bytes()).to_owned(),
            home: PathBuf::from(OsStr::from_bytes(CStr::from_ptr((*entry).pw_dir).to_bytes())),
            shell: PathBuf::from(OsStr::from_bytes(CStr::from_ptr((*entry).pw_shell).to_bytes())),
        })
    }
}

pub(super) fn system(name: &str) -> Result<PathBuf> {
    ensure!(!name.contains('/'), "comando de sistema inválido");
    for base in ["/usr/sbin", "/usr/bin", "/sbin", "/bin"] {
        let path = Path::new(base).join(name);
        if let Ok(meta) = fs::metadata(&path) {
            ensure!(
                meta.is_file() && meta.uid() == 0 && meta.mode() & 0o022 == 0,
                "comando de sistema inseguro: {}",
                path.display()
            );
            return Ok(path);
        }
    }
    bail!("requisito ausente: {name}; instale systemd, shadow/passwd e libcap2-bin")
}

pub(super) fn checked(command: &mut Command) -> Result<Output> {
    let out = command.output().context("executando comando do instalador")?;
    // Não imprimir stdout (pode conter inventário privado); stderr é erro da ferramenta.
    ensure!(
        out.status.success(),
        "comando {} falhou: {}",
        command.get_program().to_string_lossy(),
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(out)
}

pub(super) fn user_command(hub: &User, program: impl AsRef<OsStr>) -> Result<Command> {
    let mut command = Command::new(system("runuser")?);
    command.args([OsStr::new("-u"), &hub.name, OsStr::new("--")]);
    command
        .env_clear()
        .env("HOME", &hub.home)
        .env("USER", &hub.name)
        .env("PATH", "/opt/donnabot/bin:/usr/local/bin:/usr/bin:/bin")
        .env("XDG_RUNTIME_DIR", format!("/run/user/{}", hub.uid))
        .env("DBUS_SESSION_BUS_ADDRESS", format!("unix:path=/run/user/{}/bus", hub.uid))
        .arg(program);
    Ok(command)
}

pub(super) fn unit(hub: &User) -> PathBuf {
    hub.home.join(".config/systemd/user/donnabot-host.service")
}

pub(super) fn service(hub: &User, args: &[&str]) -> Result<()> {
    checked(user_command(hub, system("systemctl")?)?.arg("--user").args(args))?;
    Ok(())
}

pub(super) fn safe(path: &Path) -> Result<()> {
    ensure!(
        path.is_absolute() && !path.components().any(|c| matches!(c, Component::ParentDir)),
        "caminho absoluto sem .. exigido"
    );
    let mut current = PathBuf::from("/");
    for part in path.components().skip(1) {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(meta) => ensure!(!meta.file_type().is_symlink(), "link recusado: {}", current.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

pub(super) fn owner_mode(path: &Path, uid: u32, gid: u32, mode: u32) -> Result<()> {
    safe(path)?;
    let file = OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(path)?;
    // SAFETY: descritor aberto sem seguir link; IDs conferidos pelo instalador.
    ensure!(unsafe { libc::fchown(file.as_raw_fd(), uid, gid) } == 0, "fchown: {}", std::io::Error::last_os_error());
    file.set_permissions(fs::Permissions::from_mode(mode))?;
    Ok(())
}

pub(super) fn directory(path: &Path, uid: u32, gid: u32, mode: u32) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    safe(path)?;
    if !path.exists() {
        fs::DirBuilder::new().mode(mode).create(path)?;
    }
    ensure!(path.is_dir(), "diretório exigido: {}", path.display());
    owner_mode(path, uid, gid, mode)
}

pub(super) fn write_atomic(path: &Path, bytes: &[u8], uid: u32, gid: u32, mode: u32) -> Result<()> {
    use std::io::Write;
    safe(path)?;
    let temp = path.with_file_name(format!(".donna-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        owner_mode(&temp, uid, gid, mode)?;
        fs::rename(&temp, path)?;
        Ok(())
    })();
    let _ = fs::remove_file(temp);
    result
}

pub(super) fn place(source: &Path, target: &Path, uid: u32, gid: u32, mode: u32) -> Result<()> {
    safe(source)?;
    let input = OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(source)?;
    ensure!(input.metadata()?.is_file(), "arquivo regular exigido: {}", source.display());
    copy_into(input, target, uid, gid, mode)
}

fn copy_into(mut input: fs::File, target: &Path, uid: u32, gid: u32, mode: u32) -> Result<()> {
    safe(target)?;
    let temp = target.with_file_name(format!(".donna-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut output = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temp)?;
        std::io::copy(&mut input, &mut output)?;
        output.sync_all()?;
        owner_mode(&temp, uid, gid, mode)?;
        fs::rename(&temp, target)?;
        Ok(())
    })();
    let _ = fs::remove_file(temp);
    result
}

/// Origem de um binário do pacote, aberta com a identidade de arquivos do Hub: o root nunca
/// copia o que essa conta não lê.
fn open_as_hub(hub: &User, source: &Path) -> Result<fs::File> {
    let groups = super::identity::user_groups(&hub.name, hub.gid)?;
    super::identity::as_user(hub.uid, hub.gid, &groups, || {
        safe(source)?;
        let file = OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(source)?;
        ensure!(file.metadata()?.is_file(), "arquivo regular exigido: {}", source.display());
        Ok(file)
    })
    .with_context(|| format!("a conta do Hub não lê {}", source.display()))
}

/// O caminho e cada pasta acima dele são do root, sem escrita de grupo ou outros e sem link.
fn root_only(path: &Path) -> Result<()> {
    safe(path)?;
    for ancestor in path.ancestors() {
        let meta = fs::symlink_metadata(ancestor)?;
        ensure!(
            meta.uid() == 0 && meta.mode() & 0o022 == 0,
            "{} deve ser do root, sem escrita de grupo ou outros",
            ancestor.display()
        );
    }
    Ok(())
}

/// Hub já instalado no layout do root: é ele que o sudo executa, nunca uma cópia gravável pela conta.
fn installed_host() -> Option<PathBuf> {
    let host = Path::new(BIN).join("donnabot-host");
    let meta = fs::symlink_metadata(&host).ok()?;
    (meta.is_file() && meta.gid() == 0 && root_only(&host).is_ok()).then_some(host)
}

/// Layout anterior, com a pasta de binários da conta do Hub: ela sai inteira do caminho e
/// dá lugar a uma do root. /opt/donnabot é do root, então a troca de nome não depende dela.
fn retire_hub_bin() -> Result<()> {
    let bin = Path::new(BIN);
    safe(bin)?;
    let meta = match fs::symlink_metadata(bin) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    if meta.is_dir() && meta.uid() == 0 && meta.gid() == 0 && meta.mode() & 0o7777 == 0o755 {
        return Ok(());
    }
    let old = bin.with_file_name(format!(".bin-anterior-{}", uuid::Uuid::new_v4()));
    fs::rename(bin, &old)?;
    if let Err(error) = fs::remove_dir_all(&old) {
        println!("Pasta anterior de binários fora do caminho, mas não removida ({error}): {}", old.display());
    }
    println!("Pasta de binários da instalação anterior, da conta do Hub, trocada por uma do root.");
    Ok(())
}

fn config_owner(hub: &User) -> Result<()> {
    safe(Path::new(CONFIG))?;
    if Path::new(CONFIG).exists() {
        let text = fs::read_to_string(CONFIG)?;
        ensure!(
            text.lines().any(|s| s == format!("hub_uid={}", hub.uid))
                && text.lines().any(|s| s == format!("motor_user={MOTOR_USER}")),
            "configuração pertence a outra instalação; nada alterado"
        );
    }
    Ok(())
}

pub(super) fn prerequisites() -> Result<()> {
    // SAFETY: consulta do UID sem efeitos colaterais.
    ensure!(unsafe { libc::geteuid() } == 0, "etapa administrativa exige sudo");
    ensure!(Path::new("/run/systemd/system").is_dir(), "systemd deve estar ativo");
    for name in ["getent", "groupadd", "useradd", "usermod", "userdel", "setcap", "getcap", "systemctl", "runuser"] {
        system(name)?;
    }
    // SAFETY: syscall de consulta de versão, sem ponteiro para memória do usuário.
    let abi = unsafe { libc::syscall(libc::SYS_landlock_create_ruleset, std::ptr::null::<u8>(), 0, 1) };
    println!("Landlock ABI: {abi}. F2b futura exige >= 3; não bloqueia a F4.");
    Ok(())
}

fn nss(database: &str, key: &OsStr) -> Result<Option<String>> {
    let out = Command::new(system("getent")?).arg(database).arg(key).output()?;
    if out.status.code() == Some(2) {
        return Ok(None);
    }
    ensure!(out.status.success(), "consulta getent falhou");
    Ok(Some(String::from_utf8(out.stdout)?))
}

pub(super) fn install(hub: &User, host: &Path, helper: &Path, data: &Path, port: u16, start: bool) -> Result<()> {
    config_owner(hub)?;
    ensure!(port != 0, "porta inválida");
    crate::environment::ensure_own_port(port)?;
    let stable = hub.home.join(".local/bin/donnabot-host");
    if let Ok(meta) = fs::symlink_metadata(&stable) {
        ensure!(
            meta.file_type().is_symlink() && fs::read_link(&stable)? == Path::new(BIN).join("donnabot-host"),
            "binário antigo ou outro destino no link do app; use migrar-motor antes"
        );
    }
    crate::environment::ensure_own_data_dir(&hub.home, data)?;
    if fs::symlink_metadata(data).is_ok() {
        crate::fork::private_fs::Directory::for_owner(data, false, hub.uid, Some(hub.gid))?;
    }
    safe(data)?;
    ensure!(data.parent().is_some_and(Path::is_dir), "pai de data_dir deve existir");
    ensure!(host != helper && host.is_file() && helper.is_file(), "Hub e ajudante distintos são exigidos");
    let (host_file, helper_file) = (open_as_hub(hub, host)?, open_as_hub(hub, helper)?);
    if nss("group", WORK_GROUP.as_ref())?.is_none() {
        checked(Command::new(system("groupadd")?).args(["--system", WORK_GROUP]))?;
    }
    let work = nss("group", WORK_GROUP.as_ref())?.context("grupo ausente")?;
    let gid: u32 = work.split(':').nth(2).context("getent group inválido")?.parse()?;
    ensure!(gid != 0 && gid != hub.gid, "grupo de trabalho deve ser dedicado");
    let nologin = system("nologin")?;
    if nss("passwd", MOTOR_USER.as_ref())?.is_none() {
        checked(
            Command::new(system("useradd")?)
                .args(["--system", "--user-group", "--no-create-home", "--home-dir", MOTOR, "--shell"])
                .arg(&nologin)
                .arg(MOTOR_USER),
        )?;
    }
    let entry = nss("passwd", MOTOR_USER.as_ref())?.context("motor ausente")?;
    let uid: u32 = entry.split(':').nth(2).context("getent passwd inválido")?.parse()?;
    let motor = user(uid)?;
    ensure!(
        uid != 0
            && uid != hub.uid
            && motor.gid != 0
            && motor.gid != hub.gid
            && motor.home == Path::new(MOTOR)
            && motor.shell == nologin,
        "usuário do motor existente incompatível; não será reciclado"
    );
    for name in [&hub.name, OsStr::new(MOTOR_USER)] {
        checked(Command::new(system("usermod")?).args([
            OsStr::new("-a"),
            OsStr::new("-G"),
            OsStr::new(WORK_GROUP),
            name,
        ]))?;
    }
    directory(Path::new(BASE), 0, 0, 0o755)?;
    directory(Path::new(MOTOR), uid, motor.gid, 0o700)?;
    directory(Path::new(WORK), hub.uid, gid, 0o2770)?;
    if unit(hub).exists() {
        service(hub, &["stop", "donnabot-host.service"])?;
    }
    directory(Path::new("/opt/donnabot"), 0, 0, 0o755)?;
    retire_hub_bin()?;
    directory(Path::new(BIN), 0, 0, 0o755)?;
    root_only(Path::new(BIN))?;
    let helper_dst = Path::new(BIN).join("donnabot-motor-exec");
    copy_into(helper_file, &helper_dst, 0, hub.gid, 0o750)?;
    checked(Command::new(system("setcap")?).arg("cap_setuid,cap_setgid+ep").arg(&helper_dst))
        .context("/opt precisa suportar security.capability")?;
    let caps = checked(Command::new(system("getcap")?).arg(&helper_dst))?;
    let caps = String::from_utf8(caps.stdout)?;
    ensure!(
        caps.contains("cap_setgid,cap_setuid=ep") || caps.contains("cap_setuid,cap_setgid=ep"),
        "getcap não confirmou capacidades"
    );
    directory(Path::new("/etc/donnabot"), 0, 0, 0o755)?;
    write_atomic(
        Path::new(CONFIG),
        format!("motor_user={MOTOR_USER}\nhub_uid={}\nwork_gid={gid}\n", hub.uid).as_bytes(),
        0,
        0,
        0o644,
    )?;
    checked(user_command(hub, &helper_dst)?.args(["--cwd", MOTOR, "--", "/usr/bin/id", "-u"]))
        .context("capacidades ineficazes: /opt não pode ser nosuid e o chamador não pode usar NoNewPrivileges")?;
    let host_dst = Path::new(BIN).join("donnabot-host");
    copy_into(host_file, &host_dst, 0, 0, 0o755)?;
    for suffix in [".local", ".local/bin", ".config", ".config/systemd", ".config/systemd/user"] {
        let path = hub.home.join(suffix);
        safe(&path)?;
        if !path.exists() {
            directory(&path, hub.uid, hub.gid, 0o700)?;
        }
        ensure!(path.is_dir() && fs::metadata(&path)?.uid() == hub.uid, "diretório do Hub incompatível");
    }
    let link = hub.home.join(".local/bin/donnabot-host");
    match fs::symlink_metadata(&link) {
        Ok(meta) if meta.file_type().is_symlink() => {
            ensure!(fs::read_link(&link)? == host_dst, "link do app pertence a outro destino");
        }
        Ok(_) => bail!("binário antigo no link do app; use migrar-motor antes"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => std::os::unix::fs::symlink(&host_dst, &link)?,
        Err(e) => return Err(e.into()),
    }
    crate::fork::private_fs::Directory::for_owner(data, true, hub.uid, Some(hub.gid))?;
    let text = format!(
        "[Unit]\nDescription=Mexerica Hub\nAfter=network-online.target\n\n[Service]\nUMask=0077\nExecStartPre={BIN}/donnabot-motor-exec cleanup\nExecStart={BIN}/donnabot-host serve --port {port}\nExecStopPost={BIN}/donnabot-motor-exec cleanup\nEnvironment={}\nEnvironment=\"PATH=/opt/donnabot/bin:/usr/local/bin:/usr/bin:/bin\"\nRestart=always\nRestartSec=3\n\n[Install]\nWantedBy=default.target\n",
        quote(&format!("MEXERICA_HOME={}", data.display()))
    );
    write_atomic(&unit(hub), text.as_bytes(), hub.uid, hub.gid, 0o600)?;
    service(hub, &["daemon-reload"])?;
    if start {
        service(hub, &["enable", "--now", "donnabot-host.service"])?;
    }
    println!("Instalação pronta; link do app em ~/.local/bin/donnabot-host; dados preservados.");
    Ok(())
}

fn quote(s: &str) -> String {
    format!(
        "\"{}\"",
        s.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%").replace('\n', "\\n").replace('\r', "\\r")
    )
}

fn uninstall(hub: &User, remove_user: bool) -> Result<()> {
    config_owner(hub)?;
    if unit(hub).exists() {
        service(hub, &["disable", "--now", "donnabot-host.service"])?;
        safe(&unit(hub))?;
        fs::remove_file(unit(hub))?;
        service(hub, &["daemon-reload"])?;
    }
    if Path::new(CONFIG).exists() {
        checked(user_command(hub, Path::new(BIN).join("donnabot-motor-exec"))?.arg("cleanup"))?;
        safe(&Path::new(BIN).join("donnabot-motor-exec"))?;
        fs::remove_file(Path::new(BIN).join("donnabot-motor-exec"))?;
        fs::remove_file(CONFIG)?;
    }
    if remove_user && nss("passwd", MOTOR_USER.as_ref())?.is_some() {
        checked(Command::new(system("userdel")?).arg(MOTOR_USER))?; // nunca --remove
    }
    println!("Unidade, ajudante e configuração removidos. Dados preservados. Conta removida somente se solicitado.");
    Ok(())
}

fn request(args: Vec<OsString>) -> Result<()> {
    ensure!(
        crate::environment::Environment::current() == crate::environment::Environment::Main,
        "instalador exige binário main; Dev permanece separado"
    );
    // SAFETY: consulta sem efeitos colaterais.
    let uid = unsafe { libc::getuid() };
    ensure!(uid != 0, "execute como usuário de login; install solicitará sudo");
    let program = match installed_host() {
        Some(host) => host,
        None => crate::service::current_exe()?,
    };
    let status = Command::new(system("sudo")?)
        .arg("--")
        .arg(program)
        .arg("install-motor")
        .args(args)
        .arg("--hub-uid")
        .arg(uid.to_string())
        .status()?;
    ensure!(status.success(), "etapa administrativa recusada; confira a mensagem acima");
    Ok(())
}

pub(super) fn request_install(port: u16, helper: Option<PathBuf>) -> Result<()> {
    crate::environment::ensure_own_port(port)?;
    crate::environment::ensure_own_data_dir(&crate::service::home(), &crate::service::data_dir())?;
    let host = crate::service::current_exe()?;
    let helper = helper.unwrap_or_else(|| host.with_file_name("donnabot-motor-exec"));
    request(vec![
        "instalar".into(),
        "--host".into(),
        host.into_os_string(),
        "--ajudante".into(),
        helper.into_os_string(),
        "--data-dir".into(),
        crate::service::data_dir().into_os_string(),
        "--port".into(),
        port.to_string().into(),
    ])
}

pub(super) fn request_uninstall(remove_user: bool) -> Result<()> {
    let mut args = vec!["desinstalar".into()];
    if remove_user {
        args.push("--remover-usuario-motor".into());
    }
    request(args)
}

pub(super) fn request_migrate(action: Migration) -> Result<()> {
    let mut args: Vec<OsString> = match action {
        Migration::Aplicar { instalacao_de_teste, ajudante, backup, port } => {
            ensure!(instalacao_de_teste, "exige --instalacao-de-teste; servidor real é etapa F5");
            crate::environment::ensure_own_port(port)?;
            vec![
                "migrar".into(),
                "--ajudante".into(),
                ajudante.into_os_string(),
                "--backup".into(),
                backup.into_os_string(),
                "--port".into(),
                port.to_string().into(),
            ]
        }
        Migration::Voltar { instalacao_de_teste, backup } => {
            ensure!(instalacao_de_teste, "exige --instalacao-de-teste");
            return request(vec!["voltar".into(), "--backup".into(), backup.into_os_string()]);
        }
        Migration::Conferir => {
            println!("{}", migration::CHECKLIST);
            return Ok(());
        }
    };
    args.extend([
        "--host".into(),
        crate::service::current_exe()?.into_os_string(),
        "--data-dir".into(),
        crate::service::data_dir().into_os_string(),
    ]);
    request(args)
}

pub(super) async fn execute(action: RootAction) -> Result<()> {
    prerequisites()?;
    ensure!(
        crate::environment::Environment::current() == crate::environment::Environment::Main,
        "etapa root exige binário main"
    );
    let uid = match &action {
        RootAction::Instalar { hub_uid, .. }
        | RootAction::Desinstalar { hub_uid, .. }
        | RootAction::Migrar { hub_uid, .. }
        | RootAction::Voltar { hub_uid, .. } => *hub_uid,
    };
    let hub = user(uid)?;
    ensure!(uid != 0 && hub.gid != 0, "Hub não pode ser root nem usar grupo root");
    if let Ok(caller) = std::env::var("SUDO_UID") {
        ensure!(caller.parse::<u32>()? == uid, "sudo deve ser chamado pelo usuário do Hub");
    }
    // Serializa somente nossa instalação; lock root fora de qualquer árvore do motor.
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open("/run/lock/donnabot-install.lock")?;
    ensure!(crate::lock_file(&lock, crate::FLOCK_WAIT)?, "outro instalador Mexerica está em andamento");
    match action {
        RootAction::Instalar { host, ajudante, data_dir, port, sem_iniciar, .. } => {
            install(&hub, &host, &ajudante, &data_dir, port, !sem_iniciar)
        }
        RootAction::Desinstalar { remover_usuario_motor, .. } => uninstall(&hub, remover_usuario_motor),
        RootAction::Migrar { host, ajudante, data_dir, backup, port, .. } => {
            migration::apply(&hub, &host, &ajudante, &data_dir, &backup, port).await
        }
        RootAction::Voltar { backup, .. } => migration::rollback(&hub, &backup),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caminho_do_sudo_exige_pastas_do_root_sem_escrita_alheia() {
        assert!(root_only(Path::new("/usr/bin")).is_ok());
        let dir = std::env::temp_dir().join(format!("donna-root-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let file = dir.join("donnabot-host");
        fs::write(&file, b"x").unwrap();
        // SAFETY: consulta sem efeitos colaterais.
        if unsafe { libc::geteuid() } != 0 {
            assert!(root_only(&file).is_err());
        }
        let link = dir.join("link");
        std::os::unix::fs::symlink("/usr/bin", &link).unwrap();
        assert!(root_only(&link).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
