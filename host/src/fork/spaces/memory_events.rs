//! Invalidação de memória sem publicar conteúdo ou trocar a revisão de binding.
use crate::{LockExt, hub::Hub};
use anyhow::Result;
use rusqlite::params;

pub fn memory_changed(hub: &Hub, owner: Option<&str>) -> Result<()> {
    let source = owner.map(|id| hub.store.bot(id)).transpose()?.flatten();
    let contexts = {
        let db = hub.store.db.locked();
        let mut scopes = Vec::new();
        if let Some(owner) = owner {
            if let Some(bot) = source {
                scopes.push((bot.config.space_id, bot.config.id));
            }
            let keys = db
                .prepare("SELECT k FROM kv WHERE v=?1 AND k LIKE 'donna.space.memory.%'")?
                .query_map([owner], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for key in keys {
                if let Some((space, bot)) = key.strip_prefix("donna.space.memory.").and_then(|k| k.split_once('.')) {
                    scopes.push((space.to_owned(), bot.to_owned()));
                }
            }
        }
        let mut contexts = Vec::new();
        for (space, bot) in scopes {
            contexts.extend(
                db.prepare(
                    "SELECT id,space_id FROM donna_space_conversations WHERE space_id=?1 AND bot_id=?2 AND id<>bot_id",
                )?
                .query_map(params![space, bot], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?,
            );
        }
        if owner.is_none() {
            contexts.extend(
                db.prepare("SELECT id,space_id FROM donna_space_conversations WHERE space_id=?1 AND id<>bot_id")?
                    .query_map([super::PERSONAL], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?,
            );
        }
        contexts
    };
    for (id, space) in contexts {
        {
            let mut db = hub.store.db.locked();
            let tx = db.transaction()?;
            let rev = crate::store::next_rev(&tx)?;
            tx.execute("UPDATE donna_space_conversations SET state_revision=?2 WHERE id=?1", params![id, rev])?;
            tx.commit()?;
        }
        if let Ok(c) = hub.store.space_conversation(&space, &id) {
            super::emit_conversation(hub, &c);
        }
    }
    Ok(())
}
