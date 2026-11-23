//! Cerca de integridade e prazo antes de qualquer recuperação de crash.
//! A execução das ações fica em `transacao`.
use super::{
    armazenamento::Diretorio,
    estado::Fase,
    journal::{self, Etapa, Journal, Recuperacao},
    slots::Slot,
};
use anyhow::{Result, ensure};
use std::fs::File;

pub struct Plano {
    pub journal: Journal,
    pub acao: Recuperacao,
}

/// Retorna somente uma decisão comprovada; não faz troca nem inicia serviço.
/// O executor precisa registrar e sincronizar cada efeito antes do próximo.
pub fn validar(estado: &Diretorio, publicacao: &Diretorio, trava: &File, agora: i64) -> Result<Plano> {
    estado.validar_trava(trava)?;
    estado.exigir_sem_bloqueio()?;
    publicacao.exigir_sem_emergencia()?;
    let mut journal: Journal = estado.ler("journal")?;
    if estado.expirar_journal(publicacao, trava, &mut journal, agora)? {
        publicacao.publicar_status(estado, trava)?;
        anyhow::bail!("janela expirada; recuperação administrativa necessária");
    }
    let seletor: Option<Slot> = publicacao.seletor()?;
    let acao = journal::planejar_recuperacao(&journal, seletor)?;
    if matches!(acao, Recuperacao::Concluida) {
        if matches!(journal.etapa, Etapa::Cancelada) {
            ensure!(
                journal.estado.marca < journal.sequencia_nova && journal.estado.piso <= journal.estado.marca,
                "marcas monotônicas inconsistentes"
            );
        } else {
            ensure!(
                journal.estado.marca == journal.sequencia_nova && journal.estado.piso <= journal.estado.marca,
                "marcas monotônicas inconsistentes"
            );
        }
        return Ok(Plano { journal, acao });
    }
    // Antes da publicação, voltar ao par anterior não depende da validade do
    // manifesto novo; somente janela aberta e reversão exigem manifesto válido.
    let manifesto = match acao {
        Recuperacao::Pendente | Recuperacao::TerminarReversao => estado.manifesto_transacao(trava, &journal, agora)?,
        _ => {
            exigir_pre_publicacao(&journal)?;
            estado.manifesto_assinado(trava, &journal)?
        }
    };
    estado.snapshot_transacao(trava, &journal)?;
    if matches!(acao, Recuperacao::TerminarReversao)
        && let Some(anterior) = journal.sequencia_anterior
    {
        ensure!(
            anterior >= journal.estado.piso && anterior >= manifesto.sequencia_minima_reversivel,
            "piso impede reativar o par anterior"
        );
    }
    Ok(Plano { journal, acao })
}

pub(super) fn exigir_pre_publicacao(journal: &Journal) -> Result<()> {
    ensure!(
        !matches!(journal.estado.fase, Fase::Pendente { .. } | Fase::Expirada)
            && journal.publicado_em.is_none()
            && journal.estado.marca < journal.sequencia_nova
            && journal.estado.piso <= journal.estado.marca,
        "estado anterior à publicação inconsistente"
    );
    ensure!(
        journal.sequencia_anterior.is_none_or(|anterior| anterior >= journal.estado.piso),
        "piso impede reativar o par anterior"
    );
    Ok(())
}

#[cfg(test)]
#[test]
fn cancelamento_preserva_piso_anterior_sem_autorizar_reversao() -> Result<()> {
    use super::estado::Estado;

    let mut journal = Journal {
        estado: Estado { marca: 7, piso: 6, fase: Fase::Aceita },
        etapa: Etapa::CancelamentoIniciado,
        transacao: "a".repeat(64),
        sequencia_nova: 8,
        publicado_em: None,
        sequencia_anterior: Some(7),
        slot_anterior: Some(Slot::A),
        slot_novo: Slot::B,
        manifesto_sha256: "b".repeat(64),
        snapshot_sha256: "c".repeat(64),
    };
    assert_eq!(journal::planejar_recuperacao(&journal, Some(Slot::A))?, Recuperacao::RestabelecerAnterior);
    exigir_pre_publicacao(&journal)?;
    journal.sequencia_anterior = Some(5);
    assert!(exigir_pre_publicacao(&journal).is_err());
    journal.sequencia_anterior = Some(7);
    journal.publicado_em = Some(100);
    assert!(exigir_pre_publicacao(&journal).is_err());
    journal.estado.iniciar(8, 8, Some(7), 200, 100)?;
    assert!(journal.estado.reverter(101).is_err());
    assert!(exigir_pre_publicacao(&journal).is_err());
    Ok(())
}
