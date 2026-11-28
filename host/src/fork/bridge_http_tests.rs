use super::*;
use axum::{Router, http::StatusCode, response::Redirect, routing::post};
use std::os::unix::fs::PermissionsExt as _;

#[tokio::test]
async fn private_bridge_uses_unix_and_does_not_follow_redirects() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().canonicalize()?;
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
    let path = root.join("s");
    assert!(BridgeHttp::unix(path.clone()).is_err());
    let (listener, _socket) = crate::fork::desktop_socket::SocketFile::bind(path.clone())?;
    let bridge = BridgeHttp { client: BridgeHttp::unix(path.clone())?, port: SOCKET_PORT };
    let app = Router::new()
        .route("/api/ok", post(|| async { StatusCode::NO_CONTENT }))
        .route("/api/redirect", post(|| async { Redirect::temporary("/api/ok") }));
    let task = tokio::spawn(async move { axum::serve(listener, app).await });
    let ok = bridge.post("ok").timeout(std::time::Duration::from_secs(3)).send().await;
    let redirect = bridge.post("redirect").timeout(std::time::Duration::from_secs(3)).send().await;
    task.abort();
    assert_eq!(ok?.status(), StatusCode::NO_CONTENT);
    assert_eq!(redirect?.status(), StatusCode::TEMPORARY_REDIRECT);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666))?;
    assert!(BridgeHttp::unix(path).is_err());
    Ok(())
}
