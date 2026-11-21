// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Catálogo local de eventos. Telemetria desativada em todos os builds do Donnabot.

use crate::LockExt as _;
use crate::hub::Hub;
use crate::store::Store;
use anyhow::Result;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio::sync::{mpsc, oneshot};

pub mod errors;
mod lifecycle;
mod observe;
pub mod reported;
mod session;
mod turns;

pub use lifecycle::{KV_STARTED, Via, device_paired, host_started, phone_connected};
pub use observe::{answered, observe_call};
pub use reported::track;
pub use turns::{TurnOutcome, TurnSource, turn_finished};

const KV_ENABLED: &str = "analytics_enabled";
/// The account the install id was last merged into (one `$identify` per account).
const KV_IDENTIFIED: &str = "analytics_identified";
const FLUSH_EVERY: Duration = Duration::from_secs(30);
/// Unsent events kept while offline; the oldest go first.
const MAX_PENDING: usize = 500;

/// Everything the host reports. Names follow the `object_verb` convention; each event's
/// sender and properties: docs/reference/analytics-events.md.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    AppOpened,
    WindowOpened,
    HostStarted,
    OnboardingCompleted,
    ComputerAccessFinished,
    SignInStarted,
    SignInFailed,
    SignedIn,
    SignedOut,
    PairingCodeCreated,
    DevicePaired,
    AccessRequestDenied,
    DeviceRevoked,
    PhoneConnected,
    LinkOpened,
    ScreenViewed,
    BotCreated,
    BotUpdated,
    BotsReordered,
    BotDeleted,
    MessageSent,
    TurnStopped,
    TurnFinished,
    SessionReset,
    PermissionAnswered,
    ReactionAdded,
    HandoffAnswered,
    MemoryEdited,
    RoutineSaved,
    RoutineToggled,
    RoutineDeleted,
    RoutineRun,
    VoiceCallStarted,
    VoiceCallFinished,
    RemoteScreenOpened,
    ScreenTakenOver,
    ConnectorInstalled,
    ConnectorsImported,
    ConnectorRemoved,
    ConnectorSigninStarted,
    SkillInstalled,
    SkillRemoved,
    CredentialSaved,
    AgentSetupStarted,
    AgentSignedIn,
    SettingChanged,
    HostUpdateInstalled,
    /// Error tracking (`errors`): a panic or an uncaught error.
    Exception,
}

impl Event {
    fn name(self) -> &'static str {
        match self {
            Self::AppOpened => "app_opened",
            Self::WindowOpened => "window_opened",
            Self::HostStarted => "host_started",
            Self::OnboardingCompleted => "onboarding_completed",
            Self::ComputerAccessFinished => "computer_access_finished",
            Self::SignInStarted => "sign_in_started",
            Self::SignInFailed => "sign_in_failed",
            Self::SignedIn => "signed_in",
            Self::SignedOut => "signed_out",
            Self::PairingCodeCreated => "pairing_code_created",
            Self::DevicePaired => "device_paired",
            Self::AccessRequestDenied => "access_request_denied",
            Self::DeviceRevoked => "device_revoked",
            Self::PhoneConnected => "phone_connected",
            Self::LinkOpened => "link_opened",
            Self::ScreenViewed => "screen_viewed",
            Self::BotCreated => "bot_created",
            Self::BotUpdated => "bot_updated",
            Self::BotsReordered => "bots_reordered",
            Self::BotDeleted => "bot_deleted",
            Self::MessageSent => "message_sent",
            Self::TurnStopped => "turn_stopped",
            Self::TurnFinished => "turn_finished",
            Self::SessionReset => "session_reset",
            Self::PermissionAnswered => "permission_answered",
            Self::ReactionAdded => "reaction_added",
            Self::HandoffAnswered => "handoff_answered",
            Self::MemoryEdited => "memory_edited",
            Self::RoutineSaved => "routine_saved",
            Self::RoutineToggled => "routine_toggled",
            Self::RoutineDeleted => "routine_deleted",
            Self::RoutineRun => "routine_run",
            Self::VoiceCallStarted => "voice_call_started",
            Self::VoiceCallFinished => "voice_call_finished",
            Self::RemoteScreenOpened => "remote_screen_opened",
            Self::ScreenTakenOver => "screen_taken_over",
            Self::ConnectorInstalled => "connector_installed",
            Self::ConnectorsImported => "connectors_imported",
            Self::ConnectorRemoved => "connector_removed",
            Self::ConnectorSigninStarted => "connector_signin_started",
            Self::SkillInstalled => "skill_installed",
            Self::SkillRemoved => "skill_removed",
            Self::CredentialSaved => "credential_saved",
            Self::AgentSetupStarted => "agent_setup_started",
            Self::AgentSignedIn => "agent_signed_in",
            Self::SettingChanged => "setting_changed",
            Self::HostUpdateInstalled => "host_update_installed",
            Self::Exception => "$exception",
        }
    }
}

