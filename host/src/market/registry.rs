//! Compatibility boundary for the retired MCP Registry. No network or storage access.

use anyhow::{Result, bail};
use serde_json::Value;

pub const REGISTRY_UNAVAILABLE: &str =
    "mcp_registry_unavailable: MCP Registry foi retirado. Adicione a conexão MCP nativa por URL, comando ou config.";

/// Reject old discovery/install formats before vault unlock, runtime marks or writes.
/// A connectorId takes precedence over passive historical registryName on requests.
pub fn reject_registry_request(method: &str, body: &Value) -> Result<()> {
    let discovery = match method {
        "marketConnectors" | "connectorInfo" => true,
        "installConnector" => body.get("registryName").is_some_and(|name| !name.is_null()),
        "connectorCall" => registry_tool(body["name"].as_str().unwrap_or_default(), &body["arguments"]),
        _ => false,
    };
    if discovery {
        bail!(REGISTRY_UNAVAILABLE);
    }
    Ok(())
}

pub(super) fn registry_tool(name: &str, args: &Value) -> bool {
    name == "search_connectors"
        || (matches!(name, "request_connection" | "request_secret")
            && args["connectorId"].as_str().is_none_or(str::is_empty))
}

/// Classify a saved card, never a connectorId supplied in the completion body.
pub(super) fn reject_registry_finish(request: &Value) -> Result<()> {
    if matches!(request["kind"].as_str(), Some("connection" | "secret"))
        && request["connectorId"].as_str().is_none_or(str::is_empty)
    {
        bail!(REGISTRY_UNAVAILABLE);
    }
    Ok(())
}

