//! SQL tipado: ausência e falha de armazenamento nunca são a mesma resposta.
use super::modelo::Projection;
use super::{Fault, Result};
use crate::store::{BotConfig, Entry};
use rusqlite::{Connection, OptionalExtension, params};

const COLS: &str =
    "e.seq,e.id,e.bot_id,e.rev,e.kind,e.turn,e.data,e.created_at,e.updated_at,COALESCE(e.session,e.bot_id)";
// O mesmo predicado serve a busca, cursores, abertura, vizinhas e entrega.
const VISIBLE: &str = "e.bot_id=?1 AND EXISTS(SELECT 1 FROM bots b WHERE b.id=e.bot_id AND b.deleted=0
    AND COALESCE(json_extract(b.config,'$.kind'),'agent')='agent')
    AND (e.kind='user' OR (e.kind='agent' AND json_extract(e.data,'$.final')=1))";

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Entry> {
    let text: String = r.get(6)?;
    let data = serde_json::from_str(&text)
        .map_err(|error| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(error)))?;
    Ok(Entry {
        seq: r.get(0)?,
        id: r.get(1)?,
        bot_id: r.get(2)?,
        rev: r.get(3)?,
        kind: r.get(4)?,
        turn: r.get(5)?,
        data,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
        session_id: r.get(9)?,
    })
}

pub(super) fn bot_generation(db: &Connection, bot: &str) -> Result<i64> {
    let found: Option<String> =
        db.query_row("SELECT config FROM bots WHERE id=?1 AND deleted=0", [bot], |r| r.get(0)).optional()?;
    let Some(config) = found else {
        return Err(Fault::Denied.into());
    };
    let config: BotConfig = serde_json::from_str(&config)?;
    if config.id != bot || config.is_group() {
        return Err(Fault::Denied.into());
    }
    // Geração do perfil, já mantida transacionalmente para todos os escritores e Undo.
    // Atividade/read marks só mudam bots.rev; edição A → B → A continua invalidando a captura.
    crate::fork::spaces::config_generation_on(db, bot)
}

pub(super) fn entry(db: &Connection, bot: &str, id: &str) -> Result<Option<Projection>> {
    let sql = format!("SELECT {COLS} FROM entries e WHERE {VISIBLE} AND e.id=?2");
    Ok(db.query_row(&sql, params![bot, id], row).optional()?.map(Projection::new))
}

pub(super) fn max_seq(db: &Connection, bot: &str) -> Result<i64> {
    Ok(db.query_row(&format!("SELECT COALESCE(MAX(e.seq),0) FROM entries e WHERE {VISIBLE}"), [bot], |r| r.get(0))?)
}

// Orçamento de mensagens por página; a linha extra é a fronteira inclusiva ainda não examinada.
pub(super) const SEARCH_SCAN_LIMIT: usize = 256;

pub(super) fn search(
    db: &Connection,
    bot: &str,
    terms: &[String],
    boundary: i64,
    snapshot: i64,
    limit: usize,
) -> Result<(Vec<Projection>, Option<Projection>)> {
    let sql = format!(
        "SELECT {COLS} FROM entries e WHERE {VISIBLE} AND e.seq<=?2 AND e.seq<=?3 ORDER BY e.seq DESC LIMIT ?4"
    );
    let mut stmt = db.prepare(&sql)?;
    let mut found = Vec::new();
    for (index, entry) in
        stmt.query_map(params![bot, boundary, snapshot, i64::try_from(SEARCH_SCAN_LIMIT + 1)?], row)?.enumerate()
    {
        let projection = Projection::new(entry?);
        if index == SEARCH_SCAN_LIMIT {
            return Ok((found, Some(projection)));
        }
        // A mesma representação que pode sair ao motor decide correspondência e continuação.
        let text = projection.text.to_ascii_lowercase();
        if terms.iter().all(|term| text.contains(term)) {
            if found.len() == limit {
                return Ok((found, Some(projection)));
            }
            found.push(projection);
        }
    }
    Ok((found, None))
}

pub(super) fn neighbors(db: &Connection, bot: &str, seq: i64, before: bool, limit: usize) -> Result<Vec<Projection>> {
    let (op, order) = if before { ("<", "DESC") } else { (">", "ASC") };
    let mut stmt = db.prepare(&format!(
        "SELECT {COLS} FROM entries e WHERE {VISIBLE} AND e.seq {op} ?2 ORDER BY e.seq {order} LIMIT ?3"
    ))?;
    Ok(stmt
        .query_map(params![bot, seq, i64::try_from(limit)?], row)?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .map(Projection::new)
        .collect())
}
