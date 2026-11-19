//! D169: contextos persistidos do mesmo dono. Sem autoridade nova por seleção.
mod assets;
mod binding;
mod birth;
pub(crate) use birth::call as birth_authorized;
mod config_generation;
pub(crate) use config_generation::current_on as config_generation_on;
pub(crate) mod connector_cards;
pub(crate) mod connector_context;
pub(crate) mod connector_forward;
mod history_context;
mod memory_events;
mod model;
pub use memory_events::memory_changed;
mod resources;
mod uploads;
pub(crate) use binding::{current_memory_policy_on, execution_stamp_on, validate_dispatch_on};
pub use binding::{execution_stamp, validate_entries};
pub use history_context::{continuity, history_context, mark_context_sid, needs_history_rebuild};
pub(crate) mod notice;
mod principal;
mod runtime;
pub(crate) mod startup;
pub(crate) use principal::call as principal_authorized;
pub(crate) use principal::check_principal_save;
pub(crate) mod delete;
pub(crate) use delete::call as delete_authorized;
pub(crate) mod store;
pub(crate) mod tool_context;
pub(crate) mod tool_effects;
pub(crate) mod tool_servers;
pub use runtime::{
    ExecutionContext, bind_session, execution_config, inflight, prepare_workspace, resource_config, session_slot,
    stamp_entry,
};
mod scope;
mod skill_scope;
pub use model::*;
pub use scope::{chat_space, guard, validate_bot};
pub(crate) use skill_scope::PersonalSkillRequest;
pub(crate) use store::{is_space_principal, migrate, register_primary, require_space};
mod conversations;
pub(crate) use conversations::EngineSelection;
pub use conversations::legacy_control;
pub(crate) use conversations::{context as conversation_context, emit as emit_conversation, legacy_restart};
mod events;
#[cfg(test)]
#[path = "group_event_tests.rs"]
mod group_event_tests;
use crate::{api::devices::Caller, hub::Hub};
use anyhow::{Result, anyhow, ensure};
pub use events::filter_event;
use serde_json::{Value, json};
use std::{ops::ControlFlow, sync::Arc};

pub async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<ControlFlow<Value, Value>> {
    if let Some(value) = conversations::call(hub, caller, method, body.clone()).await? {
        return Ok(ControlFlow::Break(value));
    }
    let methods =
        ["donnaSpaces", "donnaSpaceCreate", "donnaSpaceUpdate", "donnaSpaceReorder", "donnaOwnerPresentation"];
    if !methods.contains(&method) {
        return Ok(ControlFlow::Continue(body));
    }
    if method == "donnaSpaces" {
        return Ok(ControlFlow::Break(serde_json::to_value(hub.store.spaces()?)?));
    }
    super::engine_setup::require_local(caller)?;
    let expected = body["expectedRevision"].as_i64().ok_or_else(|| anyhow!("expected_revision_required"))?;
    let state = hub.store.change_spaces(expected, |state, rev| {
        match method {
            "donnaSpaceCreate" => {
                let name = label(&body["name"])?;
                let tile: Tile = serde_json::from_value(body["tile"].clone())?;
                assets::tile(&tile)?;
                state.spaces.push(Space {
                    id: uuid::Uuid::new_v4().to_string(),
                    principal_bot_id: None,
                    kind: "space".into(),
                    name,
                    position: i64::try_from(state.spaces.len())?,
                    revision: rev,
                    tile: Some(tile),
                    appearance: Appearance::default(),
                    appearance_configured: true,
                    owner: None,
                });
            }
            "donnaSpaceUpdate" => {
                let id = body["spaceId"].as_str().ok_or_else(|| anyhow!("space_required"))?;
                let patch = body["patch"].as_object().ok_or_else(|| anyhow!("invalid_patch"))?;
                ensure!(
                    patch.keys().all(|k| matches!(k.as_str(), "name" | "tile" | "appearance")),
                    "immutable_space_field"
                );
                ensure!(id != PERSONAL || patch.keys().all(|k| k == "appearance"), "personal_space_immutable");
                let space = state.spaces.iter_mut().find(|s| s.id == id).ok_or_else(|| anyhow!("space_not_found"))?;
                if let Some(value) = patch.get("name") {
                    space.name = label(value)?;
                }
                if let Some(value) = patch.get("tile") {
                    let tile = serde_json::from_value(value.clone())?;
                    assets::tile(&tile)?;
                    space.tile = Some(tile);
                }
                if let Some(value) = patch.get("appearance") {
                    let appearance = serde_json::from_value(value.clone())?;
                    assets::appearance(&appearance)?;
                    space.appearance = appearance;
                    space.appearance_configured = true;
                }
                space.revision = rev;
            }
            "donnaSpaceReorder" => {
                let ids: Vec<String> = serde_json::from_value(body["ids"].clone())?;
                ensure!(
                    ids.len() == state.spaces.len() && ids.first().is_some_and(|id| id == PERSONAL),
                    "personal_space_immutable"
                );
                let unique: std::collections::HashSet<_> = ids.iter().collect();
                ensure!(
                    unique.len() == ids.len() && state.spaces.iter().all(|s| unique.contains(&s.id)),
                    "invalid_space_order"
                );
                state.spaces.sort_by_key(|s| ids.iter().position(|id| id == &s.id));
                for (i, s) in state.spaces.iter_mut().enumerate() {
                    s.position = i64::try_from(i)?;
                    s.revision = rev;
                }
            }
            "donnaOwnerPresentation" => {
                let owner: OwnerPresentation = serde_json::from_value(body["owner"].clone())?;
                assets::owner(&owner)?;
                let personal = state.spaces.first_mut().ok_or_else(|| anyhow!("spaces_unavailable"))?;
                personal.owner = Some(owner);
                personal.revision = rev;
            }
            _ => unreachable!(),
        }
        Ok(())
    })?;
    let value = serde_json::to_value(&state)?;
    let _ = hub.events.send(json!({"type":"spaces","spaceId":PERSONAL,"rev":state.revision,"snapshot":value}));
    Ok(ControlFlow::Break(value))
}
fn label(value: &Value) -> Result<String> {
    let value = value.as_str().ok_or_else(|| anyhow::anyhow!("invalid_space_name"))?.trim();
    anyhow::ensure!(
        !value.is_empty() && value.chars().count() <= 60 && !value.chars().any(char::is_control),
        "invalid_space_name"
    );
    Ok(value.into())
}
#[cfg(test)]
mod tests;
