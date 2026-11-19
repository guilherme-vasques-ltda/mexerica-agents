//! Título inicial de uma sessão extra pela 1ª mensagem do dono: 1ª linha com texto, sem Markdown, até 60 caracteres.
//! O título dado pelo dono (`title_source = owner`) e o da principal nunca são trocados.
use super::store::{self, SessionRow, TITLE_MAX};
use crate::store::next_rev;
use anyhow::Result;
use rusqlite::{Connection, params};

/// O título derivado de `text`; vazio quando não sobra texto.
pub(crate) fn from_message(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("```")).unwrap_or_default();
    let line = line.trim_start_matches(|c: char| c == '#' || c == '>' || c.is_whitespace());
    let line = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")).unwrap_or(line);
    let plain: String = line.chars().filter(|c| !matches!(c, '*' | '_' | '`' | '~')).collect();
    let plain = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    if plain.chars().count() <= TITLE_MAX {
        return plain;
    }
    let cut: String = plain.chars().take(TITLE_MAX - 1).collect();
    format!("{}…", cut.trim_end())
}

/// Grava o título da 1ª mensagem na transação do chamador, só se a extra ainda não tem título. Devolve a linha
/// atualizada para o evento `session`, ou `None` quando nada mudou.
pub(crate) fn first_message(c: &Connection, session: &str, text: &str) -> Result<Option<SessionRow>> {
    let title = from_message(text);
    let untitled = store::get(c, session)?
        .is_some_and(|s| !s.principal && s.title_source == "first_message" && s.title.is_empty());
    if title.is_empty() || !untitled {
        return Ok(None);
    }
    c.execute(
        "UPDATE donna_bot_sessions SET title=?2, revision=?3 WHERE id=?1",
        params![session, title, next_rev(c)?],
    )?;
    store::get(c, session)
}

/// O título da continuação: "Continuação de <origem>", cortado em 60 caracteres (plano, Títulos).
pub(crate) fn continuation(source: &str) -> String {
    let source = if source.trim().is_empty() { "sessão encerrada" } else { source.trim() };
    let title = format!("Continuação de {source}");
    if title.chars().count() <= TITLE_MAX {
        return title;
    }
    let cut: String = title.chars().take(TITLE_MAX - 1).collect();
    format!("{}…", cut.trim_end())
}
