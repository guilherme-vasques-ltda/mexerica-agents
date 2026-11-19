// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! The apps' memory methods: the two layers, the user's own edits, the dream log, and the chat cards.

use super::{file_notices, notices};
use crate::fork::autoridade::Autoridade;
use crate::fork::memory_auto::worker::MemorySection;
use crate::hub::Hub;
use crate::store::BotConfig;
use anyhow::{Result, anyhow, bail};
use codync_mem::store::{self as mem, Entry, Error, New, State, Writer};
use serde_json::{Value, json};
use std::sync::Arc;

/// Dream runs the memory screen lists.
const DREAMS_SHOWN: usize = 20;
/// The most of a project's `MEMORY.md` the screen shows.
const PROJECT_MEMORY_BYTES: u64 = 64 * 1024;

pub async fn call(hub: &Arc<Hub>, method: &str, b: &Value) -> Result<Value> {
    let hub = hub.clone();
    let method = method.to_owned();
    let b = b.clone();
    tokio::task::spawn_blocking(move || match method.as_str() {
        "memory" => view(&hub, &bot(&hub, str_arg(&b, "botId")?, &b)?),
        "memoryFile" => super::file_api::read(&hub, &bot(&hub, str_arg(&b, "botId")?, &b)?, &b),
        _ => bail!("not_allowed: autoridade original de memória ausente"),
    })
    .await?
}

pub(crate) const WRITES: &[&str] = &[
    "saveMemory",
    "pinMemory",
    "archiveMemory",
    "forgetMemory",
    "clearMemory",
    "undoDream",
    "answerMemory",
    "undoMemory",
    "saveMemoryFile",
    "deleteMemoryFile",
    "restoreMemoryFile",
    "releaseMemoryRefusal",
];

/// MB → Owner/Cloud → DB. Só a Connection recebida é usada pelo consumidor.
pub(crate) fn write<T>(
    section: &MemorySection<'_>,
    authority: &Autoridade,
    change: impl FnOnce(&rusqlite::Connection, i64, i64) -> Result<T>,
) -> Result<T> {
    let hub = section.hub();
    authority.with_db(hub, None, |db| {
        let rev = crate::store::next_rev(db)?;
        let result = change(db, crate::store::now_ms(), rev)?;
        db.execute("INSERT INTO kv(k,v) VALUES('memory.rev',?1) ON CONFLICT(k) DO UPDATE SET v=excluded.v", [rev])?;
        Ok(result)
    })
}

/// D389/D424: uma única passagem da autoridade ORIGINAL à fila das edições e sugestões do app.
/// A mesma autoridade é conferida na Tx que grava; sem task-local nem nova concessão.
pub(crate) async fn suggestion_call(hub: &Arc<Hub>, method: &str, b: &Value, autoridade: Autoridade) -> Result<Value> {
    let (hub, method, b) = (hub.clone(), method.to_owned(), b.clone());
    tokio::task::spawn_blocking(move || {
        // MX-280: closure destacada não pode gravar depois do sinal de parada.
        let _write = hub.memory_write()?;
        if WRITES.contains(&method.as_str()) {
            return dispatch(&hub, &autoridade, &method, &b);
        }
        let cfg = bot(&hub, str_arg(&b, "botId")?, &b)?;
        let id = str_arg(&b, "id")?;
        if method == "approveMemorySuggestion" {
            crate::fork::memory_auto::suggestions::approve(&hub, &cfg, id, &autoridade)
        } else {
            crate::fork::memory_auto::suggestions::discard(&hub, &cfg, id, &autoridade)
        }
    })
    .await?
}

