use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Mode {
    #[default]
    Fixed,
    Auto,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Reserve {
    pub account_id: String,
    pub account_revision: u64,
    pub selection: crate::fork::spaces::EngineSelection,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Policy {
    pub mode: Mode,
    pub guide: String,
    pub reserves: Vec<Reserve>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Document<T> {
    pub schema_version: u8,
    pub revision: u64,
    pub value: T,
}
impl<T: Default> Default for Document<T> {
    fn default() -> Self {
        Self { schema_version: 1, revision: 0, value: T::default() }
    }
}

pub(super) fn id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty() && id.len() <= 100 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
        "invalid_field: identificador de roteamento inválido."
    );
    Ok(())
}
pub(super) fn guide(text: &str) -> Result<()> {
    ensure!(
        text.len() <= 8_000
            && text.chars().count() <= 2_000
            && !text.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')),
        "invalid_field: guia deve ter até 2.000 caracteres, sem controles especiais."
    );
    Ok(())
}
pub(super) fn keys(space: &str, bot: &str) -> (String, String) {
    (format!("donna.routing.v1:{space}:bot:{bot}"), format!("donna.routing.v1:{space}:guide"))
}
pub(super) fn load<T: Default + DeserializeOwned>(db: &Connection, key: &str) -> Result<Document<T>> {
    let raw: Option<String> = db.query_row("SELECT v FROM kv WHERE k=?1", [key], |r| r.get(0)).optional()?;
    let doc: Document<T> = raw
        .map(|raw| serde_json::from_str(&raw))
        .transpose()
        .map_err(|_| anyhow!("routing_unavailable: configuração ilegível; dados preservados."))?
        .unwrap_or_default();
    ensure!(doc.schema_version == 1, "routing_unavailable: versão indisponível; dados preservados.");
    Ok(doc)
}
pub(super) fn save<T: Serialize>(db: &Connection, key: &str, mut before: Document<T>, value: T) -> Result<()> {
    before.revision =
        before.revision.checked_add(1).ok_or_else(|| anyhow!("routing_unavailable: revisão esgotada."))?;
    before.value = value;
    db.execute(
        "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
        [key, &serde_json::to_string(&before)?],
    )?;
    Ok(())
}
pub(super) fn same(a: &impl Serialize, b: &impl Serialize) -> Result<bool> {
    Ok(serde_json::to_value(a)? == serde_json::to_value(b)?)
}
