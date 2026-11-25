//! Partida do Motor no macOS: admissão só sob perfil Seatbelt comprovado para
//! o motor e cada descendente. Toda recusa vira o código estável
//! `motor_macos_indisponivel` antes de Motor/MCP; o motivo é tipado e não
//! carrega segredo nem entrada bruta.

use std::num::NonZeroU64;

pub(super) const CODIGO: &str = "motor_macos_indisponivel";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Perfil<'a> {
    pub(super) identificador: &'a str,
    pub(super) sha256: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Membro<'a> {
    pub(super) pid: NonZeroU64,
    pub(super) perfil: Option<Perfil<'a>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ObservacaoPerfil<'a> {
    FalhaDeCarga,
    Carregado(Perfil<'a>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ObservacaoCenso<'a> {
    SemCenso,
    Observado { motor: Membro<'a>, descendentes: Vec<Membro<'a>> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Capacidade {
    Mediada,
    Shell,
    Sudo,
    Tela,
}

/// Autorização de partida: move-only, sem `Copy`/`Clone`. Cada instância sai
/// apenas da verificação e não pode ser duplicada para burlar "Parar tudo";
/// quem a segura só lê o perfil admitido.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Partida<'a> {
    perfil: Perfil<'a>,
}

impl Partida<'_> {
    /// Acesso somente leitura: quem recebe a partida sabe sob qual perfil a
    /// unidade foi admitida; ninguém constrói uma partida fora da verificação.
    pub(super) fn perfil(&self) -> Perfil<'_> {
        self.perfil
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Recusa {
    Sudo,
    Shell,
    Tela,
    SemMediada,
    PerfilMalFormado,
    PerfilAusente,
    PerfilInvalido,
    Barreira,
    DescendenteForaDoPerfil,
}

impl Recusa {
    fn motivo(self) -> &'static str {
        match self {
            Recusa::Sudo => "sudo sempre negado",
            Recusa::Shell => "shell desligado por padrão",
            Recusa::Tela => "tela desligada",
            Recusa::SemMediada => "partida exige a capacidade mediada explícita",
            Recusa::PerfilMalFormado => "perfil esperado mal formado",
            Recusa::PerfilAusente => "perfil não carregado",
            Recusa::PerfilInvalido => "perfil carregado divergente do esperado",
            Recusa::Barreira => "barreira não comprovada",
            Recusa::DescendenteForaDoPerfil => "descendente fora do perfil comprovado",
        }
    }
}

pub(super) fn partir<'a>(
    esperado: Perfil<'a>,
    perfil: ObservacaoPerfil<'a>,
    censo: ObservacaoCenso<'a>,
    capacidades: &[Capacidade],
) -> Result<Partida<'a>, Recusa> {
    // Regra permanente de negação antes de qualquer concessão.
    if capacidades.contains(&Capacidade::Sudo) {
        return Err(Recusa::Sudo);
    }
    if capacidades.contains(&Capacidade::Shell) {
        return Err(Recusa::Shell);
    }
    if capacidades.contains(&Capacidade::Tela) {
        return Err(Recusa::Tela);
    }
    // Ausência de sinal não é permissão: sem o pedido mediado explícito, nega.
    if !capacidades.contains(&Capacidade::Mediada) {
        return Err(Recusa::SemMediada);
    }
    if esperado.identificador.is_empty() || esperado.sha256 == [0; 32] {
        return Err(Recusa::PerfilMalFormado);
    }
    let ObservacaoPerfil::Carregado(carregado) = perfil else {
        return Err(Recusa::PerfilAusente);
    };
    if carregado != esperado {
        return Err(Recusa::PerfilInvalido);
    }
    let ObservacaoCenso::Observado { motor, descendentes } = censo else {
        return Err(Recusa::Barreira);
    };
    if motor.perfil != Some(esperado) {
        return Err(Recusa::Barreira);
    }
    for descendente in &descendentes {
        if descendente.perfil != Some(esperado) {
            return Err(Recusa::DescendenteForaDoPerfil);
        }
    }
    Ok(Partida { perfil: esperado })
}

/// Recusa estável da partida: sempre o mesmo código, antes de Motor/MCP.
pub(super) fn recusa_estavel(recusa: Recusa) -> anyhow::Error {
    anyhow::anyhow!("{CODIGO}: {}", recusa.motivo())
}
