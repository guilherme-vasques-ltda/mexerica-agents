use super::{Migration, RootAction, migration};
use anyhow::{Context, Result, bail, ensure};
use std::{
    ffi::{CStr, CString, OsStr, OsString},
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

pub(super) fn as_hub<T: Send>(hub: &User, extra: &[u32], work: impl FnOnce() -> Result<T> + Send) -> Result<T> {
    let mut groups = super::identity::user_groups(&hub.name, hub.gid)?;
    groups.extend_from_slice(extra);
    super::identity::as_user(hub.uid, hub.gid, &groups, work)
}

pub(super) fn inside(home: &Path, path: &Path) -> Result<()> {
    let plain = |p: &Path| p.is_absolute() && !p.components().any(|c| matches!(c, Component::ParentDir));
    ensure!(plain(home) && home != Path::new("/"), "home do Hub inválida para migração e volta");
    ensure!(
        plain(path) && path.starts_with(home) && path != home,
        "caminho fora da home do Hub recusado: {}",
        path.display()
    );
    Ok(())
}

fn open_source(source: &Path) -> Result<fs::File> {
    safe(source)?;
    let file = OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(source)?;
    ensure!(file.metadata()?.is_file(), "arquivo regular exigido: {}", source.display());
    Ok(file)
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

fn exchange(a: &Path, b: &Path) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let (a, b) = (CString::new(a.as_os_str().as_bytes())?, CString::new(b.as_os_str().as_bytes())?);
    // SAFETY: caminhos absolutos terminados em NUL, ambos em /opt/donnabot, que é do root.
    let rc = unsafe { libc::renameat2(libc::AT_FDCWD, a.as_ptr(), libc::AT_FDCWD, b.as_ptr(), libc::RENAME_EXCHANGE) };
    ensure!(rc == 0, "troca da pasta de binários: {}", std::io::Error::last_os_error());
    Ok(())
}

/// Põe a pasta preparada no lugar de /opt/donnabot/bin numa troca só, dentro da pasta do root.
/// Devolve se houve troca: a pasta anterior fica no lugar da preparada até a instalação terminar.
fn swap_bin(staged: &Path) -> Result<bool> {
    let bin = Path::new(BIN);
    let old = match fs::symlink_metadata(bin) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fs::rename(staged, bin)?;
            return Ok(false);
        }
        Err(e) => return Err(e.into()),
    };
    exchange(staged, bin)?;
    if !(old.is_dir() && old.uid() == 0 && old.gid() == 0) {
        println!("Pasta de binários da instalação anterior, da conta do Hub, trocada por uma do root.");
    }
    Ok(true)
}

/// Origens do Hub e do ajudante, abertas com a identidade de arquivos do Hub antes de qualquer
/// mudança: o root nunca copia o que essa conta não lê, e copia exatamente o que foi aberto.
pub(super) fn open_sources(hub: &User, host: &Path, helper: &Path) -> Result<(fs::File, fs::File)> {
    ensure!(host != helper, "Hub e ajudante distintos são exigidos");
    as_hub(hub, &[], || Ok((open_source(host)?, open_source(helper)?)))
        .context("origem do Hub ou do ajudante ilegível pela conta do Hub; nada foi alterado")
}

fn active(hub: &User) -> Result<bool> {
    Ok(user_command(hub, system("systemctl")?)?
        .args(["--user", "is-active", "--quiet", "donnabot-host.service"])
        .status()?
        .success())
}

