// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Push via the Codync relay (it holds the APNs key; the host only holds opaque
//! per-device tickets the relay issued to the phone).
//!
//! Alerts cover completion, input requests, and failures;
//! none are sent while the phone app is connected (it's in the foreground).

use crate::LockExt;
use crate::fork::push_textos::{self, Corpo, Titulo};
use crate::hub::Hub;
use crate::remote::crypto;
use crate::store::BotConfig;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use x25519_dalek::StaticSecret;

/// Alert kinds (wire values double as the APNs category).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AlertKind {
    Done,
    NeedsInput,
    Failed,
}

/// (bot id, kind) → last alert.
static LAST_SENT: LazyLock<Mutex<HashMap<(String, AlertKind), Instant>>> = LazyLock::new(Mutex::default);
/// Tickets the relay reported as dead (app uninstalled / token rotated); purged on the next push.
static GONE: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn relay_url(_hub: &Hub) -> Option<String> {
    // Nenhum relay de push até existir configuração própria do Donnabot.
    None
}

/// True when `key` was sent less than `min_gap` ago; otherwise records now.
fn throttled(key: (String, AlertKind), min_gap: Duration) -> bool {
    let mut last = LAST_SENT.locked();
    if last.get(&key).is_some_and(|t| t.elapsed() < min_gap) {
        return true;
    }
    last.insert(key, Instant::now());
    false
}

fn post(url: String, body: Value) {
    tokio::spawn(async move {
        match crate::http_cloud().post(&url).json(&body).timeout(Duration::from_secs(10)).send().await {
            Ok(r) if r.status().is_redirection() => {
                tracing::warn!(%url, status = %r.status(), "relay respondeu com redirecionamento; push recusado");
            }
            Ok(r) if r.status().is_success() => {
                if let Some(notifications) = body["notifications"].as_array() {
                    match r.json::<Value>().await {
                        Ok(reply) => record_batch_results(notifications, &reply),
                        Err(error) => tracing::warn!(%error, "invalid relay batch response"),
                    }
                }
            }
            Ok(r) if r.status() == reqwest::StatusCode::GONE => {
                if let Some(t) = body["ticket"].as_str() {
                    GONE.locked().push(t.to_owned());
                }
            }
            Ok(r) => tracing::warn!(%url, status = %r.status(), "relay rejected push"),
            Err(error) => tracing::warn!(%url, %error, "relay unreachable"),
        }
    });
}

fn record_batch_results(notifications: &[Value], reply: &Value) {
    for result in reply["results"].as_array().into_iter().flatten() {
        let Some(index) = result["index"].as_u64().and_then(|index| usize::try_from(index).ok()) else { continue };
        let Some(ticket) = notifications.get(index).and_then(|item| item["ticket"].as_str()) else { continue };
        if result["status"] == 410 || result["superseded"] == true {
            GONE.locked().push(ticket.to_owned());
        } else if result["status"] != 200 {
            tracing::warn!(status = ?result["status"], "relay rejected a batched notification");
        }
    }
}

fn purge_dead_tickets(hub: &Hub) {
    for t in std::mem::take(&mut *GONE.locked()) {
        if let Err(error) = hub.store.remove_push_ticket(&t) {
            tracing::warn!(%error, "couldn't forget a dead push ticket");
        }
    }
}

/// The chat, its first two members (a group's avatar) and the speaker, just enough for the phone
/// to draw their faces even when its roster copy is older than the bot.
fn faces(hub: &Hub, bot: &BotConfig, from: Option<&str>) -> Vec<Value> {
    let face = |c: &BotConfig| {
        let (color, shape) = crate::fork_avatars::legacy_avatar(c);
        json!({"id": c.id, "kind": c.kind, "members": c.members, "name": c.name,
            "avatarColor": color, "avatarShape": shape})
    };
    let mut ids: Vec<&str> = bot.members.iter().take(2).map(String::as_str).collect();
    if let Some(from) = from.filter(|f| *f != bot.id && !ids.contains(f)) {
        ids.push(from);
    }
    std::iter::once(face(bot))
        .chain(ids.into_iter().filter_map(|id| hub.store.bot(id).ok().flatten()).map(|r| face(&r.config)))
        .collect()
}

/// O JSON selado para UM aparelho, já no idioma dele (MX-138): título, legenda e corpo.
fn secret_json(
    titulo: &Titulo<'_>,
    corpo: &Corpo<'_>,
    kind: AlertKind,
    idioma: &str,
    from: Option<&str>,
    faces: &[Value],
) -> Value {
    let mut secret = json!({
        "title": crate::agent::acp::truncate(&push_textos::titulo(idioma, titulo), 80),
        "subtitle": push_textos::subtitulo(idioma, kind),
        "body": crate::agent::acp::truncate(&push_textos::corpo(idioma, corpo), 400),
    });
    if let Some(from) = from {
        secret["from"] = from.into();
    }
    secret["faces"] = Value::Array(faces.to_vec());
    secret
}

