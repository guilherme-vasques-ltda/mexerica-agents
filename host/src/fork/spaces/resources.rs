use super::conversations::{Conversation, arg, context, require_revision};
use crate::{api::devices::Caller, hub::Hub};
use anyhow::{Result, anyhow, ensure};
use serde_json::Value;
use std::sync::Arc;

pub(super) async fn call(
    hub: &Arc<Hub>,
    caller: &Caller,
    method: &str,
    c: &Conversation,
    b: &Value,
) -> Result<Option<Value>> {
    if !matches!(
        method,
        "donnaConversationMemory"
            | "donnaConversationUpload"
            | "donnaConversationReadUpload"
            | "donnaConversationReadFile"
    ) {
        return Ok(None);
    }
    let original = hub.store.bot(&c.bot_id)?.ok_or_else(|| anyhow!("bot_not_found"))?.config;
    let cfg = super::resource_config(&hub.store, &original, &c.id)?;
    let value = match method {
        "donnaConversationReadFile" => {
            let entry = hub.store.entry(arg(b, "entryId")?).ok_or_else(|| anyhow!("entry_not_found"))?;
            ensure!(entry.bot_id == c.id, "cross_conversation_reference");
            crate::api::files::read(hub, b).await?
        }
        "donnaConversationMemory" => {
            super::super::engine_setup::require_local(caller)?;
            let name = arg(b, "method")?;
            ensure!(
                [
                    "memory",
                    "saveMemory",
                    "pinMemory",
                    "archiveMemory",
                    "forgetMemory",
                    "clearMemory",
                    "memoryFile",
                    "saveMemoryFile",
                    "deleteMemoryFile",
                    "restoreMemoryFile"
                ]
                .contains(&name),
                "context_memory_method_refused"
            );
            ensure!(c.space_id == super::PERSONAL || b["shared"] != true, "shared_memory_is_personal");
            if !matches!(name, "memory" | "memoryFile") {
                ensure!(!c.closed, "conversation_closed");
                require_revision(c, b)?;
            }
            let mut body = b.clone();
            body["botId"] = c.bot_id.clone().into();
            body["conversationId"] = c.id.clone().into();
            body["spaceId"] = c.space_id.clone().into();
            if crate::chat::memory::WRITES.contains(&name) {
                let original = crate::fork::autoridade::Autoridade::capturar(hub, caller)?;
                crate::chat::memory::suggestion_call(hub, name, &body, original).await?
            } else {
                crate::chat::memory::call(hub, name, &body).await?
            }
        }
        "donnaConversationUpload" => {
            ensure!(!c.closed, "conversation_closed");
            require_revision(c, b)?;
            let (hub, c, b) = (hub.clone(), c.clone(), b.clone());
            tokio::task::spawn_blocking(move || super::uploads::append(&hub.store, &c, &cfg, &b)).await??
        }
        _ => {
            let (hub, c, b) = (hub.clone(), c.clone(), b.clone());
            tokio::task::spawn_blocking(move || super::uploads::read(&hub.store, &c, &cfg, &b)).await??
        }
    };
    let current = hub.store.space_conversation(&c.space_id, &c.id)?;
    ensure!(current.revision == c.revision, "context_changed_during_resource_operation");
    let mut value = value;
    value["context"] = context(c);
    Ok(Some(value))
}
