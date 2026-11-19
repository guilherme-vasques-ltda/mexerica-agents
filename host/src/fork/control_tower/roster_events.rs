//! Destinatários efêmeros capturados antes de perder vínculo; nenhum corpo de bot.
use crate::{LockExt, hub::Hub};
use anyhow::Result;
use rusqlite::params;
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(crate) struct RosterChange {
    spaces: BTreeSet<String>,
}
impl RosterChange {
    pub(crate) fn publish(self, hub: &Hub) {
        for space in self.spaces {
            let _ = hub.events.send(json!({"type":super::EVENT,"spaceId":space,"rosterChanged":true}));
        }
    }
}

#[derive(Clone, Copy)]
enum Mutation {
    PrimaryChange,
    Deletion,
}

fn capture(hub: &Hub, bots: impl IntoIterator<Item = String>, mutation: Mutation) -> Result<RosterChange> {
    let c = hub.store.db.locked();
    let mut spaces = BTreeSet::new();
    for bot in bots {
        let affected = c
            .prepare(
                "SELECT space_id FROM donna_space_conversations WHERE bot_id=?1
             AND (?2 OR bot_id=(SELECT json_extract(v,'$.primaryBotId') FROM kv WHERE k='team.settings.v1'))
             UNION SELECT json_extract(config,'$.spaceId') FROM bots WHERE id=?1 AND deleted=0",
            )?
            .query_map(params![bot, matches!(mutation, Mutation::PrimaryChange)], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        spaces.extend(affected);
    }
    Ok(RosterChange { spaces })
}

/// Chamado sob a trava administrativa, antes de gravar a seleção nova.
pub(crate) fn primary_change(hub: &Hub, body: &Value) -> Result<RosterChange> {
    if body.get("primaryBotId").is_none() {
        return Ok(RosterChange { spaces: BTreeSet::new() });
    }
    let before = hub.store.team_settings()?.primary_bot_id;
    let after = body["primaryBotId"].as_str().map(str::to_owned);
    if before == after {
        return Ok(RosterChange { spaces: BTreeSet::new() });
    }
    capture(hub, before.into_iter().chain(after), Mutation::PrimaryChange)
}

/// A exclusão só afeta o Space nativo e as conversas estrangeiras da principal atual.
/// Vínculo apenas histórico de ex-principal não prova cartão no roster contextual.
pub(crate) fn deletion(hub: &Hub, bot: &str) -> Result<RosterChange> {
    capture(hub, [bot.to_owned()], Mutation::Deletion)
}
