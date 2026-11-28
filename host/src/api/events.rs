// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Event streams shared by SSE and channels: catch-up then live events, and setup terminals.

use super::devices::Caller;
use crate::LockExt;
use crate::hub::Hub;
use anyhow::Result;
use futures::{Stream, StreamExt, stream};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio_stream::wrappers::BroadcastStream;

/// Entries a fresh client receives per bot before paging with `history`.
const CATCH_UP_PER_BOT: i64 = 200;

struct LoginReader(Arc<crate::agent::term::Term>);
impl Drop for LoginReader {
    fn drop(&mut self) {
        self.0.disconnect_login();
    }
}

/// A setup terminal's output: everything so far, then live, ending after `exit`.
pub fn term_events(hub: &Hub, id: &str, caller: &Caller) -> Result<Option<impl Stream<Item = Value> + Send + use<>>> {
    crate::fork::personal_bootstrap::guard_generic(hub, caller)?;
    let Some(term) = hub.terms.authorized(id, caller)? else {
        return Ok(None);
    };
    if let Some(login) = &term.login {
        login.claim_reader()?;
    }
    let (head, rx) = term.attach();
    let done = head.iter().any(|v| v["type"] == "exit");
    let live = stream::unfold((rx, done), |(mut rx, done)| async move {
        if done {
            return None;
        }
        let value = rx.recv().await.ok()?;
        let done = value["type"] == "exit";
        Some((value, (rx, done)))
    });
    let guard = Arc::new(LoginReader(term.clone()));
    let caller = caller.clone();
    Ok(Some(stream::iter(head).chain(live).filter_map(move |value| {
        let _keep = &guard;
        let allowed = term.require_access(&caller).is_ok();
        async move { allowed.then_some(value) }
    })))
}

/// Counts a connected phone (pushes are held while one is connected) for as long as its stream lives.
struct PhoneClientGuard(Arc<Hub>);

impl PhoneClientGuard {
    fn new(hub: Arc<Hub>) -> Self {
        hub.phone_clients.fetch_add(1, Ordering::Relaxed);
        Self(hub)
    }
}

