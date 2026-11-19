//! Natureza epistemológica independente da origem e do autor da gravação.
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KindNature {
    Said,
    Inferred,
    Conflict,
    #[default]
    Unknown,
    /// O dono aprovou no app uma lembrança que o Hub achou sustentada por uma mensagem dele (fork, 2b1d-2): "confirmada
    /// pela mensagem do dono", nunca "o dono disse".
    Confirmed,
}

/// O modelo só pode reduzir certeza; nunca declara que o dono disse algo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Hint {
    Inferred,
    Conflict,
}

pub fn proposed(literal_owner: bool, hint: Option<Hint>) -> KindNature {
    match hint {
        Some(Hint::Conflict) => KindNature::Conflict,
        None if literal_owner => KindNature::Said,
        Some(Hint::Inferred) | None => KindNature::Inferred,
    }
}

pub fn name(kind: KindNature) -> &'static str {
    match kind {
        KindNature::Said => "said",
        KindNature::Inferred => "inferred",
        KindNature::Conflict => "conflict",
        KindNature::Unknown => "unknown",
        KindNature::Confirmed => "confirmed",
    }
}

pub fn decode(raw: &str) -> KindNature {
    match raw {
        "said" => KindNature::Said,
        "inferred" => KindNature::Inferred,
        "conflict" => KindNature::Conflict,
        "confirmed" => KindNature::Confirmed,
        _ => KindNature::Unknown,
    }
}

pub fn set(
    conn: &Connection,
    id: i64,
    kind: KindNature,
    bot: Option<&str>,
    alternative: Option<&str>,
) -> crate::store::Result<()> {
    if let Some(alternative) = alternative {
        crate::check::secrets(alternative)?;
    }
    conn.execute(
        "UPDATE memory_entries SET nature=?2,nature_bot=?3,conflict_text=?4 WHERE id=?1",
        params![id, name(kind), bot, alternative],
    )?;
    let entry = crate::store::get(conn, id)?.ok_or(crate::store::Error::NotFound)?;
    crate::origin::set(conn, id, &entry.origin)
}

pub fn set_file(
    conn: &Connection,
    version: i64,
    kind: KindNature,
    bot: Option<&str>,
    alternative: Option<&str>,
) -> crate::store::Result<()> {
    if let Some(alternative) = alternative {
        crate::check::secrets(alternative)?;
    }
    conn.execute(
        "UPDATE memory_file_versions SET nature=?2,nature_bot=?3,conflict_text=?4 WHERE id=?1",
        params![version, name(kind), bot, alternative],
    )?;
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Version {
    pub rev: i64,
    pub text: String,
    pub state: String,
    pub origin: crate::origin::Origin,
    pub nature: KindNature,
    pub nature_bot: Option<String>,
    pub conflict_text: Option<String>,
    /// Zero significa que o legado não guardou a data, não uma data inventada.
    pub at: i64,
}

pub fn previous(raw: &str) -> Option<Version> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    Some(Version {
        rev: value["rev"].as_i64()?,
        text: value["text"].as_str()?.into(),
        state: value["state"].as_str()?.into(),
        origin: crate::origin::decode(value["origin"].as_str().unwrap_or("{}")),
        nature: decode(value["nature"].as_str().unwrap_or("unknown")),
        nature_bot: value["natureBot"].as_str().map(str::to_owned),
        conflict_text: value["conflictText"].as_str().map(str::to_owned),
        at: value["at"].as_i64().unwrap_or_default(),
    })
}

#[derive(Clone, Copy)]
pub struct Revision<'a> {
    pub entry: i64,
    pub text: &'a str,
    /// Prova emitida pelo Hub; não é campo aceito do modelo.
    pub literal_owner: bool,
    pub hint: Option<Hint>,
    pub bot: &'a str,
}

