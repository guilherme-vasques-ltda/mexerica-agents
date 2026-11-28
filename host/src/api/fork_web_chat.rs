use super::devices::Forbidden;
use crate::LockExt;
use crate::agent::bot::Cmd;
use crate::api::devices::Caller;
use crate::fork::autoridade::Autoridade;
use crate::fork::owner_question::{admit_remote_before_commit, check_queue_capacity};
use crate::fork::spaces::{PERSONAL, validate_dispatch_on};
use crate::hub::Hub;
use crate::store::{BotConfig, Entry, Lane, Store};
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
    if let Some(id) = body.get("retryEntryId") {
        ensure!(id.as_str().is_some_and(valid_nonce) && body["retryEntryRevision"].as_i64().is_some(), Forbidden);
        ensure!(["text", "clientNonce", "replyTo", "pasted"].iter().all(|key| body.get(key).is_none()), Forbidden);
        return Ok(());
    }
    ensure!(body["clientNonce"].as_str().is_some_and(valid_nonce), Forbidden);
    if let Some(id) = body["replyTo"].as_str().filter(|id| !id.is_empty()) {
        let target = body[if method == "send" { "botId" } else { "conversationId" }].as_str();
        ensure!(
            hub.store.entry(id).is_some_and(|e| {
                Some(e.bot_id.as_str()) == target
                    && (!crate::environment::Environment::is_open() || e.session_id == e.bot_id)
                    && visible(Some(&e.kind), &e.data)
            }),
            Forbidden
        );
    }
    Ok(())
}

fn valid_nonce(nonce: &str) -> bool {
    !nonce.is_empty() && nonce.len() <= 128
}

pub(crate) fn retry_turn(hub: &Hub, caller: &Caller, cfg: &BotConfig, chat: &str, body: &Value) -> Result<Entry> {
    ensure!(caller.is_web_chat(), Forbidden);
    let authority = Autoridade::de_requisicao(caller).ok_or(Forbidden)?;
    let id = body["retryEntryId"].as_str().ok_or(Forbidden)?;
    let revision = body["retryEntryRevision"].as_i64().ok_or(Forbidden)?;
    let sender = hub.login_command_sender(&cfg.id)?;
    let (entry, lane) = {
        let _emit = hub.emit_lock.locked();
        let (entry, lane) = admit_remote_before_commit(hub, authority, true, |remember| {
            authority.with_db(hub, None, |tx| {
                let mut entry = Store::entry_on(tx, id)?
                    .filter(|e| {
                        e.bot_id == chat
                            && e.rev == revision
                            && e.data["status"] == "failed"
                            && e.data["webTurnFailed"] == true
                            && visible(Some(&e.kind), &e.data)
                            && e.kind == "user"
                    })
                    .ok_or(Forbidden)?;
                let bot = Store::bot_on(tx, &cfg.id)?.filter(|b| !b.deleted).ok_or(Forbidden)?.config;
                ensure!(!bot.is_group(), Forbidden);
                let space = body["spaceId"].as_str().unwrap_or(PERSONAL);
                let lane = if chat == cfg.id {
                    ensure!(space == bot.space_id && !crate::environment::Environment::is_open(), Forbidden);
                    let lane = crate::fork::sessoes::api::send_lane(tx, chat, body)?;
                    let mut effective = bot;
                    crate::fork::sessoes::admissao::apply_configured(tx, &lane, &mut effective)?;
                    ensure!(!crate::agent::backends::is_antigravity(&effective.backend), Forbidden);
                    lane
                } else {
                    let conversation = Store::space_conversation_on(tx, space, chat)?;
                    ensure!(
                        !conversation.closed
                            && conversation.bot_id == cfg.id
                            && body["expectedRevision"].as_i64() == Some(conversation.revision)
                            && entry.data["spaceContext"]["conversationRevision"] == conversation.revision,
                        Forbidden
                    );
                    validate_dispatch_on(tx, cfg, chat, std::slice::from_ref(&entry.id))?;
                    Lane::main(chat)
                };
                ensure!(
                    entry.session_id == lane.session_id() && entry.data["remoteAdmissionRequired"] == true,
                    Forbidden
                );
                ensure!(entry.data[crate::fork::antigravity::ENTRY_MARKER] != true, Forbidden);
                check_queue_capacity(tx, &lane)?;
                entry.data["status"] = "queued".into();
                let data = entry.data.as_object_mut().ok_or(Forbidden)?;
                data.remove("webTurnFailed");
                data.remove("error");
                let entry = Store::update_entry_on(tx, id, &entry.data, None)?.ok_or(Forbidden)?;
                remember(&entry);
                Ok((entry, lane))
            })
        })?;
        let _ = hub.events.send(json!({"type":"entry","rev":entry.rev,"entry":entry}));
        (entry, lane)
    };
    let text = crate::chat::prompt::of_entry(hub, cfg, &entry.data);
    let enqueued =
        authority.com_saida(hub, None, || sender.send(Cmd::Send { lane, entry_id: entry.id.clone(), text }).is_ok());
    if !matches!(enqueued, Ok(true)) {
        return Ok(hub.fail_web_turn(&entry).unwrap_or(entry));
    }
    Ok(entry)
}

pub(crate) fn receipt(hub: &Hub, chat: &str, session: &str, proof: &Value) -> Result<Vec<Entry>> {
    let nonce = proof["nonce"].as_str().filter(|n| valid_nonce(n)).ok_or(Forbidden)?;
    let text = proof["text"].as_str().ok_or(Forbidden)?.trim();
    Ok(hub
        .store
        .find_by_nonce(chat, nonce)
        .filter(|e| {
            let intent = &e.data["intent"];
            e.session_id == session
                && intent["text"] == text
                && intent["revision"] == proof["revision"]
                && intent["replyTo"] == proof["replyTo"]
                && intent["attachments"].as_array().is_some_and(Vec::is_empty)
        })
        .into_iter()
        .collect())
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
    if out["kind"] == "user" {
        out["data"]["status"] = "sent".into();
        if data["status"] == "failed" && data["webTurnFailed"] == true {
            out["data"]["webTurnFailed"] = true.into();
        }
    }
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
        "sessions" => |v| {
            let actions = if crate::environment::Environment::is_open() { &[][..] } else { &["send"][..] };
            Some(json!({"sessions": rows(&v["sessions"], session)?, "sessionActions": actions}))
        },
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
        let actions = if crate::environment::Environment::is_open() { json!([]) } else { json!(["send"]) };
        assert_eq!(
            rpc("sessions").unwrap()(&json!({"sessions":[row],"sessionActions":["engine","delete"]})),
            Some(json!({"sessions":[expected],"sessionActions":actions}))
        );
        assert_eq!(
            event(&json!({"type":"session","spaceId":"s","session":row})),
            Some(json!({"type":"session","spaceId":"s","session":expected}))
        );
    }
}
