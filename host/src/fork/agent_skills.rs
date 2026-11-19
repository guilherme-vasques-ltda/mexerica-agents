//! Formato Agent Skills (D137). Não concede ferramentas nem migra o acervo.

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::Serialize;
use serde_yaml_ng::{Mapping, Value};
use unicode_general_category::{GeneralCategory, get_general_category};
use unicode_normalization::UnicodeNormalization;

const MAX_NAME: usize = 64;

pub(crate) struct Document<'a> {
    pub name: String,
    pub description: String,
    pub body: &'a str,
}

/// Só separa a moldura Markdown; toda a semântica dos campos fica com o parser YAML.
fn split(md: &str) -> Result<(&str, &str)> {
    let mut lines = md.split_inclusive('\n');
    ensure!(lines.next().map(str::trim_end) == Some("---"), "SKILL.md deve começar com ---");
    let start = md.find('\n').ok_or_else(|| anyhow!("Frontmatter YAML não fechado com ---"))? + 1;
    let mut end = start;
    for line in lines {
        if line.trim_end() == "---" {
            return Ok((&md[start..end], md[end + line.len()..].trim()));
        }
        end += line.len();
    }
    bail!("Frontmatter YAML não fechado com ---")
}

// Categorias de letras/números usadas por isalnum no validador Python; marcas combinantes
// isoladas não são letras só por terem a propriedade Unicode Alphabetic.
fn alphanumeric(c: char) -> bool {
    matches!(
        get_general_category(c),
        GeneralCategory::UppercaseLetter
            | GeneralCategory::LowercaseLetter
            | GeneralCategory::TitlecaseLetter
            | GeneralCategory::ModifierLetter
            | GeneralCategory::OtherLetter
            | GeneralCategory::DecimalNumber
            | GeneralCategory::LetterNumber
            | GeneralCategory::OtherNumber
    )
}

fn text<'a>(map: &'a Mapping, field: &str) -> Result<&'a str> {
    match map.get(Value::String(field.into())) {
        Some(Value::String(value)) => Ok(value),
        _ => bail!("`{field}` deve ser texto"),
    }
}

fn nonempty_within(value: &str, field: &str, max: usize) -> Result<()> {
    ensure!(!value.trim().is_empty(), "`{field}` não pode ficar vazio");
    ensure!(value.chars().count() <= max, "`{field}` deve ter no máximo {max} caracteres");
    Ok(())
}

/// Segue a comparação NFKC do validador oficial; Unicode não é limitado a ASCII.
pub(crate) fn validate_name(name: &str, directory: &str) -> Result<()> {
    let name: String = name.nfkc().collect();
    nonempty_within(&name, "name", MAX_NAME)?;
    ensure!(name == name.to_lowercase(), "`name` deve estar em minúsculas");
    ensure!(
        name.chars().all(|c| alphanumeric(c) || c == '-')
            && !name.starts_with('-')
            && !name.ends_with('-')
            && !name.contains("--"),
        "`name` aceita letras, números e hífens, sem hífen nas pontas nem --"
    );
    ensure!(name == directory.nfkc().collect::<String>(), "`name` deve ser igual ao nome da pasta");
    Ok(())
}

/// Estrito para entradas novas. Não normaliza nem regrava o arquivo importado.
pub(crate) fn parse<'a>(md: &'a str, directory: &str) -> Result<Document<'a>> {
    let (yaml, body) = split(md)?;
    let value: Value = serde_yaml_ng::from_str(yaml).context("Frontmatter YAML inválido")?;
    let Value::Mapping(map) = value else { bail!("Frontmatter YAML deve ser um mapa") };
    for key in map.keys() {
        ensure!(
            matches!(key, Value::String(k) if matches!(k.as_str(),
                "name" | "description" | "license" | "compatibility" | "metadata" | "allowed-tools")),
            "Campo de frontmatter não suportado; use `metadata` para propriedades adicionais"
        );
    }
    let name = text(&map, "name")?;
    validate_name(name, directory)?;
    let description = text(&map, "description")?;
    nonempty_within(description, "description", 1024)?;
    for field in ["license", "allowed-tools", "compatibility"] {
        if map.contains_key(Value::String(field.into())) {
            let value = text(&map, field)?;
            if field == "compatibility" {
                nonempty_within(value, field, 500)?;
            }
        }
    }
    if let Some(metadata) = map.get(Value::String("metadata".into())) {
        ensure!(
            matches!(metadata, Value::Mapping(m) if m.iter().all(|(k, v)|
                matches!((k, v), (Value::String(_), Value::String(_))))),
            "`metadata` deve ser um mapa de textos para textos"
        );
    }
    Ok(Document { name: name.to_owned(), description: description.to_owned(), body })
}

