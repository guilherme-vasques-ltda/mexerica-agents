//! Migração offline das referências; cópia/donos e volta pertencem ao instalador (F4).
use super::{
    engine_accounts::{self, LaunchReference, Registry},
    motor_roots as roots,
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize)]
pub(crate) struct CopyItem {
    source: PathBuf,
    destination: PathBuf,
    kind: &'static str,
}
fn push(items: &mut Vec<CopyItem>, source: PathBuf, destination: PathBuf, kind: &'static str) -> Result<()> {
    ensure!(source.is_absolute() && destination.is_absolute(), "migração exige caminhos absolutos");
    if source == destination {
        return Ok(());
    }
    match std::fs::symlink_metadata(&source) {
        Ok(meta) => {
            ensure!(
                (meta.is_dir() || (kind == "profile" && meta.is_file())) && !meta.file_type().is_symlink(),
                "origem da migração deve ser pasta sem link"
            );
            items.push(CopyItem { source, destination, kind });
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(())
}
fn bots(db: &Connection) -> Result<Vec<(String, String)>> {
    Ok(db
        .prepare("SELECT id,config FROM bots ORDER BY id")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?)
}
/// Gate de compatibilidade F3 usado pelo coordenador da atualização: o
/// inventário de raízes precisa ser obtido sem erro antes de qualquer troca.
pub(crate) fn inventario_compativel(db: &Connection, data: &Path) -> Result<usize> {
    Ok(inventory(db, data)?.len())
}

/// Itens de cópia da F3 (origem, destino, tipo) para o coordenador executar a
/// cópia: raízes de trabalho pelo próprio Hub, perfis pelo canal do motor.
pub(crate) fn inventario_copias(db: &Connection, data: &Path) -> Result<Vec<(PathBuf, PathBuf, &'static str)>> {
    let concluida: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM kv WHERE k='d330.raizes.v1')", [], |r| r.get(0))?;
    if concluida {
        return Ok(Vec::new());
    }
    Ok(inventory(db, data)?.into_iter().map(|i| (i.source, i.destination, i.kind)).collect())
}

/// Reescrita F3 com a `host.lock` já mantida pelo coordenador (mesmo inode).
#[cfg(unix)]
pub(crate) fn migrar_com_trava(data: &Path, trava: &std::fs::File) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let dir = crate::fork::private_fs::Directory::open(data, false)?;
    let atual = dir.file(std::ffi::OsStr::new("host.lock"), libc::O_RDWR)?;
    let (a, b) = (trava.metadata()?, atual.metadata()?);
    ensure!(a.is_file() && a.dev() == b.dev() && a.ino() == b.ino(), "trava não corresponde a host.lock");
    ensure!(crate::lock_file(trava, crate::FLOCK_WAIT)?, "host.lock não está com o coordenador");
    ensure!(super::motor_exec::configured(), "configuração do motor ausente");
    let mut db = Connection::open_with_flags(data.join("codync.db"), OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    migrate(&mut db, data)
}

fn inventory(db: &Connection, data: &Path) -> Result<Vec<CopyItem>> {
    let mut items = Vec::new();
    for (id, raw) in bots(db)? {
        uuid::Uuid::parse_str(&id)?;
        let cfg: crate::store::BotConfig = serde_json::from_str(&raw)?;
        if cfg.is_group() {
            push(&mut items, data.join("bots").join(&id).join("uploads"), roots::uploads(&id), "uploads")?;
            continue;
        }
        let old =
            if cfg.cwd.is_empty() { data.join("bots").join(&id).join("workspace") } else { PathBuf::from(&cfg.cwd) };
        // Nunca copiar HOME inteiro ou um ancestral por inferência do instalador.
        ensure!(
            old.parent().is_some()
                && !data.starts_with(&old)
                && dirs::home_dir().is_none_or(|home| !home.starts_with(&old)),
            "cwd exige escolha explícita de pasta antes da migração"
        );
        let uploads = if cfg.context.is_some() || old == data.join("bots").join(&id).join("workspace") {
            old.join("uploads")
        } else {
            data.join("bots").join(&id).join("uploads")
        };
        // Mais específica primeiro: as referências a anexos não viram workspace/uploads.
        push(&mut items, uploads, roots::uploads(&id), "uploads")?;
        push(&mut items, old, roots::workspace(&id), "workspace")?;
    }
    let has_spaces: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='donna_space_conversations')",
        [],
        |r| r.get(0),
    )?;
    if has_spaces {
        let rows = db
            .prepare("SELECT space_id,id,bot_id FROM donna_space_conversations ORDER BY space_id,id")?
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (space, chat, bot) in rows {
            if chat == bot {
                continue;
            }
            uuid::Uuid::parse_str(&space)?;
            uuid::Uuid::parse_str(&chat)?;
            let source = data.join("spaces").join(&space).join("conversations").join(&chat).join("workspace");
            let target = roots::conversation(&space, &chat);
            push(&mut items, source.join("uploads"), target.join("uploads"), "uploads")?;
            push(&mut items, source, target.join("workspace"), "workspace")?;
        }
    }
    let registry = Registry::load_db(db)?;
    for account in registry.accounts {
        items.extend(profile_items(&account, data)?);
    }
    push(&mut items, data.join("skills"), roots::work().join("skills"), "skills")?;
    Ok(items)
}
fn profile_items(account: &engine_accounts::Account, data: &Path) -> Result<Vec<CopyItem>> {
    let mut items: Vec<CopyItem> = Vec::new();
    let mut profiles = Vec::new();
    if let Some(path) = account.managed.as_ref().and_then(|m| m.profile.clone()) {
        profiles.push(path);
    }
    if let LaunchReference::CliProfile { directory, .. } = &account.launch
        && !profiles.contains(directory)
    {
        profiles.push(directory.clone());
    }
    for source in profiles {
        if source.starts_with(roots::accounts()) {
            continue;
        }
        let old = data.join("engine-setup/accounts").join(&account.id);
        let destination = if let Ok(relative) = source.strip_prefix(&old) {
            roots::accounts().join(&account.id).join(relative)
        } else {
            roots::accounts()
                .join(&account.id)
                .join("legacy")
                .join(if account.backend == engine_accounts::Backend::Codex { ".codex" } else { ".claude" })
        };
        if source.starts_with(&old) {
            let source_home = source.parent().context("perfil sem home")?.to_path_buf();
            let destination_home = destination.parent().context("perfil sem home nova")?.to_path_buf();
            if !items.iter().any(|i| i.source == source_home && i.destination == destination_home) {
                push(&mut items, source_home, destination_home, "profile")?;
            }
        } else {
            if account.backend == engine_accounts::Backend::Claude
                && let Some(parent) = source.parent()
            {
                push(
                    &mut items,
                    parent.join(".claude.json"),
                    destination.parent().context("perfil sem home")?.join(".claude.json"),
                    "profile",
                )?;
            }
            push(&mut items, source, destination, "profile")?;
        }
    }
    Ok(items)
}
fn rewrite_accounts(value: &mut Value, data: &Path) -> Result<()> {
    for entry in value["accounts"].as_array_mut().context("registro sem contas")? {
        let account: engine_accounts::Account = serde_json::from_value(entry.clone())?;
        rewritten(entry, &profile_items(&account, data)?);
    }
    Ok(())
}
fn rewritten(value: &mut Value, items: &[CopyItem]) {
    match value {
        Value::String(value) => {
            if !Path::new(value.as_str()).is_absolute() {
                return;
            }
            if let Some((item, relative)) = items
                .iter()
                .filter_map(|item| Path::new(value.as_str()).strip_prefix(&item.source).ok().map(|r| (item, r)))
                .max_by_key(|(i, _)| i.source.components().count())
            {
                let destination = if relative.as_os_str().is_empty() {
                    item.destination.clone()
                } else {
                    item.destination.join(relative)
                };
                *value = destination.to_string_lossy().into_owned();
            }
        }
        Value::Array(values) => {
            for value in values {
                rewritten(value, items);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                rewritten(value, items);
            }
        }
        _ => {}
    }
}
pub(crate) fn run(data: &Path, list: bool, inherited: bool) -> Result<()> {
    ensure!(data.is_absolute(), "data-dir precisa ser absoluto");
    ensure!(!list || !inherited, "inventário não aceita trava herdada");
    let _lock = if list {
        None
    } else if inherited {
        Some(inherited_lock(data)?)
    } else {
        Some(crate::service::lock_host_at(&data.join("host.lock"))?)
    };
    let flags = if list { OpenFlags::SQLITE_OPEN_READ_ONLY } else { OpenFlags::SQLITE_OPEN_READ_WRITE };
    let mut db = Connection::open_with_flags(data.join("codync.db"), flags)?;
    if list {
        println!("{}", serde_json::to_string(&inventory(&db, data)?)?);
        return Ok(());
    }
    ensure!(super::motor_exec::configured(), "configuração do motor ausente");
    migrate(&mut db, data)
}

