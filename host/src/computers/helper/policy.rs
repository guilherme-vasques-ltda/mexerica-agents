//! Política local do executor. O limite vale aqui, no computador, e não depende do que o Hub ou o modelo pedem.
//!
//! Uma única operação existe: criar, num local que o dono liberou nele (hoje só a Área de Trabalho), uma
//! planilha NOVA, opcionalmente dentro de uma subpasta que se cria se faltar. Não há apagar, mover, renomear,
//! sobrescrever, ler arquivos do dono nem rodar comandos: o que não está aqui não é executado.
use crate::computers::xlsx::{self, Sheet, Summary};
use crate::remote::crypto;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::io::Write as _;
use std::path::{Path, PathBuf};

pub const LOCATIONS: &[&str] = &["desktop"];

/// Falha com código estável para o Hub e mensagem curta para o dono.
#[derive(Debug)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
}
fn no<T>(code: &'static str, message: impl Into<String>) -> Result<T, Refusal> {
    Err(Refusal { code, message: message.into() })
}

/// Pasta especial do sistema (Desktop do macOS, do Windows, do Linux), nunca montada por nome traduzido.
/// O desvio para teste só existe em build de desenvolvimento.
pub fn location_dir(location: &str) -> Result<PathBuf, Refusal> {
    if location != "desktop" {
        return no("location_unknown", "local desconhecido");
    }
    #[cfg(debug_assertions)]
    if let Some(dir) = std::env::var_os("MEXERICA_EXECUTOR_TEST_DESKTOP") {
        return Ok(PathBuf::from(dir));
    }
    match dirs::desktop_dir() {
        Some(dir) => Ok(dir),
        None => no("location_unavailable", "este sistema não informa a pasta Área de Trabalho"),
    }
}

pub fn home_dir() -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    if let Some(home) = std::env::var_os("MEXERICA_EXECUTOR_TEST_HOME") {
        return Some(PathBuf::from(home));
    }
    dirs::home_dir().and_then(|h| h.canonicalize().ok())
}

/// Alvos críticos do sistema e da conta: nunca viram destino, mesmo que alguém peça ou autorize.
/// Recebe o caminho já canônico. O destino precisa ficar dentro da pasta pessoal e fora das pastas de
/// configuração, credenciais e dados de aplicativos.
pub fn critical(path: &Path, home: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(home) else { return true };
    let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect();
    let Some(first) = parts.first() else { return true };
    first.starts_with('.')
        || matches!(first.as_str(), "library" | "appdata" | "applications" | "system" | "windows")
        || parts.iter().any(|p| matches!(p.as_str(), "appdata" | ".ssh" | ".gnupg"))
}

/// Confere o local liberado agora: precisa existir, ser a mesma pasta que o dono liberou e não ser crítica.
pub fn authorized_root(location: &str, granted: Option<&str>) -> Result<PathBuf, Refusal> {
    let Some(home) = home_dir() else { return no("location_unavailable", "pasta pessoal desconhecida") };
    authorized_root_in(&location_dir(location)?, &home, granted)
}

fn authorized_root_in(dir: &Path, home: &Path, granted: Option<&str>) -> Result<PathBuf, Refusal> {
    let Some(granted) = granted else {
        return no("location_not_authorized", "o dono não liberou este local neste computador");
    };
    let Ok(root) = dir.canonicalize() else { return no("location_unavailable", "a pasta liberada não existe mais") };
    if root.to_string_lossy() != granted {
        return no("location_changed", "a pasta liberada mudou de lugar; libere de novo neste computador");
    }
    if critical(&root, home) {
        return no("critical_target", "destino crítico do sistema ou da conta; recusado");
    }
    Ok(root)
}

/// O pedido já conferido: nada daqui sai do que o Hub assinou e o executor recalculou.
pub struct Plan {
    pub location: String,
    pub folder: Option<String>,
    pub filename: String,
    pub bytes: Vec<u8>,
    pub sha256: String,
    pub content_sha256: String,
    pub sheets: Vec<Summary>,
}

