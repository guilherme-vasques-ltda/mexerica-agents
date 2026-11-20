//! Orquestra cópia e volta. Só a F3 conhece e reescreve referências do banco.
use super::identity::as_user;
use super::root::{
    BASE, BIN, CONFIG, MOTOR, User, WORK, as_hub, checked, directory, inside, install, open_as_hub, owner_mode, place,
    safe, service, system, unit, user, user_command, write_atomic,
};
use crate::fork::motor_exec::fd::Directory;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::{CString, OsStr, OsString},
    fs,
    io::Read,
    os::{
        fd::AsRawFd,
        unix::{
            ffi::OsStrExt,
            fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        },
    },
    path::{Component, Path, PathBuf},
    time::Duration,
};

pub(super) const CHECKLIST: &str = "Conferir em até 30 minutos: Hub sobe; app conecta pelo link ~/.local/bin/donnabot-host; bot responde; conta copiada usa CLI (se recusar, refazer login pelo app); login novo pelo app; sentinela do cofre igual, inclusive após reinício sem SSH; conector sem bot; anexo lido; UIDs e CapEff; negativos de token, DB, binário, send_file e autoridade da ponte. Voltar se Hub não subir, bot não responder após novo login, sentinela falhar ou qualquer negativo falhar. Parada de bot/Hub e retomada sem processos sobrando pertencem ao aceite F5. Originais preservados; apagar backup somente após aceite e 7 dias.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CopyItem {
    source: PathBuf,
    destination: PathBuf,
    kind: Kind,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Workspace,
    Uploads,
    Skills,
    Profile,
}
#[derive(Clone, Serialize, Deserialize)]
struct Manifest {
    hub_uid: u32,
    data: PathBuf,
    binary: PathBuf,
    binary_mode: u32,
    data_modes: BTreeMap<String, u32>,
    unit_present: bool,
    link_target: Option<PathBuf>,
    hashes: BTreeMap<String, String>,
}

fn digest(path: &Path) -> Result<String> {
    use std::{fmt::Write, io::Read};
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 8192];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let mut result = String::with_capacity(64);
    for byte in hasher.finalize() {
        write!(result, "{byte:02x}")?;
    }
    Ok(result)
}

fn hashes(root: &Path) -> Result<BTreeMap<String, String>> {
    hashes_excluding(root, &[], None)
}

fn screen_socket(path: &Path, data_dir: Option<&Path>, meta: &fs::Metadata) -> bool {
    // O tipo vem de lstat; nome sozinho nunca autoriza ignorar um arquivo.
    meta.file_type().is_socket() && data_dir.is_some_and(|data| path == data.join("screen.sock"))
}

fn hashes_excluding(root: &Path, excluded: &[PathBuf], data_dir: Option<&Path>) -> Result<BTreeMap<String, String>> {
    fn visit(
        root: &Path,
        path: &Path,
        result: &mut BTreeMap<String, String>,
        excluded: &[PathBuf],
        data_dir: Option<&Path>,
    ) -> Result<()> {
        if excluded.iter().any(|p| path.starts_with(p)) {
            return Ok(());
        }
        safe(path)?;
        let meta = fs::symlink_metadata(path)?;
        if screen_socket(path, data_dir, &meta) {
            return Ok(());
        }
        let key = path.strip_prefix(root)?.to_str().context("nome não UTF-8 na cópia")?.to_owned();
        if meta.is_dir() {
            result.insert(format!("{key}/"), "directory".into());
            for item in fs::read_dir(path)? {
                visit(root, &item?.path(), result, excluded, data_dir)?;
            }
        } else {
            ensure!(meta.is_file(), "arquivo especial recusado na cópia: {}", path.display());
            result.insert(key, digest(path)?);
        }
        Ok(())
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result, excluded, data_dir)?;
    Ok(result)
}

fn data_modes(root: &Path) -> Result<BTreeMap<String, u32>> {
    fn visit(root: &Path, path: &Path, result: &mut BTreeMap<String, u32>) -> Result<()> {
        safe(path)?;
        let meta = fs::symlink_metadata(path)?;
        if screen_socket(path, Some(root), &meta) {
            return Ok(());
        }
        ensure!(meta.is_dir() || meta.is_file(), "arquivo especial recusado nos modos da cópia");
        result.insert(path.strip_prefix(root)?.to_str().context("nome não UTF-8")?.to_owned(), meta.mode() & 0o777);
        if meta.is_dir() {
            for entry in fs::read_dir(path)? {
                visit(root, &entry?.path(), result)?;
            }
        }
        Ok(())
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result)?;
    Ok(result)
}

fn copy_tree(source: &Path, destination: &Path, uid: u32, gid: u32, dir_mode: u32, file_mode: u32) -> Result<()> {
    copy_excluding(source, destination, uid, gid, dir_mode, file_mode, (&[], None))
}

fn copy_excluding(
    source: &Path,
    destination: &Path,
    uid: u32,
    gid: u32,
    dir_mode: u32,
    file_mode: u32,
    (excluded, data_dir): (&[PathBuf], Option<&Path>),
) -> Result<()> {
    if excluded.iter().any(|p| source.starts_with(p)) {
        return Ok(());
    }
    safe(source)?;
    safe(destination)?;
    let meta = fs::symlink_metadata(source)?;
    if screen_socket(source, data_dir, &meta) {
        return Ok(());
    }
    if meta.is_dir() {
        directory(destination, uid, gid, dir_mode)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_excluding(
                &entry.path(),
                &destination.join(entry.file_name()),
                uid,
                gid,
                dir_mode,
                file_mode,
                (excluded, data_dir),
            )?;
        }
    } else {
        ensure!(meta.is_file(), "link/arquivo especial recusado: {}", source.display());
        let executable = if dir_mode == 0o2770 { (meta.mode() & 0o110) | ((meta.mode() & 0o100) >> 3) } else { 0 };
        place(source, destination, uid, gid, file_mode | executable)?;
    }
    Ok(())
}

