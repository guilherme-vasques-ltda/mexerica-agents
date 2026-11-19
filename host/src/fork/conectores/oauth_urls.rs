//! M2-A: endereços do login MCP validados por estrutura e cliente HTTP que não segue
//! `Location`. Validação de nome/literal, não de destino: `localhost` é só o nome
//! declarado local, e um `https://` que resolve para rede interna continua alcançável.
use anyhow::{Result, anyhow, ensure};
use reqwest::Url;
use reqwest::redirect::Policy;
use serde_json::Value;
use std::net::{Ipv4Addr, Ipv6Addr};

pub(crate) const CONECTOR: &str =
    "Endereço do conector inválido. Use https, ou http só em localhost, 127.x.x.x ou [::1], sem usuário nem #.";
pub(crate) const DERIVADA: &str = "O servidor de login informou um endereço inválido; login recusado.";
pub(crate) const METADADOS: &str = "O servidor de login não informou como entrar.";
pub(crate) const EMISSOR: &str = "O servidor de login não confirmou a própria identidade; login recusado.";
pub(crate) const CADASTRO: &str = "O servidor de login recusou o cadastro do app ou redirecionou; login recusado.";
pub(crate) const PKCE: &str = "Este serviço não declara a proteção PKCE exigida (S256); login recusado.";

/// Cliente dos pedidos OAuth: nenhum pedido é reenviado ao destino de um redirecionamento.
pub(crate) fn cliente() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(Policy::none())
        .build()
        .map_err(|_| anyhow!("Não foi possível preparar a conexão de login."))
}

fn limpa(url: &Url) -> bool {
    url.username().is_empty() && url.password().is_none() && url.fragment().is_none()
}

/// Loopback por nome exato ou literal; `localhost.` ou `localhost.x` não contam.
fn loopback(url: &Url) -> bool {
    // O `url` já normaliza minúsculas e formas IPv4; IPv6 vem entre colchetes.
    let Some(host) = url.host_str() else { return false };
    if let Some(v6) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        return v6.parse::<Ipv6Addr>().is_ok_and(|ip| ip == Ipv6Addr::LOCALHOST);
    }
    host == "localhost" || host.parse::<Ipv4Addr>().is_ok_and(|ip| ip.is_loopback())
}

/// URL do conector dada pelo usuário: `https` com host, ou `http` só em loopback.
pub(crate) fn conector(raw: &str) -> Result<Url> {
    let url = Url::parse(raw).map_err(|_| anyhow!(CONECTOR))?;
    let esquema = match url.scheme() {
        "https" => url.host_str().is_some(),
        "http" => loopback(&url),
        _ => false,
    };
    ensure!(esquema && limpa(&url), CONECTOR);
    Ok(url)
}

/// Endereço vindo do servidor (metadados, issuer, authorize, token, registro). Outra
/// origem é legítima; `http` só quando conector e derivada são loopback. Query preservada.
pub(crate) fn derivada(conector: &Url, raw: &str) -> Result<Url> {
    let url = Url::parse(raw).map_err(|_| anyhow!(DERIVADA))?;
    let esquema = match url.scheme() {
        "https" => url.host_str().is_some(),
        "http" => loopback(conector) && loopback(&url),
        _ => false,
    };
    ensure!(esquema && limpa(&url), DERIVADA);
    Ok(url)
}

/// M2-B (RFC 8414 §3.3): `issuer` dos metadados é string idêntica ao emissor escolhido,
/// sem normalizar barra, maiúsculas, porta ou forma de URL.
pub(crate) fn mesmo_emissor(esperado: &str, metadados: &Value) -> bool {
    metadados.get("issuer").and_then(Value::as_str) == Some(esperado)
}

