//! Projeção de mensagem registrada. Texto histórico nunca representa um pedido atual.
use crate::store::Entry;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

pub(super) const SNIPPET: usize = 1_500;
pub(super) const MAX_TEXT: usize = 8_000;
pub(super) const MAX_RESPONSE: usize = 32_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Author {
    Human,
    Agent,
    Host,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Origin {
    AppReported,
    External,
    Generated,
    Unknown,
}

#[derive(Clone)]
pub(super) struct Projection {
    pub entry: Entry,
    pub text: String,
    pub author: Author,
    pub origin: Origin,
    pub redacted: bool,
    pub hash: String,
}

pub(super) fn hash(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().fold(String::with_capacity(64), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

impl Projection {
    pub fn new(entry: Entry) -> Self {
        let raw = entry.data["text"].as_str().unwrap_or_default();
        let text = codync_mem::redact::redact(raw);
        let redacted = text != raw;
        let author = if entry.kind == "agent" {
            Author::Agent
        } else if entry.data["from"] == "host" {
            Author::Host
        } else if entry.is_user_authored() {
            Author::Human
        } else {
            Author::Unknown
        };
        let origin = origin(&entry, author);
        // Nunca publicar um digest do segredo cru, nem do JSON interno da entrada.
        let hash = hash(
            &json!(["original_redacted.v1", text, author, origin, entry.created_at, entry.data["truncated"] == true])
                .to_string(),
        );
        Self { entry, text, author, origin, redacted, hash }
    }

    pub fn message(&self, source_ref: Option<String>, offset: usize, limit: usize) -> Value {
        let total = self.text.chars().count();
        let text: String = self.text.chars().skip(offset).take(limit).collect();
        let end = offset + text.chars().count();
        let truncated = offset > 0 || end < total;
        let date = chrono::DateTime::from_timestamp_millis(self.entry.created_at)
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        let mut result = json!({
            "id": self.entry.id, "date": date, "createdAt": self.entry.created_at,
            "from": if self.entry.kind == "user" { "user" } else { "you" },
            "text": text, "entryRevision": self.entry.rev, "authorKind": self.author,
            "originKind": self.origin, "botId": self.entry.bot_id,
            "conversationKey": conversation(&self.entry.bot_id),
            "representation": "original", "projection": "original_redacted",
            "truncated": truncated, "redacted": self.redacted,
            "coverage": {"scope": "stored_text_only", "unit": "unicode_scalar", "start": offset,
                "end": end, "totalChars": total, "complete": !truncated,
                "storageTruncated": self.entry.data["truncated"] == true},
        });
        if let Some(reference) = source_ref {
            result["sourceRef"] = reference.into();
        }
        if end < total {
            result["nextOffset"] = end.into();
        }
        result
    }
}

pub(super) fn conversation(bot: &str) -> String {
    format!("main:{bot}")
}

fn origin(entry: &Entry, author: Author) -> Origin {
    if matches!(author, Author::Agent | Author::Host) {
        return Origin::Generated;
    }
    let data = &entry.data;
    let origin = &data["contentOrigin"];
    if ["attachments", "pasted"].iter().any(|k| data[*k].as_array().is_some_and(|a| !a.is_empty()))
        || data["replyTo"].is_object()
        || origin["attachments"].as_array().is_some_and(|a| !a.is_empty())
        || origin["reply"].is_object()
    {
        return Origin::External;
    }
    let Some(parts) = origin["segments"].as_array() else {
        return Origin::Unknown;
    };
    if parts.iter().any(|p| matches!(p["kind"].as_str(), Some("pasted" | "quote" | "attachment" | "generated"))) {
        return Origin::External;
    }
    // Legado, metadado insuficiente ou uma alegação sem cobertura completa ficam desconhecidos.
    let text = data["text"].as_str().unwrap_or_default();
    let mut boundaries = std::collections::HashSet::from([0]);
    let mut length = 0;
    for c in text.chars() {
        length += c.len_utf16();
        boundaries.insert(length);
    }
    let mut end = 0;
    if author != Author::Human || origin["schema"] != 1 || origin["status"] != "appReported" || parts.is_empty() {
        return Origin::Unknown;
    }
    for part in parts {
        let Some(start) = part["start"].as_u64().and_then(|n| usize::try_from(n).ok()) else {
            return Origin::Unknown;
        };
        let Some(next) = part["end"].as_u64().and_then(|n| usize::try_from(n).ok()) else {
            return Origin::Unknown;
        };
        if start != end
            || next <= start
            || !boundaries.contains(&next)
            || !matches!(part["kind"].as_str(), Some("typed" | "voice"))
        {
            return Origin::Unknown;
        }
        end = next;
    }
    if end == length { Origin::AppReported } else { Origin::Unknown }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct OpenArgs {
    pub source_ref: String,
    #[serde(default = "two")]
    pub before: usize,
    #[serde(default = "two")]
    pub after: usize,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "max_text")]
    pub limit_chars: usize,
}
const fn two() -> usize {
    2
}
const fn max_text() -> usize {
    MAX_TEXT
}