fn parents(path: &Path, root: &Path, uid: u32, gid: u32, mode: u32) -> Result<()> {
    let relative = path.strip_prefix(root)?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        directory(&current, uid, gid, mode)?;
    }
    Ok(())
}

fn fd_owner(file: &fs::File, uid: u32, gid: u32) -> Result<()> {
    // SAFETY: descritor vivo aberto sem seguir link.
    ensure!(unsafe { libc::fchown(file.as_raw_fd(), uid, gid) } == 0, "fchown: {}", std::io::Error::last_os_error());
    Ok(())
}

fn anchored_parents(path: &Path, root: &Path, uid: u32, gid: u32) -> Result<Directory> {
    let mut dir = Directory::open(root)?;
    for component in path.strip_prefix(root)?.components() {
        let Component::Normal(name) = component else { bail!("componente inválido no destino") };
        dir = dir.child(name, Some(0o700))?;
        fd_owner(&dir.0, uid, gid)?;
        dir.mode(0o700)?;
    }
    Ok(dir)
}

fn copy_profile(
    from: &Directory,
    name: &OsStr,
    to: &Directory,
    target: &OsStr,
    hub_uid: u32,
    (uid, gid): (u32, u32),
) -> Result<()> {
    match from.child(name, None) {
        Ok(source) => {
            ensure!(source.0.metadata()?.uid() == hub_uid, "perfil com pasta de outro dono recusado");
            let destination = to.child(target, Some(0o700))?;
            fd_owner(&destination.0, uid, gid)?;
            destination.mode(0o700)?;
            for entry in source.names()? {
                copy_profile(&source, &entry, &destination, &entry, hub_uid, (uid, gid))?;
            }
            Ok(())
        }
        Err(error)
            if error.root_cause().downcast_ref::<std::io::Error>().and_then(std::io::Error::raw_os_error)
                == Some(libc::ENOTDIR) =>
        {
            ensure!(
                !target.is_empty() && !target.as_bytes().contains(&b'/') && target != "." && target != "..",
                "nome de destino inválido"
            );
            let mut input = from.file(name, libc::O_RDONLY, 0)?;
            ensure!(input.metadata()?.uid() == hub_uid, "perfil com arquivo de outro dono recusado");
            let temp = OsString::from(format!(".donna-{}", uuid::Uuid::new_v4()));
            let (temp_c, target_c) = (CString::new(temp.as_bytes())?, CString::new(target.as_bytes())?);
            let mut created = false;
            let result = (|| {
                let mut output = to.file(&temp, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL, 0o600)?;
                created = true;
                std::io::copy(&mut input, &mut output)?;
                output.sync_all()?;
                fd_owner(&output, uid, gid)?;
                output.set_permissions(fs::Permissions::from_mode(0o600))?;
                let dir = to.0.as_raw_fd();
                // SAFETY: nomes de um componente, ambos relativos ao mesmo diretório ancorado.
                let renamed = unsafe { libc::renameat(dir, temp_c.as_ptr(), dir, target_c.as_ptr()) };
                ensure!(renamed == 0, "renameat: {}", std::io::Error::last_os_error());
                Ok(())
            })();
            if result.is_err() && created {
                // SAFETY: remove só o temporário criado acima, relativo ao diretório ancorado.
                unsafe { libc::unlinkat(to.0.as_raw_fd(), temp_c.as_ptr(), 0) };
            }
            result
        }
        Err(error) => Err(error),
    }
}

fn no_acl(path: &Path) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let path_c = CString::new(path.as_os_str().as_bytes())?;
    for name in [c"system.posix_acl_access", c"system.posix_acl_default"] {
        // SAFETY: caminhos e nomes terminados em NUL; consulta sem buffer de saída.
        let size = unsafe { libc::lgetxattr(path_c.as_ptr(), name.as_ptr(), std::ptr::null_mut(), 0) };
        let error = std::io::Error::last_os_error();
        ensure!(
            size == -1 && matches!(error.raw_os_error(), Some(libc::ENODATA | libc::ENOTSUP)),
            "cópia de segurança deve estar sem ACL: {}",
            path.display()
        );
    }
    Ok(())
}

fn private_backup(path: &Path, hub_uid: u32) -> Result<()> {
    safe(path)?;
    no_acl(path)?;
    let meta = fs::symlink_metadata(path)?;
    let expected = if meta.is_dir() { 0o700 } else { 0o600 };
    ensure!(
        meta.uid() == hub_uid && meta.mode() & 0o7777 == expected,
        "permissão privada da cópia inválida: {}",
        path.display()
    );
    if meta.is_dir() {
        for entry in fs::read_dir(path)? {
            private_backup(&entry?.path(), hub_uid)?;
        }
    } else {
        ensure!(meta.is_file(), "cópia contém arquivo especial");
    }
    Ok(())
}

fn validate_backup(path: &Path) -> Result<()> {
    safe(path)?;
    ensure!(
        path.file_name().is_some_and(|n| {
            n.to_string_lossy().starts_with("donnabot-")
                && path.as_os_str() == Path::new("/var/backups").join(n).as_os_str()
        }),
        "backup deve ser /var/backups/donnabot-<identificador>, fora de qualquer árvore liberada"
    );
    safe(Path::new("/var/backups"))?;
    let meta = fs::metadata("/var/backups")?;
    ensure!(meta.uid() == 0 && meta.mode() & 0o022 == 0, "/var/backups deve ser root sem escrita por grupo/outros");
    no_acl(Path::new("/var/backups"))
}

