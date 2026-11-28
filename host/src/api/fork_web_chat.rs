use super::devices::Forbidden;
use crate::hub::Hub;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

fn fields(value: &Value, keys: &[&str]) -> Option<Value> {
    let object = value.as_object()?;
    Some(Value::Object(
        keys.iter()
            .filter_map(|key| {
                let value = object.get(*key)?;
                (!value.is_object() && !value.is_array()).then(|| ((*key).to_owned(), value.clone()))
            })
            .collect(),
    ))
}

fn strings(value: &Value) -> Option<Value> {
    Some(Value::Array(value.as_array()?.iter().filter(|v| v.is_string()).cloned().collect()))
}

fn rows(value: &Value, project: Project) -> Option<Value> {
    Some(Value::Array(value.as_array()?.iter().filter_map(project).collect()))
}

type Project = fn(&Value) -> Option<Value>;

fn visible(kind: Option<&str>, data: &Value) -> bool {
    data["text"].is_string()
        && data["from"] != "host"
        && data.get("choice_response").is_none()
        && (kind == Some("user") || (kind == Some("agent") && data["final"] == true))
}

pub(super) fn check_send(hub: &Hub, method: &str, body: &Value) -> Result<()> {
    if !matches!(method, "send" | "donnaConversationSend") {
        return Ok(());
    }
    ensure!(body.get("attachments").is_none_or(|v| v.is_null() || v.as_array().is_some_and(Vec::is_empty)), Forbidden);
    if let Some(id) = body["replyTo"].as_str().filter(|id| !id.is_empty()) {
        let target = body[if method == "send" { "botId" } else { "conversationId" }].as_str();
        ensure!(
            hub.store.entry(id).is_some_and(|e| Some(e.bot_id.as_str()) == target && visible(Some(&e.kind), &e.data)),
            Forbidden
        );
    }
    Ok(())
}

fn entry(value: &Value) -> Option<Value> {
    let data = &value["data"];
    if !visible(value["kind"].as_str(), data) {
        return None;
    }
    let mut out = fields(
        value,
        &[
            "id",
            "seq",
            "botId",
            "spaceId",
            "conversationId",
            "sessionId",
            "rev",
            "kind",
            "turn",
            "createdAt",
            "updatedAt",
        ],
    )?;
    out["data"] = fields(data, &["text", "final", "author", "status", "clientNonce"])?;
    Some(out)
}

fn conversation(value: &Value) -> Option<Value> {
    fields(
        value,
        &["id", "spaceId", "botId", "title", "revision", "primary", "closed", "stateRevision", "readRevision"],
    )
}

fn session(value: &Value) -> Option<Value> {
    fields(
        value,
        &[
            "id",
            "chatId",
            "principal",
            "parentSession",
            "depth",
            "title",
            "state",
            "createdAt",
            "lastUsedAt",
            "revision",
        ],
    )
}

fn groups(value: &Value) -> Option<Value> {
    let mut out = fields(value, &["rev"])?;
    out["groups"] = rows(&value["groups"], |row| {
        let mut group = fields(row, &["id", "name"])?;
        group["bots"] = strings(&row["bots"])?;
        Some(group)
    })?;
    Some(out)
}

fn spaces(value: &Value) -> Option<Value> {
    let mut out = fields(value, &["hubId", "version", "personalSpaceId", "revision"])?;
    out["spaces"] = rows(&value["spaces"], |row| {
        let mut space = fields(row, &["id", "principalBotId", "kind", "name", "position", "revision"])?;
        space["tile"] = Value::Null;
        space["owner"] = Value::Null;
        space["appearanceConfigured"] = false.into();
        space["appearance"] = json!(crate::fork::spaces::Appearance::default());
        Some(space)
    })?;
    Some(out)
}

fn sync_hello(value: &Value) -> Option<Value> {
    let mut out = fields(
        value,
        &["type", "hostId", "computerId", "name", "version", "minApp", "rev", "spaceId", "spacesContract"],
    )?;
    out["spaces"] = spaces(&value["spaces"])?;
    out["conversations"] = rows(&value["conversations"], conversation)?;
    if value["type"] == "hello" {
        out["botGroups"] = groups(&value["botGroups"])?;
    }
    Some(out)
}

fn context(value: &Value) -> Option<Value> {
    fields(value, &["version", "spaceId", "conversationId", "botId", "conversationRevision"])
}

