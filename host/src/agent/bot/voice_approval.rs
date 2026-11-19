//! D98: only an identified, inert native planning tool can be approved by voice.
use super::Adapter;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum VoiceApproval {
    AllowOnce,
    AppOnly,
}

pub(super) fn policy(adapter: Option<Adapter>, tool: &Value) -> VoiceApproval {
    // Do not infer a capability from a title or from ACP's broad `read`/`think` kinds.
    // MCPs (including computers and secrets), shell and older adapters stay in-app.
    if adapter != Some(Adapter::Claude)
        || tool["name"] != "TodoWrite"
        || tool["kind"] != "think"
        || !tool["_meta"].is_null()
        || tool["content"].as_array().is_some_and(|items| !items.is_empty())
        || tool.get("content").is_some_and(|v| !v.is_null() && !v.is_array())
        || tool["locations"].as_array().is_some_and(|items| !items.is_empty())
        || tool.get("locations").is_some_and(|v| !v.is_null() && !v.is_array())
    {
        return VoiceApproval::AppOnly;
    }
    let Some(raw) = tool["rawInput"].as_object() else { return VoiceApproval::AppOnly };
    if raw.len() != 1 {
        return VoiceApproval::AppOnly;
    }
    let Some(todos) = raw.get("todos").and_then(Value::as_array) else { return VoiceApproval::AppOnly };
    if todos.iter().all(|todo| {
        todo.as_object().is_some_and(|fields| {
            fields.keys().all(|k| matches!(k.as_str(), "content" | "status" | "activeForm"))
                && todo["content"].is_string()
                && matches!(todo["status"].as_str(), Some("pending" | "in_progress" | "completed"))
                && fields.get("activeForm").is_none_or(Value::is_string)
        })
    }) {
        VoiceApproval::AllowOnce
    } else {
        VoiceApproval::AppOnly
    }
}

pub(crate) fn allowed_answer(data: &Value, option_id: Option<&str>) -> bool {
    data["status"] == "pending"
        && data["voiceApproval"] == "allow_once"
        && option_id.is_some_and(|id| {
            let Some(options) = data["options"].as_array() else { return false };
            let matches: Vec<_> = options.iter().filter(|o| o["optionId"] == id).collect();
            matches.len() == 1 && matches[0]["kind"] == "allow_once"
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_identified_native_todo_updates_are_allowed() {
        let safe = json!({"name":"TodoWrite", "kind":"think", "rawInput":{"todos":[
            {"content":"Revisar plano", "status":"pending", "activeForm":"Revisando plano"}
        ]}});
        assert_eq!(policy(Some(Adapter::Claude), &safe), VoiceApproval::AllowOnce);
        for adapter in [None, Some(Adapter::Other), Some(Adapter::Codex)] {
            assert_eq!(policy(adapter, &safe), VoiceApproval::AppOnly);
        }
        for tool in [
            json!({}),
            json!({"kind":"read", "title":"Read"}),
            json!({"name":"Bash", "kind":"execute", "rawInput":{"command":"op read item"}}),
            json!({"name":"mcp__computers__read", "kind":"read"}),
            json!({"name":"mcp__payments__pay", "kind":"think"}),
            json!({"name":"mcp__team__create_bot", "kind":"think"}),
            json!({"name":"mcp__1password__read", "kind":"read"}),
        ] {
            assert_eq!(policy(Some(Adapter::Claude), &tool), VoiceApproval::AppOnly);
        }
        for (key, value) in [
            ("name", json!("mcp__computers__TodoWrite")),
            ("kind", json!("delete")),
            ("_meta", json!({"is_mcp_tool_call":true})),
            ("rawInput", json!({"todos":[], "command":"send mail"})),
            ("rawInput", json!({"todos":[{"content":"x", "status":"deleted"}]})),
            ("rawInput", Value::Null),
            ("rawInput", json!({"unknown":[]})),
            ("content", json!([{"type":"terminal"}])),
            ("locations", json!([{"path":"secret"}])),
        ] {
            let mut changed = safe.clone();
            changed[key] = value;
            assert_eq!(policy(Some(Adapter::Claude), &changed), VoiceApproval::AppOnly, "{key}");
        }
    }

    #[test]
    fn answers_require_pending_classified_card_and_exact_once_option() {
        let card = json!({"status":"pending", "voiceApproval":"allow_once", "options":[
            {"optionId":"once", "kind":"allow_once"}, {"optionId":"always", "kind":"allow_always"}
        ]});
        assert!(allowed_answer(&card, Some("once")));
        for option in [None, Some("always"), Some(""), Some("unknown")] {
            assert!(!allowed_answer(&card, option));
        }
        for (key, value) in [
            ("status", json!("answered")),
            ("voiceApproval", Value::Null),
            ("voiceApproval", json!("app_only")),
            (
                "options",
                json!([
                    {"optionId":"once", "kind":"allow_once"}, {"optionId":"once", "kind":"allow_always"}
                ]),
            ),
        ] {
            let mut changed = card.clone();
            changed[key] = value;
            assert!(!allowed_answer(&changed, Some("once")));
        }
    }
}
