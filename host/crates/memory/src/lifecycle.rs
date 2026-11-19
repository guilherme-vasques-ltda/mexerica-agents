//! Envelhecimento conservador e reforço lexical com evidência independente.
//! Nenhuma decisão promove natureza epistemológica ou elimina histórico.
use crate::{
    nature::KindNature,
    origin::Origin,
    store::{self, Author, Entry, State, Writer},
};
use rusqlite::{Connection, params};
use std::collections::HashSet;

pub const WEAK_AGE_MS: i64 = 90 * 24 * 60 * 60 * 1_000;
pub const CHECK_MS: i64 = 60 * 60 * 1_000;

pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS donna_memory_profiles(bot TEXT NOT NULL,shared INTEGER NOT NULL,body TEXT NOT NULL,PRIMARY KEY(bot,shared));
CREATE TABLE IF NOT EXISTS donna_memory_reinforcements(entry INTEGER NOT NULL,message TEXT NOT NULL,at INTEGER NOT NULL,PRIMARY KEY(entry,message));
CREATE TABLE IF NOT EXISTS donna_memory_aging(entry INTEGER NOT NULL,rev INTEGER NOT NULL,at INTEGER NOT NULL,reason TEXT NOT NULL,PRIMARY KEY(entry,rev));
CREATE TABLE IF NOT EXISTS donna_memory_age_checks(bot TEXT PRIMARY KEY,at INTEGER NOT NULL);
CREATE TRIGGER IF NOT EXISTS donna_memory_profile_insert AFTER INSERT ON memory_entries BEGIN
 DELETE FROM donna_memory_profiles WHERE bot=new.bot OR (new.bot IS NULL AND shared=1);
END;
CREATE TRIGGER IF NOT EXISTS donna_memory_profile_update AFTER UPDATE ON memory_entries BEGIN
 DELETE FROM donna_memory_profiles WHERE bot=old.bot OR bot=new.bot OR ((old.bot IS NULL OR new.bot IS NULL) AND shared=1);
END;
CREATE TRIGGER IF NOT EXISTS donna_memory_profile_delete AFTER DELETE ON memory_entries BEGIN
 DELETE FROM donna_memory_profiles WHERE bot=old.bot OR (old.bot IS NULL AND shared=1);
 DELETE FROM donna_memory_reinforcements WHERE entry=old.id;
END;
CREATE TRIGGER IF NOT EXISTS donna_memory_reinforcement_reset AFTER UPDATE OF text ON memory_entries WHEN old.text<>new.text BEGIN
 DELETE FROM donna_memory_reinforcements WHERE entry=new.id;
END;";

/// Só após validação de origem, escopo e evidência pelo Hub, dentro da mesma transação.
/// Replays e fontes já usadas não contam. Arquivados/conflitos nunca voltam por repetição.
pub fn reinforce(
    conn: &Connection,
    bot: Option<&str>,
    text: &str,
    evidence: &[String],
    origin: &Origin,
    now: i64,
    rev: i64,
) -> store::Result<bool> {
    if origin.external || !matches!(origin.kind, crate::origin::Kind::User | crate::origin::Kind::Conversation) {
        return Ok(false);
    }
    let hash = store::fingerprint(text);
    if store::fact_blocked(conn, bot, text, evidence)? {
        return Ok(false);
    }
    let matching = store::layer(conn, bot)?.into_iter().find(|e| {
        e.state == State::Active
            && matches!(e.author, Author::Bot | Author::Dream)
            && e.nature != KindNature::Conflict
            && e.conflict_text.is_none()
            && store::fingerprint(&e.text) == hash
    });
    let Some(entry) = matching else { return Ok(false) };
    let mut fresh = Vec::new();
    for message in evidence.iter().collect::<HashSet<_>>() {
        if !origin
            .sources
            .iter()
            .any(|source| source.message_id == *message && source.kind == crate::origin::Kind::User)
            || store::marked(conn, bot, &format!("message:{message}"))?
            || store::marked(conn, None, &format!("message:{message}"))?
        {
            continue;
        }
        let seen: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM donna_memory_lineage WHERE kind='fact' AND item=CAST(?1 AS TEXT) AND message=?2)
             OR EXISTS(SELECT 1 FROM donna_memory_reinforcements WHERE entry=?1 AND message=?2)",
            params![entry.id, message], |r| r.get(0),
        )?;
        if !message.is_empty() && !seen && !entry.evidence.contains(message) {
            fresh.push(message.clone());
        }
    }
    if fresh.is_empty() {
        return Ok(false);
    }
    for message in &fresh {
        conn.execute(
            "INSERT INTO donna_memory_reinforcements(entry,message,at) VALUES(?1,?2,?3)",
            params![entry.id, message, now],
        )?;
    }
    let mut merged = entry.evidence.clone();
    merged.extend(fresh);
    conn.execute(
        "UPDATE memory_entries SET evidence=?2,updated_at=?3,rev=?4 WHERE id=?1",
        params![entry.id, merged.join(","), now, rev],
    )?;
    crate::origin::set(conn, entry.id, &Origin::merge([entry.origin, origin.clone()]))?;
    Ok(true)
}

pub fn candidates(conn: &Connection, bot: Option<&str>, now: i64) -> store::Result<Vec<Entry>> {
    let mut eligible = Vec::new();
    for e in store::layer(conn, bot)? {
        if e.state != State::Active
            || e.pinned
            || e.nature != KindNature::Inferred
            || e.conflict_text.is_some()
            || !matches!(e.author, Author::Bot | Author::Dream)
            || e.updated_at <= 0
            || now.saturating_sub(e.updated_at) < WEAK_AGE_MS
            || e.evidence.iter().collect::<HashSet<_>>().len() != 1
            || store::marked(conn, bot, &format!("kept:{}", e.id))?
        {
            continue;
        }
        let reinforced: bool =
            conn.query_row("SELECT EXISTS(SELECT 1 FROM donna_memory_reinforcements WHERE entry=?1)", [e.id], |r| {
                r.get(0)
            })?;
        if !reinforced {
            eligible.push(e);
        }
    }
    Ok(eligible)
}

/// Arquivo e motivo compartilham a transação/revisão com as versões já existentes.
pub fn age(conn: &Connection, bot: Option<&str>, now: i64, rev: i64) -> store::Result<usize> {
    let entries = candidates(conn, bot, now)?;
    for e in &entries {
        store::archive(conn, Writer::Dream(e.author_bot.as_deref().unwrap_or("memory-aging")), e.id, None, now, rev)?;
        conn.execute(
            "INSERT INTO donna_memory_aging(entry,rev,at,reason) VALUES(?1,?2,?3,'weak_unreinforced_90d')",
            params![e.id, rev, now],
        )?;
    }
    Ok(entries.len())
}

#[cfg(test)]
mod tests;
