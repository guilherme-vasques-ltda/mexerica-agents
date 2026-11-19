//! D277: referências de senha são inertes nas rotas de login legado.
use anyhow::{Result, bail};
use serde_json::Value;

const RESERVED_MARKER: &str = "{{senha:";
const UNAVAILABLE: &str = "Esta referência de senha exige um caminho protegido ainda indisponível.";
pub(crate) const INSTRUCTIONS: &str = "- Referências de senha (D277): {{senha:...}} é sintaxe reservada e ainda inerte. request_login em site e type_login em login recusam qualquer entrada com a marca {{senha:, inclusive malformada, antes do cofre, pedido ou preenchimento. O caminho protegido e a coleta privada ainda estão indisponíveis. Não remova a marca nem tente outro meio de resolver, coletar ou enviar a senha.";

/// Examina a entrada bruta, sem normalizar, resolver ou repetir conteúdo no erro.
/// A marca já reserva o argumento, mesmo sem conteúdo ou fechamento válido.
pub(crate) fn reject_argument(args: &Value, field: &str) -> Result<()> {
    if args[field].as_str().is_some_and(|value| value.contains(RESERVED_MARKER)) {
        bail!(UNAVAILABLE);
    }
    Ok(())
}

/// O roteador tenta destravar o cofre antes de chamar as ferramentas legadas.
pub(crate) fn reject_api(method: &str, body: &Value) -> Result<()> {
    let field = match (method, body["name"].as_str()) {
        ("connectorCall", Some("request_login")) => "site",
        ("connectorCall", Some("request_login_code")) | ("computerCall", Some("type_login")) => "login",
        _ => return Ok(()),
    };
    reject_argument(&body["arguments"], field)
}

#[cfg(test)]
mod tests;