#[cfg(unix)]
fn inherited_lock(data: &Path) -> Result<std::fs::File> {
    use std::os::{fd::FromRawFd, unix::fs::MetadataExt};
    // SAFETY: F_DUPFD_CLOEXEC recebe um descritor inteiro e duplica a descrição aberta
    // sem herdar o novo descritor em exec; não altera nem fecha o stdin original.
    let fd = unsafe { libc::fcntl(libc::STDIN_FILENO, libc::F_DUPFD_CLOEXEC, 0) };
    ensure!(fd >= 0, "trava herdada ausente: {}", std::io::Error::last_os_error());
    // SAFETY: fcntl retornou um descritor novo e válido, cuja posse passa ao File uma única vez.
    let lock = unsafe { std::fs::File::from_raw_fd(fd) };
    let dir = crate::fork::private_fs::Directory::open(data, false)?;
    let current = dir.file(std::ffi::OsStr::new("host.lock"), libc::O_RDWR)?;
    let (a, b) = (lock.metadata()?, current.metadata()?);
    ensure!(a.is_file() && a.dev() == b.dev() && a.ino() == b.ino(), "trava herdada não corresponde a host.lock");
    ensure!(crate::lock_file(&lock, crate::FLOCK_WAIT)?, "host.lock ocupado; migração recusada");
    Ok(lock)
}

