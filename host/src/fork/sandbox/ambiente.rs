//! Pedido de preparação de imagem, não uma concessão para executar.
//!
//! O Hub escolhe a imagem de um catálogo confiável. O executor futuro prova
//! isolamento e valida autorização separadamente; aceitar este DTO não libera
//! execução nem autoriza argumentos de Docker, volumes, rede ou segredos.

use serde::{Deserialize, Deserializer};

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ImagemPreparada {
    #[serde(deserialize_with = "desserializar_digest")]
    digest: String,
}

impl ImagemPreparada {
    pub(crate) fn digest(&self) -> &str {
        &self.digest
    }
}

fn desserializar_digest<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let digest = String::deserialize(d)?;
    let valido = digest
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    if !valido {
        return Err(serde::de::Error::custom("image_digest_required"));
    }
    Ok(digest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const DIGEST: &str = "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

    #[test]
    fn aceita_apenas_digest_fixo_no_formato_canonico() {
        let pedido: ImagemPreparada = serde_json::from_value(json!({"digest": DIGEST})).unwrap();
        assert_eq!(pedido.digest(), DIGEST);
        for digest in ["latest", "sha256:abc", "sha256: ", &DIGEST.to_uppercase()] {
            assert!(serde_json::from_value::<ImagemPreparada>(json!({"digest": digest})).is_err());
        }
        assert!(serde_json::from_value::<ImagemPreparada>(json!({})).is_err());
    }

    #[test]
    fn recusa_opcoes_que_furam_o_contrato_mesmo_desativadas() {
        for campo in ["dockerSocket", "network", "privileged", "capabilities", "mounts", "env", "buildArgs"] {
            let mut pedido = json!({"digest": DIGEST});
            pedido[campo] = json!(null);
            assert!(serde_json::from_value::<ImagemPreparada>(pedido).is_err(), "{campo}");
        }
    }

    #[test]
    fn recusa_digest_repetido() {
        let pedido = format!(r#"{{"digest":"{DIGEST}","digest":"{DIGEST}"}}"#);
        assert!(serde_json::from_str::<ImagemPreparada>(&pedido).is_err());
    }
}
