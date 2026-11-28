use super::{PERSONAL, Snapshot};
use crate::{LockExt, hub::Hub, store::BotConfig};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

pub fn validate_bot(c: &Connection, cfg: &BotConfig) -> Result<()> {
    let raw: String = c.query_row("SELECT v FROM kv WHERE k='donna.spaces.v1'", [], |r| r.get(0))?;
    let state: Snapshot = serde_json::from_str(&raw)?;
    ensure!(state.spaces.iter().any(|s| s.id == cfg.space_id), "space_not_found");
    let before: Option<String> = c
        .query_row(
            "SELECT COALESCE(json_extract(config,'$.spaceId'),?2) FROM bots WHERE id=?1",
            [&cfg.id, PERSONAL],
            |r| r.get(0),
        )
        .optional()?;
    ensure!(before.is_none_or(|space| space == cfg.space_id), "space_transfer_requires_explicit_contract");
    for member in &cfg.members {
        let space: Option<String> = c
            .query_row(
                "SELECT COALESCE(json_extract(config,'$.spaceId'),?2) FROM bots WHERE id=?1 AND deleted=0",
                [member, PERSONAL],
                |r| r.get(0),
            )
            .optional()?;
        ensure!(space.is_none_or(|space| space == cfg.space_id), "cross_space_member");
    }
    if cfg.space_id != PERSONAL {
        ensure!(cfg.shared_memory != Some(true) && !cfg.manage_team, "cross_space_inheritance_refused");
    }
    Ok(())
}