pub struct Analytics {
    enabled: Mutex<bool>,
    tx: mpsc::UnboundedSender<Msg>,
    rx: Mutex<Option<mpsc::UnboundedReceiver<Msg>>>,
    session: Mutex<session::Session>,
    /// The UTC day each phone last connected (`phone_connected`), in memory only.
    phones: Mutex<HashMap<String, chrono::NaiveDate>>,
}

enum Msg {
    Event(Value),
    /// A panic, captured off the panicking thread (`errors::report_panics`).
    Report(errors::Report),
    /// Send what's queued now and answer when done.
    Flush(oneshot::Sender<()>),
}

impl Analytics {
    pub fn load(_store: &Store) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            enabled: Mutex::new(false),
            tx,
            rx: Mutex::new(Some(rx)),
            session: Mutex::default(),
            phones: Mutex::default(),
        }
    }

    pub fn state(&self) -> bool {
        *self.enabled.locked()
    }

    pub fn set_enabled(&self, store: &Store, _on: bool) -> Result<()> {
        store.kv_set(KV_ENABLED, "0")?;
        *self.enabled.locked() = false;
        Ok(())
    }
}

/// A phone's platform as the catalog names it.
fn platform(p: &str) -> &'static str {
    match p {
        "ios" => "ios",
        "android" => "android",
        "macos" => "macos",
        _ => "other",
    }
}

/// The platform of the paired phone `key` (`other` for one this computer doesn't have).
fn platform_of(hub: &Hub, key: &str) -> &'static str {
    platform(hub.store.device(key).as_ref().map_or("", |d| d.platform.as_str()))
}

/// Queues one event unless the owner turned analytics off. Signed in to a Codync account,
/// events are linked to that account; otherwise to this install's random id. Both get a
/// person profile.
pub fn capture(hub: &Hub, event: Event, mut properties: Map<String, Value>) {
    if !hub.analytics.state() {
        return;
    }
    let account = hub.cloud.status().owner.map(|o| o.user_id);
    if let Some(user) = &account
        && hub.store.kv_get(KV_IDENTIFIED).as_deref() != Some(user.as_str())
    {
        // Merges this install's earlier, anonymous events into the account.
        let identify = json!({"$anon_distinct_id": hub.host_id, "$process_person_profile": true});
        queue(hub, "$identify", user, identify.as_object().cloned().unwrap_or_default());
        if let Err(e) = hub.store.kv_set(KV_IDENTIFIED, user) {
            tracing::warn!(error = format!("{e:#}"), "couldn't save the analytics identity");
        }
    }
    properties.insert("$process_person_profile".into(), true.into());
    queue(hub, event.name(), account.as_deref().unwrap_or(&hub.host_id), properties);
}

fn queue(hub: &Hub, event: &str, distinct_id: &str, mut properties: Map<String, Value>) {
    properties.extend([
        ("$lib".into(), "codync-host".into()),
        ("app_version".into(), env!("CARGO_PKG_VERSION").into()),
        ("env".into(), (if option_env!("CODYNC_ENV") == Some("main") { "main" } else { "dev" }).into()),
        ("$os".into(), std::env::consts::OS.into()),
        ("arch".into(), std::env::consts::ARCH.into()),
        // Joins this computer's events across accounts and with its phones' `computer_paired`.
        ("computer".into(), computer_tag(&hub.identity.computer_id()).into()),
        ("$session_id".into(), hub.analytics.session.locked().id(SystemTime::now()).to_string().into()),
    ]);
    let _ = hub.analytics.tx.send(Msg::Event(json!({
        "event": event,
        "distinct_id": distinct_id,
        // When it happened, not when its batch left; the uuid makes a resent batch count once.
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "uuid": uuid::Uuid::new_v4().to_string(),
        "properties": properties,
    })));
}

