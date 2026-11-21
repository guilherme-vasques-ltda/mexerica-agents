//! D166/D167: importação passiva de bundle fixado. Sem downloader ou executor.
mod conectores;
#[cfg(test)]
mod conectores_tests;
mod criador_de_skills;
#[cfg(test)]
mod criador_de_skills_tests;
mod entrega;
#[cfg(test)]
mod entrega_tests;
mod installation;
#[cfg(test)]
mod manual_revision_tests;
#[cfg(test)]
mod marketing_tests;
pub(crate) mod preparo;
mod registry;
mod review;
mod storage;
#[cfg(test)]
mod tests;
pub(crate) use criador_de_skills::{install_guide, instructions as extensibility_instructions};
pub(crate) use entrega::{brief, comando, conferir};

use crate::{LockExt, market::Skill, store::Store};
use anyhow::{Context, Result, anyhow, bail, ensure};
use registry::{FileInfo, Origin, Package, hash};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const STATE: &str = "donna.passiveSkills.v1";
const RECEIPT: &str = ".donnabot-receipt.json";
const SEED: &str = "find-skills";
// Todo fluxo é síncrono e limitado a 5 MiB; sem await com lock ou efeito externo em voo.
static MUTATION: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    schema: u8,
    catalog_id: String,
    digest: String,
    origin: Origin,
    adaptation: Option<String>,
    license: String,
    files: Vec<FileInfo>,
    result: String,
    coverage: String,
    limitations: Vec<String>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct State {
    seed_done: bool,
    receipts: BTreeMap<String, Receipt>,
    installed: BTreeMap<String, String>,
    #[serde(default)]
    installation_namespace: Option<String>,
    #[serde(default)]
    installation_sequence: u64,
    #[serde(default)]
    installations: BTreeMap<String, installation::Version>,
    #[serde(skip)]
    observed: Option<storage::Snapshot>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstallIntent {
    catalog_id: String,
    expected_digest: String,
    /// D306: só com a encarnação vista o Hub repõe o texto do recibo de uma curada alterada.
    #[serde(default)]
    expected_installation_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CustomIntent {
    name: String,
    description: String,
    #[serde(default)]
    instructions: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RemoveIntent {
    id: String,
    expected_installation_id: String,
    expected_receipt: installation::ExpectedReceipt,
}

pub(crate) fn methods(method: &str) -> bool {
    matches!(method, "skills" | "marketSkills" | "skillDetails" | "installSkill" | "removeSkill")
}

const PROTOCOL: u8 = 2;

fn protocol_payload(body: &Value) -> Result<Value> {
    let mut payload = body.clone();
    if let Some(version) = body.get("skillProtocol") {
        ensure!(version.as_u64() == Some(u64::from(PROTOCOL)), installation::UNSUPPORTED);
        payload.as_object_mut().ok_or_else(|| anyhow!(installation::UNSUPPORTED))?.remove("skillProtocol");
    }
    Ok(payload)
}

fn details_shape(body: &Value) -> Result<()> {
    if body.get("review").is_some() {
        review::request(body)?;
        return Ok(());
    }
    ensure!(
        body.as_object().is_some_and(|o| o.len() == 1
            && (o.get("id").is_some_and(Value::is_string) || o.get("source").is_some_and(Value::is_string))),
        "Escolha uma habilidade por id ou source"
    );
    Ok(())
}

fn install_intent(body: &Value) -> Result<InstallIntent> {
    ensure!(
        body.get("source").is_none(),
        "Instalação por source foi encerrada; escolha catalogId e expectedDigest do catálogo curado"
    );
    let intent: InstallIntent = serde_json::from_value(body.clone())
        .map_err(|_| anyhow!("Intenção de catálogo inválida; use catalogId e expectedDigest"))?;
    ensure!(intent.expected_installation_id.as_deref().is_none_or(installation::valid_id), installation::UNSUPPORTED);
    Ok(intent)
}
fn custom_intent(body: &Value) -> Result<CustomIntent> {
    serde_json::from_value(body.clone()).map_err(|_| anyhow!("Texto manual inválido"))
}
fn remove_intent(body: &Value) -> Result<RemoveIntent> {
    let intent: RemoveIntent = serde_json::from_value(body.clone()).map_err(|_| anyhow!(installation::UNSUPPORTED))?;
    ensure!(
        installation::valid_id(&intent.expected_installation_id) && intent.expected_receipt.valid(),
        installation::UNSUPPORTED
    );
    Ok(intent)
}

/// Consultas sem corpo; só `skills` aceita `botId` (R3: medida do índice daquele bot).
fn query_shape(method: &str, body: &Value) -> bool {
    let Some(o) = body.as_object() else { return body.is_null() };
    o.is_empty() || (method == "skills" && o.len() == 1 && o.get("botId").is_some_and(Value::is_string))
}

fn request_shape(method: &str, body: &Value) -> Result<()> {
    match method {
        "skills" | "marketSkills" => ensure!(query_shape(method, body), "Intenção de consulta de habilidades inválida"),
        "skillDetails" => details_shape(body)?,
        "installSkill" if body.get("catalogId").is_some() || body.get("source").is_some() => {
            install_intent(body)?;
        }
        "installSkill" => {
            custom_intent(body)?;
        }
        "removeSkill" => {
            remove_intent(body)?;
        }
        _ => bail!("Intenção de habilidades inválida"),
    }
    Ok(())
}

/// Limite único dos RPCs de skills: Caller e seleção antes de dados/projeções/efeitos.
pub(crate) async fn call(
    hub: &std::sync::Arc<crate::hub::Hub>,
    caller: &crate::api::devices::Caller,
    method: &str,
    body: Value,
) -> Result<Value> {
    require_current(hub, caller, method)?;
    let request = super::spaces::PersonalSkillRequest::resolve(hub, method, body)?;
    let payload = protocol_payload(request.payload())?;
    let b = &payload;
    request_shape(method, b)?;
    let revalidate = || {
        require_current(hub, caller, method)?;
        request.revalidate(hub, method)
    };
    match method {
        // R3: com `botId` (já conferido pela guarda do Space), a lista leva a medida do índice desse bot.
        "skills" => match b.get("botId").and_then(Value::as_str) {
            Some(bot) => list_for_bot(&hub.store, bot, revalidate),
            None => crate::market::list_skills(&hub.store, revalidate),
        },
        "marketSkills" => crate::market::browse_skills(&hub.store, revalidate).await,
        "skillDetails" if b.get("review").is_some() => {
            let intent = review::request(b)?;
            let authority = crate::fork::autoridade::Autoridade::capturar(hub, caller)?;
            let out = review::examine(hub, &authority, &intent, revalidate)?;
            revalidate()?;
            Ok(out)
        }
        "skillDetails" => crate::market::skill_details(&hub.store, b, revalidate).await,
        "installSkill" => {
            let _guard = hub.store.team_lock.lock().await;
            revalidate()?;
            if b.get("catalogId").is_some() || b.get("source").is_some() {
                install(&hub.store, b, revalidate)
            } else {
                custom(&hub.store, b, revalidate)
            }
        }
        "removeSkill" => {
            // D368: autoridade ORIGINAL antes da fila; a gravação a reconfere na própria transação.
            let owner = crate::fork::autoridade::Autoridade::capturar(hub, caller)?;
            let _guard = hub.store.team_lock.lock().await;
            revalidate()?;
            let intent = remove_intent(b)?;
            let mut affected = Vec::new();
            let removed = remove_with(&hub.store, Some((hub, &owner)), b, revalidate, || {
                // Só uma condição atual pode provocar recuperação/efeitos de manutenção.
                crate::chat::manage::recover_live(hub);
                affected = hub
                    .store
                    .bots()?
                    .into_iter()
                    .filter(|r| !r.deleted && r.config.skills.iter().any(|s| s == &intent.id))
                    .map(|r| r.config.id)
                    .collect::<Vec<_>>();
                Ok(())
            });
            // Mesmo se a limpeza falhar, o registro/vínculo já pode ter sido removido.
            for id in affected {
                if let Some(row) = hub.store.bot(&id)? {
                    let _ = hub.send_cmd(&id, crate::agent::bot::Cmd::Reconfigure(Box::new(row.config)));
                    hub.emit_bot(&id);
                }
            }
            removed?;
            Ok(json!({"skillProtocol":PROTOCOL}))
        }
        _ => bail!("Intenção de habilidades inválida"),
    }
}

#[cfg(test)]
fn load(store: &Store) -> Result<(Vec<Skill>, State)> {
    storage::inspect(store)
}

fn put(tx: &rusqlite::Connection, key: &str, value: &impl Serialize) -> Result<()> {
    tx.execute(
        "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
        params![key, serde_json::to_string(value)?],
    )?;
    Ok(())
}

fn persist(store: &Store, all: &[Skill], state: &State, removed: Option<&str>) -> Result<()> {
    storage::persist(store, all, state, removed)
}

pub(crate) fn dir(store: &Store, skill: &Skill) -> PathBuf {
    store.skills_root.join(&skill.id)
}

fn installed_receipt<'a>(state: &'a State, id: &str) -> Option<&'a Receipt> {
    state.installed.get(id).and_then(|digest| state.receipts.get(digest))
}

fn public(store: &Store, skill: &Skill, state: &State) -> Result<Value> {
    Ok(serde_json::to_value(installation::public(store, skill, state)?)?)
}

fn with_snapshot<T>(
    store: &Store,
    revalidate: impl FnOnce() -> Result<()>,
    read: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let _guard = MUTATION.locked();
    let _file = root_lock(store)?;
    revalidate()?;
    read()
}

fn catalog_item(p: &Package, all: &[Skill], state: &State) -> Result<Value> {
    let mut item = serde_json::to_value(&p.metadata)?;
    let installed = all.iter().find(|s| {
        installed_receipt(state, &s.id).is_some_and(|r| r.digest == p.metadata.digest && r.catalog_id == p.metadata.id)
    });
    item["installed"] = json!(installed.is_some());
    item["installedId"] = json!(installed.map(|s| &s.id));
    let conflict = all.iter().any(|s| s.id == p.install_id) && installed.is_none();
    item["installable"] = json!(p.installable() && !conflict);
    if conflict {
        item["reason"] = json!(format!(
            "{} A identidade de instalação já pertence a outra versão/legado; nenhum arquivo será substituído. Confira e remova explicitamente o conflito antes de importar.",
            p.metadata.reason
        ));
    }
    Ok(item)
}

/// A skill está no catálogo curado (as fichas do Marketplace só sugerem skills que existem).
#[cfg(test)]
pub(crate) fn curada(id: &str) -> bool {
    registry::find(id).is_ok()
}

pub(crate) fn browse(store: &Store, revalidate: impl FnOnce() -> Result<()>) -> Result<Value> {
    with_snapshot(store, revalidate, || browse_locked(store))
}

fn browse_locked(store: &Store) -> Result<Value> {
    let (all, state) = installation::load_initialized(store)?;
    Ok(
        json!({"skillProtocol":PROTOCOL,"items":registry::packages()?.iter().map(|p| catalog_item(p, &all, &state)).collect::<Result<Vec<_>>>()?}),
    )
}

pub(crate) fn list(store: &Store, revalidate: impl FnOnce() -> Result<()>) -> Result<Value> {
    with_snapshot(store, revalidate, || list_locked(store))
}

/// R3: a lista de sempre e a medida do bloco que o perfil do bot receberia agora (`index`, null se não conferido).
/// A linha lida aqui é a medida: bot vivo do Pessoal, com a versão de seleção c1 dessa mesma leitura.
fn list_for_bot(store: &Store, bot: &str, revalidate: impl FnOnce() -> Result<()>) -> Result<Value> {
    with_snapshot(store, revalidate, || {
        let row = store
            .bot(bot)?
            .filter(|row| !row.deleted && row.config.space_id == super::spaces::PERSONAL)
            .ok_or_else(|| anyhow!("Bot não encontrado"))?;
        let mut out = list_locked(store)?;
        out["index"] = match (entrega::medida(store, &row.config.skills), row.skills_version) {
            (Some(medida), Some(version)) => {
                let mut index = json!(medida);
                index["selectionVersion"] = json!(version);
                index
            }
            _ => Value::Null,
        };
        Ok(out)
    })
}

fn list_locked(store: &Store) -> Result<Value> {
    let (all, state) = installation::load_initialized(store)?;
    // MX-264/C: marca as skills que bots novos já recebem por padrão (só leitura;
    // o padrão em si é gravado por `extensaoNovosBotsDefinir`, só do dono).
    let padroes = crate::fork::extensoes_em_todos::novos_bots_skills(store);
    let mut items = Vec::with_capacity(all.len());
    for s in &all {
        let mut item = public(store, s, &state)?;
        if padroes.contains(&s.id) {
            item["novosBots"] = json!(true);
        }
        items.push(item);
    }
    Ok(json!({"skillProtocol":PROTOCOL,"items":items}))
}

fn bounded_read(path: &Path, max: usize) -> Result<String> {
    ensure!(fs::symlink_metadata(path)?.file_type().is_file(), "Arquivo não regular");
    let mut bytes = Vec::new();
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    ensure!(file.metadata()?.file_type().is_file(), "Arquivo não regular");
    file.take((max + 1) as u64).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= max, "Arquivo excede o limite de leitura");
    Ok(String::from_utf8(bytes)?)
}