pub fn plan(kind: &str, spec: &Value) -> Result<Plan, Refusal> {
    if kind != "fs.write" || spec["purpose"] != "spreadsheet" {
        return no("unsupported_job", "este executor só cria planilhas novas; nada foi feito");
    }
    if spec["mode"] != "create" || spec["encoding"] != "base64" {
        return no("unsupported_job", "só é permitido criar arquivo novo; nada foi feito");
    }
    let Some(location) = spec["location"].as_str().filter(|l| LOCATIONS.contains(l)) else {
        return no("location_unknown", "local desconhecido");
    };
    let Some(filename) = spec["path"].as_str().filter(|n| xlsx::filename_ok(n)) else {
        return no("invalid_name", "nome de arquivo inválido");
    };
    let folder = match &spec["folder"] {
        Value::Null => None,
        Value::String(s) if xlsx::folder_ok(s) => Some(s.clone()),
        _ => return no("invalid_name", "nome de pasta inválido"),
    };
    let Some(bytes) = spec["content"]
        .as_str()
        .filter(|c| c.len() <= xlsx::MAX_FILE.div_ceil(3) * 4 + 4)
        .and_then(|c| STANDARD.decode(c).ok())
    else {
        return no("bad_content", "conteúdo inválido ou grande demais");
    };
    let sha256 = xlsx::hex(&crypto::sha256(&[&bytes]));
    if bytes.len() > xlsx::MAX_FILE || spec["size"] != json!(bytes.len()) || spec["sha256"] != json!(sha256) {
        return no("bad_content", "tamanho ou SHA-256 não conferem com o pedido");
    }
    let Ok(sheets) = xlsx::inspect(&bytes) else {
        return no("bad_content", "o conteúdo não é uma planilha válida");
    };
    // O cliente não grava o que o Hub mandou: regenera a planilha a partir do conteúdo que ele mesmo validou
    // (limites, nomes, sem fórmulas nem partes extras) e só grava se os bytes forem idênticos.
    if xlsx::validate(&sheets).is_err() || xlsx::build(&sheets).ok().as_deref() != Some(bytes.as_slice()) {
        return no("bad_content", "o arquivo não é a planilha canônica do conteúdo validado");
    }
    let (Ok(content_sha256), summary) = (xlsx::content_sha256(&sheets), xlsx::summary(&sheets)) else {
        return no("bad_content", "conteúdo ilegível");
    };
    if spec["contentSha256"] != json!(content_sha256) || spec["summary"] != json!(summary) {
        return no("bad_content", "o resumo do conteúdo não confere com o arquivo");
    }
    Ok(Plan {
        location: location.into(),
        folder,
        filename: filename.into(),
        bytes,
        sha256,
        content_sha256,
        sheets: summary,
    })
}

pub struct Receipt(pub Value);
pub struct Failure {
    pub code: &'static str,
    pub message: String,
    pub detail: Value,
}
fn fail<T>(code: &'static str, message: impl Into<String>, detail: Value) -> Result<T, Failure> {
    Err(Failure { code, message: message.into(), detail })
}

#[cfg(unix)]
mod dir {
    //! Diretórios abertos por descritor: a raiz liberada é aberta componente a componente sem seguir links e todo o
    //! resto (pasta, arquivo novo, reabertura) é relativo a esses descritores. Trocar uma pasta por um link depois da
    //! conferência não redireciona a gravação para outro lugar.
    use std::ffi::CString;
    use std::fs::File;
    use std::io;
    use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
    use std::path::{Component, Path};

    pub struct Dir(OwnedFd);

    const DIR: libc::c_int = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;

    fn name(text: &str) -> io::Result<CString> {
        CString::new(text).map_err(|_| io::Error::other("nome inválido"))
    }
    fn owned(fd: libc::c_int) -> io::Result<OwnedFd> {
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fd acabou de ser devolvido por open/openat com sucesso e ninguém mais o possui.
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }

    impl Dir {
        pub fn open_root(path: &Path) -> io::Result<Self> {
            // SAFETY: literal C terminado em NUL; a chamada não guarda o ponteiro.
            let mut dir =
                owned(unsafe { libc::open(c"/".as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) })?;
            for part in path.components() {
                match part {
                    Component::RootDir => {}
                    Component::Normal(n) => dir = Self(dir).open_dir(&n.to_string_lossy())?.0,
                    _ => return Err(io::Error::other("caminho não canônico")),
                }
            }
            Ok(Self(dir))
        }
        pub fn open_dir(&self, child: &str) -> io::Result<Self> {
            let c = name(child)?;
            // SAFETY: o descritor pertence a self e o nome é uma string C válida durante a chamada.
            owned(unsafe { libc::openat(self.0.as_raw_fd(), c.as_ptr(), DIR) }).map(Self)
        }
        /// true se criou; false se já existia (o que for, é conferido na abertura seguinte).
        pub fn mkdir(&self, child: &str) -> io::Result<bool> {
            let c = name(child)?;
            // SAFETY: igual a open_dir.
            if unsafe { libc::mkdirat(self.0.as_raw_fd(), c.as_ptr(), 0o755) } == 0 {
                return Ok(true);
            }
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::AlreadyExists { Ok(false) } else { Err(e) }
        }
        pub fn create_new(&self, child: &str) -> io::Result<File> {
            let c = name(child)?;
            let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
            // SAFETY: igual a open_dir; o modo é passado como o argumento variádico inteiro que openat espera.
            owned(unsafe { libc::openat(self.0.as_raw_fd(), c.as_ptr(), flags, 0o644 as libc::c_uint) }).map(File::from)
        }
        /// Identidade (dispositivo e inode) do diretório que este descritor realmente aponta.
        pub fn identity(&self) -> io::Result<(u64, u64)> {
            let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
            // SAFETY: o descritor pertence a self; `st` é um buffer válido para a chamada escrever.
            if unsafe { libc::fstat(self.0.as_raw_fd(), st.as_mut_ptr()) } != 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: fstat devolveu sucesso, então o buffer foi preenchido.
            let st = unsafe { st.assume_init() };
            // No macOS `st_dev` é i32: a conversão é necessária lá, mesmo que no Linux seja a mesma.
            #[allow(clippy::unnecessary_cast)]
            Ok((st.st_dev as u64, st.st_ino as u64))
        }
        /// Identidade do nome, sem abrir nem seguir link.
        pub fn entry_identity(&self, child: &str) -> io::Result<(u64, u64)> {
            let c = name(child)?;
            let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
            // SAFETY: o descritor pertence a self, o nome é uma string C válida e `st` é buffer da chamada.
            if unsafe { libc::fstatat(self.0.as_raw_fd(), c.as_ptr(), st.as_mut_ptr(), libc::AT_SYMLINK_NOFOLLOW) } != 0
            {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: fstatat devolveu sucesso, então o buffer foi preenchido.
            let st = unsafe { st.assume_init() };
            #[allow(clippy::unnecessary_cast)]
            Ok((st.st_dev as u64, st.st_ino as u64))
        }
        /// Só metadados, sem abrir nem seguir link: Ok se o nome existe, `NotFound` se não.
        pub fn exists(&self, child: &str) -> io::Result<()> {
            let c = name(child)?;
            let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
            // SAFETY: igual a open_dir; `st` é um buffer válido para a chamada escrever.
            let rc =
                unsafe { libc::fstatat(self.0.as_raw_fd(), c.as_ptr(), st.as_mut_ptr(), libc::AT_SYMLINK_NOFOLLOW) };
            if rc == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
        }
        pub fn open_read(&self, child: &str) -> io::Result<File> {
            let c = name(child)?;
            let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
            // SAFETY: igual a open_dir.
            owned(unsafe { libc::openat(self.0.as_raw_fd(), c.as_ptr(), flags) }).map(File::from)
        }
    }
}

#[cfg(not(unix))]
mod dir {
    //! Sem descritores relativos nesta plataforma o confinamento da gravação não é garantido; por isso o executor
    //! recusa gravar (o Windows não é suportado nesta fatia).
    use std::fs::File;
    use std::io;
    use std::path::Path;

    pub struct Dir;

    fn unsupported<T>() -> io::Result<T> {
        Err(io::Error::other("plataforma sem gravação confinada"))
    }