fn restore_config(previous: Option<&[u8]>) {
    let _ = match previous {
        Some(bytes) => write_atomic(Path::new(CONFIG), bytes, 0, 0, 0o644),
        None => fs::remove_file(CONFIG).map_err(Into::into),
    };
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

pub(super) fn install(
    hub: &User,
    (host_file, helper_file): (fs::File, fs::File),
    data: &Path,
    port: u16,
    start: bool,
) -> Result<()> {
    config_owner(hub)?;
    ensure!(port != 0, "porta inválida");
    crate::environment::ensure_own_port(port)?;
    inside(&hub.home, data)?;
    crate::environment::ensure_own_data_dir(&hub.home, data)?;
    let host_dst = Path::new(BIN).join("donnabot-host");
    let stable = hub.home.join(".local/bin/donnabot-host");
    // O que está na home do Hub é lido e gravado com a identidade de arquivos dele.
    as_hub(hub, &[], || {
        if let Ok(meta) = fs::symlink_metadata(&stable) {
            ensure!(
                meta.file_type().is_symlink() && fs::read_link(&stable)? == host_dst,
                "binário antigo ou outro destino no link do app; use migrar-motor antes"
            );
        }
        if fs::symlink_metadata(data).is_ok() {
            crate::fork::private_fs::Directory::for_owner(data, false, hub.uid, Some(hub.gid))?;
        }
        safe(data)?;
        ensure!(data.parent().is_some_and(Path::is_dir), "pai de data_dir deve existir");
        Ok(())
    })?;
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
    directory(Path::new("/opt/donnabot"), 0, 0, 0o755)?;
    root_only(Path::new("/opt/donnabot"))?;
    safe(Path::new(CONFIG))?;
    let previous_config = match fs::read(CONFIG) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let previous_unit = as_hub(hub, &[], || {
        safe(&unit(hub))?;
        match fs::read(unit(hub)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    })?;
    let running = previous_unit.is_some() && active(hub)?;
    // O par novo é preparado e conferido numa pasta do root ao lado; o Hub só para antes da troca.
    let staged = Path::new("/opt/donnabot").join(format!(".bin-novo-{}", uuid::Uuid::new_v4()));
    let prepared = (|| {
        directory(&staged, 0, 0, 0o755)?;
        let helper_new = staged.join("donnabot-motor-exec");
        copy_into(helper_file, &helper_new, 0, hub.gid, 0o750)?;
        copy_into(host_file, &staged.join("donnabot-host"), 0, 0, 0o755)?;
        checked(Command::new(system("setcap")?).arg("cap_setuid,cap_setgid+ep").arg(&helper_new))
            .context("/opt precisa suportar security.capability")?;
        let caps = checked(Command::new(system("getcap")?).arg(&helper_new))?;
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
        checked(user_command(hub, &helper_new)?.args(["--cwd", MOTOR, "--", "/usr/bin/id", "-u"]))
            .context("capacidades ineficazes: /opt não pode ser nosuid e o chamador não pode usar NoNewPrivileges")?;
        Ok(())
    })();
    if let Err(error) = prepared {
        let _ = fs::remove_dir_all(&staged);
        restore_config(previous_config.as_deref());
        return Err(error.context("instalação recusada antes de parar o Hub; nada foi trocado"));
    }
    let text = format!(
        "[Unit]\nDescription=Mexerica Hub\nAfter=network-online.target\n\n[Service]\nUMask=0077\nExecStartPre={BIN}/donnabot-motor-exec cleanup\nExecStart={BIN}/donnabot-host serve --port {port}\nExecStopPost={BIN}/donnabot-motor-exec cleanup\nEnvironment={}\nEnvironment=\"PATH=/opt/donnabot/bin:/usr/local/bin:/usr/bin:/bin\"\nRestart=always\nRestartSec=3\n\n[Install]\nWantedBy=default.target\n",
        quote(&format!("MEXERICA_HOME={}", data.display()))
    );
    let mut exchanged = false;
    let finished: Result<()> = (|| {
        if previous_unit.is_some() {
            service(hub, &["stop", "donnabot-host.service"])?;
        }
        exchanged = swap_bin(&staged)?;
        as_hub(hub, &[], || {
            for suffix in [".local", ".local/bin", ".config", ".config/systemd", ".config/systemd/user"] {
                let path = hub.home.join(suffix);
                safe(&path)?;
                if !path.exists() {
                    directory(&path, hub.uid, hub.gid, 0o700)?;
                }
                ensure!(path.is_dir() && fs::metadata(&path)?.uid() == hub.uid, "diretório do Hub incompatível");
            }
            match fs::symlink_metadata(&stable) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    ensure!(fs::read_link(&stable)? == host_dst, "link do app pertence a outro destino");
                }
                Ok(_) => bail!("binário antigo no link do app; use migrar-motor antes"),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => std::os::unix::fs::symlink(&host_dst, &stable)?,
                Err(e) => return Err(e.into()),
            }
            crate::fork::private_fs::Directory::for_owner(data, true, hub.uid, Some(hub.gid))?;
            write_atomic(&unit(hub), text.as_bytes(), hub.uid, hub.gid, 0o600)
        })?;
        service(hub, &["daemon-reload"])?;
        if start {
            service(hub, &["enable", "--now", "donnabot-host.service"])?;
        }
        Ok(())
    })();
    if let Err(error) = finished {
        // Desfaz a troca e religa o que estava rodando; a pasta preparada guarda o par novo ou o anterior.
        let _ = service(hub, &["stop", "donnabot-host.service"]);
        if exchanged {
            let _ = exchange(&staged, Path::new(BIN));
        }
        let _ = fs::remove_dir_all(&staged);
        restore_config(previous_config.as_deref());
        if let Some(bytes) = &previous_unit {
            let _ = as_hub(hub, &[], || write_atomic(&unit(hub), bytes, hub.uid, hub.gid, 0o600));
            let _ = service(hub, &["daemon-reload"]);
        }
        if running {
            let _ = service(hub, &["start", "donnabot-host.service"]);
        }
        return Err(error.context("instalação desfeita; binários, configuração e unidade anteriores restaurados"));
    }
    if exchanged && let Err(error) = fs::remove_dir_all(&staged) {
        println!("Pasta anterior de binários fora do caminho, mas não removida ({error}): {}", staged.display());
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
    if as_hub(hub, &[], || Ok(unit(hub).exists()))? {
        service(hub, &["disable", "--now", "donnabot-host.service"])?;
        as_hub(hub, &[], || {
            safe(&unit(hub))?;
            Ok(fs::remove_file(unit(hub))?)
        })?;
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
    let program = if let Some(host) = installed_host() {
        host
    } else {
        let exe = crate::service::current_exe()?;
        ensure!(
            !exe.starts_with(BIN),
            "instalação anterior com binários da conta do Hub: o sudo não executa {}; rode ./donnabot-host install a partir do pacote baixado",
            exe.display()
        );
        exe
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
            install(&hub, open_sources(&hub, &host, &ajudante)?, &data_dir, port, !sem_iniciar)
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