pub(crate) fn details(store: &Store, b: &Value, revalidate: impl FnOnce() -> Result<()>) -> Result<Value> {
    details_shape(b)?;
    with_snapshot(store, revalidate, || details_locked(store, b))
}

fn details_locked(store: &Store, b: &Value) -> Result<Value> {
    let (all, state) = installation::load_initialized(store)?;
    if let Some(id) = b["id"].as_str() {
        let skill = all.iter().find(|s| s.id == id).ok_or_else(|| anyhow!("Habilidade não instalada"))?;
        // Corpo, registro e recibo são observados na mesma encarnação sob os locks.
        let md = installation::checked_document(store, skill, &state)?;
        let catalog = if let Some(r) = installed_receipt(&state, id) {
            match registry::captured(&r.catalog_id, &r.digest)? {
                Some(p) if receipt(&p) == *r => {
                    let mut item = catalog_item(&p, &all, &state)?;
                    // Detalhes descrevem o envelope do recibo, nunca a nova embalagem do catálogo.
                    if registry::find(&r.catalog_id).is_ok_and(|current| current.metadata.digest != r.digest) {
                        item["installable"] = json!(false);
                    }
                    Some(item)
                }
                _ => None,
            }
        } else {
            None
        };
        return Ok(serde_json::to_value(installation::DetailsDto {
            skill_protocol: PROTOCOL,
            markdown: crate::fork::agent_skills::legacy_body(&md, id).to_owned(),
            catalog,
            receipt: installed_receipt(&state, id),
            installation: Some(installation::project(store, skill, &state, Some(&md))?),
        })?);
    }
    let p = registry::find(b["source"].as_str().ok_or_else(|| anyhow!("id ou source obrigatório"))?)?;
    let markdown = p
        .assets
        .iter()
        .find(|a| a.path == "SKILL.md")
        .map_or("", |a| crate::fork::agent_skills::legacy_body(&a.content, &p.install_id));
    Ok(serde_json::to_value(installation::DetailsDto {
        skill_protocol: PROTOCOL,
        markdown: markdown.to_owned(),
        catalog: Some(catalog_item(&p, &all, &state)?),
        receipt: None,
        installation: None,
    })?)
}

