//! Contrato Mexeriquinhas v2 do fork; não altera o schema público da biblioteca legada.
use super::{AvatarConfig, DATA, MexeriquinhaConfig, rgb};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::sync::LazyLock;

pub(super) static SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../apps/desktop/src/shared/fork-mexeriquinha.schema.json"))
        .expect("schema Mexeriquinhas incorporado inválido")
});
pub(super) static DATA_V2: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../apps/desktop/src/shared/fork-mexeriquinha-data.json"))
        .expect("dados Mexeriquinhas incorporados inválidos")
});

// Este é um subconjunto fechado, não um executor geral de JSON Schema. Metadados só
// na raiz; ramos disjuntos por estilo tornam o `any` equivalente ao oneOf publicado.
// Qualquer keyword nova precisa de implementação/revisão: nunca é ignorada silenciosamente.
fn supported_rule(rule: &Value) -> bool {
    let Some(object) = rule.as_object() else { return false };
    // v3: `assetId` é uma string livre, mas só o formato nomeado entra no subconjunto.
    if object.len() == 2 && rule["type"] == "string" && rule["format"] == "assetId" {
        return true;
    }
    if let Some(value) = object.get("const") {
        return object.len() == 1 && !value.is_object() && !value.is_array();
    }
    if let Some(options) = object.get("enum").and_then(Value::as_array) {
        let strings: std::collections::BTreeSet<_> = options.iter().filter_map(Value::as_str).collect();
        return object.len() == 1 && !options.is_empty() && strings.len() == options.len();
    }
    if object.len() != 4 || rule["type"] != "object" || rule["additionalProperties"] != false {
        return false;
    }
    let (Some(properties), Some(required)) = (rule["properties"].as_object(), rule["required"].as_array()) else {
        return false;
    };
    let keys: std::collections::BTreeSet<_> = required.iter().filter_map(Value::as_str).collect();
    !properties.is_empty()
        && keys.len() == required.len()
        && keys.iter().all(|key| properties.contains_key(*key))
        && properties.values().all(supported_rule)
}

pub(super) fn supported_schema(schema: &Value) -> bool {
    let Some(root) = schema.as_object() else { return false };
    if root.iter().any(|(key, value)| {
        key != "oneOf" && (!["$schema", "$id", "title", "description"].contains(&key.as_str()) || !value.is_string())
    }) {
        return false;
    }
    let Some(branches) = schema["oneOf"].as_array() else { return false };
    if branches.len() != 3 {
        return false;
    }
    let mut styles = std::collections::BTreeSet::new();
    for branch in branches {
        if !supported_rule(branch) {
            return false;
        }
        let (Some(v), Some(style)) =
            (branch["properties"]["v"]["const"].as_u64(), branch["properties"]["estilo"]["const"].as_str())
        else {
            return false;
        };
        let Some(required) = branch["required"].as_array() else { return false };
        if !required.contains(&json!("v")) || !required.contains(&json!("estilo")) {
            return false;
        }
        styles.insert((v, style.to_owned()));
    }
    styles == std::collections::BTreeSet::from([(2, "simples".to_owned()), (2, "3d".to_owned()), (3, "3d".to_owned())])
}

static SCHEMA_SUPPORTED: LazyLock<bool> = LazyLock::new(|| supported_schema(&SCHEMA));

fn matches(value: &Value, rule: &Value) -> bool {
    if rule["format"] == "assetId" {
        return value.as_str().is_some_and(super::asset_id_valido);
    }
    if let Some(expected) = rule.get("const") {
        return value == expected;
    }
    if let Some(options) = rule["enum"].as_array() {
        return options.contains(value);
    }
    let (Some(object), Some(properties), Some(required)) =
        (value.as_object(), rule["properties"].as_object(), rule["required"].as_array())
    else {
        return false;
    };
    required.iter().all(|key| key.as_str().is_some_and(|key| object.contains_key(key)))
        && object.iter().all(|(key, field)| properties.get(key).is_some_and(|rule| matches(field, rule)))
}

pub(super) fn validate(value: &Value) -> Result<()> {
    if !*SCHEMA_SUPPORTED
        || !SCHEMA["oneOf"].as_array().expect("schema v2 sem variantes").iter().any(|rule| matches(value, rule))
    {
        bail!("avatarConfig Mexeriquinhas inválido");
    }
    Ok(())
}

