//! Preferência de navegador: configuração do dono, independente de qualquer permissão de tela.
use crate::store::BotConfig;
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BrowserPreference {
    #[default]
    Integrated,
    Computer,
    /// Navegador desligado: o bot fica sem navegador; "Usar o computador" não muda (E2, 10/11/2026;
    /// escopo revisto pelo dono no mesmo dia: Desligado = só navegador).
    Off,
}

/// Guarda comum de todos os bots. Só a marca transacional existente do dono permite mudar a escolha.
/// Escritores internos podem preservar a preferência; criação por bot começa sempre no integrado.
pub(crate) fn check_save(db: &Connection, cfg: &BotConfig) -> Result<()> {
    let raw: Option<String> =
        db.query_row("SELECT config FROM bots WHERE id=?1", [&cfg.id], |r| r.get(0)).optional()?;
    let before = raw.as_deref().map(serde_json::from_str::<serde_json::Value>).transpose()?;
    let previous = before
        .as_ref()
        .and_then(|v| v.get("browserPreference"))
        .map(|v| serde_json::from_value::<BrowserPreference>(v.clone()))
        .transpose()?
        .unwrap_or_default();
    ensure!(
        !cfg.is_group() || cfg.browser_preference == BrowserPreference::Integrated,
        "invalid_field: preferência de navegador é exclusiva de bots agentes"
    );
    ensure!(
        before.is_some() || cfg.created_by.is_none() || cfg.browser_preference == BrowserPreference::Integrated,
        "not_allowed: bots criados por bots começam no navegador integrado"
    );
    if previous != cfg.browser_preference {
        let owner: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM donna_fixa_dono)", [], |r| r.get(0))?;
        ensure!(owner, "not_allowed: só o dono escolhe o navegador padrão do bot");
    }
    Ok(())
}
