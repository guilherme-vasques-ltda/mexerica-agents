//! Orquestra cópia e volta. Só a F3 conhece e reescreve referências do banco.
use super::root::{
    BASE, BIN, CONFIG, MOTOR, User, WORK, checked, directory, install, owner_mode, place, safe, service, system, unit,
    user, user_command, write_atomic,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
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
#[derive(Serialize, Deserialize)]
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
        result.insert(path.strip_prefix(root)?.to_str().context("nome não UTF-8")?.to_owned(), meta.mode() & 0o7777);
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

fn no_acl(path: &Path) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let path_c = std::ffi::CString::new(path.as_os_str().as_bytes())?;
    for name in [c"system.posix_acl_access", c"system.posix_acl_default"] {
        // SAFETY: caminhos e nomes terminados em NUL; consulta sem buffer de saída.
        let size = unsafe { libc::getxattr(path_c.as_ptr(), name.as_ptr(), std::ptr::null_mut(), 0) };
        let error = std::io::Error::last_os_error();
        ensure!(
            size == -1 && matches!(error.raw_os_error(), Some(libc::ENODATA | libc::ENOTSUP)),
            "cópia de segurança deve estar sem ACL: {}",
            path.display()
        );
    }
    Ok(())
}

fn private_backup(path: &Path, hub: &User) -> Result<()> {
    safe(path)?;
    no_acl(path)?;
    let meta = fs::symlink_metadata(path)?;
    let expected = if meta.is_dir() { 0o700 } else { 0o600 };
    ensure!(
        meta.uid() == hub.uid && meta.mode() & 0o7777 == expected,
        "permissão privada da cópia inválida: {}",
        path.display()
    );
    if meta.is_dir() {
        for entry in fs::read_dir(path)? {
            private_backup(&entry?.path(), hub)?;
        }
    } else {
        ensure!(meta.is_file(), "cópia contém arquivo especial");
    }
    Ok(())
}

