//! D115: aparência própria, validada no Hub e transportada pelo sync de `BotConfig`.
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::sync::LazyLock;

pub mod gerador;
pub(crate) mod mexeriquinhas;

static SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!(
        "../../../apps/desktop/src/renderer/lib/donnabot-avatars/schema/avatar-config.schema.json"
    ))
    .expect("schema de avatar incorporado inválido")
});
static ROLES: LazyLock<Vec<Value>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../apps/desktop/src/renderer/lib/donnabot-avatars/presets/roles.json"))
        .expect("presets de avatar incorporados inválidos")
});

static DATA: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../apps/desktop/src/shared/fork-avatar-data.json"))
        .expect("adaptação de avatar incorporada inválida")
});

/// O valor só pode ser construído após validar os campos do schema distribuído.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct AvatarConfig(Value);

/// `assetId` de conjunto gerado pelo Hub: uuid v4 canônico (contrato v3).
pub(crate) fn asset_id_valido(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|u| u.get_version() == Some(uuid::Version::Random) && u.to_string() == id)
}

/// O `assetId` vivo da config v3 do bot, se a aparência é um conjunto gerado.
pub(crate) fn asset_id_gerado(cfg: &crate::store::BotConfig) -> Option<&str> {
    cfg.mexeriquinha
        .as_ref()
        .and_then(|m| (m.0["v"] == 3 && m.0["estilo"] == "3d").then(|| m.0["assetId"].as_str()).flatten())
}

/// Campo interno do fork, separado do contrato público legado `AvatarConfig`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct MexeriquinhaConfig(Value);
impl<'de> Deserialize<'de> for MexeriquinhaConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let mut value = Value::deserialize(deserializer)?;
        if value.get("v").and_then(Value::as_f64) == Some(2.0) {
            value["v"] = Value::from(2);
        }
        if serde_json::to_vec(&value).map_err(serde::de::Error::custom)?.len() > 1024 {
            return Err(serde::de::Error::custom("mexeriquinha excede 1024 bytes"));
        }
        mexeriquinhas::validate(&value).map_err(serde::de::Error::custom)?;
        Ok(Self(value))
    }
}

/// Tolerância exclusiva da leitura do campo opcional de `BotConfig`.
/// Os ingressos validam o JSON cru com o newtype estrito antes desta desserialização.
pub(crate) fn read_optional_mexeriquinha<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<MexeriquinhaConfig>, D::Error> {
    let value = Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).ok())
}

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
    if serde_json::to_vec(value)?.len() > 1024 {
        bail!("avatarConfig excede 1024 bytes");
    }
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

fn folded(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' => 'a',
            'é' | 'ê' => 'e',
            'í' => 'i',
            'ó' | 'ô' | 'õ' => 'o',
            'ú' | 'ü' => 'u',
            'ç' => 'c',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn aliases(role: &Value, bot_name: bool) -> Vec<String> {
    let keys = if bot_name { &["id", "name", "role"][..] } else { &["role"][..] };
    keys.iter()
        .filter_map(|key| role[key].as_str())
        .chain(
            DATA["roleAliases"][role["id"].as_str().expect("preset sem id")]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str),
        )
        .map(folded)
        .collect()
}

fn matched_role(name: &str, description: &str) -> Option<&'static Value> {
    let name = folded(name);
    let description = folded(description);
    let contains = |text: &str, alias: &str| format!(" {text} ").contains(&format!(" {alias} "));
    // Donna só é aplicada à principal na migração ou por escolha explícita.
    // Nomes de persona só casam com o nome do bot; instruções usam função e aliases explícitos.
    // Nome exato tem prioridade; múltiplas funções não escolhem pela ordem do catálogo.
    for (text, exact, bot_name) in [(&name, true, true), (&name, false, true), (&description, false, false)] {
        let candidates: Vec<_> = ROLES
            .iter()
            .filter(|role| role["id"] != "donna")
            .filter(|role| {
                aliases(role, bot_name).iter().any(|alias| if exact { alias == text } else { contains(text, alias) })
            })
            .collect();
        if !candidates.is_empty() {
            return (candidates.len() == 1).then(|| candidates[0]);
        }
    }
    None
}

fn role_config(role: &Value) -> AvatarConfig {
    let mut config = role.clone();
    config.as_object_mut().expect("preset deve ser objeto").retain(|key, _| SCHEMA["properties"].get(key).is_some());
    serde_json::from_value(config).expect("preset deve respeitar o schema")
}

