use crate::fork::engine_setup::process::Rpc;
#[cfg(unix)]
use crate::store::Store;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const MAX_HISTORY_BYTES: usize = 64 * 1024;
const PAGE_SIZE: usize = 16;
const MAX_PAGES: usize = 16;

#[derive(Deserialize, Serialize)]
pub(super) struct Session {
    pub(super) thread: String,
    fingerprint: String,
    history: Vec<Message>,
}

#[derive(Deserialize, Serialize)]
pub(super) struct Message {
    assistant: bool,
    text: String,
}

pub(super) struct Catalog {
    pub(super) tools: Value,
    fingerprint: String,
    previous: Option<Session>,
    legacy: Option<String>,
}

impl Catalog {
    #[cfg(unix)]
    pub(super) fn key(key: &str) -> String {
        format!("{key}.hub-v3")
    }

    #[cfg(unix)]
    pub(super) fn read(store: &Store, key: &str, tools: Value) -> Result<Self> {
        let previous = store.kv_read(&Self::key(key))?.map(|v| serde_json::from_str(&v)).transpose()?;
        let mut legacy = None;
        if previous.is_none() {
            legacy = store.kv_read(&format!("{key}.hub-v2"))?;
            if legacy.is_none() {
                legacy = store.kv_read(key)?;
            }
        }
        let fingerprint = crate::remote::crypto::b64(&crate::remote::crypto::sha256(&[&serde_json::to_vec(&tools)?]));
        Ok(Self { tools, fingerprint, previous, legacy })
    }

    pub(super) fn offers(&self, name: &str) -> bool {
        self.tools.as_array().is_some_and(|tools| tools.iter().any(|tool| tool["name"] == name))
    }

    pub(super) fn resume(&self) -> Option<&str> {
        self.previous.as_ref().filter(|s| s.fingerprint == self.fingerprint).map(|s| s.thread.as_str())
    }

    pub(super) fn history_source(&self) -> Option<&str> {
        self.previous.as_ref().map(|s| s.thread.as_str()).or(self.legacy.as_deref())
    }

    pub(super) async fn history(&self, rpc: &mut Rpc) -> Result<Vec<Message>> {
        let Some(thread) = self.history_source() else { return Ok(Vec::new()) };
        ensure!(super::valid_id(thread), "open_chat_failed");
        let mut history = Vec::new();
        let mut used = 0;
        let mut cursor = Value::Null;
        for page in 0..MAX_PAGES {
            let result = rpc
                .request(
                    10,
                    "thread/items/list",
                    json!({
                        "threadId": thread, "limit": PAGE_SIZE, "sortDirection": "desc", "cursor": cursor
                    }),
                )
                .await?;
            if page == 0 && result.as_ref().is_err_and(|e| e["code"] == -32601) {
                let read = rpc.call(11, "thread/read", json!({"threadId": thread, "includeTurns": true})).await?;
                ensure!(read["thread"]["id"] == thread, "open_chat_failed");
                for turn in read["thread"]["turns"].as_array().context("open_chat_failed")?.iter().rev() {
                    for item in turn["items"].as_array().context("open_chat_failed")?.iter().rev() {
                        if !push(&mut history, &mut used, Message::from_item(item)) {
                            break;
                        }
                    }
                }
                break;
            }
            let result = result.map_err(|_| anyhow::anyhow!("open_chat_failed"))?;
            let mut room = true;
            for item in result["data"].as_array().context("open_chat_failed")? {
                room = push(&mut history, &mut used, Message::from_item(&item["item"]));
                if !room {
                    break;
                }
            }
            if !room || result["nextCursor"].is_null() {
                break;
            }
            ensure!(result["nextCursor"].is_string() && result["nextCursor"] != cursor, "open_chat_failed");
            cursor = result["nextCursor"].clone();
        }
        if let Some(previous) = &self.previous {
            for message in previous.history.iter().rev() {
                if !push(
                    &mut history,
                    &mut used,
                    Some(Message { assistant: message.assistant, text: message.text.clone() }),
                ) {
                    break;
                }
            }
        }
        history.reverse();
        Ok(history)
    }

    #[cfg(unix)]
    pub(super) fn save(&self, store: &Store, bot: &str, key: &str, thread: &str, history: Vec<Message>) -> Result<()> {
        let state = Session { thread: thread.into(), fingerprint: self.fingerprint.clone(), history };
        ensure!(store.kv_set_if_bot(bot, key, &serde_json::to_string(&state)?)?, "open_chat_cancelled");
        Ok(())
    }
}

impl Message {
    fn from_item(item: &Value) -> Option<Self> {
        let assistant = match item["type"].as_str()? {
            "userMessage" => false,
            "agentMessage" => true,
            _ => return None,
        };
        let text = if assistant {
            item["text"].as_str()?.to_owned()
        } else {
            item["content"]
                .as_array()?
                .iter()
                .filter_map(|item| (item["type"] == "text").then(|| item["text"].as_str()).flatten())
                .collect::<Vec<_>>()
                .join("\n")
        };
        (!text.is_empty()).then_some(Self { assistant, text })
    }

    pub(super) fn item(&self) -> Value {
        let (role, kind) = if self.assistant { ("assistant", "output_text") } else { ("user", "input_text") };
        json!({"type": "message", "role": role, "content": [{"type": kind, "text": self.text}]})
    }
}

fn push(history: &mut Vec<Message>, used: &mut usize, message: Option<Message>) -> bool {
    if let Some(mut message) = message {
        let remaining = MAX_HISTORY_BYTES.saturating_sub(*used + 32);
        if remaining == 0 {
            return false;
        }
        if message.text.len() > remaining {
            let mut start = message.text.len() - remaining;
            while !message.text.is_char_boundary(start) {
                start += 1;
            }
            if start == message.text.len() {
                return false;
            }
            message.text.drain(..start);
        }
        *used += message.text.len() + 32;
        history.push(message);
    }
    true
}
