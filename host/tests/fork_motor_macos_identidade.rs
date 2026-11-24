#[path = "../src/fork/fork-motor-macos/identidade.rs"]
mod identidade;

use std::num::NonZeroU64;

use identidade::{Artefato, CodigoAssinado, Observacao, ParValidado, Recusa};

const ENTITLEMENTS: &[(&str, Option<bool>)] = &[("com.apple.security.app-sandbox", Some(true))];
const CODIGO: CodigoAssinado<'_> = CodigoAssinado {
    equipe: "EQUIPEX001",
    identificador: "invalid.example.motor-fixture",
    cdhash_sha256_20: [0x81; 20],
    entitlements: ENTITLEMENTS,
};
const ARTEFATO: Artefato<'_> = Artefato { codigo: CODIGO, sha256_macho: [0x92; 32] };
const GERACAO: NonZeroU64 = NonZeroU64::new(2).unwrap();

fn admitir<'a>(
    manifesto: &Artefato<'a>,
    artefato: Observacao<Artefato<'a>>,
    par: Observacao<CodigoAssinado<'a>>,
) -> Result<(), Recusa> {
    let par = match par {
        Observacao::Ausente => Observacao::Ausente,
        Observacao::Invalida => Observacao::Invalida,
        Observacao::OutraArquitetura => Observacao::OutraArquitetura,
        Observacao::Arm64Validada(codigo) => Observacao::Arm64Validada(ParValidado { codigo, geracao: GERACAO }),
    };
    identidade::admitir(manifesto, GERACAO, artefato, par)
}

#[test]
fn so_aceita_macho_e_par_autenticado_exatos() {
    assert_eq!(admitir(&ARTEFATO, Observacao::Arm64Validada(ARTEFATO), Observacao::Arm64Validada(CODIGO)), Ok(()));
    let outro = CodigoAssinado { cdhash_sha256_20: [0x83; 20], ..CODIGO };
    let outro_artefato = Artefato { codigo: outro, ..ARTEFATO };
    assert_eq!(
        admitir(&ARTEFATO, Observacao::Arm64Validada(outro_artefato), Observacao::Arm64Validada(CODIGO)),
        Err(Recusa::ArtefatoDivergente)
    );
    assert_eq!(
        admitir(&ARTEFATO, Observacao::Arm64Validada(ARTEFATO), Observacao::Arm64Validada(outro)),
        Err(Recusa::ParDivergente)
    );
    let outros_bytes = Artefato { sha256_macho: [0x93; 32], ..ARTEFATO };
    assert_eq!(
        admitir(&ARTEFATO, Observacao::Arm64Validada(outros_bytes), Observacao::Arm64Validada(CODIGO)),
        Err(Recusa::ArtefatoDivergente)
    );
}

#[test]
fn nega_assinatura_ausente_invalida_ou_fora_do_recorte_arm64() {
    for observacao in [Observacao::Ausente, Observacao::Invalida, Observacao::OutraArquitetura] {
        assert_eq!(admitir(&ARTEFATO, observacao, Observacao::Arm64Validada(CODIGO)), Err(Recusa::SemArtefatoArm64));
    }
    for observacao in [Observacao::Ausente, Observacao::Invalida, Observacao::OutraArquitetura] {
        assert_eq!(admitir(&ARTEFATO, Observacao::Arm64Validada(ARTEFATO), observacao), Err(Recusa::SemParArm64));
    }
}

#[test]
fn nega_outro_papel_ou_equipe_e_entitlements_extras_ate_no_manifesto() {
    let outra_equipe = CodigoAssinado { equipe: "EQUIPEX002", ..CODIGO };
    let outro_papel = CodigoAssinado { identificador: "invalid.example.outro-papel", ..CODIGO };
    for codigo in [outra_equipe, outro_papel] {
        assert_eq!(
            admitir(&ARTEFATO, Observacao::Arm64Validada(ARTEFATO), Observacao::Arm64Validada(codigo)),
            Err(Recusa::ParDivergente)
        );
    }
    let extras = [ENTITLEMENTS[0], ("com.apple.security.network.client", Some(true))];
    let mach = [ENTITLEMENTS[0], ("com.apple.security.temporary-exception.mach-lookup.global-name", None)];
    let cofre = [ENTITLEMENTS[0], ("keychain-access-groups", None)];
    let sem_sandbox = [("com.apple.security.app-sandbox", Some(false))];
    for entitlements in [&extras[..], &mach, &cofre, &sem_sandbox, &[]] {
        let codigo = CodigoAssinado { entitlements, ..CODIGO };
        assert_eq!(
            admitir(&ARTEFATO, Observacao::Arm64Validada(ARTEFATO), Observacao::Arm64Validada(codigo)),
            Err(Recusa::ParDivergente)
        );
        let manifesto = Artefato { codigo, ..ARTEFATO };
        assert_eq!(
            admitir(&manifesto, Observacao::Arm64Validada(manifesto), Observacao::Arm64Validada(codigo)),
            Err(Recusa::Manifesto)
        );
    }
}

#[test]
fn mesmo_codigo_de_geracao_antiga_nao_admite_trabalho() {
    assert_eq!(
        identidade::admitir(
            &ARTEFATO,
            GERACAO,
            Observacao::Arm64Validada(ARTEFATO),
            Observacao::Arm64Validada(ParValidado { codigo: CODIGO, geracao: NonZeroU64::MIN }),
        ),
        Err(Recusa::Geracao)
    );
}
