use serde_json::{Value, json};

pub(super) fn sanitizar(frame: &mut Value) {
    let Some(error) = frame.get_mut("err") else { return };
    let status =
        error["status"].as_u64().filter(|&n| matches!(n, 400 | 401 | 403 | 404 | 409 | 429 | 500 | 503)).unwrap_or(500);
    let code = match (status, error["code"].as_str()) {
        (401, Some("auth_invalid")) => "auth_invalid",
        (401, Some("device_unpaired")) => "device_unpaired",
        (403, Some("workspace_forbidden")) => "workspace_forbidden",
        (403, Some("role_insufficient")) => "role_insufficient",
        (409, Some("workspace_epoch_mismatch")) => "workspace_epoch_mismatch",
        (401 | 403, _) => "access_denied",
        (404, _) => "not_found",
        (409, _) => "conflict",
        (429, _) => "busy",
        _ => "request_failed",
    };
    *error = json!({"status": status, "code": code, "message": code});
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descarta_mensagem_codigo_desconhecido_e_metadados() {
        let mut frame =
            json!({"id":1,"err":{"status":400,"message":"privado","code":"privado","data":{"detail":"privado"}}});
        sanitizar(&mut frame);
        assert_eq!(frame, json!({"id":1,"err":{"status":400,"message":"request_failed","code":"request_failed"}}));
    }
}