#[cfg(not(unix))]
fn inherited_lock(_data: &Path) -> Result<std::fs::File> {
    anyhow::bail!("trava herdada não suportada nesta plataforma")
}
fn migrate(db: &mut Connection, data: &Path) -> Result<()> {
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let done: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM kv WHERE k='d330.raizes.v1')", [], |r| r.get(0))?;
    if done {
        return Ok(());
    }
    let items = inventory(&tx, data)?;
    for item in &items {
        if item.kind == "profile" {
            let destination = if item.destination.file_name().is_some_and(|n| n == ".claude.json") {
                item.destination.parent().context("perfil sem home")?
            } else {
                &item.destination
            };
            roots::profile("validar", &[destination.as_os_str()])?;
        } else {
            #[cfg(unix)]
            {
                super::motor_exec::fd::Directory::open(&item.destination)
                    .with_context(|| format!("destino não copiado: {}", item.kind))?;
            }
        }
    }
    let old_bots = bots(&tx)?;
    let old_accounts: Option<String> =
        tx.query_row("SELECT v FROM kv WHERE k=?1", [engine_accounts::SLOT], |r| r.get(0)).optional()?;
    let old_team: Option<String> =
        tx.query_row("SELECT v FROM kv WHERE k='team.settings.v1'", [], |r| r.get(0)).optional()?;
    // Backup é privado no mesmo DB; nunca inclui cópia de credenciais de perfil.
    let backup = json!({"bots":old_bots,"accounts":old_accounts,"team":old_team,"copies":items});
    // A troca de raízes é uma mudança estrutural iniciada pelo dono (atualização com o
    // serviço parado): a pasta da fixa (`cwd`, campo do dono) só muda por esse caminho.
    let reescrever = |c: &Connection| -> Result<()> {
        for (id, raw) in &old_bots {
            let mut cfg: Value = serde_json::from_str(raw)?;
            let public: Vec<_> = items.iter().filter(|i| i.kind != "profile").cloned().collect();
            rewritten(&mut cfg, &public);
            let parsed: crate::store::BotConfig = serde_json::from_value(cfg.clone())?;
            if !parsed.is_group() {
                roots::prepare_workspace(&roots::workspace(id))?;
                cfg["cwd"] = json!(roots::workspace(id));
            }
            let next = serde_json::to_string(&cfg)?;
            if &next != raw {
                let rev = crate::store::next_rev(c)?;
                c.execute("UPDATE bots SET config=?1,rev=?2 WHERE id=?3", params![next, rev, id])?;
            }
        }
        Ok(())
    };
    let com_fixa: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='donna_fixa_dono')",
        [],
        |r| r.get(0),
    )?;
    if com_fixa {
        super::donna_fixa::como_dono(&tx, reescrever)?;
    } else {
        reescrever(&tx)?;
    }
    if let Some(raw) = &old_accounts {
        let mut value: Value = serde_json::from_str(raw)?;
        rewrite_accounts(&mut value, data)?;
        // Valida TODO perfil referenciado; origem ausente não publica caminho antigo.
        let registry: Registry = serde_json::from_value(value.clone())?;
        for account in registry.accounts {
            if let Some(profile) = account.managed.and_then(|m| m.profile) {
                ensure!(profile.starts_with(roots::accounts().join(&account.id)), "perfil gerido sem cópia");
                roots::profile("validar", &[profile.as_os_str()])?;
            }
            if let LaunchReference::CliProfile { directory, .. } = account.launch {
                ensure!(directory.starts_with(roots::accounts().join(&account.id)), "perfil CLI sem cópia");
                roots::profile("validar", &[directory.as_os_str()])?;
            }
        }
        tx.execute("UPDATE kv SET v=?1 WHERE k=?2", params![serde_json::to_string(&value)?, engine_accounts::SLOT])?;
    }
    if let Some(raw) = old_team {
        let mut value: Value = serde_json::from_str(&raw)?;
        value["workspaceRoot"] = json!(roots::work());
        tx.execute("UPDATE kv SET v=?1 WHERE k='team.settings.v1'", [serde_json::to_string(&value)?])?;
    }
    tx.execute("INSERT INTO kv(k,v) VALUES('d330.raizes.v1',?1)", [serde_json::to_string(&backup)?])?;
    tx.commit()?;
    Ok(())
}