/// A computer's analytics key: a hash of its id, so a phone's `computer_paired` joins the
/// computer's events without revealing the id the relay knows it by.
pub fn computer_tag(computer_id: &str) -> String {
    use sha2::{Digest as _, Sha256};
    use std::fmt::Write as _;
    let hash = Sha256::digest(format!("codync-analytics:{computer_id}"));
    hash[..8].iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Sends queued events in batches every `FLUSH_EVERY`; turning analytics off drops them.
pub fn start(hub: &Arc<Hub>) {
    if !hub.analytics.state() {
        return;
    }
    let Some(mut rx) = hub.analytics.rx.locked().take() else { return };
    errors::report_panics(hub);
    let hub = hub.clone();
    tokio::spawn(async move {
        let mut pending: Vec<Value> = Vec::new();
        let mut tick = tokio::time::interval(FLUSH_EVERY);
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    Some(Msg::Event(e)) => pending.push(e),
                    Some(Msg::Report(r)) => errors::capture_report(&hub, r),
                    Some(Msg::Flush(done)) => {
                        flush(&hub, &mut pending);
                        let _ = done.send(());
                    }
                    None => return,
                },
                _ = tick.tick() => flush(&hub, &mut pending),
            }
            if pending.len() > MAX_PENDING {
                pending.drain(..pending.len() - MAX_PENDING);
            }
        }
    });
}

fn flush(hub: &Hub, pending: &mut Vec<Value>) {
    let on = hub.analytics.state();
    let sent = on && !pending.is_empty() && send(pending);
    if sent || !on {
        pending.clear();
    }
}

/// Sends what's queued before the host exits, so a restart or update loses nothing.
pub async fn flush_before_exit(hub: &Hub) {
    if !hub.analytics.state() {
        return;
    }
    let (tx, rx) = oneshot::channel();
    if hub.analytics.tx.send(Msg::Flush(tx)).is_ok() {
        let _ = tokio::time::timeout(Duration::from_secs(3), rx).await;
    }
}

// Sem cliente HTTP, endpoint ou credencial, inclusive em release/dev-app.
fn send(_batch: &[Value]) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_analytics_does_not_start_or_queue() {
        use std::future::Future as _;

        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let identity = crate::remote::identity::Identity {
            sign: ed25519_dalek::SigningKey::from_bytes(&[1; 32]),
            box_secret: x25519_dalek::StaticSecret::from([2; 32]),
        };
        let hub = Hub::new(store, "test".into(), identity, "test".into(), 0);
        let hooks = Arc::weak_count(&hub);
        // Sem runtime Tokio: não pode instalar panic hook nem criar tarefa/timer.
        start(&hub);
        assert_eq!(Arc::weak_count(&hub), hooks);
        capture(&hub, Event::HostStarted, Map::new());
        let mut rx = hub.analytics.rx.locked();
        assert!(matches!(rx.as_mut().unwrap().try_recv(), Err(mpsc::error::TryRecvError::Empty)));
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(std::pin::pin!(flush_before_exit(&hub)).as_mut().poll(&mut context).is_ready());
    }

    #[test]
    fn telemetry_cannot_be_enabled_or_sent() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.kv_set(KV_ENABLED, "1").unwrap();
        let analytics = Analytics::load(&store);
        assert!(!analytics.state(), "preferência antiga não liga telemetria");
        analytics.set_enabled(&store, true).unwrap();
        assert!(!analytics.state());
        assert_eq!(store.kv_get(KV_ENABLED).as_deref(), Some("0"));
        assert!(send(&[json!({"event": "host_started"})]));
        // O mesmo módulo é compilado em debug, dev-app e release.
        assert!(!include_str!("analytics.rs").contains(concat!("https", "://")));
        assert!(!include_str!("analytics.rs").contains(concat!("crate::", "http()")));
    }

    #[test]
    fn computer_tag_matches_the_iphone() {
        // Same vector as the iPhone's AnalyticsTests.
        assert_eq!(computer_tag("AAECAwQFBgcICQoLDA0ODw"), "2d6878bee20aba04");
    }
}
