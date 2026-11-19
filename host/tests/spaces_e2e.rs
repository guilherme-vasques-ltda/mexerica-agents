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
        for (name, source) in [("codex", "engine_setup_cli.py"), ("codex-acp", "spaces_agent.py")] {
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
async fn create_bot_http_em_dois_hosts_preserva_hub_space_e_conta_originais() {
    async fn context(f: &Fixture, key: &str) -> (Value, Value, Value) {
        let account = f.connected(key).await;
        let accounts = f.call("donnaEngineAccounts", json!({})).await;
        let selected = accounts["accounts"].as_array().unwrap().iter().find(|a| a["id"] == account).unwrap();
        assert_eq!(selected["state"], "connected");
        let primary = f.call("teamSettings", json!({})).await["primaryBotId"].clone();
        // Conta pessoal válida disponível: omitir conta no Space nunca pode herdá-la.
        f.call(
            "updateBot",
            json!({"id":primary,"backend":"codex","cwd":f.workspaces.root(),
            "forkEditor":{"engineAccountId":account}}),
        )
        .await;
        let initial = f.call("donnaSpaces", json!({})).await;
        let spaces = f
            .call(
                "donnaSpaceCreate",
                json!({"name":"Space sintético D543",
            "expectedRevision":initial["revision"],
            "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}}}),
            )
            .await;
        (initial["personalSpaceId"].clone(), spaces["spaces"][1]["id"].clone(), account)
    }
    fn persisted(f: &Fixture) -> (String, String, i64, i64) {
        // Apenas DB descartável criada por esta fixture; nenhuma instalação real.
        let db =
            rusqlite::Connection::open_with_flags(f.home.join("codync.db"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        db.query_row(
            "SELECT
                (SELECT json_group_array(json_array(id,config,deleted,rev)) FROM (SELECT * FROM bots ORDER BY id)),
                (SELECT json_group_array(json_array(id,space_id,bot_id,revision,engine_account_id))
                    FROM (SELECT * FROM donna_space_conversations ORDER BY id)),
                (SELECT COUNT(*) FROM entries),
                COALESCE((SELECT CAST(v AS INTEGER) FROM kv WHERE k='rev'),0)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap()
    }

    let a = Fixture::new().await;
    let b = Fixture::new().await;
    assert_ne!(a.child.id(), b.child.id());
    assert_ne!(a.base, b.base);
    assert_ne!(a.home, b.home);
    assert_ne!(a.hub_id, b.hub_id);
    let (personal_a, work_a, account_a) = context(&a, "d543-a").await;
    let (personal_b, work_b, account_b) = context(&b, "d543-b").await;
    assert_ne!(work_a, work_b);
    assert_ne!(account_a, account_b);

    for (f, other, personal, work, account, foreign_work, foreign_account) in [
        (&a, &b, &personal_a, &work_a, &account_a, &work_b, &account_b),
        (&b, &a, &personal_b, &work_b, &account_b, &work_a, &account_a),
    ] {
        let before = persisted(f);
        let other_before = persisted(other);
        let body = json!({"hubId":f.hub_id,"spaceId":work,"name":"Bot sintético D543",
            "backend":"codex","model":"gpt-6.1-sol","cwd":f.workspaces.root(),
            "forkEditor":{"engineAccountId":account}});
        for (case, expected) in [
            ("hub_ausente", "hub_context_mismatch"),
            ("hub_de_outro_host", "hub_context_mismatch"),
            ("pessoal_hub_ausente", "hub_context_mismatch"),
            ("pessoal_hub_de_outro_host", "hub_context_mismatch"),
            ("space_de_outro_host", "space_not_found"),
            ("space_inexistente", "space_not_found"),
            ("conta_ausente", "space_account_selection_required"),
            ("fork_editor_ausente", "space_account_selection_required"),
            ("conta_vazia", "space_account_selection_required"),
            ("conta_de_outro_host", "account_not_found"),
        ] {
            let mut refused = body.clone();
            if case.starts_with("pessoal_") {
                refused["spaceId"] = personal.clone();
            }
            match case {
                "hub_ausente" | "pessoal_hub_ausente" => {
                    refused.as_object_mut().unwrap().remove("hubId");
                }
                "hub_de_outro_host" | "pessoal_hub_de_outro_host" => refused["hubId"] = json!(other.hub_id),
                "space_de_outro_host" => refused["spaceId"] = foreign_work.clone(),
                "space_inexistente" => refused["spaceId"] = json!("space-sintetico-inexistente"),
                "conta_ausente" => {
                    refused["forkEditor"].as_object_mut().unwrap().remove("engineAccountId");
                }
                "fork_editor_ausente" => {
                    refused.as_object_mut().unwrap().remove("forkEditor");
                }
                "conta_vazia" => refused["forkEditor"]["engineAccountId"] = json!(""),
                "conta_de_outro_host" => refused["forkEditor"]["engineAccountId"] = foreign_account.clone(),
                _ => unreachable!(),
            }
            // HTTP bruto: nenhum helper completa o Hub ou a conta omitidos na intenção.
            let response = reqwest::Client::new()
                .post(format!("{}/api/createBot", f.base))
                .bearer_auth(&f.token)
                .json(&refused)
                .timeout(Duration::from_secs(20))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), 400, "{case}");
            assert!(response.text().await.unwrap().contains(expected), "{case}: esperava {expected}");
            assert_eq!(persisted(f), before, "{case}: houve efeito no Host destinatário");
            assert_eq!(persisted(other), other_before, "{case}: houve efeito no outro Host");
        }
        let created = f.call("createBot", body).await["bot"].clone();
        assert_eq!(created["spaceId"], *work);
        assert_eq!(created["forkEditor"]["engineAccountId"], *account);
        let db =
            rusqlite::Connection::open_with_flags(f.home.join("codync.db"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        let config: String = db
            .query_row("SELECT config FROM bots WHERE id=?1", [created["id"].as_str().unwrap()], |r| r.get(0))
            .unwrap();
        let config: Value = serde_json::from_str(&config).unwrap();
        assert_eq!(config["spaceId"], *work);
        assert_eq!(config["forkEditor"]["engineAccountId"], *account);
        assert_eq!(persisted(other), other_before, "criação local alterou o outro Host");
        // Pessoal explícito exige Hub, mas continua sem obrigação de selecionar conta.
        let personal_bot = f
            .call(
                "createBot",
                json!({"hubId":f.hub_id,"spaceId":personal,
            "name":"Pessoal explícito sintético","backend":"codex","cwd":f.workspaces.root()}),
            )
            .await;
        assert_eq!(personal_bot["bot"]["spaceId"], *personal);
        // Contrato legado: sem spaceId nem hubId, continua criando no Pessoal.
        let legacy = f
            .call(
                "createBot",
                json!({"name":"Pessoal legado sintético",
            "backend":"codex","cwd":f.workspaces.root()}),
            )
            .await;
        assert_eq!(legacy["bot"]["spaceId"], *personal);
        assert!(f.logs().is_empty(), "criação não deve enviar turnos ao motor de fixture");
    }
}

#[tokio::test]
async fn donna_retains_context_after_switching_spaces_conversations_and_restart() {
    let mut f = Fixture::new().await;
    let account = f.connected("a").await;
    // D366/D368: o Berga fixo do Hub novo; o dono ajusta motor, pasta e as próprias preferências.
    let donna = f.call("teamSettings", json!({})).await["primaryBotId"].clone();
    f.call("updateBot",json!({"id":donna,"backend":"codex","cwd":f.workspaces.root(),"permission":"ask","permissionChosenByUser":true,"model":"gpt-6.1-sol","ownerPreferences":"PERSONAL_PROMPT_SENTINEL","forkEditor":{"engineAccountId":account}})).await;
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
    f.call("saveMemory", json!({"botId":donna,"shared":true,"text":"PERSONAL_MEMORY_SENTINEL"})).await;
    std::fs::write(f.workspaces.root().join("personal-secret-fixture.txt"), "PERSONAL_FILE_SENTINEL").unwrap();
    let c = f.conversation(&work, &chats[0]).await;
    f.call("donnaConversationMemory",json!({"spaceId":work,"conversationId":chats[0],"expectedRevision":c["revision"],"method":"saveMemory","text":"WORK_MEMORY_SENTINEL"})).await;
    let upload = uuid::Uuid::new_v4().to_string();
    f.call("donnaConversationUpload",json!({"spaceId":work,"conversationId":chats[0],"expectedRevision":c["revision"],"uploadId":upload,"name":"fixture.txt","offset":0,"done":true,"data":"RklMRV9BX09OTFk="})).await;
    f.refused(
        "donnaConversationReadUpload",
        json!({"spaceId":work,"conversationId":chats[1],"uploadId":upload,"offset":0}),
    )
    .await;
    let original_intent = json!({"spaceId":work,"conversationId":chats[0],"expectedRevision":c["revision"],"text":"CONTEXT_MARKER_A PAST_A_ONLY","clientNonce":"a1","attachments":[upload]});
    let first = f.send_files(&work, &chats[0], "CONTEXT_MARKER_A PAST_A_ONLY", "a1", vec![upload.clone()]).await;
    let actual = f.requests(&work, &chats[0]);
    let consumed = actual.iter().rfind(|v| v["method"] == "fixture/consumed").unwrap();
    assert_eq!(consumed["attachedFiles"][0]["text"], "FILE_A_ONLY");
    assert!(consumed["text"].as_str().unwrap().contains("WORK_MEMORY_SENTINEL"));
    f.send(&work, &chats[1], "CONTEXT_MARKER_B PAST_B_ONLY", "b1").await;
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
        assert!(!log.contains("PERSONAL_PROMPT_SENTINEL"));
        assert!(!log.contains("PERSONAL_MEMORY_SENTINEL"));
        assert!(!log.contains("PERSONAL_FILE_SENTINEL"));
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
                last.to_string().contains("PAST_A_ONLY"),
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
    assert!(!delivered.to_string().contains("PAST_A_ONLY"));
    assert!(!delivered.to_string().contains("PAST_B_ONLY"));
    let b = f.conversation(&work, &chats[1]).await;
    f.refused("donnaConversationSend",json!({"spaceId":work,"conversationId":chats[1],"expectedRevision":b["revision"],"text":"CONTEXT_MARKER_B STALE_ACCOUNT_REFUSED","clientNonce":"stale-b"})).await;
    f.bind(&work, &chats[1], &account, "gpt-6.1-sol").await;
    f.send(&work, &chats[1], "CONTEXT_MARKER_B AFTER_RESET_A", "b-after-reset-a").await;
    for chat in &chats {
        let history = f.call("donnaConversationHistory", json!({"spaceId":work,"conversationId":chat})).await;
        assert_eq!(
            history["entries"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["kind"] == "notice"
                    && e["data"]["text"]
                        == "Neste espaço o Berga não usa suas instruções, ferramentas nem a memória do Pessoal.")
                .count(),
            1
        );
        for request in f.requests(&work, chat).iter().filter(|r| r["method"] == "session/new") {
            assert_eq!(request["params"]["mcpServers"], json!([]));
        }
    }

    assert!(
        f.requests(&work, &chats[1])
            .iter()
            .rfind(|v| v["method"] == "fixture/consumed")
            .unwrap()
            .to_string()
            .contains("PAST_B_ONLY")
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
async fn conversa_propria_anuncia_so_servidores_protegidos() {
    let f = Fixture::new().await;
    let account = f.connected("propria").await;
    let bot = f.call("createBot", json!({"name":"Assistente","backend":"codex","cwd":f.workspaces.root(),"model":"gpt-6.1-sol","description":"ALMA_PROPRIA","forkEditor":{"engineAccountId":account}})).await["bot"]["id"].clone();
    let personal = f.call("donnaSpaces", json!({})).await["personalSpaceId"].clone();
    let chat =
        f.call("donnaConversationCreate", json!({"spaceId":personal,"botId":bot})).await["conversation"]["id"].clone();
    f.bind(&personal, &chat, &account, "gpt-6.1-sol").await;
    f.send(&personal, &chat, "CONTEXT_MARKER_A TOOL_READ", "propria-protegida").await;
    let requests = f.requests(&personal, &chat);
    let opened = requests.iter().find(|r| r["method"] == "session/new").unwrap();
    let servers = opened["params"]["mcpServers"].as_array().unwrap();
    assert_eq!(
        servers.iter().map(|s| s["name"].as_str().unwrap()).collect::<Vec<_>>(),
        ["connectors", "chat", "team", "memory", "tasks"]
    );
    for server in servers {
        assert!(server["env"].as_array().unwrap().iter().any(|e| e["name"] == "DONNABOT_SESSION_SLOT"
            && e["value"] == format!("conversation:{}", chat.as_str().unwrap())));
        assert!(
            server["env"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["name"] == "DONNABOT_TOOL_BINDING" && e["value"].is_string())
        );
    }
    assert!(requests.iter().any(|r| r["method"] == "session/prompt" && r.to_string().contains("ALMA_PROPRIA")));
    let c = f.conversation(&personal, &chat).await;
    f.call(
        "donnaConversationUpdate",
        json!({"spaceId":personal,"conversationId":chat,"expectedRevision":c["revision"],"title":"Título revisado"}),
    )
    .await;
    f.send(&personal, &chat, "CONTEXT_MARKER_A TOOL_READ", "propria-nova-revisao").await;
    let revised = f.requests(&personal, &chat);
    assert_eq!(revised.iter().filter(|r| r["method"] == "fixture/tool" && r["name"] == "search_memory").count(), 2);
    let definitions: Vec<_> =
        revised.iter().filter(|r| matches!(r["method"].as_str(), Some("session/new" | "session/load"))).collect();
    assert_eq!(definitions.len(), 2, "revisão nova requer novas definições, sem renovar a captura antiga");
    assert_ne!(definitions[0]["params"]["mcpServers"], definitions[1]["params"]["mcpServers"]);
}

#[tokio::test]
async fn ferramentas_reais_entregam_na_adicional_e_codex_preserva_memoria_somente_leitura() {
    let f = Fixture::new().await;
    let account = f.connected("fluxo").await;
    let bot = f.call("createBot", json!({"name":"Assistente","backend":"codex","cwd":f.workspaces.root(),"model":"gpt-6.1-sol","forkEditor":{"engineAccountId":account}})).await["bot"]["id"].clone();
    let seed = f.call("saveMemory", json!({"botId":bot,"text":"A pessoa gosta de astronomia.","shared":false})).await;
    let recipient_dir = f.workspaces.root().join("destinatario");
    std::fs::create_dir_all(&recipient_dir).unwrap();
    let recipient = f.call("createBot", json!({"name":"Outro bot","backend":"codex","cwd":recipient_dir,"model":"gpt-6.1-sol","forkEditor":{"engineAccountId":account}})).await["bot"]["id"].clone();
    let personal = f.call("donnaSpaces", json!({})).await["personalSpaceId"].clone();
    let chat =
        f.call("donnaConversationCreate", json!({"spaceId":personal,"botId":bot})).await["conversation"]["id"].clone();
    let c = f.bind(&personal, &chat, &account, "gpt-6.1-sol").await;
    f.call("donnaConversationSend", json!({"spaceId":personal,"conversationId":chat,"expectedRevision":c["revision"],"text":format!("CONTEXT_MARKER_A TOOL_FLOW TARGET_BOT={}",recipient.as_str().unwrap()),"clientNonce":"tools-reais"})).await;
    let deadline = Instant::now() + Duration::from_secs(40);
    let entries = loop {
        let page = f.call("donnaConversationHistory", json!({"spaceId":personal,"conversationId":chat})).await;
        let entries = page["entries"].as_array().unwrap();
        if entries.iter().any(|e| e["kind"] == "permission") {
            break entries.clone();
        }
        assert!(Instant::now() < deadline, "sem cartão: {page}; logs={:?}", f.requests(&personal, &chat));
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert!(entries.iter().any(|e| e["data"]["text"] == "MENSAGEM_ADICIONAL"));
    let file = entries.iter().find(|e| e["data"]["files"].is_array()).unwrap();
    assert_eq!(file["data"]["files"][0]["name"], "arquivo-adicional.txt");
    assert!(entries.iter().any(|e| e["data"]["delegationId"].is_string()));
    assert!(entries.iter().any(|e| {
        e["data"]["botReactions"]
            .as_array()
            .is_some_and(|reactions| reactions.iter().any(|r| r["bot"] == bot && r["emoji"] == "👍"))
    }));
    // A main omite/recusa mutantes de memória em Codex, inclusive na adicional.
    assert!(entries.iter().all(|e| e["data"]["memory"]["confirmation"] != true));
    let calls = f.requests(&personal, &chat);
    let refused = calls.iter().find(|r| r["method"] == "fixture/tool" && r["name"] == "remember").unwrap();
    assert_eq!(refused["result"]["isError"], true);
    assert!(refused.to_string().contains("codex_memory_mutation_refused"));
    let read = calls.iter().find(|r| r["method"] == "fixture/tool" && r["name"] == "search_memory").unwrap();
    let result: Value = serde_json::from_str(read["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(result["results"].as_array().unwrap().iter().any(|entry| entry["id"] == seed["entry"]["id"]));
    let permission = entries.iter().find(|e| e["kind"] == "permission").unwrap();
    f.refused(
        "respondPermission",
        json!({"botId":bot,"entryId":permission["id"],"entryRev":permission["rev"],"optionId":"allow"}),
    )
    .await;
    f.call("respondPermission", json!({"spaceId":personal,"conversationId":chat,"botId":chat,"entryId":permission["id"],"entryRev":permission["rev"],"optionId":"allow"})).await;
    let main = f.call("history", json!({"botId":bot})).await;
    assert!(
        !main.to_string().contains("MENSAGEM_ADICIONAL")
            && !main.to_string().contains("arquivo-adicional.txt")
            && !main.to_string().contains("delegationId")
            && !main.to_string().contains("astronomia"),
        "efeito vazou à principal: {main}"
    );
    let saved = f.call("memory", json!({"botId":bot})).await;
    assert!(saved.to_string().contains("astronomia"), "namespace original: {saved}");
    let target = f.call("history", json!({"botId":recipient})).await;
    assert!(target.to_string().contains("CONTEXT_MARKER_B"));
}