#[cfg(all(test, unix))]
mod lock_tests {
    use super::*;
    use fs2::FileExt;
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        process::{Command, Stdio},
    };

    #[test]
    fn migra_banco_com_trava_direta_ou_herdada() -> Result<()> {
        if let Some(path) = std::env::var_os("MX335_TEST_DATA") {
            return run(Path::new(&path), false, std::env::var_os("MX335_TEST_INHERITED").is_some());
        }
        let root = std::env::temp_dir().join(format!("mx335-happy-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root)?;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        for inherited in [false, true] {
            let data = root.join(if inherited { "herdada" } else { "direta" });
            fs::create_dir(&data)?;
            fs::set_permissions(&data, fs::Permissions::from_mode(0o700))?;
            let db = Connection::open(data.join("codync.db"))?;
            db.execute_batch("CREATE TABLE kv(k TEXT PRIMARY KEY,v TEXT); CREATE TABLE bots(id TEXT PRIMARY KEY,config TEXT,rev INTEGER);")?;
            drop(db);
            let lock = fs::OpenOptions::new().create_new(true).read(true).write(true).open(data.join("host.lock"))?;
            let mut command = Command::new(std::env::current_exe()?);
            command
                .args(["--exact", "fork::motor_migration::lock_tests::migra_banco_com_trava_direta_ou_herdada"])
                .env("MX335_TEST_DATA", &data)
                .env("DONNABOT_MOTOR_TEST_HELPER", "1");
            if inherited {
                lock.try_lock_exclusive()?;
                command.env("MX335_TEST_INHERITED", "1").stdin(Stdio::from(lock.try_clone()?));
            }
            let output = command.output()?;
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            let db = Connection::open(data.join("codync.db"))?;
            let done: bool =
                db.query_row("SELECT EXISTS(SELECT 1 FROM kv WHERE k='d330.raizes.v1')", [], |r| r.get(0))?;
            assert!(done, "migração deve gravar recibo");
            if inherited {
                let contender = fs::OpenOptions::new().read(true).write(true).open(data.join("host.lock"))?;
                assert!(contender.try_lock_exclusive().is_err(), "instalador mantém a trava após o filho");
            }
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }
}

#[cfg(test)]
pub(super) mod root_proof {
    use super::*;
    pub(crate) fn proof(temp: &Path) {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch(
        "CREATE TABLE kv(k TEXT PRIMARY KEY,v TEXT); CREATE TABLE bots(id TEXT PRIMARY KEY,config TEXT,rev INTEGER);",
    )
    .unwrap();
        let bot = uuid::Uuid::new_v4().to_string();
        let source = temp.join("origem-workspace");
        std::fs::create_dir(&source).unwrap();
        let config = json!({"id":bot,"name":"Teste D330","backend":"codex","cwd":source});
        db.execute("INSERT INTO bots VALUES(?1,?2,0)", params![bot, config.to_string()]).unwrap();
        // Destino ainda não copiado: erro não muda config nem grava recibo de migração.
        assert!(migrate(&mut db, temp).is_err());
        assert_eq!(bots(&db).unwrap()[0].1, config.to_string());
        roots::prepare_workspace(&roots::workspace(&bot)).unwrap();
        let account = format!("ac_{}", "b".repeat(64));
        let store = crate::store::Store::open(Path::new(":memory:")).unwrap();
        let dest = engine_accounts::managed::new_profile(&store, &account, engine_accounts::Backend::Codex).unwrap();
        let generation = dest.parent().unwrap().file_name().unwrap();
        let old = temp.join("engine-setup/accounts").join(&account).join(generation).join(".codex");
        std::fs::create_dir_all(&old).unwrap();
        let launch = json!({"kind":"cli_profile","executable":"/bin/true","directory":old});
        let registry = json!({"salt":uuid::Uuid::new_v4().to_string(),"accounts":[{
            "id":account,"backend":"codex","billing":"unverified","state":"unverified","launch":launch,"scope":"owner","quotaIdentity":null,"verifiedAt":null,"subscriptionProof":null,"models":[],
            "managed":{"label":"Teste","revision":1,"generation":0,"enabled":false,"requestId":"root-test","principal":null,"profile":old,"setupRequest":null,"catalog":null}
        }]});
        db.execute("INSERT INTO kv VALUES(?1,?2)", params![engine_accounts::SLOT, registry.to_string()]).unwrap();
        // Duas contas históricas usavam a mesma origem global: cópias distintas.
        let shared = temp.join("perfil-global/.codex");
        std::fs::create_dir_all(&shared).unwrap();
        let mut registry = registry;
        let mut legacy_destinations = Vec::new();
        for digit in ["c", "d"] {
            let id = format!("ac_{}", digit.repeat(64));
            let destination = roots::accounts().join(&id).join("legacy/.codex");
            let status = std::process::Command::new(super::super::motor_exec::helper_path())
                .args(["--cwd", "/", "--", "/bin/mkdir", "-p", destination.to_str().unwrap()])
                .env("DONNABOT_MOTOR_PRIVATE", "1")
                .output()
                .unwrap();
            assert!(status.status.success());
            let mut entry = registry["accounts"][0].clone();
            entry["id"] = json!(id);
            entry["managed"] = Value::Null;
            entry["launch"]["directory"] = json!(shared);
            registry["accounts"].as_array_mut().unwrap().push(entry);
            legacy_destinations.push(destination);
        }
        db.execute("UPDATE kv SET v=?1 WHERE k=?2", params![registry.to_string(), engine_accounts::SLOT]).unwrap();
        let mut rewritten_registry = registry.clone();
        rewrite_accounts(&mut rewritten_registry, temp).unwrap();
        for (entry, destination) in
            rewritten_registry["accounts"].as_array().unwrap()[1..].iter().zip(&legacy_destinations)
        {
            assert_eq!(entry["launch"]["directory"], json!(destination));
        }
        let once = rewritten_registry.clone();
        rewrite_accounts(&mut rewritten_registry, temp).unwrap();
        assert_eq!(once, rewritten_registry);
        migrate(&mut db, temp).unwrap();
        let migrated = Registry::load_db(&db).unwrap();
        assert_eq!(migrated.accounts[0].managed.as_ref().unwrap().profile.as_ref(), Some(&dest));
        assert!(
            matches!(&migrated.accounts[0].launch,LaunchReference::CliProfile {directory,..} if directory == &dest)
        );
        for (account, destination) in migrated.accounts[1..].iter().zip(&legacy_destinations) {
            assert!(
                matches!(&account.launch, LaunchReference::CliProfile { directory, .. } if directory == destination)
            );
        }
        let backup: String = db.query_row("SELECT v FROM kv WHERE k='d330.raizes.v1'", [], |r| r.get(0)).unwrap();
        let before = bots(&db).unwrap();
        migrate(&mut db, temp).unwrap();
        assert_eq!(bots(&db).unwrap(), before);
        assert_eq!(
            db.query_row::<String, _, _>("SELECT v FROM kv WHERE k='d330.raizes.v1'", [], |r| r.get(0)).unwrap(),
            backup
        );
        assert_eq!(serde_json::from_str::<Value>(&backup).unwrap()["accounts"], registry.to_string());
        engine_accounts::managed::remove_profile_home(dest.parent().unwrap()).unwrap();
        for destination in legacy_destinations {
            engine_accounts::managed::remove_profile_home(destination.parent().unwrap()).unwrap();
        }
        std::fs::remove_dir_all(roots::bot(&bot)).unwrap();
        println!(
            "PASS migração transacional com backup, Metadata.profile e lançamento; falta de cópia recusa; repetição idempotente"
        );
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn inventario_recusa_dados_e_preserva_anexos_antigos() {
            let root = std::env::temp_dir().join(format!("d330-inventario-{}", uuid::Uuid::new_v4()));
            let data = root.join("dados");
            let id = uuid::Uuid::new_v4().to_string();
            let workspace = data.join("bots").join(&id).join("workspace");
            std::fs::create_dir_all(workspace.join("uploads")).unwrap();
            let db = Connection::open_in_memory().unwrap();
            db.execute_batch("CREATE TABLE kv(k TEXT PRIMARY KEY,v TEXT); CREATE TABLE bots(id TEXT,config TEXT);")
                .unwrap();
            let mut cfg = json!({"id":id,"name":"Teste","cwd":data});
            db.execute("INSERT INTO bots VALUES(?1,?2)", params![id, cfg.to_string()]).unwrap();
            assert!(inventory(&db, &data).is_err());
            cfg["cwd"] = json!(workspace);
            db.execute("UPDATE bots SET config=?1", [cfg.to_string()]).unwrap();
            let items = inventory(&db, &data).unwrap();
            assert!(items.iter().any(|i| i.kind == "uploads"
                && i.source == workspace.join("uploads")
                && i.destination == roots::uploads(&id)));
            std::fs::remove_dir_all(root).unwrap();
        }

        #[test]
        fn perfis_compartilhados_reescrevem_por_conta_sem_colisao() {
            let temp = std::env::temp_dir().join(format!("d330-profile-map-{}", uuid::Uuid::new_v4()));
            let shared = temp.join("shared/.codex");
            std::fs::create_dir_all(&shared).unwrap();
            let mut value = json!({"salt":"test","accounts":[]});
            for digit in ["c", "d"] {
                value["accounts"].as_array_mut().unwrap().push(json!({
                    "id":format!("ac_{}",digit.repeat(64)),"backend":"codex","billing":"unverified","state":"unverified",
                    "launch":{"kind":"cli_profile","executable":"/bin/true","directory":shared},"scope":"owner",
                    "quotaIdentity":null,"verifiedAt":null,"subscriptionProof":null,"models":[],"managed":null
                }));
            }
            rewrite_accounts(&mut value, &temp).unwrap();
            for entry in value["accounts"].as_array().unwrap() {
                assert_eq!(
                    entry["launch"]["directory"],
                    json!(roots::accounts().join(entry["id"].as_str().unwrap()).join("legacy/.codex"))
                );
            }
            let once = value.clone();
            rewrite_accounts(&mut value, &temp).unwrap();
            assert_eq!(value, once);
            std::fs::remove_dir_all(temp).unwrap();
        }

        #[test]
        fn migra_prefixo_exato_e_prefere_anexo_a_workspace() {
            let items = vec![
                CopyItem {
                    source: "/antigo/workspace".into(),
                    destination: "/novo/workspace".into(),
                    kind: "workspace",
                },
                CopyItem {
                    source: "/antigo/workspace/uploads".into(),
                    destination: "/novo/uploads".into(),
                    kind: "uploads",
                },
            ];
            let mut value = json!({"cwd":"/antigo/workspace","profile":"/antigo/workspace/uploads/id/a.txt","outro":"/antigo/workspace-falso/x","texto":"não alterar /antigo/workspace"});
            rewritten(&mut value, &items);
            assert_eq!(
                value,
                json!({"cwd":"/novo/workspace","profile":"/novo/uploads/id/a.txt","outro":"/antigo/workspace-falso/x","texto":"não alterar /antigo/workspace"})
            );
            let before = value.clone();
            rewritten(&mut value, &items);
            assert_eq!(value, before);
        }
    }
}