/// Migração determinística de aparência; nunca interpreta nome/instruções como autoridade.
pub(super) fn from_legacy(avatar: &AvatarConfig, primary: bool) -> MexeriquinhaConfig {
    if primary && *avatar == super::preset("", "", true) {
        return MexeriquinhaConfig(
            json!({"v":2,"estilo":"simples","modeloId":"berga","aparencia":DATA_V2["modelos"]["berga"]["aparencia"]}),
        );
    }
    let mut appearance = DATA_V2["padrao"].clone();
    let target = rgb(avatar.0["color"].as_str().expect("cor legada validada"));
    let color = DATA_V2["cores"]
        .as_object()
        .expect("paleta v2")
        .iter()
        .min_by_key(|(id, color)| {
            let distance =
                rgb(color.as_str().expect("cor v2")).iter().zip(target).map(|(a, b)| (a - b).pow(2)).sum::<i32>();
            (distance, (*id).clone())
        })
        .map(|(id, _)| id.clone())
        .expect("paleta v2 não vazia");
    appearance["cor"] = color.into();
    let eye = match avatar.0["face"].as_str() {
        Some("glasses") => json!("oculos-redondos"),
        Some("shades") => json!("oculos-escuros"),
        _ => {
            DATA_V2["olhosLegados"].get(avatar.0["eye"].as_str().unwrap_or_default()).cloned().unwrap_or(json!("ponto"))
        }
    };
    appearance["olhos"] = eye;
    appearance["acessorio"] = DATA_V2["acessoriosLegados"]
        .get(avatar.0["head"].as_str().unwrap_or_default())
        .cloned()
        .unwrap_or(json!("nenhum"));
    if matches!(avatar.0["neck"].as_str(), Some("tie" | "bowtie")) {
        appearance["roupa"] = json!("blazer");
        appearance["objeto"] = json!("gravata");
    }
    if let Some(badge) =
        DATA_V2["badgesLegados"].get(avatar.0["badge"].as_str().unwrap_or_default()).and_then(Value::as_object)
    {
        for (key, value) in badge {
            appearance[key] = value.clone();
        }
    }
    MexeriquinhaConfig(json!({"v":2,"estilo":"simples","aparencia":appearance}))
}

/// Projeção estritamente legada usada nas cores/formas de clientes antigos.
pub(super) fn legacy_projection(avatar: &MexeriquinhaConfig) -> AvatarConfig {
    let appearance = if avatar.0["estilo"] == "simples" {
        &avatar.0["aparencia"]
    } else {
        // v3 gerado: a projeção legada usa o conjunto de fallback, não o assetId.
        let conjunto = avatar.0["fallbackConjuntoId"]
            .as_str()
            .or_else(|| avatar.0["conjuntoId"].as_str())
            .expect("conjunto validado");
        &DATA_V2["conjuntos"][conjunto]
    };
    let mut old = DATA["defaultConfig"].clone();
    old["shape"] = json!("oval");
    old["color"] = DATA_V2["cores"][appearance["cor"].as_str().expect("cor v2 validada")].clone();
    old["eye"] = json!(match appearance["olhos"].as_str() {
        Some("sonolentos") => "bar",
        Some("brilhantes") => "round",
        Some("piscando") => "cat",
        _ => "auto",
    });
    old["face"] = json!(match appearance["olhos"].as_str() {
        Some("oculos-redondos") => "glasses",
        Some("oculos-escuros" | "oculos-verdes") => "shades",
        _ => "none",
    });
    AvatarConfig(old)
}

/// Id de conjunto PRONTO do catálogo fixo (fallbackConjuntoId do v3 e o enum do v2).
pub(super) fn conjunto_catalogo(id: &str) -> bool {
    DATA_V2["conjuntos"][id].is_object()
}

pub(super) fn label(avatar: &MexeriquinhaConfig) -> String {
    if let Some(name) =
        avatar.0.get("modeloId").and_then(Value::as_str).and_then(|id| DATA_V2["modelos"][id]["nome"].as_str())
    {
        return name.to_owned();
    }
    if avatar.0["estilo"] == "3d" { "Mexeriquinha 3D".into() } else { "Mexeriquinha Simples".into() }
}
