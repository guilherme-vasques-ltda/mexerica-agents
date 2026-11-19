//! Envelopes limitados, sem segredo e sem autoridade. Todo localizador é revalidado no banco.
use super::modelo::{Projection, conversation};
use super::{Fault, Result};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

const MAX_ENVELOPE: usize = 8_192;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Source {
    version: u8,
    pub namespace: String,
    pub bot: String,
    conversation: String,
    pub entry: String,
    pub revision: i64,
    pub projection: String,
}

impl Source {
    pub fn new(namespace: &str, row: &Projection) -> Self {
        Self {
            version: 1,
            namespace: namespace.into(),
            bot: row.entry.bot_id.clone(),
            conversation: conversation(&row.entry.bot_id),
            entry: row.entry.id.clone(),
            revision: row.entry.rev,
            projection: row.hash.clone(),
        }
    }
    pub fn encode(&self) -> Result<String> {
        encode("hs1.", self)
    }
    pub fn decode(value: &str) -> Result<Self> {
        let source: Self = decode("hs1.", value).map_err(|_| Fault::SourceUnavailable)?;
        if source.version != 1
            || source.revision <= 0
            || source.entry.len() > 128
            || source.entry.is_empty()
            || source.bot.len() > 128
            || source.bot.is_empty()
            || !digest(&source.namespace)
            || !digest(&source.projection)
            || source.conversation != conversation(&source.bot)
        {
            return Err(Fault::SourceUnavailable.into());
        }
        Ok(source)
    }
    pub fn scope(&self, namespace: &str, bot: &str) -> Result<()> {
        if self.namespace != namespace || self.bot != bot || self.conversation != conversation(bot) {
            return Err(Fault::SourceUnavailable.into());
        }
        Ok(())
    }
    pub fn matches(&self, row: &Projection) -> Result<()> {
        if self.entry != row.entry.id || self.revision != row.entry.rev || self.projection != row.hash {
            return Err(Fault::StaleReference.into());
        }
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Cursor {
    pub version: u8,
    pub boundary: Source,
    pub snapshot_max_seq: i64,
    pub terms: Vec<String>,
}
impl Cursor {
    pub fn encode(&self) -> Result<String> {
        encode("hc1.", self)
    }
    pub fn decode(value: &str) -> Result<Self> {
        let cursor: Self = decode("hc1.", value).map_err(|_| Fault::InvalidCursor)?;
        // Reusar a validação fechada da referência, inclusive dentro do cursor.
        Source::decode(&cursor.boundary.encode()?).map_err(|_| Fault::InvalidCursor)?;
        if cursor.version != 1
            || cursor.snapshot_max_seq < 1
            || cursor.terms.len() > 8
            || cursor.terms.iter().any(|s| s.is_empty() || s.len() > 1024 || s != &s.to_lowercase())
        {
            return Err(Fault::InvalidCursor.into());
        }
        Ok(cursor)
    }
}

fn digest(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn encode(prefix: &str, value: &impl Serialize) -> Result<String> {
    let value = format!("{prefix}{}", URL_SAFE_NO_PAD.encode(serde_json::to_vec(value)?));
    if value.len() > MAX_ENVELOPE {
        return Err(Fault::InvalidArguments.into());
    }
    Ok(value)
}
fn decode<T: DeserializeOwned>(prefix: &str, value: &str) -> Result<T> {
    if value.len() > MAX_ENVELOPE {
        return Err(Fault::InvalidCursor.into());
    }
    let bytes = URL_SAFE_NO_PAD.decode(value.strip_prefix(prefix).ok_or(Fault::InvalidCursor)?)?;
    Ok(serde_json::from_slice(&bytes)?)
}