/// Sem prova literal, uma revisão diferente conserva ambas hipóteses sem escolher uma certeza.
pub fn revise(
    conn: &Connection,
    writer: crate::store::Writer<'_>,
    proposal: Revision<'_>,
    now: i64,
    rev: i64,
) -> crate::store::Result<crate::store::Entry> {
    let old = crate::store::get(conn, proposal.entry)?.ok_or(crate::store::Error::NotFound)?;
    // `Writer::Override` dispensa só o veredito de conteúdo; vazio, teto e segredo seguem recusando.
    match crate::check::text(proposal.text) {
        Err(
            crate::check::Refusal::HiddenCharacters
            | crate::check::Refusal::PromptLink
            | crate::check::Refusal::Instruction,
        ) if matches!(writer, crate::store::Writer::Override(_)) => Ok(()),
        r => r,
    }?;
    let proposed = proposed(proposal.literal_owner, proposal.hint);
    let conflict = proposed != KindNature::Said;
    let text = if conflict { old.text.as_str() } else { proposal.text };
    crate::store::revise(conn, writer, proposal.entry, text, now, rev)?;
    let kind = if conflict { KindNature::Conflict } else { proposed };
    set(conn, proposal.entry, kind, conflict.then_some(proposal.bot), conflict.then_some(proposal.text))?;
    crate::store::get(conn, proposal.entry)?.ok_or(crate::store::Error::NotFound)
}

