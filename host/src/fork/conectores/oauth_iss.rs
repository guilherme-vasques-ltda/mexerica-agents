//! M3-B (MCP 2026-07-28, Authorization Response Validation; RFC 9207 §2.4): o retorno do
//! login pertence ao servidor de autorização escolhido naquela tentativa. Emissor e política
//! são capturados no início, dos metadados já validados (M2-B), e nunca recapturados no retorno.
use anyhow::{Result, bail, ensure};
use serde_json::Value;

/// Recusa fixa: sem emissor, URL, `error`/`error_description`/`error_uri` ou corpo externo.
pub(crate) const RETORNO: &str =
    "O retorno do login não confirmou o serviço esperado; login recusado. Inicie o login novamente.";
/// Anúncio presente com tipo diferente de booleano; nunca presumido `false`.
pub(crate) const ANUNCIO: &str =
    "O servidor de login declarou de forma inválida se confirma a própria identidade no retorno; login recusado.";
/// Campo `iss` do pedido do app com tipo diferente de texto.
pub(crate) const FORMA: &str = "O retorno do login veio malformado; login recusado. Inicie o login novamente.";

const CAMPO: &str = "authorization_response_iss_parameter_supported";

/// Emissor original e política da mesma tentativa, guardados no `Pending`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Esperado {
    emissor: String,
    exige: bool,
}

impl Esperado {
    /// `true` exige `iss`; `false` ou ausente aceitam ausência; outro tipo (inclusive `null`) recusa.
    pub(crate) fn capturar(emissor: &str, metadados: &Value) -> Result<Self> {
        let exige = match metadados.get(CAMPO) {
            None => false,
            Some(Value::Bool(b)) => *b,
            Some(_) => bail!(ANUNCIO),
        };
        Ok(Self { emissor: emissor.to_owned(), exige })
    }

    /// `iss` presente é sempre comparado como string exata, mesmo sem anúncio e em retorno de
    /// erro: sem aparar, caixa, porta, barra ou decodificação adicional.
    pub(crate) fn conferir(&self, iss: Option<&str>) -> Result<()> {
        let aceito = match iss {
            Some(iss) => iss == self.emissor,
            None => !self.exige,
        };
        ensure!(aceito, RETORNO);
        Ok(())
    }
}

/// `iss` do `connectorSignInFinish`: ausente ou `null` é ausência; texto segue intacto; outro tipo recusa.
pub(crate) fn do_pedido(b: &Value) -> Result<Option<&str>> {
    match b.get("iss") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(_) => bail!(FORMA),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const E: &str = "https://auth.exemplo/t/";

    fn com(v: &Value) -> Result<Esperado> {
        Esperado::capturar(E, &json!({ CAMPO: v }))
    }

    #[test]
    fn tabela_da_politica_capturada() {
        let exige = com(&json!(true)).unwrap();
        assert!(exige.conferir(Some(E)).is_ok());
        assert_eq!(exige.conferir(None).unwrap_err().to_string(), RETORNO);
        for legado in [com(&json!(false)).unwrap(), Esperado::capturar(E, &json!({})).unwrap()] {
            assert!(legado.conferir(None).is_ok());
            assert!(legado.conferir(Some(E)).is_ok());
            assert_eq!(legado.conferir(Some("https://outro.exemplo/t/")).unwrap_err().to_string(), RETORNO);
        }
    }

    #[test]
    fn anuncio_de_outro_tipo_recusa_sem_presumir_false() {
        for v in [Value::Null, json!("true"), json!(1), json!(0), json!([]), json!({})] {
            assert_eq!(com(&v).unwrap_err().to_string(), ANUNCIO, "{v}");
        }
    }

    #[test]
    fn iss_compara_string_exata_sem_normalizar() {
        for politica in [json!(true), json!(false)] {
            let e = com(&politica).unwrap();
            for outro in [
                "",
                "https://auth.exemplo/t",
                "https://auth.exemplo/t//",
                "https://Auth.Exemplo/t/",
                "HTTPS://auth.exemplo/t/",
                "https://auth.exemplo:443/t/",
                " https://auth.exemplo/t/",
                "https://auth.exemplo/t/ ",
                "https://auth.exemplo/%74/",
                "https%3A%2F%2Fauth.exemplo%2Ft%2F",
            ] {
                assert_eq!(e.conferir(Some(outro)).unwrap_err().to_string(), RETORNO, "{outro:?}");
            }
        }
    }

    #[test]
    fn iss_do_pedido_nao_coage_tipo_errado_a_ausencia() {
        assert_eq!(do_pedido(&json!({})).unwrap(), None);
        assert_eq!(do_pedido(&json!({"iss": null})).unwrap(), None);
        assert_eq!(do_pedido(&json!({"iss": " https://a/ "})).unwrap(), Some(" https://a/ "));
        for v in [json!(1), json!(false), json!([E]), json!({"iss": E})] {
            assert_eq!(do_pedido(&json!({"iss": v})).unwrap_err().to_string(), FORMA, "{v}");
        }
    }
}
