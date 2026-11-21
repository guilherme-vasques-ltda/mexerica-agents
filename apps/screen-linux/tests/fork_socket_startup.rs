//! Prova isolada da entrada real: configuração ausente falha antes de GST/portal.
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn missing_socket_ignores_available_home_and_codync_fixture() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("relógio")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("startup-fixture-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&root).expect("criar raiz da fixture");
    let upstream = root.join(".codync");
    std::fs::create_dir(&upstream).expect("criar .codync da fixture");
    let listener = std::os::unix::net::UnixListener::bind(upstream.join("screen.sock"))
        .expect("bind do socket da fixture");
    listener.set_nonblocking(true).expect("modo não bloqueante");
    let mut child = Command::new(env!("CARGO_BIN_EXE_codync-screen"))
        .env("HOME", &root)
        .env("CODYNC_HOME", &upstream)
        .env("DONNABOT_HOME", &upstream)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("subir o helper");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().expect("consultar o helper").is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().expect("matar o helper");
            let _ = child.wait();
            let _ = std::fs::remove_dir_all(&root);
            panic!("helper não recusou a ausência de --socket");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().expect("esperar o helper");
    let refused = matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock);
    let unchanged = !upstream.join("screen-restore-token").exists();
    drop(listener);
    let _ = std::fs::remove_dir_all(&root);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("obrigatório --socket"));
    assert!(refused);
    assert!(unchanged);
}
