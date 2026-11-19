//! Migração expandir/contrair: copia uma vez para as tabelas novas e, só depois
//! da cópia concluída, apaga as tabelas e as revisões da versão anterior (N+1).
use super::key;
use crate::fork::spaces::PERSONAL;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::{HashMap, HashSet};

/// Excluir um Space elimina também a cópia legada, na mesma transação.
pub(crate) fn delete_space(c: &Connection, space: &str) -> Result<()> {
    let exists: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='donna_bot_sections')",
        [],
        |r| r.get(0),
    )?;
    if exists {
        let mut st = c.prepare("PRAGMA table_info(donna_bot_sections)")?;
        let cols = st.query_map([], |r| r.get::<_, String>(1))?.collect::<rusqlite::Result<Vec<_>>>()?;
        if cols.iter().any(|name| name == "space_id") {
            let members: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='donna_bot_section_assignments')", [], |r| r.get(0))?;
            if members {
                c.execute("DELETE FROM donna_bot_section_assignments WHERE section_id IN (SELECT id FROM donna_bot_sections WHERE space_id=?1)", [space])?;
            }
            c.execute("DELETE FROM donna_bot_sections WHERE space_id=?1", [space])?;
        }
    }
    c.execute("DELETE FROM kv WHERE k=?1", [format!("donna.botSections.rev.{space}")])?;
    Ok(())
}

/// Contração N+1: apaga as tabelas e as revisões da versão anterior. Só roda
/// depois de a cópia concluir; uma falha nela nunca apaga dados.
fn contract(c: &Connection) -> Result<()> {
    c.execute_batch("DROP TABLE IF EXISTS donna_bot_section_assignments; DROP TABLE IF EXISTS donna_bot_sections; DELETE FROM kv WHERE k='donna.botSections.rev' OR k GLOB 'donna.botSections.rev.*';")?;
    Ok(())
}

