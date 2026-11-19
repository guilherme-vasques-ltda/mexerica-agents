//! D340: área com exatamente 1 membro direto e sem chefe informado → esse membro vira o chefe.
//! Determinística (só grafo final, grafo gravado e principal), para a prévia e o Salvar darem o mesmo
//! resultado e o replay do mesmo recibo continuar igual. Nunca rebaixa papel nem toca em chefe informado.
use super::{
    model::{Graph, ReportsTo, Role},
    source, store, validation,
};
use anyhow::Result;
use rusqlite::{Connection, params};

/// Conta só bots membros diretos (`teamId == node.id`); subáreas e seus membros não contam.
/// `before` é o grafo gravado: só ele diz se o chefe era membro da área que ficou vazia.
/// Grava só `managerBotId`/`role`; devolve as áreas que ganharam chefe (o Reporta a fica com o app).
pub(super) fn normalize_single_leader(graph: &mut Graph, before: &Graph, primary: Option<&str>) -> Vec<String> {
    let Graph { nodes, members, .. } = graph;
    let mut promote = Vec::new();
    for node in nodes.iter_mut() {
        let mut direct = members.iter().filter(|m| m.team_id.as_deref() == Some(node.id.as_str()));
        match (node.manager_bot_id.as_deref(), direct.next(), direct.next()) {
            (None, Some(only), None) if primary != Some(only.bot_id.as_str()) => {
                node.manager_bot_id = Some(only.bot_id.clone());
                promote.push((node.id.clone(), only.bot_id.clone()));
            }
            // O chefe era o membro e saiu, sem ninguém na área: a área fica sem chefe; o papel dele fica.
            (Some(chief), None, _)
                if before.members.iter().any(|m| m.bot_id == chief && m.team_id.as_deref() == Some(&node.id)) =>
            {
                node.manager_bot_id = None;
            }
            _ => {}
        }
    }
    for member in members.iter_mut().filter(|m| promote.iter().any(|(_, id)| *id == m.bot_id)) {
        member.role = Role::Manager;
    }
    promote.into_iter().map(|(node, _)| node).collect()
}

fn depth(graph: &Graph, id: &str) -> usize {
    let mut at = graph.nodes.iter().find(|n| n.id == id);
    let mut depth = 0;
    while let Some(parent) = at.and_then(|n| n.parent_id.as_deref()) {
        depth += 1;
        at = graph.nodes.iter().find(|n| n.id == parent);
        if depth > validation::MAX_NODES {
            break;
        }
    }
    depth
}

/// Só na migração, de cima para baixo: o chefe promovido que reporta a Você passa a reportar ao chefe da
/// área de cima, se houver e se o quadro continuar válido (sem círculo). Quem já reporta a um bot não muda.
fn report_up(graph: &mut Graph, mut areas: Vec<String>, scope: &source::SourceScope) {
    areas.sort_by_key(|id| depth(graph, id));
    for id in areas {
        let Some(node) = graph.nodes.iter().find(|n| n.id == id) else { continue };
        let (Some(chief), Some(parent)) = (node.manager_bot_id.clone(), node.parent_id.as_deref()) else { continue };
        let Some(up) = graph.nodes.iter().find(|n| n.id == parent).and_then(|n| n.manager_bot_id.clone()) else {
            continue;
        };
        let Some(at) = graph.members.iter().position(|m| m.bot_id == chief) else { continue };
        if up == chief || graph.members[at].reports_to != (ReportsTo::Owner {}) {
            continue;
        }
        let mut candidate = graph.clone();
        candidate.members[at].reports_to = ReportsTo::Bot { bot_id: up };
        let mut projected = candidate.clone();
        store::project(&mut projected, scope, false);
        if validation::graph(&projected, scope).is_ok() {
            *graph = candidate;
        }
    }
}

/// Migração única e idempotente (na abertura do Store): aplica a regra ao que já está gravado, em todos
/// os Spaces com quadro. Mudou → grava e avança a revisão como um Salvar; a 2ª execução não muda nada.
pub(super) fn migrate(c: &Connection) -> Result<()> {
    let tx = c.unchecked_transaction()?;
    let spaces = tx
        .prepare("SELECT space_id FROM donna_team_revisions ORDER BY space_id")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for space in spaces {
        let Ok(scope) = source::read(&tx, &space) else {
            tracing::warn!(space, "times: catálogo ilegível; regra do chefe único fica para o próximo Salvar");
            continue;
        };
        let mut graph = store::read_graph(&tx, &space)?;
        let before = graph.clone();
        let promoted = normalize_single_leader(&mut graph, &before, scope.primary_bot_id.as_deref());
        report_up(&mut graph, promoted, &scope);
        if graph != before {
            store::write_graph(&tx, &space, &graph)?;
            let rev = crate::store::next_rev(&tx)?;
            tx.execute("UPDATE donna_team_revisions SET revision=?2 WHERE space_id=?1", params![space, rev])?;
        }
    }
    tx.commit()?;
    Ok(())
}