pub fn preset(name: &str, description: &str, donna: bool) -> AvatarConfig {
    let role = if donna { ROLES.iter().find(|r| r["id"] == "donna") } else { matched_role(name, description) };
    role.map_or_else(
        || serde_json::from_value(DATA["defaultConfig"].clone()).expect("padrão deve respeitar o schema"),
        role_config,
    )
}

fn legacy_config(shape: &str, color: &str) -> AvatarConfig {
    let mut config = DATA["defaultConfig"].clone();
    if let Some(shape) = DATA["legacyShapes"].get(shape) {
        config["shape"] = shape.clone();
    }
    if let Some(color) = DATA["legacyColors"].get(color) {
        config["color"] = color["avatarHex"].clone();
    }
    serde_json::from_value(config).expect("avatar legado deve respeitar o schema")
}

fn profile_config(cfg: &crate::store::BotConfig) -> AvatarConfig {
    matched_role(&cfg.name, &cfg.description)
        .map_or_else(|| legacy_config(&cfg.avatar_shape, &cfg.avatar_color), role_config)
}

fn rgb(hex: &str) -> [i32; 3] {
    let n = i32::from_str_radix(hex.trim_start_matches('#'), 16).expect("cor incorporada/validada inválida");
    [(n >> 16) & 255, (n >> 8) & 255, n & 255]
}

/// Projeção aproximada para clientes móveis e notificações ainda no contrato upstream.
pub fn legacy_avatar(cfg: &crate::store::BotConfig) -> (String, String) {
    let avatar = cfg.avatar_config.clone().unwrap_or_else(|| profile_config(cfg));
    let target = rgb(avatar.0["color"].as_str().expect("cor validada"));
    let color = DATA["legacyColors"]
        .as_object()
        .expect("paleta incorporada")
        .iter()
        .min_by_key(|(_, swatch)| {
            rgb(swatch["hex"].as_str().expect("cor incorporada"))
                .iter()
                .zip(target)
                .map(|(a, b)| (a - b).pow(2))
                .sum::<i32>()
        })
        .map(|(id, _)| id.clone())
        .expect("paleta não vazia");
    let shape = DATA["mobileShapes"][avatar.0["shape"].as_str().expect("forma validada")]
        .as_str()
        .expect("forma móvel incorporada")
        .to_owned();
    (color, shape)
}

pub fn label(cfg: &crate::store::BotConfig) -> String {
    let avatar = cfg.avatar_config.clone().unwrap_or_else(|| profile_config(cfg));
    if let Some(mexeriquinha) = &cfg.mexeriquinha {
        return mexeriquinhas::label(mexeriquinha);
    }
    ROLES.iter().find(|role| role_config(role) == avatar).and_then(|role| role["role"].as_str()).map_or_else(
        || {
            format!(
                "Bot personalizado ({}, {})",
                avatar.0["shape"].as_str().expect("forma validada"),
                avatar.0["color"].as_str().expect("cor validada")
            )
        },
        str::to_owned,
    )
}

/// Aparência inicial da principal, já no contrato v2; não altera o preset legado público.
pub fn principal_mexeriquinha() -> MexeriquinhaConfig {
    mexeriquinhas::from_legacy(&preset("", "", true), true)
}

/// Chamado em toda escrita, inclusive criação por ferramentas e alterações internas.
pub fn settle(cfg: &mut crate::store::BotConfig) {
    // Escritas legadas diretas também vencem: não reponha o v2 sobre uma edição explícita.
    if let (Some(legacy), Some(mexeriquinha)) = (&cfg.avatar_config, &cfg.mexeriquinha)
        && *legacy != mexeriquinhas::legacy_projection(mexeriquinha)
    {
        cfg.mexeriquinha = None;
    }
    if cfg.mexeriquinha.is_none() && cfg.avatar_config.is_none() {
        cfg.mexeriquinha = Some(mexeriquinhas::from_legacy(&profile_config(cfg), false));
    }
    if let Some(mexeriquinha) = &cfg.mexeriquinha {
        cfg.avatar_config = Some(mexeriquinhas::legacy_projection(mexeriquinha));
    } else if cfg.avatar_config.is_none() {
        cfg.avatar_config = Some(profile_config(cfg));
    }
    (cfg.avatar_color, cfg.avatar_shape) = legacy_avatar(cfg);
}

/// Ambos os campos são explícitos e consistentes; o campo público nunca contém v2.
pub fn validate_config(cfg: &crate::store::BotConfig) -> Result<()> {
    if let (Some(legacy), Some(mexeriquinha)) = (&cfg.avatar_config, &cfg.mexeriquinha)
        && *legacy != mexeriquinhas::legacy_projection(mexeriquinha)
    {
        bail!("avatarConfig não corresponde à projeção de mexeriquinha");
    }
    Ok(())
}

