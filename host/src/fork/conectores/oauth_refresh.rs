//! Renovação OAuth da Mexerica, sem nova autoridade nem registro de credenciais.
//! A classificação conservadora e as guardas/CAS são adaptações da Mexerica.
use super::catalogo_servicos::{Auth, RegistroOauth};
use super::{oauth_binding as binding, oauth_cimd, oauth_runtime::RuntimeIntent};
use crate::hub::Hub;
use crate::market::{
    self, Connector,
    oauth::{self, OAuth},
};
use anyhow::{Result, anyhow, ensure};
use reqwest::StatusCode;
use serde_json::Value;

const EARLY: u64 = 60;
const RETRY: &str = "Não foi possível renovar a conexão agora. Tente novamente em instantes.";
/// Servidores podem revogar o refresh anterior no primeiro uso.
static REFRESH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Renewal {
    Renewed,
    /// Inconclusivo preserva o cadastro, mas não comprova que um acesso pode ser usado.
    Retry {
        fallback: bool,
    },
    Revoked,
    Unregistered,
}

pub(crate) fn classify(status: StatusCode, v: &Value) -> Renewal {
    // Status temporário prevalece mesmo se um proxy devolver um erro OAuth definitivo.
    if status.is_server_error() || matches!(status.as_u16(), 408 | 429) {
        return Renewal::Retry { fallback: true };
    }
    if status == StatusCode::OK
        && v.get("error").is_none()
        && v["access_token"].as_str().is_some_and(|s| !s.trim().is_empty())
        // Ausente é opcional; presente inválido não pode misturar access novo e refresh velho.
        && v.get("refresh_token").is_none_or(|r| r.as_str().is_some_and(|s| !s.trim().is_empty()))
        && v.get("expires_in").is_none_or(|s| s.as_u64().and_then(|s| oauth::now().checked_add(s)).is_some())
    {
        return Renewal::Renewed;
    }
    match (status.as_u16(), v["error"].as_str()) {
        (400, Some("invalid_grant")) => Renewal::Revoked,
        (400 | 401, Some("invalid_client")) => Renewal::Unregistered,
        (400, Some("temporarily_unavailable")) => Renewal::Retry { fallback: true },
        _ => Renewal::Retry { fallback: false },
    }
}

/// Contrato normal permite acesso novo sem `expires_in`; isso nunca prova prazo de fallback.
pub(crate) fn usable(o: &OAuth) -> Result<()> {
    ensure!(!o.access_token.trim().is_empty() && o.expires_at.is_none_or(|t| t > oauth::now()), "{RETRY}");
    Ok(())
}

fn checked(hub: &Hub, c: &Connector, intent: &RuntimeIntent, refused: Option<&str>, fallback: bool) -> Result<OAuth> {
    binding::check_runtime(hub, c, intent, || {
        if let Some(binding) = &c.oauth_binding {
            binding.check_scopes()?;
        }
        let o = c.oauth.as_ref().ok_or_else(|| anyhow!("{RETRY}"))?;
        usable(o)?;
        ensure!(refused != Some(o.access_token.as_str()), "{RETRY}");
        if fallback {
            ensure!(refused.is_none() && o.expires_at.is_some_and(|t| t > oauth::now()), "{RETRY}");
        }
        Ok(o.clone())
    })
}

