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
    if !digest_canonico(&digest) {
        return Err(serde::de::Error::custom("image_digest_required"));
    }
    Ok(digest)
}

fn digest_canonico(digest: &str) -> bool {
    digest
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

/// O pedido escolhe um ID; repositório, tag e digest não vêm do chamador.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct PedidoImagem {
    imagem_id: String,
}

#[derive(Clone, Copy)]
struct EntradaCatalogo {
    id: &'static str,
    repositorio: &'static str,
    digest: &'static str,
}

// Nenhuma imagem de produção foi aprovada nesta fatia.
const CATALOGO_LOCAL: &[EntradaCatalogo] = &[];

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ErroImagem {
    NaoCatalogada,
    CatalogoInvalido,
    DigestNaoConfirmado,
}

/// A referência identifica conteúdo esperado, sem conceder autorização de uso.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ImagemFixada {
    repositorio: &'static str,
    digest: &'static str,
}

impl PedidoImagem {
    pub(crate) fn resolver(&self) -> Result<ImagemFixada, ErroImagem> {
        self.resolver_no(CATALOGO_LOCAL)
    }

    fn resolver_no(&self, catalogo: &[EntradaCatalogo]) -> Result<ImagemFixada, ErroImagem> {
        let mut selecionada = None;
        for (indice, entrada) in catalogo.iter().enumerate() {
            if !segmento_canonico(entrada.id)
                || !repositorio_canonico(entrada.repositorio)
                || !digest_canonico(entrada.digest)
                || catalogo[..indice].iter().any(|anterior| anterior.id == entrada.id)
            {
                return Err(ErroImagem::CatalogoInvalido);
            }
            if entrada.id == self.imagem_id {
                selecionada = Some(ImagemFixada { repositorio: entrada.repositorio, digest: entrada.digest });
            }
        }
        selecionada.ok_or(ErroImagem::NaoCatalogada)
    }
}

impl ImagemFixada {
    pub(crate) fn referencia(&self) -> String {
        format!("{}@{}", self.repositorio, self.digest)
    }

    /// Compara o digest do manifesto observado para esta referência por um
    /// futuro preparador confiável. Não é um hash calculado nem prova de isolamento.
    pub(crate) fn conferir_digest(&self, observado: Option<&str>) -> Result<(), ErroImagem> {
        if observado == Some(self.digest) { Ok(()) } else { Err(ErroImagem::DigestNaoConfirmado) }
    }
}

// Subconjunto deliberadamente estreito: host DNS explícito, sem porta, e caminho
// minúsculo. Sem tag, digest embutido, URL ou alias de repositório do cliente.
fn repositorio_canonico(repositorio: &str) -> bool {
    let Some((host, caminho)) = repositorio.split_once('/') else {
        return false;
    };
    repositorio.len() <= 255
        && host.contains('.')
        && host.split('.').all(segmento_canonico)
        && host.rsplit('.').next().is_some_and(|s| s.starts_with(|c: char| c.is_ascii_lowercase()))
        && caminho.split('/').all(segmento_canonico)
}

fn segmento_canonico(segmento: &str) -> bool {
    segmento.len() <= 63
        && segmento
            .split('-')
            .all(|parte| !parte.is_empty() && parte.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
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

#[cfg(test)]
mod testes_catalogo {
    use super::*;
    use serde_json::json;

    const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const OUTRO: &str = "sha256:1123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const BASE: EntradaCatalogo =
        EntradaCatalogo { id: "img-base", repositorio: "registry.example.invalid/ambientes/base", digest: DIGEST };
    const TOOLS: EntradaCatalogo =
        EntradaCatalogo { id: "img-tools", repositorio: "registry.example.invalid/ambientes/tools", digest: OUTRO };
    const CATALOGO: &[EntradaCatalogo] = &[BASE, TOOLS];

    fn pedido(id: &str) -> PedidoImagem {
        serde_json::from_value(json!({"imagemId": id})).unwrap()
    }

    #[test]
    fn pedido_nao_admite_referencia_ou_opcoes_externas() {
        for campo in [
            "digest",
            "repository",
            "tag",
            "dockerSocket",
            "network",
            "privileged",
            "capabilities",
            "mounts",
            "env",
            "buildArgs",
        ] {
            let mut value = json!({"imagemId": "img-base"});
            value[campo] = json!(null);
            assert!(serde_json::from_value::<PedidoImagem>(value).is_err(), "{campo}");
        }
        for tampered in [r"{}", r#"{"imagemId":"img-base","imagemId":"img-base"}"#, r#"{"imagemId":42}"#] {
            assert!(serde_json::from_str::<PedidoImagem>(tampered).is_err());
        }
    }

    #[test]
    fn seleciona_so_entrada_exata_do_catalogo() {
        for entrada in CATALOGO {
            let fixa = pedido(entrada.id).resolver_no(CATALOGO).unwrap();
            assert_eq!(fixa.referencia(), format!("{}@{}", entrada.repositorio, entrada.digest));
        }
        for id in ["", "img-bases", "IMG-BASE", " img-base", BASE.repositorio, DIGEST] {
            assert_eq!(pedido(id).resolver_no(CATALOGO), Err(ErroImagem::NaoCatalogada));
        }
        assert_eq!(pedido(BASE.id).resolver(), Err(ErroImagem::NaoCatalogada));
    }

    #[test]
    fn recusa_catalogo_invalido_ou_com_id_repetido() {
        for outra in [BASE, EntradaCatalogo { digest: OUTRO, ..BASE }] {
            assert_eq!(pedido(TOOLS.id).resolver_no(&[BASE, outra, TOOLS]), Err(ErroImagem::CatalogoInvalido));
        }
        for repositorio in [
            "base",
            "ambientes/base",
            "https://registry.example.invalid/base",
            "registry.example.invalid/base:latest",
            "registry.example.invalid/base@sha256:abc",
            "registry.example.invalid:5000/base",
            "registry.example.invalid/a/../base",
            "registry.example.invalid//base",
            "registry.example.invalid/UPPER",
        ] {
            let outra = EntradaCatalogo { repositorio, ..TOOLS };
            assert_eq!(pedido(BASE.id).resolver_no(&[BASE, outra]), Err(ErroImagem::CatalogoInvalido));
        }
        for digest in [
            "latest",
            "sha256:abc",
            "sha256: ",
            "sha256:0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF",
        ] {
            let outra = EntradaCatalogo { digest, ..TOOLS };
            assert_eq!(pedido(BASE.id).resolver_no(&[BASE, outra]), Err(ErroImagem::CatalogoInvalido));
        }
    }

    #[test]
    fn compara_digest_observado_sem_normalizar_ou_usar_fallback() {
        let fixa = pedido(BASE.id).resolver_no(CATALOGO).unwrap();
        assert_eq!(fixa.conferir_digest(Some(DIGEST)), Ok(()));
        for observado in [None, Some(OUTRO), Some("sha256:abc"), Some(&DIGEST.to_uppercase())] {
            assert_eq!(fixa.conferir_digest(observado), Err(ErroImagem::DigestNaoConfirmado));
        }
    }
}