pub(super) fn rpc(method: &str) -> Option<Project> {
    Some(match method {
        "hello" => sync_hello,
        "workspaceContext" => |v| {
            let mut out = fields(v, &["schema", "hubId", "kind", "workspaceId", "userId", "role", "workspaceEpoch"])?;
            out["capabilities"] = json!(["chat"]);
            Some(out)
        },
        "donnaSpaces" => spaces,
        "botGroups" => groups,
        "sessions" => |v| Some(json!({"sessions": rows(&v["sessions"], session)?, "sessionActions": ["send"]})),
        "donnaConversations" => |v| {
            let mut out = fields(v, &["hubId", "spaceId"])?;
            out["conversations"] = rows(&v["conversations"], conversation)?;
            out["principal"] =
                fields(&v["principal"], &["id", "name", "avatarColor", "avatarShape"]).unwrap_or_default();
            Some(out)
        },
        "history" => |v| Some(json!({"entries": rows(&v["entries"], entry)?})),
        "donnaConversationHistory" => |v| {
            let mut out = fields(v, &["hubId", "hasMore"])?;
            out["context"] = context(&v["context"])?;
            out["entries"] = rows(&v["entries"], entry)?;
            Some(out)
        },
        "send" => |v| Some(json!({"entry": entry(&v["entry"])?})),
        "donnaConversationSend" => |v| {
            let mut out = fields(v, &["hubId", "replayed", "queued"])?;
            out["context"] = context(&v["context"])?;
            out["entry"] = entry(&v["entry"])?;
            Some(out)
        },
        _ => return None,
    })
}

pub(super) fn event(value: &Value) -> Option<Value> {
    let mut out = fields(value, &["type", "spaceId", "rev"])?;
    match value["type"].as_str()? {
        "hello" => return sync_hello(value),
        "bot" => {
            let bot = &value["bot"];
            let mut row = fields(
                bot,
                &[
                    "id",
                    "spaceId",
                    "conversationId",
                    "kind",
                    "leader",
                    "name",
                    "description",
                    "avatarColor",
                    "avatarShape",
                    "title",
                    "pinned",
                    "position",
                    "hidden",
                    "createdAt",
                    "rev",
                    "deleted",
                    "status",
                ],
            )?;
            row["members"] = strings(&bot["members"]).unwrap_or_else(|| json!([]));
            out["bot"] = row;
        }
        "spaces" => out["snapshot"] = spaces(&value["snapshot"])?,
        "conversation" => out["conversation"] = conversation(&value["conversation"])?,
        "botGroups" => out["snapshot"] = groups(&value["snapshot"])?,
        "session" => out["session"] = session(&value["session"])?,
        "entry" => out["entry"] = entry(&value["entry"])?,
        "sessionDeleted" => return fields(value, &["type", "spaceId", "rev", "chatId", "sessionId"]),
        "botDeleted" => return fields(value, &["type", "spaceId", "rev", "id"]),
        "resync" => {}
        _ => return None,
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn somente_metodos_com_projecao_estao_no_perfil_web() {
        for (_, methods) in crate::fork::acesso_rpc::TABELA {
            for method in *methods {
                assert_eq!(rpc(method).is_some(), crate::fork::acesso_rpc::WEB_CHAT.contains(method), "{method}");
            }
        }
        assert!(event(&json!({"type":"unknown","data":"privado"})).is_none());
    }

    #[test]
    fn grupos_e_sessoes_so_exportam_campos_do_catalogo_de_chat() {
        let groups = json!({"rev":8,"administration":"privado","groups":[
            {"id":"g","name":"Grupo","bots":["b",{"path":"privado"}],"owner":"privado"}
        ]});
        let expected = json!({"rev":8,"groups":[{"id":"g","name":"Grupo","bots":["b"]}]});
        assert_eq!(rpc("botGroups").unwrap()(&groups), Some(expected.clone()));
        assert_eq!(
            event(&json!({"type":"botGroups","spaceId":"s","snapshot":groups})),
            Some(json!({"type":"botGroups","spaceId":"s","snapshot":expected}))
        );
        let row =
            json!({"id":"session","chatId":"chat","title":"Conversa","engine":{"id":"privado"},"origin":"privado"});
        let expected = json!({"id":"session","chatId":"chat","title":"Conversa"});
        assert_eq!(
            rpc("sessions").unwrap()(&json!({"sessions":[row],"sessionActions":["engine","delete"]})),
            Some(json!({"sessions":[expected],"sessionActions":["send"]}))
        );
        assert_eq!(
            event(&json!({"type":"session","spaceId":"s","session":row})),
            Some(json!({"type":"session","spaceId":"s","session":expected}))
        );
    }
}
