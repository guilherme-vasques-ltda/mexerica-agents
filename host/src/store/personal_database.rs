//! Handoff local da DB de preflight. Nunca recebe descritor/path de request.
use super::personal_bootstrap::{Evidence, evidence};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags};
use std::fs::{File, Metadata};
use std::path::{Path, PathBuf};

/// Deve rodar na Connection real de toda Store antes de migrations/escritas.
pub(super) fn configure(connection: &Connection) -> Result<()> {
    // MX-280: abertura convive com escritores de fundo do próprio Hub (sonho,
    // nomeação, varredura) — sem a janela de espera o primeiro DDL/escrita da
    // Store nova falhava com "database is locked" em vez de aguardar o turno.
    connection.busy_timeout(std::time::Duration::from_secs(1))?;
    connection.pragma_update(None, "recursive_triggers", true)?;
    ensure!(
        connection.pragma_query_value(None, "recursive_triggers", |r| r.get::<_, i64>(0))? == 1,
        "recursive ownership triggers unavailable"
    );
    Ok(())
}

#[derive(PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}
fn file_identity(metadata: &Metadata) -> Result<FileIdentity> {
    ensure!(metadata.is_file() && !metadata.file_type().is_symlink(), "personal database must be regular");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(FileIdentity { device: metadata.dev(), inode: metadata.ino() })
    }
    #[cfg(not(unix))]
    {
        anyhow::bail!("personal database handoff requires Unix")
    }
}

/// Consulta o VFS/handle efetivamente aberto; não presume que stat(path) descreva a Connection.
fn unchanged_handle(connection: &Connection) -> Result<()> {
    let mut moved: std::ffi::c_int = 0;
    // SAFETY: Connection emprestada e viva, chamada síncrona; SQLite escreve somente
    // um int em moved. Nenhuma instrução/statement usa o handle simultaneamente.
    let result = unsafe {
        rusqlite::ffi::sqlite3_file_control(
            connection.handle(),
            c"main".as_ptr(),
            rusqlite::ffi::SQLITE_FCNTL_HAS_MOVED,
            (&raw mut moved).cast(),
        )
    };
    ensure!(result == rusqlite::ffi::SQLITE_OK && moved == 0, "personal database handle moved or unverifiable");
    Ok(())
}

/// Identidade do arquivo do banco de uma Connection já autorizada, capturada
/// sob a transação do dono (MX-260): permite que uma consulta pesada rode numa
/// reabertura `READ_ONLY` fora da transação de escrita sem abrir TOCTOU — se o
/// arquivo nesse caminho for trocado entre a validação e a leitura, a conferência
/// falha fechada.
pub(crate) struct IdentidadeBanco {
    path: PathBuf,
    identidade: FileIdentity,
}
impl IdentidadeBanco {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
    /// Confere dev+inode do caminho e o handle efetivamente aberto.
    pub(crate) fn conferir(&self, connection: &Connection) -> Result<()> {
        ensure!(
            file_identity(&std::fs::symlink_metadata(&self.path)?)? == self.identidade,
            "authorized database file changed"
        );
        unchanged_handle(connection)
    }
}
/// `None` para banco sem arquivo real (`:memory:`, temp) — o chamador fica na
/// conexão autorizada, como antes. O handle autorizado é conferido NO MOMENTO
/// da captura (`SQLITE_FCNTL_HAS_MOVED`): se o arquivo do caminho foi trocado
/// depois de aberto, o inode do caminho não é o do handle → falha fechada, sem
/// capturar a identidade de outro banco.
pub(crate) fn identidade_da(connection: &Connection) -> Result<Option<IdentidadeBanco>> {
    identidade_da_impl(connection, || Ok(()))
}
/// A troca pode cair ENTRE a checagem do handle e o `stat` do caminho: sem a
/// revalidação final, a captura registraria o inode do substituto e a reabertura
/// o aceitaria. Confere handle → stat → handle de novo; qualquer troca dentro
/// da janela falha fechada. `entre` só existe para o teste injetar a troca.
fn identidade_da_impl(connection: &Connection, entre: impl FnOnce() -> Result<()>) -> Result<Option<IdentidadeBanco>> {
    let path = PathBuf::from(connection.path().unwrap_or_default());
    if path.as_os_str().is_empty() || path == Path::new(":memory:") {
        return Ok(None);
    }
    unchanged_handle(connection)?;
    let identidade = file_identity(&std::fs::symlink_metadata(&path)?)?;
    entre()?;
    unchanged_handle(connection)?;
    Ok(Some(IdentidadeBanco { path, identidade }))
}
#[cfg(test)]
pub(crate) fn identidade_da_injetando(
    connection: &Connection,
    entre: impl FnOnce() -> Result<()>,
) -> Result<Option<IdentidadeBanco>> {
    identidade_da_impl(connection, entre)
}

