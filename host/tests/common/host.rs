// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! A real host on disposable data, and what the end-to-end tests ask of it.

// Each test binary compiles `common` and uses only part of this.
#![allow(dead_code)]

use serde_json::{Value, json};
use std::{
    path::PathBuf,
    process::Child,
    time::{Duration, Instant},
};

/// Origem correspondente ao compositor do app para texto efetivamente digitado no fixture.
pub fn typed_origin(text: &str) -> Value {
    let length = text.encode_utf16().count();
    json!({"schema":1,"draftId":uuid::Uuid::new_v4().to_string(),"revision":1,
        "textLength":length,"channel":"desktopComposer","deviceId":uuid::Uuid::new_v4().to_string(),
        "interaction":"send","segments":if length == 0 { vec![] } else { vec![json!({"start":0,"end":length,"kind":"typed"})] }})
}

pub struct Host {
    child: Child,
    /// The host's `MEXERICA_HOME`.
    pub home: PathBuf,
    base: String,
    token: String,
}

impl Drop for Host {
    fn drop(&mut self) {
        super::stop(&mut self.child);
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

impl Host {
    fn session_binding(&self, bot: &str) -> Option<String> {
        let db = rusqlite::Connection::open(self.home.join("codync.db")).ok()?;
        db.query_row("SELECT id FROM donna_session_bindings WHERE bot=?1 AND revoked_at IS NULL ORDER BY (id=(SELECT v FROM kv WHERE k='session.binding.active.'||?1)) DESC,rowid DESC LIMIT 1",[bot],|r|r.get(0)).ok()
    }

    pub async fn start() -> Self {
        let home = std::env::temp_dir().join(format!("codync-memory-e2e-{}", uuid::Uuid::new_v4()));
        let (child, base) = super::serve(&home).await;
        let token = std::fs::read_to_string(home.join("token")).unwrap().trim().to_owned();
        Self { child, home, base, token }
    }

    pub async fn start_with_env(env: &[(&str, std::ffi::OsString)]) -> Self {
        let home = std::env::temp_dir().join(format!("codync-memory-e2e-{}", uuid::Uuid::new_v4()));
        let (child, base) = super::serve_with_env(&home, env).await;
        let token = std::fs::read_to_string(home.join("token")).unwrap().trim().to_owned();
        Self { child, home, base, token }
    }

    /// Hub com ambiente limpo: só `env` e o mínimo de `serve_isolado`.
    pub async fn start_isolado(env: &[(&str, std::ffi::OsString)]) -> Self {
        let home = std::env::temp_dir().join(format!("codync-memory-e2e-{}", uuid::Uuid::new_v4()));
        let (child, base) = super::serve_isolado(&home, env).await;
        let token = std::fs::read_to_string(home.join("token")).unwrap().trim().to_owned();
        Self { child, home, base, token }
    }

    #[cfg(unix)]
    pub async fn start_with_background(mode: &str) -> Self {
        use std::os::unix::fs::PermissionsExt;

        let home = std::env::temp_dir().join(format!("codync-background-e2e-{}", uuid::Uuid::new_v4()));
        let bin = home.join("bin");
        let profile = home.join("profile");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&profile).unwrap();
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let cli = bin.join("claude");
        std::fs::copy(fixtures.join("background_cli.py"), &cli).unwrap();
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700)).unwrap();
        let adapter = bin.join("npx");
        let agent = fixtures.join("memory_agent.py");
        let quoted = agent.to_string_lossy().replace('\'', "'\\''");
        std::fs::write(
            &adapter,
            format!(
                "#!/bin/sh\n[ \"$#\" = 2 ] && [ \"$1\" = '-y' ] && [ \"$2\" = '@agentclientprotocol/claude-agent-acp@0.94.0' ] || exit 64\nexec python3 -u '{quoted}'\n"
            ),
        )
        .unwrap();
        std::fs::set_permissions(&adapter, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = std::env::join_paths(
            std::iter::once(bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())),
        )
        .unwrap();
        let background_mode = match mode {
            "marker" => home.join("dream-boundary").into_os_string(),
            "name-marker" => home.join("naming-boundary").into_os_string(),
            "managed" => "".into(),
            _ => mode.into(),
        };
        let env = [
            ("PATH", path),
            ("DONNABOT_FIXTURE_BACKGROUND_MODE", background_mode),
            ("CLAUDE_CONFIG_DIR", profile.into_os_string()),
        ];
        let (child, base) = super::serve_with_env(&home, &env).await;
        let token = std::fs::read_to_string(home.join("token")).unwrap().trim().to_owned();
        if mode == "managed" {
            let id = format!("ac_{}", "b".repeat(64));
            let launch = json!({"kind":"cli_profile", "executable":"/fixture/claude", "directory":"/fixture/profile"});
            let proof = json!({"method":"claude_auth_status", "plan":"max"});
            let registry = json!({"salt":uuid::Uuid::new_v4().to_string(), "accounts":[{
                "id":id,"backend":"claude","billing":"subscription","state":"connected",
                "launch":launch,"scope":"owner","quotaIdentity":id,"verifiedAt":1,
                "subscriptionProof":proof,"models":[{"id":"haiku","options":[]}],
                "managed":{"label":"Outra conta","revision":1,"generation":0,"enabled":true,
                    "requestId":"fixture","principal":"fixture","profile":null,"setupRequest":null,
                    "catalog":{"accountId":id,"principal":"fixture","launch":launch,"revision":1,"subscription":proof}}
            }]});
            let db = rusqlite::Connection::open(home.join("codync.db")).unwrap();
            db.execute("INSERT INTO kv(k,v) VALUES('fork.engine-accounts.v1',?1)", [registry.to_string()]).unwrap();
        }
        Self { child, home, base, token }
    }

    /// Stops the host, lets `between` change its files, and starts it again on the same data.
    pub async fn restart(&mut self, between: impl FnOnce(&std::path::Path)) {
        super::stop(&mut self.child);
        between(&self.home);
        let (child, base) = super::serve(&self.home).await;
        self.child = child;
        self.base = base;
    }

    /// Como [`Host::restart`], mas no ambiente limpo de [`Host::start_isolado`] (CLIs falsos, HOME e PATH do teste).
    pub async fn restart_isolado(&mut self, env: &[(&str, std::ffi::OsString)]) {
        super::stop(&mut self.child);
        let (child, base) = super::serve_isolado(&self.home, env).await;
        self.child = child;
        self.base = base;
    }

    /// Stream real do terminal, com a mesma autenticação local do app desktop.
    pub async fn terminal_stream(&self, id: &str) -> reqwest::Response {
        reqwest::Client::new()
            .get(format!("{}/term/{}", self.base, id))
            .bearer_auth(&self.token)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
    }

    pub async fn post(&self, method: &str, body: &Value) -> (bool, Value) {
        let mut body = body.clone();
        if matches!(method, "memoryCall" | "chatCall" | "manageCall" | "teamCall" | "routineCall" | "computersCall")
            && body.get("sessionBinding").is_none()
            && let Some(binding) = body["botId"].as_str().and_then(|bot| self.session_binding(bot))
        {
            body["sessionBinding"] = json!(binding);
        }
        let response = reqwest::Client::new()
            .post(format!("{}/api/{method}", self.base))
            .bearer_auth(&self.token)
            .json(&body)
            .timeout(Duration::from_secs(120))
            .send()
            .await
            .unwrap();
        let ok = response.status().is_success();
        (ok, response.json().await.unwrap())
    }

    pub async fn call(&self, method: &str, body: Value) -> Value {
        let (ok, body) = self.post(method, &body).await;
        assert!(ok, "{method}: {body}");
        body
    }

    /// A call that must be refused: its error text.
    pub async fn refused(&self, method: &str, body: Value) -> String {
        let (ok, body) = self.post(method, &body).await;
        assert!(!ok, "{method} should have been refused: {body}");
        body["error"].as_str().unwrap_or_default().to_owned()
    }

    pub async fn bot(&self, name: &str, permission: &str) -> String {
        let bot =
            json!({"name": name, "backend": "custom", "command": "unused", "connectors": [], "permission": permission});
        self.call("createBot", bot).await["bot"]["id"].as_str().unwrap().to_owned()
    }

    /// What a client holds after catching up (see `common::snapshot`).
    pub async fn snapshot(&self) -> Value {
        super::snapshot(&self.base, &self.token).await
    }

    pub async fn memory(&self, bot: &str) -> Value {
        self.call("memory", json!({"botId": bot})).await
    }

    /// A memory tool call as the bot's main session makes it: its internal server carries `sessionSlot: "main"`.
    fn memory_transport_arguments(&self, bot: &str, mut arguments: Value) -> Value {
        let db = rusqlite::Connection::open(self.home.join("codync.db")).unwrap();
        let capability:Option<String>=db.query_row("SELECT capability FROM donna_memory_transports WHERE bot=?1 AND active=1 AND slot='main' ORDER BY rowid DESC LIMIT 1",
            [bot],|r|r.get(0)).ok();
        if let Some(capability) = capability {
            arguments["__memoryTransport"] = capability.into();
        }
        arguments
    }

    pub async fn tool(&self, bot: &str, name: &str, arguments: Value) -> Value {
        let arguments = self.memory_transport_arguments(bot, arguments);
        self.call("memoryCall", json!({"botId": bot, "name": name, "arguments": arguments, "sessionSlot": "main"}))
            .await
    }

    pub async fn tool_refused(&self, bot: &str, name: &str, arguments: Value) -> String {
        let arguments = self.memory_transport_arguments(bot, arguments);
        self.refused("memoryCall", json!({"botId": bot, "name": name, "arguments": arguments, "sessionSlot": "main"}))
            .await
    }

    /// A bot running `tests/fixtures/<fixture>` with `args`.
    /// D366: o Berga que o Hub novo cria (D190), com o agente de teste no motor. A principal é fixa;
    /// o app do dono só ajusta motor e ferramentas.
    pub async fn fixture_donna(&self, fixture: &str, args: &str) -> String {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(fixture);
        let command = format!("{} {args}", super::python_agent(&script));
        let id = self.call("teamSettings", json!({})).await["primaryBotId"].as_str().unwrap().to_owned();
        self.call(
            "updateBot",
            json!({"id": id, "backend": "custom", "command": command, "connectors": [], "notify": false}),
        )
        .await;
        id
    }

    pub async fn fixture_bot(&self, name: &str, fixture: &str, args: &str) -> String {
        self.fixture_bot_as(name, fixture, args, "custom").await
    }

    /// The same, created already with `backend` (D372: a conversa nasce com o motor do bot na criação; editar o
    /// padrão depois só vale para conversas novas).
    pub async fn fixture_bot_as(&self, name: &str, fixture: &str, args: &str, backend: &str) -> String {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(fixture);
        let command = format!("{} {args}", super::python_agent(&script));
        let bot = json!({"name": name, "backend": backend, "command": command, "connectors": [], "notify": false});
        self.call("createBot", bot).await["bot"]["id"].as_str().unwrap().to_owned()
    }

    /// Starts a real user turn with `instructions_agent.py hold`, waiting until the fixture receives it.
    /// Memory writes happen inside this turn, independently of the bot's permission mode (D60).
    pub async fn hold_user_turn(&self, bot: &str, text: &str) -> String {
        self.hold_user_turn_with_origin(bot, text, typed_origin(text)).await
    }

    pub async fn hold_user_turn_with_origin(&self, bot: &str, text: &str, origin: Value) -> String {
        let sent = self
            .call("send", json!({"botId": bot, "text": text, "clientNonce": uuid::Uuid::new_v4().to_string(), "contentOrigin": origin}))
            .await;
        let path = self.home.join("bots").join(bot).join("workspace/requests.jsonl");
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let log = std::fs::read_to_string(&path).unwrap_or_default();
            let received = log.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).any(|r| {
                r["method"] == "session/prompt"
                    && r["params"]["prompt"][0]["text"].as_str().is_some_and(|t| t.contains(text))
            });
            if received {
                return sent["entry"]["id"].as_str().unwrap().to_owned();
            }
            assert!(Instant::now() < deadline, "{bot} didn't receive the user turn: {log}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Sends `text` and waits for the bot's reply.
    pub async fn reply(&self, bot: &str, text: &str) {
        let finals = |history: &Value| {
            history["entries"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["kind"] == "agent" && e["data"]["final"] == true)
                .count()
        };
        let before = finals(&self.call("history", json!({"botId": bot})).await);
        let nonce = uuid::Uuid::new_v4().to_string();
        self.call(
            "send",
            json!({"botId": bot, "text": text, "clientNonce": nonce, "contentOrigin": typed_origin(text)}),
        )
        .await;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let history = self.call("history", json!({"botId": bot})).await;
            if finals(&history) > before {
                return;
            }
            assert!(Instant::now() < deadline, "{bot} didn't answer {text:?}: {history}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// The JSON lines a fixture agent logged to `file` in a bot's workspace.
    pub fn log(&self, bot: &str, file: &str) -> Vec<Value> {
        let log = std::fs::read_to_string(self.home.join("bots").join(bot).join("workspace").join(file)).unwrap();
        log.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()).collect()
    }

    /// The requests `instructions_agent.py` logged in a bot's workspace.
    pub fn requests(&self, bot: &str, method: &str) -> Vec<Value> {
        self.log(bot, "requests.jsonl").into_iter().filter(|r| r["method"] == method).collect()
    }

    /// The text of every prompt a bot's agent got, oldest first.
    pub fn prompts(&self, bot: &str) -> Vec<String> {
        self.requests(bot, "session/prompt")
            .iter()
            .map(|r| r["params"]["prompt"][0]["text"].as_str().unwrap().to_owned())
            .collect()
    }
}
