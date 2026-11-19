//! Selos privados de admissão. Um hash não é grant e nunca substitui as regras do Hub.
use super::conversations::{Conversation, EngineSelection};
use crate::LockExt;
use crate::{
    fork::engine_accounts::{Registry, managed},
    store::{BotConfig, Store},
};
use anyhow::{Result, anyhow, ensure};
use base64::Engine as _;
use rusqlite::{Connection, OptionalExtension};
use serde_json::json;
use sha2::{Digest, Sha256};

fn hash(value: &serde_json::Value) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(value.to_string().as_bytes()))
}
pub(super) fn account(store: &Store, id: &str) -> Result<(u64, String)> {
    account_on(&store.db.locked(), id)
}
pub(super) fn account_on(db: &Connection, id: &str) -> Result<(u64, String)> {
    account_in(&Registry::load_db(db)?, id)
}
fn account_in(registry: &Registry, id: &str) -> Result<(u64, String)> {
    let account = registry.account(id)?;
    let revision = account.managed.as_ref().map_or(1, |m| m.revision);
    let metadata = account
        .managed
        .as_ref()
        .map(|m| json!({"revision":m.revision,"generation":m.generation,"principal":m.principal,"enabled":m.enabled}));
    Ok((revision, hash(&json!({"id":account.id,"backend":account.backend,"launch":account.launch,"managed":metadata}))))
}
pub(super) fn select(
    store: &Store,
    source: &BotConfig,
    conversation: &mut Conversation,
    body: &serde_json::Value,
) -> Result<()> {
    let changed = body.get("engineAccountId").is_some() || body.get("engineSelection").is_some();
    if !changed {
        return Ok(());
    }
    if let Some(id) = body.get("engineAccountId") {
        conversation.engine_account_id =
            if id.is_null() { None } else { Some(id.as_str().ok_or_else(|| anyhow!("invalid_account"))?.to_owned()) };
    }
    let Some(id) = conversation.engine_account_id.as_deref() else {
        conversation.engine_selection = None;
        conversation.account_revision = None;
        conversation.account_stamp = None;
        return Ok(());
    };
    let (revision, stamp) = account(store, id)?;
    ensure!(body["expectedAccountRevision"].as_u64() == Some(revision), "account_revision_conflict");
    let selection = if let Some(value) = body.get("engineSelection") {
        serde_json::from_value(value.clone())?
    } else {
        conversation.engine_selection.clone().unwrap_or_else(|| EngineSelection {
            backend: source.backend.clone(),
            model: source.model.clone(),
            model_options: source.model_options.clone(),
        })
    };
    ensure!(selection.model.is_some(), "model_selection_required");
    let mut cfg = source.clone();
    cfg.backend.clone_from(&selection.backend);
    cfg.model.clone_from(&selection.model);
    cfg.model_options = selection.model_options.clone();
    cfg.command = None;
    cfg.fork_editor.engine_account_id = Some(id.into());
    managed::validate_selection(store, &cfg)?;
    conversation.engine_selection = Some(selection);
    conversation.account_revision = Some(i64::try_from(revision)?);
    conversation.account_stamp = Some(stamp);
    Ok(())
}
fn verify_in(registry: &Registry, conversation: &Conversation, cfg: &BotConfig) -> Result<()> {
    let id = conversation.engine_account_id.as_deref().ok_or_else(|| anyhow!("space_account_selection_required"))?;
    ensure!(cfg.fork_editor.engine_account_id.as_deref() == Some(id), "account_binding_changed");
    let (revision, stamp) = account_in(registry, id)?;
    ensure!(
        conversation.account_revision == Some(i64::try_from(revision)?)
            && conversation.account_stamp.as_deref() == Some(&stamp),
        "account_binding_changed"
    );
    managed::validate_selection_in(registry, cfg)
}
pub fn execution_stamp(store: &Store, cfg: &BotConfig) -> Result<String> {
    execution_stamp_on(&store.db.locked(), cfg)
}
fn stamp(host_id: Option<&str>, generation: Option<i64>, conversation: &Conversation, cfg: &BotConfig) -> String {
    hash(&stamp_payload(host_id, generation, conversation, cfg))
}
fn stamp_payload(
    host_id: Option<&str>,
    generation: Option<i64>,
    conversation: &Conversation,
    cfg: &BotConfig,
) -> serde_json::Value {
    let mut value = json!({"bindingVersion":2,"memoryPolicy":crate::fork::bot_admission::MemoryPolicy::of_config(cfg),"hubId":host_id,"spaceId":cfg.space_id,"conversationId":conversation.id,"botId":cfg.id,
        "revision":conversation.revision,"account":conversation.engine_account_id,"accountStamp":conversation.account_stamp,
        "backend":cfg.backend,"model":cfg.model,"modelOptions":cfg.model_options,"permission":cfg.permission,
        "sharedMemory":crate::chat::memory::shared(cfg),"connectors":cfg.connectors,"skills":cfg.skills,"logins":cfg.logins,"computer":cfg.computer,"manageTeam":cfg.manage_team,"cwd":cfg.cwd});
    // Geração do perfil (main #250+): A → B → A não restaura captura antiga.
    if let Some(generation) = generation {
        value["profileGeneration"] = generation.into();
    }
    value
}
fn current_context_on(c: &Connection, cfg: &BotConfig) -> Result<(Conversation, BotConfig)> {
    let context = cfg.context.as_ref().ok_or_else(|| anyhow!("conversation_context_required"))?;
    let original = Store::bot_on(c, &cfg.id)?.filter(|b| !b.deleted).ok_or_else(|| anyhow!("bot_not_found"))?.config;
    let conversation = super::runtime::execution_conversation_on(c, &original, &context.conversation_id, false)?
        .ok_or_else(|| anyhow!("conversation_context_required"))?;
    ensure!(conversation.space_id == cfg.space_id && conversation.bot_id == cfg.id, "cross_conversation_identity");
    let memory = super::runtime::context_memory_on(c, &original, &conversation)?
        .ok_or_else(|| anyhow!("memory_binding_unavailable"))?;
    let current = super::runtime::context_config(&original, &conversation, &memory, &cfg.cwd);
    Ok((conversation, current))
}
pub(crate) fn current_memory_policy_on(
    c: &Connection,
    cfg: &BotConfig,
) -> Result<crate::fork::bot_admission::MemoryPolicy> {
    let current = if cfg.context.is_some() {
        current_context_on(c, cfg)?.1
    } else {
        Store::bot_on(c, &cfg.id)?.filter(|b| !b.deleted).ok_or_else(|| anyhow!("bot_not_found"))?.config
    };
    Ok(crate::fork::bot_admission::MemoryPolicy::of_config(&current))
}
pub(crate) fn execution_stamp_on(c: &Connection, cfg: &BotConfig) -> Result<String> {
    let (conversation, current) = current_context_on(c, cfg)?;
    verify_in(&Registry::load_db(c)?, &conversation, cfg)?;
    let host_id: Option<String> = c.query_row("SELECT v FROM kv WHERE k='host_id'", [], |r| r.get(0)).optional()?;
    let foreign = cfg.context.as_ref().is_some_and(|context| context.foreign_identity);
    let generation = if foreign { None } else { Some(super::config_generation::current_on(c, &cfg.id)?) };
    let captured = stamp(host_id.as_deref(), generation, &conversation, cfg);
    ensure!(captured == stamp(host_id.as_deref(), generation, &conversation, &current), "conversation_binding_changed");
    Ok(captured)
}
fn entry_matches(cfg: &BotConfig, entry: &crate::store::Entry, stamp: &str) -> bool {
    entry.bot_id == cfg.session_owner() && entry.data["executionBinding"].as_str() == Some(stamp)
}

