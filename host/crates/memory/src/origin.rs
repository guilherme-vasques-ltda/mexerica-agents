//! Procedência de dados, independente do autor da gravação. Nunca concede autoridade.
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    User,
    Conversation,
    External,
    #[default]
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub message_id: String,
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Origin {
    pub schema: u8,
    pub kind: Kind,
    pub sources: Vec<Source>,
    pub external: bool,
}

impl Default for Origin {
    fn default() -> Self {
        Self { schema: 1, kind: Kind::Unknown, sources: Vec::new(), external: true }
    }
}

impl Origin {
    /// Mistura nunca lava marca externa ou desconhecida. Só o Hub constrói as fontes.
    pub fn merge(origins: impl IntoIterator<Item = Self>) -> Self {
        let mut sources = Vec::new();
        let mut external = false;
        let mut unknown = false;
        for origin in origins {
            external |= origin.external || origin.kind == Kind::External;
            unknown |= origin.kind == Kind::Unknown;
            for source in origin.sources {
                if !sources.contains(&source) {
                    sources.push(source);
                }
            }
        }
        let kind = if unknown || sources.is_empty() {
            Kind::Unknown
        } else if external {
            Kind::External
        } else {
            Kind::Conversation
        };
        Self { schema: 1, kind, sources, external: external || unknown }
    }
}

pub fn decode(raw: &str) -> Origin {
    serde_json::from_str(raw).ok().filter(|o: &Origin| o.schema == 1).unwrap_or_default()
}

pub fn set(conn: &Connection, id: i64, origin: &Origin) -> crate::store::Result<()> {
    let raw = serde_json::to_string(origin).unwrap_or_else(|_| "{}".into());
    conn.execute("UPDATE memory_entries SET origin=?2 WHERE id=?1", params![id, raw])?;
    conn.execute(
        "INSERT OR REPLACE INTO donna_memory_versions(entry,rev,text,state,origin,evidence,nature,nature_bot,conflict_text,at)
         SELECT id,rev,text,state,origin,evidence,nature,nature_bot,conflict_text,updated_at FROM memory_entries WHERE id=?1",
        [id],
    )?;
    Ok(())
}

pub fn set_file(conn: &Connection, version: i64, origin: &Origin) -> crate::store::Result<()> {
    conn.execute(
        "UPDATE memory_file_versions SET origin=?2 WHERE id=?1",
        params![version, serde_json::to_string(origin).unwrap_or_else(|_| "{}".into())],
    )?;
    Ok(())
}
