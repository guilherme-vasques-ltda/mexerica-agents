//! Entrada real da CLI, sem Hub/SSH/cofre: pastas sintéticas e ponte stdio.
#![cfg(unix)]
use std::{
    ffi::{OsStr, OsString},
    fs::{DirBuilder, OpenOptions, Permissions},
    os::unix::{
        ffi::OsStringExt,
        fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const SETUP: &str = "DONNABOT_MCP_SETUP_LOCAL";
const CREDENTIAL: &str = "DONNABOT_PONTE_TOKEN";
const REFUSED_ROOT: &str = "pasta legada/upstream recusada";

struct Fixture {
    root: PathBuf,
    data: PathBuf,
    profile: PathBuf,
}
fn private_dir(path: &Path) {
    DirBuilder::new().mode(0o700).create(path).expect("diretório sintético exclusivo");
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)
        .expect("descritor do diretório sintético");
    directory.set_permissions(Permissions::from_mode(0o700)).expect("modo privado antes do conteúdo");
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("d520-mcp-scope-{}", uuid::Uuid::new_v4()));
        private_dir(&root);
        let data = root.join("data");
        private_dir(&data);
        let profile = data.join("managed-profile");
        private_dir(&profile);
        // Poison pill: qualquer tentativa de ler token local recebe diretório, não segredo.
        private_dir(&data.join("token"));
        Self { root, data, profile }
    }
    fn command(&self, args: &[&str], setup: Option<&OsStr>) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_donnabot-host"));
        command.env_clear().env("HOME", &self.profile).env("MEXERICA_HOME", &self.data).args(args);
        if let Some(value) = setup {
            command.env(SETUP, value);
        }
        command
    }
    fn unchanged(&self) {
        assert!(self.data.join("token").is_dir());
        for name in ["codync.db", "host.lock", "identity.json"] {
            assert!(!self.data.join(name).exists(), "a CLI não deve criar dados locais");
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn d520_mcp_without_setup_uses_bridge_stdio_without_reading_local_token() {
    let fixture = Fixture::new();
    let mut command =
        tokio::process::Command::from(fixture.command(&["mcp", "memory", "--bot", "fixture", "--port", "1"], None));
    let mut child = command
        .env(CREDENTIAL, "ponte_sintetica_sem_hub")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("ponte stdio sintética");
    let mut input = child.stdin.take().expect("stdin da ponte sintética");
    let mut lines = BufReader::new(child.stdout.take().expect("stdout da ponte sintética")).lines();
    input.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n").await.expect("ping da ponte sintética");
    input.flush().await.expect("envio do ping");
    let line = lines.next_line().await.expect("leitura da ponte").expect("resposta ao ping, sem EOF prematuro");
    let reply: serde_json::Value = serde_json::from_str(&line).expect("resposta JSON-RPC");
    assert_eq!(reply["id"], 1);
    assert_eq!(reply["result"], serde_json::json!({}));
    assert!(reply.get("error").is_none());
    drop(input);
    assert!(child.wait().await.expect("encerramento da ponte sintética").success());
    fixture.unchanged();
}

#[test]
fn d520_mcp_without_bridge_credential_refuses_without_local_fallback() {
    let fixture = Fixture::new();
    for token in [None, Some(""), Some(" ")] {
        let mut command = fixture.command(&["mcp", "memory", "--bot", "fixture", "--port", "1"], None);
        if let Some(token) = token {
            command.env(CREDENTIAL, token);
        }
        let output = command.output().expect("subprocesso sem credencial da ponte");
        assert!(!output.status.success());
        assert_eq!(output.stdout, [] as [u8; 0]);
        assert!(String::from_utf8_lossy(&output.stderr).contains("Credencial da ponte ausente"));
        fixture.unchanged();
    }
}

#[test]
fn d520_r9_setup_presence_including_empty_and_non_utf8_keeps_preflight_before_token() {
    let fixture = Fixture::new();
    for setup in [OsString::from("1"), OsString::new(), OsString::from_vec(vec![0xff])] {
        let output = fixture
            .command(&["mcp", "remote", "--connector", "fixture", "--context", "fixture", "--port", "1"], Some(&setup))
            .env(CREDENTIAL, "ponte_sintetica_sem_hub")
            .output()
            .expect("subprocesso R9 sintético");
        assert!(!output.status.success());
        assert_eq!(output.stdout, [] as [u8; 0]);
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(REFUSED_ROOT),
            "guarda da raiz deve anteceder a leitura do poison token"
        );
        fixture.unchanged();
    }
}

#[test]
fn d520_local_readers_info_serve_and_default_keep_refusing_selected_ancestor() {
    let fixture = Fixture::new();
    for args in [vec!["info", "--json"], vec!["serve"], vec![]] {
        let output = fixture
            .command(&args, None)
            .env(CREDENTIAL, "ponte_sintetica_sem_hub")
            .output()
            .expect("leitor local em raiz sintética recusada");
        assert!(!output.status.success());
        assert_eq!(output.stdout, [] as [u8; 0]);
        assert!(String::from_utf8_lossy(&output.stderr).contains(REFUSED_ROOT));
        fixture.unchanged();
    }
}
