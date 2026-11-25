#[path = "../src/fork/fork-motor-macos/admissao.rs"]
mod admissao;
#[path = "../src/fork/fork-motor-macos/partida.rs"]
mod partida;

use std::num::NonZeroU64;

use partida::{Capacidade, Membro, ObservacaoCenso, ObservacaoPerfil, Perfil, Recusa, partir, recusa_estavel};

const PERFIL: Perfil<'_> = Perfil { identificador: "invalid.example.motor-seatbelt", sha256: [0x71; 32] };

fn membro(pid: u64, perfil: Option<Perfil<'_>>) -> Membro<'_> {
    Membro { pid: NonZeroU64::new(pid).expect("PID da fixture deve ser diferente de zero"), perfil }
}

fn censo_limpo(descendentes: Vec<Membro<'_>>) -> ObservacaoCenso<'_> {
    ObservacaoCenso::Observado { motor: membro(31, Some(PERFIL)), descendentes }
}

fn partir_mediado<'a>(
    perfil: ObservacaoPerfil<'a>,
    censo: ObservacaoCenso<'a>,
    capacidades: &[Capacidade],
) -> Result<partida::Partida<'a>, Recusa> {
    partir(PERFIL, perfil, censo, capacidades)
}

#[test]
fn sudo_sempre_negado_antes_de_qualquer_prova() {
    // A regra permanente nega antes mesmo de um perfil ausente ou censo falso.
    assert_eq!(
        partir_mediado(
            ObservacaoPerfil::FalhaDeCarga,
            ObservacaoCenso::SemCenso,
            &[Capacidade::Mediada, Capacidade::Sudo]
        ),
        Err(Recusa::Sudo)
    );
}

#[test]
fn shell_e_tela_ficam_desligados_nesta_candidata() {
    for capacidade in [Capacidade::Shell, Capacidade::Tela] {
        assert_eq!(
            partir_mediado(
                ObservacaoPerfil::Carregado(PERFIL),
                censo_limpo(vec![]),
                &[Capacidade::Mediada, capacidade],
            ),
            Err(if capacidade == Capacidade::Shell { Recusa::Shell } else { Recusa::Tela })
        );
    }
}

#[test]
fn perfil_que_nao_carrega_recusa_antes_do_censo() {
    assert_eq!(
        partir_mediado(ObservacaoPerfil::FalhaDeCarga, censo_limpo(vec![]), &[Capacidade::Mediada]),
        Err(Recusa::PerfilAusente)
    );
}

#[test]
fn perfil_carregado_divergente_recusa() {
    let outro = Perfil { sha256: [0x73; 32], ..PERFIL };
    assert_eq!(
        partir_mediado(ObservacaoPerfil::Carregado(outro), censo_limpo(vec![]), &[Capacidade::Mediada]),
        Err(Recusa::PerfilInvalido)
    );
}

#[test]
fn perfil_esperado_mal_formado_recusa() {
    let sem_id = Perfil { identificador: "", ..PERFIL };
    let hash_zero = Perfil { sha256: [0; 32], ..PERFIL };
    for esperado in [sem_id, hash_zero] {
        assert_eq!(
            partir(esperado, ObservacaoPerfil::Carregado(esperado), censo_limpo(vec![]), &[Capacidade::Mediada]),
            Err(Recusa::PerfilMalFormado)
        );
    }
}

#[test]
fn barreira_sem_censo_ou_motor_fora_do_perfil_recusa() {
    assert_eq!(
        partir_mediado(ObservacaoPerfil::Carregado(PERFIL), ObservacaoCenso::SemCenso, &[Capacidade::Mediada]),
        Err(Recusa::Barreira)
    );
    let motor_fora = ObservacaoCenso::Observado { motor: membro(31, None), descendentes: vec![] };
    assert_eq!(
        partir_mediado(ObservacaoPerfil::Carregado(PERFIL), motor_fora, &[Capacidade::Mediada]),
        Err(Recusa::Barreira)
    );
}

