use super::{
    model::{Graph, Member, OwnerContact, ReportsTo, Role},
    source::SourceScope,
};
use anyhow::{Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const MAX_NODES: usize = 128;
pub(super) const MAX_MEMBERS: usize = 512;
pub(super) const MAX_ROUTES: usize = 2048;
const MAX_DEPTH: usize = 8;

pub(super) fn id(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty() && value.len() <= 128 && !value.chars().any(|c| c.is_control() || c.is_whitespace()),
        "invalid_field: identificador inválido (máximo de 128 bytes)."
    );
    Ok(())
}

pub(super) fn uuid(value: &str) -> Result<()> {
    let parsed = uuid::Uuid::parse_str(value).ok();
    ensure!(
        parsed.is_some_and(|id| !id.is_nil() && id.to_string() == value),
        "invalid_field: use um UUID canônico, minúsculo e não nulo."
    );
    Ok(())
}

pub(super) fn limits(graph: &Graph) -> Result<()> {
    ensure!(
        graph.nodes.len() <= MAX_NODES && graph.members.len() <= MAX_MEMBERS && graph.routes.len() <= MAX_ROUTES,
        "times_limit: limite de 128 áreas, 512 bots ou 2048 rotas excedido."
    );
    Ok(())
}

/// Valida o estado final completo, nunca uma sequência de patches parcialmente válidos.
pub(super) fn graph(graph: &Graph, source: &SourceScope) -> Result<()> {
    limits(graph)?;
    let nodes: BTreeMap<_, _> = graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let members: BTreeMap<_, _> = graph.members.iter().map(|m| (m.bot_id.as_str(), m)).collect();
    ensure!(
        nodes.len() == graph.nodes.len() && members.len() == graph.members.len(),
        "invalid_field: ID repetido no quadro."
    );
    let roster: BTreeSet<_> = source.bots.iter().map(|b| b.id.as_str()).collect();
    ensure!(
        members.keys().copied().collect::<BTreeSet<_>>() == roster,
        "bot_catalog_conflict: inclua todos e somente os bots atuais deste Space; canais e bots apagados não são aceitos."
    );
    let manager = |id: &str| -> Result<()> {
        super::validation::id(id)?;
        ensure!(
            members.get(id).is_some_and(|m| m.role == Role::Manager),
            "manager_unavailable: escolha um gerente atual deste Space ou realoque seus dependentes."
        );
        Ok(())
    };
    let mut areas = BTreeMap::<String, Vec<String>>::new();
    let mut structural = BTreeMap::<String, Vec<String>>::new();
    let mut reports = BTreeMap::<String, Vec<String>>::new();
    for node in &graph.nodes {
        uuid(&node.id)?;
        ensure!(
            !node.name.is_empty()
                && node.name == node.name.trim()
                && node.name.chars().count() <= 80
                && !node.name.chars().any(char::is_control),
            "invalid_field: nome da área deve ter de 1 a 80 caracteres, sem controles ou espaços nas pontas."
        );
        ensure!((0..=1_000_000).contains(&node.position), "invalid_field: posição fora do limite.");
        let edges = areas.entry(area(&node.id)).or_default();
        if let Some(parent) = &node.parent_id {
            ensure!(
                nodes.contains_key(parent.as_str()),
                "team_not_found: realoque as subáreas antes de remover a área."
            );
            edges.push(area(parent));
        }
        structural.insert(area(&node.id), edges.clone());
        if let Some(id) = &node.manager_bot_id {
            manager(id)?;
        }
    }
    for member in &graph.members {
        id(&member.bot_id)?;
        let edges = structural.entry(bot(&member.bot_id)).or_default();
        if let Some(team) = &member.team_id {
            ensure!(nodes.contains_key(team.as_str()), "team_not_found: realoque os bots antes de remover a área.");
            edges.push(area(team));
        }
        if let Some(parent) = &member.parent_bot_id {
            ensure!(
                members.contains_key(parent.as_str()),
                "bot_not_found: realoque os subbots antes de remover seu pai."
            );
            edges.push(bot(parent));
        }
        let report_edges = reports.entry(bot(&member.bot_id)).or_default();
        match &member.reports_to {
            ReportsTo::Owner {} => ensure!(
                member.owner_contact == OwnerContact::Reports,
                "invalid_contact: Reporta a Você exige contato Relatórios."
            ),
            ReportsTo::Bot { bot_id } => {
                manager(bot_id)?;
                report_edges.push(bot(bot_id));
            }
        }
        if source.primary_bot_id.as_deref() == Some(&member.bot_id) {
            ensure!(
                *member == Member::initial(member.bot_id.clone(), true),
                "primary_protected: o bot principal atual é raiz, gerente e reporta a Você; releia o catálogo."
            );
        }
    }
    let mut routes = BTreeSet::new();
    for route in &graph.routes {
        ensure!(
            route.from_bot_id != route.to_bot_id
                && members.contains_key(route.from_bot_id.as_str())
                && members.contains_key(route.to_bot_id.as_str())
                && routes.insert((&route.from_bot_id, &route.to_bot_id)),
            "invalid_route: rota deve ligar dois bots distintos deste Space, sem repetição."
        );
    }
    acyclic(&areas, "area_cycle")?;
    acyclic(&structural, "subbot_cycle")?;
    acyclic(&reports, "report_cycle")?;
    // Dois DAGs isolados podem esconder A abaixo de B com B reportando a A.
    for (id, edges) in reports {
        structural.entry(id).or_default().extend(edges);
    }
    acyclic(&structural, "compound_cycle")?;
    Ok(())
}

fn area(id: &str) -> String {
    format!("area:{id}")
}

fn bot(id: &str) -> String {
    format!("bot:{id}")
}

fn acyclic(edges: &BTreeMap<String, Vec<String>>, code: &str) -> Result<()> {
    fn visit<'a>(
        id: &'a str,
        edges: &'a BTreeMap<String, Vec<String>>,
        path: &mut BTreeSet<&'a str>,
        heights: &mut BTreeMap<&'a str, usize>,
        code: &str,
    ) -> Result<usize> {
        ensure!(!path.contains(id), "{code}: ligação cria um ciclo; revise o quadro.");
        if let Some(height) = heights.get(id) {
            return Ok(*height);
        }
        ensure!(path.len() < MAX_DEPTH, "times_depth: limite de oito níveis excedido.");
        path.insert(id);
        let mut height = 1;
        if let Some(parents) = edges.get(id) {
            for parent in parents {
                height = height.max(1 + visit(parent, edges, path, heights, code)?);
            }
        }
        path.remove(id);
        ensure!(height <= MAX_DEPTH, "times_depth: limite de oito níveis excedido.");
        heights.insert(id, height);
        Ok(height)
    }
    let mut heights = BTreeMap::new();
    for id in edges.keys() {
        visit(id, edges, &mut BTreeSet::new(), &mut heights, code)?;
    }
    Ok(())
}
