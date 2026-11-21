// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! The apps' methods for long-form memory files: list, read with versions, save, delete and restore. The user's
//! own edits skip the checks a bot's pass, and every write is compare-and-swap on the content the app loaded.

use super::files;
use crate::fork::autoridade::Autoridade;
use crate::fork::memory_auto::worker::MemorySection;
use crate::hub::Hub;
use crate::store::BotConfig;
use anyhow::{Result, anyhow, bail, ensure};
use codync_mem::files::{self as mf, Author, Cause};
use serde_json::{Value, json};

/// Versions the detail view lists.
const VERSIONS_SHOWN: usize = 20;

/// Both layers' files for the memory screen, after a scan.
pub fn listing(hub: &Hub, cfg: &BotConfig) -> Result<Value> {
    files::scan(hub, cfg);
    let layer = |bot: Option<&str>| -> Result<Value> {
        let list = hub.store.memory_read(|c| mf::layer(c, bot))?;
        Ok(json!({"files": list, "folder": files::folder(bot).display().to_string()}))
    };
    Ok(json!({
        "yours": if cfg.space_id == crate::fork::spaces::PERSONAL {layer(None)?}else{json!({"files":[],"folder":null})},
        "bot": layer(Some(cfg.memory_id()))?,
        "cap": mf::FILE_CAP,
        "folderCap": mf::FOLDER_CAP,
    }))
}

pub fn read(hub: &Hub, cfg: &BotConfig, b: &Value) -> Result<Value> {
    let name = b["file"].as_str().ok_or_else(|| anyhow!("file required"))?;
    detail(hub, cfg, (b["shared"] != true).then_some(cfg.memory_id()), name)
}

pub fn call(hub: &Hub, authority: &Autoridade, cfg: &BotConfig, method: &str, b: &Value) -> Result<Value> {
    let name = b["file"].as_str().ok_or_else(|| anyhow!("file required"))?;
    let layer = (b["shared"] != true).then_some(cfg.memory_id());
    ensure!(layer.is_some() || name != super::profile::NAME, "memory_user_owned");
    let base = b["base"].as_str().filter(|h| !h.is_empty());
    // Só leitura antes da Tx. O CAS autoritativo do diário é conferido dentro dela.
    let disk = if files::folder(layer).is_dir() { files::read(layer, name)? } else { None };
    let version = super::api::write(&MemorySection::enter(hub), authority, |db, now, rev| {
        ensure!(
            disk == mf::current(db, layer, name)?,
            "{name} changed meanwhile. Read it again and redo the change on top of it."
        );
        let old = if method == "restoreMemoryFile" {
            let id = b["version"].as_i64().ok_or_else(|| anyhow!("version required"))?;
            Some(
                mf::version(db, id)?
                    .filter(|v| v.name == name && v.bot.as_deref() == layer)
                    .ok_or_else(|| anyhow!("unknown version"))?,
            )
        } else {
            None
        };
        let (content, cause) = match method {
            "saveMemoryFile" => {
                let content = b["content"].as_str().ok_or_else(|| anyhow!("content required"))?;
                ensure!(!content.trim().is_empty(), "A memory file can't be empty; delete it instead.");
                (Some(content), Cause::Edit)
            }
            "deleteMemoryFile" => (None, Cause::Delete),
            "restoreMemoryFile" => (old.as_ref().and_then(|v| v.content.as_deref()), Cause::Restore),
            _ => bail!("unknown method: {method}"),
        };
        record(
            db,
            &mf::Change {
                bot: layer,
                name,
                content,
                author: Author::User,
                author_bot: None,
                cause,
                mtime: now,
                size: content.map_or(0, |s| s.len() as u64),
            },
            base,
            old.as_ref(),
            now,
            rev,
        )
    })?;
    project(hub, layer, name);
    super::changed(hub, layer);
    Ok(json!({"version": version}))
}

/// Caminho User: versão e compare-and-swap só no diário, na Connection autorizada.
pub(super) fn record(
    db: &rusqlite::Connection,
    change: &mf::Change<'_>,
    expected: Option<&str>,
    source: Option<&mf::Version>,
    now: i64,
    rev: i64,
) -> Result<mf::Version> {
    ensure!(mf::valid_name(change.name), "Nome de arquivo de memória inválido.");
    let trusted = mf::current(db, change.bot, change.name)?;
    ensure!(
        trusted.as_deref().map(mf::hash).as_deref() == expected,
        "{} changed meanwhile. Read it again and redo the change on top of it.",
        change.name
    );
    if let Some(alternative) = source.and_then(|v| v.conflict_text.as_deref()) {
        codync_mem::check::secrets(alternative)?;
    }
    if trusted.as_deref() == change.content && source.is_none() && change.cause != Cause::Edit {
        let file = mf::get(db, change.bot, change.name)?.ok_or(codync_mem::store::Error::NotFound)?;
        return Ok(mf::version(db, file.version)?.ok_or(codync_mem::store::Error::NotFound)?);
    }
    let creating = source.is_none() && change.cause == Cause::Edit && mf::get(db, change.bot, change.name)?.is_none();
    let version = mf::record(db, change, now, rev)?;
    if let Some(source) = source {
        codync_mem::origin::set_file(db, version.id, &source.origin)?;
        codync_mem::nature::set_file(
            db,
            version.id,
            source.nature,
            source.nature_bot.as_deref(),
            source.conflict_text.as_deref(),
        )?;
    } else if change.cause == Cause::Edit {
        if creating {
            codync_mem::origin::set_file(
                db,
                version.id,
                &codync_mem::origin::Origin {
                    schema: 1,
                    kind: codync_mem::origin::Kind::User,
                    sources: Vec::new(),
                    external: false,
                },
            )?;
        }
        codync_mem::nature::set_file(db, version.id, codync_mem::nature::KindNature::Said, None, None)?;
    }
    Ok(mf::version(db, version.id)?.ok_or(codync_mem::store::Error::NotFound)?)
}

/// Depois do commit e de todas as guardas: falha de projeção não desfaz o diário.
pub(super) fn project(hub: &Hub, layer: Option<&str>, name: &str) {
    if files::project_committed(hub, layer, name).is_err() {
        tracing::warn!("projeção de edição de memória aguardando reconciliação");
    }
}

/// A file as it is on disk now, its hash for the next save, and its recent versions.
fn detail(hub: &Hub, cfg: &BotConfig, layer: Option<&str>, name: &str) -> Result<Value> {
    files::scan(hub, cfg);
    // D6: a nota do dono não é projetada no disco; a tela a lê do diário, sem a marca interna.
    let content = if layer.is_none() && name == super::profile::NAME {
        hub.store.memory_read(|c| mf::current(c, layer, name))?.map(|s| super::profile::visible(&s).to_owned())
    } else {
        files::read(layer, name)?
    };
    let mut versions = hub.store.memory_read(|c| mf::versions(c, layer, name, VERSIONS_SHOWN))?;
    if layer.is_none() && name == super::profile::NAME {
        for v in &mut versions {
            v.content = v.content.take().map(|s| super::profile::visible(&s).to_owned());
        }
    }
    Ok(json!({
        "file": name,
        "shared": layer.is_none(),
        "content": content,
        "hash": content.as_deref().map(mf::hash),
        "path": files::folder(layer).join(name).display().to_string(),
        "versions": versions,
    }))
}