/// Undo restaura apenas uma versão real do mesmo fato, incluindo origem/natureza; não recebe snapshot do modelo.
pub fn restore(
    conn: &Connection,
    id: i64,
    previous_rev: i64,
    now: i64,
    rev: i64,
) -> crate::store::Result<crate::store::Entry> {
    let current = crate::store::get(conn, id)?.ok_or(crate::store::Error::NotFound)?;
    if previous_rev >= current.rev {
        return Err(crate::store::Error::NotFound);
    }
    let (text, alternative): (String, Option<String>) = conn.query_row(
        "SELECT text,conflict_text FROM donna_memory_versions WHERE entry=?1 AND rev=?2",
        params![id, previous_rev],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    crate::check::secrets(&text)?;
    if let Some(alternative) = alternative {
        crate::check::secrets(&alternative)?;
    }
    let changed = conn.execute(
        "UPDATE memory_entries SET
        text=(SELECT text FROM donna_memory_versions WHERE entry=?1 AND rev=?2),
        state=(SELECT state FROM donna_memory_versions WHERE entry=?1 AND rev=?2),
        origin=(SELECT origin FROM donna_memory_versions WHERE entry=?1 AND rev=?2),
        evidence=(SELECT evidence FROM donna_memory_versions WHERE entry=?1 AND rev=?2),
        nature=(SELECT nature FROM donna_memory_versions WHERE entry=?1 AND rev=?2),
        nature_bot=(SELECT nature_bot FROM donna_memory_versions WHERE entry=?1 AND rev=?2),
        conflict_text=(SELECT conflict_text FROM donna_memory_versions WHERE entry=?1 AND rev=?2),
        updated_at=?3,rev=?4 WHERE id=?1 AND EXISTS(SELECT 1 FROM donna_memory_versions WHERE entry=?1 AND rev=?2)",
        params![id, previous_rev, now, rev],
    )?;
    if changed != 1 {
        return Err(crate::store::Error::NotFound);
    }
    let entry = crate::store::get(conn, id)?.ok_or(crate::store::Error::NotFound)?;
    crate::origin::set(conn, id, &entry.origin)?;
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{self, New, Writer};
    #[test]
    fn hint_nao_fabrica_fala_do_dono_e_legado_fica_desconhecido() {
        assert!(serde_json::from_str::<Hint>("\"said\"").is_err());
        assert!(serde_json::from_str::<Hint>("\"unknown\"").is_err());
        assert_eq!(proposed(false, None), KindNature::Inferred);
        assert_eq!(proposed(false, Some(Hint::Conflict)), KindNature::Conflict);
        assert_eq!(proposed(true, None), KindNature::Said);
        assert_eq!(proposed(true, Some(Hint::Inferred)), KindNature::Inferred);
        let db = Connection::open_in_memory().unwrap();
        crate::schema::migrate(&db).unwrap();
        let entry = store::add(
            &db,
            Writer::User,
            New { bot: Some("bot"), text: "Mora em Curitiba", room: None, evidence: &[] },
            1,
            1,
        )
        .unwrap();
        assert_eq!(entry.nature, KindNature::Unknown, "autor não fabrica natureza de registro legado");
        assert!(entry.nature_bot.is_none());
    }

    #[test]
    fn conflito_preserva_anterior_e_undo_restaura_natureza_origem_e_data_versionada() {
        let db = Connection::open_in_memory().unwrap();
        crate::schema::migrate(&db).unwrap();
        let original = store::add(
            &db,
            Writer::Bot("bot"),
            New { bot: Some("bot"), text: "Mora em Curitiba", room: None, evidence: &["m1".into()] },
            10,
            1,
        )
        .unwrap();
        set(&db, original.id, KindNature::Said, None, None).unwrap();
        let origin = crate::origin::Origin {
            schema: 1,
            kind: crate::origin::Kind::External,
            sources: Vec::new(),
            external: true,
        };
        crate::origin::set(&db, original.id, &origin).unwrap();
        let conflict = revise(
            &db,
            Writer::Bot("bot"),
            Revision { entry: original.id, text: "Mora em Recife", literal_owner: false, hint: None, bot: "bot" },
            20,
            2,
        )
        .unwrap();
        assert_eq!(conflict.text, "Mora em Curitiba");
        assert_eq!(conflict.conflict_text.as_deref(), Some("Mora em Recife"));
        assert_eq!(conflict.nature, KindNature::Conflict);
        assert_eq!(conflict.nature_bot.as_deref(), Some("bot"));
        let previous = conflict.previous_version.unwrap();
        assert_eq!(previous.nature, KindNature::Said);
        assert_eq!(previous.at, 10);
        assert_eq!(previous.origin, origin);
        let restored = restore(&db, original.id, previous.rev, 30, 3).unwrap();
        assert_eq!(restored.nature, KindNature::Said);
        assert!(restored.conflict_text.is_none());
        assert!(restored.nature_bot.is_none());
        assert_eq!(restored.origin, origin, "natureza nunca lava origem externa");
        let said = revise(
            &db,
            Writer::Bot("bot"),
            Revision { entry: original.id, text: "Mora em Recife", literal_owner: true, hint: None, bot: "bot" },
            40,
            4,
        )
        .unwrap();
        assert_eq!(said.text, "Mora em Recife");
        assert_eq!(said.nature, KindNature::Said);
        assert_eq!(said.origin, origin);
    }
}

#[cfg(test)]
mod file_tests {
    use super::*;
    use crate::files::{self, Author, Cause, Change};
    #[test]
    fn nota_conserva_natureza_por_versao_e_restauracao_independente_da_origem() {
        let db = Connection::open_in_memory().unwrap();
        crate::schema::migrate(&db).unwrap();
        let text = "---\ndescription: Viagens\n---\nPrefere trem\n";
        let change = Change {
            bot: Some("bot"),
            name: "viagens.md",
            content: Some(text),
            author: Author::Bot,
            author_bot: Some("bot"),
            cause: Cause::Edit,
            mtime: 10,
            size: text.len() as u64,
        };
        let first = files::record(&db, &change, 10, 1).unwrap();
        set_file(&db, first.id, KindNature::Said, None, None).unwrap();
        let old = files::version(&db, first.id).unwrap().unwrap();
        let next = files::record(&db, &change, 20, 2).unwrap();
        set_file(&db, next.id, KindNature::Conflict, Some("bot"), Some("Prefere avião")).unwrap();
        assert_eq!(files::current(&db, Some("bot"), "viagens.md").unwrap().as_deref(), Some(text));
        assert_eq!(files::get(&db, Some("bot"), "viagens.md").unwrap().unwrap().nature, KindNature::Conflict);
        assert_eq!(files::version(&db, first.id).unwrap().unwrap().nature, KindNature::Said);
        let restored = files::record(&db, &change, 30, 3).unwrap();
        set_file(&db, restored.id, old.nature, old.nature_bot.as_deref(), old.conflict_text.as_deref()).unwrap();
        let current = files::version(&db, restored.id).unwrap().unwrap();
        assert_eq!(current.nature, KindNature::Said);
        assert!(current.conflict_text.is_none());
        assert_eq!(current.origin, old.origin);
    }
}