fn private_dir(path: &Path) -> Result<()> {
    if !path.exists() {
        let mut b = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            b.mode(0o700);
        }
        b.create(path)?;
    }
    ensure!(fs::symlink_metadata(path)?.file_type().is_dir(), "Pasta de habilidades não regular");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn root_lock(store: &Store) -> Result<File> {
    if let Some(parent) = store.skills_root.parent() {
        fs::create_dir_all(parent)?;
    }
    private_dir(&store.skills_root)?;
    let path = store.skills_root.join(".catalog.lock");
    if let Ok(meta) = fs::symlink_metadata(&path) {
        ensure!(meta.file_type().is_file(), "Lock de habilidades inválido");
    }
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let lock = options.open(path)?;
    acquire(&lock, LOCK_WAIT)?;
    #[cfg(windows)]
    clean_orphans(&store.skills_root, move_no_replace);
    Ok(lock)
}

// Cópia herdada por filho em exec: espera curta comum (`crate::lock_file`, #281).
const LOCK_WAIT: std::time::Duration = crate::FLOCK_WAIT;

fn acquire(lock: &File, wait: std::time::Duration) -> Result<()> {
    ensure!(
        crate::lock_file(lock, wait).context("Falha ao reservar as habilidades")?,
        "Outra alteração de habilidades está em andamento"
    );
    Ok(())
}