fn f3_inventory(hub: &User, host: &Path, data: &Path) -> Result<Vec<CopyItem>> {
    let output = checked(user_command(hub, host)?.arg("migrar-raizes").arg("--data-dir").arg(data).arg("--inventario"))
        .context("F3 deve estar integrada: migrar-raizes --inventario; nada foi parado nem alterado")?;
    let items: Vec<CopyItem> = serde_json::from_slice(&output.stdout).context("inventário F3 inválido")?;
    for (i, item) in items.iter().enumerate() {
        safe(&item.destination)?;
        ensure!(
            items[..i].iter().all(|other| other.destination != item.destination),
            "destino repetido no inventário: {}",
            item.destination.display()
        );
        ensure!(
            !matches!(item.kind, Kind::Workspace) || !data.starts_with(&item.source),
            "workspace contém dados privados do Hub; origem de cópia recusada"
        );
        let root = match item.kind {
            Kind::Profile => {
                // O root abre a origem do perfil por descritor: só dentro da home do Hub.
                inside(&hub.home, &item.source)?;
                MOTOR
            }
            _ => WORK,
        };
        ensure!(item.destination.starts_with(root) && item.destination != Path::new(root), "destino fora da raiz F3");
        ensure!(
            !item.source.starts_with(BASE) && !item.source.starts_with("/var/backups"),
            "origem já migrada ou backup recusado"
        );
        ensure!(
            !item.destination.exists(),
            "destino já existe: {}; não sobrescrever dados",
            item.destination.display()
        );
    }
    // Valida a árvore inteira antes da parada, lendo só o que o Hub já lê.
    as_hub(hub, &[], || {
        items.iter().try_for_each(|item| {
            safe(&item.source)?;
            hashes(&item.source)?;
            if matches!(item.kind, Kind::Profile) {
                owned_by(&item.source, hub.uid)?;
            }
            Ok(())
        })
    })?;
    Ok(items)
}

/// Mesma regra de dono de `copy_profile`, conferida antes de parar o Hub.
fn owned_by(path: &Path, uid: u32) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(meta.uid() == uid, "perfil com item de outro dono recusado: {}", path.display());
    if meta.is_dir() {
        for entry in fs::read_dir(path)? {
            owned_by(&entry?.path(), uid)?;
        }
    }
    Ok(())
}