/// `bot` é o chat que o alerta abre; `from` é o bot membro que falou quando o chat é um grupo.
/// `titulo`/`corpo` são montados por quem chama; cada aparelho recebe os textos no idioma que ELE
/// escolheu no app (`push_textos`), nunca no do remetente.
pub(crate) fn notify(
    hub: &Hub,
    bot: &BotConfig,
    from: Option<&str>,
    titulo: &Titulo<'_>,
    corpo: &Corpo<'_>,
    kind: AlertKind,
) {
    purge_dead_tickets(hub);
    if bot.notify == Some(false) || bot.hidden || hub.phone_connected() {
        return;
    }
    if throttled((bot.id.clone(), kind), Duration::from_secs(5)) {
        return;
    }
    let Some(relay) = relay_url(hub) else { return };
    let from = from.filter(|f| *f != bot.id);
    let faces = faces(hub, bot, from);
    let computer_id = hub.identity.computer_id();
    let mut notifications = Vec::new();
    for t in hub.store.push_tickets() {
        // MX-138: idioma, linha genérica e segredo são POR APARELHO.
        let idioma = push_textos::idioma_do_aparelho(&hub.store, &t.device_key);
        let secret = secret_json(titulo, corpo, kind, idioma, from, &faces).to_string();
        let mut data = json!({"botId": bot.id, "computerId": computer_id, "ctx": t.ctx});
        if let Some(key) = t.push_key.as_deref().and_then(|k| crypto::unb64_n::<32>(k).ok()) {
            let eph = StaticSecret::from(crypto::random::<32>());
            match crypto::seal_push(&hub.identity.cid_raw(), &key, secret.as_bytes(), &eph) {
                Ok(sealed) => data["sealed"] = sealed.into(),
                Err(error) => tracing::warn!(error = format!("{error:#}"), "couldn't seal a notification"),
            }
        }
        notifications.push(json!({
            "ticket": t.ticket,
            "alert": {"title": "Mexerica", "body": push_textos::generica(idioma, kind)},
            "mutableContent": true,
            "threadId": format!("{computer_id}:{}", bot.id),
            "category": kind,
            "data": data,
        }));
    }
    if !notifications.is_empty() {
        post(format!("{relay}/push-batch"), json!({"notifications": notifications}));
    }
}

/// Live Activities usam a projeção canônica; indisponibilidade não encerra nem consome tickets.
pub fn live_activity_update(hub: &Hub, bot_id: &str) {
    crate::fork::runtime_audiencia::live_activity::send(
        hub,
        bot_id,
        || {
            purge_dead_tickets(hub);
            relay_url(hub)
        },
        post,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MX-138: o segredo selado sai no idioma pedido — é o que cada aparelho mostra.
    #[test]
    fn o_segredo_sai_no_idioma_do_aparelho() {
        let faces = vec![];
        let pt = secret_json(&Titulo::Precisa("Berga"), &Corpo::Terminou, AlertKind::NeedsInput, "pt-BR", None, &faces);
        let en = secret_json(&Titulo::Precisa("Berga"), &Corpo::Terminou, AlertKind::NeedsInput, "en", None, &faces);
        assert_eq!(pt["title"], "Berga precisa de você");
        assert_eq!(pt["subtitle"], "Precisa da sua resposta");
        assert_eq!(pt["body"], "Terminou.");
        assert_eq!(en["title"], "Berga needs you");
        assert_eq!(en["subtitle"], "Response needed");
        assert_eq!(en["body"], "Finished.");
    }

    #[test]
    fn faces_projetam_o_avatar_real_mesmo_com_campos_legados_atrasados() {
        let dir = std::env::temp_dir().join(format!("donnabot-avatar-push-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = crate::store::Store::open(std::path::Path::new(":memory:")).unwrap();
        let hub = Hub::new(
            store,
            "teste".into(),
            crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
            "teste".into(),
            0,
        );
        let mut bot: BotConfig =
            serde_json::from_value(json!({"id":"bot","name":"Hype","avatarColor":"blue","avatarShape":"blob"}))
                .unwrap();
        bot.avatar_config = Some(crate::fork_avatars::preset("Hype", "", false));
        let expected = crate::fork_avatars::legacy_avatar(&bot);
        let face = &faces(&hub, &bot, None)[0];
        assert_eq!(face["avatarColor"], expected.0);
        assert_eq!(face["avatarShape"], expected.1);
        assert_ne!(face["avatarColor"], "blue");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