pub(super) fn dispatch(hub: &Hub, authority: &Autoridade, method: &str, b: &Value) -> Result<Value> {
    if method == "releaseMemoryRefusal" {
        let message = b["messageId"].as_str().unwrap_or_default();
        return super::injecao_gate::release(hub, str_arg(b, "id")?, message, authority, b);
    }
    if let "answerMemory" | "undoMemory" = method {
        let id = str_arg(b, "entryId")?;
        if method == "undoMemory" && id.starts_with("memory-auto-") {
            return crate::fork::memory_auto::apply::undo(hub, id, authority);
        }
        let entry = hub.store.entry(id).ok_or_else(|| anyhow!("Cartão não encontrado."))?;
        if method == "answerMemory" && entry.data["memory"]["confirmation"] == true {
            let chat = str_arg(b, "botId")?;
            let revision = b["entryRev"].as_i64().ok_or_else(|| anyhow!("Atualize o app para responder ao cartão."))?;
            let accept = b["accept"].as_bool().ok_or_else(|| anyhow!("Escolha salvar ou recusar."))?;
            return notices::answer_confirmed(hub, id, chat, revision, accept);
        }
        let file = file_notices::is_file_card(&entry.data);
        return match (method, file) {
            ("answerMemory", true) => file_notices::answer(hub, id, b["accept"] == true),
            ("answerMemory", false) => notices::answer(hub, id, b["accept"] == true),
            (_, true) => file_notices::undo(hub, authority, id),
            _ => notices::undo(hub, authority, id),
        };
    }
    if method == "memoryFile" || method.ends_with("MemoryFile") {
        return super::file_api::call(hub, authority, &bot(hub, str_arg(b, "botId")?, b)?, method, b);
    }
    let cfg = bot(hub, str_arg(b, "botId")?, b)?;
    let id = || b["id"].as_i64().ok_or_else(|| anyhow!("id required"));
    match method {
        "memory" => view(hub, &cfg),
        "saveMemory" => {
            let text = str_arg(b, "text")?;
            let entry = if let Some(id) = b["id"].as_i64() {
                edit(hub, authority, &cfg, id, |c, now, rev| {
                    mem::revise(c, Writer::User, id, text, now, rev)?;
                    codync_mem::nature::set(c, id, codync_mem::nature::KindNature::Said, None, None)?;
                    mem::get(c, id)?.ok_or(Error::NotFound)
                })?
            } else {
                let layer = (b["shared"] != true).then_some(cfg.memory_id());
                let new = New { bot: layer, text, room: None, evidence: &[] };
                let entry = write(&MemorySection::enter(hub), authority, |c, now, rev| {
                    let entry = mem::add(c, Writer::User, new, now, rev)?;
                    codync_mem::origin::set(
                        c,
                        entry.id,
                        &codync_mem::origin::Origin {
                            schema: 1,
                            kind: codync_mem::origin::Kind::User,
                            sources: Vec::new(),
                            external: false,
                        },
                    )?;
                    codync_mem::nature::set(c, entry.id, codync_mem::nature::KindNature::Said, None, None)?;
                    Ok(mem::get(c, entry.id)?.ok_or(Error::NotFound)?)
                })?;
                super::changed(hub, entry.bot.as_deref());
                entry
            };
            Ok(json!({"entry": entry}))
        }
        "pinMemory" => {
            let (id, pinned) = (id()?, b["pinned"] == true);
            edit(hub, authority, &cfg, id, |c, now, rev| mem::pin(c, id, pinned, now, rev))?;
            Ok(json!({}))
        }
        "archiveMemory" => {
            let id = id()?;
            if b["archived"] == false {
                edit(hub, authority, &cfg, id, |c, now, rev| mem::bring_back(c, id, now, rev))?;
            } else {
                edit(hub, authority, &cfg, id, |c, now, rev| mem::archive(c, Writer::User, id, None, now, rev))?;
            }
            Ok(json!({}))
        }
        "forgetMemory" => {
            let id = id()?;
            edit(hub, authority, &cfg, id, |c, _, _| mem::forget(c, id))?;
            Ok(json!({}))
        }
        "clearMemory" => {
            let layer = (b["shared"] != true).then_some(cfg.memory_id());
            let disk_names = std::fs::read_dir(super::files::folder(layer))
                .into_iter()
                .flatten()
                .filter_map(|entry| entry.ok().and_then(|entry| entry.file_name().into_string().ok()))
                .filter(|name| codync_mem::files::valid_name(name))
                .collect::<Vec<_>>();
            let names = write(&MemorySection::enter(hub), authority, |db, _, rev| {
                let mut names = disk_names;
                // A limpeza da camada compartilhada também apaga as recusas cujo alvo é essa camada,
                // de todos os bots — antes de esquecê-la, enquanto os alvos ainda existem.
                if layer.is_none() {
                    crate::fork::memory_auto::recibo_injecao::purge_shared(db)?;
                }
                mem::forget_layer(db, layer)?;
                if let Some(layer) = layer {
                    crate::fork::memory_auto::recibo_injecao::purge(db, layer)?;
                }
                if layer.is_none() {
                    super::profile::removed(db, rev)?;
                }
                names.extend(codync_mem::files::forget_layer(db, layer)?);
                names.sort();
                names.dedup();
                Ok(names)
            })?;
            for name in names {
                super::file_api::project(hub, layer, &name);
            }
            super::changed(hub, layer);
            Ok(json!({}))
        }
        "undoDream" => {
            let id = id()?;
            let projections = write(&MemorySection::enter(hub), authority, |db, now, rev| {
                let before = codync_mem::dream::get(db, id)?.ok_or(Error::NotFound)?;
                anyhow::ensure!(before.bot == cfg.id, "unknown dream");
                let dream = codync_mem::dream::undo(db, id, now, rev)?;
                let mut projections = Vec::new();
                if !before.undone {
                    for change in dream.files.iter().rev() {
                        let layer = (!change.shared).then_some(dream.bot.as_str());
                        let versions = codync_mem::files::versions(db, layer, &change.name, 50)?;
                        let Some(at) = versions.iter().position(|v| v.id == change.version) else { continue };
                        let expected = versions[at].content.as_deref().map(codync_mem::files::hash);
                        // Uma nota alterada desde o sonho fica como está.
                        if codync_mem::files::current(db, layer, &change.name)?.as_deref().map(codync_mem::files::hash)
                            != expected
                        {
                            continue;
                        }
                        let source = versions.get(at + 1);
                        let content = source.and_then(|v| v.content.as_deref());
                        super::file_api::record(
                            db,
                            &codync_mem::files::Change {
                                bot: layer,
                                name: &change.name,
                                content,
                                author: codync_mem::files::Author::User,
                                author_bot: None,
                                cause: codync_mem::files::Cause::Undo,
                                mtime: now,
                                size: content.map_or(0, |s| s.len() as u64),
                            },
                            expected.as_deref(),
                            source,
                            now,
                            rev,
                        )?;
                        projections.push((layer.map(str::to_owned), change.name.clone()));
                    }
                }
                Ok(projections)
            })?;
            for (layer, name) in projections {
                super::file_api::project(hub, layer.as_deref(), &name);
            }
            // A run may have written to either layer.
            super::changed(hub, None);
            Ok(json!({}))
        }
        _ => bail!("unknown method: {method}"),
    }
}