/// Validador canônico da última permissão. Só usa a `Connection` recebida;
/// nenhuma leitura recursiva de Store/Cloud, mutação de status ou await.
pub(crate) fn validate_dispatch_on(c: &Connection, cfg: &BotConfig, chat: &str, ids: &[String]) -> Result<()> {
    if cfg.context.is_none() {
        if chat != cfg.id {
            let original =
                Store::bot_on(c, &cfg.id)?.filter(|b| !b.deleted).ok_or_else(|| anyhow!("bot_not_found"))?.config;
            ensure!(
                super::runtime::execution_conversation_on(c, &original, chat, false)?.is_none(),
                "conversation_context_required"
            );
        }
        return Ok(());
    }
    ensure!(chat == cfg.session_owner(), "cross_conversation_identity");
    let stamp = execution_stamp_on(c, cfg)?;
    for id in ids {
        let entry = Store::entry_on(c, id)?.ok_or_else(|| anyhow!("message_not_found"))?;
        ensure!(entry_matches(cfg, &entry, &stamp), "conversation_binding_changed");
    }
    Ok(())
}
pub fn validate_entries(hub: &crate::hub::Hub, cfg: &BotConfig, ids: &[String]) -> Result<()> {
    if cfg.context.is_none() {
        return Ok(());
    }
    let stamp = execution_stamp(&hub.store, cfg);
    let mut invalid = false;
    for id in ids {
        let entry = hub.store.entry(id).ok_or_else(|| anyhow!("message_not_found"))?;
        super::connector_cards::check_resume(hub, cfg, &entry)?;
        let valid = stamp.as_ref().is_ok_and(|stamp| entry_matches(cfg, &entry, stamp));
        let valid = valid && super::uploads::validate_entry(&hub.store, cfg, &entry).is_ok();
        if !valid {
            if entry.bot_id == cfg.session_owner() {
                let mut data = entry.data.clone();
                data["status"] = "failed".into();
                data["error"] = "O contexto ou a conta mudou. Confira antes de reenviar.".into();
                hub.set_entry(id, &data);
            }
            invalid = true;
        }
    }
    stamp?;
    ensure!(!invalid, "conversation_binding_changed");
    Ok(())
}

