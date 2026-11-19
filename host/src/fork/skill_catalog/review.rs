//! d2a: comparação manual sem efeito, no mesmo catálogo. Não emite aprovação/recibo.
use super::{MUTATION, PROTOCOL, hash, installation, installed_receipt, registry, root_lock, storage};
use crate::{LockExt, fork::autoridade::Autoridade, hub::Hub, store::Store};
use anyhow::{Result, anyhow, ensure};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{Value, json};

const INVALID: &str = "skill_review_invalid";
const SCOPE: &str = "skill_review_scope_unavailable";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Request {
    id: String,
    review: Review,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Review {
    protocol: u8,
    expected_installation_id: String,
    proposal: Option<Proposal>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Proposal {
    expected_body_digest: String,
    markdown: String,
}

pub(super) fn request(body: &Value) -> Result<Request> {
    ensure!(body.get("review").and_then(|r| r.get("proposal")).is_none_or(Value::is_object), INVALID);
    let intent: Request = serde_json::from_value(body.clone()).map_err(|_| anyhow!(INVALID))?;
    ensure!(intent.review.protocol == 1 && installation::valid_id(&intent.review.expected_installation_id), INVALID);
    crate::fork::agent_skills::validate_name(&intent.id, &intent.id).map_err(|_| anyhow!(INVALID))?;
    if let Some(proposal) = &intent.review.proposal {
        let digest = &proposal.expected_body_digest;
        ensure!(
            digest.len() == 64 && digest.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
            INVALID
        );
        ensure!(proposal.markdown.len() <= registry::MAX_SKILL, INVALID);
        crate::fork::agent_skills::parse(&proposal.markdown, &intent.id).map_err(|_| anyhow!(INVALID))?;
    }
    Ok(intent)
}

/// A captura original ocorre antes dos locks. O callback só consulta a Connection recebida.
pub(super) fn examine(
    hub: &Hub,
    authority: &Autoridade,
    intent: &Request,
    revalidate: impl FnOnce() -> Result<()>,
) -> Result<Value> {
    let _guard = MUTATION.locked();
    let _file = root_lock(&hub.store)?;
    revalidate()?;
    let (all, state) = authority.with_db(hub, None, storage::current_on)?;
    let mut comparison = prepare(&hub.store, &all, &state, intent)?;
    authority.with_db(hub, None, |db| {
        // Catálogo/base foram lidos antes do arquivo; agora a mesma Connection do
        // alcance confere que são ainda aquele snapshot. Nenhum Store no callback.
        storage::unchanged(db, state.observed.as_ref().ok_or_else(|| anyhow!(installation::UNAVAILABLE))?)?;
        comparison["affectedBots"] = json!(affected(db, &intent.id)?);
        comparison["comparisonDigest"] = json!(hash(registry::canonical(&comparison)?.as_bytes()));
        Ok(json!({"skillProtocol":PROTOCOL,"review":comparison}))
    })
}

fn affected(db: &Connection, skill: &str) -> Result<Vec<Value>> {
    let mut query = db.prepare("SELECT id,config,donna_skills_version FROM bots WHERE deleted=0 ORDER BY id")?;
    let rows = query.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?))
    })?;
    let mut bots = Vec::new();
    for row in rows {
        let (id, config, version) = row?;
        // Perfil corrompido não pode ser descartado como se não contivesse vínculo.
        let cfg: crate::store::BotConfig = serde_json::from_str(&config).map_err(|_| anyhow!(SCOPE))?;
        if cfg.skills.iter().any(|id| id == skill) {
            ensure!(cfg.space_id == crate::fork::spaces::PERSONAL && cfg.id == id, SCOPE);
            let version = crate::fork::skill_selection::valid_version(version).ok_or_else(|| anyhow!(SCOPE))?;
            bots.push(json!({"id":id,"name":cfg.name,"selectionVersion":version}));
        }
    }
    Ok(bots)
}

/// Sem persistência, migração, reemissão ou reserva. Exame não autoriza commit futuro.
fn prepare(store: &Store, all: &[crate::market::Skill], state: &super::State, intent: &Request) -> Result<Value> {
    let skill = all.iter().find(|skill| skill.id == intent.id).ok_or_else(|| anyhow!(installation::STALE))?;
    ensure!(installation::matches_id(state, &intent.id, &intent.review.expected_installation_id), installation::STALE);
    ensure!(skill.source == "custom" && installed_receipt(state, &intent.id).is_none(), "skill_review_manual_only");
    let (revision, digest) = installation::manual_base(state, &intent.id)?;
    let base = installation::checked_document(store, skill, state)?;
    // d1 garante base parseável; não normalizar nem recapturar bytes de um legado.
    ensure!(base.len() <= registry::MAX_SKILL, "skill_review_base_unknown");
    crate::fork::agent_skills::parse(&base, &intent.id).map_err(|_| anyhow!("skill_review_base_unknown"))?;
    let proposal = intent
        .review
        .proposal
        .as_ref()
        .map(|proposal| -> Result<Value> {
            ensure!(proposal.expected_body_digest == digest, "skill_review_base_changed");
            let next = revision.checked_add(1).ok_or_else(|| anyhow!("skill_review_base_unknown"))?;
            Ok(json!({"revision":next,"bodyDigest":hash(proposal.markdown.as_bytes()),"markdown":proposal.markdown}))
        })
        .transpose()?;
    Ok(json!({
        "reviewProtocol":1,"effect":"none","id":skill.id,
        "installationId":intent.review.expected_installation_id,
        "base":{"revision":revision,"bodyDigest":digest,"markdown":base},
        "proposal":proposal
    }))
}