pub fn chat_space(hub: &Hub, id: &str) -> Result<String> {
    hub.store
        .db
        .locked()
        .query_row("SELECT space_id FROM donna_space_conversations WHERE id=?1", [id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| anyhow!("chat_not_found"))
}

pub fn guard(hub: &Hub, method: &str, body: &Value) -> Result<()> {
    let contextual = method == crate::fork::browser::METHOD
        || crate::fork::browser::local::METHODS.contains(&method)
        || method.starts_with("donnaSpace")
        || crate::fork::roteamento::api::METHODS.contains(&method)
        || method.starts_with("donnaConversation")
        || method == "donnaOwnerPresentation"
        || crate::fork::routine_creation::handles(method)
        || method == crate::fork::control_tower::METHOD
        || super::super::native_catalog::methods(method)
        || (method == "createBot" && body.get("spaceId").is_some());
    if contextual || body.get("hubId").is_some() {
        ensure!(body["hubId"].as_str() == Some(hub.host_id.as_str()), "hub_context_mismatch");
    }
    if crate::fork::mail_manager::api::METHODS.contains(&method) {
        if !body.get("spaceId").is_none_or(|v| v.is_null() || v.as_str() == Some(PERSONAL)) {
            tracing::warn!(reason = "invalid_space", "mail_manager: operação negada");
            return Err(anyhow!("invalid_space"));
        }
        return Ok(());
    }
    let space = match body.get("spaceId") {
        Some(v) => v.as_str().ok_or_else(|| anyhow!("invalid_space"))?,
        None => PERSONAL,
    };
    hub.store.require_space(space)?;
    // O Store de grupos valida bot e Space na transação e retorna o código
    // estável `bot_missing`; a guarda genérica anteciparia outra resposta.
    if method == "botGroupsEdit" {
        return Ok(());
    }
    // Grupos não executam motor nem aceitam seleção de conta.
    if method == "createBot" && space != PERSONAL && body["kind"] != "group" {
        ensure!(
            body["forkEditor"]["engineAccountId"].as_str().is_some_and(|id| !id.trim().is_empty()),
            "space_account_selection_required"
        );
    }
    // A lista aceita botId vazio para o Space e valida o vínculo dentro da transação.
    if crate::fork::nunca_fazer::METHODS.contains(&method) {
        return Ok(());
    }
    // O campo geral não tem Space; o do Space é validado na própria transação.
    if crate::fork::instrucao_geral::METHODS.contains(&method) {
        return Ok(());
    }
    // MX-264: o lote de extensões é intenção do dono para o Hub inteiro, sem
    // contexto de Space; cada bot tem existência/espaço/regra reconferidos na
    // prévia e de novo dentro da transação (`extensoes_em_todos`), e `spaceId`
    // no corpo é recusado pelo `deny_unknown_fields` do pedido.
    if crate::fork::extensoes_em_todos::METHODS.contains(&method) {
        return Ok(());
    }
    // RPCs contextuais que conferem a conversa, a entrada e o executor por conta própria.
    if method.starts_with("donnaConversation")
        || method == "donnaConversations"
        || method == "donnaRoutingReceipt"
        || matches!(method, "donnaUsageWaitCancel" | "donnaUsageWaitResume")
    {
        return Ok(());
    }
    let conversation = body
        .get("conversationId")
        .map(|v| hub.store.space_conversation(space, v.as_str().ok_or_else(|| anyhow!("invalid_conversation"))?))
        .transpose()?;
    if let Some(c) = &conversation {
        ensure!(
            ["botId", "peerId", "chatId", "entryId", "replyTo"].iter().any(|k| body[*k].is_string())
                || (method == "updateBot" && body["id"].is_string()),
            "conversation_selector_unsupported"
        );
        // Só estes consumidores legados usam o ID da conversa (ou o dono da entrada)
        // como destino. Os demais exigem a principal ou a RPC contextual própria.
        ensure!(
            c.id == c.bot_id
                || matches!(
                    method,
                    "history"
                        | "readFile"
                        | "react"
                        | "respondPermission"
                        | "stop"
                        | "releaseConversation"
                        | "newSession"
                        | "markRead"
                        | "answerMemory"
                        | "undoMemory"
                ),
            "use_contextual_conversation_api"
        );
    }
    for key in ["botId", "peerId", "chatId"] {
        if let Some(id) = body[key].as_str() {
            check_bot(hub, space, id)?;
            ensure!(conversation.as_ref().is_none_or(|c| c.id == id), "cross_conversation_reference");
            if let Ok(c) = hub.store.space_conversation(space, id)
                && c.id != c.bot_id
            {
                ensure!(
                    conversation.as_ref().is_some_and(|current| current.id == c.id),
                    "conversation_context_required"
                );
            }
        }
    }
    if method == "updateBot"
        && let Some(id) = body["id"].as_str()
    {
        check_bot(hub, space, id)?;
        ensure!(conversation.as_ref().is_none_or(|c| c.id == id), "cross_conversation_reference");
    }
    for key in ["entryId", "replyTo"] {
        if let Some(id) = body[key].as_str().filter(|id| !id.is_empty()) {
            let entry = hub.store.entry(id).ok_or_else(|| anyhow!("entry_not_found"))?;
            ensure!(chat_space(hub, &entry.bot_id)? == space, "cross_space_reference");
            let owner = hub.store.space_conversation(space, &entry.bot_id)?;
            ensure!(conversation.as_ref().is_none_or(|c| c.id == entry.bot_id), "cross_conversation_reference");
            ensure!(owner.id == owner.bot_id || conversation.is_some(), "conversation_context_required");
        }
    }
    for key in ["members", "botIds"] {
        if let Some(ids) = body[key].as_array() {
            for id in ids.iter().filter_map(Value::as_str) {
                check_bot(hub, space, id)?;
            }
        }
    }
    if method == "reorderBots"
        && let Some(ids) = body["ids"].as_array()
    {
        for id in ids.iter().filter_map(Value::as_str) {
            check_bot(hub, space, id)?;
        }
    }
    if let Some(patch) = body.get("patch") {
        if let Some(id) = patch["spaceId"].as_str() {
            ensure!(id == space, "cross_space_reference");
        }
        if let Some(members) = patch["members"].as_array() {
            for id in members.iter().filter_map(Value::as_str) {
                check_bot(hub, space, id)?;
            }
        }
    }
    // O namespace de memória compartilhada legado pertence exclusivamente a Pessoal.
    if space != PERSONAL {
        ensure!(body["shared"] != true && body["arguments"]["shared"] != true, "shared_memory_is_personal");
        ensure!(
            !matches!(method, "teamCall" | "manageCall" | "chatCall" | "setTeamSettings" | "undoTeamChange"),
            "space_tool_not_bound"
        );
    }
    // MCP recebido de uma sessão de outro contexto não pode voltar ao cfg.id pessoal por omissão.
    if method.ends_with("Call")
        && let Some(bot) = body["botId"].as_str()
    {
        if let Some(lane) = hub.runtime(bot).lane
            && lane.chat != bot
        {
            ensure!(chat_space(hub, &lane.chat)? == space, "cross_space_runtime");
        }
        if let Some(args) = body["arguments"].as_object() {
            for key in ["botId", "peerId", "targetBotId"] {
                if let Some(id) = args.get(key).and_then(Value::as_str) {
                    check_bot(hub, space, id)?;
                }
            }
        }
    }
    Ok(())
}
fn check_bot(hub: &Hub, space: &str, id: &str) -> Result<()> {
    if let Some(bot) = hub.store.bot(id)? {
        ensure!(bot.config.space_id == space, "cross_space_reference");
        ensure!(!bot.deleted, "bot_deleted: este bot foi apagado.");
        return Ok(());
    }
    hub.store.space_conversation(space, id)?;
    Ok(())
}