/// Campos privados; só capture local constrói. Não é configuração nem prova vinda do cliente.
pub(crate) struct CapturedDatabase {
    path: PathBuf,
    file: File,
    identity: FileIdentity,
    connection: Connection,
    expected: Evidence,
}
impl CapturedDatabase {
    pub(crate) fn evidence(&self) -> &Evidence {
        &self.expected
    }
    fn check_file(&self) -> Result<()> {
        unchanged_handle(&self.connection)?;
        ensure!(
            file_identity(&self.file.metadata()?)? == self.identity
                && file_identity(&std::fs::symlink_metadata(&self.path)?)? == self.identity,
            "personal database file changed"
        );
        unchanged_handle(&self.connection)
    }
    /// Consome a MESMA Connection da leitura. Falha antes de liberar escrita/migração/Store.
    pub(super) fn into_verified(self) -> Result<(PathBuf, Connection)> {
        self.check_file()?;
        ensure!(evidence(&self.connection)? == self.expected, "personal database ledger changed after preflight");
        self.check_file()?;
        self.connection.pragma_update(None, "query_only", false)?;
        Ok((self.path, self.connection))
    }
}

/// SEG-A-R1-01: banco D430 validado POSITIVAMENTE na MESMA Connection capturada, ainda `query_only`.
/// Campos privados; só `CapturedDatabase::validar_d430` constrói e só `Store::open_d430` consome.
pub(crate) struct ValidadaD430 {
    database: CapturedDatabase,
}
impl ValidadaD430 {
    pub(super) fn into_verified(self) -> Result<(PathBuf, Connection)> {
        self.database.into_verified()
    }
}
impl CapturedDatabase {
    /// Valida antes de qualquer Store/migração/escrita; falha deixa o banco intacto (sem reparo nem criação).
    pub(crate) fn validar_d430(self, validar: impl FnOnce(&Connection) -> Result<()>) -> Result<ValidadaD430> {
        let somente_leitura = |c: &Connection| -> Result<()> {
            ensure!(
                c.pragma_query_value(None, "query_only", |r| r.get::<_, i64>(0))? == 1,
                "D430 validation must stay query-only"
            );
            Ok(())
        };
        self.check_file()?;
        somente_leitura(&self.connection)?;
        {
            let tx = self.connection.unchecked_transaction()?;
            validar(&tx)?;
            tx.commit()?;
        }
        somente_leitura(&self.connection)?;
        self.check_file()?;
        Ok(ValidadaD430 { database: self })
    }
}

/// Capacidade de nascimento: só `create_exclusive` constrói, sem bool/JSON de chamador.
pub(crate) struct CreatedDatabase {
    database: CapturedDatabase,
}
impl CreatedDatabase {
    pub(super) fn into_verified(self) -> Result<(PathBuf, Connection)> {
        self.database.check_file()?;
        ensure_empty_created(&self.database)?;
        self.database.into_verified()
    }
}
fn ensure_empty_created(database: &CapturedDatabase) -> Result<()> {
    ensure!(
        database.file.metadata()?.len() == 0
            && database.connection.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0))? == 0,
        "exclusive database changed before initialization"
    );
    Ok(())
}

/// Reserva o nome exclusivamente e conserva seu FD até validar a Connection real.
/// `AlreadyExists` é erro, nunca uma autorização para abrir a DB que apareceu.
pub(crate) fn create_exclusive(path: &Path) -> Result<CreatedDatabase> {
    #[cfg(unix)]
    let file = {
        let dir = crate::fork::private_fs::Directory::open(path.parent().context("database parent")?, false)?;
        dir.file(path.file_name().context("database name")?, libc::O_RDWR | libc::O_CREAT | libc::O_EXCL)?
    };
    #[cfg(not(unix))]
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
        .context("creating canonical database exclusively")?;
    let identity = file_identity(&file.metadata()?)?;
    #[cfg(test)]
    tests::after_create(path)?;
    ensure!(
        file_identity(&std::fs::symlink_metadata(path)?)? == identity && file.metadata()?.len() == 0,
        "exclusive database replaced before opening"
    );
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    connection.pragma_update(None, "query_only", true)?;
    ensure!(
        connection.pragma_query_value(None, "query_only", |r| r.get::<_, i64>(0))? == 1,
        "exclusive database must be query-only until verified"
    );
    let database =
        CapturedDatabase { path: path.to_path_buf(), file, identity, connection, expected: Evidence::default() };
    database.check_file()?;
    ensure_empty_created(&database)?;
    ensure!(evidence(&database.connection)? == database.expected, "exclusive database is not empty");
    Ok(CreatedDatabase { database })
}

/// Abertura existente RW SEM CREATE; até a qualificação só executa leituras.
/// Conservar capacidade RW no handle evita reabrir uma DB read-only por pathname.
pub(crate) fn capture(path: &Path) -> Result<Option<CapturedDatabase>> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let identity = file_identity(&file.metadata()?)?;
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .context("opening existing personal preflight database")?;
    connection.pragma_update(None, "query_only", true)?;
    ensure!(
        connection.pragma_query_value(None, "query_only", |r| r.get::<_, i64>(0))? == 1,
        "personal preflight must be query-only"
    );
    unchanged_handle(&connection)?;
    ensure!(file_identity(&std::fs::symlink_metadata(path)?)? == identity, "personal database changed while opening");
    let expected = evidence(&connection)?;
    let captured = CapturedDatabase { path: path.to_path_buf(), file, identity, connection, expected };
    captured.check_file()?;
    Ok(Some(captured))
}

