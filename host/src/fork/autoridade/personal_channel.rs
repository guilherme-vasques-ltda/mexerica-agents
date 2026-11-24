//! PR-B: variante privada do MESMO Original, sem promoção a Caller/contas.
use super::{Autoridade, Original};
use crate::{
    fork::{
        donna_fixa::metodos::{Classe, METODOS_VINCULO_PESSOAL},
        personal_link::channel::{AuthenticatedIngress, FatosSessao},
    },
    hub::Hub,
};
use anyhow::{Result, ensure};
use rusqlite::Connection;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IntencaoCanal {
    SessionOpen,
    LinkStatus,
    SessionClose,
}
#[derive(Debug)]
pub(crate) struct CapturaCanal {
    fatos: Arc<FatosSessao>,
    intencao: IntencaoCanal,
}
impl CapturaCanal {
    pub(super) fn conferir(&self, db: &Connection) -> Result<()> {
        self.fatos.conferir(db)
    }
}
impl Autoridade {
    pub(crate) fn capturar_canal(hub: &Hub, ingresso: &AuthenticatedIngress, intencao: IntencaoCanal) -> Result<Self> {
        // Inventário próprio só classifica/recusa; não substitui ingresso, intenção ou as guardas abaixo.
        let metodo = match intencao {
            IntencaoCanal::SessionOpen => "session.open",
            IntencaoCanal::LinkStatus => "link.status",
            IntencaoCanal::SessionClose => "session.close",
        };
        let classe = METODOS_VINCULO_PESSOAL.iter().find_map(|(nome, classe)| (*nome == metodo).then_some(*classe));
        ensure!(classe == Some(Classe::VinculoPessoal), "not_allowed: classe do canal pessoal");
        let original = Self {
            original: Original::Canal(Arc::new(CapturaCanal { fatos: ingresso.fatos(), intencao })),
            captura: Arc::new(()),
            sessao: None,
        };
        original.with_db(hub, None, |_| Ok(()))?;
        Ok(original)
    }
    pub(crate) fn exigir_canal(&self, intencao: IntencaoCanal) -> Result<()> {
        ensure!(matches!(&self.original,Original::Canal(c) if c.intencao==intencao), "not_allowed: intenção canal");
        Ok(())
    }
}
