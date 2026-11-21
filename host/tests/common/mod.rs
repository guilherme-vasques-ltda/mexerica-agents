// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Shared by the end-to-end tests.

pub mod host;
pub mod workspaces;

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Guarda o filho próprio inclusive durante panic/cancelamento da prontidão.
struct StartingHost(Option<Child>);

impl Drop for StartingHost {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut()
            && !matches!(child.try_wait(), Ok(Some(_)))
        {
            stop(child);
        }
    }
}

/// Runs `codync-host serve` for `home` until it answers with this home's token: the process and its base URL. A port
/// picked here is only free for a moment; when something else takes it before the host binds, the
/// host exits and this starts it again on another one.
pub async fn serve(home: &Path) -> (Child, String) {
    serve_with_env(home, &[]).await
}

pub async fn serve_with_env(home: &Path, env: &[(&str, std::ffi::OsString)]) -> (Child, String) {
    serve_inner(home, env, false).await
}

/// Como `serve_with_env`, mas o Hub NÃO herda o ambiente do teste: só `env` e as variáveis mínimas abaixo. Sem
/// chaves de API, PATH, HOME ou configuração da máquina.
pub async fn serve_isolado(home: &Path, env: &[(&str, std::ffi::OsString)]) -> (Child, String) {
    serve_inner(home, env, true).await
}

async fn serve_inner(home: &Path, env: &[(&str, std::ffi::OsString)], limpar: bool) -> (Child, String) {
    let deadline = Instant::now() + Duration::from_secs(60);
    let client = reqwest::Client::builder().timeout(Duration::from_secs(1)).build().unwrap();
    loop {
        let port = std::net::TcpListener::bind("127.0.0.1:0").expect("free port").local_addr().expect("port").port();
        let base = format!("http://127.0.0.1:{port}");
        // A raiz de política do host em teste aponta para uma pasta vazia do
        // próprio teste: o /etc da máquina não pode derrubar tarefa de fundo.
        let policy_root = home.join("policy-root");
        std::fs::create_dir_all(&policy_root).expect("pasta de política do teste");
        let mut command = Command::new(env!("CARGO_BIN_EXE_donnabot-host"));
        if limpar {
            command.env_clear();
        }
        if !env.iter().any(|(key, _)| *key == "MEXERICA_CLAUDE_SDK") {
            command.env_remove("MEXERICA_CLAUDE_SDK");
        }
        let mut child = StartingHost(Some(
            command
                .args(["serve", "--bind", "127.0.0.1", "--port", &port.to_string()])
                .env("MEXERICA_HOME", home)
                .env("CODEX_HOME", home.join("codex"))
                .env("CLAUDE_CONFIG_DIR", home.join("claude-test"))
                .env("CODYNC_CLOUD", "off")
                .env("DONNABOT_TEST_POLICY_ROOT", &policy_root)
                .envs(env.iter().map(|(key, value)| (*key, value)))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("start the host"),
        ));
        let pid = child.0.as_ref().expect("starting host").id();
        let (mut token_present, mut last_http, mut last_error) = (false, None, "not_attempted");
        let mut exit = child.0.as_mut().expect("starting host").try_wait().expect("host state");
        while exit.is_none() {
            // Another test can bind the sampled port first. Its public /health is not our host's readiness.
            // teamSettings confirma a API/Store com o token exato, sem descoberta de URLs do hello.
            let token = std::fs::read_to_string(home.join("token"));
            token_present = token.as_ref().is_ok_and(|token| !token.trim().is_empty());
            if let Ok(token) = token {
                match client
                    .post(format!("{base}/api/teamSettings"))
                    .bearer_auth(token.trim())
                    .json(&serde_json::json!({}))
                    .send()
                    .await
                {
                    Ok(response) => {
                        last_http = Some(response.status().as_u16());
                        last_error = "none";
                        if response.status().is_success() {
                            return (child.0.take().expect("ready host"), base);
                        }
                    }
                    Err(error) => {
                        // Não formatar reqwest::Error: pode incluir URL/cabeçalhos.
                        last_error = if error.is_timeout() {
                            "timeout"
                        } else if error.is_connect() {
                            "connect"
                        } else {
                            "request"
                        };
                    }
                }
            }
            assert!(
                Instant::now() < deadline,
                "host didn't start: pid={pid}, tokenPresent={token_present}, lastHttp={last_http:?}, lastError={last_error}, exit={exit:?}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
            exit = child.0.as_mut().expect("starting host").try_wait().expect("host state");
        }
        assert!(
            Instant::now() < deadline,
            "host didn't start: pid={pid}, tokenPresent={token_present}, lastHttp={last_http:?}, lastError={last_error}, exit={exit:?}"
        );
    }
}

/// Stops a test host the way launchd does (SIGTERM), so it shuts down its own
/// children: a SIGKILL would orphan whatever it had running, such as a harness
/// sign-in check (`cursor-agent status` then spins forever). Kills it after 5 s.
pub fn stop(child: &mut Child) {
    let _ = Command::new("kill").arg(child.id().to_string()).status();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Python as the test machine names it.
pub fn python() -> &'static str {
    if cfg!(windows) { "python" } else { "python3" }
}

/// The agent command that runs a Python fixture (quoted for `sh -c` / `cmd /c`). UTF-8 mode makes it read the host's
/// UTF-8 the way real agents do: Windows Python reads pipes in the ANSI code page, turning a `”` into a lone surrogate.
pub fn python_agent(script: &Path) -> String {
    let path = script.display().to_string();
    if cfg!(windows) {
        format!("{} -X utf8 -u \"{path}\"", python())
    } else {
        format!("{} -X utf8 -u '{}'", python(), path.replace('\'', "'\\''"))
    }
}

/// What a client holds after the event stream's catch-up: `{"rev", "bots": [...], "entries": [...]}`,
/// the newest row per id. Reads until the stream goes quiet.
pub async fn snapshot(base: &str, token: &str) -> serde_json::Value {
    use futures::StreamExt as _;
    let res = reqwest::Client::new()
        .get(format!("{base}/events?since=0&client=test"))
        .bearer_auth(token)
        .send()
        .await
        .expect("open the event stream");
    let mut body = res.bytes_stream();
    let mut buf = String::new();
    let (mut rev, mut bots, mut entries) = (serde_json::Value::Null, Vec::<serde_json::Value>::new(), Vec::new());
    while let Ok(Some(Ok(chunk))) = tokio::time::timeout(Duration::from_millis(300), body.next()).await {
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(i) = buf.find('\n') {
            let line: String = buf.drain(..=i).collect();
            let Some(event) = line
                .trim_end()
                .strip_prefix("data:")
                .and_then(|data| serde_json::from_str::<serde_json::Value>(data.trim_start()).ok())
            else {
                continue;
            };
            let (rows, row) = match event["type"].as_str() {
                Some("hello") => {
                    rev = event["rev"].clone();
                    continue;
                }
                Some("bot") => (&mut bots, event["bot"].clone()),
                Some("entry") => (&mut entries, event["entry"].clone()),
                _ => continue,
            };
            rows.retain(|r| r["id"] != row["id"]);
            rows.push(row);
        }
    }
    serde_json::json!({"rev": rev, "bots": bots, "entries": entries})
}

/// A roster can already contain Donna; fixture assertions must address their own bot.
#[allow(dead_code)] // Cada binário de integração usa somente parte deste módulo.
pub fn snapshot_bot<'a>(snapshot: &'a serde_json::Value, id: &str) -> &'a serde_json::Value {
    snapshot["bots"].as_array().unwrap().iter().find(|bot| bot["id"] == id).expect("bot in snapshot")
}
