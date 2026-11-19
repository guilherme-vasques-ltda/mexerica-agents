//! Variante privada D430 do `Original` (PR-A): só o ingresso do procedimento administrativo
//! inicial a constrói, com intenção fechada. Não deriva de `Caller`, não é serializável, não
//! alcança dispatcher/`engine.accounts`/Control e não recebe concessão nova após a captura.
use super::{Autoridade, Original};
use crate::{fork::personal_link::procedure::IngressoInicial, hub::Hub};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IntencaoInicial {
    AceiteRaiz,
    ConfirmaPersistencia,
    ConcedeGestao,
}

/// Fatos capturados uma vez; a guarda pré/pós confere a MESMA sessão/instância/runtime.
/// O alvo (raiz/desafio) é CAS próprio do escritor, fora desta guarda.
#[derive(Debug)]
pub(crate) struct CapturaInicial {
    instalacao: String,
    hub: String,
    runtime: String,
    sessao: Arc<AtomicBool>,
    instancia: Arc<AtomicBool>,
    intencao: IntencaoInicial,
}

impl CapturaInicial {
    /// Só a Connection recebida e RAM síncrona; sem Store/Cloud/FS/rede.
    pub(super) fn conferir(&self, db: &Connection) -> Result<()> {
        // Vida real: a tarefa da conexão (EOF) e a parada da instância desligam estas bandeiras fora do worker.
        ensure!(
            self.sessao.load(Ordering::Acquire) && self.instancia.load(Ordering::Acquire),
            "not_allowed: sessão administrativa ou instância encerrada"
        );
        let instalacao: Option<(String, String)> = db
            .query_row("SELECT install_id,hub_key FROM personal_link_installation WHERE singleton=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        ensure!(
            instalacao.as_ref().is_some_and(|(i, h)| *i == self.instalacao && *h == self.hub),
            "not_allowed: instalação D430 divergente"
        );
        ensure!(
            crate::fork::personal_link::writer::runtime_atual(db)?.as_deref() == Some(self.runtime.as_str()),
            "not_allowed: runtime D430 encerrado"
        );
        Ok(())
    }
}

impl Autoridade {
    /// Captura ANTES de fila/await de negócio; revalida já na Connection original.
    pub(crate) fn capturar_inicial(hub: &Hub, ingresso: &IngressoInicial, intencao: IntencaoInicial) -> Result<Self> {
        ensure!(hub.identity.sign_pub_b64() == ingresso.hub(), "not_allowed: H divergente da instância");
        let original = Self {
            original: Original::Inicial(Arc::new(CapturaInicial {
                instalacao: ingresso.instalacao().to_owned(),
                hub: ingresso.hub().to_owned(),
                runtime: ingresso.runtime().to_owned(),
                sessao: ingresso.sessao(),
                instancia: ingresso.instancia(),
                intencao,
            })),
            captura: Arc::new(()),
        };
        original.with_db(hub, None, |_| Ok(()))?;
        Ok(original)
    }

    /// O escritor exige a intenção exata; qualquer outra Autoridade recusa.
    pub(crate) fn exigir_inicial(&self, intencao: IntencaoInicial) -> Result<()> {
        ensure!(
            matches!(&self.original, Original::Inicial(c) if c.intencao == intencao),
            "not_allowed: intenção administrativa D430 divergente"
        );
        Ok(())
    }
}
