//! DON-835: equipe e chefe por bot visível, sem expor o grafo administrativo.
use crate::{LockExt, store::Store};
use anyhow::Result;
use std::collections::{HashMap, HashSet};

pub(crate) fn consultar(store: &Store, space: &str) -> Result<HashMap<String, (Option<String>, bool)>> {
    let snapshot = {
        let c = store.db.locked();
        // Mesma projeção do Quadro, em Tx descartada: nada é gravado.
        let tx = c.unchecked_transaction()?;
        super::store::snapshot(&tx, space)?
    };
    let mut teams = HashMap::new();
    for bot in snapshot.bots.iter().filter(|b| !b.hidden) {
        let node = snapshot
            .graph
            .members
            .iter()
            .find(|m| m.bot_id == bot.id)
            .and_then(|m| m.team_id.as_ref())
            .and_then(|id| snapshot.graph.nodes.iter().find(|n| &n.id == id));
        let chief = node.is_some_and(|n| n.manager_bot_id.as_deref() == Some(bot.id.as_str()));
        let mut path = Vec::new();
        let mut current = node;
        let mut visited = HashSet::new();
        while let Some(n) = current {
            if !visited.insert(n.id.as_str()) {
                break;
            }
            path.push(n.name.as_str());
            current = n.parent_id.as_ref().and_then(|id| snapshot.graph.nodes.iter().find(|p| &p.id == id));
        }
        path.reverse();
        let team = (!path.is_empty()).then(|| path.join(" › "));
        teams.insert(bot.id.clone(), (team, chief));
    }
    Ok(teams)
}

/// Regras de delegação (D371): as áreas do bot, da mais específica para a mais geral, pela mesma
/// projeção do Quadro (a principal fica sem área). Chefe sem `teamId` conta pela área que chefia.
pub(crate) fn areas_do_bot(c: &rusqlite::Connection, space: &str, bot_id: &str) -> Result<Vec<(String, String)>> {
    // Tx descartada: nada é gravado.
    let tx = c.unchecked_transaction()?;
    let snapshot = super::store::snapshot(&tx, space)?;
    let nodes = &snapshot.graph.nodes;
    let own = snapshot.graph.members.iter().find(|m| m.bot_id == bot_id).and_then(|m| m.team_id.as_ref());
    let primary = snapshot.primary_bot_id.as_deref() == Some(bot_id);
    let start = own.or_else(|| {
        (!primary).then(|| nodes.iter().find(|n| n.manager_bot_id.as_deref() == Some(bot_id)).map(|n| &n.id)).flatten()
    });
    let mut out = Vec::new();
    let mut current = start.and_then(|id| nodes.iter().find(|n| &n.id == id));
    while let Some(n) = current {
        if out.iter().any(|(id, _)| id == &n.id) {
            break;
        }
        out.push((n.id.clone(), n.name.clone()));
        current = n.parent_id.as_ref().and_then(|id| nodes.iter().find(|p| &p.id == id));
    }
    Ok(out)
}
