use crate::store::BotConfig;
use anyhow::{Result, bail};
use serde_json::{Map, Value};

pub const EDITABLE_FIELDS: [&str; 11] = [
    "name",
    "description",
    "avatarConfig",
    "mexeriquinha",
    "title",
    "backend",
    "model",
    "modelOptions",
    "fastMode",
    "browserPreference",
    "forkEditor",
];

/// Um único patch validado antes de qualquer persistência; ausência não equivale a null.
pub fn patch(value: &Value, before: &BotConfig) -> Result<Map<String, Value>> {
    let Some(fields) = value.as_object() else { bail!("invalid_field: patch deve ser um objeto.") };
    crate::fork_avatars::validate_patch(value).map_err(|_| anyhow::anyhow!("invalid_field: avatar inválido."))?;
    let mut patch = Map::new();
    for (key, value) in fields {
        if !EDITABLE_FIELDS.contains(&key.as_str()) {
            bail!("invalid_field: {key} não é editável neste perfil.");
        }
        let checked = match key.as_str() {
            "name" => {
                let name = value.as_str().ok_or_else(|| anyhow::anyhow!("invalid_field: nome deve ser texto."))?;
                if name.trim() == before.name.trim() {
                    continue;
                }
                Value::String(checked_name(name)?.to_owned())
            }
            "description" => {
                let text =
                    value.as_str().ok_or_else(|| anyhow::anyhow!("invalid_field: instruções devem ser texto."))?;
                if text.trim() == before.description.trim() {
                    continue;
                }
                crate::chat::profile::checked_instructions(text)
                    .map_err(|_| anyhow::anyhow!("invalid_field: instruções excedem 4.000 caracteres."))?;
                Value::String(text.trim().to_owned())
            }
            "mexeriquinha" => value.clone(),
            "avatarConfig" => {
                crate::fork_avatars::validate_patch(&serde_json::json!({"avatarConfig":value}))
                    .map_err(|_| anyhow::anyhow!("invalid_field: avatar inválido."))?;
                value.clone()
            }
            "title" => {
                let text = value.as_str().ok_or_else(|| anyhow::anyhow!("invalid_field: função deve ser texto."))?;
                let title =
                    crate::fork::funcao::limpa(text).map_err(|error| anyhow::anyhow!("invalid_field: {error}"))?;
                if title == before.title {
                    continue;
                }
                Value::String(title)
            }
            "backend" => {
                anyhow::ensure!(
                    value.as_str().is_some_and(crate::agent::backends::is_known),
                    "invalid_field: motor não reconhecido por este Hub."
                );
                value.clone()
            }
            "model" => {
                match value {
                    Value::Null => checked_model(None)?,
                    Value::String(model) => checked_model(Some(model))?,
                    _ => bail!("invalid_field: modelo inválido."),
                }
                value.clone()
            }
            "modelOptions" => {
                let options: std::collections::BTreeMap<String, String> = serde_json::from_value(value.clone())
                    .map_err(|_| anyhow::anyhow!("invalid_field: opções inválidas."))?;
                checked_model_options(&options)?;
                value.clone()
            }
            "browserPreference" => {
                let _: crate::fork::browser::BrowserPreference = serde_json::from_value(value.clone())
                    .map_err(|_| anyhow::anyhow!("invalid_field: preferência de navegador inválida."))?;
                value.clone()
            }
            "fastMode" => {
                anyhow::ensure!(value.is_boolean(), "invalid_field: fastMode inválido.");
                value.clone()
            }
            "forkEditor" => {
                let _: crate::fork::engine_setup::BotEngineSelection = serde_json::from_value(value.clone())
                    .map_err(|_| anyhow::anyhow!("invalid_field: conta inválida."))?;
                value.clone()
            }
            _ => unreachable!("allowlist conferida acima"),
        };
        patch.insert(key.clone(), checked);
    }
    Ok(patch)
}

/// Regra do modelo do editor, compartilhada com a troca por conversa sem conta gerenciada (D372). Pura.
pub(crate) fn checked_model(model: Option<&str>) -> Result<()> {
    anyhow::ensure!(model.is_none_or(|v| !v.is_empty() && v.len() <= 100), "invalid_field: modelo inválido.");
    Ok(())
}

/// Regra das opções de modelo do editor, compartilhada com a troca por conversa sem conta gerenciada (D372). Pura.
pub(crate) fn checked_model_options(options: &std::collections::BTreeMap<String, String>) -> Result<()> {
    anyhow::ensure!(
        options.len() <= 8 && options.iter().all(|(k, v)| k.len() <= 60 && v.len() <= 60),
        "invalid_field: opções inválidas."
    );
    Ok(())
}

/// Nome novo, compartilhado por editor, gestão, nomeador e futuras specs 14/16.
pub(crate) fn checked_name(name: &str) -> Result<&str> {
    let name = crate::chat::profile::checked_name(name)
        .map_err(|_| anyhow::anyhow!("invalid_field: nome deve ter entre 1 e 40 caracteres."))?;
    if ["user", "usuário", "usuario", "você", "voce", "todos", "all", "everyone"]
        .contains(&name.to_lowercase().as_str())
    {
        bail!("invalid_field: nome reservado.");
    }
    Ok(name)
}
