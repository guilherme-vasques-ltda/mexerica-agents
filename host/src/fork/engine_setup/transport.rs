//! Saída nativa é dado não confiável. Só uma apresentação allowlisted sai da operação.
use crate::fork::engine_accounts::Backend;
use anyhow::{Result, ensure};
use serde::Serialize;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Challenge {
    pub kind: &'static str,
    pub url: String,
    pub display_domain: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

pub(crate) fn challenge(backend: Backend, kind: &'static str, raw: &str, code: Option<&str>) -> Result<Challenge> {
    ensure!(raw.len() <= 8192 && !raw.chars().any(char::is_control) && !raw.contains('\\'), "auth_url_invalid");
    let url = reqwest::Url::parse(raw)?;
    let domain = url.host_str().unwrap_or_default();
    let allowed = match backend {
        Backend::Codex => domain == "auth.openai.com" && url.path() == "/codex/device" && url.query().is_none(),
        Backend::Claude => {
            matches!(domain, "claude.ai" | "platform.claude.com" | "console.anthropic.com")
                && matches!(url.path(), "/oauth/authorize" | "/oauth/authorize/")
        }
        Backend::Grok => {
            matches!(domain, "auth.x.ai" | "accounts.x.ai")
                && url.query_pairs().count() <= 1
                && url.query_pairs().all(|(key, value)| key == "user_code" && validate_code(&value).is_ok())
        }
    };
    ensure!(
        allowed
            && url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none()
            && url.fragment().is_none(),
        "auth_url_invalid"
    );
    if let Some(code) = code {
        validate_code(code)?;
        // O endereço que a tela abre nunca leva um código diferente do que ela mostra.
        ensure!(url.query_pairs().all(|(_, value)| value == code), "auth_url_invalid");
    }
    Ok(Challenge { kind, url: url.to_string(), display_domain: domain.into(), code: code.map(str::to_owned) })
}

pub(crate) fn validate_code(code: &str) -> Result<()> {
    ensure!(
        !code.is_empty()
            && code.len() <= 2048
            && code.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.~#".contains(&b)),
        "auth_code_invalid"
    );
    Ok(())
}

/// Parser incremental: OSC/CSI nunca viram links, clipboard ou entrada do terminal.
#[derive(Default)]
pub(crate) struct Presentation {
    text: String,
    escape: u8,
}
impl Presentation {
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Result<Option<Challenge>> {
        for &b in bytes {
            match self.escape {
                1 => {
                    self.escape = match b {
                        b']' => 2,
                        b'[' => 4,
                        _ => 0,
                    }
                }
                2 => {
                    if b == 7 {
                        self.escape = 0;
                    } else if b == 27 {
                        self.escape = 3;
                    }
                }
                3 => self.escape = if b == b'\\' { 0 } else { 2 },
                4 => {
                    if (0x40..=0x7e).contains(&b) {
                        self.escape = 0;
                    }
                }
                _ if b == 27 => self.escape = 1,
                _ if b.is_ascii_graphic() || b == b' ' || b == b'\n' || b == b'\r' || b == b'\t' => {
                    self.text.push(char::from(b));
                }
                _ => {}
            }
        }
        ensure!(self.text.len() <= 32 * 1024, "auth_output_limit");
        if let Some(start) = self.text.find("https://") {
            let suffix = &self.text[start..];
            if let Some(end) = suffix.find(char::is_whitespace) {
                let raw = &suffix[..end];
                return challenge(Backend::Claude, "paste_code", raw, None).map(Some);
            }
        }
        Ok(None)
    }
}
