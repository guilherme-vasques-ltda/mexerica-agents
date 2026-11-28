use serde_json::Value;

const BOT_CHAT_FIELDS: &[&str] = &[
    "id",
    "spaceId",
    "conversationId",
    "kind",
    "members",
    "leader",
    "name",
    "description",
    "avatarColor",
    "avatarShape",
    "avatarConfig",
    "mexeriquinha",
    "title",
    "backend",
    "model",
    "pinned",
    "position",
    "hidden",
    "createdAt",
    "rev",
    "deleted",
    "status",
    "activity",
    "activityCode",
    "resumeAt",
    "startedAt",
    "workingChat",
    "pendingQuestions",
    "unread",
    "lastMessage",
    "lastAt",
];
const HELLO_CHAT_FIELDS: &[&str] = &[
    "type",
    "hostId",
    "computerId",
    "rev",
    "spaceId",
    "spacesContract",
    "spaces",
    "conversations",
    "workspace",
    "botGroups",
];

pub(super) fn project(mut event: Value) -> Option<Value> {
    match event["type"].as_str()? {
        "hello" => {
            event.as_object_mut()?.retain(|key, _| HELLO_CHAT_FIELDS.contains(&key.as_str()));
        }
        "bot" => event["bot"].as_object_mut()?.retain(|key, _| BOT_CHAT_FIELDS.contains(&key.as_str())),
        "spaces" | "conversation" | "botGroups" | "botDeleted" | "session" | "sessionDeleted" | "entry" | "resync" => {}
        _ => return None,
    }
    Some(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn eventos_web_omitem_controles_e_configuracao_do_bot() {
        let hello = project(json!({"type":"hello", "rev":1, "screen":{}, "usage":{}, "spaceLifecycleContract":1}));
        assert_eq!(hello, Some(json!({"type":"hello", "rev":1})));
        let bot = project(
            json!({"type":"bot", "bot":{"id":"bot-1", "cwd":"pasta", "command":"comando", "ownerPreferences":"preferências", "screenAccess":[]}}),
        );
        assert_eq!(bot, Some(json!({"type":"bot", "bot":{"id":"bot-1"}})));
        assert!(project(json!({"type":"screen", "screen":{}})).is_none());
        assert!(project(json!({"type":"usoNovo"})).is_none());
        assert!(project(json!({"type":"entry", "entry":{}})).is_some());
    }
}
