// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! End to end: a real `codync-host serve` driving a scripted ACP agent
//! (`fake_agent.py`) through a full turn with an approval in the middle.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test code: failing loudly is the point

mod common;

/// The host's wire crypto doubles as this test's device implementation.
#[path = "../src/remote/crypto.rs"]
#[allow(dead_code)]
mod crypto;

use common::workspaces::Workspaces;
use ed25519_dalek::SigningKey;
use futures::{SinkExt as _, StreamExt as _};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

struct Host {
    child: Child,
    base: String,
    token: String,
    home: PathBuf,
    workspaces: Workspaces,
}

impl Drop for Host {
    fn drop(&mut self) {
        common::stop(&mut self.child);
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

async fn start_host() -> Host {
    let home = std::env::temp_dir().join(format!("codync-e2e-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&home).unwrap();
    let workspaces = Workspaces::new(&home);
    let (child, base) = common::serve(&home).await;
    let token = std::fs::read_to_string(home.join("token")).unwrap().trim().to_owned();
    Host { child, base, token, home, workspaces }
}

// Só o wire usa CLIs fictícios. Nunca consultar contas pessoais da máquina de teste.
async fn start_host_with_account_clis() -> Host {
    let home = std::env::temp_dir().join(format!("donnabot-accounts-wire-{}", uuid::Uuid::new_v4()));
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let workspaces = Workspaces::new(&home);
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/engine_accounts_cli.py");
    for backend in ["claude", "codex"] {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let target = bin.join(backend);
            std::fs::copy(&fixture, &target).unwrap();
            std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        #[cfg(windows)]
        std::fs::write(
            bin.join(format!("{backend}.cmd")),
            format!("@echo off\r\n{} -u \"{}\" {backend} %*\r\n", common::python(), fixture.display()),
        )
        .unwrap();
    }
    let paths = std::iter::once(bin)
        .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()))
        .collect::<Vec<_>>();
    let env =
        [("PATH", std::env::join_paths(paths).unwrap()), ("CLAUDE_CONFIG_DIR", home.join("claude").into_os_string())];
    let (child, base) = common::serve_with_env(&home, &env).await;
    let token = std::fs::read_to_string(home.join("token")).unwrap().trim().to_owned();
    Host { child, base, token, home, workspaces }
}

impl Host {
    fn session_binding(&self, bot: &str) -> Option<String> {
        let db = rusqlite::Connection::open(self.home.join("codync.db")).ok()?;
        db.query_row(
            "SELECT id FROM donna_session_bindings WHERE bot=?1 AND revoked_at IS NULL ORDER BY rowid DESC LIMIT 1",
            [bot],
            |r| r.get(0),
        )
        .ok()
    }

    async fn call(&self, method: &str, mut body: Value) -> Value {
        if method == "chatCall"
            && let Some(binding) = body["botId"].as_str().and_then(|bot| self.session_binding(bot))
        {
            body["sessionBinding"] = json!(binding);
        }
        let res = reqwest::Client::new()
            .post(format!("{}/api/{method}", self.base))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = res.status();
        let v: Value = res.json().await.unwrap();
        assert!(status.is_success(), "{method} failed: {v}");
        v
    }

    /// Uma chamada que pode ser recusada: devolve o corpo, sucesso ou erro.
    async fn try_call(&self, method: &str, body: Value) -> Value {
        let res = reqwest::Client::new()
            .post(format!("{}/api/{method}", self.base))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .unwrap();
        let ok = res.status().is_success();
        let v: Value = res.json().await.unwrap();
        assert!(!ok || v.get("error").is_none(), "{method}: {v}");
        v
    }

    /// Polls the bot's history until `pred` matches an entry.
    async fn wait_for(&self, bot: &str, pred: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let h = self.call("history", json!({"botId": bot})).await;
            if let Some(e) = h["entries"].as_array().unwrap().iter().rev().find(|e| pred(e)) {
                return e.clone();
            }
            assert!(Instant::now() < deadline, "timed out; history: {h}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

/// `/health` provado: um nonce novo e o HMAC-SHA256 do token local sobre ele, sem o token na URL.
/// Devolve o endereço e o nonce, que o recibo da resposta precisa conferir.
fn proved(base: &str, token: &str, nonce: [u8; 32]) -> (String, String) {
    use base64::Engine as _;
    use hmac::{Hmac, KeyInit as _, Mac as _};
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(token.as_bytes()).unwrap();
    mac.update(b"mexerica/health-proof/v1");
    mac.update(&nonce);
    let mut proof = String::with_capacity(64);
    for byte in mac.finalize().into_bytes() {
        use std::fmt::Write as _;
        let _ = write!(proof, "{byte:02x}");
    }
    let nonce = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(nonce);
    (format!("{base}/health?nonce={nonce}&proof={proof}"), nonce)
}

/// O recibo que o Hub devolve: HMAC do token sobre o nonce e a identidade que saiu.
fn recibo(token: &str, nonce: &str, health: &Value) -> String {
    use base64::Engine as _;
    use hmac::{Hmac, KeyInit as _, Mac as _};
    let texto = |field: &str| health[field].as_str().unwrap_or_default().to_owned();
    let identidade =
        format!("{}\n{}\n{}\n{}", texto("hostId"), texto("computerId"), texto("binaryPath"), texto("binaryHash"));
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(token.as_bytes()).unwrap();
    mac.update(b"mexerica/health-receipt/v1");
    mac.update(&base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(nonce).unwrap());
    mac.update(identidade.as_bytes());
    let mut out = String::with_capacity(64);
    for byte in mac.finalize().into_bytes() {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[tokio::test]
async fn health_names_this_binary() {
    let host = start_host().await;
    let anonymous: Value = reqwest::get(format!("{}/health", host.base)).await.unwrap().json().await.unwrap();
    assert_eq!(anonymous["ok"], true);
    assert_eq!(anonymous["busy"], false);
    for field in ["hostId", "computerId", "binaryPath", "binaryHash"] {
        assert!(anonymous.get(field).is_none(), "{field}: {anonymous}");
    }
    let client = reqwest::Client::new();
    let (errado, _) = proved(&host.base, "token-ficticio", [3u8; 32]);
    assert_eq!(client.get(errado).send().await.unwrap().status(), 401);
    let (certo, nonce) = proved(&host.base, &host.token, [4u8; 32]);
    let health: Value = client.get(certo).send().await.unwrap().json().await.unwrap();
    assert_eq!(health["receipt"], recibo(&host.token, &nonce, &health), "o recibo prova a resposta");
    let (outro, _) = proved(&host.base, &host.token, [4u8; 32]);
    assert_eq!(client.get(outro).send().await.unwrap().status(), 401, "o mesmo nonce não abre duas vezes");
    assert!(health["binaryHash"].as_str().is_some_and(|hash| hash.len() == 64));
    // The apps compare it with the path they resolve, which on Windows has no `\\?\` prefix.
    let exe = std::fs::canonicalize(env!("CARGO_BIN_EXE_donnabot-host")).unwrap();
    let exe = exe.to_str().unwrap();
    assert_eq!(health["binaryPath"], exe.strip_prefix(r"\\?\").unwrap_or(exe));
}

#[tokio::test]
async fn default_donna_is_synced_on_a_new_hub_and_is_not_recreated() {
    let mut host = common::host::Host::start().await;
    let snapshot = host.snapshot().await;
    let bots = snapshot["bots"].as_array().unwrap();
    assert_eq!(bots.len(), 1, "a Donna chega no stream sem createBot pelo app");
    let donna = &bots[0];
    let id = donna["id"].as_str().unwrap().to_owned();
    assert_eq!(donna["name"], "Berga");
    assert_eq!(donna["status"], "idle");
    assert_eq!(donna["permission"], "ask");
    assert_eq!(donna["manageTeam"], true);
    assert_eq!(donna["hidden"], false);
    assert_eq!(donna["managedWorkspace"], true);
    assert_eq!(donna["workspaceIssue"], Value::Null);
    assert_eq!(host.call("teamSettings", json!({})).await["primaryBotId"], id);
    let history = host.call("history", json!({"botId": id})).await;
    let entries = history["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["kind"], "notice");
    assert!(entries[0]["data"]["text"].as_str().unwrap().contains("Gerenciar contas de IA"));
    // D366: o Berga é fixo. Renomear, apagar e trocar a principal são recusados pelo HTTP real.
    let fixed = json!({"error": "donna_fixed: O Berga é fixo; só a aparência muda."});
    for (method, body) in [
        ("updateBot", json!({"id": id, "name": "Minha assistente"})),
        ("deleteBot", json!({"botId": id})),
        ("setTeamSettings", json!({"primaryBotId": null})),
    ] {
        let (ok, answer) = host.post(method, &body).await;
        assert!(!ok, "{method}");
        assert_eq!(answer, fixed, "{method}");
    }
    // Aparência muda; reiniciar não recria nem duplica a Donna.
    host.call("updateBot", json!({"id": id, "pinned": true})).await;
    host.restart(|_| {}).await;
    let snapshot = host.snapshot().await;
    assert_eq!(snapshot["bots"].as_array().unwrap().len(), 1);
    assert_eq!(common::snapshot_bot(&snapshot, &id)["name"], "Berga");
    assert_eq!(common::snapshot_bot(&snapshot, &id)["pinned"], true);
    assert_eq!(host.call("teamSettings", json!({})).await["primaryBotId"], id);
}

// SIGTERM is Unix only; Windows stops the host by ending its process.
#[cfg(unix)]
#[tokio::test]
async fn termination_exits_with_an_open_event_stream() {
    let mut host = start_host().await;
    let stream =
        reqwest::Client::new().get(format!("{}/events", host.base)).bearer_auth(&host.token).send().await.unwrap();
    assert!(stream.status().is_success());
    assert!(Command::new("/bin/kill").args(["-TERM", &host.child.id().to_string()]).status().unwrap().success());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = host.child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "open SSE prevented host shutdown");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    drop(stream);
}

#[tokio::test]
async fn turn_with_approval_reaches_a_final_reply() {
    if Command::new(common::python()).arg("--version").output().is_err() {
        eprintln!("skipping: Python not available");
        return;
    }
    let host = start_host().await;
    let agent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fake_agent.py");
    let bot = host
        .call(
            "createBot",
            json!({
                "name": "Tester", "backend": "custom", "command": common::python_agent(&agent),
                "cwd": host.workspaces.root(), "permission": "ask",
            }),
        )
        .await["bot"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    host.call("send", json!({"botId": bot, "text": "go", "clientNonce": "n1"})).await;
    // Retrying with the same nonce must not queue a second turn.
    host.call("send", json!({"botId": bot, "text": "go", "clientNonce": "n1"})).await;

    let card = host.wait_for(&bot, |e| e["kind"] == "permission" && e["data"]["status"] == "pending").await;
    assert_eq!(card["data"]["title"], "Autorizar editar arquivo: sed -i s/x/y/ a.txt");
    let bots = common::snapshot(&host.base, &host.token).await;
    assert_eq!(common::snapshot_bot(&bots, &bot)["status"], "needsInput");

    host.call(
        "respondPermission",
        json!({"botId": card["botId"], "entryRev": card["rev"], "entryId": card["id"], "optionId": "allow"}),
    )
    .await;
    let reply = host.wait_for(&bot, |e| e["kind"] == "agent" && e["data"]["final"] == true).await;
    assert!(reply["data"]["text"].as_str().unwrap().contains("\"allow\""), "{reply}");

    // The narration before the tool call stays out of the chat.
    let history = host.call("history", json!({"botId": bot})).await;
    let entries = history["entries"].as_array().unwrap();
    let finals = entries.iter().filter(|e| e["kind"] == "agent" && e["data"]["final"] == true).count();
    assert_eq!(finals, 1);
    assert_eq!(entries.iter().filter(|e| e["kind"] == "user").count(), 1);
    let tool = entries.iter().find(|e| e["kind"] == "tool").unwrap();
    assert_eq!(tool["data"]["status"], "completed");
    assert_eq!(tool["data"]["diffs"][0]["added"], 1);

    let synced = common::snapshot(&host.base, &host.token).await;
    assert_eq!(common::snapshot_bot(&synced, &bot)["status"], "idle");
    assert_eq!(common::snapshot_bot(&synced, &bot)["unread"], 1);
}

#[tokio::test]
async fn sent_messages_are_the_reply() {
    if Command::new(common::python()).arg("--version").output().is_err() {
        eprintln!("skipping: Python not available");
        return;
    }
    let host = start_host().await;
    let agent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fake_agent.py");
    let bot = host
        .call(
            "createBot",
            json!({
                "name": "Tester", "backend": "custom", "command": common::python_agent(&agent),
                "cwd": host.workspaces.root(), "permission": "ask",
            }),
        )
        .await["bot"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let send = |text: &str| json!({"botId": bot, "name": "send_message", "arguments": {"text": text}});

    // Between turns there's no one to talk to.
    let idle = reqwest::Client::new()
        .post(format!("{}/api/chatCall", host.base))
        .bearer_auth(&host.token)
        .json(&send("hello?"))
        .send()
        .await
        .unwrap();
    assert!(!idle.status().is_success());

    host.call("send", json!({"botId": bot, "text": "go", "clientNonce": "n1"})).await;
    let card = host.wait_for(&bot, |e| e["kind"] == "permission" && e["data"]["status"] == "pending").await;
    // Mid-turn (the agent waits on the approval), each message is its own chat bubble at once.
    host.call("chatCall", send("Found it.")).await;
    host.call("chatCall", send("Fixing the typo now.")).await;
    let first = host.wait_for(&bot, |e| e["kind"] == "agent" && e["data"]["text"] == "Found it.").await;
    assert_eq!(first["data"]["final"], true);

    host.call(
        "respondPermission",
        json!({"botId": card["botId"], "entryRev": card["rev"], "entryId": card["id"], "optionId": "allow"}),
    )
    .await;
    let deadline = Instant::now() + Duration::from_secs(20);
    while common::snapshot_bot(&common::snapshot(&host.base, &host.token).await, &bot)["status"] != "idle" {
        assert!(Instant::now() < deadline, "turn didn't finish");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // What the agent wrote as its reply stays in the trace.
    let history = host.call("history", json!({"botId": bot})).await;
    let entries = history["entries"].as_array().unwrap();
    assert!(entries.iter().any(|e| e["data"]["text"].as_str().is_some_and(|t| t.starts_with("Done"))));
    let finals: Vec<&str> = entries
        .iter()
        .filter(|e| e["kind"] == "agent" && e["data"]["final"] == true)
        .map(|e| e["data"]["text"].as_str().unwrap())
        .collect();
    assert_eq!(finals, ["Found it.", "Fixing the typo now."]);
}

#[tokio::test]
async fn rejects_requests_without_the_token() {
    let host = start_host().await;
    let res = reqwest::Client::new().post(format!("{}/api/hello", host.base)).json(&json!({})).send().await.unwrap();
    assert_eq!(res.status(), 401);
    let res = reqwest::Client::new()
        .post(format!("{}/api/hello", host.base))
        .bearer_auth("wrong")
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// A phone talking to the host over the direct E2E channel.
struct Phone {
    ws: Ws,
    sealer: crypto::Sealer,
    opener: crypto::Opener,
}

enum Frame {
    Inner(Value),
    Closed(u16),
}

impl Phone {
    /// Connects and completes the handshake, verifying the host against the pinned `sk`.
    async fn connect(host: &Host, key: &SigningKey, sk: &[u8; 32], pair: bool) -> Self {
        let url = format!("{}/channel?v=1", host.base.replace("http://", "ws://"));
        let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
        let cid = crypto::computer_id_raw(sk);
        let dk = key.verifying_key().to_bytes();
        let ek = x25519_dalek::StaticSecret::from(crypto::random::<32>());
        let ek_pub = crypto::x25519_pub(&ek);
        let n = crypto::random::<32>();
        let sig = crypto::sign(key, &crypto::hs1_input(&cid, &dk, &ek_pub, &n));
        let hello = json!({"t": "hello", "v": 1, "dk": crypto::b64(&dk), "ek": crypto::b64(&ek_pub),
            "n": crypto::b64(&n), "sig": crypto::b64(&sig), "pair": pair});
        ws.send(hello.to_string().into()).await.unwrap();
        let welcome: Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(welcome["t"], "welcome", "{welcome}");
        let ek_h: [u8; 32] = crypto::unb64_n(welcome["ek"].as_str().unwrap()).unwrap();
        let th = crypto::transcript_hash(&cid, &dk, &ek_pub, &n, &ek_h);
        crypto::verify(sk, &th, &crypto::unb64_n(welcome["sig"].as_str().unwrap()).unwrap()).unwrap();
        let keys = crypto::channel_keys(&crypto::x25519(&ek, &ek_h).unwrap(), &th);
        Self { ws, sealer: crypto::Sealer::new(keys.d2h), opener: crypto::Opener::new(keys.h2d, 16 << 20) }
    }

    async fn send(&mut self, v: Value) {
        for f in self.sealer.seal(v.to_string().as_bytes()).unwrap() {
            self.ws.send(f.to_string().into()).await.unwrap();
        }
    }

    async fn recv(&mut self) -> Frame {
        use tokio_tungstenite::tungstenite::Message;
        loop {
            let msg = tokio::time::timeout(Duration::from_secs(20), self.ws.next()).await.expect("host answers");
            match msg.unwrap().unwrap() {
                Message::Text(t) => {
                    let f: Value = serde_json::from_str(t.as_str()).unwrap();
                    if let Some(bytes) = self.opener.open(&f).unwrap() {
                        return Frame::Inner(serde_json::from_slice(&bytes).unwrap());
                    }
                }
                Message::Close(c) => return Frame::Closed(c.map_or(1005, |c| c.code.into())),
                _ => {}
            }
        }
    }

    async fn inner(&mut self) -> Value {
        match self.recv().await {
            Frame::Inner(v) => v,
            Frame::Closed(code) => panic!("closed with {code}"),
        }
    }

    async fn call(&mut self, id: u64, m: &str, b: Value) -> Value {
        self.send(json!({"id": id, "m": m, "b": b})).await;
        loop {
            let v = self.inner().await;
            if v["id"] == id && v.get("ev").is_none() {
                assert!(v["ok"].is_object(), "{m}: {v}");
                return v["ok"].clone();
            }
        }
    }
}

fn query_param(url: &str, name: &str) -> String {
    url.split(['?', '&']).find_map(|p| p.strip_prefix(&format!("{name}="))).unwrap().to_owned()
}

#[tokio::test]
async fn phone_pairs_and_chats_over_the_direct_channel() {
    if Command::new(common::python()).arg("--version").output().is_err() {
        eprintln!("skipping: Python not available");
        return;
    }
    let host = start_host().await;
    let old = tokio_tungstenite::connect_async(format!("{}/channel?v=2", host.base.replace("http://", "ws://"))).await;
    let Err(tokio_tungstenite::tungstenite::Error::Http(res)) = old else {
        panic!("an unknown version must be refused")
    };
    assert_eq!(res.status(), 426);

    let qr = host.call("pairing", json!({})).await;
    let url = qr["pairingUrl"].as_str().unwrap();
    let sk: [u8; 32] = crypto::unb64_n(&query_param(url, "sk")).unwrap();
    assert_eq!(query_param(url, "id"), crypto::computer_id(&sk));
    let key = SigningKey::from_bytes(&crypto::random());

    let mut phone = Phone::connect(&host, &key, &sk, true).await;
    let code = query_param(url, "code");
    phone.send(json!({"id": 1, "m": "pair", "b": {"code": code, "name": "Test iPhone", "platform": "ios"}})).await;
    assert_eq!(phone.inner().await["ok"]["computerId"], crypto::computer_id(&sk));
    assert!(matches!(phone.recv().await, Frame::Closed(4100)));

    let mut phone = Phone::connect(&host, &key, &sk, false).await;
    let hello = phone.call(1, "hello", json!({})).await;
    assert_eq!(hello["boxKey"], query_param(url, "bk"));
    // Phones check themselves against it (docs/reference/compatibility.md).
    assert!(hello["minApp"].as_str().is_some_and(|v| !v.is_empty()));
    let agent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fake_agent.py");
    let bot = phone
        .call(
            2,
            "createBot",
            json!({
                "name": "Tester", "backend": "custom", "command": common::python_agent(&agent),
                "cwd": host.workspaces.root(), "permission": "auto", "permissionChosenByUser": true,
            }),
        )
        .await["bot"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    phone.send(json!({"id": 3, "sub": "events", "b": {"since": 0, "client": "ios"}})).await;
    assert_eq!(phone.inner().await["ev"]["type"], "hello");
    let sent = phone
        .call(
            4,
            "send",
            json!({"botId": bot, "text": "go", "clientNonce": "n1",
        "ingress":{"caller":"local"},
        "contentOrigin":{"schema":1,"draftId":uuid::Uuid::new_v4().to_string(),"revision":1,"textLength":2,
            "deviceId":uuid::Uuid::new_v4().to_string(),"channel":"desktopComposer","interaction":"send",
            "segments":[{"start":0,"end":2,"kind":"typed"}]}}),
        )
        .await;
    assert_eq!(sent["entry"]["data"]["contentOrigin"]["status"], "appReported");
    assert_eq!(
        sent["entry"]["data"]["contentOrigin"]["ingress"],
        json!({"caller":"device", "device":crypto::b64(key.verifying_key().as_bytes())})
    );
    let (mut respondeu, mut terminou) = (false, false);
    while !respondeu || !terminou {
        let v = phone.inner().await;
        let e = &v["ev"]["entry"];
        if v["id"] == 3 && e["kind"] == "permission" && e["data"]["status"] == "pending" {
            phone
                .send(json!({"id":5, "m":"respondPermission", "b":{
                    "botId":e["botId"], "entryRev":e["rev"], "entryId":e["id"], "optionId":"allow"
                }}))
                .await;
        }
        if v["id"] == 5 && v.get("ev").is_none() {
            assert!(v["ok"].is_object(), "{v}");
            respondeu = true;
        }
        if v["id"] == 3 && e["kind"] == "agent" && e["data"]["final"] == true {
            assert!(e["data"]["text"].as_str().unwrap().starts_with("Done"), "{e}");
            terminou = true;
        }
    }

    let devices = host.call("devices", json!({})).await;
    assert_eq!(devices["devices"][0]["name"], "Test iPhone");
    assert_eq!(devices["devices"][0]["connected"], true);
}

#[tokio::test]
async fn personal_workspaces_are_unique_persistent_and_optional() {
    let host = start_host().await;
    let agent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/workspace_agent.py");
    let first = host
        .call("createBot", json!({"name": "One", "backend": "custom", "command": common::python_agent(&agent)}))
        .await;
    let second = host.call("createBot", json!({"name": "Two", "backend": "claude", "cwd": ""})).await;
    let first = &first["bot"];
    let second = &second["bot"];
    let canonical = |path: &Value| std::fs::canonicalize(path.as_str().unwrap()).unwrap();
    let dir = canonical(&first["cwd"]);
    assert_eq!(
        dir,
        std::fs::canonicalize(host.home.join("bots").join(first["id"].as_str().unwrap()).join("workspace")).unwrap()
    );
    assert!(dir.is_dir());
    assert_ne!(dir, canonical(&second["cwd"]));
    assert_eq!(first["managedWorkspace"], true);
    host.call("send", json!({"botId": first["id"], "text": "verify"})).await;
    host.wait_for(first["id"].as_str().unwrap(), |e| e["kind"] == "agent" && e["data"]["final"] == true).await;
    let execution: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("execution.json")).unwrap()).unwrap();
    assert_eq!(canonical(&execution["process"]), dir);
    assert_eq!(canonical(&execution["session"]), dir);
    std::fs::write(dir.join("keep.txt"), "persistent output").unwrap();
    let renamed = host.call("updateBot", json!({"id": first["id"], "name": "Renamed", "cwd": ""})).await;
    assert_eq!(canonical(&renamed["bot"]["cwd"]), dir);
    let project = host.workspaces.root().join("project");
    std::fs::create_dir(&project).unwrap();
    let explicit = host.call("updateBot", json!({"id": first["id"], "cwd": project})).await;
    assert_eq!(canonical(&explicit["bot"]["cwd"]), std::fs::canonicalize(&project).unwrap());
    assert_eq!(explicit["bot"]["managedWorkspace"], false);
    let restored = host.call("updateBot", json!({"id": first["id"], "cwd": ""})).await;
    assert_eq!(canonical(&restored["bot"]["cwd"]), dir);
    assert_eq!(std::fs::read_to_string(dir.join("keep.txt")).unwrap(), "persistent output");
    host.call("deleteBot", json!({"botId": first["id"]})).await;
    assert!(dir.join("keep.txt").is_file(), "deleting a bot must retain its files");
    let group = host.call("createBot", json!({"name": "Group", "kind": "group", "members": [second["id"]]})).await;
    assert_eq!(group["bot"]["cwd"], "");
    assert_eq!(group["bot"]["managedWorkspace"], false);
}

// ---------- wire shape snapshot (docs/reference/compatibility.md) ----------

/// Snapshot of what clients decode: field paths and JSON types of `hello` and the event
/// stream after a full turn. Values aren't kept, only the shape.
const WIRE_SHAPE: &str = "tests/fixtures/wire-shape.json";
/// Re-records the snapshot (refused while a field is removed or retyped without a new `minApp`).
const UPDATE_ENV: &str = "CODYNC_UPDATE_WIRE_SHAPE";
/// Depends on the machine (installed harnesses, displays, addresses, local usage), not the code.
const MACHINE_DEPENDENT: [&str; 4] = ["backends", "screen", "urls", "usage"];

fn json_type(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Every field path under `v` with its type; array elements merge under `[]`.
fn shape(v: &Value, path: &str, out: &mut std::collections::BTreeMap<String, String>) {
    out.insert(path.to_owned(), json_type(v).to_owned());
    match v {
        Value::Object(map) => {
            for (k, child) in map {
                if !MACHINE_DEPENDENT.contains(&k.as_str()) {
                    shape(child, &format!("{path}.{k}"), out);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                shape(item, &format!("{path}[]"), out);
            }
        }
        _ => {}
    }
}

/// The shape of a stream event, keyed by its type (and an entry's kind: their data differ).
fn event_shape(ev: &Value, out: &mut std::collections::BTreeMap<String, String>) {
    let ty = ev["type"].as_str().unwrap_or("?");
    let key = match ev["entry"]["kind"].as_str() {
        Some(kind) if ty == "entry" => format!("event:entry({kind})"),
        _ => format!("event:{ty}"),
    };
    shape(ev, &key, out);
}

/// What changed between the recorded and the current shape: (breaking, additions).
/// A field that was `null` on either side can't be judged by its type.
fn shape_diff(
    recorded: &std::collections::BTreeMap<String, String>,
    current: &std::collections::BTreeMap<String, String>,
) -> (Vec<String>, Vec<String>) {
    let mut breaking = vec![];
    for (path, ty) in recorded {
        match current.get(path) {
            None => breaking.push(format!("removed {path} ({ty})")),
            Some(now) if now != ty && now != "null" && ty != "null" => {
                breaking.push(format!("retyped {path}: {ty} → {now}"));
            }
            Some(_) => {}
        }
    }
    let added = current
        .iter()
        .filter(|(path, ty)| recorded.get(*path).is_none_or(|old| old == "null" && *ty != "null"))
        .map(|(path, ty)| format!("{path} ({ty})"))
        .collect();
    (breaking, added)
}

#[test]
fn shape_diff_tells_breaking_from_additive() {
    let map = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
    let recorded = map(&[("a", "object"), ("a.x", "string"), ("a.y", "null"), ("a.z", "number")]);
    let current = map(&[("a", "object"), ("a.x", "number"), ("a.y", "string"), ("a.w", "bool")]);
    let (breaking, added) = shape_diff(&recorded, &current);
    assert_eq!(breaking, ["retyped a.x: string → number", "removed a.z (number)"]);
    assert_eq!(added, ["a.w (bool)", "a.y (string)"]);
}

/// Reads the event stream's catch-up until it goes quiet.
async fn catch_up(host: &Host) -> Vec<Value> {
    let res = reqwest::Client::new()
        .get(format!("{}/events?since=0&client=test", host.base))
        .bearer_auth(&host.token)
        .send()
        .await
        .unwrap();
    let mut body = res.bytes_stream();
    let (mut buf, mut events) = (String::new(), vec![]);
    while let Ok(Some(Ok(chunk))) = tokio::time::timeout(Duration::from_millis(1500), body.next()).await {
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(i) = buf.find('\n') {
            let line: String = buf.drain(..=i).collect();
            if let Some(data) = line.trim_end().strip_prefix("data:")
                && let Ok(v) = serde_json::from_str::<Value>(data.trim_start())
            {
                events.push(v);
            }
        }
    }
    events
}

/// Older apps read what the host sends: a removed or retyped field breaks them. This fails
/// until the field is kept, or `MIN_APP` is raised the two-release way and the snapshot
/// re-recorded. New fields only need recording (`CODYNC_UPDATE_WIRE_SHAPE=1`).
#[tokio::test]
async fn wire_shape_matches_the_snapshot() {
    if Command::new(common::python()).arg("--version").output().is_err() {
        eprintln!("skipping: Python not available");
        return;
    }
    let host = start_host_with_account_clis().await;
    let agent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fake_agent.py");
    let created = host
        .call(
            "createBot",
            json!({
                "name": "Tester", "backend": "custom", "command": common::python_agent(&agent),
                "cwd": host.workspaces.root(), "permission": "ask", "title": "  Testes \t de  fumaça ",
                // Cliente antigo explícito continua aceito; o snapshot cobre a união legado + v2.
                "avatarConfig": {"shape":"cubo","color":"#3B82F6","eye":"auto","head":"none","face":"none","ears":"none","chin":"none","neck":"none","cheeks":"none","badge":"none","badgeStyle":"light"},
            }),
        )
        .await;
    // D133: a função chega limpa ao app e muda pelo mesmo updateBot.
    assert_eq!(created["bot"]["title"], "Testes de fumaça");
    let bot = created["bot"]["id"].as_str().unwrap().to_owned();
    let renamed = host.call("updateBot", json!({"id": bot, "title": "Revisor"})).await;
    assert_eq!(renamed["bot"]["title"], "Revisor");
    let sent = host
        .call(
            "send",
            json!({"botId": bot, "text": "go", "clientNonce": "n1", "ingress":{"caller":"device","device":"forged"},
        "contentOrigin":{"schema":1,"draftId":uuid::Uuid::new_v4().to_string(),"revision":1,"textLength":2,
            "deviceId":uuid::Uuid::new_v4().to_string(),"channel":"desktopComposer","interaction":"send",
            "segments":[{"start":0,"end":2,"kind":"typed"}]}}),
        )
        .await;
    assert_eq!(sent["entry"]["data"]["contentOrigin"]["status"], "appReported");
    assert_eq!(sent["entry"]["data"]["contentOrigin"]["ingress"], json!({"caller":"local"}));
    // Reenvio com a mesma chave devolve a mensagem original, sem trocar a procedência; com outro texto é recusado.
    let retry = host.call("send", json!({"botId":bot,"text":"go","clientNonce":"n1","human":true})).await;
    assert_eq!(retry["entry"]["id"], sent["entry"]["id"]);
    assert_eq!(retry["entry"]["data"]["contentOrigin"], sent["entry"]["data"]["contentOrigin"]);
    assert_eq!(retry["entry"]["data"]["text"], "go");
    let changed = host.try_call("send", json!({"botId":bot,"text":"changed","clientNonce":"n1","human":true})).await;
    assert!(changed.to_string().contains("nonce_intent_conflict"), "{changed}");
    let card = host.wait_for(&bot, |e| e["kind"] == "permission" && e["data"]["status"] == "pending").await;
    host.call(
        "respondPermission",
        json!({"botId": card["botId"], "entryRev": card["rev"], "entryId": card["id"], "optionId": "allow"}),
    )
    .await;
    host.wait_for(&bot, |e| e["kind"] == "agent" && e["data"]["final"] == true).await;
    // A bot another bot created: its `createdBy`.
    let lead = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/team_agent.py");
    let lead = host
        .call(
            "createBot",
            json!({"name": "Lead", "backend": "custom", "command": common::python_agent(&lead), "connectors": [],
                "permission": "auto", "permissionChosenByUser": true, "manageTeam": true}),
        )
        .await["bot"]
        .clone();
    assert_eq!(lead["manageTeam"], false, "legacy clients can save without assigning authority");
    // D366: quem gerencia é o Berga fixo do Hub novo, com o agente de teste no motor.
    let command = common::python_agent(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/team_agent.py"));
    let lead = host.call("teamSettings", json!({})).await["primaryBotId"].as_str().unwrap().to_owned();
    host.call("updateBot", json!({"id": lead, "backend": "custom", "command": command, "connectors": []})).await;
    let workspace_root = host.workspaces.root();
    host.call("setTeamSettings", json!({"primaryBotId":lead, "permission":"auto", "workspaceRoot":workspace_root}))
        .await;
    // D67/PR27: o usuário escolhe a autonomia; createBot já não aceita auto como padrão.
    host.call("updateBot", json!({"id":lead, "permission":"auto", "permissionChosenByUser":true})).await;
    host.call("send", json!({"botId": lead, "text": "MANAGE create_bot {\"name\": \"Made\"}", "clientNonce": "n2"}))
        .await;
    // D125: origem desconhecida em Auto também precisa de confirmação síncrona.
    let card = host.wait_for(&lead, |e| e["kind"] == "permission" && e["data"]["status"] == "pending").await;
    host.call(
        "respondPermission",
        json!({"botId":card["botId"],"entryRev":card["rev"],"entryId":card["id"],"optionId":"allow"}),
    )
    .await;
    let made = host.wait_for(&lead, |e| e["kind"] == "agent" && e["data"]["final"] == true).await;
    assert!(made["data"]["text"].as_str().unwrap().starts_with("Done: "), "{made}");

    let hello = host.call("hello", json!({})).await;
    let min_app = hello["minApp"].as_str().unwrap().to_owned();
    let mut current = std::collections::BTreeMap::new();
    shape(&hello, "hello", &mut current);
    shape(&sent, "send", &mut current);
    let accounts = host.call("donnaEngineAccounts", json!({})).await;
    assert_eq!(accounts["accounts"].as_array().unwrap().len(), 2);
    assert_eq!(accounts["accounts"][0]["billing"], "subscription");
    assert_eq!(accounts["accounts"][1]["models"][0]["id"], "gpt-6.1-sol");
    for private in ["TEST_SECRET_SENTINEL", "wire-owner@", "/private/profile"] {
        assert!(!accounts.to_string().contains(private), "o contrato expôs {private}");
    }
    shape(&accounts, "engineAccounts", &mut current);
    let made = host.call("botGroupsEdit", json!({"op":"add", "name":"Marketing"})).await;
    let group_id = made["created"].as_str().unwrap();
    host.call("botGroupsEdit", json!({"op":"place", "botId":bot, "groupId":group_id})).await;
    let groups = host.call("botGroups", json!({})).await;
    assert_eq!(groups["groups"][0]["bots"], json!([bot]));
    shape(&groups, "botGroups", &mut current);
    let edited = host.call("botGroupsEdit", json!({"op":"rename", "id":group_id, "name":"Vendas"})).await;
    shape(&edited, "botGroupsEdit", &mut current);
    let added = host.call("botGroupsEdit", json!({"op":"add", "name":"Outra"})).await;
    shape(&added, "botGroupsEdit", &mut current);
    // Abre o stream antes da mudança para registrar também o evento ao vivo real.
    let stream = reqwest::Client::new()
        .get(format!("{}/events?since=0&client=test", host.base))
        .bearer_auth(&host.token)
        .send()
        .await
        .unwrap();
    let profile = host.call("memoryProfile", json!({})).await;
    shape(&profile, "memoryProfile", &mut current);
    let profile = host.call("saveMemoryProfile", json!({"content":"<!-- mexerica:sobre-voce v1 -->\n# Sobre você\n\n## Quem é\nGosta de café.\n", "base":profile["revision"]})).await;
    shape(&profile, "saveMemoryProfile", &mut current);
    let functions = host.call("functionModels", json!({})).await;
    assert_eq!(functions["functions"].as_array().unwrap().len(), 13);
    shape(&functions, "functionModelsInitial", &mut current);
    let revision = functions["document"]["revision"].as_u64().unwrap();
    let account_id = accounts["accounts"][1]["id"].as_str().unwrap();
    let saved = host
        .call(
            "saveFunctionModel",
            json!({"expectedRevision":revision,"functionId":"bot_naming",
        "selection":{"mode":"custom","accountId":account_id,"modelId":"gpt-6.1-sol",
        "modelOptions":{"reasoning_effort":"high"}}}),
        )
        .await;
    assert_eq!(saved["document"]["revision"], revision + 1);
    shape(&saved, "saveFunctionModel", &mut current);
    let functions = host.call("functionModels", json!({})).await;
    for private in ["TEST_SECRET_SENTINEL", "wire-owner@", "/private/profile", "quotaIdentity", "launch"] {
        assert!(!functions.to_string().contains(private), "o contrato expôs {private}");
    }
    shape(&functions, "functionModels", &mut current);
    let checked = host.call("checkFunctionModel", json!({"functionId":"bot_naming"})).await;
    assert_eq!(checked["unavailable"]["reason"], "codex_subscription_unavailable");
    shape(&checked, "checkFunctionModel", &mut current);
    let restored =
        host.call("restoreFunctionModel", json!({"expectedRevision":revision+1,"functionId":"bot_naming"})).await;
    shape(&restored, "restoreFunctionModel", &mut current);
    let restored = host.call("restoreFunctionModels", json!({"expectedRevision":revision+2})).await;
    shape(&restored, "restoreFunctionModels", &mut current);
    host.call("botGroupsEdit", json!({"op":"rename", "id":group_id, "name":"Vendas"})).await;
    let mut body = stream.bytes_stream();
    let mut buffer = String::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut saw_groups = false;
    let mut saw_functions = false;
    while !(saw_groups && saw_functions) && Instant::now() < deadline {
        let chunk = tokio::time::timeout(Duration::from_secs(2), body.next()).await.unwrap().unwrap().unwrap();
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(i) = buffer.find('\n') {
            let line: String = buffer.drain(..=i).collect();
            if let Some(data) = line.trim_end().strip_prefix("data:")
                && let Ok(ev) = serde_json::from_str::<Value>(data.trim_start())
                && ev["type"] == "botGroups"
            {
                event_shape(&ev, &mut current);
                saw_groups = true;
            }
            if let Some(data) = line.trim_end().strip_prefix("data:")
                && let Ok(ev) = serde_json::from_str::<Value>(data.trim_start())
                && ev["type"] == "functionModelsChanged"
            {
                event_shape(&ev, &mut current);
                saw_functions = true;
            }
        }
    }
    assert!(saw_groups, "o stream deve entregar a mudança de grupos ao vivo");
    assert!(saw_functions, "o stream deve entregar a revisão de funções ao vivo");
    for ev in catch_up(&host).await {
        event_shape(&ev, &mut current);
    }
    assert!(current.keys().any(|k| k.starts_with("event:entry(tool)")), "the turn's entries are missing");

    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(WIRE_SHAPE);
    let recorded: Value = std::fs::read_to_string(&file).map_or(Value::Null, |s| serde_json::from_str(&s).unwrap());
    let recorded_shapes: std::collections::BTreeMap<String, String> =
        serde_json::from_value(recorded["shapes"].clone()).unwrap_or_default();
    let (breaking, added) = shape_diff(&recorded_shapes, &current);
    let floor_raised = recorded["minApp"].as_str() != Some(min_app.as_str());
    assert!(
        breaking.is_empty() || floor_raised,
        "Older apps would break: {breaking:#?}\nKeep these fields, or follow the two-release rule \
         in docs/reference/compatibility.md: raise MIN_APP (host/src/compat.rs) once that iPhone \
         version is live, then re-record with {UPDATE_ENV}=1 cargo test --test e2e wire_shape"
    );
    if std::env::var_os(UPDATE_ENV).is_some() {
        let json = json!({"minApp": min_app, "shapes": current});
        std::fs::write(&file, serde_json::to_string_pretty(&json).unwrap() + "\n").unwrap();
        return;
    }
    assert!(
        breaking.is_empty() && added.is_empty() && !floor_raised,
        "The wire shape changed.\nAdded: {added:#?}\nRemoved or retyped (minApp was raised): {breaking:#?}\n\
         Record it with {UPDATE_ENV}=1 cargo test --test e2e wire_shape"
    );
}

#[tokio::test]
async fn d520_real_startup_creates_missing_private_home_and_token() {
    let host = common::host::Host::start().await;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let directory = std::fs::symlink_metadata(&host.home).expect("pasta sintética criada pelo Host");
        assert!(directory.is_dir() && !directory.file_type().is_symlink());
        // SAFETY: getter sem ponteiros ou efeitos colaterais.
        let uid = unsafe { libc::geteuid() };
        assert_eq!(directory.uid(), uid);
        assert_eq!(directory.permissions().mode() & 0o7777, 0o700);
        let token = std::fs::symlink_metadata(host.home.join("token")).expect("token da fixture");
        assert!(token.is_file() && !token.file_type().is_symlink());
        assert_eq!(token.permissions().mode() & 0o7777, 0o600);
    }
    // Drop encerra somente o processo próprio da fixture e limpa seus arquivos.
    drop(host);
}
