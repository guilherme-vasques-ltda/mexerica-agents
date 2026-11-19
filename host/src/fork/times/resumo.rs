//! MB-3: projeção mínima do quadro do Pessoal para o resumo do dia. Mesma leitura do quadro
//! (`store::snapshot`) na Connection recebida; não grava nem muda revisão.
use super::model::ReportsTo;
use anyhow::Result;
use rusqlite::Connection;

pub(crate) struct Elo {
    pub bot_id: String,
    pub name: String,
    pub team_id: Option<String>,
    /// Bot de cima; `None` = reporta a Você.
    pub up: Option<String>,
}

pub(crate) struct Area {
    pub id: String,
    pub name: String,
    pub manager: Option<String>,
}

pub(crate) struct Quadro {
    pub elos: Vec<Elo>,
    pub areas: Vec<Area>,
}

pub(crate) fn quadro(c: &Connection, space: &str) -> Result<Quadro> {
    let snapshot = super::store::snapshot(c, space)?;
    let name = |id: &str| snapshot.bots.iter().find(|b| b.id == id).map(|b| b.name.clone());
    let elos = snapshot
        .graph
        .members
        .iter()
        .filter_map(|m| {
            Some(Elo {
                name: name(&m.bot_id)?,
                bot_id: m.bot_id.clone(),
                team_id: m.team_id.clone(),
                up: match &m.reports_to {
                    ReportsTo::Owner {} => None,
                    ReportsTo::Bot { bot_id } => Some(bot_id.clone()),
                },
            })
        })
        .collect();
    let mut nodes = snapshot.graph.nodes;
    nodes.sort_by(|a, b| (a.position, &a.name).cmp(&(b.position, &b.name)));
    let areas = nodes.into_iter().map(|n| Area { id: n.id, name: n.name, manager: n.manager_bot_id }).collect();
    Ok(Quadro { elos, areas })
}

#[cfg(test)]
pub(crate) fn resumo_testes_snapshot(c: &Connection, space: &str) -> serde_json::Value {
    serde_json::to_value(super::store::snapshot(c, space).unwrap()).unwrap()
}

#[cfg(test)]
pub(crate) fn resumo_testes_change(c: &Connection, request: serde_json::Value) {
    let change: super::model::Change = serde_json::from_value(request).unwrap();
    let tx = c.unchecked_transaction().unwrap();
    super::store::change(&tx, &change, true).unwrap();
    tx.commit().unwrap();
}
