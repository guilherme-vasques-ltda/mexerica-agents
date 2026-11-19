//! Private external-event consent record. It is not an event admission authority.
use super::{read, routine_revision, write};
use crate::{
    routines::{Routine, Trigger},
    store::Entry,
};
use anyhow::{Result, ensure};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Contract {
    repository: String,
    event: String,
    action: Action,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum Action {
    Notify { text: String },
    Task { title: String, description: String },
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Grant {
    version: u8,
    #[serde(rename = "grantId")]
    id: String,
    origin: String,
    space_id: String,
    bot_id: String,
    routine_id: String,
    entry_id: String,
    reviewed_entry_revision: i64,
    authority_revision: String,
    contract: Contract,
}
fn key(id: &str) -> String {
    format!("donna.routine-external-grant.v1.{}", crate::agent::registry::sha256_hex(id.as_bytes()))
}
/// Only fields defining authority; scheduler state, activation and errors are not authority.
pub(crate) fn authority_revision(r: &Routine) -> Result<String> {
    Ok(crate::agent::registry::sha256_hex(&serde_json::to_vec(&json!({
        "version":1,"id":r.id,"botId":r.bot_id,"instruction":r.instruction,
        "triggers":r.triggers,"timeoutSeconds":r.timeout_seconds,
        "deleted":r.deleted,"externalCreation":r.external_creation
    }))?))
}
fn valid_text(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && text.len() <= max
}
fn part_valid(p: &str) -> bool {
    valid_text(p, 100)
        && !p.starts_with('-')
        && p != "."
        && p != ".."
        && p.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}
pub(crate) fn record(
    c: &Connection,
    space: &str,
    entry: &Entry,
    routine: &Routine,
    expected_revision: Option<&str>,
    contract: Option<&Contract>,
) -> Result<()> {
    let contract = contract.ok_or_else(|| anyhow::anyhow!("routine_external_contract_required"))?;
    ensure!(expected_revision == Some(routine_revision(routine)?.as_str()), "routine_review_conflict");
    let parts: Vec<_> = contract.repository.split('/').collect();
    ensure!(parts.len() == 2 && parts.iter().all(|p| part_valid(p)), "routine_external_contract_invalid");
    ensure!(valid_text(&contract.event, 200) && !contract.event.contains('*'), "routine_external_contract_invalid");
    match &contract.action {
        Action::Notify { text } => ensure!(valid_text(text, 4000), "routine_external_contract_invalid"),
        Action::Task { title, description } => {
            ensure!(valid_text(title, 200) && valid_text(description, 4000), "routine_external_contract_invalid");
        }
    }
    ensure!(
        routine.triggers.len() == 1
            && matches!(&routine.triggers[0], Trigger::Event{source,event,filters} if source=="github" && event==&contract.event && filters.len()==1 && filters.get("repo").is_some_and(|r| r.eq_ignore_ascii_case(&contract.repository))),
        "routine_external_contract_invalid"
    );
    let revision = authority_revision(routine)?;
    let k = key(&routine.id);
    if let Some(raw) = read(c, &k)? {
        let old: Grant = serde_json::from_str(&raw)?;
        ensure!(
            old.version == 1
                && old.routine_id == routine.id
                && old.bot_id == routine.bot_id
                && old.space_id == space
                && old.authority_revision != revision,
            "routine_external_grant_exists"
        );
    }
    let mut normalized: Contract = serde_json::from_value(serde_json::to_value(contract)?)?;
    normalized.repository.make_ascii_lowercase();
    write(
        c,
        &k,
        &Grant {
            version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            origin: "localTransport".into(),
            space_id: space.into(),
            bot_id: routine.bot_id.clone(),
            routine_id: routine.id.clone(),
            entry_id: entry.id.clone(),
            reviewed_entry_revision: entry.rev,
            authority_revision: revision,
            contract: normalized,
        },
    )
}
pub(crate) fn revoke(c: &Connection, space: &str, routine: &Routine) -> Result<()> {
    let k = key(&routine.id);
    let raw = read(c, &k)?.ok_or_else(|| anyhow::anyhow!("routine_external_grant_missing"))?;
    let grant: Grant = serde_json::from_str(&raw)?;
    ensure!(
        grant.version == 1
            && grant.space_id == space
            && grant.bot_id == routine.bot_id
            && grant.routine_id == routine.id,
        "routine_external_grant_invalid"
    );
    c.execute("DELETE FROM kv WHERE k=?1", [k])?;
    Ok(())
}
/// Called in the same transaction as routine, bot and Space deletion.
pub(crate) fn prune(c: &Connection, raw: &str) -> Result<()> {
    let rows = c
        .prepare("SELECT k,v FROM kv WHERE k GLOB 'donna.routine-external-grant.v1.*'")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if rows.is_empty() {
        return Ok(());
    }
    let state: serde_json::Value = serde_json::from_str(raw)?;
    let routines = match state.get("routines") {
        None => &[][..],
        Some(value) => value.as_array().ok_or_else(|| anyhow::anyhow!("routine_state_invalid"))?.as_slice(),
    };
    for (k, v) in rows {
        let Ok(g) = serde_json::from_str::<Grant>(&v) else {
            tracing::warn!("unreadable external routine grant removed");
            c.execute("DELETE FROM kv WHERE k=?1", [&k])?;
            continue;
        };
        let current =
            routines.iter().find(|r| r["id"] == g.routine_id && r["botId"] == g.bot_id && r["deleted"] == false);
        let valid = match current {
            Some(value) => serde_json::from_value::<Routine>(value.clone())
                .ok()
                .and_then(|r| authority_revision(&r).ok())
                .is_some_and(|rev| rev == g.authority_revision),
            None => false,
        };
        if !valid {
            c.execute("DELETE FROM kv WHERE k=?1", [k])?;
        }
    }
    Ok(())
}
pub(crate) fn delete_bot(c: &Connection, bot: &str) -> Result<()> {
    let rows = c
        .prepare("SELECT k,v FROM kv WHERE k GLOB 'donna.routine-external-grant.v1.*'")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (k, v) in rows {
        let Ok(g) = serde_json::from_str::<Grant>(&v) else {
            tracing::warn!("unreadable external routine grant removed");
            c.execute("DELETE FROM kv WHERE k=?1", [&k])?;
            continue;
        };
        if g.bot_id == bot {
            c.execute("DELETE FROM kv WHERE k=?1", params![k])?;
        }
    }
    Ok(())
}
