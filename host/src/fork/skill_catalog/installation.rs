//! Identidade e DTO RPC locais ao catálogo. Não participa do espelho nem de execução.
use super::{Receipt, State, bounded_read, dir, hash, installed_receipt};
use crate::{market::Skill, store::Store};
use anyhow::{Result, anyhow, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;

pub(super) const UNSUPPORTED: &str = "skill_installation_unsupported";
pub(super) const STALE: &str = "skill_installation_stale";
pub(super) const CHANGED: &str = "skill_installation_changed";
pub(super) const UNAVAILABLE: &str = "skill_installation_unavailable";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Version {
    id: String,
    record_digest: String,
    body_digest: Option<String>,
    /// d1: emitida somente pelo salvamento manual humano, nunca por migração.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    manual_revision: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct InstalledSkillDto<'a> {
    id: &'a str,
    name: &'a str,
    description: &'a str,
    source: &'a str,
    path: String,
    receipt: Option<&'a Receipt>,
    installation_id: &'a str,
    content_status: ContentStatus,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum ContentStatus {
    Verified,
    Changed,
    Unavailable,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DetailsDto<'a> {
    pub skill_protocol: u8,
    pub markdown: String,
    pub catalog: Option<Value>,
    pub receipt: Option<&'a Receipt>,
    pub installation: Option<InstalledSkillDto<'a>>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(super) enum ExpectedReceipt {
    None {},
    Catalog {
        #[serde(rename = "catalogId")]
        catalog_id: String,
        digest: String,
    },
}
impl ExpectedReceipt {
    pub(super) fn matches(&self, actual: Option<&Receipt>) -> bool {
        match (self, actual) {
            (Self::None {}, None) => true,
            (Self::Catalog { catalog_id, digest }, Some(r)) => *catalog_id == r.catalog_id && *digest == r.digest,
            _ => false,
        }
    }
    pub(super) fn valid(&self) -> bool {
        match self {
            Self::None {} => true,
            Self::Catalog { catalog_id, digest } => {
                !catalog_id.is_empty()
                    && digest.len() == 64
                    && digest.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            }
        }
    }
}

pub(super) fn valid_id(id: &str) -> bool {
    id.split_once(':').is_some_and(|(namespace, sequence)| {
        uuid::Uuid::parse_str(namespace).is_ok_and(|u| u.to_string() == namespace)
            && sequence.parse::<u64>().is_ok_and(|n| n > 0 && n.to_string() == sequence)
    })
}

fn record_digest(skill: &Skill, state: &State) -> Result<String> {
    Ok(hash(&serde_json::to_vec(&(skill, installed_receipt(state, &skill.id)))?))
}

pub(super) fn document(store: &Store, skill: &Skill) -> Result<String> {
    crate::fork::agent_skills::validate_name(&skill.id, &skill.id).map_err(|_| anyhow!(UNAVAILABLE))?;
    ensure!(fs::symlink_metadata(dir(store, skill)).is_ok_and(|m| m.file_type().is_dir()), UNAVAILABLE);
    bounded_read(&dir(store, skill).join("SKILL.md"), 256 * 1024).map_err(|_| anyhow!(UNAVAILABLE))
}

pub(super) fn issue(skill: &Skill, state: &mut State) -> Result<()> {
    let namespace = state.installation_namespace.get_or_insert_with(|| uuid::Uuid::new_v4().to_string()).clone();
    ensure!(uuid::Uuid::parse_str(&namespace).is_ok_and(|u| u.to_string() == namespace), UNAVAILABLE);
    let next = state.installation_sequence.checked_add(1).ok_or_else(|| anyhow!(UNAVAILABLE))?;
    let record_digest = record_digest(skill, state)?;
    // R7: só a expectativa anterior do MESMO registro sobrevive à reemissão.
    // Ausência, migração ou registro incompatível nunca adotam bytes do filesystem.
    let previous = state.installations.get(&skill.id).filter(|v| v.record_digest == record_digest);
    let curated = installed_receipt(state, &skill.id);
    let version = Version {
        id: format!("{namespace}:{next}"),
        record_digest,
        // Um recibo já fixa os bytes: migrar não pode legitimar uma adulteração anterior.
        body_digest: match curated {
            Some(r) => r.files.iter().find(|file| file.path == "SKILL.md").map(|file| file.sha256.clone()),
            None => previous.and_then(|v| v.body_digest.clone()),
        },
        manual_revision: if curated.is_none() { previous.and_then(|v| v.manual_revision) } else { None },
    };
    state.installation_sequence = next;
    state.installations.insert(skill.id.clone(), version);
    Ok(())
}

/// O único emissor de revisão inicial recebe o digest dos bytes canônicos já validados.
/// O arquivo publicado é conferido depois; nunca é usado para fabricar a expectativa.
pub(super) fn issue_manual(skill: &Skill, state: &mut State, digest: String) -> Result<()> {
    ensure!(skill.source == "custom" && installed_receipt(state, &skill.id).is_none(), UNAVAILABLE);
    issue(skill, state)?;
    let version = state.installations.get_mut(&skill.id).ok_or_else(|| anyhow!(UNAVAILABLE))?;
    version.body_digest = Some(digest);
    version.manual_revision = Some(1);
    Ok(())
}

/// Somente o dono storage migra/reconcilia as duas keys sob a fronteira do catálogo.
pub(super) fn load_initialized(store: &Store) -> Result<(Vec<Skill>, State)> {
    super::storage::load_initialized(store)
}

pub(super) fn validate_metadata(state: &State) -> Result<()> {
    let Some(namespace) = &state.installation_namespace else {
        ensure!(state.installation_sequence == 0 && state.installations.is_empty(), UNAVAILABLE);
        return Ok(());
    };
    ensure!(uuid::Uuid::parse_str(namespace).is_ok_and(|id| id.to_string() == *namespace), UNAVAILABLE);
    let digest = |text: &str| {
        text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    };
    for version in state.installations.values() {
        ensure!(valid_id(&version.id), UNAVAILABLE);
        let (origin, counter) = version.id.split_once(':').ok_or_else(|| anyhow!(UNAVAILABLE))?;
        ensure!(origin == namespace && counter.parse::<u64>()? <= state.installation_sequence, UNAVAILABLE);
        ensure!(digest(&version.record_digest) && version.body_digest.as_deref().is_none_or(digest), UNAVAILABLE);
        ensure!(
            version.manual_revision.is_none_or(|revision| revision == 1 && version.body_digest.is_some()),
            UNAVAILABLE
        );
    }
    Ok(())
}

/// Registro e recibo ainda são os da instalação emitida. O corpo fica com `status`.
fn record_matches(skill: &Skill, state: &State) -> Result<bool> {
    let version = state.installations.get(&skill.id).ok_or_else(|| anyhow!(UNAVAILABLE))?;
    ensure!(valid_id(&version.id), UNAVAILABLE);
    Ok(version.record_digest == record_digest(skill, state)?)
}

/// D306: remover/reinstalar a versão vista não lê o corpo, mas exige o registro emitido.
pub(super) fn record_intact(skill: &Skill, state: &State) -> Result<()> {
    ensure!(record_matches(skill, state)?, CHANGED);
    Ok(())
}

fn status(skill: &Skill, state: &State, md: Option<&str>) -> Result<ContentStatus> {
    if !record_matches(skill, state)? {
        return Ok(ContentStatus::Changed);
    }
    let version = state.installations.get(&skill.id).ok_or_else(|| anyhow!(UNAVAILABLE))?;
    match (&version.body_digest, md) {
        (Some(expected), Some(md)) if *expected == hash(md.as_bytes()) => Ok(ContentStatus::Verified),
        (Some(_), Some(_)) => Ok(ContentStatus::Changed),
        _ => Ok(ContentStatus::Unavailable),
    }
}

pub(super) fn checked_document(store: &Store, skill: &Skill, state: &State) -> Result<String> {
    let md = document(store, skill)?;
    match status(skill, state, Some(&md))? {
        ContentStatus::Verified => Ok(md),
        ContentStatus::Changed => bail!(CHANGED),
        ContentStatus::Unavailable => bail!(UNAVAILABLE),
    }
}

pub(super) fn public<'a>(store: &Store, skill: &'a Skill, state: &'a State) -> Result<InstalledSkillDto<'a>> {
    let md = document(store, skill).ok();
    project(store, skill, state, md.as_deref())
}

pub(super) fn project<'a>(
    store: &Store,
    skill: &'a Skill,
    state: &'a State,
    md: Option<&str>,
) -> Result<InstalledSkillDto<'a>> {
    Ok(InstalledSkillDto {
        id: &skill.id,
        name: &skill.name,
        description: &skill.description,
        source: &skill.source,
        path: dir(store, skill).join("SKILL.md").to_string_lossy().into_owned(),
        receipt: installed_receipt(state, &skill.id),
        installation_id: &state.installations.get(&skill.id).ok_or_else(|| anyhow!(UNAVAILABLE))?.id,
        content_status: status(skill, state, md)?,
    })
}

/// Versão fixa emitida pelo Hub (encarnação, registro e corpo); só o estado, sem FS.
/// `None` sem instalação ou sem digest do corpo: não há o que fixar.
pub(super) fn pin(state: &State, id: &str) -> Option<String> {
    let v = state.installations.get(id)?;
    Some(format!("{}:{}:{}", v.id, v.record_digest, v.body_digest.as_deref()?))
}

pub(super) fn matches_id(state: &State, id: &str, expected: &str) -> bool {
    state.installations.get(id).is_some_and(|v| v.id == expected)
}

/// d2a: só a revisão emitida com expectativa comprovada pode iniciar comparação.
pub(super) fn manual_base<'a>(state: &'a State, id: &str) -> Result<(u64, &'a str)> {
    let version = state.installations.get(id).ok_or_else(|| anyhow!(UNAVAILABLE))?;
    let revision = version.manual_revision.ok_or_else(|| anyhow!("skill_review_base_unknown"))?;
    let digest = version.body_digest.as_deref().ok_or_else(|| anyhow!(UNAVAILABLE))?;
    Ok((revision, digest))
}
