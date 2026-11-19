//! Phones viewing the screen: prepared WebRTC sessions, offers, takeover and the on/off switch.

use super::{KV_ENABLED, Screen, Viewer, ViewerState, fork_viewer::Original};
use crate::LockExt;
use crate::hub::Hub;
use anyhow::{Result, anyhow, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

/// WebRTC ICE servers. Credentials are memory-only and must not be logged.
#[derive(Clone, Serialize, Deserialize)]
pub struct IceServer {
    pub urls: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IceConfig {
    pub ice_servers: Vec<IceServer>,
    pub expires_at: i64,
}

impl Default for IceConfig {
    fn default() -> Self {
        Self { ice_servers: vec![], expires_at: crate::store::now_ms() + 3_600_000 }
    }
}

impl Screen {
    /// Reserve one session before the phone gathers ICE; the host keeps the exact configuration.
    pub fn prepare(&self, owner: &str, ice: IceConfig) -> Result<Value> {
        let mut sessions = self.sessions.locked();
        // Troca/EOF usam a mesma trava: não inserir depois uma origem já retirada.
        let link = self.link()?;
        let now = crate::store::now_ms();
        sessions.rows.retain(|_, v| {
            let v = v.locked();
            v.state != ViewerState::Prepared || v.pending_until > now
        });
        let active = || sessions.rows.values().filter(|v| v.locked().state != ViewerState::Uncertain);
        if active().count() >= 32 || active().filter(|v| v.locked().owner == owner).count() >= 4 {
            bail!("too many screen sessions; close another screen and try again");
        }
        let session = uuid::Uuid::new_v4().to_string();
        let result = json!({"session": session, "iceServers": ice.ice_servers, "expiresAt": ice.expires_at});
        sessions.rows.insert(
            session,
            Arc::new(std::sync::Mutex::new(Viewer {
                link,
                owner: owner.to_owned(),
                ice: Some(ice),
                state: ViewerState::Prepared,
                pending_until: now + 60_000,
                origin_answer: None,
            })),
        );
        Ok(result)
    }

    pub fn owns_session(&self, session: &str, owner: &str) -> bool {
        self.sessions.locked().rows.get(session).is_some_and(|v| {
            let v = v.locked();
            v.state != ViewerState::Uncertain && v.owner == owner
        })
    }

    /// Answers an offer on a session `prepare` reserved.
    pub async fn offer(&self, owner: &str, sdp: &str, session: &str, display: Option<u32>) -> Result<Value> {
        let original = Original::capture(self, session.into()).ok_or_else(super::fork_viewer::missing)?;
        ensure!(original.owned_by(owner), "unknown screen session");
        self.link()?;
        original.require_current(self)?;
        if !self.status.locked().capture {
            bail!("Mexerica isn't allowed to record this computer's screen yet. Allow it in System Settings there.");
        }
        ensure!(!original.expired(), "screen connection expired; reconnect to continue");
        let ice = original.ice()?;
        let remote = !ice.ice_servers.is_empty();
        #[cfg(test)]
        super::fork_viewer::offer_admission_cut().await;
        let res = original
            .enqueue_answer(
                self,
                &json!({
                    "session": session, "sdp": sdp, "display": display, "iceServers": ice.ice_servers,
                    "maxBitrateBps": if remote { 4_000_000 } else { 16_000_000 },
                    "maxFramerate": if remote { 30 } else { 60 },
                }),
            )?
            .await;
        let Ok(res) = res else {
            original.require_current(self)?;
            // Cleanup fica na mesma origem. Falha/timeout não provam término nativo.
            original.expire(self);
            if original.current(self) && original.close(self).await.is_err() {
                tracing::warn!("couldn't confirm control response for failed screen session");
            }
            return Err(anyhow!("O ajudante não concluiu a sessão de tela original"));
        };
        let answer = res["sdp"].as_str().ok_or_else(|| anyhow!("the screen helper sent no answer"))?;
        original.require_current(self)?;
        if !self.enabled() {
            original.close(self).await?;
            bail!("screen session closed while connecting");
        }
        original.start(self)?;
        self.emit();
        Ok(json!({"session": session, "sdp": answer}))
    }

    pub async fn close(&self, session: &str) -> Result<()> {
        let Some(original) = Original::capture(self, session.into()) else { return Ok(()) };
        original.close_individual(self).await?.discard();
        Ok(())
    }

    pub fn takeover(&self, on: bool) {
        self.control.locked().user = on;
        self.emit();
    }

    /// Only callable from this computer (see `api/devices.rs`).
    pub async fn set_enabled(&self, store: &crate::store::Store, on: bool) -> Result<()> {
        if on && cfg!(windows) {
            anyhow::bail!("Remote screen isn't available on Windows yet.");
        }
        store.kv_set(KV_ENABLED, if on { "1" } else { "0" })?;
        self.set_viewers_enabled(on).await;
        self.emit();
        Ok(())
    }
}

/// Media bypasses the API, so keep checking the device lease while a viewer is connected.
// Captura ao construir a future, antes de spawn/primeiro poll. Nenhuma tarefa
// atrasada adota o registro que sucedeu àquele que ela deveria acompanhar.
pub fn watch_viewer(hub: Arc<Hub>, session: String, owner: String) -> impl Future<Output = ()> + Send + 'static {
    let original = Original::capture(&hub.screen, session);
    async move {
        let Some(original) = original else { return };
        if !original.owned_by(&owner) {
            return;
        }
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            if !original.current(&hub.screen) {
                return;
            }
            let authorized = owner == "local"
                || crate::api::devices::authorize(&hub, &owner)
                    .is_ok_and(|d| d.scopes.contains(&crate::store::Scope::Screen));
            if original.closing() || original.expired() || !authorized || !hub.screen.enabled() {
                if original.close(&hub.screen).await.is_err() {
                    tracing::warn!("couldn't confirm control response for expired screen session");
                    // Mantém o controle periódico herdado sobre a MESMA origem.
                    // Erro/timeout não confirmam término nativo nem adotam sucessor.
                } else {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::helper::Link;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicI64;
    use tokio::sync::{broadcast, mpsc};

    #[test]
    fn prepared_sessions_are_bounded_and_do_not_count_as_viewers() {
        let (tx, _) = broadcast::channel(8);
        let screen = Screen::new(Some(true), tx);
        let (tx, _) = mpsc::unbounded_channel();
        *screen.link.locked() =
            Some(Arc::new(Link { id: 1, tx, pending: Mutex::default(), next_id: AtomicI64::new(1) }));
        let config = IceConfig {
            ice_servers: vec![IceServer {
                urls: vec!["turns:turn.cloudflare.com:443?transport=tcp".into()],
                username: Some("u".into()),
                credential: Some("secret".into()),
            }],
            ..IceConfig::default()
        };
        let prepared = screen.prepare("phone", config).unwrap();
        let id = prepared["session"].as_str().unwrap();
        assert!(screen.owns_session(id, "phone"));
        assert!(!screen.owns_session(id, "other"));
        assert_eq!(prepared["iceServers"][0]["credential"], "secret");
        assert_eq!(screen.state()["viewers"], 0);
        assert!(!screen.state().to_string().contains("secret"));
        for _ in 0..3 {
            screen.prepare("phone", IceConfig::default()).unwrap();
        }
        assert!(screen.prepare("phone", IceConfig::default()).is_err());
        for viewer in screen.sessions.locked().rows.values_mut() {
            viewer.locked().pending_until = 0;
        }
        assert!(screen.prepare("phone", IceConfig::default()).is_ok());
        *screen.enabled.locked() = Some(false);
        assert!(screen.prepare("phone", IceConfig::default()).is_err());
    }
}