/// Changes one entry the bot can see, then tells the bots that see it.
fn edit(
    hub: &Hub,
    authority: &Autoridade,
    cfg: &BotConfig,
    id: i64,
    change: impl FnOnce(&rusqlite::Connection, i64, i64) -> mem::Result<Entry>,
) -> Result<Entry> {
    let bot = cfg.memory_id().to_owned();
    let entry = write(&MemorySection::enter(hub), authority, |c, now, rev| {
        let entry = mem::get(c, id)?.ok_or(Error::NotFound)?;
        if entry.bot.as_deref().is_some_and(|owner| owner != bot)
            || (entry.bot.is_none() && cfg.space_id != crate::fork::spaces::PERSONAL)
        {
            return Err(Error::NotFound.into());
        }
        Ok(change(c, now, rev)?)
    })?;
    super::changed(hub, entry.bot.as_deref());
    Ok(entry)
}

fn view(hub: &Hub, cfg: &BotConfig) -> Result<Value> {
    let bot = cfg.memory_id();
    let (yours, own, dreams) = hub.store.memory_read(|c| {
        Ok((
            if cfg.space_id == crate::fork::spaces::PERSONAL { mem::layer(c, None)? } else { Vec::new() },
            mem::layer(c, Some(bot))?,
            codync_mem::dream::recent(c, bot, DREAMS_SHOWN)?,
        ))
    })?;
    let layer = |entries: Vec<Entry>, layer: Option<&str>| {
        let used: usize =
            entries.iter().filter(|e| e.state == State::Active).map(|e| codync_mem::tokens::estimate(&e.text)).sum();
        json!({"entries": entries, "used": used, "cap": mem::cap(layer)})
    };
    let session =
        hub.runtime(&cfg.id).session.or_else(|| hub.store.bot(&cfg.id).ok().flatten().and_then(|row| row.session_id));
    let session = session.as_deref().map(|id| hub.store.memory_session_info(&cfg.id, id)).transpose()?;
    let discarded = hub
        .store
        .kv_read(&format!("memory.dream.discarded.{}", cfg.id))?
        .and_then(|value| serde_json::from_str::<Value>(&value).ok());
    Ok(json!({
        "sessionDiagnostic": session,
        "automatic": crate::fork::memory_auto::queue::disposition(&hub.store, &cfg.id, cfg.session_owner())?,
        "nonSaved": crate::fork::memory_auto::audit::list(&hub.store, cfg)?,
        "openingOmitted": super::omitted::list(&hub.store, cfg)?,
        "learning": crate::fork::memory_learning::mode(&hub.store)?,
        "suggestions": crate::fork::memory_auto::suggestions::list(&hub.store, cfg)?,
        "discardedDreamChanges": discarded,
        "shared": super::shared(cfg),
        "yours": layer(yours, None),
        "bot": layer(own, Some(bot)),
        "files": super::file_api::listing(hub, cfg)?,
        "dreams": dreams,
        "projectMemory": project_memory(cfg),
        "memoryRev": hub.store.memory_rev(),
    }))
}