/// Depois do merge de um patch: uma escolha legada explícita vence somente neste bot.
pub fn merge_patch(merged: &mut Value, patch: &Value) -> Result<()> {
    if let Some(value) = patch.get("mexeriquinha") {
        let mexeriquinha: MexeriquinhaConfig = serde_json::from_value(value.clone())?;
        merged["avatarConfig"] = serde_json::to_value(mexeriquinhas::legacy_projection(&mexeriquinha))?;
    } else if patch.get("avatarConfig").is_some() {
        merged.as_object_mut().context("perfil inválido")?.remove("mexeriquinha");
    }
    Ok(())
}

/// Rejeita null e o nome alternativo, antes de o Option compatível apagar a distinção.
pub fn validate_patch(patch: &Value) -> Result<()> {
    for key in patch.as_object().into_iter().flat_map(|object| object.keys()) {
        let normalized: String =
            key.chars().filter(|ch| *ch != '_' && *ch != '-').flat_map(char::to_lowercase).collect();
        if (normalized.starts_with("mexeriquinha") && key != "mexeriquinha")
            || (normalized == "avatarconfig" && key != "avatarConfig")
        {
            bail!("Use somente avatarConfig e mexeriquinha no protocolo do app.");
        }
    }
    if let Some(value) = patch.get("avatarConfig") {
        validate(value)?;
    }
    if let Some(value) = patch.get("mexeriquinha") {
        let mexeriquinha: MexeriquinhaConfig = serde_json::from_value(value.clone())?;
        if let Some(legacy) = patch.get("avatarConfig")
            && *legacy != serde_json::to_value(mexeriquinhas::legacy_projection(&mexeriquinha))?
        {
            bail!("avatarConfig não corresponde à projeção de mexeriquinha");
        }
    }
    Ok(())
}

/// O caminho interno usado por bots não ganha os poderes do cliente autenticado.
pub fn user_only(patch: &Value) -> Result<()> {
    if patch.as_object().into_iter().flat_map(|object| object.keys()).any(|key| {
        let normalized: String =
            key.chars().filter(|ch| *ch != '_' && *ch != '-').flat_map(char::to_lowercase).collect();
        normalized.starts_with("mexeriquinha")
            || ["avatarconfig", "avatarcolor", "avatarshape"].contains(&normalized.as_str())
    }) {
        bail!("Só o usuário pode alterar o avatar de um bot.");
    }
    Ok(())
}

/// Projeção para leituras SQL de perfil do upstream, mantendo uma única fonte de aparência.
pub fn profile_query(sql: &str) -> String {
    sql.replace(
        "{profile}",
        "CASE WHEN json_valid(config) THEN json_set(config, '$.avatarConfig', json(avatar_config)) ELSE config END",
    )
}

/// A coluna própria mantém somente o contrato legado; config.mexeriquinha guarda o v2 do fork.
/// Uma escrita por Hub antigo pode descartar a chave desconhecida apenas do perfil regravado.
pub fn stored(cfg: &crate::store::BotConfig) -> Result<(String, String)> {
    let mut profile = serde_json::to_value(cfg)?;
    profile.as_object_mut().context("perfil inválido")?.remove("avatarConfig");
    Ok((profile.to_string(), serde_json::to_string(&cfg.avatar_config)?))
}

pub fn restored(profile: &str, avatar: Option<&str>) -> Result<crate::store::BotConfig> {
    let mut profile: Value = serde_json::from_str(profile)?;
    if let Some(avatar) = avatar {
        profile["avatarConfig"] = serde_json::from_str(avatar)?;
    }
    if let Some(value) = profile.get("mexeriquinha") {
        let valid = serde_json::from_value::<MexeriquinhaConfig>(value.clone()).ok();
        let consistent = valid.as_ref().is_some_and(|value| {
            profile.get("avatarConfig").is_none_or(|legacy| {
                legacy.is_null()
                    || *legacy
                        == serde_json::to_value(mexeriquinhas::legacy_projection(value)).expect("projeção serializável")
            })
        });
        if !consistent {
            profile.as_object_mut().context("perfil inválido")?.remove("mexeriquinha");
        }
    }
    Ok(serde_json::from_value(profile)?)
}