    impl Dir {
        pub fn open_root(_: &Path) -> io::Result<Self> {
            unsupported()
        }
        pub fn open_dir(&self, _: &str) -> io::Result<Self> {
            unsupported()
        }
        pub fn mkdir(&self, _: &str) -> io::Result<bool> {
            unsupported()
        }
        pub fn create_new(&self, _: &str) -> io::Result<File> {
            unsupported()
        }
        pub fn open_read(&self, _: &str) -> io::Result<File> {
            unsupported()
        }
        pub fn exists(&self, _: &str) -> io::Result<()> {
            unsupported()
        }
        pub fn identity(&self) -> io::Result<(u64, u64)> {
            unsupported()
        }
        pub fn entry_identity(&self, _: &str) -> io::Result<(u64, u64)> {
            unsupported()
        }
    }
}

use dir::Dir;

fn fail_kind(kind: std::io::ErrorKind) -> bool {
    kind == std::io::ErrorKind::NotFound
}

/// (dispositivo, inode): o que liga o arquivo criado à tentativa e ao caminho mostrado no recibo.
pub type FileId = (u64, u64);

#[cfg(unix)]
fn identity(meta: &std::fs::Metadata) -> FileId {
    use std::os::unix::fs::MetadataExt as _;
    (meta.dev(), meta.ino())
}
/// Sem gravação confinada nesta plataforma, nenhum arquivo chega a ser criado: esta identidade nunca vale como prova.
#[cfg(not(unix))]
fn identity(_: &std::fs::Metadata) -> FileId {
    (0, 0)
}

/// Lê o arquivo já aberto (sem seguir links): precisa ser arquivo comum, sem outros nomes, dentro do limite.
fn read_file(mut file: std::fs::File) -> std::io::Result<Vec<u8>> {
    use std::io::Read as _;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > xlsx::MAX_FILE as u64 {
        return Err(std::io::Error::other("não é um arquivo comum dentro do limite"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        if meta.nlink() != 1 {
            return Err(std::io::Error::other("o arquivo tem outros nomes (links)"));
        }
    }
    let mut out = Vec::new();
    (&mut file).take(xlsx::MAX_FILE as u64 + 1).read_to_end(&mut out)?;
    Ok(out)
}

/// Confere o arquivo reaberto do disco: tamanho, SHA-256 e o conteúdo da planilha contra o que o Hub descreveu.
/// "Existe com o mesmo hash" não basta: o conteúdo tem de abrir como planilha e bater com o resumo das abas.
fn verify_file(
    plan: &Plan,
    file: std::io::Result<std::fs::File>,
    path: &Path,
    folder_created: Option<bool>,
    expected: Option<FileId>,
) -> Result<Receipt, Failure> {
    let verify = |why: &str| -> Failure {
        Failure {
            code: "verify_failed",
            message: format!(
                "o arquivo foi gravado mas não passou na conferência ({why}); confira {} à mão",
                path.display()
            ),
            detail: json!({"folderCreated":folder_created,"path":path.display().to_string()}),
        }
    };
    let Ok(file) = file else { return Err(verify("não reabriu")) };
    // O recibo mostra um caminho: ele só vale se ainda apontar para o arquivo conferido (pasta movida no meio não vale).
    let here = file.metadata().ok().map(|m| identity(&m));
    let there = std::fs::symlink_metadata(path).ok().map(|m| identity(&m));
    if here.is_none() || here != there || expected.is_some_and(|e| here != Some(e)) {
        return Err(verify("o caminho não aponta mais para o arquivo gravado"));
    }
    let Ok(back) = read_file(file) else { return Err(verify("não reabriu")) };
    let sha = xlsx::hex(&crypto::sha256(&[&back]));
    if back.len() != plan.bytes.len() || sha != plan.sha256 {
        return Err(verify("SHA-256 diferente"));
    }
    let Ok(sheets): Result<Vec<Sheet>, _> = xlsx::inspect(&back) else {
        return Err(verify("não abriu como planilha"));
    };
    if xlsx::content_sha256(&sheets).ok().as_deref() != Some(plan.content_sha256.as_str())
        || xlsx::summary(&sheets) != plan.sheets
    {
        return Err(verify("conteúdo diferente"));
    }
    // Depois de ler e validar, o nome ainda tem de apontar para o objeto conferido.
    if std::fs::symlink_metadata(path).ok().map(|m| identity(&m)) != here {
        return Err(verify("o caminho mudou durante a conferência"));
    }
    Ok(Receipt(json!({
        "path": path.display().to_string(), "location": plan.location, "folder": plan.folder,
        "folderCreated": folder_created, "created": true, "reopened": true,
        "size": back.len(), "sha256": sha, "contentSha256": plan.content_sha256, "sheets": plan.sheets,
    })))
}

/// Cria a pasta (se faltar), grava o arquivo SEM sobrescrever, reabre do disco e confere. Só chama depois do start do Hub.
/// `on_created` recebe a identidade do arquivo recém-criado ANTES de qualquer byte ser gravado: é a prova durável
/// de autoria que a recuperação pós-queda exige.
pub fn execute(
    plan: &Plan,
    root: &Path,
    root_id: Option<FileId>,
    on_created: &dyn Fn(FileId) -> std::io::Result<()>,
) -> Result<Receipt, Failure> {
    let Ok(root_dir) = Dir::open_root(root) else {
        return fail(
            "location_unavailable",
            "a pasta liberada não pôde ser aberta como pasta comum; nada foi gravado",
            json!({}),
        );
    };
    let changed = || {
        fail::<Receipt>(
            "location_changed",
            "a pasta liberada não é a mesma que você confirmou; nada foi gravado",
            json!({}),
        )
    };
    // Nada é criado se a raiz já não for a que o dono confirmou.
    if let Some(id) = root_id
        && root_dir.identity().ok() != Some(id)
    {
        return changed();
    }
    let (child, shown, folder_created) = match &plan.folder {
        None => (None, root.to_path_buf(), false),
        Some(folder) => {
            let created = match root_dir.mkdir(folder) {
                Ok(created) => created,
                Err(e) => return fail("folder_failed", format!("não foi possível criar a pasta: {e}"), json!({})),
            };
            match root_dir.open_dir(folder) {
                Ok(dir) => (Some(dir), root.join(folder), created),
                Err(_) => {
                    return fail(
                        "folder_not_directory",
                        "já existe algo com esse nome que não é uma pasta comum (arquivo, atalho ou link); nada foi gravado",
                        json!({"folderCreated":created}),
                    );
                }
            }
        }
    };
    let dir = child.as_ref().unwrap_or(&root_dir);
    // O descritor que vai gravar tem de ser a pasta que o dono confirmou, não só o caminho que a nomeia.
    if let Some(id) = root_id
        && root_dir.identity().ok() != Some(id)
    {
        return changed();
    }
    if let Some(folder) = &plan.folder {
        let entry = root_dir.entry_identity(folder).ok();
        if entry.is_none() || dir.identity().ok() != entry {
            return changed();
        }
    }
    let path = shown.join(&plan.filename);
    let mut file = match dir.create_new(&plan.filename) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return fail(
                "file_exists",
                "já existe um arquivo com esse nome; nada foi alterado",
                json!({"folderCreated":folder_created,"path":path.display().to_string()}),
            );
        }
        Err(e) => {
            return fail(
                "write_failed",
                format!("não foi possível criar o arquivo: {e}"),
                json!({"folderCreated":folder_created}),
            );
        }
    };
    let created = file.metadata().ok().map(|m| identity(&m));
    let recorded = created.ok_or_else(|| std::io::Error::other("sem identidade")).and_then(on_created);
    if let Err(e) = recorded.and_then(|()| file.write_all(&plan.bytes)).and_then(|()| file.sync_all()) {
        return fail(
            "write_incomplete",
            format!("a gravação falhou no meio; confira {}: {e}", path.display()),
            json!({"folderCreated":folder_created,"path":path.display().to_string()}),
        );
    }
    // O descritor original fica aberto durante toda a conferência.
    let receipt = verify_file(plan, dir.open_read(&plan.filename), &path, Some(folder_created), created);
    drop(file);
    receipt
}

/// A pasta de agora é a mesma que o dono confirmou (dispositivo e inode)? Uma pasta comum nova no mesmo caminho não herda a liberação.
pub fn same_dir(root: &Path, dev: u64, ino: u64) -> bool {
    dev != 0 && ino != 0 && std::fs::metadata(root).is_ok_and(|m| identity(&m) == (dev, ino))
}

/// Identidade (dispositivo e inode) de uma pasta, para registrar na liberação.
pub fn dir_identity(root: &Path) -> Option<FileId> {
    std::fs::metadata(root).ok().map(|m| identity(&m))
}

/// Depois de uma queda no meio da gravação: olha, sem gravar nada, o que a MESMA tentativa deixou. Só devolve
/// recibo se o arquivo for exatamente o objeto (dispositivo e inode) que a tentativa registrou ao criá-lo e passar na
/// mesma conferência completa de uma gravação normal. Conteúdo igual de outro arquivo não prova nada.
pub fn recover(plan: &Plan, root: &Path, root_id: Option<FileId>, created: FileId) -> Option<Receipt> {
    let root_dir = Dir::open_root(root).ok()?;
    if let Some(id) = root_id
        && root_dir.identity().ok() != Some(id)
    {
        return None;
    }
    let (dir, shown) = match &plan.folder {
        None => (root_dir, root.to_path_buf()),
        Some(folder) => {
            let entry = root_dir.entry_identity(folder).ok()?;
            let dir = root_dir.open_dir(folder).ok()?;
            if dir.identity().ok() != Some(entry) {
                return None;
            }
            (dir, root.join(folder))
        }
    };
    let file = dir.open_read(&plan.filename).ok()?;
    if identity(&file.metadata().ok()?) != created {
        return None;
    }
    verify_file(plan, Ok(file), &shown.join(&plan.filename), None, Some(created)).ok()
}

/// O alvo comprovadamente não existia antes desta tentativa? Qualquer erro além de "não existe" conta como "existia".
pub fn target_absent(plan: &Plan, root: &Path) -> bool {
    let Ok(root_dir) = Dir::open_root(root) else { return false };
    let dir = match &plan.folder {
        None => root_dir,
        Some(folder) => match root_dir.open_dir(folder) {
            Ok(dir) => dir,
            Err(e) => return fail_kind(e.kind()),
        },
    };
    dir.exists(&plan.filename).is_err_and(|e| fail_kind(e.kind()))
}

#[cfg(all(test, unix))]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::computers::xlsx::{Cell, build, build_unvalidated, content_sha256, hex, summary};

