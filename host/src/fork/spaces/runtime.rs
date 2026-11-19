//! Configuração transitória de execução: identidade estável, contexto sem heranças implícitas.
use crate::{
    LockExt,
    store::{BotConfig, Store},
};
use anyhow::{Result, anyhow, ensure};
use rusqlite::OptionalExtension;
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionContext {
    pub conversation_id: String,
    pub memory_id: String,
    pub foreign_identity: bool,
}
pub fn session_slot(cfg: &BotConfig) -> Option<String> {
    cfg.context.as_ref().map(|c| format!("conversation:{}", c.conversation_id))
}

pub fn execution_config(store: &Store, original: &BotConfig, chat: &str) -> Result<BotConfig> {
    configuration(store, original, chat, false)
}
pub fn resource_config(store: &Store, original: &BotConfig, chat: &str) -> Result<BotConfig> {
    configuration(store, original, chat, true)
}
fn configuration(store: &Store, original: &BotConfig, chat: &str, view: bool) -> Result<BotConfig> {
    if chat == original.id {
        return Ok(original.clone());
    }
    let conversation = execution_conversation_on(&store.db.locked(), original, chat, view)?;
    let Some(conversation) = conversation else { return Ok(original.clone()) };
    let space = conversation.space_id.clone();
    let foreign = space != original.space_id;
    let memory = if foreign {
        let key = context_memory_key(&conversation, original);
        let mut db = store.db.locked();
        let tx = db.transaction()?;
        let before = context_memory_on(&tx, original, &conversation)?;
        let id = before.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        uuid::Uuid::parse_str(&id)?;
        tx.execute("INSERT OR IGNORE INTO kv(k,v) VALUES(?1,?2)", [key, id.clone()])?;
        tx.commit()?;
        id
    } else {
        original.memory_id().to_owned()
    };
    let root = store.engine_setup_root.parent().ok_or_else(|| anyhow!("space_storage_unavailable"))?;
    let root =
        if crate::fork::motor_exec::configured() { crate::fork::motor_roots::work() } else { root.to_path_buf() };
    let cwd = root
        .join("spaces")
        .join(&space)
        .join("conversations")
        .join(chat)
        .join("workspace")
        .to_string_lossy()
        .into_owned();
    Ok(context_config(original, &conversation, &memory, &cwd))
}

fn context_memory_key(conversation: &super::conversations::Conversation, original: &BotConfig) -> String {
    format!("donna.space.memory.{}.{}", conversation.space_id, original.id)
}

pub(super) fn context_memory_on(
    c: &rusqlite::Connection,
    original: &BotConfig,
    conversation: &super::conversations::Conversation,
) -> Result<Option<String>> {
    if conversation.space_id == original.space_id {
        return Ok(Some(original.memory_id().to_owned()));
    }
    let id: Option<String> = c
        .query_row("SELECT v FROM kv WHERE k=?1", [context_memory_key(conversation, original)], |r| r.get(0))
        .optional()?;
    if let Some(id) = &id {
        uuid::Uuid::parse_str(id)?;
    }
    Ok(id)
}

pub(super) fn context_config(
    original: &BotConfig,
    conversation: &super::conversations::Conversation,
    memory: &str,
    cwd: &str,
) -> BotConfig {
    let space = &conversation.space_id;
    let chat = &conversation.id;
    let foreign = conversation.space_id != original.space_id;
    let mut cfg = original.clone();
    cfg.space_id.clone_from(space);
    cfg.command = None;
    cfg.fork_editor.engine_account_id.clone_from(&conversation.engine_account_id);
    if let Some(selection) = &conversation.engine_selection {
        cfg.backend.clone_from(&selection.backend);
        cfg.model.clone_from(&selection.model);
        cfg.model_options = selection.model_options.clone();
    }
    if !foreign {
        // A pasta da adicional é gerenciada; isso não pode ampliar a memória compartilhada.
        cfg.shared_memory = Some(crate::chat::memory::shared(original));
    }
    if foreign {
        cfg.description.clear();
        cfg.owner_preferences.clear();
        cfg.connectors.clear();
        cfg.skills.clear();
        cfg.logins.clear();
        cfg.computer = false;
        cfg.manage_team = false;
        cfg.harness_memory = false;
        cfg.shared_memory = Some(false);
        // Regras do bot original continuam teto. A preferência de Space não as relaxa.
        cfg.permission = crate::store::restrictive(original.permission, crate::store::Permission::Ask);
    }
    cfg.cwd = cwd.into();
    cfg.context =
        Some(ExecutionContext { conversation_id: chat.clone(), memory_id: memory.into(), foreign_identity: foreign });
    cfg
}

