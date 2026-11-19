//! M3-F: cadastro só é reutilizável no emissor que o Hub vinculou na emissão.
use crate::market::{Connector, oauth::OAuth};
use anyhow::{Result, anyhow};

pub(crate) const SEM_CADASTRO: &str = "Não foi possível vincular o cadastro ao serviço de login. Este serviço não oferece cadastro automático; conexão recusada. Use uma conexão com cabeçalho de token no app, sem enviar segredos no chat.";

pub(crate) fn reutilizavel(c: &Connector, issuer: &str, wanted: &[String]) -> bool {
    c.oauth_binding.as_ref().and_then(|binding| binding.registration_issuer()) == Some(issuer)
        && c.oauth.as_ref().is_some_and(|oauth| {
            !oauth.client_id.is_empty() && wanted.iter().all(|uri| oauth.redirect_uris.contains(uri))
        })
}

/// O cadastro e seu vínculo terminam juntos; o chamador fornece a guarda original.
pub(crate) fn apagar(c: &mut Connector) -> Result<()> {
    c.oauth = Some(OAuth::default());
    let binding = c.oauth_binding.as_mut().ok_or_else(|| anyhow!("Vínculo ausente"))?;
    binding.reconnect();
    binding.clear_registration();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fork::conectores::oauth_binding::Binding;
    use serde_json::json;

    #[test]
    fn reuso_exige_emissor_original_exato_e_cadastro_compativel() {
        let mut c: Connector = serde_json::from_value(json!({"id":"fixture","name":"fixture",
            "url":"https://fixture.invalid","oauth":{"clientId":"igual","redirectUris":["callback"]}}))
        .unwrap();
        c.oauth_binding = Some(Binding::new());
        let wanted = vec!["callback".into()];
        assert!(!reutilizavel(&c, "https://AS.invalid/path", &wanted));
        c.oauth_binding.as_mut().unwrap().register("https://AS.invalid/path");
        assert!(reutilizavel(&c, "https://AS.invalid/path", &wanted));
        for issuer in [
            "https://as.invalid/path",
            "https://AS.invalid/path/",
            "https://AS.invalid/Path",
            " https://AS.invalid/path",
        ] {
            assert!(!reutilizavel(&c, issuer, &wanted));
        }
        assert!(!reutilizavel(&c, "https://AS.invalid/path", &["other".into()]));
        c.oauth.as_mut().unwrap().client_id.clear();
        assert!(!reutilizavel(&c, "https://AS.invalid/path", &wanted));
    }
}