impl Drop for PhoneClientGuard {
    fn drop(&mut self) {
        self.0.phone_clients.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Event types only this computer's own apps receive.
const LOCAL_EVENTS: &[&str] = &["accessRequests", "cloud"];

#[cfg(test)]
mod tests;

/// Subscribes first, then yields a catch-up (everything after `since`, in rev order),
/// then live events. Duplicates are fine: clients upsert by id/rev. Shared by SSE and channels.
#[cfg(test)]
pub fn events_stream(
    hub: &Arc<Hub>,
    since: i64,
    client: Option<&str>,
    caller: &Caller,
) -> Result<impl Stream<Item = Value> + Send + use<>> {
    scoped_events_stream(hub, since, client, caller, None)
}

#[cfg(test)]
pub fn scoped_events_stream(
    hub: &Arc<Hub>,
    since: i64,
    client: Option<&str>,
    caller: &Caller,
    space: Option<&str>,
) -> Result<impl Stream<Item = Value> + Send + use<>> {
    scoped_events_stream_at(hub, since, client, caller, space, None)
}

/// `workspace_epoch`: o que o cliente conhece; ausente é aceito por compatibilidade.
pub fn scoped_events_stream_at(
    hub: &Arc<Hub>,
    since: i64,
    client: Option<&str>,
    caller: &Caller,
    space: Option<&str>,
    workspace_epoch: Option<u64>,
) -> Result<impl Stream<Item = Value> + Send + use<>> {
    crate::fork::personal_bootstrap::guard_generic(hub, caller)?;
    let retrato = crate::fork::workspace::abrir(hub, super::origem(caller), workspace_epoch)?;
    let workspace = crate::fork::workspace::contexto(hub, caller, &retrato)?;
    let space = space.unwrap_or(crate::fork::spaces::PERSONAL).to_owned();
    hub.store.require_space(&space)?;
    let live = BroadcastStream::new(hub.events.subscribe());
    let local = matches!(caller, Caller::Local);
    let mut catch_up: Vec<Value> = vec![];
    for bot in hub.bots_json_in_space(since, &space)? {
        catch_up.push(json!({"type": "bot", "rev": bot["rev"], "bot": bot}));
    }
    for e in hub.store.entries_since_in_space(&space, since, CATCH_UP_PER_BOT)? {
        catch_up.push(json!({"type": "entry", "rev": e.rev, "entry": e}));
    }
    catch_up.extend(crate::fork::sessoes::excluir::catch_up(&hub.store.db.locked(), &space, since)?);
    catch_up.sort_by_key(|v| v["rev"].as_i64().unwrap_or(0));
    let groups = hub.store.bot_groups_in_space(&space).unwrap_or_else(|error| {
        tracing::warn!(%error, "não foi possível ler grupos para o hello");
        crate::store::bot_groups::Snapshot { rev: 0, groups: vec![] }
    });
    let mut hello = json!({
        "type": "hello",
        "hostId": hub.host_id,
        "computerId": hub.identity.computer_id(),
        "rev": hub.store.current_rev(),
        "usage": hub.usage.locked().clone(),
        "screen": crate::fork::runtime_audiencia::screen::state(hub, &space),
        "botGroups": groups,
    });
    hello["spaceId"] = space.clone().into();
    hello["spacesContract"] = json!(1);
    hello["spaceBirthContract"] = json!(1);
    hello["spaceLifecycleContract"] = json!(1);
    hello["spaces"] = serde_json::to_value(hub.store.spaces()?)?;
    hello["conversations"] = serde_json::to_value(hub.store.space_conversations(&space)?)?;
    hello["workspace"] = serde_json::to_value(&workspace)?;
    catch_up.insert(0, hello);
    let snapshot_hub = hub.clone();
    let snapshot_space = space.clone();
    let head = stream::iter(catch_up).filter_map(move |v| {
        let event = open_session_event_visible(local, &v)
            .then(|| crate::fork::runtime_audiencia::revalidate_snapshot(&snapshot_hub, &snapshot_space, v))
            .flatten();
        async move { event }
    });

    let times_owner = crate::fork::bot_editor::authorization::require_owner(caller).is_ok();
    let guard = Arc::new(matches!(client, Some("ios" | "android")).then(|| PhoneClientGuard::new(hub.clone())));
    let event_hub = hub.clone();
    let tower_caller = Arc::new(crate::fork::control_tower::caller_copy(caller));
    let tail = live.flat_map(move |msg| {
        let _keep = &guard;
        let canonical_project_signal = msg.as_ref().is_ok_and(|v| {
            v["type"] == crate::fork::projetos::EVENT && v.as_object().is_some_and(|obj| obj.len() == 3)
        });
        let projected = match msg {
            Ok(v) if !open_session_event_visible(local, &v) => None,
            Ok(v) if v["type"] == crate::fork::projetos::EVENT => {
                crate::fork::projetos::event(&event_hub, &tower_caller, &space, &v)
            }
            Ok(v) if v["type"] == crate::fork::governanca::EVENT => {
                crate::fork::governanca::event(&event_hub, &tower_caller, &space, &v)
            }
            Ok(v) if v["type"] == crate::fork::control_tower::EVENT => {
                crate::fork::control_tower::event(&event_hub, &tower_caller, &space, &v)
            }
            Ok(v) if v["type"] == "times" && !times_owner => None,
            Ok(v) if v["type"] == "whatsappChanged" && !times_owner => None,
            Ok(v) if !local && v["type"].as_str().is_some_and(|t| LOCAL_EVENTS.contains(&t)) => None,
            Ok(v) => crate::fork::spaces::filter_event(&event_hub, &space, v),
            // Lagged: tell the client to resync from its last rev.
            Err(_) => Some(json!({"type": "resync"})),
        };
        let attention = projected.as_ref().and_then(|v| {
            if v["type"] == crate::fork::projetos::EVENT && !canonical_project_signal {
                return None;
            }
            crate::fork::control_tower::attention_signal(v)
        });
        let hub = event_hub.clone();
        let caller = tower_caller.clone();
        let space = space.clone();
        stream::iter(projected.into_iter().map(Ok).chain(attention.into_iter().flatten().map(Err))).filter_map(
            move |event| {
                let next = match event {
                    Ok(value) => Some(value),
                    Err(kind) => crate::fork::control_tower::attention_hint(&hub, &caller, &space, kind),
                };
                std::future::ready(next)
            },
        )
    });
    Ok(crate::fork::workspace::cortar(hub, &retrato, head.chain(tail)))
}

fn open_session_event_visible(local: bool, event: &Value) -> bool {
    if local || !crate::environment::Environment::is_open() {
        return true;
    }
    match event["type"].as_str() {
        Some("session" | "sessionDeleted") => false,
        Some("entry") => {
            let entry = &event["entry"];
            entry["sessionId"].as_str().is_some_and(|id| entry["botId"] == id)
        }
        _ => true,
    }
}
