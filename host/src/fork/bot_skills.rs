//! Preparo interno do perfil, não autoridade, admissão ou entrega revogável de Skills.
use crate::{
    LockExt,
    fork::{
        bot_admission::{self, Origin, Ticket},
        sessoes::bindings::{self, BindingRef},
        skill_catalog::preparo,
        skill_selection::Observed,
    },
    store::{Lane, Store},
};
use anyhow::{Result, anyhow, ensure};
use rusqlite::Connection;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct Prepared {
    bot: String,
    ticket: Ticket,
    lane: Lane,
    visible: String,
    origin: Origin,
    entries: Vec<String>,
    selection: Option<Observed>,
    catalog: Option<Arc<preparo::Observed>>,
    binding: Option<BindingRef>,
    execution: Option<bindings::ExecutionRef>,
}
impl Prepared {
    pub(crate) fn capture(
        store: &Store,
        bot: &str,
        ticket: &Ticket,
        lane: &Lane,
        visible: String,
        fresh: bool,
    ) -> Result<Self> {
        let (record, selection, mut catalog) = {
            let mut db = store.db.locked();
            let tx = db.transaction()?;
            let record = bot_admission::prepared_on(&tx, bot, ticket)?;
            let selection = fresh.then(|| Observed::read(&tx, bot)).and_then(Result::ok);
            let catalog = selection
                .as_ref()
                .filter(|s| !s.skills.is_empty())
                .and_then(|s| preparo::Observed::read(&tx, &s.skills).ok());
            (record, selection, catalog)
        };
        let mut prepared = Self {
            bot: bot.into(),
            ticket: ticket.clone(),
            lane: lane.clone(),
            visible,
            origin: record.origin,
            entries: record.entry_ids,
            selection,
            catalog: None,
            binding: None,
            execution: None,
        };
        prepared.check_origin(&store.db.locked())?;
        if let Some(catalog) = &mut catalog {
            catalog.read_documents(store);
        }
        prepared.catalog = catalog.map(Arc::new);
        // Divergência de Skills conserva indisponibilidade; origem divergente nunca degrada.
        let db = store.db.locked();
        prepared.check_origin(&db)?;
        if prepared.check_sources(&db).is_err() {
            prepared.selection = None;
            prepared.catalog = None;
        }
        Ok(prepared)
    }
    pub(crate) fn owns(&self, ticket: &Ticket) -> bool {
        self.ticket == *ticket
    }
    pub(crate) fn check_destination(&self, bot: &str, lane: &Lane, visible: &str) -> Result<()> {
        ensure!(
            self.bot == bot
                && self.lane.chat == lane.chat
                && self.lane.session == lane.session
                && self.visible == visible,
            "skills_preparation_destination_changed"
        );
        Ok(())
    }
    pub(crate) fn check_origin(&self, c: &Connection) -> Result<()> {
        let record = bot_admission::prepared_on(c, &self.bot, &self.ticket)?;
        ensure!(
            record.conversation_id == self.lane.chat
                && record.origin == self.origin
                && record.entry_ids == self.entries,
            "skills_preparation_origin_changed"
        );
        if record.origin == Origin::Human {
            for id in &self.entries {
                let entry = Store::entry_on(c, id)?.ok_or_else(|| anyhow!("skills_preparation_input_missing"))?;
                ensure!(
                    entry.bot_id == self.lane.chat && entry.session_id == self.lane.session_id(),
                    "skills_preparation_destination_changed"
                );
            }
        }
        if let Some(binding) = &self.binding {
            let current = bindings::resolve(c, binding.id.as_str(), &self.bot)?;
            ensure!(
                current.session_ref == binding.session_ref && current.acp_session == binding.acp_session,
                "skills_preparation_binding_changed"
            );
        }
        Ok(())
    }
    fn check_sources(&self, c: &Connection) -> Result<()> {
        let selection = self.selection.as_ref().ok_or_else(|| anyhow!("skills_preparation_unavailable"))?;
        selection.check(c, &self.bot)?;
        if !selection.skills.is_empty() {
            self.catalog.as_ref().ok_or_else(|| anyhow!("skills_preparation_unavailable"))?.check(c)?;
        }
        Ok(())
    }
    /// Retry de setup reutiliza o ID já observado; nunca emite ou resolve um substituto.
    pub(crate) fn pending_binding(&self, store: &Store) -> Result<Option<BindingRef>> {
        self.check_origin(&store.db.locked())?;
        Ok(self.binding.as_ref().filter(|binding| binding.acp_session.is_none()).cloned())
    }
    pub(crate) fn bind(&mut self, store: &Store, binding: &BindingRef) -> Result<()> {
        self.check_origin(&store.db.locked())?;
        ensure!(
            binding.bot == self.bot
                && binding.session_ref.chat_id == self.lane.chat
                && binding.session_ref.id == self.visible,
            "skills_preparation_destination_changed"
        );
        if let Some(original) = &self.binding {
            ensure!(
                original.id == binding.id && original.acp_session == binding.acp_session,
                "skills_preparation_binding_changed"
            );
        } else {
            self.binding = Some(binding.clone());
        }
        self.check_origin(&store.db.locked())
    }
    /// Completa apenas a ACP do vínculo original emitido antes de new/load.
    pub(crate) fn complete(&mut self, store: &Store, binding: &BindingRef, acp: &str) -> Result<()> {
        let original = self.binding.as_mut().ok_or_else(|| anyhow!("skills_preparation_binding_missing"))?;
        ensure!(
            original.id == binding.id && original.acp_session.as_deref().is_none_or(|s| s == acp),
            "skills_preparation_binding_changed"
        );
        original.acp_session = Some(acp.into());
        self.check_origin(&store.db.locked())
    }
    /// FS primeiro; a última conferência só usa a Connection. Não certifica o envio futuro.
    pub(crate) fn project(&self, store: &Store) -> Result<Option<String>> {
        self.check_origin(&store.db.locked())?;
        let block = self.catalog.as_ref().and_then(|catalog| catalog.project(store));
        let db = store.db.locked();
        self.check_origin(&db)?;
        if self.check_sources(&db).is_err() {
            return Ok(Some(preparo::WARNING.into()));
        }
        Ok(block)
    }
}

/// Limite operacional da montagem; não anuncia capacidade de leitura revogável.
pub(crate) const INSTRUCTIONS: &str = "No Pessoal, o Hub conserva uma observação interna de Skills durante o preparo e monta perfil e registro da sessão do mesmo bloco. Divergência detectada em montagem nova omite as skills afetadas e avisa; não procure seus arquivos. Isso não torna caminhos imutáveis, não revoga conteúdo já recebido e não certifica cache, histórico, /id já expandido ou prompt raw. Não anuncie leitura revogável, isolamento por bot ou publicação 121B; essas capacidades continuam bloqueadas.";

#[cfg(test)]
mod tests;

pub(crate) mod read;