struct Stage(PathBuf);
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut opts = OpenOptions::new();
    opts.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut f = opts.open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

fn receipt(p: &Package) -> Receipt {
    Receipt {
        schema: 1,
        catalog_id: p.metadata.id.clone(),
        digest: p.metadata.digest.clone(),
        origin: p.metadata.origin.clone(),
        adaptation: p.metadata.adaptation.clone(),
        license: p.metadata.license.clone(),
        files: p.metadata.files.clone(),
        result: "imported".into(),
        coverage: "pinnedPassiveTextOnly".into(),
        limitations: vec![
            "Integridade e proveniência do texto; sem scanner completo, aprovação Guard/ISO ou runtime.".into(),
        ],
    }
}

fn check_tree(root: &Path, expected: &BTreeSet<String>, prefix: &str) -> Result<()> {
    ensure!(fs::symlink_metadata(root)?.file_type().is_dir(), "Pasta de importação inválida");
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().into_string().map_err(|_| anyhow!("Path inválido"))?;
        let path = format!("{prefix}{name}");
        let kind = fs::symlink_metadata(entry.path())?.file_type();
        if kind.is_dir() {
            ensure!(expected.iter().any(|p| p.starts_with(&format!("{path}/"))), "Diretório extra na importação");
            check_tree(&entry.path(), expected, &format!("{path}/"))?;
        } else {
            ensure!(kind.is_file() && expected.contains(&path), "Arquivo extra/link na importação");
        }
    }
    Ok(())
}

fn verify_published(dest: &Path, files: &[registry::Asset], r: Option<&Receipt>) -> Result<()> {
    let mut expected: BTreeSet<_> = files.iter().map(|f| f.path.clone()).collect();
    if r.is_some() {
        expected.insert(RECEIPT.into());
    }
    check_tree(dest, &expected, "")?;
    for a in files {
        let content = bounded_read(&dest.join(&a.path), a.bytes)?;
        ensure!(
            content.len() == a.bytes && hash(content.as_bytes()) == a.sha256,
            "Importação existente tem conteúdo diferente"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            ensure!(fs::metadata(dest.join(&a.path))?.permissions().mode() & 0o111 == 0, "Arquivo executável recusado");
        }
    }
    if let Some(r) = r {
        let actual: Receipt = serde_json::from_str(&bounded_read(&dest.join(RECEIPT), 256 * 1024)?)?;
        ensure!(actual == *r, "Recibo existente difere da versão curada");
    }
    Ok(())
}

fn sync_directories(path: &Path) -> Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if fs::symlink_metadata(entry.path())?.file_type().is_dir() {
            sync_directories(&entry.path())?;
        }
    }
    sync_directory(path)
}

/// O Windows não abre pasta como arquivo (`File::open` nega acesso); lá a gravação já é por arquivo.
fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn rename_new(source: &Path, dest: &Path) -> Result<()> {
    rename_flagged(source, dest, false)
}

/// Troca atômica de duas pastas existentes (Linux `RENAME_EXCHANGE`, macOS `RENAME_SWAP`).
fn rename_swap(source: &Path, dest: &Path) -> Result<()> {
    rename_flagged(source, dest, true)
}