pub(super) async fn apply(hub: &User, host: &Path, helper: &Path, data: &Path, backup: &Path, port: u16) -> Result<()> {
    validate_backup(backup)?;
    let record = record_path(backup);
    ensure!(
        fs::symlink_metadata(backup).is_err() && fs::symlink_metadata(&record).is_err(),
        "backup já existe; não será recriado"
    );
    ensure!(!Path::new(CONFIG).exists(), "instalação já separada; use reinstalação, não migração");
    inside(&hub.home, data)?;
    // As origens que a instalação vai copiar precisam ser legíveis pela conta do Hub antes da parada.
    open_as_hub(hub, host)?;
    open_as_hub(hub, helper)?;
    as_hub(hub, &[], || {
        // A cópia é gravada com a identidade do Hub: /var/backups precisa ser atravessável por ela.
        match fs::symlink_metadata(backup) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => bail!("o Hub não alcança /var/backups: {error}"),
            Ok(_) => bail!("backup já existe; não será recriado"),
        }
        safe(data)?;
        ensure!(data.is_dir() && fs::metadata(data)?.uid() == hub.uid, "data_dir deve pertencer ao Hub");
        Ok(())
    })?;
    let _ = f3_inventory(hub, host, data)?; // pré-validação sem parar serviço
    // /health só usa loopback, sem segredo, e precisa confirmar o Hub e a inatividade.
    let health: serde_json::Value = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{port}/health"))
        .timeout(Duration::from_secs(5))
        .send()
        .await?
        .json()
        .await?;
    ensure!(
        health["ok"] == true && health["busy"] == false && health["environment"] == "main",
        "Hub deve responder e estar busy:false antes da migração"
    );
    let link = hub.home.join(".local/bin/donnabot-host");
    let (old_binary, link_target) = as_hub(hub, &[], || {
        let old_binary = link.canonicalize().context("binário antigo no caminho conhecido pelo app ausente")?;
        safe(&old_binary)?;
        hashes_excluding(data, &[], Some(data))?;
        Ok((old_binary, fs::read_link(&link).ok()))
    })?;
    inside(&hub.home, &old_binary)?;
    ensure!(
        health["binaryPath"].as_str() == old_binary.to_str(),
        "Hub ativo e link do app apontam para binários distintos"
    );
    let result = (|| {
        service(hub, &["stop", "donnabot-host.service"])?;
        // O guard é solto ao sair da tentativa, antes de qualquer recuperação do serviço.
        let lock = as_hub(hub, &[], || {
            Ok(fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(data.join("host.lock"))?)
        })?;
        ensure!(crate::lock_file(&lock, crate::FLOCK_WAIT)?, "Hub ainda está ativo; migração recusada");
        let items = f3_inventory(hub, host, data)?;
        directory(backup, hub.uid, hub.gid, 0o700)?;
        no_acl(backup)?;
        // Tudo o que é lido ou gravado em árvores do Hub usa a identidade de arquivos do Hub.
        let (unit_present, binary_mode, modes, backup_hashes) = as_hub(hub, &[], || {
            copy_excluding(data, &backup.join("data"), hub.uid, hub.gid, 0o700, 0o600, (&[], Some(data)))?;
            ensure!(
                hashes_excluding(data, &[], Some(data))? == hashes(&backup.join("data"))?,
                "hash/contagem diverge no backup do data_dir"
            );
            place(&old_binary, &backup.join("host"), hub.uid, hub.gid, 0o600)?;
            let unit_present = unit(hub).exists();
            if unit_present {
                place(&unit(hub), &backup.join("unit"), hub.uid, hub.gid, 0o600)?;
            }
            directory(&backup.join("profiles-work"), hub.uid, hub.gid, 0o700)?;
            for (i, item) in items.iter().enumerate() {
                copy_tree(
                    &item.source,
                    &backup.join("profiles-work").join(i.to_string()),
                    hub.uid,
                    hub.gid,
                    0o700,
                    0o600,
                )?;
            }
            private_backup(backup, hub.uid)?;
            Ok((unit_present, fs::metadata(&old_binary)?.mode() & 0o777, data_modes(data)?, hashes(backup)?))
        })?;
        let manifest = Manifest {
            hub_uid: hub.uid,
            data: data.into(),
            binary: old_binary.clone(),
            binary_mode,
            data_modes: modes,
            unit_present,
            link_target,
            hashes: backup_hashes,
        };
        // Destinos e modos da volta ficam num registro do root, fora da cópia gravável pelo Hub.
        write_atomic(&record, &serde_json::to_vec_pretty(&manifest)?, 0, 0, 0o600)?;
        // Só agora o caminho antigo já está salvo e pode virar link estável.
        as_hub(hub, &[], || Ok(fs::remove_file(&link)?))?;
        install(hub, host, helper, data, port, false)?;
        let config = fs::read_to_string(CONFIG)?;
        let gid: u32 = config.lines().find_map(|s| s.strip_prefix("work_gid=")).context("work_gid ausente")?.parse()?;
        let motor_text = checked(Command::new(system("getent")?).args(["passwd", "donnabot-motor"]))?;
        let motor_text = String::from_utf8(motor_text.stdout)?;
        let uid: u32 = motor_text.split(':').nth(2).context("motor inválido")?.parse()?;
        let motor = user(uid)?;
        for (i, item) in items.iter().enumerate() {
            let excluded: Vec<_> = if matches!(item.kind, Kind::Workspace) {
                items
                    .iter()
                    .filter(|other| matches!(other.kind, Kind::Uploads) && other.source.starts_with(&item.source))
                    .map(|other| other.source.clone())
                    .collect()
            } else {
                Vec::new()
            };
            let source_hashes = as_hub(hub, &[], || {
                ensure!(
                    hashes(&item.source)? == hashes(&backup.join("profiles-work").join(i.to_string()))?,
                    "hash diverge do backup {i}"
                );
                hashes_excluding(&item.source, &excluded, None)
            })?;
            let destination_hashes = if matches!(item.kind, Kind::Profile) {
                // A raiz do motor não é do Hub: cópia ancorada em descritores, origem só do Hub.
                inside(&hub.home, &item.source)?;
                let destination_parent = anchored_parents(
                    item.destination.parent().context("destino sem pai")?,
                    Path::new(MOTOR),
                    uid,
                    motor.gid,
                )?;
                copy_profile(
                    &Directory::open(item.source.parent().context("origem sem pai")?)?,
                    item.source.file_name().context("origem sem nome")?,
                    &destination_parent,
                    item.destination.file_name().context("destino sem nome")?,
                    hub.uid,
                    (uid, motor.gid),
                )?;
                as_user(uid, motor.gid, &[], || hashes(&item.destination))?
            } else {
                let (dm, fm) = if matches!(item.kind, Kind::Workspace) { (0o2770, 0o660) } else { (0o2750, 0o640) };
                as_hub(hub, &[gid], || {
                    parents(
                        item.destination.parent().context("destino sem pai")?,
                        Path::new(WORK),
                        hub.uid,
                        gid,
                        0o2770,
                    )?;
                    copy_excluding(&item.source, &item.destination, hub.uid, gid, dm, fm, (&excluded, None))?;
                    hashes(&item.destination)
                })?
            };
            ensure!(source_hashes == destination_hashes, "hash/contagem diverge na cópia {i}");
        }
        checked(
            user_command(hub, Path::new(BIN).join("donnabot-host"))?
                .arg("migrar-raizes")
                .arg("--data-dir")
                .arg(data)
                .arg("--trava-stdin")
                .stdin(std::process::Stdio::from(lock.try_clone()?)),
        )?;
        Ok(())
    })();
    recover_before_manifest(result, backup, || service(hub, &["start", "donnabot-host.service"]))?;
    service(hub, &["enable", "--now", "donnabot-host.service"])?;
    println!("Cópia e troca concluídas; ainda não há aceite. {CHECKLIST}");
    Ok(())
}

use std::process::Command;

fn recover_before_manifest(result: Result<()>, backup: &Path, restart: impl FnOnce() -> Result<()>) -> Result<()> {
    if let Err(error) = result {
        if fs::symlink_metadata(record_path(backup)).is_err() {
            let recovery = match restart() {
                Ok(()) => "Hub antigo religado".to_owned(),
                Err(restart_error) => format!(
                    "Hub antigo não religou: {restart_error:#}; execute systemctl --user start donnabot-host.service"
                ),
            };
            return Err(error.context(format!(
                "migração parou antes do manifesto; dados, binário e unidade antigos não foram trocados; {recovery}. \
                 Não use voltar sem manifesto; cópia parcial em {}",
                backup.display()
            )));
        }
        return Err(error.context(format!(
            "migração parou; cópia em {}. Use migrar-motor voltar --instalacao-de-teste --backup CAMINHO",
            backup.display()
        )));
    }
    Ok(())
}