pub(crate) fn migrate(c: &Connection) -> Result<()> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS bot_groups(id TEXT PRIMARY KEY,space_id TEXT NOT NULL,name TEXT NOT NULL,ord INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS bot_group_members(bot_id TEXT PRIMARY KEY,group_id TEXT NOT NULL); CREATE INDEX IF NOT EXISTS bot_group_members_group ON bot_group_members(group_id);")?;
    if let Err(error) = copy(c) {
        tracing::error!(%error, "migração de grupos incompleta; será retomada na próxima abertura");
        return Ok(());
    }
    // Falha ao apagar as tabelas antigas não pode impedir o Hub de abrir: os dados já estão copiados.
    if let Err(error) = contract(c) {
        tracing::error!(%error, "não foi possível apagar as tabelas antigas de seções; será tentado na próxima abertura");
    }
    Ok(())
}
/// Visível ao módulo pai para os testes simularem a abertura da versão N.
pub(super) fn copy(c: &Connection) -> Result<()> {
    c.execute_batch("BEGIN IMMEDIATE")?;
    let result = copy_in_transaction(c);
    match result {
        Ok(()) => {
            if let Err(error) = c.execute_batch("COMMIT") {
                let _ = c.execute_batch("ROLLBACK");
                Err(error.into())
            } else {
                Ok(())
            }
        }
        Err(e) => {
            c.execute_batch("ROLLBACK")?;
            Err(e)
        }
    }
}
fn copy_in_transaction(c: &Connection) -> Result<()> {
    let done: bool =
        c.query_row("SELECT EXISTS(SELECT 1 FROM kv WHERE k='bot_groups.migrated' AND v='1')", [], |r| r.get(0))?;
    if done {
        return Ok(());
    }
    let exists: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='donna_bot_sections')",
        [],
        |r| r.get(0),
    )?;
    if exists {
        let mut st = c.prepare("PRAGMA table_info(donna_bot_sections)")?;
        let cols = st.query_map([], |r| r.get::<_, String>(1))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let sql = if cols.iter().any(|x| x == "space_id") {
            "SELECT id,name,COALESCE(space_id,?1) FROM donna_bot_sections ORDER BY COALESCE(space_id,?1),position,id"
        } else {
            "SELECT id,name,?1 FROM donna_bot_sections ORDER BY position,id"
        };
        let mut st = c.prepare(sql)?;
        let rows = st
            .query_map([PERSONAL], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let spaces = crate::fork::spaces::store::read(c)?.spaces.into_iter().map(|s| s.id).collect::<HashSet<_>>();
        let mut seen: HashMap<String, HashSet<String>> = HashMap::new();
        let mut reserved: HashMap<String, HashSet<String>> = HashMap::new();
        for (_, name, space) in &rows {
            if spaces.contains(space) {
                reserved.entry(space.clone()).or_default().insert(key(if name.is_empty() { "Grupo" } else { name }));
            }
        }
        let mut ord: HashMap<String, i64> = HashMap::new();
        for (id, original, space) in rows {
            if !spaces.contains(&space) {
                continue;
            }
            let base = if original.is_empty() { "Grupo" } else { &original };
            let names = seen.entry(space.clone()).or_default();
            let mut name = base.to_owned();
            let mut suffix = 2;
            while names.contains(&key(&name)) {
                let tail = format!(" ({suffix})");
                name = format!(
                    "{}{tail}",
                    base.chars().take(40usize.saturating_sub(tail.chars().count())).collect::<String>()
                );
                suffix += 1;
                while reserved.get(&space).is_some_and(|existing| existing.contains(&key(&name))) {
                    let tail = format!(" ({suffix})");
                    name = format!(
                        "{}{tail}",
                        base.chars().take(40usize.saturating_sub(tail.chars().count())).collect::<String>()
                    );
                    suffix += 1;
                }
            }
            names.insert(key(&name));
            let n = ord.entry(space.clone()).or_default();
            c.execute(
                "INSERT INTO bot_groups(id,space_id,name,ord) VALUES(?1,?2,?3,?4)",
                params![id, space, name, *n],
            )?;
            *n += 1;
        }
        let assignments: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='donna_bot_section_assignments')",
            [],
            |r| r.get(0),
        )?;
        if assignments {
            c.execute("INSERT INTO bot_group_members(bot_id,group_id) SELECT a.bot_id,g.id FROM donna_bot_section_assignments a JOIN bot_groups g ON g.id=a.section_id JOIN bots b ON b.id=a.bot_id WHERE b.deleted=0 AND json_valid(b.config) AND COALESCE(json_extract(b.config,'$.spaceId'),?1)=g.space_id",[PERSONAL])?;
        }
        for space in spaces {
            let old = if space == PERSONAL {
                "donna.botSections.rev".to_owned()
            } else {
                format!("donna.botSections.rev.{space}")
            };
            let new = format!("bot_groups.rev.{space}");
            let prior: Option<i64> =
                c.query_row("SELECT CAST(v AS INTEGER) FROM kv WHERE k=?1", [&old], |r| r.get(0)).optional()?;
            if let Some(rev) = prior {
                c.execute("INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=MAX(CAST(v AS INTEGER),CAST(excluded.v AS INTEGER))",params![new,rev.to_string()])?;
            }
        }
        // A revisão de um snapshot nunca pode ficar à frente do contador que
        // será usado para a próxima edição, inclusive após importar um backup.
        c.execute("INSERT INTO kv(k,v) VALUES('rev',COALESCE((SELECT MAX(CAST(v AS INTEGER)) FROM kv WHERE k GLOB 'bot_groups.rev.*'),0)) ON CONFLICT(k) DO UPDATE SET v=MAX(CAST(v AS INTEGER),CAST(excluded.v AS INTEGER))", [])?;
    }
    c.execute("INSERT INTO kv(k,v) VALUES('bot_groups.migrated','1') ON CONFLICT(k) DO UPDATE SET v='1'", [])?;
    Ok(())
}
