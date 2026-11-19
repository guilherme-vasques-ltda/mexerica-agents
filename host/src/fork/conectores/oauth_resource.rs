//! M3-C: identidade PRM por candidato (RFC 9728 §§3, 3.1, 3.3 e 6).
use serde_json::Value;

pub(crate) const ERRO: &str = "O serviço não confirmou a identidade do recurso; login recusado.";
pub(crate) const ENDPOINT: &str = "O servidor de login informou um recurso adicional; login recusado.";
const WELL_KNOWN: &str = "/.well-known/oauth-protected-resource";

pub(crate) struct Candidato {
    pub url: String,
    esperado: String,
}

pub(crate) struct Validado {
    pub resource: String,
    pub documento: Value,
}

impl Candidato {
    pub fn conferir(&self, documento: Value) -> Option<Validado> {
        let resource = documento.get("resource")?.as_str()?;
        if resource.is_empty() || resource != self.esperado {
            return None;
        }
        Some(Validado { resource: resource.to_owned(), documento })
    }
}

/// `url` já passou por `oauth_urls::conector`. A identidade bruta não é normalizada.
pub(crate) fn candidatos(url: &str, hint: Option<String>) -> Vec<Candidato> {
    let mut candidatos = Vec::new();
    if let Some(hint) = hint {
        candidatos.push(Candidato { url: hint, esperado: url.to_owned() });
    }
    if let Some((scheme, rest)) = url.split_once("://") {
        let end = rest.find(['/', '?']).unwrap_or(rest.len());
        let origem = format!("{scheme}://{}", &rest[..end]);
        let suffix = &rest[end..];
        // Só a barra imediatamente após host, sem caminho semântico, sai do lookup.
        let suffix = if suffix == "/" || suffix.starts_with("/?") { &suffix[1..] } else { suffix };
        candidatos.push(Candidato { url: format!("{origem}{WELL_KNOWN}{suffix}"), esperado: url.to_owned() });
        candidatos.push(Candidato { url: format!("{origem}{WELL_KNOWN}"), esperado: origem });
    }
    candidatos
}

/// Query do endpoint não pode acrescentar outra audiência à escolhida pelo Hub.
/// `query_pairs` decodifica a chave uma vez, incluindo percent-encoding.
pub(crate) fn endpoint_sem_resource(url: &reqwest::Url) -> bool {
    !url.query_pairs().any(|(key, _)| key == "resource")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resource_exige_tipo_e_igualdade_exata_sem_normalizacao() {
        let esperado = "https://Mcp.exemplo/mcp/é?x=1";
        let c = Candidato { url: String::new(), esperado: esperado.into() };
        assert!(c.conferir(json!({"resource": esperado})).is_some());
        assert!(c.conferir(json!({})).is_none());
        for value in [
            Value::Null,
            json!(1),
            json!(true),
            json!([]),
            json!({}),
            json!(""),
            json!(format!(" {esperado}")),
            json!(format!("{esperado} ")),
            json!("https://mcp.exemplo/mcp/é?x=1"),
            json!("https://Mcp.exemplo/mcp/e\u{301}?x=1"),
            json!("https://Mcp.exemplo/mcp/é/?x=1"),
            json!("https://Mcp.exemplo/mcp/é?x=2"),
        ] {
            assert!(c.conferir(json!({"resource": value})).is_none());
        }
    }

    #[test]
    fn endpoint_recusa_resource_inclusive_chave_codificada_ou_duplicada() {
        for query in ["resource=outro", "%72esource=outro", "resour%63e=outro", "resource", "resource=a&resource=b"] {
            let url = reqwest::Url::parse(&format!("https://as.exemplo/endpoint?{query}")).unwrap();
            assert!(!endpoint_sem_resource(&url), "{query}");
        }
        let url = reqwest::Url::parse("https://as.exemplo/endpoint?tenant=a&x=1&next=resource%3Da").unwrap();
        assert!(endpoint_sem_resource(&url));
        assert_eq!(url.query(), Some("tenant=a&x=1&next=resource%3Da"));
    }

    #[test]
    fn cada_lookup_conserva_identidade_path_query_barra_e_hint() {
        for (resource, suffix) in [
            ("https://mcp.exemplo/mcp/?a=1", "/mcp/?a=1"),
            ("https://mcp.exemplo?x=1", "?x=1"),
            ("https://mcp.exemplo/?x=1", "?x=1"),
            ("https://mcp.exemplo/", ""),
            ("https://mcp.exemplo", ""),
        ] {
            let cs = candidatos(resource, Some("https://meta.exemplo/prm".into()));
            assert_eq!(cs[0].url, "https://meta.exemplo/prm");
            assert_eq!(cs[0].esperado, resource);
            assert_eq!(cs[1].url, format!("https://mcp.exemplo{WELL_KNOWN}{suffix}"));
            assert_eq!(cs[1].esperado, resource);
            assert_eq!(cs[2].url, format!("https://mcp.exemplo{WELL_KNOWN}"));
            assert_eq!(cs[2].esperado, "https://mcp.exemplo");
            assert!(cs[1].conferir(json!({"resource":resource})).is_some());
            assert!(cs[2].conferir(json!({"resource":"https://mcp.exemplo/mcp"})).is_none());
        }
    }
}
