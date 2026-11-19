//! A new bot's defaults, for a client's `createBot` and a bot's `create_bot` (`chat::manage`) alike.

use crate::hub::Hub;
use crate::store::BotConfig;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::sync::Arc;

/// The bot or group `b` asks for, with the defaults for what the caller left out.
pub async fn with_defaults(hub: &Arc<Hub>, mut b: Value) -> Result<BotConfig> {
    b["id"] = "".into();
    let non_personal = b["spaceId"].as_str().is_some_and(|id| id != crate::fork::spaces::PERSONAL);
    if non_personal {
        anyhow::ensure!(
            b["kind"] == "group" || b["backend"].as_str().is_some_and(|b| !b.is_empty()),
            "space_backend_selection_required"
        );
        if b["skills"].is_null() {
            b["skills"] = json!([]);
        }
        b["sharedMemory"] = json!(false);
        b["manageTeam"] = json!(false);
        b["harnessMemory"] = json!(false);
    }
    // D91: MCP connectors and tool accounts require the user's explicit choice.
    if b["connectors"].is_null() && b["kind"] != "group" {
        b["connectors"] = json!([]);
    }
    if b["logins"].is_null() && b["kind"] != "group" {
        b["logins"] = json!([]);
    }
    // D166: instalar não seleciona habilidades para bots novos.
    if b["skills"].is_null() && b["kind"] != "group" {
        b["skills"] = json!([]);
    }
    // And the agent, when the client names none.
    if b["backend"].as_str().is_none_or(str::is_empty) && b["kind"] != "group" {
        super::backends::recheck_signed_out(&hub.store, "codex").await;
        let agents = tokio::task::spawn_blocking(super::backends::list).await?;
        b["backend"] = default_backend(&agents).into();
    }
    serde_json::from_value(b).context("invalid bot")
}

/// D42: prefer Codex; fall back only when unavailable or known to be signed out on the Hub.
/// An unknown sign-in state doesn't justify switching. An unchosen registry agent is never a default.
fn default_backend(agents: &[Value]) -> &str {
    let runnable = || agents.iter().filter(|a| a["available"] == true && a["signedIn"] != false);
    if runnable().any(|a| a["id"] == "codex") {
        return "codex";
    }
    if runnable().any(|a| a["id"] == "claude") {
        return "claude";
    }
    runnable().find(|a| a["curated"] == true).and_then(|a| a["id"].as_str()).unwrap_or("codex")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_agent_is_codex_then_an_available_signed_in_alternative() {
        assert_eq!(default_backend(&[]), "codex", "nothing ready: keep Codex for setup");
        let mut agents = vec![
            json!({"id": "goose", "available": true, "curated": false}),
            json!({"id": "claude", "available": true, "curated": true, "signedIn": true}),
            json!({"id": "codex", "available": true, "curated": true, "signedIn": true}),
        ];
        assert_eq!(default_backend(&agents), "codex");
        agents[2]["signedIn"] = false.into();
        assert_eq!(default_backend(&agents), "claude");
        agents[2]["signedIn"] = Value::Null;
        assert_eq!(default_backend(&agents), "codex", "unknown login does not trigger a fallback");
        agents[2]["available"] = false.into();
        assert_eq!(default_backend(&agents), "claude", "Codex unavailable on the Hub");
        agents[1]["signedIn"] = false.into();
        agents.push(json!({"id":"cursor", "available":true, "curated":true}));
        assert_eq!(default_backend(&agents), "cursor", "skip signed-out and unchosen registry agents");
    }

    #[tokio::test]
    async fn new_bots_only_receive_explicit_connectors_and_logins() {
        let dir = std::env::temp_dir().join(format!("donnabot-new-bot-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = crate::store::Store::open(std::path::Path::new(":memory:")).unwrap();
        let connector = serde_json::from_value(json!({"id":"mcp", "name":"MCP", "command":"unused"})).unwrap();
        crate::market::save_connectors(&store, &[connector]).unwrap();
        store.kv_set("skills", r#"[{"id":"legacy","name":"Legacy"}]"#).unwrap();
        let hub = Hub::new(
            store,
            "test".into(),
            crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
            "test".into(),
            0,
        );
        let body = json!({"name":"New", "backend":"claude"});
        let new = with_defaults(&hub, body.clone()).await.unwrap();
        assert_eq!(new.connectors, Vec::<String>::new());
        assert_eq!(new.logins, Vec::<String>::new());
        assert!(new.skills.is_empty(), "D166: instalado não ativa em bot novo");
        assert_eq!(new.backend, "claude", "the user's chosen engine is preserved");
        let mut selected = body;
        selected["connectors"] = json!(["mcp", "ferramenta-mail", "conta-externa-x"]);
        selected["logins"] = json!(["mail-account"]);
        selected["skills"] = json!(["legacy"]);
        let explicit = with_defaults(&hub, selected).await.unwrap();
        assert_eq!(explicit.connectors, ["mcp", "ferramenta-mail", "conta-externa-x"]);
        assert_eq!(explicit.logins, ["mail-account"]);
        assert_eq!(explicit.skills, ["legacy"], "escolha explícita preservada");
        let _ = std::fs::remove_dir_all(dir);
    }
}
