//! Onde a sessão roda (D235). SQL NULL = local, o computador do bot. Sessões só guarda e devolve; nada no fio no 2a.
use anyhow::{Result, anyhow, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Place {
    Computer,
    Cloud,
}

/// `id` é o identificador opaco do cadastro de lugares da D235, não o id de computador da D83.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunsOn {
    pub kind: Place,
    pub id: String,
}

/// O texto da coluna (`None` = local).
pub(crate) fn column(runs_on: Option<&RunsOn>) -> Result<Option<String>> {
    runs_on
        .map(|r| {
            ensure!(!r.id.trim().is_empty(), "invalid_runs_on");
            Ok(serde_json::to_string(r)?)
        })
        .transpose()
}

/// Lê a coluna. Chave extra, JSON ilegível, `kind` desconhecido ou `id` vazio falham, nunca viram local.
pub(crate) fn parse(raw: Option<&str>) -> Result<Option<RunsOn>> {
    let Some(raw) = raw else { return Ok(None) };
    let value: Value = serde_json::from_str(raw).map_err(|_| anyhow!("invalid_runs_on"))?;
    let runs_on: RunsOn = serde_json::from_value(value.clone()).map_err(|_| anyhow!("invalid_runs_on"))?;
    ensure!(serde_json::to_value(&runs_on)? == value && !runs_on.id.trim().is_empty(), "invalid_runs_on");
    Ok(Some(runs_on))
}