fn rename_flagged(source: &Path, dest: &Path, swap: bool) -> Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::ffi::OsStrExt;
        let source = std::ffi::CString::new(source.as_os_str().as_bytes())?;
        let dest = std::ffi::CString::new(dest.as_os_str().as_bytes())?;
        // Ponteiros de CStrings válidos durante a chamada. Flags impedem substituição; a troca só vale
        // entre duas pastas existentes e mantém as duas (a antiga vai para o stage).
        #[cfg(target_os = "linux")]
        let flags = if swap { libc::RENAME_EXCHANGE } else { libc::RENAME_NOREPLACE };
        #[cfg(target_os = "linux")]
        // SAFETY: CStrings mantêm ponteiros válidos durante a chamada; não há ownership transferido.
        let result = unsafe { libc::renameat2(libc::AT_FDCWD, source.as_ptr(), libc::AT_FDCWD, dest.as_ptr(), flags) };
        #[cfg(target_os = "macos")]
        let flags = if swap { libc::RENAME_SWAP } else { libc::RENAME_EXCL };
        #[cfg(target_os = "macos")]
        // SAFETY: CStrings mantêm ponteiros válidos durante a chamada; não há ownership transferido.
        let result = unsafe { libc::renamex_np(source.as_ptr(), dest.as_ptr(), flags) };
        if result != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        // Sem troca atômica no Windows: a troca sai em passos (ver `swap_in_steps`) e todo passo recusa
        // destino existente, então um destino criado no intervalo nunca é substituído.
        if !swap {
            return Ok(move_no_replace(source, dest)?);
        }
        swap_in_steps(source, dest, move_no_replace)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (source, dest, swap);
        bail!("Publicação atômica sem substituição indisponível nesta plataforma");
    }
}

/// `MoveFileExW` sem `MOVEFILE_REPLACE_EXISTING`: o próprio sistema recusa quando o destino existe
/// (arquivo, pasta, vazia ou não, ou junção). O `fs::rename` do Rust substitui o destino.
#[cfg(windows)]
fn move_no_replace(source: &Path, dest: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    // Forma estendida (`\\?\`): sem ela o `MoveFileExW` falha acima de 260 unidades, mesmo quando o
    // staging (que passa pelo `std`) deu certo.
    let wide = |path: &Path| -> std::io::Result<Vec<u16>> {
        let units: Vec<u16> = std::path::absolute(path)?.as_os_str().encode_wide().collect();
        let (verbatim, unc): (Vec<u16>, Vec<u16>) =
            (r"\\?\".encode_utf16().collect(), r"\\?\UNC\".encode_utf16().collect());
        let mut full = if units.starts_with(&verbatim) {
            units
        } else if let Some(rest) = units.strip_prefix(&[0x5c_u16, 0x5c_u16][..]) {
            [unc, rest.to_vec()].concat()
        } else {
            [verbatim, units].concat()
        };
        full.push(0);
        Ok(full)
    };
    let (source, dest) = (wide(source)?, wide(dest)?);
    if source[..source.len() - 1].contains(&0) || dest[..dest.len() - 1].contains(&0) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "caminho com NUL"));
    }
    // SAFETY: os dois buffers terminam em NUL, não têm NUL no meio e ficam vivos durante a chamada; flags 0
    // não substitui destino existente.
    if unsafe { MoveFileExW(source.as_ptr(), dest.as_ptr(), 0) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Troca duas pastas sem rename atômico: `dest` sai para `.old-*`, `source` entra em `dest` e a antiga
/// volta para `source` (o stage a apaga). Se a entrada falha, a anterior volta; se o desfazer também
/// falha, os dois erros chegam ao chamador. O lock de filesystem serializa os escritores do Hub.
#[cfg(any(windows, test))]
fn swap_in_steps(source: &Path, dest: &Path, mv: impl Fn(&Path, &Path) -> std::io::Result<()>) -> Result<()> {
    // O nome guarda o destino (".old-<uuid>-<destino>"): se o desfazer falhar, `clean_orphans` sabe para onde
    // devolver a única cópia da versão anterior.
    let name = dest.file_name().unwrap_or_default().to_string_lossy();
    let old = dest.with_file_name(format!(".old-{}-{name}", uuid::Uuid::new_v4()));
    mv(dest, &old).context("Não foi possível tirar a versão anterior do caminho")?;
    if let Err(error) = mv(source, dest) {
        return match mv(&old, dest) {
            Ok(()) => Err(anyhow!(error).context("Troca de pastas recusada; versão anterior mantida")),
            Err(undo) => Err(anyhow!(
                "Troca de pastas interrompida ({error}) e o desfazer falhou ({undo}); a versão anterior ficou em {}",
                old.file_name().unwrap_or_default().to_string_lossy()
            )),
        };
    }
    // `dest` já tem a versão nova; levar a antiga para o stage é só limpeza, em melhor esforço.
    if let Err(error) = mv(&old, source) {
        tracing::warn!(%error, "Não foi possível mover a versão anterior para o stage; apagando");
        if let Err(error) = fs::remove_dir_all(&old) {
            tracing::warn!(%error, "Pasta .old-* órfã não foi apagada; a próxima operação a remove");
        }
    }
    Ok(())
}

/// Destino guardado em `.old-<uuid>-<destino>`; só devolve se o nome inteiro casar: prefixo, UUID de
/// verdade, hífen separador e destino que passa na validação de nome de skill. O resto é ignorado.
#[cfg(any(windows, test))]
fn old_destination(name: &str) -> Option<&str> {
    let rest = name.strip_prefix(".old-")?;
    let (id, dest) = (rest.get(..36)?, rest.get(37..)?);
    (rest.as_bytes().get(36) == Some(&b'-')
        && uuid::Uuid::parse_str(id).is_ok_and(|parsed| parsed.hyphenated().to_string() == id)
        && crate::fork::agent_skills::validate_name(dest, dest).is_ok())
    .then_some(dest)
}

/// Limpa o que uma queda deixou, sob o lock do catálogo (nenhum escritor está em passo de troca ou de
/// preparo): `.stage-*` é apagado; `.old-*` volta para o destino que o nome guarda quando esse destino
/// não existe (era a única cópia) e só é apagado quando o destino já existe.
#[cfg(any(windows, test))]
fn clean_orphans(root: &Path, mv: impl Fn(&Path, &Path) -> std::io::Result<()>) {
    let Ok(entries) = fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let old = name.starts_with(".old-");
        if !(old || name.starts_with(".stage-")) {
            continue;
        }
        let path = entry.path();
        if !fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_dir()) {
            continue;
        }
        if old {
            let Some(dest) = old_destination(&name) else { continue };
            let dest = root.join(dest);
            // Só apaga depois de ver o destino de fato: sem destino a cópia volta; erro de leitura a mantém.
            match fs::symlink_metadata(&dest) {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    if let Err(error) = mv(&path, &dest) {
                        tracing::warn!(%error, "Versão anterior do catálogo não pôde ser restaurada; mantida em .old-*");
                    } else {
                        tracing::warn!("Versão anterior do catálogo de habilidades restaurada após troca interrompida");
                    }
                    continue;
                }
                Err(error) => {
                    tracing::warn!(%error, "Destino do catálogo ilegível; versão anterior mantida em .old-*");
                    continue;
                }
            }
        }
        if let Err(error) = fs::remove_dir_all(&path) {
            tracing::warn!(%error, "Pasta órfã do catálogo de habilidades não foi apagada");
        }
    }
}

