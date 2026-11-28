use super::{PERSONAL, chat_space};
use crate::{LockExt, hub::Hub};
use serde_json::Value;

/// Falta de classificação nunca é autorização implícita para outro contexto.
pub fn filter_event(hub: &Hub, space: &str, mut event: Value) -> Option<Value> {
    let kind = event["type"].as_str()?;
    if kind == "sessionDeleted" {
        return (event["spaceId"] == space).then_some(event);
    }
    if kind == "botDeleted" {
        use crate::LockExt;
        let id = event["id"].as_str()?;
        let affected = hub
            .store
            .db
            .locked()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM donna_space_conversations WHERE space_id=?1 AND bot_id=?2 AND id<>bot_id)",
                rusqlite::params![space, id],
                |r| r.get::<_, bool>(0),
            )
            .unwrap_or(false);
        if affected {
            return Some(serde_json::json!({"type":"resync","spaceId":space}));
        }
    }
    if kind == "screen" {
        event["screen"] = crate::fork::runtime_audiencia::screen::state(hub, space);
        event["spaceId"] = space.into();
        return Some(event);
    }
    if matches!(kind, "spaces" | "resync") {
        return Some(event);
    }
    if kind == "botGroups" {
        return (event["spaceId"] == space).then_some(event);
    }
    if matches!(kind, "times" | "memoryProfile") && event["spaceId"] == space {
        return Some(event);
    }
    let chat = match kind {
        "bot" => event["bot"]["id"].as_str(),
        "botDeleted" => event["id"].as_str(),
        "entry" => event["entry"]["botId"].as_str(),
        "conversation" => event["conversation"]["id"].as_str(),
        "session" => event["session"]["chatId"].as_str(),
        // Caixa do WhatsApp do bot: só no Space do bot (o dono é conferido em api/events.rs).
        "whatsappChanged" => event["botId"].as_str(),
        "chatCleared" => event["chatId"].as_str(),
        _ => None,
    };
    if let Some(chat) = chat.map(str::to_owned) {
        // Tombstone de bot excluído: a conversa já saiu do banco na mesma Tx, então
        // o espaço vem do `spaceId` que ficou gravado no config do próprio bot.
        let belongs = if kind == "bot" && event["bot"]["deleted"] == true {
            hub.store
                .db
                .locked()
                .query_row("SELECT json_extract(config,'$.spaceId') FROM bots WHERE id=?1", [&chat], |r| {
                    r.get::<_, Option<String>>(0)
                })
                .ok()
                .flatten()
                .as_deref()
                == Some(space)
        } else {
            chat_space(hub, &chat).ok().as_deref() == Some(space)
        };
        if !belongs {
            return None;
        }
        if kind == "bot" {
            crate::fork::runtime_audiencia::revalidate_event(hub, space, &mut event["bot"]);
        }
        event["spaceId"] = space.into();
        if event["entry"].is_object() {
            event["entry"]["spaceId"] = space.into();
            event["entry"]["conversationId"] = chat.into();
        }
        return Some(event);
    }
    (space == PERSONAL && matches!(kind, "usage" | "accessRequests" | "cloud" | "functionModelsChanged"))
        .then_some(event)
}
