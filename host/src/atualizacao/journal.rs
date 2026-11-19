//! Registros duráveis locais. Não autoriza publicação por si só.
use super::{
    armazenamento::Diretorio,
    estado::{Estado, Fase},
    slots::Slot,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::fs::File;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Etapa {
    Preparada,
    SlotPreparado,
    DesativacaoIniciada,
    AnteriorDesativado,
    SeletorTrocado,
    Pendente,
    ReversaoIniciada,
    SeletorRestaurado,
    Concluida,
    /// Volta anterior à publicação; a marca monotônica não avança.
    CancelamentoIniciado,
    /// Final: o par anterior (ou nenhum) permanece ativo.
    Cancelada,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Journal {
    pub estado: Estado,
    pub etapa: Etapa,
    pub transacao: String,
    pub sequencia_nova: u64,
    pub publicado_em: Option<i64>,
    pub sequencia_anterior: Option<u64>,
    pub slot_anterior: Option<Slot>,
    pub slot_novo: Slot,
    pub manifesto_sha256: String,
    pub snapshot_sha256: String,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Operacao {
    Atualizar,
    Aceitar,
    Reverter,
    Recuperar,
    /// Etapa administrativa: instalação, verificação e repinagem da raiz.
    Administrar,
    /// Revogação administrativa da fronteira privilegiada.
    Revogar,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Resultado {
    Inicio,
    Sucesso,
    Recusada,
    Falha,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Motivo {
    Solicitacao,
    Politica,
    Integridade,
    Estado,
    Sistema,
    Expiracao,
    ParadaSolicitada,
    Relogio,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origem {
    Sudo,
    #[serde(rename = "admin/root")]
    AdminRoot,
    GuardaRoot,
}

#[derive(Serialize)]
pub struct Evento {
    pub horario_unix: i64,
    pub uid: u32,
    pub origem: Origem,
    pub operacao: Operacao,
    pub sequencia_atual: Option<u64>,
    pub sequencia_anterior: Option<u64>,
    pub resultado: Resultado,
    pub motivo: Motivo,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Bloqueio {
    motivo: String,
}

impl Diretorio {
    /// Deve preceder qualquer alteração privilegiada. Somente dados enumerados são auditados.
    pub fn inicio(&self, evento: &Evento) -> Result<()> {
        ensure!(matches!(evento.resultado, Resultado::Inicio), "evento de início exigido");
        self.auditar(evento)
    }

    /// Erro de relógio/estado bloqueia novas operações até intervenção root.
    pub fn bloquear_por_falha(&self, trava: &File) -> Result<()> {
        self.validar_trava(trava)?;
        self.gravar("bloqueio", &Bloqueio { motivo: "falha_de_verificacao".into() })
    }

    /// Marca de bloqueio é gravada antes do journal expirado. Um crash entre as
    /// duas escritas deixa o bloqueio ativo, nunca uma transação aceitável.
    pub fn expirar_journal(&self, trava: &File, journal: &mut Journal, agora: i64) -> Result<bool> {
        self.validar_trava(trava)?;
        if !matches!(journal.estado.fase, Fase::Pendente { prazo, .. } if agora >= prazo) {
            return Ok(false);
        }
        self.gravar("bloqueio", &Bloqueio { motivo: "expiracao_sem_aceite".into() })?;
        journal.estado.fase = Fase::Expirada;
        self.gravar("journal", journal)?;
        Ok(true)
    }
}

impl Diretorio {
    /// Apenas fecha uma janela já publicada. Não substitui o bloqueio do serviço.
    /// O UID deve vir da identidade autenticada pela fronteira de invocação.
    pub fn aceitar_transacao(
        &self,
        publicacao: &Diretorio,
        trava: &File,
        chamador: super::chamador::Identidade,
        agora: i64,
    ) -> Result<()> {
        self.validar_trava(trava)?;
        let journal_lido: Result<Journal> = self.ler("journal");
        let sequencia_atual = journal_lido.as_ref().ok().map(|j| j.estado.marca);
        let sequencia_anterior = journal_lido.as_ref().ok().and_then(|j| j.sequencia_anterior);
        let evento = |resultado, motivo| Evento {
            horario_unix: agora,
            uid: chamador.uid(),
            origem: chamador.origem(),
            operacao: Operacao::Aceitar,
            sequencia_atual,
            sequencia_anterior,
            resultado,
            motivo,
        };
        self.inicio(&evento(Resultado::Inicio, Motivo::Solicitacao))?;
        let mut motivo = Motivo::Estado;
        let resultado = (|| -> Result<()> {
            self.exigir_sem_bloqueio()?;
            publicacao.exigir_sem_emergencia()?;
            let mut journal = journal_lido?;
            if self.expirar_journal(trava, &mut journal, agora)? {
                motivo = Motivo::Expiracao;
                publicacao.publicar_status(self, trava)?;
                anyhow::bail!("janela expirada; recuperação administrativa necessária");
            }
            // Antes de encerrar uma janela, confira também o manifesto assinado
            // e o snapshot da transação; somente a repetição já concluída pode
            // dispensá-los, pois não muda o par publicado.
            let plano = super::recuperacao::validar(self, publicacao, trava, agora)?.acao;
            ensure!(
                journal.estado.marca == journal.sequencia_nova && journal.estado.piso <= journal.estado.marca,
                "marcas monotônicas inconsistentes"
            );
            if matches!(journal.etapa, Etapa::Concluida) && matches!(journal.estado.fase, Fase::Aceita) {
                ensure!(matches!(plano, Recuperacao::Concluida), "aceite concluído inconsistente");
                // Um crash após gravar o journal pode ter deixado o status antigo.
                publicacao.publicar_status(self, trava)?;
                return Ok(());
            }
            ensure!(
                matches!(journal.etapa, Etapa::Pendente) && matches!(plano, Recuperacao::Pendente),
                "seletor não corresponde à transação pendente"
            );
            journal.estado.aceitar(agora)?;
            journal.etapa = Etapa::Concluida;
            self.gravar("journal", &journal)?;
            publicacao.publicar_status(self, trava)?;
            Ok(())
        })();
        let saida = if resultado.is_ok() { Resultado::Sucesso } else { Resultado::Recusada };
        let motivo = if resultado.is_ok() { Motivo::Solicitacao } else { motivo };
        self.auditar(&evento(saida, motivo))?;
        resultado
    }
}

/// Próxima ação após crash. A decisão não muda arquivos nem reativa capacidades.
#[derive(Debug, PartialEq, Eq)]
pub enum Recuperacao {
    CancelarPreparacao,
    RestabelecerAnterior,
    ResolverPublicacao,
    Pendente,
    TerminarReversao,
    Concluida,
}

pub fn planejar_recuperacao(journal: &Journal, seletor: Option<Slot>) -> Result<Recuperacao> {
    ensure!(journal.slot_anterior != Some(journal.slot_novo), "slots da transação coincidem");
    ensure!(journal.slot_anterior.is_some() == journal.sequencia_anterior.is_some(), "par anterior inconsistente");
    let antigo = journal.slot_anterior;
    let novo = Some(journal.slot_novo);
    let acao = match journal.etapa {
        Etapa::Preparada | Etapa::SlotPreparado => {
            ensure!(seletor == antigo, "seletor inesperado na preparação");
            Recuperacao::CancelarPreparacao
        }
        Etapa::DesativacaoIniciada => {
            ensure!(seletor == antigo, "seletor inesperado na desativação");
            Recuperacao::RestabelecerAnterior
        }
        Etapa::AnteriorDesativado => {
            if seletor == antigo {
                Recuperacao::RestabelecerAnterior
            } else if seletor == novo {
                Recuperacao::ResolverPublicacao
            } else {
                anyhow::bail!("seletor inesperado na troca")
            }
        }
        Etapa::SeletorTrocado => {
            ensure!(seletor == novo, "seletor inesperado após troca");
            Recuperacao::ResolverPublicacao
        }
        Etapa::Pendente => {
            ensure!(
                seletor == novo && matches!(journal.estado.fase, Fase::Pendente { .. }),
                "publicação pendente inconsistente"
            );
            Recuperacao::Pendente
        }
        Etapa::ReversaoIniciada => {
            ensure!(seletor == antigo || seletor == novo, "seletor inesperado durante reversão");
            Recuperacao::TerminarReversao
        }
        Etapa::SeletorRestaurado => {
            ensure!(seletor == antigo, "seletor anterior não restaurado");
            Recuperacao::TerminarReversao
        }
        Etapa::CancelamentoIniciado => {
            ensure!(seletor == antigo || seletor == novo, "seletor inesperado durante cancelamento");
            Recuperacao::RestabelecerAnterior
        }
        Etapa::Cancelada => {
            ensure!(
                seletor == antigo
                    && !matches!(journal.estado.fase, Fase::Pendente { .. } | Fase::Expirada)
                    && journal.estado.marca < journal.sequencia_nova,
                "cancelamento inconsistente"
            );
            Recuperacao::Concluida
        }
        Etapa::Concluida => {
            match journal.estado.fase {
                Fase::Aceita => ensure!(seletor == novo, "seletor não corresponde ao aceite"),
                Fase::Revertida => ensure!(seletor == antigo, "seletor não corresponde à reversão"),
                _ => anyhow::bail!("estado final inválido"),
            }
            Recuperacao::Concluida
        }
    };
    Ok(acao)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recuperacao_cobre_todas_as_fases_e_estados_finais() {
        let mut journal = Journal {
            estado: Estado { marca: 7, piso: 3, fase: Fase::Pendente { anterior: Some(6), prazo: 200 } },
            etapa: Etapa::Preparada,
            transacao: "c".repeat(64),
            sequencia_nova: 7,
            publicado_em: Some(100),
            sequencia_anterior: Some(6),
            slot_anterior: Some(Slot::A),
            slot_novo: Slot::B,
            manifesto_sha256: "a".repeat(64),
            snapshot_sha256: "b".repeat(64),
        };
        for etapa in [Etapa::Preparada, Etapa::SlotPreparado] {
            journal.etapa = etapa;
            assert_eq!(planejar_recuperacao(&journal, Some(Slot::A)).unwrap(), Recuperacao::CancelarPreparacao);
            assert!(planejar_recuperacao(&journal, Some(Slot::B)).is_err());
        }
        journal.etapa = Etapa::SeletorTrocado;
        assert_eq!(planejar_recuperacao(&journal, Some(Slot::B)).unwrap(), Recuperacao::ResolverPublicacao);
        assert!(planejar_recuperacao(&journal, Some(Slot::A)).is_err());
        journal.etapa = Etapa::SeletorRestaurado;
        assert_eq!(planejar_recuperacao(&journal, Some(Slot::A)).unwrap(), Recuperacao::TerminarReversao);
        assert!(planejar_recuperacao(&journal, Some(Slot::B)).is_err());
        journal.etapa = Etapa::Concluida;
        journal.estado.fase = Fase::Aceita;
        assert_eq!(planejar_recuperacao(&journal, Some(Slot::B)).unwrap(), Recuperacao::Concluida);
        assert!(planejar_recuperacao(&journal, Some(Slot::A)).is_err());
        journal.estado.fase = Fase::Revertida;
        assert_eq!(planejar_recuperacao(&journal, Some(Slot::A)).unwrap(), Recuperacao::Concluida);
        assert!(planejar_recuperacao(&journal, Some(Slot::B)).is_err());
        journal.estado.fase = Fase::Expirada;
        assert!(planejar_recuperacao(&journal, Some(Slot::A)).is_err());
    }

    #[test]
    fn recuperacao_recusa_seletor_incompatível() {
        let mut journal = Journal {
            estado: Estado { marca: 7, piso: 3, fase: Fase::Pendente { anterior: Some(6), prazo: 200 } },
            etapa: Etapa::AnteriorDesativado,
            transacao: "c".repeat(64),
            sequencia_nova: 7,
            publicado_em: Some(100),
            sequencia_anterior: Some(6),
            slot_anterior: Some(Slot::A),
            slot_novo: Slot::B,
            manifesto_sha256: "a".repeat(64),
            snapshot_sha256: "b".repeat(64),
        };
        assert_eq!(planejar_recuperacao(&journal, Some(Slot::A)).unwrap(), Recuperacao::RestabelecerAnterior);
        assert_eq!(planejar_recuperacao(&journal, Some(Slot::B)).unwrap(), Recuperacao::ResolverPublicacao);
        assert!(planejar_recuperacao(&journal, None).is_err());
        journal.etapa = Etapa::DesativacaoIniciada;
        assert_eq!(planejar_recuperacao(&journal, Some(Slot::A)).unwrap(), Recuperacao::RestabelecerAnterior);
        assert!(planejar_recuperacao(&journal, Some(Slot::B)).is_err());
        journal.etapa = Etapa::Pendente;
        assert_eq!(planejar_recuperacao(&journal, Some(Slot::B)).unwrap(), Recuperacao::Pendente);
        assert!(planejar_recuperacao(&journal, Some(Slot::A)).is_err());
        journal.etapa = Etapa::ReversaoIniciada;
        assert_eq!(planejar_recuperacao(&journal, Some(Slot::A)).unwrap(), Recuperacao::TerminarReversao);
        assert_eq!(planejar_recuperacao(&journal, Some(Slot::B)).unwrap(), Recuperacao::TerminarReversao);
        journal.etapa = Etapa::CancelamentoIniciado;
        assert_eq!(planejar_recuperacao(&journal, Some(Slot::A)).unwrap(), Recuperacao::RestabelecerAnterior);
        assert_eq!(planejar_recuperacao(&journal, Some(Slot::B)).unwrap(), Recuperacao::RestabelecerAnterior);
        assert!(planejar_recuperacao(&journal, None).is_err());
        journal.etapa = Etapa::Cancelada;
        journal.estado = Estado { marca: 6, piso: 3, fase: Fase::Aceita };
        assert_eq!(planejar_recuperacao(&journal, Some(Slot::A)).unwrap(), Recuperacao::Concluida);
        assert!(planejar_recuperacao(&journal, Some(Slot::B)).is_err());
        journal.estado.marca = 7;
        assert!(planejar_recuperacao(&journal, Some(Slot::A)).is_err());
    }
}
