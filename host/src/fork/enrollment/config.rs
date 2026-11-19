//! Qual nuvem este Hub usa: só a variável `MEXERICA_CLOUD_URL` (ou a antiga `DONNABOT_CLOUD_URL`, aceita por 2 releases) do serviço, definida pelo operador
//! (unidade systemd do usuário `donnabot` ou provisionador). Nunca vem de cliente, kv, request ou
//! build do upstream. Ausente ou recusada, a nuvem fica desligada como antes.

use reqwest::Url;
use std::sync::OnceLock;

/// Variável do serviço com a origem da nuvem, por exemplo `https://api.exemplo.app`.
pub(crate) const ENV: &str = "MEXERICA_CLOUD_URL";
/// Nome antigo (issue #915): continua valendo, com aviso de depreciação, por 2 releases.
pub(crate) const LEGACY_ENV: &str = "DONNABOT_CLOUD_URL";

/// Nome novo primeiro (vazio conta como ausente); senão o antigo. O `bool` diz que o antigo foi usado.
pub(crate) fn raw_from(get: impl Fn(&str) -> Option<String>) -> Option<(String, bool)> {
    if let Some(v) = get(ENV).filter(|v| !v.is_empty()) {
        return Some((v, false));
    }
    get(LEGACY_ENV).filter(|v| !v.is_empty()).map(|v| (v, true))
}

const MAX_LEN: usize = 255;

/// Por que um valor configurado foi recusado. As mensagens não repetem o valor (pode ir a log).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refused {
    Malformed,
    TooLong,
    /// Só `https`, ou `http` para um IP de loopback literal (testes locais).
    Scheme,
    /// Usuário/senha, caminho, query ou fragmento: a configuração é só uma origem.
    NotAnOrigin,
    /// Host com ponto final, sem nome de domínio ou fora da forma canônica.
    Host,
    /// Nuvem do upstream (`codync.dev`): o fork nunca fala com ela.
    Upstream,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Malformed => "endereço da nuvem malformado",
            Self::TooLong => "endereço da nuvem longo demais",
            Self::Scheme => "a nuvem precisa de https (http só em loopback literal)",
            Self::NotAnOrigin => "a nuvem deve ser só uma origem, sem credenciais, caminho, query ou fragmento",
            Self::Host => "host da nuvem fora da forma canônica",
            Self::Upstream => "nuvem do upstream recusada",
        })
    }
}

/// A origem configurada, validada uma vez por processo. `None`: nuvem desligada.
pub(crate) fn cloud_url() -> Option<String> {
    static CONFIGURED: OnceLock<Option<String>> = OnceLock::new();
    CONFIGURED
        .get_or_init(|| {
            let (raw, legacy) = raw_from(|k| std::env::var(k).ok())?;
            let name = if legacy { LEGACY_ENV } else { ENV };
            if legacy {
                tracing::warn!("{LEGACY_ENV} está obsoleta e sai em 2 releases; use {ENV}");
            }
            // A mesma validação para as duas; a nova inválida NÃO cai na antiga.
            match parse(&raw) {
                Ok(origin) => Some(origin),
                Err(e) => {
                    tracing::warn!(error = %e, "{name} recusada; a nuvem fica desligada");
                    None
                }
            }
        })
        .clone()
}