    fn tmp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mexerica-executor-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }
    fn sheets() -> Vec<Sheet> {
        vec![Sheet {
            name: "Vendas".into(),
            rows: vec![
                vec![Cell::Text("Produto".into()), Cell::Text("Qtd".into())],
                vec![Cell::Text("Caneta fictícia".into()), Cell::Number(7.0)],
            ],
        }]
    }
    fn spec_with(folder: Option<&str>, filename: &str) -> Value {
        let s = sheets();
        let bytes = build(&s).unwrap();
        json!({"purpose":"spreadsheet","location":"desktop","folder":folder,"path":filename,"mode":"create","size":bytes.len(),
            "sha256":hex(&crypto::sha256(&[&bytes])),"contentSha256":content_sha256(&s).unwrap(),"summary":summary(&s),
            "content":STANDARD.encode(&bytes),"encoding":"base64"})
    }
    fn plan_of(folder: Option<&str>, filename: &str) -> Plan {
        plan("fs.write", &spec_with(folder, filename)).unwrap_or_else(|r| panic!("{r:?}"))
    }

    #[test]
    fn plan_accepts_only_the_exact_job_the_hub_describes() {
        assert!(plan("fs.write", &spec_with(Some("Relatórios X"), "Vendas.xlsx")).is_ok());
        assert_eq!(plan("exec", &spec_with(None, "a.xlsx")).err().unwrap().code, "unsupported_job");
        assert_eq!(plan("fs.read", &spec_with(None, "a.xlsx")).err().unwrap().code, "unsupported_job");
        let mut overwrite = spec_with(None, "a.xlsx");
        overwrite["mode"] = json!("overwrite");
        assert_eq!(plan("fs.write", &overwrite).err().unwrap().code, "unsupported_job");
        let mut generic = spec_with(None, "a.xlsx");
        generic["purpose"] = json!("file");
        assert_eq!(plan("fs.write", &generic).err().unwrap().code, "unsupported_job");
        for bad in ["../a.xlsx", "a/b.xlsx", "a.xlsx.exe", "C:\\x.xlsx", ".x.xlsx", "x.xls"] {
            assert_eq!(plan("fs.write", &spec_with(None, bad)).err().unwrap().code, "invalid_name", "{bad}");
        }
        for bad in ["..", "../x", "a/b", "a\\b", ".oculta", "CON", "nul.txt", "COM1", "x.", " x", "~/y"] {
            assert_eq!(plan("fs.write", &spec_with(Some(bad), "a.xlsx")).err().unwrap().code, "invalid_name", "{bad}");
        }
        let mut place = spec_with(None, "a.xlsx");
        place["location"] = json!("documents");
        assert_eq!(plan("fs.write", &place).err().unwrap().code, "location_unknown");
        for (key, value) in [
            ("sha256", json!("0".repeat(64))),
            ("size", json!(1)),
            ("contentSha256", json!("x")),
            ("summary", json!([])),
        ] {
            let mut tampered = spec_with(None, "a.xlsx");
            tampered[key] = value;
            assert_eq!(plan("fs.write", &tampered).err().unwrap().code, "bad_content", "{key}");
        }
        let junk = b"nao sou planilha";
        let mut not_xlsx = spec_with(None, "a.xlsx");
        not_xlsx["content"] = json!(STANDARD.encode(junk));
        not_xlsx["size"] = json!(junk.len());
        not_xlsx["sha256"] = json!(hex(&crypto::sha256(&[junk])));
        assert_eq!(plan("fs.write", &not_xlsx).err().unwrap().code, "bad_content");
    }

    #[test]
    fn plan_refuses_a_file_with_an_integer_beyond_the_safe_limit() {
        for n in [9_007_199_254_740_992.0, -9_007_199_254_740_992.0, 3.5e18, -3.5e18] {
            let sheets = vec![Sheet { name: "Vendas".into(), rows: vec![vec![Cell::Number(n)]] }];
            let bytes = build_unvalidated(&sheets).unwrap();
            let mut spec = spec_with(None, "Vendas.xlsx");
            spec["size"] = json!(bytes.len());
            spec["sha256"] = json!(hex(&crypto::sha256(&[&bytes])));
            spec["contentSha256"] = json!(content_sha256(&sheets).unwrap());
            spec["summary"] = json!(summary(&sheets));
            spec["content"] = json!(STANDARD.encode(&bytes));
            assert_eq!(plan("fs.write", &spec).err().unwrap().code, "bad_content", "{n}");
        }
    }

    #[test]
    fn creates_the_folder_and_the_file_then_reopens_and_proves_the_content() {
        let root = tmp("ok");
        let receipt = execute(&plan_of(Some("Pasta X"), "Vendas.xlsx"), &root, None, &|_| Ok(())).ok().unwrap().0;
        let path = root.join("Pasta X").join("Vendas.xlsx");
        assert_eq!(receipt["path"], json!(path.display().to_string()));
        assert_eq!(
            (receipt["folderCreated"].clone(), receipt["created"].clone(), receipt["reopened"].clone()),
            (json!(true), json!(true), json!(true))
        );
        let disk = std::fs::read(&path).unwrap();
        assert_eq!(receipt["sha256"], json!(hex(&crypto::sha256(&[&disk]))));
        assert_eq!(content_sha256(&xlsx::inspect(&disk).unwrap()).unwrap(), content_sha256(&sheets()).unwrap());
        // Pasta que já existe é reaproveitada; arquivo novo ao lado.
        let again = execute(&plan_of(Some("Pasta X"), "Outra.xlsx"), &root, None, &|_| Ok(())).ok().unwrap().0;
        assert_eq!(again["folderCreated"], json!(false));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_inspects_the_same_attempt_without_writing_and_only_the_created_file_becomes_a_receipt() {
        use std::cell::Cell;
        use std::os::unix::fs::MetadataExt as _;
        let root = tmp("recover");
        let plan = plan_of(Some("Pasta X"), "Vendas.xlsx");
        let path = root.join("Pasta X").join("Vendas.xlsx");
        assert!(target_absent(&plan, &root));
        assert!(recover(&plan, &root, None, (1, 1)).is_none());
        std::fs::create_dir(root.join("Pasta X")).unwrap();
        assert!(target_absent(&plan, &root));
        std::fs::write(&path, b"trabalho do dono").unwrap();
        assert!(!target_absent(&plan, &root));
        let meta = std::fs::metadata(&path).unwrap();
        assert!(recover(&plan, &root, None, (meta.dev(), meta.ino())).is_none());
        assert_eq!(std::fs::read(&path).unwrap(), b"trabalho do dono", "recuperar nunca grava");
        std::fs::remove_file(&path).unwrap();

        let created = Cell::new(None);
        assert!(
            execute(&plan, &root, None, &|id| {
                created.set(Some(id));
                Ok(())
            })
            .is_ok()
        );
        let id = created.get().expect("a identidade é registrada antes dos bytes");
        let before = std::fs::read(&path).unwrap();
        let receipt = recover(&plan, &root, None, id).expect("o arquivo criado pela tentativa vira recibo");
        assert_eq!((receipt.0["reopened"].clone(), receipt.0["sha256"].clone()), (json!(true), json!(plan.sha256)));
        assert_eq!(std::fs::read(&path).unwrap(), before);

        // Conteúdo idêntico, mas de outro arquivo (outro inode): não prova autoria.
        std::fs::rename(&path, path.with_extension("antigo")).unwrap();
        std::fs::write(&path, &plan.bytes).unwrap();
        assert!(recover(&plan, &root, None, id).is_none(), "arquivo igual de outro objeto não vira recibo");
        // Metade da gravação: o próprio arquivo, mas inválido.
        let meta = std::fs::metadata(&path).unwrap();
        std::fs::write(&path, &plan.bytes[..plan.bytes.len() / 2]).unwrap();
        assert!(
            recover(&plan, &root, None, (meta.dev(), meta.ino())).is_none(),
            "gravação pela metade não vira recibo"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_descriptor_that_writes_must_be_the_folder_the_owner_confirmed() {
        let root = tmp("rootid");
        let plan = plan_of(Some("Pasta X"), "Vendas.xlsx");
        let id = dir_identity(&root).unwrap();
        assert!(same_dir(&root, id.0, id.1) && !same_dir(&root, id.0, id.1 + 1) && !same_dir(&root, 0, 0));
        let wrong = execute(&plan, &root, Some((id.0, id.1 + 1)), &|_| Ok(())).err().unwrap();
        assert_eq!(wrong.code, "location_changed");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0, "nada é criado quando a pasta não é a confirmada");
        assert!(execute(&plan, &root, Some(id), &|_| Ok(())).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn never_overwrites_an_existing_file_even_with_the_same_name() {
        let root = tmp("exists");
        let first = plan_of(None, "Vendas.xlsx");
        assert!(execute(&first, &root, None, &|_| Ok(())).is_ok());
        let again = execute(&first, &root, None, &|_| Ok(())).err().unwrap();
        assert_eq!(again.code, "file_exists");
        assert!(again.detail.get("sameContent").is_none(), "colisão não lê nem compara o arquivo existente");
        std::fs::write(root.join("Dono.xlsx"), b"trabalho do dono").unwrap();
        let other = execute(&plan_of(None, "Dono.xlsx"), &root, None, &|_| Ok(())).err().unwrap();
        assert_eq!(other.code, "file_exists");
        assert_eq!(std::fs::read(root.join("Dono.xlsx")).unwrap(), b"trabalho do dono");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn links_cannot_redirect_the_write_or_the_folder() {
        use std::os::unix::fs::symlink;
        let (root, outside) = (tmp("links"), tmp("outside"));
        std::fs::write(outside.join("vitima.txt"), b"nao toque").unwrap();
        symlink(outside.join("vitima.txt"), root.join("Atalho.xlsx")).unwrap();
        symlink(outside.join("nao-existe"), root.join("Pendente.xlsx")).unwrap();
        for name in ["Atalho.xlsx", "Pendente.xlsx"] {
            assert_eq!(
                execute(&plan_of(None, name), &root, None, &|_| Ok(())).err().unwrap().code,
                "file_exists",
                "{name}"
            );
        }
        assert_eq!(std::fs::read(outside.join("vitima.txt")).unwrap(), b"nao toque");
        assert!(!outside.join("nao-existe").exists());
        symlink(&outside, root.join("Pasta")).unwrap();
        assert_eq!(
            execute(&plan_of(Some("Pasta"), "Vendas.xlsx"), &root, None, &|_| Ok(())).err().unwrap().code,
            "folder_not_directory"
        );
        std::fs::write(root.join("Arquivo"), b"x").unwrap();
        assert_eq!(
            execute(&plan_of(Some("Arquivo"), "Vendas.xlsx"), &root, None, &|_| Ok(())).err().unwrap().code,
            "folder_not_directory"
        );
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn critical_targets_are_refused_and_ordinary_ones_are_not() {
        let home = Path::new("/home/user");
        for bad in [
            "/home/user",
            "/home/user/Library",
            "/home/user/Library/Preferences",
            "/home/user/AppData/Roaming",
            "/home/user/appdata/local/Temp",
            "/home/user/.ssh",
            "/home/user/.config/x",
            "/home/user/OneDrive/AppData",
            "/etc",
            "/",
            "/usr/bin",
            "/home/shared",
            "/System/Library",
            "/Users/user/Desktop",
        ] {
            assert!(critical(Path::new(bad), home), "{bad}");
        }
        for ok in [
            "/home/user/Desktop",
            "/home/user/Desktop/Pasta X",
            "/home/user/OneDrive/Área de Trabalho",
            "/home/user/Documents/Library",
        ] {
            assert!(!critical(Path::new(ok), home), "{ok}");
        }
    }

    #[test]
    fn the_granted_folder_must_be_the_same_one_and_never_critical() {
        let base = tmp("auth");
        let home = base.join("home");
        let desktop = home.join("Desktop");
        std::fs::create_dir_all(&desktop).unwrap();
        let granted = desktop.canonicalize().unwrap().to_string_lossy().into_owned();
        let home = home.canonicalize().unwrap();
        assert_eq!(authorized_root_in(&desktop, &home, None).err().unwrap().code, "location_not_authorized");
        assert!(authorized_root_in(&desktop, &home, Some(&granted)).is_ok());
        assert_eq!(authorized_root_in(&desktop, &home, Some("/outro/lugar")).err().unwrap().code, "location_changed");
        assert_eq!(
            authorized_root_in(&base.join("sumiu"), &home, Some(&granted)).err().unwrap().code,
            "location_unavailable"
        );
        let library = home.join("Library");
        std::fs::create_dir_all(&library).unwrap();
        let library_path = library.canonicalize().unwrap().to_string_lossy().into_owned();
        assert_eq!(authorized_root_in(&library, &home, Some(&library_path)).err().unwrap().code, "critical_target");
        assert_eq!(
            authorized_root_in(&base, &home, Some(&base.to_string_lossy())).err().unwrap().code,
            "critical_target"
        );
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn recovery_only_accepts_the_granted_root_and_the_same_subfolder() {
        let root = tmp("recovergranted");
        let plan = plan_of(Some("Pasta X"), "Vendas.xlsx");
        let root_id = dir_identity(&root).unwrap();
        assert!(execute(&plan, &root, Some(root_id), &|_| Ok(())).is_ok());
        let path = root.join("Pasta X").join("Vendas.xlsx");
        let created = identity(&std::fs::metadata(&path).unwrap());
        assert!(recover(&plan, &root, Some(root_id), created).is_some());
        assert!(recover(&plan, &root, Some((root_id.0, root_id.1 + 1)), created).is_none());
        std::fs::rename(root.join("Pasta X"), root.join("Pasta antiga")).unwrap();
        std::fs::create_dir(root.join("Pasta X")).unwrap();
        std::fs::write(&path, &plan.bytes).unwrap();
        let copied = identity(&std::fs::metadata(&path).unwrap());
        assert_ne!(copied, created);
        assert!(recover(&plan, &root, Some((root_id.0, root_id.1 + 1)), copied).is_none());
        assert!(recover(&plan, &root, Some(root_id), created).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_swapped_subfolder_changes_the_identity_seen_from_the_granted_root() {
        let root = tmp("entryid");
        let dir = Dir::open_root(&root).unwrap();
        std::fs::create_dir(root.join("Pasta X")).unwrap();
        let before = dir.entry_identity("Pasta X").unwrap();
        assert_eq!(before, dir_identity(&root.join("Pasta X")).unwrap());
        std::fs::rename(root.join("Pasta X"), root.join("Pasta antiga")).unwrap();
        std::fs::create_dir(root.join("Pasta X")).unwrap();
        assert_ne!(before, dir.entry_identity("Pasta X").unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }
}