fn validate_backup(path: &Path) -> Result<()> {
    safe(path)?;
    ensure!(
        path.parent() == Some(Path::new("/var/backups"))
            && path.file_name().is_some_and(|n| n.to_string_lossy().starts_with("donnabot-")),
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
    for item in &items {
        safe(&item.source)?;
        safe(&item.destination)?;
        ensure!(
            !matches!(item.kind, Kind::Workspace) || !data.starts_with(&item.source),
            "workspace contém dados privados do Hub; origem de cópia recusada"
        );
        let root = match item.kind {
            Kind::Profile => MOTOR,
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
        hashes(&item.source)?; // valida árvore inteira antes da parada
    }
    Ok(items)
}

pub(super) async fn apply(hub: &User, host: &Path, helper: &Path, data: &Path, backup: &Path, port: u16) -> Result<()> {
    validate_backup(backup)?;
    ensure!(!backup.exists(), "backup já existe; não será recriado");
    ensure!(!Path::new(CONFIG).exists(), "instalação já separada; use reinstalação, não migração");
    safe(data)?;
    ensure!(data.is_dir() && fs::metadata(data)?.uid() == hub.uid, "data_dir deve pertencer ao Hub");
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
    let old_binary = link.canonicalize().context("binário antigo no caminho conhecido pelo app ausente")?;
    ensure!(
        health["binaryPath"].as_str() == old_binary.to_str(),
        "Hub ativo e link do app apontam para binários distintos"
    );
    safe(&old_binary)?;
    let link_target = fs::read_link(&link).ok();
    hashes_excluding(data, &[], Some(data))?;
    let result = (|| {
        service(hub, &["stop", "donnabot-host.service"])?;
        // O guard é solto ao sair da tentativa, antes de qualquer recuperação do serviço.
        let lock = fs::OpenOptions::new().read(true).write(true).open(data.join("host.lock"))?;
        ensure!(crate::lock_file(&lock, crate::FLOCK_WAIT)?, "Hub ainda está ativo; migração recusada");
        let items = f3_inventory(hub, host, data)?;
        directory(backup, hub.uid, hub.gid, 0o700)?;
        no_acl(backup)?;
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
            copy_tree(&item.source, &backup.join("profiles-work").join(i.to_string()), hub.uid, hub.gid, 0o700, 0o600)?;
        }
        private_backup(backup, hub)?;
        let manifest = Manifest {
            hub_uid: hub.uid,
            data: data.into(),
            binary: old_binary.clone(),
            binary_mode: fs::metadata(&old_binary)?.mode() & 0o777,
            data_modes: data_modes(data)?,
            unit_present,
            link_target,
            hashes: hashes(backup)?,
        };
        write_atomic(&backup.join("manifest.json"), &serde_json::to_vec_pretty(&manifest)?, hub.uid, hub.gid, 0o600)?;
        // Só agora o caminho antigo já está salvo e pode virar link estável.
        fs::remove_file(&link)?;
        install(hub, host, helper, data, port, false)?;
        let config = fs::read_to_string(CONFIG)?;
        let gid: u32 = config.lines().find_map(|s| s.strip_prefix("work_gid=")).context("work_gid ausente")?.parse()?;
        let motor_text = checked(Command::new(system("getent")?).args(["passwd", "donnabot-motor"]))?;
        let motor_text = String::from_utf8(motor_text.stdout)?;
        let uid: u32 = motor_text.split(':').nth(2).context("motor inválido")?.parse()?;
        let motor = user(uid)?;
        for (i, item) in items.iter().enumerate() {
            let (root, owner, group, dm, fm) = match item.kind {
                Kind::Profile => (MOTOR, uid, motor.gid, 0o700, 0o600),
                Kind::Workspace => (WORK, hub.uid, gid, 0o2770, 0o660),
                Kind::Uploads | Kind::Skills => (WORK, hub.uid, gid, 0o2750, 0o640),
            };
            parents(
                item.destination.parent().context("destino sem pai")?,
                Path::new(root),
                owner,
                group,
                if root == WORK { 0o2770 } else { 0o700 },
            )?;
            let excluded: Vec<_> = if matches!(item.kind, Kind::Workspace) {
                items
                    .iter()
                    .filter(|other| matches!(other.kind, Kind::Uploads) && other.source.starts_with(&item.source))
                    .map(|other| other.source.clone())
                    .collect()
            } else {
                Vec::new()
            };
            copy_excluding(&item.source, &item.destination, owner, group, dm, fm, (&excluded, None))?;
            ensure!(
                hashes_excluding(&item.source, &excluded, None)? == hashes(&item.destination)?,
                "hash/contagem diverge na cópia {i}"
            );
            ensure!(
                hashes(&item.source)? == hashes(&backup.join("profiles-work").join(i.to_string()))?,
                "hash diverge do backup {i}"
            );
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
        if !backup.join("manifest.json").exists() {
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
            crate::fork::motor_exec::fd::Directory::open(inactive.parent().context("configuração inativa sem pai")?)?
                .0
                .sync_all()?;
            // Só remove a origem depois de persistir conteúdo, modo e entrada do destino.
            fs::remove_file(source)?;
            crate::fork::motor_exec::fd::Directory::open(source.parent().context("configuração sem pai")?)?
                .0
                .sync_all()?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

pub(super) fn rollback(hub: &User, backup: &Path) -> Result<()> {
    validate_backup(backup)?;
    no_acl(backup)?;
    let meta = fs::metadata(backup)?;
    ensure!(meta.uid() == hub.uid && meta.mode() & 0o7777 == 0o700, "backup deve ser Hub 0700");
    private_backup(backup, hub)?;
    let manifest: Manifest = serde_json::from_slice(&fs::read(backup.join("manifest.json"))?)?;
    ensure!(manifest.hub_uid == hub.uid, "cópia pertence a outro Hub");
    let mut actual = hashes(backup)?;
    actual.remove("manifest.json");
    ensure!(actual == manifest.hashes, "hashes da cópia divergiram; nada restaurado");
    safe(&manifest.data)?;
    let stable = hub.home.join(".local/bin/donnabot-host");
    if manifest.binary == stable {
        safe(stable.parent().context("link sem pai")?)?;
    } else {
        safe(&manifest.binary)?;
    }
    ensure!(manifest.data.is_dir(), "data_dir atual ausente");
    if unit(hub).exists() {
        service(hub, &["stop", "donnabot-host.service"])?;
    }
    let lock = fs::OpenOptions::new().read(true).write(true).open(manifest.data.join("host.lock"))?;
    ensure!(crate::lock_file(&lock, crate::FLOCK_WAIT)?, "Hub ainda ativo; volta recusada");
    let after = manifest.data.with_file_name(format!("donnabot-pos-troca-{}", uuid::Uuid::new_v4()));
    directory(&after, hub.uid, hub.gid, 0o700)?;
    // O diretório ativo e seu host.lock nunca mudam de inode durante a volta.
    super::rollback_data::restore(&manifest.data, &backup.join("data"), &after, |source, destination| {
        copy_tree(source, destination, hub.uid, hub.gid, 0o700, 0o600)
    })?;
    ensure!(
        hashes_excluding(&backup.join("data"), &[backup.join("data/host.lock")], None)?
            == hashes_excluding(&manifest.data, &[manifest.data.join("host.lock")], Some(&manifest.data))?,
        "hash diverge na restauração de dados"
    );
    if manifest.binary == stable && stable.is_symlink() {
        ensure!(fs::read_link(&stable)? == Path::new(BIN).join("donnabot-host"), "link estável mudou de destino");
        fs::remove_file(&stable)?;
    }
    for (relative, mode) in &manifest.data_modes {
        let path = manifest.data.join(relative);
        ensure!(path.starts_with(&manifest.data), "modo fora do data_dir");
        owner_mode(&path, hub.uid, hub.gid, *mode)?;
    }
    place(&backup.join("host"), &manifest.binary, hub.uid, hub.gid, manifest.binary_mode)?;
    ensure!(digest(&backup.join("host"))? == digest(&manifest.binary)?, "hash do binário restaurado diverge");
    let link = hub.home.join(".local/bin/donnabot-host");
    if let Some(target) = manifest.link_target {
        if fs::symlink_metadata(&link).is_ok() {
            fs::remove_file(&link)?;
        }
        std::os::unix::fs::symlink(target, &link)?;
    } else {
        if link.is_symlink() {
            fs::remove_file(&link)?;
        }
        place(&backup.join("host"), &link, hub.uid, hub.gid, manifest.binary_mode)?;
    }
    if manifest.unit_present {
        place(&backup.join("unit"), &unit(hub), hub.uid, hub.gid, 0o600)?;
    } else if unit(hub).exists() {
        fs::remove_file(unit(hub))?;
    }
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
        fs::{PermissionsExt, symlink},
        net::UnixListener,
    };

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
        fs::write(backup.join("manifest.json"), b"{}")?;
        let error = recover_before_manifest(Err(anyhow::anyhow!("falha ficticia depois")), &backup, || {
            panic!("não reiniciar serviço sobre troca iniciada")
        })
        .unwrap_err();
        assert!(error.to_string().contains("Use migrar-motor voltar"));
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