pub(crate) async fn fresh(hub: &Hub, c: &Connector, o: &OAuth, intent: &RuntimeIntent, stale: bool) -> Result<OAuth> {
    // A checagem inicial não exige o par antigo: uma rotação legítima da mesma fonte
    // ainda pode ser adotada após o lock de refresh, como no contrato existente.
    let active = intent.current(hub, c)?;
    if let Some(binding) = &active.oauth_binding {
        binding.check_scopes()?;
    }
    let signed_out = || anyhow!("Entre novamente no conector em Extensões.");
    if o.access_token.trim().is_empty() {
        return Err(signed_out());
    }
    let bound = c.oauth_binding.as_ref();
    let resource = bound.and_then(|binding| binding.resource());
    let resource = match market::contas::auth_da_conta(c).map_err(|_| anyhow!("{RETRY}"))? {
        Some(Auth::AppProprio { configuracao: Some(config), emissor, .. }) => {
            ensure!(
                resource == Some(config.recurso)
                    && bound.and_then(binding::Binding::registration_issuer) == Some(*emissor)
                    && o.token_endpoint == config.token,
                "{RETRY}"
            );
            resource
        }
        Some(Auth::AppProprio { provedor: "google", .. }) => None,
        Some(Auth::Oauth { registro: RegistroOauth::Cimd, client_id_metadata_document_url }) => {
            let url = oauth_cimd::url(*client_id_metadata_document_url)?;
            ensure!(
                resource.is_some()
                    && bound.and_then(binding::Binding::registration_issuer).is_none()
                    && o.client_id == url
                    && o.client_secret.is_none()
                    && !o.client_secret_basic,
                "Entre novamente no conector em Extensões."
            );
            resource
        }
        _ => resource,
    };
    if let Some(resource) = resource {
        ensure!(
            resource.chars().all(|c| !c.is_whitespace() && !c.is_control())
                && reqwest::Url::parse(resource).is_ok_and(|url| {
                    matches!(url.scheme(), "http" | "https") && url.host_str().is_some() && url.fragment().is_none()
                }),
            "{RETRY}"
        );
    }
    let refused = stale.then_some(o.access_token.as_str());
    let expiring = o.expires_at.is_some_and(|t| t <= oauth::now().saturating_add(EARLY));
    if !stale && !expiring {
        return checked(hub, c, intent, refused, false);
    }
    let _one = REFRESH.lock().await;
    // Identidade é Binding/config/RuntimeIntent; desigualdade do par só detecta rotação.
    let current = intent.current(hub, c)?;
    let current_oauth = current.oauth.as_ref().ok_or_else(signed_out)?;
    if current_oauth != o {
        // Um par concorrente não ignora expiração nem o 401 do MESMO access token.
        return checked(hub, &current, intent, refused, false);
    }
    binding::check_runtime(hub, c, intent, || Ok(()))?;
    let Some(refresh) = o.refresh_token.as_deref() else {
        oauth::sign_out(hub, c, intent)?;
        return Err(signed_out());
    };
    let mut form = vec![("grant_type", "refresh_token"), ("refresh_token", refresh)];
    // Legado sem grant conserva a omissão anterior; nunca inventar resource pela URL.
    if let Some(resource) = resource {
        form.push(("resource", resource));
    }
    let response = oauth::token_post(o, &form).await;
    binding::check_runtime(hub, c, intent, || Ok(()))?;
    let (renewal, answer) = match response {
        Ok((status, v)) => (classify(status, &v), v),
        Err(_) => (Renewal::Retry { fallback: true }, Value::Null),
    };
    match renewal {
        Renewal::Renewed => {
            let scope =
                c.oauth_binding.as_ref().ok_or_else(|| anyhow!("Vínculo ausente"))?.validate_refresh_scopes(&answer);
            if scope.is_err() {
                binding::change_runtime(hub, c, intent, |c| {
                    c.oauth_binding.as_mut().ok_or_else(|| anyhow!("Vínculo ausente"))?.retain_scopes()
                })?;
                return Err(anyhow!(super::oauth_token_scopes::ERRO));
            }
            let (saved, ()) = binding::change_runtime(hub, c, intent, |c| {
                oauth::store_tokens(c, &answer);
                Ok(())
            })?;
            // expires_in=0 persiste o par JUNTO para não perder refresh rotativo;
            // o acesso vencido não sai. Um OLD nunca vence o CAS de um par NEW.
            checked(hub, &saved, intent, refused, false)
        }
        Renewal::Retry { fallback: true } => checked(hub, c, intent, refused, true),
        Renewal::Retry { fallback: false } => {
            binding::check_runtime(hub, c, intent, || Ok(()))?;
            Err(anyhow!("{RETRY}"))
        }
        Renewal::Revoked => {
            oauth::sign_out(hub, c, intent)?;
            Err(signed_out())
        }
        Renewal::Unregistered => {
            binding::change_runtime(hub, c, intent, super::oauth_registration::apagar)?;
            Err(signed_out())
        }
    }
}

