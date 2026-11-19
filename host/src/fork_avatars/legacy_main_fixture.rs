//! Fixture de compatibilidade copiada da main e2dd0f54 (fork próprio, Apache-2.0).
//! `BotConfig` completo e schema congelados em host/tests/fixtures; somente a dependência
//! `AvatarConfig` do modelo foi redirecionada ao validador antigo abaixo.
#![allow(dead_code)]
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::sync::LazyLock;
#[path = "../../tests/fixtures/bot-model-main-e2dd0f54.rs"]
pub mod model;
static SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../tests/fixtures/avatar-config-main-e2dd0f54.schema.json")).unwrap()
});
/// O valor só pode ser construído após validar os campos do schema distribuído.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct AvatarConfig(Value);

impl<'de> Deserialize<'de> for AvatarConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        validate(&value).map_err(serde::de::Error::custom)?;
        Ok(Self(value))
    }
}

/// O schema atual contém apenas objeto fechado, strings, enums e a cor hexadecimal.
/// Lemos enums/campos diretamente dele para não manter duas listas divergentes.
fn validate(value: &Value) -> Result<()> {
    let object = value.as_object().context("avatarConfig deve ser um objeto")?;
    let properties = SCHEMA["properties"].as_object().context("schema sem propriedades")?;
    if object.len() != properties.len() || object.keys().any(|key| !properties.contains_key(key)) {
        bail!("avatarConfig deve conter somente os campos exigidos pelo schema");
    }
    for (key, rule) in properties {
        let field = object.get(key).with_context(|| format!("avatarConfig.{key} é obrigatório"))?;
        if key == "color" {
            let color = field.as_str().context("A cor deve ser hexadecimal #RRGGBB")?;
            if color.len() != 7 || !color.starts_with('#') || !color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit) {
                bail!("A cor deve ser hexadecimal #RRGGBB");
            }
        } else if !rule["enum"].as_array().context("schema de enum inválido")?.contains(field) {
            bail!("avatarConfig.{key} desconhecido");
        }
    }
    Ok(())
}

pub fn profile_query(sql: &str) -> String {
    sql.replace(
        "{profile}",
        "CASE WHEN json_valid(config) THEN json_set(config, '$.avatarConfig', json(avatar_config)) ELSE config END",
    )
}

/// A coluna própria é a fonte da aparência; o JSON upstream guarda o restante do perfil.
pub fn stored(cfg: &model::BotConfig) -> Result<(String, String)> {
    let mut profile = serde_json::to_value(cfg)?;
    profile.as_object_mut().context("perfil inválido")?.remove("avatarConfig");
    Ok((profile.to_string(), serde_json::to_string(&cfg.avatar_config)?))
}

pub fn restored(profile: &str, avatar: Option<&str>) -> Result<model::BotConfig> {
    let mut profile: Value = serde_json::from_str(profile)?;
    if let Some(avatar) = avatar {
        profile["avatarConfig"] = serde_json::from_str(avatar)?;
    }
    Ok(serde_json::from_value(profile)?)
}