/// Migração transacional e idempotente da coluna própria `bots.avatar_config`.
/// Cada alteração recebe rev novo para alcançar clientes que já sincronizaram.
pub fn migrate(c: &Connection) -> Result<()> {
    const MIGRATION: &str = "mexeriquinhas.internal.v2.migrated";
    let tx = c.unchecked_transaction()?;
    let columns: Vec<String> =
        tx.prepare("PRAGMA table_info(bots)")?.query_map([], |r| r.get(1))?.collect::<rusqlite::Result<_>>()?;
    if !columns.iter().any(|column| column == "avatar_config") {
        tx.execute_batch("ALTER TABLE bots ADD COLUMN avatar_config TEXT")?;
    }
    // Uma única migração: escolhas legadas enviadas explicitamente depois dela continuam legadas.
    let migrated: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM kv WHERE k=?1)", [MIGRATION], |r| r.get(0))?;
    if migrated {
        tx.commit()?;
        return Ok(());
    }
    // Como team::read, configurações antigas ilegíveis usam padrões seguros.
    let settings: Option<String> =
        tx.query_row("SELECT v FROM kv WHERE k = 'team.settings.v1'", [], |r| r.get(0)).optional()?;
    let primary = settings
        .and_then(|raw| serde_json::from_str::<crate::store::TeamSettings>(&raw).ok())
        .and_then(|settings| settings.primary_bot_id);
    let rows: Vec<(String, String, Option<String>)> = tx
        .prepare("SELECT id, config, avatar_config FROM bots WHERE deleted = 0")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, raw, stored_avatar) in rows {
        let mut profile: crate::store::BotConfig = match restored(&raw, stored_avatar.as_deref()) {
            Ok(profile) => profile,
            Err(error) => {
                tracing::warn!(bot = %id, %error, "ignorando perfil ilegível na migração de avatar");
                continue;
            }
        };
        let migrating_legacy = profile.mexeriquinha.is_none();
        if profile.mexeriquinha.is_some() {
            if profile.avatar_config.is_some() {
                continue;
            }
        } else {
            if profile.avatar_config.is_none() && primary.as_deref() == Some(&id) {
                profile.avatar_config = Some(preset("", "", true));
            }
            let legacy = profile.avatar_config.clone().unwrap_or_else(|| profile_config(&profile));
            profile.mexeriquinha = Some(mexeriquinhas::from_legacy(&legacy, primary.as_deref() == Some(&id)));
            profile.avatar_config = profile.mexeriquinha.as_ref().map(mexeriquinhas::legacy_projection);
        }
        settle(&mut profile);
        // Preserva inclusive chaves de versões futuras que este BotConfig ainda não conhece.
        let mut config: Value = serde_json::from_str(&raw)?;
        if migrating_legacy {
            // O original fica fora do perfil/fio e sobrevive a regravações por um Hub antigo.
            // SQL NULL vira null; o texto JSON "null" continua sendo uma string raw distinta.
            let appearance: serde_json::Map<String, Value> = ["avatarConfig", "avatarColor", "avatarShape"]
                .into_iter()
                .filter_map(|key| config.get(key).map(|value| (key.to_owned(), value.clone())))
                .collect();
            let backup = serde_json::json!({"avatar_config": stored_avatar, "config": appearance});
            tx.execute(
                "INSERT OR IGNORE INTO kv(k,v) VALUES(?1,?2)",
                params![format!("mexeriquinhas.legacy.original.{id}"), backup.to_string()],
            )?;
        }
        config.as_object_mut().context("perfil inválido")?.remove("avatarConfig");
        config["mexeriquinha"] = serde_json::to_value(&profile.mexeriquinha)?;
        config["avatarColor"] = profile.avatar_color.into();
        config["avatarShape"] = profile.avatar_shape.into();
        let avatar = serde_json::to_string(&profile.avatar_config)?;
        let rev: i64 = tx.query_row(
            "INSERT INTO kv(k,v) VALUES('rev','1') ON CONFLICT(k) DO UPDATE SET v=CAST(v AS INTEGER)+1 RETURNING CAST(v AS INTEGER)", [], |r| r.get(0),
        )?;
        tx.execute(
            "UPDATE bots SET config = ?2, rev = ?3, avatar_config = ?4 WHERE id = ?1",
            params![id, config.to_string(), rev, avatar],
        )?;
    }
    tx.execute("INSERT INTO kv(k,v) VALUES(?1,'1')", [MIGRATION])?;
    tx.commit()?;
    Ok(())
}

use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod mexeriquinhas_tests;

#[cfg(test)]
mod legacy_main_fixture;