/// Payload e prazo nascem do MESMO OAuth que escreve o Bearer. Sem Serialize/Debug:
/// o testemunho é transitório e privado, nunca DTO do app ou nova autoridade.
pub(crate) struct Target {
    payload: Value,
    deadline: AccessDeadline,
    #[cfg(test)]
    clock: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
}

enum AccessDeadline {
    Manual,
    OAuth { expires_at: Option<u64> },
}

impl Target {
    pub(crate) fn new(
        hub: &Hub,
        url: &str,
        mut headers: std::collections::BTreeMap<String, String>,
        oauth: Option<&OAuth>,
    ) -> Result<Self> {
        #[cfg(not(test))]
        let _ = hub;
        let deadline = match oauth {
            Some(oauth) => {
                usable(oauth)?;
                headers.insert("Authorization".into(), format!("Bearer {}", oauth.access_token));
                AccessDeadline::OAuth { expires_at: oauth.expires_at }
            }
            None => AccessDeadline::Manual,
        };
        Ok(Self {
            payload: serde_json::json!({"url":url,"headers":headers}),
            deadline,
            #[cfg(test)]
            clock: oauth::upstream02_tests::delivery_clock(hub),
        })
    }

    /// Só leitura; o payload não pode trocar de Bearer após capturar seu prazo.
    pub(crate) fn payload(&self) -> &Value {
        &self.payload
    }

    /// Última ação local da entrega Hub, APÓS `check_runtime` inteiro. Sem locks/I/O.
    /// O registro canônico pode ter um par B; este prazo continua sendo do target A.
    pub(crate) fn validate_delivery(&self) -> Result<()> {
        if let AccessDeadline::OAuth { expires_at: Some(expires_at) } = self.deadline {
            #[cfg(test)]
            let now =
                self.clock.as_ref().map_or_else(oauth::now, |clock| clock.load(std::sync::atomic::Ordering::SeqCst));
            #[cfg(not(test))]
            let now = oauth::now();
            ensure!(expires_at > now, "{RETRY}");
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn into_test_value(self) -> Result<Value> {
        self.validate_delivery()?;
        Ok(self.payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn upstream03_refresh_field_is_optional_but_present_must_be_nonempty_string() {
        let read = |value| classify(StatusCode::OK, &value);
        assert_eq!(read(json!({"access_token":"new"})), Renewal::Renewed);
        assert_eq!(read(json!({"access_token":"new","refresh_token":"rotated"})), Renewal::Renewed);
        for refresh in [json!(""), json!(" \t "), Value::Null, json!(17), json!(false), json!([]), json!({})] {
            assert_eq!(
                read(json!({"access_token":"new","refresh_token":refresh,"expires_in":3600})),
                Renewal::Retry { fallback: false }
            );
        }
    }

    #[test]
    fn upstream03_classifies_only_confirmed_protocol_errors_as_destructive() {
        let read = |status, value| classify(StatusCode::from_u16(status).unwrap(), &value);
        assert_eq!(read(200, json!({"access_token":"new"})), Renewal::Renewed);
        assert_eq!(read(400, json!({"error":"invalid_grant"})), Renewal::Revoked);
        for status in [400, 401] {
            assert_eq!(read(status, json!({"error":"invalid_client"})), Renewal::Unregistered);
        }
        for status in [408, 429, 500, 502, 503] {
            assert_eq!(read(status, json!({"error":"invalid_grant"})), Renewal::Retry { fallback: true });
        }
        for (status, value) in [
            (200, json!({"access_token":"  "})),
            (200, json!({"access_token":"new","error":"invalid_grant"})),
            (200, json!({"access_token":"new","expires_in":"invalid"})),
            (200, json!({"error":"invalid_grant"})),
            (401, json!({"error":"invalid_grant"})),
            (400, json!({"error":"invalid_scope"})),
            (400, json!({"error":"invalid_target"})),
            (400, json!({"error":"unknown-arbitrary-value"})),
            (403, Value::Null),
        ] {
            assert_eq!(read(status, value), Renewal::Retry { fallback: false });
        }
    }
}
