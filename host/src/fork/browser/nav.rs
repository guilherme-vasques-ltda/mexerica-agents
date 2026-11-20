//! Gate de navegação: toda URL é validada no Hub antes de qualquer
//! `Page.navigate`/`Target.createTarget`. Fail-closed: esquema não-https,
//! destino não público ou DNS misto recusam antes de tocar o CDP.
#![allow(dead_code)]

use anyhow::{Result, bail, ensure};
use std::time::Duration;

pub(crate) const MAX_URL: usize = 2048;
pub(super) const NAV_TIMEOUT: Duration = Duration::from_secs(30);

/// Só `parse_navigation_url` constrói: o campo privado impede criar um destino sem validação.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ValidatedNavigation {
    pub url: String,
    pub host: String,
    pub port: u16,
    seal: (),
}

/// Só sintaxe e esquema; não resolve DNS. `http` é recusado porque o proxy
/// da v1 só aceita CONNECT na 443 (ver `net_proxy`).
pub(crate) fn parse_navigation_url(raw: &str) -> Result<ValidatedNavigation> {
    ensure!(!raw.is_empty() && raw.len() <= MAX_URL, "browser_navigation_url_invalid");
    ensure!(!raw.chars().any(|c| c.is_control() || c.is_whitespace()), "browser_navigation_url_invalid");
    let url = reqwest::Url::parse(raw).map_err(|_| anyhow::anyhow!("browser_navigation_url_invalid"))?;
    match url.scheme() {
        "https" => {}
        "http" => bail!("browser_navigation_http_denied: somente HTTPS na v1"),
        _ => bail!("browser_navigation_scheme_denied"),
    }
    ensure!(url.username().is_empty() && url.password().is_none(), "browser_navigation_url_invalid");
    // Equivalente a `url.host()` (Host::Ipv6): o `host_str()` devolve o
    // IPv6 com colchetes, que não parseia como IpAddr e cairia no DNS.
    // Sem os colchetes, o literal cai na checagem de IP logo abaixo.
    let raw_host = url.host_str().ok_or_else(|| anyhow::anyhow!("browser_navigation_url_invalid"))?;
    let host = raw_host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(raw_host).to_owned();
    ensure!(
        !host.is_empty()
            && !host.eq_ignore_ascii_case("localhost")
            && !host.to_ascii_lowercase().ends_with(".localhost"),
        "browser_navigation_address_denied"
    );
    let port = url.port().unwrap_or(443);
    ensure!(port == 443, "browser_navigation_port_denied");
    Ok(ValidatedNavigation { url: url.to_string(), host, port, seal: () })
}

/// Sintaxe + resolução única com a política de rede (#832): resposta DNS
/// mista ou endereço não público recusa a navegação inteira.
pub(crate) async fn validate_navigation_url(raw: &str) -> Result<ValidatedNavigation> {
    let parsed = parse_navigation_url(raw)?;
    if let Ok(ip) = parsed.host.parse::<std::net::IpAddr>() {
        super::network::validate_addresses(&[std::net::SocketAddr::new(ip, parsed.port)], parsed.port)
            .map_err(|_| anyhow::anyhow!("browser_navigation_address_denied"))?;
        return Ok(parsed);
    }
    super::network::resolve(&parsed.host, parsed.port)
        .await
        .map_err(|_| anyhow::anyhow!("browser_navigation_address_denied"))?;
    Ok(parsed)
}

impl<W: tokio::io::AsyncWrite + Unpin + Send + 'static> super::cdp::Cdp<W> {
    /// Único caminho de `Page.navigate`: valida no Hub e confere a autoridade da instância depois de toda espera assíncrona, imediatamente antes de enviar.
    pub(crate) async fn navigate_validated(
        &self,
        session: &str,
        raw_url: &str,
        authorized: impl Fn() -> bool,
    ) -> Result<String> {
        let validated = validate_navigation_url(raw_url).await?;
        ensure!(authorized(), "browser_authority_revoked: a autoridade da sessão foi revogada");
        let result = self.navigate(session, &validated).await?;
        if let Some(error) = result.get("errorText").and_then(|v| v.as_str()).filter(|t| !t.is_empty()) {
            bail!("browser_navigation_failed: {error}");
        }
        Ok(validated.url)
    }

    /// Único caminho de `Target.createTarget`: mesma validação da navegação.
    pub(crate) async fn create_target_validated(&self, raw_url: &str) -> Result<String> {
        let validated = validate_navigation_url(raw_url).await?;
        let result = self.create_target(&validated).await?;
        result
            .get("targetId")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("browser_navigation_target_missing"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aceita_https_publica() {
        let v = parse_navigation_url("https://example.com/pagina?x=1").unwrap();
        assert_eq!((v.host.as_str(), v.port), ("example.com", 443));
    }

    #[test]
    fn recusa_esquemas_e_http() {
        for url in [
            "file:///etc/hostname",
            "file:///etc/passwd",
            "chrome://settings",
            "chrome-extension://abc/popup.html",
            "devtools://devtools/bundled/inspector.html",
            "view-source:https://example.com/",
            "data:text/html,ola",
            "blob:https://example.com/1",
            "ftp://example.com/arquivo",
            "about:blank",
            "javascript:alert(1)",
            "http://example.com/",
        ] {
            assert!(parse_navigation_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn recusa_credencial_porta_e_localhost() {
        for url in [
            "https://user:senha@example.com/",
            "https://example.com:8443/",
            "https://localhost/",
            "https://app.localhost/",
            "https://exa mple.com/",
            "",
        ] {
            assert!(parse_navigation_url(url).is_err(), "{url}");
        }
    }

    #[tokio::test]
    async fn recusa_ip_nao_publico_sem_dns() {
        for url in [
            "https://127.0.0.1/",
            "https://10.0.0.5/admin", // vazamento-ok: IP privado para teste de recusa
            "https://192.168.1.1/",   // vazamento-ok: IP privado para teste de recusa
            "https://169.254.169.254/latest/meta-data/", // vazamento-ok: IP reservado para teste de recusa
            "https://[::1]/",
            "https://[::ffff:127.0.0.1]/",
        ] {
            assert!(validate_navigation_url(url).await.is_err(), "{url}");
        }
        // IP público literal não precisa de DNS.
        assert!(validate_navigation_url("https://8.8.8.8/").await.is_ok());
        // IPv6 público literal: sem colchetes no host, validado como IP.
        let v6 = parse_navigation_url("https://[2606:4700:4700::1111]/").unwrap();
        assert_eq!(v6.host, "2606:4700:4700::1111");
        assert!(validate_navigation_url("https://[2606:4700:4700::1111]/").await.is_ok());
    }
}
