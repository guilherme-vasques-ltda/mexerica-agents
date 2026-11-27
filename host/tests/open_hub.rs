#![cfg(unix)]
#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};
use std::fs::{self, DirBuilder};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::{Child, Command};

struct Profile(PathBuf);

impl Drop for Profile {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn client(socket: &Path) -> reqwest::Client {
    reqwest::Client::builder()
        .unix_socket(socket.to_path_buf())
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

async fn health(client: &reqwest::Client, child: &mut Child) -> Value {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Ok(response) = client.get("http://localhost/health").send().await
                && response.status().is_success()
            {
                break response.json().await.unwrap();
            }
            assert!(child.try_wait().unwrap().is_none(), "Hub exited before readiness");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap()
}

/// `/health` provado: um nonce novo e o HMAC-SHA256 do token local sobre ele, sem o token na URL.
/// Devolve o endereço e o nonce, que o recibo da resposta precisa conferir.
fn proved(token: &str, nonce: [u8; 32]) -> (String, String) {
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
    (format!("http://localhost/health?nonce={nonce}&proof={proof}"), nonce)
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

/// Identidade do Hub, que o /health só mostra com a prova do token local e o recibo na resposta.
async fn identity(client: &reqwest::Client, credential: &str, nonce: [u8; 32]) -> Value {
    let (url, nonce) = proved(credential, nonce);
    let health: Value = client.get(url).send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
    for field in ["hostId", "computerId"] {
        assert!(health[field].as_str().is_some_and(|value| !value.is_empty()), "{field}: {health}");
    }
    assert_eq!(health["receipt"], recibo(credential, &nonce, &health), "o recibo prova a resposta");
    health
}

async fn close_stdin(child: &mut Child) {
    drop(child.stdin.take());
    assert!(tokio::time::timeout(Duration::from_secs(10), child.wait()).await.unwrap().unwrap().success());
}

async fn refused(command: &mut Command) {
    let mut child = command.spawn().unwrap();
    let _pipe = child.stdin.take().unwrap();
    assert!(!tokio::time::timeout(Duration::from_secs(10), child.wait()).await.unwrap().unwrap().success());
}

async fn rpc(client: &reqwest::Client, bearer: &str, method: &str, body: Value) -> Value {
    client
        .post(format!("http://localhost/api/{method}"))
        .bearer_auth(bearer)
        .json(&body)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn open_hub_owns_its_socket_and_exits_at_stdin_eof() {
    if option_env!("CODYNC_ENV") != Some("open") {
        return;
    }
    let id = uuid::Uuid::new_v4().simple().to_string();
    let root = std::env::temp_dir().canonicalize().unwrap().join(&id[..8]);
    DirBuilder::new().mode(0o700).create(&root).unwrap();
    let root = Profile(root);
    let private = root.0.join(".mexerica-open");
    let home = private.join("home");
    let tmp = private.join("tmp");
    let session = tmp.join("h-a");
    DirBuilder::new().recursive(true).mode(0o700).create(&home).unwrap();
    DirBuilder::new().recursive(true).mode(0o700).create(&session).unwrap();
    let socket = session.join("s");
    let data = home.join(".mexerica-open");
    let host = |socket: &Path| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_donnabot-host"));
        command
            .env_clear()
            .env("HOME", &home)
            .env("TMPDIR", &tmp)
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("OPEN_HUB_SOCKET", socket)
            .current_dir(&home)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        command
    };
    let serve = ["serve", "--only-this-mac", "--bind", "127.0.0.1", "--port", "19524"];
    refused(host(&socket).arg("status")).await;
    refused(host(&socket).args(serve).env("MEXERICA_HOME", &data)).await;
    fs::write(&socket, b"existing").unwrap();
    refused(host(&socket).args(serve)).await;
    assert_eq!(fs::read(&socket).unwrap(), b"existing");
    fs::remove_file(&socket).unwrap();
    assert!(!data.exists());
    let occupied = tokio::net::TcpListener::bind("127.0.0.1:19524").await.unwrap();
    refused(host(&socket).args(serve)).await;
    assert!(!socket.exists());
    assert!(!data.exists());
    drop(occupied);

    symlink(&tmp, &data).unwrap();
    refused(host(&socket).args(serve)).await;
    assert!(!socket.exists());
    fs::remove_file(&data).unwrap();
    close_stdin(&mut host(&socket).args(serve).spawn().unwrap()).await;
    assert!(!socket.exists());

    let mut child = host(&socket).args(serve).spawn().unwrap();
    let session_client = client(&socket);
    let first = health(&session_client, &mut child).await;
    assert_eq!(first["environment"], "open");
    assert!(first.get("hostId").is_none() && first.get("computerId").is_none(), "{first}");
    assert_eq!(fs::metadata(&socket).unwrap().permissions().mode() & 0o777, 0o600);
    let tcp = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(2)).build().unwrap();
    assert_eq!(tcp.get("http://127.0.0.1:19524/health").send().await.unwrap().status(), reqwest::StatusCode::NOT_FOUND);
    let endpoint = "http://localhost/api/hello";
    assert_eq!(session_client.post(endpoint).send().await.unwrap().status(), reqwest::StatusCode::UNAUTHORIZED);
    for path in ["/events", "/term/fixture"] {
        assert_eq!(
            session_client.get(format!("http://localhost{path}")).send().await.unwrap().status(),
            reqwest::StatusCode::UNAUTHORIZED
        );
    }
    let token_path = data.join("token");
    assert_eq!(fs::metadata(&token_path).unwrap().permissions().mode() & 0o777, 0o600);
    let credential = fs::read_to_string(&token_path).unwrap();
    let first = identity(&session_client, &credential, [3u8; 32]).await;
    assert!(session_client.post(endpoint).bearer_auth(&credential).send().await.unwrap().status().is_success());
    let catalog = rpc(&session_client, &credential, "marketCatalogo", json!({})).await;
    let services = catalog["servicos"].as_array().unwrap();
    assert!(services.iter().any(|s| s["id"] == "linear"));
    assert!(services.iter().all(|s| s["contas"] == json!([])));
    assert_eq!(catalog["appsProprios"]["google"], Value::Null);
    for method in ["marketContas", "marketConectar", "donnaWhatsapp", "credentialStatus"] {
        let response = session_client
            .post(format!("http://localhost/api/{method}"))
            .bearer_auth(&credential)
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN, "{method}");
    }
    assert!(!rpc(&session_client, &credential, "skills", json!({})).await.is_null());
    for body in [
        json!({"source": "pdf"}),
        json!({"name": "manual", "description": "d", "instructions": "texto"}),
        json!({"catalogId": "grilling", "source": "pdf"}),
    ] {
        let response = session_client
            .post("http://localhost/api/installSkill")
            .bearer_auth(&credential)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN, "installSkill {body}");
    }
    assert_eq!(
        tcp.post("http://127.0.0.1:19524/api/hello").bearer_auth(&credential).send().await.unwrap().status(),
        reqwest::StatusCode::NOT_FOUND
    );
    let spaces = rpc(&session_client, &credential, "donnaSpaces", json!({"hubId": first["hostId"]})).await;
    let created = rpc(
        &session_client,
        &credential,
        "donnaSpaceCreate",
        json!({
            "hubId": first["hostId"],
            "expectedRevision": spaces["revision"],
            "name": "Fixture Open",
            "tile": {"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"FO"}}
        }),
    )
    .await;
    assert_eq!(created["spaces"][1]["name"], "Fixture Open");
    assert!(created["spaces"][1]["principalBotId"].is_null());
    let bot =
        rpc(&session_client, &credential, "createBot", json!({"name":"Fixture Bot","backend":"codex","skills":[]}))
            .await;
    let renamed =
        rpc(&session_client, &credential, "updateBot", json!({"id":bot["bot"]["id"],"name":"Fixture Renamed"})).await;
    assert_eq!(renamed["bot"]["name"], "Fixture Renamed");
    let editor_request = json!({"botId":bot["bot"]["id"]});
    let mut responses = Vec::new();
    for (method, body) in [("donnaEngineAccess", json!({})), ("donnaBotEditor", editor_request.clone())] {
        responses.push(
            session_client
                .post(format!("http://localhost/api/{method}"))
                .bearer_auth(&credential)
                .json(&body)
                .send()
                .await
                .unwrap(),
        );
    }
    assert_eq!(
        responses.iter().map(reqwest::Response::status).collect::<Vec<_>>(),
        [reqwest::StatusCode::OK; 2],
        "the local window needs the engine permission probe and bot editor"
    );
    let mut responses = responses.into_iter();
    let access: Value = responses.next().unwrap().json().await.unwrap();
    assert_eq!(access, json!({"capability":"engine.accounts","canManage":true}));
    let editor: Value = responses.next().unwrap().json().await.unwrap();
    assert_eq!(editor["profile"]["name"], "Fixture Renamed");
    assert_eq!(
        editor["capabilities"]["editableFields"],
        json!(["name", "description", "avatarConfig", "mexeriquinha", "title"])
    );
    assert_eq!(editor["engineAccounts"], json!([]));
    assert!(editor["engineRuntime"].is_null());
    let version = &editor["settingsVersion"];
    for patch in [json!({"cwd":"outside"}), json!({"forkEditor":{"engineAccountId":null}}), json!({"backend":"claude"})]
    {
        assert_eq!(
            session_client
                .post("http://localhost/api/donnaBotEditorSave")
                .bearer_auth(&credential)
                .json(&json!({"botId":bot["bot"]["id"],"expectedVersion":version,"patch":patch}))
                .send()
                .await
                .unwrap()
                .status(),
            reqwest::StatusCode::FORBIDDEN
        );
    }
    let edited = rpc(
        &session_client,
        &credential,
        "donnaBotEditorSave",
        json!({
            "botId":bot["bot"]["id"], "expectedVersion":version, "patch":{"description":"Fixture description"}
        }),
    )
    .await;
    assert_eq!(edited["profile"]["description"], "Fixture description");
    assert_ne!(edited["settingsVersion"], *version);
    assert_ne!(edited["application"], "apply_pending");
    for method in ["donnaEngineAccess", "donnaBotEditor", "donnaBotEditorSave"] {
        assert_eq!(
            session_client
                .post(format!("http://localhost/api/{method}"))
                .json(&editor_request)
                .send()
                .await
                .unwrap()
                .status(),
            reqwest::StatusCode::UNAUTHORIZED
        );
    }
    let external = root.0.join("outside");
    fs::create_dir(&external).unwrap();
    for (method, body) in [
        ("createBot", json!({"name":"Fixture External","backend":"codex","cwd":external})),
        ("updateBot", json!({"id":bot["bot"]["id"],"cwd":external})),
    ] {
        assert_eq!(
            session_client
                .post(format!("http://localhost/api/{method}"))
                .bearer_auth(&credential)
                .json(&body)
                .send()
                .await
                .unwrap()
                .status(),
            reqwest::StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        rpc(&session_client, &credential, "donnaEngineAccounts", json!({"refresh":true})).await["accounts"],
        json!([])
    );
    for (backend, kind) in [("claude", "cli"), ("grok", "cli"), ("codex", "api")] {
        assert_eq!(
            session_client
                .post("http://localhost/api/donnaEngineAccountCreate")
                .bearer_auth(&credential)
                .json(&json!({"backend":backend,"kind":kind,"label":"Open","clientRequestId":"refused"}))
                .send()
                .await
                .unwrap()
                .status(),
            reqwest::StatusCode::BAD_REQUEST
        );
    }
    let account = rpc(
        &session_client,
        &credential,
        "donnaEngineAccountCreate",
        json!({"backend":"codex","kind":"cli","label":"Open","clientRequestId":"create-open"}),
    )
    .await;
    assert_eq!(account["state"], "unverified");
    let operation = rpc(&session_client, &credential, "donnaEngineSetupStart", json!({
        "engineAccountId":account["id"],"expectedRevision":account["revision"],"action":"login","clientRequestId":"login-open"
    })).await;
    let final_state = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let reply = rpc(
                &session_client,
                &credential,
                "donnaEngineSetupAction",
                json!({
                    "operationId":operation["operationId"],"generation":operation["generation"],"action":"read"
                }),
            )
            .await;
            if reply["phase"] == "failed" {
                break reply;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        final_state["errorCode"],
        if cfg!(target_os = "macos") { "open_codex_unavailable" } else { "open_codex_platform" }
    );
    assert!(final_state["challenge"].is_null());
    let berga = rpc(&session_client, &credential, "teamSettings", json!({})).await["primaryBotId"].clone();
    let personal = &created["spaces"][0]["id"];
    let conversation = json!({"hubId": first["hostId"], "spaceId": personal, "conversationId": berga});
    let listed = rpc(&session_client, &credential, "donnaConversations", conversation.clone()).await;
    assert!(listed["conversations"].as_array().unwrap().iter().any(|c| c["id"] == berga));
    let before = rpc(&session_client, &credential, "donnaConversationHistory", conversation.clone()).await;
    let message = json!({"hubId": first["hostId"], "spaceId": personal, "conversationId": berga, "botId": berga, "text": "Oi", "clientNonce": "open-chat"});
    let refused =
        session_client.post("http://localhost/api/send").bearer_auth(&credential).json(&message).send().await.unwrap();
    assert_eq!(refused.status(), reqwest::StatusCode::BAD_REQUEST);
    assert!(refused.text().await.unwrap().contains("open_chat_account_required"));
    for (key, value) in [("attachments", json!(["upload-fixture"])), ("sessionId", json!("session-fixture"))] {
        let mut body = message.clone();
        body[key] = value;
        let status = session_client
            .post("http://localhost/api/send")
            .bearer_auth(&credential)
            .json(&body)
            .send()
            .await
            .unwrap()
            .status();
        assert_eq!(status, reqwest::StatusCode::FORBIDDEN, "{key}");
    }
    let after = rpc(&session_client, &credential, "donnaConversationHistory", conversation).await;
    assert_eq!(after["entries"], before["entries"], "a refused message is not saved");
    rpc(&session_client, &credential, "stop", json!({"botId": berga})).await;
    for method in [
        "donnaConversationSend",
        "requestScreenPermission",
        "setCloud",
        "installHostUpdate",
        "credentialStatus",
        "donnaWhatsappSave",
        "donnaWhatsappPair",
        "donnaWhatsappWacliRetry",
        "listDirs",
        "installConnector",
        "importConnectors",
        "connectorVerify",
        "saveRoutine",
        "credentialSaveLogin",
        "donnaEngineAccountUsage",
        "donnaEngineFallback",
        "donnaEngineFallbackSave",
        "donnaEngineFallbackAction",
        "unlistedFixtureMethod",
    ] {
        assert_eq!(
            session_client
                .post(format!("http://localhost/api/{method}"))
                .bearer_auth(&credential)
                .send()
                .await
                .unwrap()
                .status(),
            reqwest::StatusCode::FORBIDDEN,
            "{method} must be rejected by the Open API"
        );
    }
    let stream = session_client.get("http://localhost/events").bearer_auth(&credential).send().await.unwrap();
    assert_eq!(stream.status(), reqwest::StatusCode::OK);
    close_stdin(&mut child).await;
    assert!(!socket.exists());
    {
        let saved =
            rusqlite::Connection::open_with_flags(data.join("codync.db"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        let config: String = saved
            .query_row("SELECT config FROM bots WHERE id = ?1", [bot["bot"]["id"].as_str().unwrap()], |row| row.get(0))
            .unwrap();
        assert_eq!(serde_json::from_str::<Value>(&config).unwrap()["name"], "Fixture Renamed");
    }

    let second_session = tmp.join("h-b");
    DirBuilder::new().mode(0o700).create(&second_session).unwrap();
    let second_socket = second_session.join("s");
    let second_client = client(&second_socket);
    let mut child = host(&second_socket).args(serve).spawn().unwrap();
    health(&second_client, &mut child).await;
    let new_credential = fs::read_to_string(&token_path).unwrap();
    assert!(new_credential != credential, "restart must rotate the bearer");
    let second = identity(&second_client, &new_credential, [5u8; 32]).await;
    assert_eq!(first["hostId"], second["hostId"]);
    assert_eq!(first["computerId"], second["computerId"]);
    assert_eq!(
        second_client.post(endpoint).bearer_auth(&credential).send().await.unwrap().status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
    assert!(second_client.post(endpoint).bearer_auth(&new_credential).send().await.unwrap().status().is_success());
    let reopened = rpc(&second_client, &new_credential, "donnaSpaces", json!({"hubId": second["hostId"]})).await;
    assert_eq!(reopened["spaces"], created["spaces"]);
    close_stdin(&mut child).await;
    assert!(!second_socket.exists());
}