/// M3-A (MCP 2026-07-28, Security Considerations, Authorization Code Protection):
/// `code_challenge_methods_supported` é array só de strings e contém `S256` exato.
/// Ausente, outro tipo, vazio, item não-string ou só `plain` recusam; sem trim/casefold.
pub(crate) fn pkce_s256(metadados: &Value) -> bool {
    metadados.get("code_challenge_methods_supported").and_then(Value::as_array).is_some_and(|metodos| {
        metodos.iter().all(Value::is_string) && metodos.iter().any(|m| m.as_str() == Some("S256"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conector_aceita_https_e_loopback_por_nome_ou_literal() {
        for ok in [
            "https://x.exemplo/mcp",
            "http://127.0.0.1:8080/mcp",
            "http://127.1.2.3:9",
            "http://[::1]:9/mcp",
            "http://localhost:9",
            "http://LOCALHOST:9",
        ] {
            assert!(conector(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://localhost.exemplo.invalid/mcp",
            "http://127.0.0.1.exemplo.invalid/mcp",
            "http://localhost.:9",
            "http://10.0.0.2/mcp",
            "http://[::ffff:127.0.0.1]/mcp",
            "https://u:p@x.exemplo/mcp",
            "https://u@x.exemplo/mcp",
            "https://x.exemplo/mcp#frag",
            "https://x.exemplo/mcp#",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "https://",
        ] {
            assert_eq!(conector(bad).unwrap_err().to_string(), CONECTOR, "{bad}");
        }
    }

    #[test]
    fn derivada_aceita_outra_origem_e_recusa_http_fora_de_loopback() {
        let remoto = conector("https://mcp.exemplo/mcp").unwrap();
        let local = conector("http://127.0.0.1:9/mcp").unwrap();
        assert!(derivada(&remoto, "https://auth.outro.exemplo/token").is_ok());
        assert!(derivada(&local, "https://auth.outro.exemplo/token").is_ok());
        assert!(derivada(&local, "http://localhost:10/token").is_ok());
        assert!(derivada(&remoto, "http://127.0.0.1:10/token").is_err());
        assert!(derivada(&local, "http://auth.exemplo/token").is_err());
        assert!(derivada(&remoto, "https://u:p@auth.exemplo/token").is_err());
        assert!(derivada(&remoto, "https://auth.exemplo/authorize#x").is_err());
        assert!(derivada(&remoto, "javascript:x").is_err());
        let authorize = derivada(&remoto, "https://auth.exemplo/authorize?tenant=a&x=1").unwrap();
        assert_eq!(authorize.query(), Some("tenant=a&x=1"));
    }

    #[test]
    fn mesmo_emissor_compara_string_exata() {
        let com = |issuer: Value| serde_json::json!({"issuer": issuer});
        assert!(mesmo_emissor("https://auth.exemplo", &com("https://auth.exemplo".into())));
        assert!(mesmo_emissor("https://auth.exemplo/t/", &com("https://auth.exemplo/t/".into())));
        assert!(!mesmo_emissor("https://auth.exemplo", &serde_json::json!({})));
        for outro in [
            Value::Null,
            serde_json::json!(1),
            "https://outro.exemplo".into(),
            "https://auth.exemplo/".into(),
            "https://Auth.Exemplo".into(),
            "https://auth.exemplo:443".into(),
        ] {
            assert!(!mesmo_emissor("https://auth.exemplo", &com(outro.clone())), "{outro}");
        }
        assert!(!mesmo_emissor("https://auth.exemplo/t/", &com("https://auth.exemplo/t".into())));
    }

    #[test]
    fn pkce_exige_lista_de_strings_com_s256_exato() {
        let com = |metodos: Value| serde_json::json!({"code_challenge_methods_supported": metodos});
        assert!(pkce_s256(&com(serde_json::json!(["S256"]))));
        assert!(pkce_s256(&com(serde_json::json!(["plain", "S256"]))));
        assert!(!pkce_s256(&serde_json::json!({})));
        for recusa in [
            Value::Null,
            serde_json::json!("S256"),
            serde_json::json!({"S256": true}),
            serde_json::json!([]),
            serde_json::json!(["plain"]),
            serde_json::json!(["S256", 1]),
            serde_json::json!(["S256", null]),
            serde_json::json!(["s256"]),
            serde_json::json!([" S256"]),
            serde_json::json!(["S256 "]),
            serde_json::json!([["S256"]]),
        ] {
            assert!(!pkce_s256(&com(recusa.clone())), "{recusa}");
        }
    }
}
