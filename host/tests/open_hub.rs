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
    let root = Profile(Path::new("/tmp").canonicalize().unwrap().join(format!("oh-{}", uuid::Uuid::new_v4().simple())));
    let private = root.0.join(".mexerica-open");
    let home = private.join("home");
    let tmp = private.join("tmp");
    let session = tmp.join("h-test");
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
    assert!(session_client.post(endpoint).bearer_auth(&credential).send().await.unwrap().status().is_success());
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
    for method in [
        "send",
        "requestScreenPermission",
        "setCloud",
        "installHostUpdate",
        "donnaEngineSetupStart",
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

    let second_session = tmp.join("h-restart");
    DirBuilder::new().mode(0o700).create(&second_session).unwrap();
    let second_socket = second_session.join("s");
    let second_client = client(&second_socket);
    let mut child = host(&second_socket).args(serve).spawn().unwrap();
    let second = health(&second_client, &mut child).await;
    assert_eq!(first["hostId"], second["hostId"]);
    assert_eq!(first["computerId"], second["computerId"]);
    let new_credential = fs::read_to_string(&token_path).unwrap();
    assert!(new_credential != credential, "restart must rotate the bearer");
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