/// Claude Code's own memory of the bot's folder (`~/.claude/projects/<folder>/memory/MEMORY.md`), where the user
/// kept it on, shown read-only so nothing a bot remembers is out of the user's sight. Empty text until Claude Code
/// saves something, so the apps show its section as soon as the switch is on.
fn project_memory(cfg: &BotConfig) -> Value {
    if cfg.backend != "claude" || !crate::agent::own_memory::kept(cfg) {
        return Value::Null;
    }
    let folder: String = cfg.cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let Some(path) = dirs::home_dir().map(|home| home.join(".claude/projects").join(folder).join("memory/MEMORY.md"))
    else {
        return Value::Null;
    };
    let read = std::fs::File::open(&path).and_then(|file| {
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut std::io::Read::take(file, PROJECT_MEMORY_BYTES), &mut bytes)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    });
    match read {
        Ok(text) => json!({"path": path.to_string_lossy(), "text": text}),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({"path": path.to_string_lossy(), "text": ""}),
        Err(error) => {
            tracing::warn!(%error, "couldn't read Claude Code's memory");
            Value::Null
        }
    }
}

fn bot(hub: &Hub, id: &str, body: &Value) -> Result<BotConfig> {
    let cfg = hub.store.bot(id)?.filter(|b| !b.deleted).ok_or_else(|| anyhow!("unknown bot"))?.config;
    if cfg.is_group() {
        bail!("a group has no memory of its own");
    }
    if let Some(chat) = body["conversationId"].as_str() {
        let conversation =
            hub.store.space_conversation(body["spaceId"].as_str().unwrap_or(crate::fork::spaces::PERSONAL), chat)?;
        anyhow::ensure!(conversation.bot_id == id, "cross_conversation_identity");
        return crate::fork::spaces::resource_config(&hub.store, &cfg, chat);
    }
    Ok(cfg)
}

fn str_arg<'a>(b: &'a Value, k: &str) -> Result<&'a str> {
    b[k].as_str().ok_or_else(|| anyhow!("{k} required"))
}