#[cfg(test)]
mod d175_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn legacy_requests_fail_and_native_requests_remain_available() {
        for (method, body) in [
            ("marketConnectors", json!({"search":"x","cursor":"next"})),
            ("connectorInfo", json!({"registryName":""})),
            ("installConnector", json!({"registryName":"", "command":"fixture"})),
            ("installConnector", json!({"registryName":123})),
            ("connectorCall", json!({"name":"search_connectors","arguments":{}})),
            ("connectorCall", json!({"name":"request_connection","arguments":{}})),
            ("connectorCall", json!({"name":"request_secret","arguments":{}})),
        ] {
            assert!(
                reject_registry_request(method, &body).unwrap_err().to_string().contains("mcp_registry_unavailable:")
            );
        }
        assert!(reject_registry_request("installConnector", &json!({"registryName":null,"command":"fixture"})).is_ok());
        assert!(
            reject_registry_request(
                "connectorCall",
                &json!({"name":"request_connection","arguments":{"connectorId":"saved"}})
            )
            .is_ok()
        );
    }

    #[test]
    fn advertised_tools_require_saved_connector() {
        let tools = crate::market::requests::tools();
        let tools = tools.as_array().unwrap();
        assert!(!tools.iter().any(|tool| tool["name"] == "search_connectors"));
        let request = tools.iter().find(|tool| tool["name"] == "request_connection").unwrap();
        assert!(request["inputSchema"]["required"].as_array().unwrap().iter().any(|field| field == "connectorId"));
        assert!(request["inputSchema"]["properties"].get("registryName").is_none());
        assert!(crate::market::requests::INSTRUCTIONS_GUIDE.contains("connectorId"));
    }

    #[test]
    fn legacy_and_incomplete_connectors_keep_identity_and_redact_values() {
        let legacy: crate::market::Connector = serde_json::from_value(json!({
            "id":"legacy-id", "name":"legacy", "registryName":"fixture/legacy", "command":"fixture",
            "args":["fixture-arg"], "env":{"SYNTHETIC":"secret-fixture"}
        }))
        .unwrap();
        let incomplete: crate::market::Connector = serde_json::from_value(json!({
            "id":"incomplete-id", "name":"incomplete", "registryName":"fixture/incomplete"
        }))
        .unwrap();
        assert_eq!(legacy.public()["registryName"], "fixture/legacy");
        assert_eq!(legacy.public()["configuration"], "configured");
        assert!(!legacy.public().to_string().contains("secret-fixture"));
        assert_eq!(legacy.args, ["fixture-arg"]);
        assert_eq!(incomplete.public()["id"], "incomplete-id");
        assert_eq!(incomplete.public()["configuration"], "manualRequired");
        assert!(incomplete.acp(std::path::Path::new("fixture"), 0).is_none());
    }

    #[tokio::test]
    async fn native_add_import_preserve_existing_fixtures() {
        let store = crate::store::Store::open(std::path::Path::new(":memory:")).unwrap();
        let legacy: crate::market::Connector = serde_json::from_value(json!({
            "id":"legacy-id", "name":"Legacy", "registryName":"fixture/legacy",
            "command":"fixture", "args":["original"], "env":{"KEY":"synthetic"}
        }))
        .unwrap();
        crate::market::update_connectors(&store, |all| {
            all.push(legacy.clone());
            Ok(())
        })
        .unwrap();
        let legacy = crate::market::connectors(&store).unwrap().remove(0);
        crate::market::add_custom_connector(&store, &json!({"name":"Manual", "command":"fixture", "args":["added"]}))
            .await
            .unwrap();
        crate::market::import_connectors(
            &store,
            r#"{"mcpServers":{"Imported":{"command":"fixture","args":["imported"]}}}"#,
            crate::fork::spaces::PERSONAL,
        )
        .await
        .unwrap();
        let saved = crate::market::connectors(&store).unwrap();
        assert_eq!(saved.len(), 3);
        assert_eq!(saved.iter().find(|c| c.id == "legacy-id").unwrap(), &legacy);
        assert_eq!(crate::market::list_connectors(&store).unwrap()["items"].as_array().unwrap().len(), 3);
        assert!(store.bots().unwrap().is_empty());
    }

    #[test]
    fn harness_compilation_delivers_native_guidance() {
        let text = crate::agent::harness::compile();
        assert!(text.contains("search_connectors foram retirados"));
        assert!(text.contains("connectorId"));
    }

    #[tokio::test]
    async fn d175_dispatch_rejects_legacy_before_effects() {
        use crate::{
            LockExt,
            api::{devices::Caller, dispatch},
            hub::Hub,
            remote::identity::Identity,
        };
        let dir = std::env::temp_dir().join(format!("d175-dispatch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = crate::store::Store::open(std::path::Path::new(":memory:")).unwrap();
        let hub =
            Hub::new(store, "fixture-hub".into(), Identity::load_or_create(&dir).unwrap(), "fixture-token".into(), 0);
        let bot: crate::store::BotConfig = serde_json::from_value(json!({
            "id":"fixture-bot", "name":"Fixture", "cwd":dir, "backend":"fixture", "connectors":["legacy-id"]
        }))
        .unwrap();
        hub.store.save_bot(&bot).unwrap();
        let connector: crate::market::Connector = serde_json::from_value(json!({
            "id":"legacy-id", "name":"Legacy", "registryName":"fixture/legacy",
            "command":"fixture", "args":["original"], "env":{"KEY":"synthetic"}
        }))
        .unwrap();
        crate::market::save_connectors(&hub.store, std::slice::from_ref(&connector)).unwrap();
        let connector = crate::market::connectors(&hub.store).unwrap().remove(0);
        let encrypted = hub.store.kv_read("connectors").unwrap();
        let before_bot = serde_json::to_value(hub.store.bot("fixture-bot").unwrap().unwrap().config).unwrap();
        let execution = crate::fork::sessoes::bindings::fixture_execution(&hub, "fixture-bot");
        let before_rev = hub.store.current_rev();
        let before_entries = hub.store.entries_since(0, 100).unwrap().len();
        let key = hub.store.secret_key.locked().take();
        assert!(key.is_some());
        for (method, mut body) in [
            ("marketConnectors", json!({"search":"fixture", "cursor":"next"})),
            ("connectorInfo", json!({"registryName":"fixture/legacy"})),
            ("installConnector", json!({"registryName":"fixture/legacy", "name":"wrong", "command":"fixture"})),
            ("connectorCall", json!({"botId":"fixture-bot", "name":"search_connectors", "arguments":{}})),
            (
                "connectorCall",
                json!({"botId":"fixture-bot", "name":"request_connection", "arguments":{"registryName":"fixture/legacy"}}),
            ),
            ("connectorCall", json!({"botId":"fixture-bot", "name":"request_secret", "arguments":{}})),
        ] {
            if method == "connectorCall" {
                body["sessionBinding"] = json!(execution.acp_binding().unwrap());
            }
            let error = dispatch(&hub, &Caller::Local, method, body).await.unwrap_err().to_string();
            assert!(error.contains("mcp_registry_unavailable:"), "{method}: {error}");
            assert!(hub.store.secret_key.locked().is_none());
            assert!(hub.store.kv_read("vault.id").unwrap().is_none());
            assert_eq!(hub.store.kv_read("connectors").unwrap(), encrypted);
            assert!(!hub.computers.external("fixture-bot"));
            assert_eq!(hub.store.current_rev(), before_rev);
            assert_eq!(hub.store.entries_since(0, 100).unwrap().len(), before_entries);
            assert_eq!(
                serde_json::to_value(hub.store.bot("fixture-bot").unwrap().unwrap().config).unwrap(),
                before_bot
            );
        }
        let denied = dispatch(&hub, &Caller::Pairing { key: "fixture".into() }, "marketConnectors", json!({}))
            .await
            .unwrap_err();
        assert!(denied.is::<crate::api::devices::Forbidden>());
        *hub.store.secret_key.locked() = key;
        assert_eq!(crate::market::connectors(&hub.store).unwrap(), vec![connector]);
        drop(hub);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod d175_f1_r7 {
    use crate::{
        LockExt,
        api::{devices::Caller, dispatch},
        hub::Hub,
        remote::identity::Identity,
        store::{EntryKind, Lane, Store},
    };
    use serde_json::json;
    #[tokio::test]
    async fn legacy_card_rejected_before_vault_and_cancel_controls() {
        let dir = std::env::temp_dir().join(format!("d175-f1-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let hub = Hub::new(
            Store::open(std::path::Path::new(":memory:")).unwrap(),
            "fixture-hub".into(),
            Identity::load_or_create(&dir).unwrap(),
            "synthetic".into(),
            0,
        );
        let bot: crate::store::BotConfig = serde_json::from_value(
            json!({"id":"fixture-bot","name":"Fixture","cwd":dir,"backend":"fixture","connectors":["legacy-id"]}),
        )
        .unwrap();
        hub.store.save_bot(&bot).unwrap();
        let connector: crate::market::Connector = serde_json::from_value(
            json!({"id":"legacy-id","name":"Fixture","registryName":"fixture/legacy","env":{"KEY":"synthetic"}}),
        )
        .unwrap();
        crate::market::save_connectors(&hub.store, std::slice::from_ref(&connector)).unwrap();
        let legacy = hub.store.insert_entry(&Lane::main("fixture-bot"), EntryKind::Notice, 1, &json!({"connectionRequest":{"kind":"connection","status":"pending","botId":"fixture-bot","registryName":"fixture/legacy"}})).unwrap();
        let entry_before = serde_json::to_value(hub.store.entry(&legacy.id).unwrap()).unwrap();
        let rev = hub.store.current_rev();
        let ciphertext = hub.store.kv_read("connectors").unwrap();
        let bot_before = serde_json::to_value(hub.store.bot("fixture-bot").unwrap().unwrap().config).unwrap();
        let key = hub.store.secret_key.locked().take();
        assert!(key.is_some());
        let error = dispatch(&hub, &Caller::Local, "connectorRequestFinish", json!({"entryId":legacy.id}))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("mcp_registry_unavailable:"), "{error}");
        assert!(hub.store.secret_key.locked().is_none());
        assert!(hub.store.kv_read("vault.id").unwrap().is_none());
        assert!(hub.store.kv_read("vault.initialized").unwrap().is_none());
        assert_eq!(hub.store.current_rev(), rev);
        assert_eq!(serde_json::to_value(hub.store.entry(&legacy.id).unwrap()).unwrap(), entry_before);
        assert_eq!(hub.store.kv_read("connectors").unwrap(), ciphertext);
        assert_eq!(serde_json::to_value(hub.store.bot("fixture-bot").unwrap().unwrap().config).unwrap(), bot_before);
        assert!(!hub.computers.external("fixture-bot"));
        assert!(
            dispatch(&hub, &Caller::Local, "connectorRequestFinish", json!({"entryId":"missing", "cancel":true}))
                .await
                .is_err()
        );
        assert!(hub.store.secret_key.locked().is_none());
        assert!(hub.store.kv_read("vault.id").unwrap().is_none());
        let cancelled =
            dispatch(&hub, &Caller::Local, "connectorRequestFinish", json!({"entryId":legacy.id, "cancel":true}))
                .await
                .unwrap();
        assert_eq!(cancelled["entry"]["data"]["connectionRequest"]["status"], "cancelled");
        assert!(hub.store.secret_key.locked().is_none());
        assert!(hub.store.kv_read("vault.id").unwrap().is_none());
        assert_eq!(hub.store.kv_read("connectors").unwrap(), ciphertext);
        assert_eq!(serde_json::to_value(hub.store.bot("fixture-bot").unwrap().unwrap().config).unwrap(), bot_before);
        *hub.store.secret_key.locked() = key;
        let native = hub.store.insert_entry(&Lane::main("fixture-bot"), EntryKind::Notice, 1, &json!({"connectionRequest":{"kind":"connection","status":"pending","botId":"fixture-bot","connectorId":"legacy-id"}})).unwrap();
        let error = dispatch(&hub, &Caller::Local, "connectorRequestFinish", json!({"entryId":native.id}))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("mcp_manual_required:"), "native path should reach native verification: {error}");
        assert!(hub.store.kv_read("vault.id").unwrap().is_none());
        drop(hub);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