fn deactivate_config(
    source: &Path,
    inactive: &Path,
    rename: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> Result<()> {
    safe(source)?;
    safe(inactive)?;
    match rename(source, inactive) {
        Ok(()) => Ok(()),
        Err(error) if error.raw_os_error() == Some(libc::EXDEV) => {
            let meta = fs::symlink_metadata(source)?;
            ensure!(meta.is_file(), "configuração deve ser arquivo regular");
            // place usa O_NOFOLLOW, cópia sincronizada e troca atômica no FS do destino.
            place(source, inactive, meta.uid(), meta.gid(), meta.mode() & 0o7777)?;
            fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(inactive)?
                .sync_all()?;
            Directory::open(inactive.parent().context("configuração inativa sem pai")?)?.0.sync_all()?;
            // Só remove a origem depois de persistir conteúdo, modo e entrada do destino.
            fs::remove_file(source)?;
            Directory::open(source.parent().context("configuração sem pai")?)?.0.sync_all()?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn record_path(backup: &Path) -> PathBuf {
    let mut name = backup.file_name().unwrap_or_default().to_owned();
    name.push(".manifesto.json");
    backup.with_file_name(name)
}

fn trusted_manifest(hub_uid: u32, home: &Path, backup: &Path, owner: u32) -> Result<Manifest> {
    let record = record_path(backup);
    safe(&record)?;
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&record)
        .context("registro protegido da cópia ausente; volta recusada")?;
    let meta = file.metadata()?;
    ensure!(
        meta.is_file() && meta.uid() == owner && meta.mode() & 0o7777 == 0o600 && meta.nlink() == 1,
        "registro da cópia sem dono e modo protegidos; volta recusada"
    );
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    ensure!(manifest.hub_uid == hub_uid, "cópia pertence a outro Hub");
    inside(home, &manifest.data)?;
    inside(home, &manifest.binary)?;
    ensure!(
        manifest.binary_mode & !0o777 == 0
            && manifest.data_modes.iter().all(|(relative, mode)| {
                let relative = Path::new(relative);
                mode & !0o777 == 0 && relative.components().all(|c| matches!(c, Component::Normal(_)))
            }),
        "modo especial ou caminho fora do data_dir no registro"
    );
    Ok(manifest)
}

fn checked_manifest(hub_uid: u32, home: &Path, backup: &Path) -> Result<Manifest> {
    trusted_manifest(hub_uid, home, backup, 0)
}

fn verify_backup(hub_uid: u32, backup: &Path, manifest: &Manifest) -> Result<()> {
    no_acl(backup)?;
    let meta = fs::symlink_metadata(backup)?;
    ensure!(meta.is_dir() && meta.uid() == hub_uid && meta.mode() & 0o7777 == 0o700, "backup deve ser Hub 0700");
    private_backup(backup, hub_uid)?;
    ensure!(hashes(backup)? == manifest.hashes, "hashes da cópia divergiram; nada restaurado");
    ensure!(manifest.data.is_dir(), "data_dir atual ausente");
    Ok(())
}

fn restore_hub_files(hub: &User, backup: &Path, manifest: &Manifest) -> Result<PathBuf> {
    let stable = hub.home.join(".local/bin/donnabot-host");
    safe(stable.parent().context("link sem pai")?)?;
    if manifest.binary != stable {
        safe(&manifest.binary)?;
    }
    safe(&manifest.data)?;
    let after = manifest.data.with_file_name(format!("donnabot-pos-troca-{}", uuid::Uuid::new_v4()));
    directory(&after, hub.uid, hub.gid, 0o700)?;
    // O diretório ativo e seu host.lock nunca mudam de inode durante a volta.
    super::rollback_data::restore(&manifest.data, &backup.join("data"), &after, |source, destination| {
        copy_tree(source, destination, hub.uid, hub.gid, 0o700, 0o600)
    })?;
    // Conteúdo restaurado confere com o registro do root, não com a cópia gravável.
    let expected: BTreeMap<String, String> = manifest
        .hashes
        .iter()
        .filter_map(|(key, value)| {
            let relative = key.strip_prefix("data/")?;
            let relative = if relative.is_empty() { "/" } else { relative };
            (relative != "host.lock").then(|| (relative.to_owned(), value.clone()))
        })
        .collect();
    ensure!(
        expected == hashes_excluding(&manifest.data, &[manifest.data.join("host.lock")], Some(&manifest.data))?,
        "hash diverge na restauração de dados"
    );
    if manifest.binary == stable && stable.is_symlink() {
        ensure!(fs::read_link(&stable)? == Path::new(BIN).join("donnabot-host"), "link estável mudou de destino");
        fs::remove_file(&stable)?;
    }
    for (relative, mode) in manifest.data_modes.iter().rev() {
        owner_mode(&manifest.data.join(relative), hub.uid, hub.gid, *mode)?;
    }
    place(&backup.join("host"), &manifest.binary, hub.uid, hub.gid, manifest.binary_mode)?;
    ensure!(manifest.hashes.get("host") == Some(&digest(&manifest.binary)?), "hash do binário restaurado diverge");
    if let Some(target) = &manifest.link_target {
        if fs::symlink_metadata(&stable).is_ok() {
            fs::remove_file(&stable)?;
        }
        std::os::unix::fs::symlink(target, &stable)?;
        ensure!(
            stable.canonicalize()? == manifest.binary,
            "link do app restaurado não leva ao binário conferido: {}",
            manifest.binary.display()
        );
    } else {
        if stable.is_symlink() {
            fs::remove_file(&stable)?;
        }
        place(&backup.join("host"), &stable, hub.uid, hub.gid, manifest.binary_mode)?;
        ensure!(manifest.hashes.get("host") == Some(&digest(&stable)?), "hash do link do app diverge");
    }
    if manifest.unit_present {
        place(&backup.join("unit"), &unit(hub), hub.uid, hub.gid, 0o600)?;
        ensure!(manifest.hashes.get("unit") == Some(&digest(&unit(hub))?), "hash da unidade restaurada diverge");
    } else if unit(hub).exists() {
        fs::remove_file(unit(hub))?;
    }
    Ok(after)
}

pub(super) fn rollback(hub: &User, backup: &Path) -> Result<()> {
    validate_backup(backup)?;
    let manifest = checked_manifest(hub.uid, &hub.home, backup)?;
    let unit_present = as_hub(hub, &[], || {
        verify_backup(hub.uid, backup, &manifest)?;
        Ok(unit(hub).exists())
    })?;
    if unit_present {
        service(hub, &["stop", "donnabot-host.service"])?;
    }
    let lock = as_hub(hub, &[], || {
        Ok(fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(manifest.data.join("host.lock"))?)
    })?;
    ensure!(crate::lock_file(&lock, crate::FLOCK_WAIT)?, "Hub ainda ativo; volta recusada");
    // Nada nas árvores do Hub é gravado com poderes de root.
    let after = as_hub(hub, &[], || restore_hub_files(hub, backup, &manifest))?;
    if Path::new(CONFIG).exists() {
        // Ajudante permanece inerte e sem configuração, conforme plano de volta.
        checked(user_command(hub, Path::new(BIN).join("donnabot-motor-exec"))?.arg("cleanup"))?;
        safe(Path::new(CONFIG))?;
        deactivate_config(Path::new(CONFIG), &backup.with_extension("motor-conf-inativo"), |source, target| {
            fs::rename(source, target)
        })?;
    }
    drop(lock);
    service(hub, &["daemon-reload"])?;
    if manifest.unit_present {
        service(hub, &["enable", "--now", "donnabot-host.service"])?;
    }
    println!(
        "Volta concluída e hashes iguais. Dados após troca preservados em {}. Conferir sentinela e bot pela Operação.",
        after.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::{
        fs::{OpenOptionsExt, PermissionsExt, symlink},
        net::UnixListener,
    };

    fn private_file(path: &Path, bytes: &[u8]) -> Result<()> {
        use std::io::Write;
        fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?.write_all(bytes)?;
        Ok(())
    }

    fn private_dir(path: &Path) -> Result<()> {
        fs::create_dir(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        Ok(())
    }

    struct Volta {
        dir: PathBuf,
        home: PathBuf,
        backup: PathBuf,
        manifest: Manifest,
    }

    fn volta_ficticia() -> Result<Volta> {
        let dir = std::env::temp_dir().join(format!("donna-f4-volta-{}", uuid::Uuid::new_v4()));
        let home = dir.join("home");
        let data = home.join("dados");
        fs::create_dir_all(&data)?;
        let backup = dir.join("donnabot-teste");
        private_dir(&backup)?;
        private_dir(&backup.join("data"))?;
        private_file(&backup.join("data/codync.db"), b"banco ficticio")?;
        private_file(&backup.join("host"), b"binario ficticio")?;
        // SAFETY: getter da identidade corrente, sem efeitos colaterais.
        let uid = unsafe { libc::geteuid() };
        let manifest = Manifest {
            hub_uid: uid,
            data,
            binary: home.join(".local/bin/donnabot-host"),
            binary_mode: 0o755,
            data_modes: BTreeMap::from([(String::new(), 0o700), ("codync.db".into(), 0o600)]),
            unit_present: false,
            link_target: None,
            hashes: hashes(&backup)?,
        };
        Ok(Volta { dir, home, backup, manifest })
    }

    #[test]
    fn volta_recusa_manifesto_alterado_pela_conta_do_hub_depois_do_backup() -> Result<()> {
        let Volta { dir, home, backup, manifest } = volta_ficticia()?;
        let uid = manifest.hub_uid;
        let record = record_path(&backup);
        private_file(&record, &serde_json::to_vec(&manifest)?)?;
        let alterados = [
            Manifest { data: PathBuf::from("/etc"), ..manifest.clone() },
            Manifest { binary: PathBuf::from("/etc/donnabot-volta-teste"), ..manifest.clone() },
            Manifest { binary_mode: 0o4755, ..manifest.clone() },
            Manifest { data_modes: BTreeMap::from([("../../etc".into(), 0o600)]), ..manifest.clone() },
        ];
        let mut ignorados = Vec::new();
        for alterado in &alterados {
            // A conta do Hub grava a cópia dentro do backup; a volta segue só o registro do root.
            let _ = fs::remove_file(backup.join("manifest.json"));
            private_file(&backup.join("manifest.json"), &serde_json::to_vec(alterado)?)?;
            let lido = trusted_manifest(uid, &home, &backup, uid)?;
            ignorados.push(
                lido.data == manifest.data
                    && lido.binary == manifest.binary
                    && lido.binary_mode == manifest.binary_mode
                    && lido.data_modes == manifest.data_modes,
            );
        }
        let _ = fs::remove_file(backup.join("manifest.json"));
        private_file(&backup.join("manifest.json"), &serde_json::to_vec(&manifest)?)?;
        fs::remove_file(&record)?;
        let sem_registro = checked_manifest(uid, &home, &backup)
            .is_err_and(|e| format!("{e:#}").contains("registro protegido da cópia ausente"));
        fs::remove_dir_all(&dir)?;
        assert_eq!(ignorados, [true; 4], "destinos e modos alterados no backup não podem orientar a volta");
        assert!(sem_registro, "sem registro protegido do root, a volta é recusada");
        Ok(())
    }

    #[test]
    fn volta_usa_so_registro_protegido_e_confina_destinos_a_home() -> Result<()> {
        let Volta { dir, home, backup, manifest } = volta_ficticia()?;
        let uid = manifest.hub_uid;
        let record = record_path(&backup);
        let gravar = |manifest: &Manifest| -> Result<()> {
            let _ = fs::remove_file(&record);
            private_file(&record, &serde_json::to_vec(manifest)?)
        };
        gravar(&manifest)?;
        let alterado = Manifest { binary: PathBuf::from("/etc/donnabot-volta-teste"), ..manifest.clone() };
        private_file(&backup.join("manifest.json"), &serde_json::to_vec(&alterado)?)?;
        assert_eq!(trusted_manifest(uid, &home, &backup, uid)?.binary, manifest.binary);
        assert!(trusted_manifest(uid, &home, &backup, uid + 1).is_err(), "registro de outro dono recusado");
        fs::set_permissions(&record, fs::Permissions::from_mode(0o644))?;
        assert!(trusted_manifest(uid, &home, &backup, uid).is_err(), "registro com modo aberto recusado");
        fs::remove_file(&record)?;
        let real = dir.join("registro-real");
        private_file(&real, &serde_json::to_vec(&manifest)?)?;
        symlink(&real, &record)?;
        assert!(trusted_manifest(uid, &home, &backup, uid).is_err(), "registro por link recusado");
        fs::remove_file(&record)?;
        gravar(&manifest)?;
        fs::hard_link(&record, dir.join("segundo-nome"))?;
        assert!(trusted_manifest(uid, &home, &backup, uid).is_err(), "registro com hardlink recusado");
        fs::remove_file(dir.join("segundo-nome"))?;
        for fora in [
            Manifest { data: PathBuf::from("/etc"), ..manifest.clone() },
            Manifest { data: home.clone(), ..manifest.clone() },
            Manifest { binary: home.join("../fora"), ..manifest.clone() },
            Manifest { binary_mode: 0o4755, ..manifest.clone() },
            Manifest { data_modes: BTreeMap::from([("../../etc".into(), 0o600)]), ..manifest.clone() },
            Manifest { data_modes: BTreeMap::from([("codync.db".into(), 0o4600)]), ..manifest.clone() },
            Manifest { data_modes: BTreeMap::from([("/etc".into(), 0o600)]), ..manifest.clone() },
            Manifest { hub_uid: uid + 1, ..manifest.clone() },
        ] {
            gravar(&fora)?;
            assert!(trusted_manifest(uid, &home, &backup, uid).is_err(), "registro fora das raízes recusado");
        }
        gravar(&manifest)?;
        assert!(trusted_manifest(uid, &home, &backup, uid).is_ok());
        assert!(inside(Path::new("/"), Path::new("/etc/x")).is_err());
        let slash = Path::new("/var/backups/donnabot-teste/");
        assert!(validate_backup(slash).is_err(), "barra final no backup recusada");
        assert_eq!(record_path(slash), Path::new("/var/backups/donnabot-teste.manifesto.json"));
        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn volta_recusa_copia_alterada_depois_do_registro() -> Result<()> {
        let Volta { dir, backup, manifest, .. } = volta_ficticia()?;
        let uid = manifest.hub_uid;
        verify_backup(uid, &backup, &manifest)?;
        fs::write(backup.join("host"), b"binario trocado")?;
        assert!(verify_backup(uid, &backup, &manifest).is_err());
        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn perfil_copiado_por_descritor_recusa_link_e_preserva_conteudo() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("donna-f4-perfil-{}", uuid::Uuid::new_v4()));
        let source = dir.join("home/.codex-ficticio");
        fs::create_dir_all(source.join("sessions"))?;
        fs::write(source.join("auth.json"), b"{}")?;
        fs::write(source.join("sessions/a.jsonl"), b"linha")?;
        fs::write(dir.join("home/.claude.json"), b"{}")?;
        let motor = dir.join("motor");
        fs::create_dir(&motor)?;
        // SAFETY: getters da identidade corrente, sem efeitos colaterais.
        let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
        let destination = anchored_parents(&motor.join("contas/a"), &motor, uid, gid)?;
        let from = Directory::open(&dir.join("home"))?;
        copy_profile(&from, OsStr::new(".codex-ficticio"), &destination, OsStr::new(".codex"), uid, (uid, gid))?;
        copy_profile(&from, OsStr::new(".claude.json"), &destination, OsStr::new(".claude.json"), uid, (uid, gid))?;
        let copied = motor.join("contas/a/.codex");
        assert_eq!(hashes(&source)?, hashes(&copied)?);
        assert_eq!(fs::metadata(copied.join("sessions/a.jsonl"))?.mode() & 0o7777, 0o600);
        assert_eq!(fs::metadata(copied.join("sessions"))?.mode() & 0o7777, 0o700);
        assert_eq!(fs::read(motor.join("contas/a/.claude.json"))?, b"{}");
        symlink("/etc/hostname", source.join("sessions/link"))?;
        let again = anchored_parents(&motor.join("contas/b"), &motor, uid, gid)?;
        assert!(
            copy_profile(&from, OsStr::new(".codex-ficticio"), &again, OsStr::new(".codex"), uid, (uid, gid)).is_err()
        );
        assert!(!motor.join("contas/b/.codex/sessions/link").exists());
        assert!(
            copy_profile(&from, OsStr::new(".codex-ficticio"), &again, OsStr::new(".x"), uid + 1, (uid, gid)).is_err()
        );
        assert!(
            copy_profile(&from, OsStr::new(".claude.json"), &again, OsStr::new(".y"), uid + 1, (uid, gid)).is_err()
        );
        assert!(copy_profile(&from, OsStr::new(".claude.json"), &again, OsStr::new("../z"), uid, (uid, gid)).is_err());
        let fifo = CString::new(dir.join("home/fila").as_os_str().as_bytes())?;
        // SAFETY: caminho terminado em NUL dentro da pasta temporária do teste.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(copy_profile(&from, OsStr::new("fila"), &again, OsStr::new("fila"), uid, (uid, gid)).is_err());
        assert!(again.names()?.iter().all(|n| !n.to_string_lossy().starts_with(".donna-")), "temporário removido");
        assert!(as_user(0, gid, &[], || Ok(())).is_err());
        assert!(as_user(uid.max(1), 0, &[], || Ok(())).is_err());
        assert!(as_user(uid.max(1), gid.max(1), &[0], || Ok(())).is_err());
        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn identidade_do_hub_mantem_grupos_suplementares_e_recusa_root() -> Result<()> {
        // SAFETY: getter da identidade corrente, sem efeitos colaterais.
        let hub = user(unsafe { libc::geteuid() })?;
        let groups = super::super::identity::user_groups(&hub.name, hub.gid)?;
        assert!(groups.contains(&hub.gid));
        let root = user(0)?;
        assert!(super::super::identity::user_groups(&root.name, root.gid)?.contains(&0));
        assert!(as_user(hub.uid.max(1), hub.gid.max(1), &[0], || Ok(())).is_err(), "grupo root nunca entra");
        Ok(())
    }

    #[test]
    fn desativa_config_com_exdev_sem_perder_conteudo_dono_ou_modo() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("donna-f4-exdev-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(dir.join("etc"))?;
        fs::create_dir_all(dir.join("backup"))?;
        let source = dir.join("etc/motor.conf");
        let target = dir.join("backup/motor-conf-inativo");
        fs::write(&source, b"configuracao ficticia")?;
        fs::set_permissions(&source, fs::Permissions::from_mode(0o644))?;
        let original = fs::metadata(&source)?;
        let mut invoked = false;
        deactivate_config(&source, &target, |_, _| {
            invoked = true;
            Err(std::io::Error::from_raw_os_error(libc::EXDEV))
        })?;
        assert!(invoked && !source.exists());
        assert_eq!(fs::read(&target)?, b"configuracao ficticia");
        let copied = fs::metadata(&target)?;
        assert_eq!((copied.uid(), copied.gid(), copied.mode()), (original.uid(), original.gid(), original.mode()));
        fs::remove_dir_all(dir)?;
        Ok(())
    }

    #[test]
    fn erro_antes_manifest_religa_hub_e_nao_indica_volta_inexistente() -> Result<()> {
        let backup = std::env::temp_dir().join(format!("donna-f4-recuperar-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&backup)?;
        let mut restarted = false;
        let error = recover_before_manifest(Err(anyhow::anyhow!("falha ficticia")), &backup, || {
            restarted = true;
            Ok(())
        })
        .unwrap_err();
        let message = format!("{error:#}");
        assert!(restarted && message.contains("Hub antigo religado"));
        assert!(message.contains("não foram trocados") && !message.contains("Use migrar-motor voltar"));
        let error = recover_before_manifest(Err(anyhow::anyhow!("falha ficticia")), &backup, || {
            Err(anyhow::anyhow!("restart ficticio falhou"))
        })
        .unwrap_err();
        assert!(error.to_string().contains("systemctl --user start donnabot-host.service"));
        fs::write(record_path(&backup), b"{}")?;
        let error = recover_before_manifest(Err(anyhow::anyhow!("falha ficticia depois")), &backup, || {
            panic!("não reiniciar serviço sobre troca iniciada")
        })
        .unwrap_err();
        assert!(error.to_string().contains("Use migrar-motor voltar"));
        fs::remove_file(record_path(&backup))?;
        fs::remove_dir_all(backup)?;
        Ok(())
    }

    #[test]
    fn data_dir_ignora_so_socket_screen_exato_e_recusa_outros_sockets_e_link() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("donna-f4-socket-{}", uuid::Uuid::new_v4()));
        let data = dir.join("data");
        let backup = dir.join("backup");
        fs::create_dir_all(data.join("nested"))?;
        let socket = data.join("screen.sock");
        let listener = UnixListener::bind(&socket)?;
        // SAFETY: getters da identidade corrente, sem efeitos colaterais.
        let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
        let copy = || copy_excluding(&data, &backup, uid, gid, 0o700, 0o600, (&[], Some(&data)));
        copy()?;
        assert!(!backup.join("screen.sock").exists());
        assert!(!data_modes(&data)?.contains_key("screen.sock"));
        assert_eq!(hashes_excluding(&data, &[], Some(&data))?, hashes(&backup)?);
        // Inventários de workspace/perfil não recebem a exceção do data_dir.
        assert!(hashes(&data).is_err());
        for relative in ["outro.sock", "nested/screen.sock"] {
            let unexpected = data.join(relative);
            let other = UnixListener::bind(&unexpected)?;
            assert!(hashes_excluding(&data, &[], Some(&data)).is_err());
            assert!(copy().is_err());
            assert!(data_modes(&data).is_err());
            drop(other);
            fs::remove_file(unexpected)?;
        }
        drop(listener);
        fs::remove_file(&socket)?;
        let target = data.join("arquivo");
        fs::write(&target, b"dado ficticio")?;
        symlink(&target, &socket)?;
        assert!(hashes_excluding(&data, &[], Some(&data)).is_err());
        assert!(copy().is_err());
        assert!(data_modes(&data).is_err());
        fs::remove_file(&socket)?;
        // Arquivo regular com esse nome é dado: continua no hash e na cópia.
        fs::write(&socket, b"arquivo regular ficticio")?;
        copy()?;
        assert!(hashes_excluding(&data, &[], Some(&data))?.contains_key("screen.sock"));
        assert_eq!(fs::read(backup.join("screen.sock"))?, fs::read(&socket)?);
        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn copia_workspace_transfere_execucao_do_dono_ao_grupo_sem_alterar_backup_privado() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("donna-f4-modo-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir)?;
        let source = dir.join("script.sh");
        fs::write(&source, b"#!/bin/sh\nexit 0\n")?;
        fs::set_permissions(&source, fs::Permissions::from_mode(0o700))?;
        // SAFETY: getters da identidade corrente, sem efeitos colaterais.
        let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
        let copied = dir.join("copiado.sh");
        copy_tree(&source, &copied, uid, gid, 0o2770, 0o660)?;
        assert_eq!(fs::metadata(&copied)?.mode() & 0o7777, 0o770);
        assert_eq!(fs::read(&copied)?, fs::read(&source)?);
        let backup = dir.join("backup.sh");
        copy_tree(&source, &backup, uid, gid, 0o700, 0o600)?;
        assert_eq!(fs::metadata(&backup)?.mode() & 0o7777, 0o600);
        fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