#[test]
fn descendente_fora_do_perfil_recusa_a_partida() {
    let outro_perfil = Perfil { sha256: [0x79; 32], ..PERFIL };
    for descendente in [membro(32, None), membro(33, Some(outro_perfil))] {
        assert_eq!(
            partir_mediado(ObservacaoPerfil::Carregado(PERFIL), censo_limpo(vec![descendente]), &[Capacidade::Mediada]),
            Err(Recusa::DescendenteForaDoPerfil)
        );
    }
}

#[test]
fn capacidades_vazias_recusam_a_partida() {
    // Ausência de pedido não é permissão: sem a capacidade mediada explícita, nega.
    assert_eq!(partir_mediado(ObservacaoPerfil::Carregado(PERFIL), censo_limpo(vec![]), &[]), Err(Recusa::SemMediada));
    let estavel = recusa_estavel(Recusa::SemMediada).to_string();
    assert!(estavel.starts_with("motor_macos_indisponivel: "), "recusa fora do código estável: {estavel}");
}

#[test]
fn partida_autorizada_so_com_perfil_censo_e_capacidade_mediada() {
    let capacidades = [Capacidade::Mediada];
    for censo in [censo_limpo(vec![]), censo_limpo(vec![membro(32, Some(PERFIL)), membro(33, Some(PERFIL))])] {
        // A interface da partida é somente leitura: o perfil sai da verificação,
        // nunca entra; construir uma partida fora dela é erro de compilação.
        let autorizada = partir_mediado(ObservacaoPerfil::Carregado(PERFIL), censo, &capacidades)
            .expect("partida autorizada pela verificação");
        assert_eq!(autorizada.perfil(), PERFIL);
    }
}

#[test]
fn toda_recusa_vira_o_codigo_estavel_antes_de_motor_e_mcp() {
    let recusas = [
        (
            PERFIL,
            Recusa::Sudo,
            ObservacaoPerfil::FalhaDeCarga,
            ObservacaoCenso::SemCenso,
            vec![Capacidade::Mediada, Capacidade::Sudo],
        ),
        (PERFIL, Recusa::Shell, ObservacaoPerfil::Carregado(PERFIL), censo_limpo(vec![]), vec![Capacidade::Shell]),
        (PERFIL, Recusa::Tela, ObservacaoPerfil::Carregado(PERFIL), censo_limpo(vec![]), vec![Capacidade::Tela]),
        (
            Perfil { identificador: "", ..PERFIL },
            Recusa::PerfilMalFormado,
            ObservacaoPerfil::Carregado(PERFIL),
            censo_limpo(vec![]),
            vec![Capacidade::Mediada],
        ),
        (PERFIL, Recusa::PerfilAusente, ObservacaoPerfil::FalhaDeCarga, censo_limpo(vec![]), vec![Capacidade::Mediada]),
        (PERFIL, Recusa::SemMediada, ObservacaoPerfil::Carregado(PERFIL), censo_limpo(vec![]), vec![]),
        (
            PERFIL,
            Recusa::PerfilInvalido,
            ObservacaoPerfil::Carregado(Perfil { sha256: [0x73; 32], ..PERFIL }),
            censo_limpo(vec![]),
            vec![Capacidade::Mediada],
        ),
        (
            PERFIL,
            Recusa::Barreira,
            ObservacaoPerfil::Carregado(PERFIL),
            ObservacaoCenso::SemCenso,
            vec![Capacidade::Mediada],
        ),
        (
            PERFIL,
            Recusa::DescendenteForaDoPerfil,
            ObservacaoPerfil::Carregado(PERFIL),
            censo_limpo(vec![membro(32, None)]),
            vec![Capacidade::Mediada],
        ),
    ];
    for (esperado, esperada, perfil, censo, capacidades) in recusas {
        let observada = partir(esperado, perfil, censo.clone(), &capacidades).expect_err("partida que recusa");
        assert_eq!(observada, esperada);
        let estavel = recusa_estavel(observada).to_string();
        assert!(estavel.starts_with("motor_macos_indisponivel: "), "recusa fora do código estável: {estavel}");
    }
    // A partida recusa com o mesmo código da recusa de produção.
    assert_eq!(partida::CODIGO, admissao::CODIGO);
    assert!(admissao::indisponivel().is_err());
}
