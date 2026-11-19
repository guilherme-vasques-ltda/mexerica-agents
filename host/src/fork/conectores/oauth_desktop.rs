//! Login de conector com o Hub em OUTRO computador (túnel SSH do app desktop).
//!
//! O retorno do navegador cai no Mac, não no servidor: um `redirect_uri` com a porta do Hub (`127.0.0.1:19422`)
//! daria `ERR_CONNECTION_REFUSED`, porque o túnel usa uma porta local aleatória. Então o app desktop abre, só
//! enquanto o login está em andamento, um ouvinte em `127.0.0.1:<porta fixa>` (`apps/desktop/src/main/fork-oauth-loopback.ts`)
//! e repassa o retorno ao Hub pelo canal já autenticado (`connectorSignInFinish`), como faz o app do celular.
//!
//! Fronteiras: a porta vem de uma lista fechada (as duas portas do app, uma por ambiente), nunca de texto livre;
//! só o chamador local (token do app) pode pedi-la; o `state`, o PKCE e o `iss` valem como em qualquer retorno, e o
//! `state` só conclui com o mesmo chamador que iniciou (`Origin`), nunca pela página `GET /oauth/callback` do Hub.
use anyhow::{Result, bail};
use serde_json::Value;

/// Portas de retorno do app desktop: principal e dev. A mesma lista vive em `apps/desktop/src/main/fork-oauth-loopback.ts`.
pub(crate) const PORTAS: [u16; 2] = [19424, 19425];

/// Recusa fixa: sem ecoar o valor recebido.
pub(crate) const RECUSA: &str = "A porta de retorno do login não é uma das portas do app; login recusado.";

/// `loopbackPort` do `connectorSignIn`: ausente ou `null` é ausência (retorno pelo próprio Hub); número fora da lista
/// ou de outro tipo recusa.
pub(crate) fn do_pedido(b: &Value) -> Result<Option<u16>> {
    match b.get("loopbackPort") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_u64().and_then(|n| u16::try_from(n).ok()) {
            Some(porta) if PORTAS.contains(&porta) => Ok(Some(porta)),
            _ => bail!(RECUSA),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn so_as_portas_do_app_passam_e_ausencia_e_ausencia() {
        assert_eq!(do_pedido(&json!({})).unwrap(), None);
        assert_eq!(do_pedido(&json!({"loopbackPort": null})).unwrap(), None);
        assert_eq!(do_pedido(&json!({"loopbackPort": 19424})).unwrap(), Some(19424));
        assert_eq!(do_pedido(&json!({"loopbackPort": 19425})).unwrap(), Some(19425));
        for ruim in [
            json!(0),
            json!(19422),
            json!(80),
            json!(65536),
            json!(-1),
            json!(19424.5),
            json!("19424"),
            json!(true),
            json!([19424]),
        ] {
            let erro = do_pedido(&json!({"loopbackPort": ruim})).unwrap_err().to_string();
            assert_eq!(erro, RECUSA, "{ruim}");
        }
    }
}