fn publish(store: &Store, id: &str, files: &[registry::Asset], r: Option<&Receipt>) -> Result<bool> {
    let dest = store.skills_root.join(id);
    if fs::symlink_metadata(&dest).is_ok() {
        ensure!(r.is_some(), "Destino existente; nenhuma versão será sobrescrita");
        verify_published(&dest, files, r)?;
        return Ok(false); // Recupera só um pacote completo com recibo idêntico após interrupção.
    }
    let stage = staged(store, files, r)?;
    // O lock de filesystem serializa todos os escritores do Hub; nenhum destino é substituído.
    ensure!(fs::symlink_metadata(&dest).is_err(), "Destino criado durante a importação");
    rename_new(&stage.0, &dest)?;
    sync_directory(&store.skills_root)?;
    // Confere de novo o que ficou em `dest`: o stage pode ter sido mexido entre a prova e a publicação.
    if let Err(error) = verify_published(&dest, files, r) {
        let _ = fs::remove_dir_all(&dest);
        return Err(error.context("A importação publicada não conferiu; removida"));
    }
    Ok(true)
}

/// D306: troca a pasta de uma curada alterada pelos bytes do pacote/recibo, num rename atômico.
/// O conteúdo antigo vai para o stage e é apagado; nada dele é lido ou reaproveitado.
fn replace(store: &Store, id: &str, files: &[registry::Asset], r: &Receipt) -> Result<()> {
    let dest = store.skills_root.join(id);
    let stage = staged(store, files, Some(r))?;
    match fs::symlink_metadata(&dest) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => rename_new(&stage.0, &dest)?,
        Ok(meta) if meta.file_type().is_dir() => rename_swap(&stage.0, &dest)?,
        _ => bail!(installation::UNAVAILABLE),
    }
    sync_directory(&store.skills_root)?;
    Ok(())
}