/// O título livre da criação vira um identificador técnico. Não altera o slug de conectores.
fn unique_name(title: &str, taken: &[String]) -> Result<String> {
    let mut base = String::new();
    for c in title.nfkc().flat_map(char::to_lowercase) {
        if alphanumeric(c) {
            base.push(c);
        } else if !base.is_empty() && !base.ends_with('-') {
            base.push('-');
        }
    }
    let base = base.chars().take(MAX_NAME).collect::<String>().trim_end_matches('-').to_owned();
    ensure!(!base.is_empty(), "O nome deve conter alguma letra ou número");
    let mut name = base.clone();
    let mut suffix = 2_u64;
    while taken.iter().any(|id| id.nfkc().eq(name.chars())) {
        let ending = format!("-{suffix}");
        let prefix = base.chars().take(MAX_NAME - ending.len()).collect::<String>();
        name = format!("{}{ending}", prefix.trim_end_matches('-'));
        suffix += 1;
    }
    validate_name(&name, &name)?;
    Ok(name)
}

/// Serialização YAML, seguida da mesma validação usada na importação, antes de qualquer escrita.
pub(crate) fn custom_document(
    title: &str,
    description: &str,
    instructions: &str,
    taken: &[String],
) -> Result<(String, String)> {
    #[derive(Serialize)]
    struct Frontmatter<'a> {
        name: &'a str,
        description: &'a str,
    }
    let name = unique_name(title, taken)?;
    let yaml = serde_yaml_ng::to_string(&Frontmatter { name: &name, description }).context("Serializando YAML")?;
    let md = format!("---\n{yaml}---\n\n{instructions}\n");
    parse(&md, &name)?;
    Ok((name, md))
}

