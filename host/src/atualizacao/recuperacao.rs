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
    if estado.expirar_journal(trava, &mut journal, agora)? {
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
            ensure!(
                !matches!(journal.estado.fase, Fase::Pendente { .. } | Fase::Expirada)
                    && journal.estado.marca < journal.sequencia_nova,
                "estado anterior à publicação inconsistente"
            );
            estado.manifesto_assinado(trava, &journal)?
        }
    };
    estado.snapshot_transacao(trava, &journal)?;
    if matches!(acao, Recuperacao::RestabelecerAnterior | Recuperacao::TerminarReversao)
        && let Some(anterior) = journal.sequencia_anterior
    {
        ensure!(
            anterior >= journal.estado.piso && anterior >= manifesto.sequencia_minima_reversivel,
            "piso impede reativar o par anterior"
        );
    }
    Ok(Plano { journal, acao })
}