/// Ponto de observação `cfg(test)` NO inicializador real (`Store::from_connection`), logo antes da 1ª DDL.
#[cfg(test)]
pub(crate) mod observador {
    use rusqlite::Connection;
    type Observar = Box<dyn FnOnce(&Connection)>;
    std::thread_local! {
        static ANTES: std::cell::RefCell<Option<Observar>> = const { std::cell::RefCell::new(None) };
    }
    pub(crate) fn observar(f: impl FnOnce(&Connection) + 'static) {
        ANTES.with(|slot| *slot.borrow_mut() = Some(Box::new(f)));
    }
    pub(crate) fn antes_da_primeira_ddl(c: &Connection) {
        if let Some(f) = ANTES.with(|slot| slot.borrow_mut().take()) {
            f(c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LockExt;
    type Hook = Box<dyn FnOnce(&Path) -> Result<()>>;
    std::thread_local! { static AFTER_CREATE: std::cell::RefCell<Option<Hook>> = const { std::cell::RefCell::new(None) }; }
    pub(super) fn after_create(path: &Path) -> Result<()> {
        let hook = AFTER_CREATE.with(|slot| slot.borrow_mut().take());
        if let Some(hook) = hook {
            hook(path)?;
        }
        Ok(())
    }

    fn personal_source(dir: &Path) -> Result<(PathBuf, Evidence)> {
        use crate::{
            remote::crypto,
            store::{Device, DeviceSource, Scope, Store},
        };
        let path = dir.join("personal.db");
        let store = Store::open(&path)?;
        let (cfg, _) = crate::fork::personal_bootstrap::provisioning::fixture()?;
        store.personal_install_generation(&cfg)?;
        let key = crypto::b64(&[213; 32]);
        store.put_device(&Device {
            key: key.clone(),
            name: "fixture".into(),
            platform: "fixture".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![Scope::Control],
            lease_until: None,
            created_at: 1,
            last_seen_at: None,
        })?;
        store.remove_device(&key)?;
        store
            .db
            .locked()
            .execute("UPDATE personal_generation SET ever_claimed=1,tombstone=1,revision=revision+1", [])?;
        store.kv_set("schema", "5")?;
        drop(store);
        let expected = super::super::personal_bootstrap::preflight(&path)?;
        Ok((path, expected))
    }
    fn replacement_case(before_connection: bool) -> Result<()> {
        use crate::store::Store;
        let dir = std::env::temp_dir().join(format!("exclusive-database-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir)?;
        let (source, expected) = personal_source(&dir)?;
        let original = std::fs::read(&source)?;
        let target = dir.join("new.db");
        let empty = dir.join("exclusive-empty.db");
        if before_connection {
            let saved_empty = empty.clone();
            AFTER_CREATE.with(|slot| {
                assert!(
                    slot.borrow_mut()
                        .replace(Box::new(move |path| {
                            std::fs::rename(path, &saved_empty)?;
                            std::fs::rename(&source, path)?;
                            Ok(())
                        }))
                        .is_none()
                );
            });
            assert!(create_exclusive(&target).and_then(Store::open_created).is_err());
        } else {
            let created = create_exclusive(&target)?;
            std::fs::rename(&target, &empty)?;
            std::fs::rename(&source, &target)?;
            assert!(Store::open_created(created).is_err());
        }
        assert_eq!(std::fs::read(&target)?, original);
        assert_eq!(std::fs::metadata(&empty)?.len(), 0); // Falha não apaga nenhum dos arquivos.
        assert!(super::super::personal_bootstrap::preflight(&target)? == expected);
        let c = Connection::open_with_flags(&target, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        assert_eq!(c.query_row("SELECT v FROM kv WHERE k='schema'", [], |r| r.get::<_, String>(0))?, "5");
        assert_eq!(
            c.query_row("SELECT count(*) FROM kv WHERE k IN ('host_id','token')", [], |r| r.get::<_, u32>(0))?,
            0
        );
        drop(c);
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }
    #[test]
    fn exclusive_creation_refuses_replacement_after_create_before_connection() -> Result<()> {
        replacement_case(true)
    }
    #[test]
    fn exclusive_creation_refuses_replacement_before_store_handoff() -> Result<()> {
        replacement_case(false)
    }
    #[test]
    fn recursive_triggers_configuration_changes_off_connection_before_writes() -> Result<()> {
        let connection = Connection::open_in_memory()?;
        connection.pragma_update(None, "recursive_triggers", false)?;
        assert_eq!(connection.pragma_query_value(None, "recursive_triggers", |r| r.get::<_, i64>(0))?, 0);
        configure(&connection)?; // Mesma função chamada por Store::from_connection em produção.
        assert_eq!(connection.pragma_query_value(None, "recursive_triggers", |r| r.get::<_, i64>(0))?, 1);
        Ok(())
    }
}