#[cfg(test)]
mod memory_binding_tests {
    use super::*;
    #[test]
    fn selo_antigo_sem_politica_nao_e_promovido_e_cosmeticos_nao_sao_poderes() {
        let mut cfg: BotConfig =
            serde_json::from_value(json!({"id":"bot","name":"Bot","backend":"codex","sharedMemory":true})).unwrap();
        cfg.context = Some(super::super::ExecutionContext {
            conversation_id: "conversation".into(),
            memory_id: "bot".into(),
            foreign_identity: false,
        });
        let conversation: Conversation = serde_json::from_value(json!({
            "id":"conversation","botId":"bot","spaceId":cfg.space_id,"title":"Conversa","revision":1,
            "primary":false,"closed":false,"stateRevision":1,"readRevision":0
        }))
        .unwrap();
        let current = stamp(Some("hub"), Some(1), &conversation, &cfg);
        let mut old = stamp_payload(Some("hub"), Some(1), &conversation, &cfg);
        old.as_object_mut().unwrap().remove("memoryPolicy");
        old.as_object_mut().unwrap().remove("bindingVersion");
        let legacy = hash(&old);
        assert_ne!(legacy, current);
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let entry = store
            .insert_entry(
                &crate::store::Lane::main("conversation"),
                crate::store::EntryKind::User,
                1,
                &json!({"status":"queued","text":"legado","executionBinding":legacy}),
            )
            .unwrap();
        assert!(!entry_matches(&cfg, &entry, &current));
        cfg.name = "Nome novo".into();
        cfg.avatar_color = "red".into();
        assert_eq!(current, stamp(Some("hub"), Some(1), &conversation, &cfg));
        cfg.shared_memory = Some(false);
        assert_ne!(current, stamp(Some("hub"), Some(1), &conversation, &cfg));
    }
}