/// Legado continua legível, com diagnóstico explícito no log, sem alterar arquivo ou cadastro.
pub(crate) fn legacy_body<'a>(md: &'a str, directory: &str) -> &'a str {
    match parse(md, directory) {
        Ok(document) => document.body,
        Err(error) => {
            tracing::warn!(skill = directory, %error, "Skill legada com formato inválido; arquivo e cadastro preservados");
            split(md).map_or_else(|_| md.trim(), |(_, body)| body)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_round_trip_and_real_scalars() {
        let description = "Use quando: houver PDF # revisão \"ação\" 'sim'\n---\nOutra linha: 日本語 🦀";
        let (id, md) =
            custom_document("Minha Skill", description, "# Instruções\n---\nLeia.", &[]).expect("fixture válida");
        let parsed = parse(&md, &id).expect("fixture válida");
        assert_eq!(id, "minha-skill");
        assert_eq!(parsed.name, id);
        assert_eq!(parsed.description, description);
        assert_eq!(parsed.body, "# Instruções\n---\nLeia.");
        for (yaml, expected) in [
            ("'Use quando: houver PDF # sim'", "Use quando: houver PDF # sim"),
            ("\"Aspas: \\\"sim\\\" # ação\\nPróxima\"", "Aspas: \"sim\" # ação\nPróxima"),
            ("|-\n  Linha: um # sim\n  Linha dois", "Linha: um # sim\nLinha dois"),
            (">-\n  Uma linha\n  dobrada.", "Uma linha dobrada."),
        ] {
            let md = format!("---\r\nname: 'pdf' # nota\r\ndescription: {yaml}\r\n---\r\n# PDF");
            let parsed = parse(&md, "pdf").expect("fixture válida");
            assert_eq!(parsed.name, "pdf");
            assert_eq!(parsed.description, expected);
            assert_eq!(parsed.body, "# PDF");
        }
    }

    #[test]
    fn names_remain_valid_after_collision_and_truncation() {
        let title = format!("{} fim", "é".repeat(63));
        let (id, md) = custom_document(&title, "Descrição", "", &[]).expect("fixture válida");
        assert_eq!(id, "é".repeat(63)); // truncamento não deixa hífen na ponta
        assert_eq!(parse(&md, &id).expect("fixture válida").name, id);
        let base = "é".repeat(64);
        let taken = vec![base.clone(), format!("{}-2", "é".repeat(62))];
        let (id, md) = custom_document(&base, "Descrição", "", &taken).expect("fixture válida");
        assert_eq!(id, format!("{}-3", "é".repeat(62)));
        assert_eq!(id.chars().count(), 64);
        assert_eq!(parse(&md, &id).expect("fixture válida").name, id);
        let (id, _) = custom_document("ＰＤＦ Ação 日本語", "Descrição", "", &[]).expect("fixture válida");
        assert_eq!(id, "pdf-ação-日本語");
        validate_name("ｐｄｆ", "pdf").expect("fixture válida");
        validate_name("ac\u{327}a\u{303}o", "ação").expect("fixture válida");
        assert!(validate_name("a\u{345}", "a\u{345}").is_err());
        for name in ["", "PDF", "-pdf", "pdf-", "pdf--x", "pdf/x", "pdf x"] {
            assert!(validate_name(name, name).is_err(), "{name}");
        }
        assert!(validate_name(&"a".repeat(65), &"a".repeat(65)).is_err());
        assert!(validate_name("pdf", "pdf-2").is_err());
    }

    #[test]
    fn required_fields_and_character_limits() {
        custom_document("pdf", &"🦀".repeat(1024), "", &[]).expect("fixture válida");
        assert!(custom_document("pdf", &"🦀".repeat(1025), "", &[]).is_err());
        for yaml in [
            "name: pdf",
            "description: x",
            "name: pdf\ndescription: ''",
            "name: pdf\ndescription: '  '",
            "name: 12\ndescription: x",
            "name: pdf\ndescription: true",
            "name: pdf\ndescription: null",
            "name: pdf\ndescription: [texto]",
            "name: pdf\ndescription: {x: y}",
            "name: pdf\ndescription: use quando: PDF",
            "name: [pdf]\ndescription: x",
            "name: pdf\nname: outro\ndescription: x",
            "[pdf, x]",
            "!!str pdf",
        ] {
            assert!(parse(&format!("---\n{yaml}\n---\n"), "pdf").is_err(), "{yaml}");
        }
        for md in ["# Sem frontmatter", "---\nname: pdf", "---", "---\n---\n"] {
            assert!(parse(md, "pdf").is_err(), "{md}");
        }
    }

    #[test]
    fn optional_fields_are_optional_and_typed() {
        let yaml = "name: pdf\ndescription: x\nlicense: MIT\nallowed-tools: Bash(git:*) Read\ncompatibility: |\n  Requer Python\nmetadata:\n  author: José\n  version: '1.0'";
        let md = format!("---\n{yaml}\n---\nCorpo");
        assert_eq!(parse(&md, "pdf").expect("fixture válida").body, "Corpo");
        let prefix = "---\nname: pdf\ndescription: x\n";
        for optional in [
            "license: null",
            "license: [MIT]",
            "allowed-tools: [Read]",
            "allowed-tools: true",
            "compatibility: ''",
            "compatibility: null",
            "compatibility: 12",
            "metadata: null",
            "metadata: []",
            "metadata: {version: 1}",
            "metadata: {1: texto}",
            "metadata: {a: {b: c}}",
            "extra: x",
            "license: !Objeto x",
        ] {
            assert!(parse(&format!("{prefix}{optional}\n---\n"), "pdf").is_err(), "{optional}");
        }
        for (count, valid) in [(500, true), (501, false)] {
            let md = format!("{prefix}compatibility: {}\n---\n", "é".repeat(count));
            assert_eq!(parse(&md, "pdf").is_ok(), valid);
        }
        assert_eq!(legacy_body("---\nname: PDF\n---\nCorpo antigo", "pdf"), "Corpo antigo");
        assert_eq!(legacy_body("Arquivo sem YAML", "pdf"), "Arquivo sem YAML");
    }
}
