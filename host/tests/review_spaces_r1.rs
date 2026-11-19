//! D169: Hub/ACP reais com CLI/conta/cofre de fixture, sem dados ou provedores reais.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod common;
use common::workspaces::Workspaces;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    process::Child,
    time::{Duration, Instant},
};
fn hash_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
        write!(out, "{b:02x}").unwrap();
        out
    })
}
struct Fixture {
    child: Child,
    base: String,
    token: String,
    hub_id: String,
    home: PathBuf,
    user_home: PathBuf,
    workspaces: Workspaces,
    env: Vec<(&'static str, std::ffi::OsString)>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        common::stop(&mut self.child);
        if let Ok(dir) = std::env::var("D169_EVIDENCE_DIR") {
            let dest = PathBuf::from(dir);
            std::fs::create_dir_all(&dest).unwrap();
            for path in self.logs() {
                let name = path.parent().unwrap().parent().unwrap().file_name().unwrap();
                std::fs::copy(&path, dest.join(format!("{}.jsonl", name.to_string_lossy()))).unwrap();
            }
        }
        let _ = std::fs::remove_dir_all(&self.user_home);
    }
}
impl Fixture {
    async fn raw(&self, method: &str, body: Value) -> (u16, Value) {
        let response = reqwest::Client::new()
            .post(format!("{}/api/{method}", self.base))
            .bearer_auth(&self.token)
            .json(&body)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .unwrap();
        let status = response.status().as_u16();
        (status, response.json().await.unwrap())
    }
    fn logs(&self) -> Vec<PathBuf> {
        let mut result = Vec::new();
        let root = self.home.join("spaces");
        if let Ok(spaces) = std::fs::read_dir(root) {
            for space in spaces.flatten() {
                if let Ok(chats) = std::fs::read_dir(space.path().join("conversations")) {
                    for chat in chats.flatten() {
                        let log = chat.path().join("workspace/space-requests.jsonl");
                        if log.is_file() {
                            result.push(log);
                        }
                    }
                }
            }
        }
        result
    }
    fn requests(&self, work: &Value, chat: &Value) -> Vec<Value> {
        let path = self
            .home
            .join("spaces")
            .join(work.as_str().unwrap())
            .join("conversations")
            .join(chat.as_str().unwrap())
            .join("workspace/space-requests.jsonl");
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }
    async fn conversation(&self, space: &Value, chat: &Value) -> Value {
        self.call("donnaConversations", json!({"spaceId":space})).await["conversations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == *chat)
            .unwrap()
            .clone()
    }
    async fn refused(&self, method: &str, mut body: Value) {
        if method.starts_with("donnaSpace")
            || method.starts_with("donnaConversation")
            || method == "donnaOwnerPresentation"
        {
            body["hubId"] = self.hub_id.clone().into();
        }
        let response = reqwest::Client::new()
            .post(format!("{}/api/{method}", self.base))
            .bearer_auth(&self.token)
            .json(&body)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .unwrap();
        assert!(!response.status().is_success(), "{method}: {}", response.text().await.unwrap());
    }
    async fn bind(&self, space: &Value, chat: &Value, account: &Value, model: &str) -> Value {
        let c = self.conversation(space, chat).await;
        let accounts = self.call("donnaEngineAccounts", json!({})).await;
        let a = accounts["accounts"].as_array().unwrap().iter().find(|a| a["id"] == *account).unwrap();
        self.call("donnaConversationUpdate",json!({"spaceId":space,"conversationId":chat,"expectedRevision":c["revision"],"engineAccountId":account,"expectedAccountRevision":a["revision"],"engineSelection":{"backend":"codex","model":model,"modelOptions":{}}})).await["conversation"].clone()
    }
    async fn new() -> Self {
        let user_home = std::env::temp_dir().join(format!("d169-spaces-http-{}", uuid::Uuid::new_v4()));
        let home = user_home.join("data");
        let bin = home.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        for (name, source) in [("codex", "engine_setup_cli.py"), ("codex-acp", "review_spaces_agent_r1.py")] {
            let target = bin.join(name);
            std::fs::copy(fixtures.join(source), &target).unwrap();
            if name == "codex" {
                let source = std::fs::read_to_string(&target)
                    .unwrap()
                    .replace("[{'id': 'gpt-6.1-sol'", "[{'id': 'gpt-fixture-b'}, {'id': 'gpt-6.1-sol'");
                std::fs::write(&target, source).unwrap();
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o700)).unwrap();
            }
        }
        let env = vec![
            ("PATH", std::ffi::OsString::from(format!("{}:/usr/bin:/bin", bin.display()))),
            ("HOME", user_home.clone().into_os_string()),
        ];
        let workspaces = Workspaces::new(&home);
        let (child, base) = common::serve_with_env(&home, &env).await;
        let token = std::fs::read_to_string(home.join("token")).unwrap().trim().to_owned();
        let hello: Value = reqwest::Client::new()
            .post(format!("{base}/api/hello"))
            .bearer_auth(&token)
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let hub_id = hello["hostId"].as_str().unwrap().to_owned();
        Self { child, base, token, hub_id, home, user_home, workspaces, env }
    }
    async fn call(&self, method: &str, mut body: Value) -> Value {
        if method.starts_with("donnaSpace")
            || method.starts_with("donnaConversation")
            || method == "donnaOwnerPresentation"
        {
            body["hubId"] = self.hub_id.clone().into();
        }
        let response = reqwest::Client::new()
            .post(format!("{}/api/{method}", self.base))
            .bearer_auth(&self.token)
            .json(&body)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let value: Value = response.json().await.unwrap();
        assert!(status.is_success(), "{method}: {value}");
        value
    }
    async fn connected(&self, key: &str) -> Value {
        let a=self.call("donnaEngineAccountCreate",json!({"backend":"codex","label":"Fixture explícita","kind":"cli","clientRequestId":format!("fixture-account-{key}")})).await;
        let op=self.call("donnaEngineSetupStart",json!({"engineAccountId":a["id"],"expectedRevision":1,"action":"login","clientRequestId":format!("fixture-login-{key}")})).await;
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let state = self
                .call(
                    "donnaEngineSetupAction",
                    json!({"operationId":op["operationId"],"generation":op["generation"],"action":"read"}),
                )
                .await;
            if state["phase"] == "connected" {
                break;
            }
            assert_ne!(state["phase"], "failed", "{state}");
            assert!(Instant::now() < deadline, "{state}");
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
        a["id"].clone()
    }
    async fn send(&self, space: &Value, chat: &Value, marker: &str, nonce: &str) -> Value {
        self.send_files(space, chat, marker, nonce, vec![]).await
    }
    async fn send_files(
        &self,
        space: &Value,
        chat: &Value,
        marker: &str,
        nonce: &str,
        attachments: Vec<String>,
    ) -> Value {
        let listed = self.call("donnaConversations", json!({"spaceId":space})).await;
        let revision =
            listed["conversations"].as_array().unwrap().iter().find(|c| c["id"] == *chat).unwrap()["revision"].clone();
        let sent=self.call("donnaConversationSend",json!({"spaceId":space,"conversationId":chat,"expectedRevision":revision,"text":marker,"clientNonce":nonce,"attachments":attachments})).await;
        let turn = sent["entry"]["turn"].clone();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let page = self.call("donnaConversationHistory", json!({"spaceId":space,"conversationId":chat})).await;
            if page["entries"].as_array().unwrap().iter().any(|e| {
                e["kind"] == "agent"
                    && e["turn"] == turn
                    && e["data"]["final"] == true
                    && e["data"]["text"]
                        == if marker.contains("CONTEXT_MARKER_B") { "CONTEXT_MARKER_B" } else { "CONTEXT_MARKER_A" }
            }) {
                break;
            }
            assert!(Instant::now() < deadline, "conversa: {page}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        // A resposta pode preceder a liberação do ator; estado final da fixture já foi gravado.
        tokio::time::sleep(Duration::from_millis(100)).await;
        sent
    }
    async fn restart(&mut self) {
        common::stop(&mut self.child);
        let (child, base) = common::serve_with_env(&self.home, &self.env).await;
        self.child = child;
        self.base = base;
    }
}
#[tokio::test]
async fn review_r1_http_acp_external_raw_sid_aba_reset() {
    let mut f = Fixture::new().await;
    let account = f.connected("a").await;
    // D366/D368: o Berga fixo do Hub novo; o dono ajusta motor, pasta e as próprias preferências.
    let donna = f.call("teamSettings", json!({})).await["primaryBotId"].clone();
    f.call("updateBot",json!({"id":donna,"backend":"codex","cwd":f.workspaces.root(),"permission":"ask","permissionChosenByUser":true,"model":"gpt-6.1-sol","ownerPreferences":"R1_PERSONAL_PROMPT_334C","forkEditor":{"engineAccountId":account}})).await;
    f.call("setTeamSettings", json!({"primaryBotId":donna,"permission":"ask"})).await;
    let initial = f.call("donnaSpaces", json!({})).await;
    let spaces=f.call("donnaSpaceCreate",json!({"name":"Fixture Trabalho","expectedRevision":initial["revision"],"tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"FT"}}})).await;
    let work = spaces["spaces"][1]["id"].clone();
    let account_dto = f.call("donnaEngineAccounts", json!({})).await;
    let account_revision =
        account_dto["accounts"].as_array().unwrap().iter().find(|a| a["id"] == account).unwrap()["revision"].clone();
    let mut chats = vec![];
    for title in ["A", "B"] {
        let c=f.call("donnaConversationCreate",json!({"spaceId":work,"botId":donna,"title":title})).await["conversation"].clone();
        f.call("donnaConversationUpdate",json!({"spaceId":work,"conversationId":c["id"],"expectedRevision":c["revision"],"engineAccountId":account,"expectedAccountRevision":account_revision})).await;
        chats.push(c["id"].clone());
    }
    let no_revision=reqwest::Client::new().post(format!("{}/api/donnaConversationSend",f.base)).bearer_auth(&f.token).json(&json!({"spaceId":work,"conversationId":chats[0],"text":"CONTEXT_MARKER_A","clientNonce":"missing-revision","hubId":f.hub_id})).send().await.unwrap();
    assert!(!no_revision.status().is_success(), "intenção sem revisão não pode escolher binding novo");
    f.call("saveMemory", json!({"botId":donna,"shared":true,"text":"R1_PERSONAL_MEMORY_334C"})).await;
    std::fs::write(f.workspaces.root().join("personal-secret-fixture.txt"), "R1_PERSONAL_FILE_334C").unwrap();
    let c = f.conversation(&work, &chats[0]).await;
    f.call("donnaConversationMemory",json!({"spaceId":work,"conversationId":chats[0],"expectedRevision":c["revision"],"method":"saveMemory","text":"R1_WORK_MEMORY_334C"})).await;
    let upload = uuid::Uuid::new_v4().to_string();
    f.call("donnaConversationUpload",json!({"spaceId":work,"conversationId":chats[0],"expectedRevision":c["revision"],"uploadId":upload,"name":"fixture.txt","offset":0,"done":true,"data":"UjFfQUxQSEFfRklMRV8zMzRD"})).await;
    f.refused(
        "donnaConversationReadUpload",
        json!({"spaceId":work,"conversationId":chats[1],"uploadId":upload,"offset":0}),
    )
    .await;
    let original_intent = json!({"spaceId":work,"conversationId":chats[0],"expectedRevision":c["revision"],"text":"CONTEXT_MARKER_A R1_ALPHA_HISTORY_334C","clientNonce":"a1","attachments":[upload]});
    let first =
        f.send_files(&work, &chats[0], "CONTEXT_MARKER_A R1_ALPHA_HISTORY_334C", "a1", vec![upload.clone()]).await;
    for mutation in [json!({"text":"R1_CHANGED_TEXT"}), json!({"attachments":[]}), json!({"replyTo":"R1_OTHER_ENTRY"})]
    {
        let mut changed = original_intent.clone();
        for (key, value) in mutation.as_object().unwrap() {
            changed[key] = value.clone();
        }
        f.refused("donnaConversationSend", changed).await;
    }
    let mut wrong_hub = original_intent.clone();
    wrong_hub["hubId"] = "R1_OTHER_REAL_HUB".into();
    let (status, value) = f.raw("donnaConversationSend", wrong_hub).await;
    assert_eq!(status, 400);
    assert_eq!(value["error"], "hub_context_mismatch");
    let actual = f.requests(&work, &chats[0]);
    let consumed = actual.iter().rfind(|v| v["method"] == "fixture/consumed").unwrap();
    assert_eq!(consumed["attachedFiles"][0]["text"], "R1_ALPHA_FILE_334C");
    assert!(consumed["text"].as_str().unwrap().contains("R1_WORK_MEMORY_334C"));
    f.send(&work, &chats[1], "CONTEXT_MARKER_B R1_BETA_HISTORY_334C", "b1").await;
    f.restart().await;
    f.send(&work, &chats[0], "CONTEXT_MARKER_A", "a2").await;
    for (i, chat) in chats.iter().enumerate() {
        let path = f
            .home
            .join("spaces")
            .join(work.as_str().unwrap())
            .join("conversations")
            .join(chat.as_str().unwrap())
            .join("workspace/space-requests.jsonl");
        let log = std::fs::read_to_string(path).unwrap();
        assert!(!log.contains("R1_PERSONAL_PROMPT_334C"));
        assert!(!log.contains("R1_PERSONAL_MEMORY_334C"));
        assert!(!log.contains("R1_PERSONAL_FILE_334C"));
        assert!(!log.contains(if i == 0 { "CONTEXT_MARKER_B" } else { "CONTEXT_MARKER_A" }));
        if i == 0 {
            assert!(
                !log.contains("session/load"),
                "SID colidido não pode ser usado sozinho para retomar outro contexto"
            );
            let last = log
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .rfind(|v| v["method"] == "session/prompt")
                .unwrap();
            assert!(
                last.to_string().contains("R1_ALPHA_HISTORY_334C"),
                "sessão fresca deve recompor só o histórico persistido próprio"
            );
        }
    }
    // ABA: selected account/model changes and returns; old nonce replays the old result only.
    let account_b = f.connected("b").await;
    f.bind(&work, &chats[0], &account_b, "gpt-fixture-b").await;
    f.send(&work, &chats[0], "CONTEXT_MARKER_A MODEL_B_ADMITTED", "a-model-b").await;
    let requests = f.requests(&work, &chats[0]);
    let delivered = requests.iter().rfind(|v| v["method"] == "session/prompt").unwrap();
    assert_eq!(delivered["selectedModel"], "gpt-fixture-b");
    assert!(delivered["profile"].as_str().unwrap().contains(account_b.as_str().unwrap()));
    f.bind(&work, &chats[0], &account, "gpt-6.1-sol").await;
    let count = f.requests(&work, &chats[0]).iter().filter(|v| v["method"] == "session/prompt").count();
    let replay = f.call("donnaConversationSend", original_intent.clone()).await;
    assert_eq!(replay["replayed"], true);
    assert_eq!(replay["entry"]["id"], first["entry"]["id"]);
    let mut stale = original_intent.clone();
    stale["clientNonce"] = "new-nonce-stale-revision".into();
    f.refused("donnaConversationSend", stale).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(count, f.requests(&work, &chats[0]).iter().filter(|v| v["method"] == "session/prompt").count());
    let c = f.conversation(&work, &chats[0]).await;
    f.call("donnaConversationRead", json!({"spaceId":work,"conversationId":chats[0]})).await;
    assert_eq!(c["revision"], f.conversation(&work, &chats[0]).await["revision"]);
    f.send(&work, &chats[0], "CONTEXT_MARKER_A AFTER_ABA", "a-after-aba").await;
    let records = f.requests(&work, &chats[0]);
    let delivered = records.iter().rfind(|v| v["method"] == "fixture/consumed").unwrap();
    assert_eq!(delivered["selectedModel"], "gpt-6.1-sol");
    assert!(delivered["profile"].as_str().unwrap().contains(account.as_str().unwrap()));
    let c = f.conversation(&work, &chats[0]).await;
    let accounts = f.call("donnaEngineAccounts", json!({})).await;
    let previous = accounts["accounts"].as_array().unwrap().iter().find(|a| a["id"] == account).unwrap();
    let renamed = f
        .call(
            "donnaEngineAccountRename",
            json!({"engineAccountId":account,"expectedRevision":previous["revision"],"label":"Fixture A revisada"}),
        )
        .await;
    f.call(
        "donnaEngineAccountRename",
        json!({"engineAccountId":account,"expectedRevision":renamed["revision"],"label":"Fixture explícita"}),
    )
    .await;
    f.refused("donnaConversationSend",json!({"spaceId":work,"conversationId":chats[0],"expectedRevision":c["revision"],"text":"CONTEXT_MARKER_A NO_STALE_ACCOUNT","clientNonce":"stale-account"})).await;
    f.bind(&work, &chats[0], &account, "gpt-6.1-sol").await;
    f.send(&work, &chats[0], "CONTEXT_MARKER_A FRESH_ACCOUNT_REVISION", "fresh-account-rev").await;
    let c = f.conversation(&work, &chats[0]).await;
    let reset = f
        .call(
            "donnaConversationNewSession",
            json!({"spaceId":work,"conversationId":chats[0],"expectedRevision":c["revision"]}),
        )
        .await;
    assert!(reset["conversation"]["revision"].as_i64().unwrap() > c["revision"].as_i64().unwrap());
    f.restart().await;
    f.send(&work, &chats[0], "CONTEXT_MARKER_A AFTER_RESET", "a-reset").await;
    let records = f.requests(&work, &chats[0]);
    let delivered = records.iter().rfind(|v| v["method"] == "fixture/consumed").unwrap();
    assert!(!delivered.to_string().contains("R1_ALPHA_HISTORY_334C"));
    assert!(!delivered.to_string().contains("R1_BETA_HISTORY_334C"));
    let b = f.conversation(&work, &chats[1]).await;
    f.refused("donnaConversationSend",json!({"spaceId":work,"conversationId":chats[1],"expectedRevision":b["revision"],"text":"CONTEXT_MARKER_B STALE_ACCOUNT_REFUSED","clientNonce":"stale-b"})).await;
    f.bind(&work, &chats[1], &account, "gpt-6.1-sol").await;
    f.send(&work, &chats[1], "CONTEXT_MARKER_B AFTER_RESET_A", "b-after-reset-a").await;
    assert!(
        f.requests(&work, &chats[1])
            .iter()
            .rfind(|v| v["method"] == "fixture/consumed")
            .unwrap()
            .to_string()
            .contains("R1_BETA_HISTORY_334C")
    );
    let c = f.conversation(&work, &chats[0]).await;
    let held=f.call("donnaConversationSend",json!({"spaceId":work,"conversationId":chats[0],"expectedRevision":c["revision"],"text":"CONTEXT_MARKER_A HOLD_A_ONLY","clientNonce":"held-a"})).await;
    let deadline = Instant::now() + Duration::from_secs(10);
    while !f
        .requests(&work, &chats[0])
        .iter()
        .any(|v| v["method"] == "fixture/consumed" && v["text"].as_str().unwrap_or_default().contains("HOLD_A_ONLY"))
    {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let b = f.conversation(&work, &chats[1]).await;
    let queued=f.call("donnaConversationSend",json!({"spaceId":work,"conversationId":chats[1],"expectedRevision":b["revision"],"text":"CONTEXT_MARKER_B AFTER_CANCEL","clientNonce":"queued-b"})).await;
    f.refused("donnaConversationStop",json!({"spaceId":work,"conversationId":chats[0],"expectedRevision":c["revision"],"entryId":queued["entry"]["id"]})).await;
    f.refused(
        "donnaConversationNewSession",
        json!({"spaceId":work,"conversationId":chats[0],"expectedRevision":c["revision"]}),
    )
    .await;
    f.refused("donnaConversationUpdate",json!({"spaceId":work,"conversationId":chats[0],"expectedRevision":c["revision"],"engineAccountId":account_b,"expectedAccountRevision":1})).await;
    f.call("stop", json!({"botId":donna})).await;
    assert!(!f.requests(&work, &chats[0]).iter().any(|v| v["method"] == "session/cancel"));
    f.call("donnaConversationStop",json!({"spaceId":work,"conversationId":chats[0],"expectedRevision":c["revision"],"entryId":held["entry"]["id"]})).await;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let page = f.call("donnaConversationHistory", json!({"spaceId":work,"conversationId":chats[1]})).await;
        if page["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "agent" && e["turn"] == queued["entry"]["turn"] && e["data"]["final"] == true)
        {
            assert!(!page.to_string().contains("LATE_A_ONLY"));
            break;
        }
        assert!(Instant::now() < deadline, "{page}");
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    let personal = f.call("donnaConversations", json!({})).await;
    // D366: o Berga fixo do Hub novo é o da fixture; nenhuma cópia por Space.
    let personal = personal["conversations"].as_array().unwrap();
    assert_eq!(personal.len(), 1);
    assert_eq!(personal.iter().filter(|c| c["botId"] == donna).count(), 1);
    let db = rusqlite::Connection::open(f.home.join("codync.db")).unwrap();
    let original: String =
        db.query_row("SELECT config FROM bots WHERE id=?1", [donna.as_str().unwrap()], |row| row.get(0)).unwrap();
    let original: Value = serde_json::from_str(&original).unwrap();
    assert_eq!(original["backend"], "codex");
    assert_eq!(original["model"], "gpt-6.1-sol");
    assert_eq!(original["forkEditor"]["engineAccountId"], account);
    // D366: só o Berga fixo do Hub novo.
    assert_eq!(db.query_row("SELECT COUNT(*) FROM bots WHERE deleted=0", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
}

#[tokio::test]
async fn review_r1_svg_dimensions_and_png_payload_validation_over_http() {
    use base64::Engine as _;
    let f = Fixture::new().await;
    let mut state = f.call("donnaSpaces", json!({})).await;
    let mut observed = Vec::new();
    for (label, svg, expected_ok) in [
        (
            "benign_1024",
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1024\" height=\"1024\" viewBox=\"0 0 1024 1024\"><rect width=\"10\" height=\"10\"/></svg>",
            true,
        ),
        (
            "oversize_1025",
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1025\" height=\"1025\"><rect width=\"10\" height=\"10\"/></svg>",
            false,
        ),
        (
            "oversize_1b",
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1000000000\" height=\"1000000000\"><rect width=\"10\" height=\"10\"/></svg>",
            false,
        ),
        ("px_1024", r#"<svg width="1024px" height="1024px"/>"#, true),
        ("viewbox_only", r#"<svg viewBox="0 0 1024 1024"/>"#, true),
        ("nan", r#"<svg width="NaN" height="1"/>"#, false),
        ("negative", r#"<svg width="-1" height="1"/>"#, false),
        ("overflow", r#"<svg width="1e999" height="1"/>"#, false),
        ("percent", r#"<svg width="100%" height="100%"/>"#, false),
        ("em", r#"<svg width="1em" height="1em"/>"#, false),
        ("missing", r#"<svg width="1"/>"#, false),
        ("viewbox_oversize", r#"<svg width="1" height="1" viewBox="0 0 1025 1"/>"#, false),
        ("viewbox_negative_size", r#"<svg viewBox="0 0 -1 1"/>"#, false),
        ("viewbox_nonfinite", r#"<svg viewBox="0 0 NaN 1"/>"#, false),
        ("viewbox_extreme_origin", r#"<svg viewBox="1000000000 0 1 1"/>"#, false),
        ("viewbox_wrong_arity", r#"<svg viewBox="0 0 1"/>"#, false),
        ("active_denied", "<svg onload=\"alert(1)\"/>", false),
    ] {
        let value = format!("data:image/svg+xml;base64,{}", base64::engine::general_purpose::STANDARD.encode(svg));
        let request = json!({"hubId":f.hub_id,"name":label,"expectedRevision":state["revision"],"tile":{"color":"oceano","style":"soft","symbol":{"kind":"logo","value":value}}});
        let (status, response) = f.raw("donnaSpaceCreate", request).await;
        let ok = (200..300).contains(&status);
        observed.push(json!({"case":label,"status":status,"expectedOk":expected_ok,"actualOk":ok}));
        if ok {
            state = response;
        }
    }
    println!("R1_ASSET_OBSERVATIONS={}", json!(observed));
    assert!(
        observed.iter().all(|o| o["expectedOk"] == o["actualOk"]),
        "SVG dimension contract failed: {}",
        json!(observed)
    );
}

#[tokio::test]
async fn review_r1_subscription_retry_must_reach_additional_conversation() {
    let f = Fixture::new().await;
    let account = f.connected("r1-rate").await;
    let donna = f.call("teamSettings", json!({})).await["primaryBotId"].clone();
    f.call("updateBot",json!({"id":donna,"backend":"codex","cwd":f.workspaces.root(),"permission":"ask","permissionChosenByUser":true,"model":"gpt-6.1-sol","forkEditor":{"engineAccountId":account}})).await;
    f.call("setTeamSettings", json!({"primaryBotId":donna,"permission":"ask"})).await;
    let initial = f.call("donnaSpaces", json!({})).await;
    let spaces=f.call("donnaSpaceCreate",json!({"name":"R1 Rate","expectedRevision":initial["revision"],"tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"RR"}}})).await;
    let work = spaces["spaces"][1]["id"].clone();
    let chat =
        f.call("donnaConversationCreate", json!({"spaceId":work,"botId":donna})).await["conversation"]["id"].clone();
    let c = f.bind(&work, &chat, &account, "gpt-6.1-sol").await;
    let sent=f.call("donnaConversationSend",json!({"spaceId":work,"conversationId":chat,"expectedRevision":c["revision"],"text":"CONTEXT_MARKER_A R1_RATE_LIMIT","clientNonce":"r1-rate"})).await;
    let deadline = Instant::now() + Duration::from_secs(10);
    let page = loop {
        let page = f.call("donnaConversationHistory", json!({"spaceId":work,"conversationId":chat})).await;
        if page["entries"].as_array().unwrap().iter().any(|e| e["data"]["subscriptionWait"].is_object()) {
            break page;
        }
        assert!(Instant::now() < deadline, "{page}");
        tokio::time::sleep(Duration::from_millis(40)).await;
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    let count = f.requests(&work, &chat).iter().filter(|e| e["method"] == "session/prompt").count();
    std::fs::write(
        f.home
            .join("spaces")
            .join(work.as_str().unwrap())
            .join("conversations")
            .join(chat.as_str().unwrap())
            .join("workspace/fixture-release-limit"),
        "synthetic release",
    )
    .unwrap();
    let retry=f.raw("donnaConversationRetry",json!({"hubId":f.hub_id,"spaceId":work,"conversationId":chat,"expectedRevision":c["revision"],"entryId":sent["entry"]["id"]})).await;
    let has_wait = page["entries"].as_array().unwrap().iter().any(|e| e["data"]["subscriptionWait"].is_object());
    println!(
        "R1_RETRY_OBSERVATIONS={}",
        json!({"page":page,"retryStatus":retry.0,"retry":retry.1,"hasWait":has_wait,"prompts":count})
    );
    assert!(has_wait && (200..300).contains(&retry.0), "new conversation Retry has no reachable subscription wait");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let history = f.call("donnaConversationHistory", json!({"spaceId":work,"conversationId":chat})).await;
        if history["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["turn"] == sent["entry"]["turn"] && e["kind"] == "agent" && e["data"]["final"] == true)
        {
            break;
        }
        assert!(Instant::now() < deadline, "retomada sem conclusão: {history}");
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    let records = f.requests(&work, &chat);
    let delivered = records.iter().filter(|e| e["method"] == "fixture/consumed").collect::<Vec<_>>();
    assert_eq!(delivered.len(), 2, "um Retry admitido deve retomar uma vez");
    for prompt in &delivered {
        assert_eq!(prompt["selectedModel"], "gpt-6.1-sol");
        assert!(prompt["profile"].as_str().unwrap().contains(account.as_str().unwrap()));
    }
    assert!(!records.iter().any(|e| e["method"] == "session/load"));
    assert!(delivered[1]["text"].as_str().unwrap().contains("Não repita ações já concluídas"));
    f.refused(
        "donnaConversationRetry",
        json!({"spaceId":work,"conversationId":chat,"expectedRevision":c["revision"],"entryId":sent["entry"]["id"]}),
    )
    .await;
    println!("R2_RETRY_CONSUMED={}", json!(delivered));
}

#[tokio::test]
async fn review_r1_legacy_history_cannot_ignore_selected_conversation() {
    let f = Fixture::new().await;
    let bot = f.call("createBot", json!({"name":"R1 main identity","backend":"codex","cwd":f.workspaces.root()})).await
        ["bot"]["id"]
        .clone();
    let personal = f.call("donnaSpaces", json!({})).await["personalSpaceId"].clone();
    let entry = f.call("logCall", json!({"botId":bot,"seconds":1337})).await["entry"]["id"].clone();
    let a = f
        .call("donnaConversationCreate", json!({"spaceId":personal,"botId":bot,"title":"R1 additional selected"}))
        .await["conversation"]["id"]
        .clone();
    let isolated = f.call("donnaConversationHistory", json!({"spaceId":personal,"conversationId":a})).await;
    assert_eq!(isolated["entries"].as_array().unwrap().as_slice(), [] as [Value; 0]);
    let benign = f.raw("history", json!({"spaceId":personal,"conversationId":bot,"botId":bot})).await;
    assert_eq!(benign.0, 200);
    assert!(benign.1["entries"].as_array().unwrap().iter().any(|e| e["id"] == entry));
    let crossed = f.raw("history", json!({"spaceId":personal,"conversationId":a,"botId":bot})).await;
    println!(
        "R1_GENERIC_CONTEXT={}",
        json!({"selectedConversation":a,"mainIdentity":bot,"mainEntry":entry,"isolated":isolated,"benignStatus":benign.0,"crossedStatus":crossed.0,"crossed":crossed.1})
    );
    assert!(
        !(200..300).contains(&crossed.0),
        "legacy history ignored explicit additional conversation and returned main transcript"
    );
    for method in ["send", "memory", "saveMemory", "readUpload", "upload", "routines", "stop", "newSession", "markRead"]
    {
        f.refused(method,json!({"spaceId":personal,"conversationId":a,"botId":bot,"text":"R2_MUST_NOT_WRITE","uploadId":uuid::Uuid::new_v4().to_string(),"offset":0,"name":"test.txt","data":"QQ=="})).await;
    }
    f.refused("routinesOverview", json!({"spaceId":personal,"conversationId":a})).await;
    f.refused("updateBot", json!({"spaceId":personal,"conversationId":a,"id":bot,"name":"R2_MUST_NOT_CHANGE"})).await;
    let after = f.call("donnaConversationHistory", json!({"spaceId":personal,"conversationId":a})).await;
    assert_eq!(after["entries"].as_array().unwrap().as_slice(), [] as [Value; 0]);
    let proper = f.raw("history", json!({"spaceId":personal,"conversationId":a,"botId":a})).await;
    assert_eq!(proper.0, 200);
    assert_eq!(proper.1["entries"].as_array().unwrap().as_slice(), [] as [Value; 0]);
}

#[tokio::test]
async fn review_r1_forged_upload_manifest_cannot_choose_absolute_path() {
    use sha2::{Digest, Sha256};
    let f = Fixture::new().await;
    let bot = f.call("createBot", json!({"name":"R1 files","backend":"codex","cwd":f.workspaces.root()})).await["bot"]
        ["id"]
        .clone();
    let personal = f.call("donnaSpaces", json!({})).await["personalSpaceId"].clone();
    let c = f.call("donnaConversationCreate", json!({"spaceId":personal,"botId":bot})).await["conversation"].clone();
    let upload = uuid::Uuid::new_v4().to_string();
    f.call("donnaConversationUpload",json!({"spaceId":personal,"conversationId":c["id"],"expectedRevision":c["revision"],"uploadId":upload,"name":"normal.txt","offset":0,"data":"QQ==","done":true})).await;
    let good = f
        .call(
            "donnaConversationReadUpload",
            json!({"spaceId":personal,"conversationId":c["id"],"uploadId":upload,"offset":0}),
        )
        .await;
    assert_eq!(good["data"], "QQ==");
    let target = f.home.join("r1-synthetic-outside-conversation.txt");
    let content = b"R1_OUTSIDE_CAPTURED_CONVERSATION_334C";
    std::fs::write(&target, content).unwrap();
    let digest = hash_hex(&Sha256::digest(content));
    let db = rusqlite::Connection::open(f.home.join("codync.db")).unwrap();
    db.execute(
        "UPDATE donna_space_uploads SET name=?1,size=?2,sha256=?3 WHERE conversation_id=?4 AND id=?5",
        rusqlite::params![
            target.to_str().unwrap(),
            i64::try_from(content.len()).unwrap(),
            digest,
            c["id"].as_str().unwrap(),
            upload
        ],
    )
    .unwrap();
    drop(db);
    let crossed = f
        .raw(
            "donnaConversationReadUpload",
            json!({"hubId":f.hub_id,"spaceId":personal,"conversationId":c["id"],"uploadId":upload,"offset":0}),
        )
        .await;
    println!(
        "R1_FORGED_MANIFEST={}",
        json!({"status":crossed.0,"response":crossed.1,"syntheticTarget":target,"validUploadRead":good["data"]})
    );
    assert!(!(200..300).contains(&crossed.0), "forged manifest selected a file outside its conversation");
    let valid=f.raw("donnaConversationUpload",json!({"hubId":f.hub_id,"spaceId":personal,"conversationId":c["id"],"expectedRevision":c["revision"],"uploadId":upload,"name":"normal.txt","offset":0,"data":"QQ==","done":true})).await;
    assert_eq!(valid.0, 400, "append também deve validar o manifesto antes de abrir");
    let db = rusqlite::Connection::open(f.home.join("codync.db")).unwrap();
    for bad in ["../outside", "..", ".", "a/b", "a\\b", "a:b", "a\0b", "a\nb"] {
        db.execute("UPDATE donna_space_uploads SET name=?1 WHERE id=?2", rusqlite::params![bad, upload]).unwrap();
        let res = f
            .raw(
                "donnaConversationReadUpload",
                json!({"hubId":f.hub_id,"spaceId":personal,"conversationId":c["id"],"uploadId":upload,"offset":0}),
            )
            .await;
        assert_eq!(res.0, 400, "nome {bad:?}: {}", res.1);
    }
    let benign_digest = hash_hex(&Sha256::digest(b"A"));
    for mutation in [
        "size=-1",
        "size=104857601",
        "complete=2",
        "revision=0",
        "sha256='not-a-digest'",
        "complete=0,sha256='not-a-digest'",
    ] {
        db.execute(
            "UPDATE donna_space_uploads SET name='normal.txt',size=1,complete=1,revision=5,sha256=?1 WHERE id=?2",
            rusqlite::params![benign_digest, upload],
        )
        .unwrap();
        db.execute(&format!("UPDATE donna_space_uploads SET {mutation} WHERE id=?1"), [&upload]).unwrap();
        let res = f
            .raw(
                "donnaConversationReadUpload",
                json!({"hubId":f.hub_id,"spaceId":personal,"conversationId":c["id"],"uploadId":upload,"offset":0}),
            )
            .await;
        assert_eq!(res.0, 400, "manifesto {mutation}: {}", res.1);
    }
    db.execute(
        "UPDATE donna_space_uploads SET name='normal.txt',size=1,complete=1,revision=5,sha256=?1 WHERE id=?2",
        rusqlite::params![benign_digest, upload],
    )
    .unwrap();
    drop(db);
    assert_eq!(
        f.call(
            "donnaConversationReadUpload",
            json!({"spaceId":personal,"conversationId":c["id"],"uploadId":upload,"offset":0})
        )
        .await["data"],
        "QQ=="
    );
}

#[tokio::test]
async fn review_r1_real_crud_groups_sections_upload_boundaries_and_reopen() {
    use base64::Engine as _;
    let mut f = Fixture::new().await;
    let account = f.connected("r1-crud").await;
    let initial = f.call("donnaSpaces", json!({})).await;
    let personal = initial["personalSpaceId"].clone();
    let state=f.call("donnaSpaceCreate",json!({"name":"R1 scoped","expectedRevision":initial["revision"],"tile":{"color":"musgo","style":"filled","symbol":{"kind":"letters","value":"R1"}}})).await;
    let work = state["spaces"][1]["id"].clone();
    let old = f.call("createBot", json!({"name":"R1 personal","backend":"codex"})).await["bot"]["id"].clone();
    let a = f
        .call(
            "createBot",
            json!({"hubId":f.hub_id,"spaceId":work,"name":"R1 A","backend":"codex",
        "forkEditor":{"engineAccountId":account}}),
        )
        .await["bot"]
        .clone();
    let b = f
        .call(
            "createBot",
            json!({"hubId":f.hub_id,"spaceId":work,"name":"R1 B","backend":"codex",
        "forkEditor":{"engineAccountId":account}}),
        )
        .await["bot"]
        .clone();
    assert_eq!(a["spaceId"], work);
    assert_eq!(a["skills"], json!([]));
    assert_eq!(a["sharedMemory"], false);
    f.refused("history", json!({"botId":a["id"]})).await;
    f.call("updateBot", json!({"spaceId":work,"id":a["id"],"name":"R1 A updated"})).await;
    f.refused("updateBot", json!({"spaceId":work,"id":a["id"],"patch":{"spaceId":personal}})).await;
    f.refused(
        "createBot",
        json!({"hubId":f.hub_id,"spaceId":work,"name":"R1 mixed denied","kind":"group","members":[a["id"],old]}),
    )
    .await;
    let group = f
        .call(
            "createBot",
            json!({"hubId":f.hub_id,"spaceId":work,"name":"R1 real group","kind":"group","members":[a["id"],b["id"]]}),
        )
        .await["bot"]
        .clone();
    assert_eq!(group["spaceId"], work);
    assert_eq!(group["members"], json!([a["id"], b["id"]]));
    let added = f.call("botGroupsEdit", json!({"spaceId":work,"op":"add","name":"R1 group"})).await;
    let gid = added["created"].clone();
    f.call("botGroupsEdit", json!({"spaceId":work,"op":"place","botId":a["id"],"groupId":gid})).await;
    let modern = f.call("botGroups", json!({"spaceId":work})).await;
    assert_eq!(modern["groups"][0]["bots"], json!([a["id"]]));
    f.refused("botGroupsEdit", json!({"spaceId":personal,"op":"rename","id":gid,"name":"crossed"})).await;
    let c = f.call("donnaConversationCreate", json!({"spaceId":work,"botId":a["id"]})).await["conversation"].clone();
    let upload = uuid::Uuid::new_v4().to_string();
    let part = json!({"spaceId":work,"conversationId":c["id"],"expectedRevision":c["revision"],"uploadId":upload,"name":"r1.txt","offset":0,"data":"QUJD","done":false});
    f.call("donnaConversationUpload", part.clone()).await;
    f.restart().await;
    f.call("donnaConversationUpload", part.clone()).await;
    for (field, value) in [("name", json!("other.txt")), ("data", json!("WFla")), ("offset", json!(4))] {
        let mut bad = part.clone();
        bad[field] = value;
        f.refused("donnaConversationUpload", bad).await;
    }
    let mut too_big = part.clone();
    too_big["data"] = base64::engine::general_purpose::STANDARD.encode(vec![0_u8; 384 * 1024 + 1]).into();
    f.refused("donnaConversationUpload", too_big).await;
    let mut past_total = part.clone();
    past_total["offset"] = json!(100 * 1024 * 1024);
    f.refused("donnaConversationUpload", past_total).await;
    let mut final_part = part.clone();
    final_part["offset"] = json!(3);
    final_part["data"] = json!("RA==");
    final_part["done"] = true.into();
    f.call("donnaConversationUpload", final_part.clone()).await;
    assert_eq!(f.call("donnaConversationUpload", final_part).await["replayed"], true);
    let read = f
        .call(
            "donnaConversationReadUpload",
            json!({"spaceId":work,"conversationId":c["id"],"uploadId":upload,"offset":0}),
        )
        .await;
    assert_eq!(read["data"], "QUJDRA==");
    let path = f
        .home
        .join("spaces")
        .join(work.as_str().unwrap())
        .join("conversations")
        .join(c["id"].as_str().unwrap())
        .join("workspace/uploads")
        .join(&upload)
        .join("r1.txt");
    std::fs::write(&path, "WXYZ").unwrap();
    f.refused(
        "donnaConversationReadUpload",
        json!({"spaceId":work,"conversationId":c["id"],"uploadId":upload,"offset":0}),
    )
    .await;
    std::fs::write(&path, "ABCD").unwrap();
    f.call("donnaConversationMemory",json!({"spaceId":work,"conversationId":c["id"],"expectedRevision":c["revision"],"method":"saveMemory","text":"R1_RESET_MEMORY_PRESERVED"})).await;
    f.call(
        "donnaConversationNewSession",
        json!({"spaceId":work,"conversationId":c["id"],"expectedRevision":c["revision"]}),
    )
    .await;
    assert_eq!(
        f.call(
            "donnaConversationReadUpload",
            json!({"spaceId":work,"conversationId":c["id"],"uploadId":upload,"offset":0})
        )
        .await["data"],
        "QUJDRA=="
    );
    assert!(
        f.call("donnaConversationMemory", json!({"spaceId":work,"conversationId":c["id"],"method":"memory"}))
            .await
            .to_string()
            .contains("R1_RESET_MEMORY_PRESERVED")
    );
    f.call("deleteBot", json!({"spaceId":work,"botId":b["id"]})).await;
    println!(
        "R1_BOUNDARIES_OK real bots/group, upload replay/name/offset/chunk/total/hash/reopen, reset preserves memory/upload"
    );
}

#[tokio::test]
async fn review_r1_sse_scopes_real_ids_and_rejects_unknown_space() {
    use futures::StreamExt;
    let f = Fixture::new().await;
    let account = f.connected("r1-sse").await;
    let initial = f.call("donnaSpaces", json!({})).await;
    let personal = initial["personalSpaceId"].clone();
    let state=f.call("donnaSpaceCreate",json!({"name":"R1 stream","expectedRevision":initial["revision"],"tile":{"color":"oceano","style":"filled","symbol":{"kind":"letters","value":"RS"}}})).await;
    let work = state["spaces"][1]["id"].clone();
    let p = f.call("createBot", json!({"name":"R1 personal stream","backend":"codex"})).await["bot"]["id"].clone();
    let w = f
        .call(
            "createBot",
            json!({"hubId":f.hub_id,"spaceId":work,"name":"R1 work stream","backend":"codex",
        "forkEditor":{"engineAccountId":account}}),
        )
        .await["bot"]["id"]
        .clone();
    let pe = f.call("logCall", json!({"botId":p,"seconds":31337})).await["entry"]["id"].clone();
    let we = f.call("logCall", json!({"spaceId":work,"botId":w,"seconds":31338})).await["entry"]["id"].clone();
    for (query, expected, allowed, denied) in [
        (String::new(), personal.clone(), pe.clone(), we.clone()),
        (format!("&spaceId={}", work.as_str().unwrap()), work.clone(), we, pe),
    ] {
        let response = reqwest::Client::new()
            .get(format!("{}/events?since=0&client=r1{query}", f.base))
            .bearer_auth(&f.token)
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        let mut stream = response.bytes_stream();
        let mut raw = String::new();
        while let Ok(Some(Ok(chunk))) = tokio::time::timeout(Duration::from_millis(150), stream.next()).await {
            raw.push_str(&String::from_utf8_lossy(&chunk));
        }
        let events = raw
            .lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .collect::<Vec<_>>();
        assert!(events.iter().any(|e| e["type"] == "hello" && e["hostId"] == f.hub_id && e["spaceId"] == expected));
        assert!(events.iter().any(|e| e["entry"]["id"] == allowed));
        assert!(!events.iter().any(|e| e["entry"]["id"] == denied));
        println!(
            "R1_SSE_OBSERVATIONS={}",
            json!({"spaceId":expected,"hubId":f.hub_id,"events":events.len(),"allowed":allowed,"denied":denied})
        );
    }
    let response = reqwest::Client::new()
        .get(format!("{}/events?since=0&spaceId=99999999-9999-4999-8999-999999999999", f.base))
        .bearer_auth(&f.token)
        .send()
        .await
        .unwrap();
    assert!(!response.status().is_success());
}

#[tokio::test]
async fn review_r1_retry_does_not_acknowledge_an_ordinary_active_turn() {
    let f = Fixture::new().await;
    let account = f.connected("r1-retry-active").await;
    let bot = f
        .call(
            "createBot",
            json!({"name":"R1 retry","backend":"codex","model":"gpt-6.1-sol","forkEditor":{"engineAccountId":account}}),
        )
        .await["bot"]["id"]
        .clone();
    let personal = f.call("donnaSpaces", json!({})).await["personalSpaceId"].clone();
    let chat =
        f.call("donnaConversationCreate", json!({"spaceId":personal,"botId":bot})).await["conversation"]["id"].clone();
    let c = f.bind(&personal, &chat, &account, "gpt-6.1-sol").await;
    let sent=f.call("donnaConversationSend",json!({"spaceId":personal,"conversationId":chat,"expectedRevision":c["revision"],"text":"CONTEXT_MARKER_A HOLD_A_ONLY","clientNonce":"r1-retry-held"})).await;
    let deadline = Instant::now() + Duration::from_secs(10);
    while !f.requests(&personal, &chat).iter().any(|v| v["method"] == "fixture/consumed") {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let retry=f.raw("donnaConversationRetry",json!({"hubId":f.hub_id,"spaceId":personal,"conversationId":chat,"expectedRevision":c["revision"],"entryId":sent["entry"]["id"]})).await;
    let count = f.requests(&personal, &chat).iter().filter(|v| v["method"] == "session/prompt").count();
    println!(
        "R1_RETRY_ACTIVE={}",
        json!({"status":retry.0,"response":retry.1,"prompts":count,"actualState":"ordinary active turn, no subscription wait"})
    );
    f.call("donnaConversationStop",json!({"spaceId":personal,"conversationId":chat,"expectedRevision":c["revision"],"entryId":sent["entry"]["id"]})).await;
    assert!(
        !(200..300).contains(&retry.0),
        "Retry claimed acceptance for an ordinary running turn with no subscription wait"
    );
}

// R2: checkpoint/ABA/Stop/restart alcançam o mesmo ator e não resolvem outro binding.
#[tokio::test]
async fn r2_waiting_context_survives_restart_and_rejects_aba_and_uncertain_auto_retry() {
    let mut f = Fixture::new().await;
    let account = f.connected("r2-wait").await;
    let bot = f.call("teamSettings", json!({})).await["primaryBotId"].clone();
    f.call("updateBot",json!({"id":bot,"backend":"codex","model":"gpt-6.1-sol","ownerPreferences":"R1_PERSONAL_PROMPT_334C","forkEditor":{"engineAccountId":account}})).await;
    f.call("setTeamSettings", json!({"primaryBotId":bot,"permission":"ask"})).await;
    let initial = f.call("donnaSpaces", json!({})).await;
    let state=f.call("donnaSpaceCreate",json!({"name":"R2 wait","expectedRevision":initial["revision"],"tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"RW"}}})).await;
    let work = state["spaces"][1]["id"].clone();
    let a =
        f.call("donnaConversationCreate", json!({"spaceId":work,"botId":bot,"title":"A"})).await["conversation"]["id"]
            .clone();
    let b =
        f.call("donnaConversationCreate", json!({"spaceId":work,"botId":bot,"title":"B"})).await["conversation"]["id"]
            .clone();
    let c = f.bind(&work, &a, &account, "gpt-6.1-sol").await;
    let original = json!({"spaceId":work,"conversationId":a,"expectedRevision":c["revision"],"text":"CONTEXT_MARKER_A R1_RATE_LIMIT R2_WAIT_ORIGINAL","clientNonce":"r2-wait-1"});
    let sent = f.call("donnaConversationSend", original.clone()).await;
    let wait = |f: &Fixture| {
        let db = rusqlite::Connection::open(f.home.join("codync.db")).unwrap();
        db.query_row("SELECT v FROM kv WHERE k=?1", [format!("turn.waiting.{}", bot.as_str().unwrap())], |r| {
            r.get::<_, String>(0)
        })
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while !wait(&f).is_some_and(|w| w["notice"].is_string()) {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let checkpoint = wait(&f).unwrap();
    assert_eq!(checkpoint["input"]["chat"], a);
    assert_eq!(checkpoint["input"]["contextBinding"], sent["entry"]["data"]["executionBinding"]);
    f.refused("donnaConversationRetry",json!({"spaceId":work,"conversationId":b,"expectedRevision":f.conversation(&work,&b).await["revision"],"entryId":sent["entry"]["id"]})).await;
    f.refused(
        "donnaConversationRetry",
        json!({"spaceId":work,"conversationId":a,"expectedRevision":c["revision"],"entryId":checkpoint["notice"]}),
    )
    .await;
    f.refused(
        "donnaConversationUpdate",
        json!({"spaceId":work,"conversationId":a,"expectedRevision":c["revision"],"engineAccountId":Value::Null}),
    )
    .await;
    f.refused(
        "donnaConversationNewSession",
        json!({"spaceId":work,"conversationId":a,"expectedRevision":c["revision"]}),
    )
    .await;
    f.restart().await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(wait(&f).unwrap()["input"]["contextBinding"], checkpoint["input"]["contextBinding"]);
    assert_eq!(
        f.requests(&work, &a).iter().filter(|e| e["method"] == "fixture/consumed").count(),
        1,
        "espera limpa não é replay no startup"
    );
    let accounts = f.call("donnaEngineAccounts", json!({})).await;
    let rev = accounts["accounts"].as_array().unwrap().iter().find(|v| v["id"] == account).unwrap()["revision"].clone();
    let renamed = f
        .call("donnaEngineAccountRename", json!({"engineAccountId":account,"expectedRevision":rev,"label":"R2 ABA"}))
        .await;
    f.call(
        "donnaEngineAccountRename",
        json!({"engineAccountId":account,"expectedRevision":renamed["revision"],"label":"Fixture explícita"}),
    )
    .await;
    f.refused(
        "donnaConversationRetry",
        json!({"spaceId":work,"conversationId":a,"expectedRevision":c["revision"],"entryId":sent["entry"]["id"]}),
    )
    .await;
    // A mesma revisão é revalidada também na admissão automática após reinício.
    let db = rusqlite::Connection::open(f.home.join("codync.db")).unwrap();
    db.execute(
        "UPDATE kv SET v=json_set(v,'$.retryAt',0) WHERE k=?1",
        [format!("turn.waiting.{}", bot.as_str().unwrap())],
    )
    .unwrap();
    drop(db);
    f.restart().await;
    let deadline = Instant::now() + Duration::from_secs(10);
    while wait(&f).is_none_or(|w| w["phase"] != "paused") {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    f.refused(
        "donnaConversationRetry",
        json!({"spaceId":work,"conversationId":a,"expectedRevision":c["revision"],"entryId":sent["entry"]["id"]}),
    )
    .await;
    assert_eq!(
        f.call("donnaConversationSend", original).await["replayed"],
        true,
        "nonce antigo devolve resultado, não retoma"
    );
    assert_eq!(f.requests(&work, &a).iter().filter(|e| e["method"] == "fixture/consumed").count(), 1);
    f.call(
        "donnaConversationStop",
        json!({"spaceId":work,"conversationId":a,"expectedRevision":c["revision"],"entryId":sent["entry"]["id"]}),
    )
    .await;
    f.refused(
        "donnaConversationRetry",
        json!({"spaceId":work,"conversationId":a,"expectedRevision":c["revision"],"entryId":sent["entry"]["id"]}),
    )
    .await;
    let c = f.bind(&work, &a, &account, "gpt-6.1-sol").await;
    let sent=f.call("donnaConversationSend",json!({"spaceId":work,"conversationId":a,"expectedRevision":c["revision"],"text":"CONTEXT_MARKER_A R1_RATE_LIMIT R2_UNCERTAIN","clientNonce":"r2-wait-2"})).await;
    let deadline = Instant::now() + Duration::from_secs(10);
    while !wait(&f).is_some_and(|w| w["input"]["entries"][0] == sent["entry"]["id"] && w["notice"].is_string()) {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let cwd = f
        .home
        .join("spaces")
        .join(work.as_str().unwrap())
        .join("conversations")
        .join(a.as_str().unwrap())
        .join("workspace");
    std::fs::write(cwd.join("fixture-release-limit"), "synthetic").unwrap();
    std::fs::write(cwd.join("fixture-hold-retry"), "synthetic").unwrap();
    f.call(
        "donnaConversationRetry",
        json!({"spaceId":work,"conversationId":a,"expectedRevision":c["revision"],"entryId":sent["entry"]["id"]}),
    )
    .await;
    let deadline = Instant::now() + Duration::from_secs(10);
    while f.requests(&work, &a).iter().filter(|e| e["method"] == "fixture/consumed").count() < 3 {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert_eq!(wait(&f).unwrap()["phase"], "retrying");
    f.restart().await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(wait(&f).unwrap()["phase"], "paused");
    assert_eq!(
        f.requests(&work, &a).iter().filter(|e| e["method"] == "fixture/consumed").count(),
        3,
        "tentativa incerta não é repetida no restart"
    );
    std::fs::remove_file(cwd.join("fixture-hold-retry")).unwrap();
    f.call(
        "donnaConversationRetry",
        json!({"spaceId":work,"conversationId":a,"expectedRevision":c["revision"],"entryId":sent["entry"]["id"]}),
    )
    .await;
    let deadline = Instant::now() + Duration::from_secs(10);
    while wait(&f).is_some_and(|w| w["phase"] != "finished") {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let records = f.requests(&work, &a);
    let consumed = records.iter().filter(|e| e["method"] == "fixture/consumed").collect::<Vec<_>>();
    assert_eq!(consumed.len(), 4);
    for v in &consumed {
        assert!(v["profile"].as_str().unwrap().contains(account.as_str().unwrap()));
        assert_eq!(v["selectedModel"], "gpt-6.1-sol");
        assert_eq!(v["cwd"], cwd.to_str().unwrap());
        assert!(!v["text"].as_str().unwrap().contains("R1_PERSONAL_PROMPT_334C"));
    }
    assert!(!records.iter().any(|e| e["method"] == "session/load"));
    assert_eq!(f.requests(&work, &b).as_slice(), [] as [Value; 0]);
    println!("R2_WAIT_RESTART_ABA_OK consumed={} contextBindingCaptured=true", consumed.len());
}
