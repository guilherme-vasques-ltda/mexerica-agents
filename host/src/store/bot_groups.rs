//! Persistência dos grupos e das suas atribuições, isoladas por Space.
pub(crate) mod legado;
#[cfg(test)]
#[path = "bot_groups/legado_exclusao_tests.rs"]
mod legado_exclusao_tests;
#[cfg(test)]
#[path = "bot_groups/nomes_tests.rs"]
mod nomes_tests;
#[cfg(test)]
#[path = "bot_groups/tests.rs"]
mod tests;

use crate::{LockExt, api::GroupError, store::Store};
use anyhow::Result;
use icu_properties::{
    CodePointSetData,
    props::{DefaultIgnorableCodePoint, Emoji, EmojiModifier, EmojiModifierBase, ExtendedPictographic},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use unicode_general_category::{GeneralCategory, get_general_category};
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};
use unicode_width::UnicodeWidthChar;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Group {
    pub id: String,
    pub name: String,
    pub bots: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Snapshot {
    pub rev: i64,
    pub groups: Vec<Group>,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub(crate) enum Edit {
    Add {
        name: String,
        #[serde(rename = "botId")]
        bot_id: Option<String>,
    },
    Rename {
        id: String,
        name: String,
    },
    Remove {
        id: String,
    },
    Place {
        #[serde(rename = "botId")]
        bot_id: String,
        #[serde(rename = "groupId")]
        group_id: Option<String>,
    },
}

fn failure(code: &'static str, message: &'static str) -> anyhow::Error {
    GroupError { code, message }.into()
}
fn missing_group() -> anyhow::Error {
    failure("group_missing", "Este grupo não existe mais.")
}
fn missing_bot() -> anyhow::Error {
    failure("bot_missing", "Este bot não existe mais neste Space.")
}
pub(crate) fn key(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .nfd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .collect()
}
fn valid_name(name: &str) -> Result<String> {
    let invisible = CodePointSetData::new::<DefaultIgnorableCodePoint>();
    let emoji = CodePointSetData::new::<Emoji>();
    let pictographic = CodePointSetData::new::<ExtendedPictographic>();
    let modifier = CodePointSetData::new::<EmojiModifier>();
    let modifier_base = CodePointSetData::new::<EmojiModifierBase>();
    let chars: Vec<char> = name.chars().collect();
    let mut i = 0;
    let mut visible = false;
    while i < chars.len() {
        let c = chars[i];
        if emoji.contains(c) && !modifier.contains(c) {
            visible = true;
            let mut base = c;
            i += 1;
            loop {
                if chars.get(i) == Some(&'\u{fe0f}') {
                    i += 1;
                }
                if chars.get(i).is_some_and(|ch| modifier.contains(*ch)) {
                    if !modifier_base.contains(base) {
                        return Err(failure("name_invalid", "Informe um nome de 1 a 40 caracteres visíveis."));
                    }
                    i += 1;
                }
                if base == '🏴'
                    && chars.get(i).is_some_and(|ch| {
                        ('\u{e0030}'..='\u{e0039}').contains(ch) || ('\u{e0061}'..='\u{e007a}').contains(ch)
                    })
                {
                    while chars.get(i).is_some_and(|ch| {
                        ('\u{e0030}'..='\u{e0039}').contains(ch) || ('\u{e0061}'..='\u{e007a}').contains(ch)
                    }) {
                        i += 1;
                    }
                    if chars.get(i) != Some(&'\u{e007f}') {
                        return Err(failure("name_invalid", "Informe um nome de 1 a 40 caracteres visíveis."));
                    }
                    i += 1;
                }
                if chars.get(i) != Some(&'\u{200d}') {
                    break;
                }
                if !pictographic.contains(base) {
                    return Err(failure("name_invalid", "Informe um nome de 1 a 40 caracteres visíveis."));
                }
                i += 1;
                let Some(next) =
                    chars.get(i).copied().filter(|ch| pictographic.contains(*ch) && !modifier.contains(*ch))
                else {
                    return Err(failure("name_invalid", "Informe um nome de 1 a 40 caracteres visíveis."));
                };
                base = next;
                i += 1;
            }
            continue;
        }
        if modifier.contains(c)
            || matches!(c, '\u{2800}' | '\u{3164}' | '\u{115f}' | '\u{1160}')
            || invisible.contains(c)
            || matches!(
                get_general_category(c),
                GeneralCategory::Control
                    | GeneralCategory::Format
                    | GeneralCategory::Surrogate
                    | GeneralCategory::PrivateUse
                    | GeneralCategory::Unassigned
                    | GeneralCategory::LineSeparator
                    | GeneralCategory::ParagraphSeparator
            )
        {
            return Err(failure("name_invalid", "Informe um nome de 1 a 40 caracteres visíveis."));
        }
        if !c.is_whitespace() && c.width().unwrap_or(0) > 0 {
            visible = true;
        }
        i += 1;
    }
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if !visible || name.is_empty() || name.chars().count() > 40 {
        return Err(failure("name_invalid", "Informe um nome de 1 a 40 caracteres visíveis."));
    }
    Ok(name)
}
fn migrated(c: &Connection) -> Result<bool> {
    Ok(c.query_row("SELECT v='1' FROM kv WHERE k='bot_groups.migrated'", [], |r| r.get(0)).optional()?.unwrap_or(false))
}
fn snapshot(c: &Connection, space: &str) -> Result<Snapshot> {
    crate::fork::spaces::require_space(c, space)?;
    if !migrated(c)? {
        return Ok(Snapshot { rev: 0, groups: vec![] });
    }
    let rev = c
        .query_row("SELECT CAST(v AS INTEGER) FROM kv WHERE k=?1", [format!("bot_groups.rev.{space}")], |r| r.get(0))
        .optional()?
        .unwrap_or(0);
    let mut stmt = c.prepare("SELECT id,name FROM bot_groups WHERE space_id=?1 ORDER BY ord,id")?;
    let rows = stmt
        .query_map([space], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut groups = Vec::new();
    for (id, name) in rows {
        let mut members = c.prepare("SELECT m.bot_id FROM bot_group_members m JOIN bots b ON b.id=m.bot_id WHERE m.group_id=?1 AND b.deleted=0 AND json_valid(b.config) AND COALESCE(json_extract(b.config,'$.spaceId'),?2)=?2 ORDER BY m.bot_id")?;
        let bots = members.query_map(params![id, space], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        groups.push(Group { id, name, bots });
    }
    Ok(Snapshot { rev, groups })
}
fn check_bot(c: &Connection, space: &str, bot: &str) -> Result<()> {
    let exists: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM bots WHERE id=?1 AND deleted=0 AND json_valid(config) AND COALESCE(json_extract(config,'$.spaceId'),?2)=?2)", params![bot, space], |r| r.get(0))?;
    if !exists {
        return Err(missing_bot());
    }
    Ok(())
}
fn check_group(c: &Connection, space: &str, id: &str) -> Result<()> {
    let exists: bool =
        c.query_row("SELECT EXISTS(SELECT 1 FROM bot_groups WHERE id=?1 AND space_id=?2)", params![id, space], |r| {
            r.get(0)
        })?;
    if !exists {
        return Err(missing_group());
    }
    Ok(())
}
fn check_name(c: &Connection, space: &str, id: Option<&str>, name: &str) -> Result<()> {
    let mut stmt = c.prepare("SELECT id,name FROM bot_groups WHERE space_id=?1")?;
    let rows = stmt.query_map([space], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    for row in rows {
        let (other, candidate) = row?;
        if Some(other.as_str()) != id && key(&candidate) == key(name) {
            return Err(failure("name_taken", "Já existe um grupo com esse nome neste Space."));
        }
    }
    Ok(())
}
fn renumber(c: &Connection, space: &str) -> Result<()> {
    let mut stmt = c.prepare("SELECT id FROM bot_groups WHERE space_id=?1 ORDER BY ord,id")?;
    let ids = stmt.query_map([space], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for (ord, id) in ids.iter().enumerate() {
        c.execute("UPDATE bot_groups SET ord=?1 WHERE id=?2", params![i64::try_from(ord)?, id])?;
    }
    Ok(())
}
impl Store {
    pub(crate) fn bot_group_space(&self, bot: &str) -> Result<Option<String>> {
        Ok(self
            .db
            .locked()
            .query_row(
                "SELECT g.space_id FROM bot_group_members m JOIN bot_groups g ON g.id=m.group_id WHERE m.bot_id=?1",
                [bot],
                |r| r.get(0),
            )
            .optional()?)
    }
    pub(crate) fn bot_groups_in_space(&self, space: &str) -> Result<Snapshot> {
        snapshot(&self.db.locked(), space)
    }
    pub(crate) fn edit_bot_groups(&self, space: &str, edit: Edit) -> Result<(Snapshot, Option<String>, Vec<String>)> {
        #[cfg(test)]
        crate::fork::sessoes::call_tests::before_effect(self, "bot_groups_write");
        crate::fork::donna_fixa::pedido::gravar(self, |tx| edit_bot_groups_on(tx, space, edit))
    }
}

fn edit_bot_groups_on(tx: &Connection, space: &str, edit: Edit) -> Result<(Snapshot, Option<String>, Vec<String>)> {
    crate::fork::spaces::require_space(tx, space)?;
    if !migrated(tx)? {
        return Err(failure("migrating", "Os grupos estão sendo atualizados."));
    }
    tx.execute("DELETE FROM bot_group_members WHERE group_id IN (SELECT id FROM bot_groups WHERE space_id=?1) AND NOT EXISTS (SELECT 1 FROM bots b WHERE b.id=bot_id AND b.deleted=0 AND json_valid(b.config) AND COALESCE(json_extract(b.config,'$.spaceId'),?1)=?1)", [space])?;
    let mut affected = Vec::new();
    let mut created = None;
    match edit {
        Edit::Add { name, bot_id } => {
            let name = valid_name(&name)?;
            check_name(tx, space, None, &name)?;
            if let Some(bot) = &bot_id {
                check_bot(tx, space, bot)?;
            }
            let id = uuid::Uuid::new_v4().to_string();
            tx.execute("INSERT INTO bot_groups(id,space_id,name,ord) VALUES(?1,?2,?3,(SELECT COUNT(*) FROM bot_groups WHERE space_id=?2))", params![id,space,name])?;
            if let Some(bot) = bot_id {
                tx.execute("INSERT INTO bot_group_members(bot_id,group_id) VALUES(?1,?2) ON CONFLICT(bot_id) DO UPDATE SET group_id=excluded.group_id", params![bot,id])?;
                affected.push(bot);
            }
            created = Some(id);
            renumber(tx, space)?;
        }
        Edit::Rename { id, name } => {
            check_group(tx, space, &id)?;
            let name = valid_name(&name)?;
            check_name(tx, space, Some(&id), &name)?;
            tx.execute("UPDATE bot_groups SET name=?1 WHERE id=?2", params![name, id])?;
        }
        Edit::Remove { id } => {
            check_group(tx, space, &id)?;
            let mut s = tx.prepare("SELECT bot_id FROM bot_group_members WHERE group_id=?1 ORDER BY bot_id")?;
            affected = s.query_map([&id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            drop(s);
            tx.execute("DELETE FROM bot_group_members WHERE group_id IN (SELECT id FROM bot_groups WHERE id=?1 AND space_id=?2)",params![id,space])?;
            tx.execute("DELETE FROM bot_groups WHERE id=?1 AND space_id=?2", params![id, space])?;
            renumber(tx, space)?;
        }
        Edit::Place { bot_id, group_id } => {
            check_bot(tx, space, &bot_id)?;
            if let Some(id) = group_id {
                check_group(tx, space, &id)?;
                tx.execute("INSERT INTO bot_group_members(bot_id,group_id) VALUES(?1,?2) ON CONFLICT(bot_id) DO UPDATE SET group_id=excluded.group_id",params![bot_id,id])?;
            } else {
                tx.execute("DELETE FROM bot_group_members WHERE bot_id=?1", [&bot_id])?;
            }
            affected.push(bot_id);
        }
    }
    for bot in &affected {
        let rev = super::next_rev(tx)?;
        tx.execute("UPDATE bots SET rev=?1 WHERE id=?2", params![rev, bot])?;
    }
    let rev = super::next_rev(tx)?;
    tx.execute(
        "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
        params![format!("bot_groups.rev.{space}"), rev.to_string()],
    )?;
    let state = snapshot(tx, space)?;
    Ok((state, created, affected))
}
pub(crate) fn forget_bot(c: &Connection, id: &str, _bot_rev: i64) -> Result<()> {
    let space: Option<String> = c
        .query_row(
            "SELECT g.space_id FROM bot_group_members m JOIN bot_groups g ON g.id=m.group_id WHERE m.bot_id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    c.execute("DELETE FROM bot_group_members WHERE bot_id=?1", [id])?;
    if let Some(space) = space {
        let rev = super::next_rev(c)?;
        c.execute(
            "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
            params![format!("bot_groups.rev.{space}"), rev.to_string()],
        )?;
    }
    Ok(())
}
