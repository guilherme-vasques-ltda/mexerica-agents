use super::{
    lider,
    model::{Change, Graph, Member, Node, OwnerContact, ReportsTo, Role, Route, RouteState, Snapshot},
    source, validation,
};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

fn revision(c: &Connection, space: &str) -> Result<i64> {
    Ok(c.query_row("SELECT revision FROM donna_team_revisions WHERE space_id=?1", [space], |r| r.get::<_, i64>(0))?)
}

pub(super) fn read_graph(c: &Connection, space: &str) -> Result<Graph> {
    let nodes = c
        .prepare(
            "SELECT id,name,parent_id,manager_bot_id,position FROM donna_team_nodes WHERE space_id=?1 ORDER BY id",
        )?
        .query_map([space], |r| {
            Ok(Node {
                id: r.get(0)?,
                name: r.get(1)?,
                parent_id: r.get(2)?,
                manager_bot_id: r.get(3)?,
                position: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let members = c
        .prepare("SELECT bot_id,team_id,parent_bot_id,role,reports_to_bot_id,owner_contact FROM donna_bot_org WHERE space_id=?1 ORDER BY bot_id")?
        .query_map([space], |r| {
            let report: Option<String> = r.get(4)?;
            let role: String = r.get(3)?;
            let contact: String = r.get(5)?;
            Ok(Member {
                bot_id: r.get(0)?, team_id: r.get(1)?, parent_bot_id: r.get(2)?,
                role: if role == "manager" { Role::Manager } else { Role::Executor },
                reports_to: report.map_or(ReportsTo::Owner {}, |bot_id| ReportsTo::Bot { bot_id }),
                owner_contact: match contact.as_str() {
                    "urgent_only" => OwnerContact::UrgentOnly,
                    "on_request" => OwnerContact::OnRequest,
                    _ => OwnerContact::Reports,
                },
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let routes = c
        .prepare(
            "SELECT from_bot_id,to_bot_id FROM donna_team_routes WHERE space_id=?1 ORDER BY from_bot_id,to_bot_id",
        )?
        .query_map([space], |r| {
            Ok(Route { from_bot_id: r.get(0)?, to_bot_id: r.get(1)?, state: RouteState::Proposed })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Graph { nodes, members, routes })
}

/// Fonte e grafo são lidos sob a mesma transação. A projeção da principal nunca grava Org.
pub(super) fn snapshot(c: &Connection, space: &str) -> Result<Snapshot> {
    let source = source::read(c, space)?;
    let mut graph = read_graph(c, space)?;
    project(&mut graph, &source, true);
    let revision = c
        .query_row("SELECT revision FROM donna_team_revisions WHERE space_id=?1", [space], |r| r.get(0))
        .optional()?
        .unwrap_or(0);
    Ok(Snapshot {
        version: 1,
        space_id: space.into(),
        revision,
        catalog_version: source.catalog_version,
        primary_bot_id: source.primary_bot_id,
        bots: source.bots,
        graph,
    })
}

pub(super) fn project(graph: &mut Graph, source: &source::SourceScope, protect_primary: bool) {
    for bot in &source.bots {
        let primary = source.primary_bot_id.as_deref() == Some(bot.id.as_str());
        if let Some(member) = graph.members.iter_mut().find(|m| m.bot_id == bot.id) {
            if primary && protect_primary {
                *member = Member::initial(bot.id.clone(), true);
            }
        } else {
            graph.members.push(Member::initial(bot.id.clone(), primary));
        }
    }
    graph.sort();
}

pub(super) fn change(c: &Connection, request: &Change, save: bool) -> Result<(Value, bool)> {
    let space = request.space_id.as_deref().unwrap_or(crate::fork::spaces::PERSONAL);
    let mut graph = request.graph();
    validation::limits(&graph)?;
    graph.sort();
    ensure!(request.expected_revision >= 0, "invalid_field: revisão inválida.");
    let digest = source::digest(&json!({
        "spaceId":space,"expectedRevision":request.expected_revision,"catalogVersion":request.catalog_version,"graph":graph
    }))?;
    // A Connection já pertence à transação de Autoridade::with_db, que reconfere e confirma.
    let tx = c;
    // O recibo antecede o CAS e a validação do catálogo antigo. Retry não reaplica a intenção.
    if let Some(nonce) = &request.client_request_id {
        validation::uuid(nonce)?;
        let receipt: Option<(String, i64)> = tx
            .query_row(
                "SELECT digest,revision FROM donna_team_receipts WHERE space_id=?1 AND client_request_id=?2",
                params![space, nonce],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((before, revision)) = receipt {
            ensure!(
                before == digest,
                "request_conflict: identificador já usado para outra alteração; use uma nova intenção."
            );
            let snapshot = snapshot(tx, space)?;
            return Ok((json!({"snapshot":snapshot,"receipt":{"clientRequestId":nonce,"revision":revision}}), false));
        }
    }
    let mut current = snapshot(tx, space)?;
    ensure!(current.revision == request.expected_revision, "times_conflict: o quadro mudou; releia antes de salvar.");
    ensure!(
        current.catalog_version == request.catalog_version,
        "catalog_conflict: o bot principal ou os outros bots mudaram; releia e revise o rascunho antes de salvar."
    );
    let source = source::read(tx, space)?;
    // D340 sobre o estado final, antes de validar; o digest acima continua sendo o do pedido.
    let stored = read_graph(tx, space)?;
    lider::normalize_single_leader(&mut graph, &stored, source.primary_bot_id.as_deref());
    validation::graph(&graph, &source)?;
    if !save {
        current.graph = graph;
        let mut answer = serde_json::to_value(current)?;
        answer["preview"] = true.into();
        return Ok((answer, false));
    }
    tx.execute("INSERT OR IGNORE INTO donna_team_revisions(space_id,revision) VALUES(?1,0)", [space])?;
    let mut before = stored;
    project(&mut before, &source, false);
    let changed = before != graph;
    if changed {
        let rev = crate::store::next_rev(tx)?;
        write_graph(tx, space, &graph)?;
        let count = tx.execute(
            "UPDATE donna_team_revisions SET revision=?2 WHERE space_id=?1 AND revision=?3",
            params![space, rev, request.expected_revision],
        )?;
        ensure!(count == 1, "times_conflict: o quadro mudou; releia antes de salvar.");
    }
    let nonce = request
        .client_request_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("invalid_field: clientRequestId obrigatório."))?;
    let revision = revision(tx, space)?;
    tx.execute(
        "INSERT INTO donna_team_receipts(space_id,client_request_id,digest,revision) VALUES(?1,?2,?3,?4)",
        params![space, nonce, digest, revision],
    )?;
    let snapshot = snapshot(tx, space)?;
    let answer = json!({"snapshot":snapshot,"receipt":{"clientRequestId":nonce,"revision":revision}});
    Ok((answer, changed))
}

pub(super) fn write_graph(c: &Connection, space: &str, graph: &Graph) -> Result<()> {
    // O Store mantém foreign_keys desligado; a integridade vem da validação final e do guard de exclusão.
    c.execute("DELETE FROM donna_team_routes WHERE space_id=?1", [space])?;
    c.execute("DELETE FROM donna_bot_org WHERE space_id=?1", [space])?;
    c.execute("DELETE FROM donna_team_nodes WHERE space_id=?1", [space])?;
    for node in &graph.nodes {
        c.execute(
            "INSERT INTO donna_team_nodes(space_id,id,name,parent_id,manager_bot_id,position) VALUES(?1,?2,?3,?4,?5,?6)",
            params![space, node.id, node.name, node.parent_id, node.manager_bot_id, node.position],
        )?;
    }
    for member in &graph.members {
        let report = match &member.reports_to {
            ReportsTo::Owner {} => None,
            ReportsTo::Bot { bot_id } => Some(bot_id),
        };
        let role = match member.role {
            Role::Manager => "manager",
            Role::Executor => "executor",
        };
        let contact = match member.owner_contact {
            OwnerContact::Reports => "reports",
            OwnerContact::UrgentOnly => "urgent_only",
            OwnerContact::OnRequest => "on_request",
        };
        c.execute(
            "INSERT INTO donna_bot_org(space_id,bot_id,team_id,parent_bot_id,role,reports_to_bot_id,owner_contact) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![space, member.bot_id, member.team_id, member.parent_bot_id, role, report, contact],
        )?;
    }
    for route in &graph.routes {
        c.execute(
            "INSERT INTO donna_team_routes(space_id,from_bot_id,to_bot_id,state) VALUES(?1,?2,?3,'proposed')",
            params![space, route.from_bot_id, route.to_bot_id],
        )?;
    }
    Ok(())
}

pub(crate) fn check_bot_deletion(c: &Connection, id: &str) -> Result<()> {
    let dependent: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_team_nodes WHERE manager_bot_id=?1)
         OR EXISTS(SELECT 1 FROM donna_bot_org WHERE bot_id<>?1 AND (parent_bot_id=?1 OR reports_to_bot_id=?1))
         OR EXISTS(SELECT 1 FROM donna_team_routes WHERE from_bot_id=?1 OR to_bot_id=?1)",
        [id],
        |r| r.get(0),
    )?;
    ensure!(
        !dependent,
        "times_dependents: abra Equipes, realoque explicitamente as áreas, subbots e reportes e remova as rotas deste bot antes de excluí-lo."
    );
    Ok(())
}

/// Executado na transação legada de exclusão. Falha reverte inclusive principal/revisões.
pub(crate) fn on_bot_deleted(c: &Connection, id: &str) -> Result<()> {
    check_bot_deletion(c, id)?;
    let space: Option<String> =
        c.query_row("SELECT space_id FROM donna_bot_org WHERE bot_id=?1", [id], |r| r.get(0)).optional()?;
    if let Some(space) = space {
        c.execute("DELETE FROM donna_bot_org WHERE bot_id=?1", [id])?;
        let rev = crate::store::next_rev(c)?;
        c.execute("UPDATE donna_team_revisions SET revision=?2 WHERE space_id=?1", params![space, rev])?;
    }
    Ok(())
}