/// Valida e devolve a origem canônica (`esquema://host[:porta]`, sem barra final).
pub(crate) fn parse(raw: &str) -> Result<String, Refused> {
    if raw.len() > MAX_LEN {
        return Err(Refused::TooLong);
    }
    // Nada de espaços, controles ou barras invertidas que o parser normalizaria em silêncio.
    if raw.is_empty() || raw.chars().any(|c| c.is_whitespace() || c.is_control() || c == '\\') {
        return Err(Refused::Malformed);
    }
    let url = Url::parse(raw).map_err(|_| Refused::Malformed)?;
    let rest = raw.split_once("://").map(|(_, rest)| rest).ok_or(Refused::Malformed)?;
    // Só a autoridade, com no máximo uma barra final; o texto cru também é conferido.
    let authority = rest.strip_suffix('/').unwrap_or(rest);
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
        || authority.contains(['/', '?', '#', '@'])
    {
        return Err(Refused::NotAnOrigin);
    }
    let host = url.host_str().ok_or(Refused::Host)?;
    let origin = match url.scheme() {
        "https" => {
            let domain = url.domain().ok_or(Refused::Host)?;
            if domain.ends_with('.') || !domain.contains('.') || domain.split('.').any(str::is_empty) {
                return Err(Refused::Host);
            }
            let canonical = domain.to_ascii_lowercase();
            if canonical == "codync.dev" || canonical.ends_with(".codync.dev") {
                return Err(Refused::Upstream);
            }
            canonical
        }
        "http" if matches!(host, "127.0.0.1" | "[::1]") => host.to_owned(),
        _ => return Err(Refused::Scheme),
    };
    Ok(match url.port() {
        Some(port) => format!("{}://{origin}:{port}", url.scheme()),
        None => format!("{}://{origin}", url.scheme()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_name_wins_and_old_name_still_works() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
        };
        // instalação antiga: só DONNABOT_CLOUD_URL no serviço
        assert_eq!(
            raw_from(env(&[("DONNABOT_CLOUD_URL", "https://velho.exemplo.app")])),
            Some(("https://velho.exemplo.app".into(), true))
        );
        assert_eq!(
            raw_from(env(&[
                ("MEXERICA_CLOUD_URL", "https://novo.exemplo.app"),
                ("DONNABOT_CLOUD_URL", "https://velho.exemplo.app")
            ])),
            Some(("https://novo.exemplo.app".into(), false))
        );
        assert_eq!(
            raw_from(env(&[("MEXERICA_CLOUD_URL", ""), ("DONNABOT_CLOUD_URL", "https://velho.exemplo.app")])),
            Some(("https://velho.exemplo.app".into(), true))
        );
        assert_eq!(raw_from(env(&[])), None);
        // valor INVÁLIDO na nova não cai na antiga: a nova é escolhida e a validação recusa
        let (raw, legacy) = raw_from(env(&[
            ("MEXERICA_CLOUD_URL", "http://nao-loopback.exemplo.app"),
            ("DONNABOT_CLOUD_URL", "https://velho.exemplo.app"),
        ]))
        .expect("nova escolhida");
        assert!(!legacy);
        assert_eq!(parse(&raw), Err(Refused::Scheme));
        // a antiga passa pela mesma validação (https, origem, sem caminho)
        assert_eq!(parse("https://velho.exemplo.app").as_deref(), Ok("https://velho.exemplo.app"));
        assert_eq!(parse("http://velho.exemplo.app"), Err(Refused::Scheme));
        assert_eq!(parse("https://velho.exemplo.app/x"), Err(Refused::NotAnOrigin));
    }

    #[test]
    fn accepts_only_a_strict_origin() {
        assert_eq!(parse("https://api.exemplo.app"), Ok("https://api.exemplo.app".into()));
        assert_eq!(parse("https://API.Exemplo.APP/"), Ok("https://api.exemplo.app".into()), "case canônico");
        assert_eq!(parse("https://api.exemplo.app:443"), Ok("https://api.exemplo.app".into()), "porta padrão some");
        assert_eq!(parse("https://api.exemplo.app:8443"), Ok("https://api.exemplo.app:8443".into()));
        assert_eq!(parse("http://127.0.0.1:8787"), Ok("http://127.0.0.1:8787".into()));
        assert_eq!(parse("http://[::1]:8787/"), Ok("http://[::1]:8787".into()));
    }

    #[test]
    fn refuses_anything_that_is_not_an_operator_origin() {
        for (raw, why) in [
            ("http://api.exemplo.app", Refused::Scheme),
            ("http://localhost:8787", Refused::Scheme),
            ("http://10.0.0.2:8787", Refused::Scheme),
            ("ws://api.exemplo.app", Refused::Scheme),
            ("ftp://api.exemplo.app", Refused::Scheme),
            ("https://user:pw@api.exemplo.app", Refused::NotAnOrigin),
            ("https://api.exemplo.app/v1", Refused::NotAnOrigin),
            ("https://api.exemplo.app/?x=1", Refused::NotAnOrigin),
            ("https://api.exemplo.app/#f", Refused::NotAnOrigin),
            ("https://api.exemplo.app//", Refused::NotAnOrigin),
            ("https://api.exemplo.app.", Refused::Host),
            ("https://localhost", Refused::Host),
            ("https://8.8.8.8", Refused::Host),
            ("https://api.codync.dev", Refused::Upstream),
            ("https://Dev-API.CODYNC.dev/", Refused::Upstream),
            ("https://codync.dev", Refused::Upstream),
            (" https://api.exemplo.app", Refused::Malformed),
            ("https://api.exemplo.app\n", Refused::Malformed),
            ("https:\\\\api.exemplo.app", Refused::Malformed),
            ("api.exemplo.app", Refused::Malformed),
            ("", Refused::Malformed),
        ] {
            assert_eq!(parse(raw), Err(why), "{raw:?}");
        }
        assert_eq!(parse(&format!("https://{}.app", "a".repeat(300))), Err(Refused::TooLong));
    }

    #[test]
    fn refusals_never_repeat_the_value() {
        let secret = "https://user:segredo@api.exemplo.app";
        let e = parse(secret).unwrap_err();
        assert!(!e.to_string().contains("segredo"));
    }
}
