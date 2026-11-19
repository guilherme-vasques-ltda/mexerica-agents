//! A bot that manages the team, through a real host and its `manage` MCP process: the approval card that shows a
//! change in full, the new bot's record of who made it, and a change the user denies.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::host::Host;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

struct TeamWorkspace(std::path::PathBuf);
impl Drop for TeamWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn wait(host: &Host, bot: &str, found: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let history = host.call("history", json!({"botId": bot})).await;
        if let Some(entry) = history["entries"].as_array().unwrap().iter().find(|e| found(e)) {
            return entry.clone();
        }
        assert!(Instant::now() < deadline, "{bot}: {history}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Has the fixture lead call `tool` with `args`, answers its approval card with `option`, and returns its reply.
async fn manage(host: &Host, lead: &str, tool: &str, args: &Value, option: &str, check: impl Fn(&str)) -> String {
    host.call("send", json!({"botId": lead, "text": format!("MANAGE {tool} {args}")})).await;
    let card = wait(host, lead, |e| e["kind"] == "permission" && e["data"]["status"] == "pending").await;
    check(card["data"]["detail"].as_str().unwrap());
    host.call(
        "respondPermission",
        json!({"botId": card["botId"], "entryRev": card["rev"], "entryId": card["id"], "optionId": option}),
    )
    .await;
    let reply =
        wait(host, lead, |e| e["kind"] == "agent" && e["data"]["final"] == true && e["turn"] == card["turn"]).await;
    reply["data"]["text"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn a_lead_creates_a_bot_the_user_allowed_and_not_one_change_they_denied() {
    let host = Host::start().await;
    let lead = host.fixture_donna("team_agent.py", "").await;
    let workspace = TeamWorkspace(host.home.with_extension("workspaces"));
    host.call("setTeamSettings", json!({"primaryBotId": lead, "workspaceRoot":workspace.0})).await;
    host.call("updateBot", json!({"id": lead, "permission": "ask", "permissionChosenByUser": true})).await;

    let routine = json!({"name": "Scan", "instruction": "Scan for vulnerabilities", "triggers": [{"type": "interval", "seconds": 1800}]});
    let args = json!({"name": "Security Analyst", "instructions": "Reviews code for vulnerabilities.",
        "routines": [routine]});
    let approved_backend = std::cell::RefCell::new(String::new());
    let reply = manage(&host, &lead, "create_bot", &args, "allow", |detail| {
        *approved_backend.borrow_mut() = detail.lines().find_map(|line| line.strip_prefix("Agent: ")).unwrap().into();
        for line in [
            "Name: Security Analyst",
            "Approvals: ask",
            "Manage the team: false",
            "Instructions:\nReviews code for vulnerabilities.",
            "• Scan (Every 30 minutes): Scan for vulnerabilities",
        ] {
            assert!(detail.contains(line), "{line:?} not in {detail}");
        }
    })
    .await;
    let made: Value = serde_json::from_str(reply.strip_prefix("Done: ").unwrap_or_else(|| panic!("{reply}"))).unwrap();
    let id = made["bot"]["id"].as_str().unwrap();
    wait(&host, &lead, |e| e["data"]["text"] == "Bot Security Analyst criado").await;
    let bot = |sync: &Value| sync["bots"].as_array().unwrap().iter().find(|b| b["id"] == id).cloned().unwrap();
    let analyst = bot(&host.snapshot().await);
    assert_eq!(analyst["backend"], *approved_backend.borrow(), "the card shows the actual default engine");
    assert_eq!(
        (&analyst["createdBy"], &analyst["permission"], &analyst["manageTeam"]),
        (&json!(lead), &json!("ask"), &json!(false))
    );
    assert_eq!(host.call("routines", json!({"botId": id})).await["routines"][0]["name"], "Scan");

    let reply = manage(&host, &lead, "update_bot", &json!({"botId": id, "hidden": true}), "deny", |detail| {
        assert_eq!(detail, "Hidden: true");
    })
    .await;
    assert!(reply.starts_with("Refused: ") && reply.contains("didn't allow"), "{reply}");
    assert_eq!(bot(&host.snapshot().await)["hidden"], false);

    // A principal não pode trocar nem o próprio perfil/modelo. A recusa não abre card.
    let sent = host
        .call(
            "send",
            json!({"botId":lead, "text":format!("MANAGE update_bot {}", json!({"botId":lead, "model":"fixture-2"}))}),
        )
        .await;
    let reply = wait(&host, &lead, |e| {
        e["kind"] == "agent" && e["data"]["final"] == true && e["turn"] == sent["entry"]["turn"]
    })
    .await;
    assert!(reply["data"]["text"].as_str().unwrap().starts_with("Refused: "), "{reply}");
    let sync = host.snapshot().await;
    let lead_bot = sync["bots"].as_array().unwrap().iter().find(|b| b["id"] == lead).unwrap();
    assert_ne!(lead_bot["model"], "fixture-2");

    // Only a bot's `create_bot` records a creator: a client's `createBot` can't claim one.
    let mine = json!({"name": "Mine", "backend": "custom", "command": "unused", "connectors": [], "createdBy": lead});
    let mine = host.call("createBot", mine).await["bot"]["id"].as_str().unwrap().to_owned();
    assert!(
        host.snapshot().await["bots"].as_array().unwrap().iter().any(|b| b["id"] == mine && b["createdBy"].is_null())
    );
}

/// A change whose wait ends without an answer ends its card too, and changes nothing.
#[tokio::test]
async fn a_card_ends_when_its_turn_stops_or_its_call_is_dropped() {
    let host = Host::start().await;
    let lead = host.fixture_donna("team_agent.py", "").await;
    let workspace = TeamWorkspace(host.home.with_extension("workspaces"));
    host.call("setTeamSettings", json!({"primaryBotId": lead, "workspaceRoot":workspace.0})).await;
    host.call("updateBot", json!({"id": lead, "permission": "ask", "permissionChosenByUser": true})).await;
    let card = |title: &'static str| move |e: &Value| e["kind"] == "permission" && e["data"]["title"] == title;
    let named = |sync: &Value, name: &str| sync["bots"].as_array().unwrap().iter().any(|b| b["name"] == name);

    // The user stops the turn: the card is cancelled and the tool refused.
    host.call("send", json!({"botId": lead, "text": "MANAGE create_bot {\"name\": \"Stopped\"}"})).await;
    let stopped = wait(&host, &lead, card("Criar bot Stopped")).await;
    host.call("stop", json!({"botId": lead})).await;
    wait(&host, &lead, |e| e["id"] == stopped["id"] && e["data"]["status"] == "cancelled").await;
    wait(&host, &lead, |e| {
        e["kind"] == "agent" && e["data"]["text"].as_str().is_some_and(|t| t.starts_with("Refused: "))
    })
    .await;

    // The agent cancels the call (its tool timeout): the card expires and the bot works on.
    host.call("send", json!({"botId": lead, "text": "BLOCK"})).await;
    let workspace = host.home.join("bots").join(&lead).join("workspace");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !std::fs::read_to_string(workspace.join("prompts.jsonl")).is_ok_and(|log| log.contains("BLOCK")) {
        assert!(Instant::now() < deadline, "turno da fixture não começou");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let servers: Value =
        serde_json::from_str(&std::fs::read_to_string(workspace.join("servers.json")).unwrap()).unwrap();
    let manage = servers.as_array().unwrap().iter().find(|s| s["name"] == "manage").unwrap();
    let args: Vec<&str> = manage["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect();
    let mut mcp = std::process::Command::new(manage["command"].as_str().unwrap())
        .args(args)
        .envs(
            manage["env"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| (e["name"].as_str().unwrap(), e["value"].as_str().unwrap())),
        )
        .env("MEXERICA_HOME", &host.home)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = mcp.stdin.take().unwrap();
    let mut send = |msg: Value| std::io::Write::write_all(&mut stdin, format!("{msg}\n").as_bytes()).unwrap();
    send(json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call",
        "params": {"name": "create_bot", "arguments": {"name": "Abandoned"}}}));
    let abandoned = wait(&host, &lead, card("Criar bot Abandoned")).await;
    send(json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 7}}));
    wait(&host, &lead, |e| e["id"] == abandoned["id"] && e["data"]["status"] == "expired").await;
    let sync = host.snapshot().await;
    assert_eq!(sync["bots"].as_array().unwrap().iter().find(|b| b["id"] == lead).unwrap()["status"], "working");
    assert!(!named(&sync, "Stopped") && !named(&sync, "Abandoned"), "nothing was created");
    std::fs::write(workspace.join("release"), "").unwrap();
    drop(stdin);
    mcp.wait().unwrap();
}

/// ACP notifications and actual internal MCP results must not contaminate the next effect or turn.
#[tokio::test]
async fn d125_internal_hub_tools_allow_owner_management_until_external_content_arrives() {
    for permission in ["auto", "ask"] {
        let host = Host::start().await;
        let lead = host.fixture_donna("team_agent.py", "").await;
        let workspace = TeamWorkspace(host.home.with_extension("workspaces"));
        host.call("setTeamSettings", json!({"primaryBotId":lead,"workspaceRoot":workspace.0})).await;
        host.call("updateBot", json!({"id":lead,"permission":permission,"permissionChosenByUser":true})).await;
        let flow = json!([
            ["manage", "list_team", {}],
            ["manage", "create_bot", {"name":"Scout"}],
            ["manage", "update_bot", {"botId":"$created","instructions":"Configurado pelo dono"}],
            ["memory", "read_memory", {}],
            ["memory", "remember", {"text":"O dono prefere café.","shared":false}]
        ]);
        let (results, cards) = owner_flow(&host, &lead, flow.clone(), "typed").await;
        assert_eq!(results.len(), 5, "{permission}: {results:?}");
        assert!(results.iter().all(|r| r["isError"] != true), "{permission}: {results:?}");
        assert_eq!(cards, if permission == "ask" { 2 } else { 0 });
        let made: Value = serde_json::from_str(results[1]["content"][0]["text"].as_str().unwrap()).unwrap();
        let id = made["bot"]["id"].as_str().unwrap();
        let sync = host.snapshot().await;
        assert!(
            sync["bots"]
                .as_array()
                .unwrap()
                .iter()
                .any(|b| b["id"] == id && b["description"] == "Configurado pelo dono")
        );
        wait(&host, &lead, |e| e["kind"] == "notice" && e["data"]["text"] == "Bot Scout criado").await;
        let update = json!([["manage", "update_bot", {"botId":id,"name":"Scout atualizado"}]]);
        let (results, _) = owner_flow(&host, &lead, update.clone(), "typed").await;
        assert_eq!(results[0]["isError"], Value::Null, "novo turno: {results:?}");
        // Colagem marcada é conteúdo de fora de fato; não é só um título de ferramenta.
        let (results, cards) = owner_flow(&host, &lead, update, "pasted").await;
        assert_eq!(cards, 0);
        assert_eq!(results[0]["isError"], true);
        assert!(results[0]["content"][0]["text"].as_str().unwrap().contains("confirmação do dono"));
    }
}

async fn owner_flow(host: &Host, lead: &str, steps: Value, origin: &str) -> (Vec<Value>, usize) {
    let text = format!("HUB_FLOW {steps}");
    let sent = host
        .call(
            "send",
            json!({"botId":lead,"text":text,"contentOrigin":{
                "schema":1,"draftId":uuid::Uuid::new_v4().to_string(),"revision":1,
                "textLength":text.encode_utf16().count(),"deviceId":uuid::Uuid::new_v4().to_string(),
                "channel":"desktopComposer","interaction":"send",
                "segments":[{"start":0,"end":text.encode_utf16().count(),"kind":origin}]
            }}),
        )
        .await;
    let mut answered = std::collections::HashSet::new();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let history = host.call("history", json!({"botId":lead})).await;
        let entries = history["entries"].as_array().unwrap();
        for card in entries.iter().filter(|e| e["kind"] == "permission" && e["data"]["status"] == "pending") {
            if answered.insert(card["id"].as_str().unwrap().to_owned()) {
                host.call(
                    "respondPermission",
                    json!({"botId":lead,"entryId":card["id"],"entryRev":card["rev"],"optionId":"allow"}),
                )
                .await;
            }
        }
        if let Some(reply) = entries
            .iter()
            .find(|e| e["kind"] == "agent" && e["data"]["final"] == true && e["turn"] == sent["entry"]["turn"])
        {
            let text = reply["data"]["text"].as_str().unwrap();
            return (serde_json::from_str(text.strip_prefix("Flow: ").unwrap()).unwrap(), answered.len());
        }
        assert!(Instant::now() < deadline, "sem resposta: {history}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