fn staged(store: &Store, files: &[registry::Asset], r: Option<&Receipt>) -> Result<Stage> {
    let stage = Stage(store.skills_root.join(format!(".stage-{}", uuid::Uuid::new_v4())));
    private_dir(&stage.0)?;
    let mut total = 0_usize;
    for a in files {
        total += a.content.len();
        ensure!(total <= 5 * 1024 * 1024, "Limite cumulativo da importação");
        let path = stage.0.join(&a.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        write_new(&path, a.content.as_bytes())?;
    }
    if let Some(r) = r {
        write_new(&stage.0.join(RECEIPT), &serde_json::to_vec(r)?)?;
    }
    verify_published(&stage.0, files, r)?;
    sync_directories(&stage.0)?;
    Ok(stage)
}

fn install_locked(store: &Store, p: &Package, seed: bool, repair: Option<&str>) -> Result<Value> {
    p.validate()?;
    ensure!(p.installable(), "{}", p.metadata.reason);
    let (mut all, mut state) = installation::load_initialized(store)?;
    // D306 anti-ABA: o reparo vale só para a encarnação vista. Ausente, removida ou outra (inclusive
    // íntegra) recusa antes de qualquer publicação; sem o campo, o contrato de instalação não muda.
    if let Some(expected) = repair {
        ensure!(
            all.iter().any(|s| s.id == p.install_id) && installation::matches_id(&state, &p.install_id, expected),
            installation::STALE
        );
    }
    let r = receipt(p);
    if let Some(existing) = all.iter().find(|s| s.id == p.install_id) {
        ensure!(
            installed_receipt(&state, &existing.id) == Some(&r),
            "Identidade já instalada com outra versão; preserve/remova explicitamente a versão existente"
        );
        if let Err(error) = installation::checked_document(store, existing, &state) {
            // D306: só o gesto com a encarnação vista repõe os bytes do pacote curado do recibo.
            let Some(expected) = repair else { return Err(error) };
            ensure!(installation::matches_id(&state, &existing.id, expected), installation::STALE);
            installation::record_intact(existing, &state)?;
            replace(store, &existing.id, &p.assets, &r)?;
            // Conferido depois de gravar: falha não é sucesso, e o perfil segue sem a skill.
            installation::checked_document(store, existing, &state)
                .and_then(|_| verify_published(&dir(store, existing), &p.assets, Some(&r)))
                .map_err(|_| anyhow!("Reinstalação gravada, mas o texto não conferiu; nada foi entregue aos bots"))?;
            return Ok(
                json!({"skillProtocol":PROTOCOL,"skill":public(store,existing,&state)?,"alreadyInstalled":false}),
            );
        }
        verify_published(&dir(store, existing), &p.assets, Some(&r))?;
        if seed && !state.seed_done {
            state.seed_done = true;
            persist(store, &all, &state, None)?;
        }
        return Ok(json!({"skillProtocol":PROTOCOL,"skill":public(store,existing,&state)?,"alreadyInstalled":true}));
    }
    let created = publish(store, &p.install_id, &p.assets, Some(&r))?;
    let skill = Skill {
        id: p.install_id.clone(),
        name: p.metadata.name.clone(),
        description: p.metadata.description.clone(),
        source: format!("catalog:{}", p.metadata.id),
    };
    all.push(skill.clone());
    state.installed.insert(skill.id.clone(), r.digest.clone());
    state.receipts.insert(r.digest.clone(), r);
    state.seed_done |= seed || p.metadata.id == SEED;
    if installation::issue(&skill, &mut state).and_then(|()| persist(store, &all, &state, None)).is_err() {
        if created {
            let _ = fs::remove_dir_all(dir(store, &skill));
        }
        bail!("Não foi possível registrar a importação; tente novamente");
    }
    Ok(json!({"skillProtocol":PROTOCOL,"skill":public(store,&skill,&state)?,"alreadyInstalled":false}))
}

pub(crate) fn install(store: &Store, body: &Value, revalidate: impl FnOnce() -> Result<()>) -> Result<Value> {
    let intent = install_intent(body)?;
    let p = registry::find(&intent.catalog_id)?;
    ensure!(
        intent.expected_digest == p.metadata.digest,
        "A versão do catálogo mudou; confira os detalhes antes de instalar"
    );
    let _guard = MUTATION.locked();
    let _file = root_lock(store)?;
    revalidate()?;
    install_locked(store, &p, false, intent.expected_installation_id.as_deref())
}

pub(crate) fn custom(store: &Store, body: &Value, revalidate: impl FnOnce() -> Result<()>) -> Result<Value> {
    custom_with(store, body, revalidate, |_| Ok(()))
}

// A fronteira permite intercalar a adulteração de arquivo na prova do escritor real.
fn custom_with(
    store: &Store,
    body: &Value,
    revalidate: impl FnOnce() -> Result<()>,
    after_publish: impl FnOnce(&Path) -> Result<()>,
) -> Result<Value> {
    let intent = custom_intent(body)?;
    let _guard = MUTATION.locked();
    let _file = root_lock(store)?;
    revalidate()?;
    let (mut all, mut state) = installation::load_initialized(store)?;
    let taken = all.iter().map(|s| s.id.clone()).collect::<Vec<_>>();
    let (id, md) =
        crate::fork::agent_skills::custom_document(&intent.name, &intent.description, &intent.instructions, &taken)?;
    ensure!(md.len() <= registry::MAX_SKILL, "SKILL.md excede 32 KiB");
    let asset = registry::Asset {
        path: "SKILL.md".into(),
        bytes: md.len(),
        sha256: hash(md.as_bytes()),
        mode: "100644".into(),
        content: md,
    };
    let approved_digest = asset.sha256.clone();
    publish(store, &id, &[asset], None)?;
    after_publish(&store.skills_root.join(&id).join("SKILL.md"))?;
    let skill =
        Skill { id, name: intent.name.trim().to_owned(), description: intent.description, source: "custom".into() };
    all.push(skill.clone());
    if installation::issue_manual(&skill, &mut state, approved_digest)
        .and_then(|()| installation::checked_document(store, &skill, &state).map(|_| ()))
        .and_then(|()| persist(store, &all, &state, None))
        .is_err()
    {
        let _ = fs::remove_dir_all(dir(store, &skill));
        bail!("Não foi possível registrar o texto manual");
    }
    Ok(json!({"skillProtocol":PROTOCOL,"skill":public(store,&skill,&state)?,"alreadyInstalled":false}))
}

#[cfg(test)]
pub(crate) fn remove(store: &Store, body: &Value, revalidate: impl FnOnce() -> Result<()>) -> Result<()> {
    remove_with(store, None, body, revalidate, || Ok(()))
}

fn remove_with(
    store: &Store,
    owner: Option<(&crate::hub::Hub, &crate::fork::autoridade::Autoridade)>,
    body: &Value,
    revalidate: impl FnOnce() -> Result<()>,
    before_effect: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let intent = remove_intent(body)?;
    crate::fork::agent_skills::validate_name(&intent.id, &intent.id)?;
    let _guard = MUTATION.locked();
    let _file = root_lock(store)?;
    revalidate()?;
    let (mut all, mut state) = installation::load_initialized(store)?;
    let Some(skill) = all.iter().find(|s| s.id == intent.id) else {
        return Ok(()); // Não limpa órfãos nem deduz sucesso de uma encarnação nova.
    };
    ensure!(
        installation::matches_id(&state, &intent.id, &intent.expected_installation_id)
            && intent.expected_receipt.matches(installed_receipt(&state, &intent.id)),
        installation::STALE
    );
    // D306: remover não põe texto no bot; corpo alterado/ilegível não impede, registro alterado sim.
    installation::record_intact(skill, &state)?;
    before_effect()?;
    // Persiste indisponibilidade e revogação antes de apagar bytes. Falha deixa órfão privado, nunca registro parcial.
    all.retain(|s| s.id != intent.id);
    if installed_receipt(&state, &intent.id).is_some_and(|r| r.catalog_id == SEED) {
        state.seed_done = true;
    }
    state.installed.remove(&intent.id);
    state.installations.remove(&intent.id);
    match owner {
        Some((hub, owner)) => storage::persist_as_owner(hub, owner, &all, &state, Some(&intent.id))?,
        None => persist(store, &all, &state, Some(&intent.id))?,
    }
    let dest = store.skills_root.join(&intent.id);
    if let Ok(meta) = fs::symlink_metadata(&dest) {
        ensure!(meta.file_type().is_dir(), "Registro removido; pasta irregular preservada para conferência");
        fs::remove_dir_all(dest).map_err(|_| anyhow!("Registro removido; limpeza dos arquivos ainda pendente"))?;
    }
    Ok(())
}

/// c1: catálogo observado sob MUTATION + filesystem, conferido de novo na Connection da escrita.
pub(crate) struct Vista(storage::Snapshot);
impl Vista {
    /// Só a Connection recebida: nenhum Store, Cloud ou lock novo dentro da transação.
    pub(crate) fn conferir(&self, db: &rusqlite::Connection) -> Result<()> {
        storage::unchanged(db, &self.0)
    }
}

/// Instalações vistas no gesto ainda são as atuais enquanto `write` grava. A ordem
/// é a do catálogo: MUTATION → filesystem → DB; `write` não chama o catálogo.
pub(crate) fn with_installations<T>(
    store: &Store,
    expected: &[(String, String)],
    enabling: bool,
    write: impl FnOnce(&Vista) -> Result<T>,
) -> Result<T> {
    let _guard = MUTATION.locked();
    let _file = root_lock(store)?;
    let (all, state) = installation::load_initialized(store)?;
    for (id, expected_id) in expected {
        ensure!(installation::valid_id(expected_id), installation::STALE);
        let skill = all.iter().find(|s| s.id == *id).ok_or_else(|| anyhow!(installation::STALE))?;
        ensure!(installation::matches_id(&state, id, expected_id), installation::STALE);
        // Desligar uma versão alterada é permitido; ligar exige o conteúdo verificado.
        if enabling {
            installation::checked_document(store, skill, &state)?;
        }
    }
    let observed = state.observed.ok_or_else(|| anyhow!(installation::UNAVAILABLE))?;
    write(&Vista(observed))
}

pub(crate) fn seed(store: &Store) -> Result<()> {
    let _guard = MUTATION.locked();
    let _file = root_lock(store)?;
    let (all, mut state) = installation::load_initialized(store)?;
    if state.seed_done {
        return Ok(());
    }
    let p = registry::find(SEED)?;
    if state.receipts.values().any(|r| r.catalog_id == SEED) {
        // Um seed antigo com outro destino/digest já cumpriu a tentativa inicial. Não é upgrade.
        state.seed_done = true;
        persist(store, &all, &state, None)?;
        return Ok(());
    }
    if all
        .iter()
        .any(|s| s.id == p.install_id && installed_receipt(&state, &s.id).is_none_or(|r| r.digest != p.metadata.digest))
    {
        // Um legado com o mesmo nome não é instalação do seed e não pode impedir o Hub de iniciar.
        state.seed_done = true;
        persist(store, &all, &state, None)?;
        tracing::warn!("Seed find-skills não instalado: identidade ocupada por legado/versão diferente, preservados");
        return Ok(());
    }
    install_locked(store, &p, true, None)?;
    Ok(())
}

/// Revalida o chamador depois de aguardar `team_lock`, sem aceitar bot/tenant/capability do pacote.
pub(crate) fn require_current(hub: &crate::hub::Hub, caller: &crate::api::devices::Caller, method: &str) -> Result<()> {
    use crate::api::devices::{self, Caller, Forbidden};
    match caller {
        Caller::Device { key, .. } => {
            let d = devices::authorize(hub, key).map_err(|_| Forbidden)?;
            devices::permit(&Caller::Device { key: key.clone(), scopes: d.scopes, autoridade: None }, method)?;
        }
        _ => devices::permit(caller, method)?,
    }
    Ok(())
}
