use std::num::NonZeroU64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CodigoAssinado<'a> {
    pub(super) equipe: &'a str,
    pub(super) identificador: &'a str,
    pub(super) cdhash_sha256_20: [u8; 20],
    pub(super) entitlements: &'a [(&'a str, Option<bool>)],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Artefato<'a> {
    pub(super) codigo: CodigoAssinado<'a>,
    pub(super) sha256_macho: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ParValidado<'a> {
    pub(super) codigo: CodigoAssinado<'a>,
    pub(super) geracao: NonZeroU64,
}

#[derive(Clone, Copy)]
pub(super) enum Observacao<T> {
    Ausente,
    Invalida,
    OutraArquitetura,
    Arm64Validada(T),
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Recusa {
    Manifesto,
    SemArtefatoArm64,
    ArtefatoDivergente,
    SemParArm64,
    ParDivergente,
    Geracao,
}

pub(super) fn admitir(
    manifesto: &Artefato<'_>,
    geracao: NonZeroU64,
    artefato: Observacao<Artefato<'_>>,
    par: Observacao<ParValidado<'_>>,
) -> Result<(), Recusa> {
    if manifesto.codigo.entitlements != [("com.apple.security.app-sandbox", Some(true))]
        || manifesto.codigo.identificador.is_empty()
        || manifesto.codigo.equipe.is_empty()
        || manifesto.codigo.cdhash_sha256_20 == [0; 20]
        || manifesto.sha256_macho == [0; 32]
    {
        return Err(Recusa::Manifesto);
    }
    let Observacao::Arm64Validada(artefato) = artefato else {
        return Err(Recusa::SemArtefatoArm64);
    };
    if artefato != *manifesto {
        return Err(Recusa::ArtefatoDivergente);
    }
    let Observacao::Arm64Validada(par) = par else {
        return Err(Recusa::SemParArm64);
    };
    if par.codigo != manifesto.codigo {
        return Err(Recusa::ParDivergente);
    }
    if par.geracao != geracao {
        return Err(Recusa::Geracao);
    }
    Ok(())
}