/// Mesmo produtor de contexto no preparo e na transação de permissão.
pub(super) fn execution_conversation_on(
    c: &rusqlite::Connection,
    original: &BotConfig,
    chat: &str,
    view: bool,
) -> Result<Option<super::conversations::Conversation>> {
    if chat == original.id {
        return Ok(None);
    }
    let space: Option<String> =
        c.query_row("SELECT space_id FROM donna_space_conversations WHERE id=?1", [chat], |r| r.get(0)).optional()?;
    let Some(space) = space else {
        return Err(anyhow!("conversation_not_found"));
    };
    let conversation = Store::space_conversation_on(c, &space, chat)?;
    ensure!(view || !conversation.closed, "conversation_closed");
    if conversation.bot_id != original.id {
        let group = Store::bot_on(c, chat)?
            .filter(|r| !r.deleted && r.config.is_group())
            .ok_or_else(|| anyhow!("cross_conversation_identity"))?
            .config;
        ensure!(group.space_id == original.space_id && group.members.contains(&original.id), "cross_space_member");
        return Ok(None);
    }
    uuid::Uuid::parse_str(&space)?;
    uuid::Uuid::parse_str(chat)?;
    let foreign = space != original.space_id;
    if foreign {
        ensure!(
            crate::store::team_settings_on(c)?.primary_bot_id.as_deref() == Some(&original.id),
            "cross_space_reference"
        );
    }
    Ok(Some(conversation))
}

pub fn prepare_workspace(cfg: &BotConfig) -> Result<()> {
    if cfg.context.is_none() {
        return Ok(());
    }
    let path = std::path::Path::new(&cfg.cwd);
    if crate::fork::motor_exec::configured() {
        return crate::fork::motor_roots::prepare_workspace(path);
    }
    // Caminho é emitido pelo Hub e nunca desserializado de um patch de cliente.
    let mut ancestors = path.ancestors().take(6).collect::<Vec<_>>();
    ancestors.reverse();
    for path in ancestors {
        super::super::engine_accounts::managed::private_dir(path)?;
    }
    Ok(())
}
pub fn bind_session(store: &Store, bot: &str, chat: &str, sid: &str) -> Result<()> {
    ensure!(!sid.is_empty() && sid.len() <= 1024, "invalid_context_session");
    let key = format!("donna.space.session.{}", crate::remote::crypto::b64(format!("{bot}\0{chat}\0{sid}").as_bytes()));
    let mut db = store.db.locked();
    let tx = db.transaction()?;
    let owner: Option<String> = tx.query_row("SELECT v FROM kv WHERE k=?1", [&key], |r| r.get(0)).optional()?;
    ensure!(owner.as_deref().is_none_or(|owner| owner == chat), "session_belongs_to_other_conversation");
    tx.execute("INSERT OR IGNORE INTO kv(k,v) VALUES(?1,?2)", [key, chat.into()])?;
    tx.commit()?;
    Ok(())
}
pub fn stamp_entry(c: &rusqlite::Connection, chat: &str, data: &Value) -> Result<Value> {
    let context: Option<(String, String, i64)> = c
        .query_row("SELECT space_id,bot_id,revision FROM donna_space_conversations WHERE id=?1", [chat], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?;
    let mut data = data.clone();
    if let Some((space, bot, revision)) = context {
        data["spaceContext"] =
            json!({"version":1,"spaceId":space,"conversationId":chat,"botId":bot,"conversationRevision":revision});
    }
    Ok(data)
}
pub fn inflight(raw: &str, bot: &str) -> Option<crate::store::Lane> {
    let legacy = raw.parse::<i64>().ok().map(|started| (started, bot.to_owned()));
    let value: Value = serde_json::from_str(raw).unwrap_or(Value::Null);
    if value.get("admissionVersion").is_some() {
        return None;
    }
    let (started, chat) =
        legacy.or_else(|| Some((value["started"].as_i64()?, value["conversationId"].as_str()?.to_owned())))?;
    (crate::store::now_ms() - started < 60 * 60 * 1000).then(|| crate::store::Lane::main(&chat))
}
