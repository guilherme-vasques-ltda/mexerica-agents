//! Ganchos de preparo interno: a mesma observação acompanha o Ticket, inclusive Pending.
use super::{Actor, Conn, Instructions};
use crate::{
    LockExt,
    chat::context::{self, RenderedProfile},
    fork::{bot_admission::Ticket, bot_skills::Prepared, sessoes::bindings::BindingRef},
    store::{BotConfig, Store},
};
use anyhow::{Result, anyhow, ensure};

pub(super) fn project(store: &Store, cfg: &BotConfig, prepared: Option<&Prepared>) -> Result<Option<String>> {
    if cfg.context.is_some() {
        return Ok(crate::market::skills_brief(store, &cfg.skills));
    }
    match prepared {
        Some(prepared) => prepared.project(store),
        None => Ok(Some(crate::fork::skill_catalog::preparo::WARNING.into())),
    }
}
impl Actor {
    /// Consumer candidato privado: ID e sessão ACP exatos, nunca um caminho.
    pub(super) fn on_skill_read(&mut self, id: serde_json::Value, params: serde_json::Value) {
        let waiting = self.waiting_for_reset();
        let Some(conn) = &mut self.conn else { return };
        let request = serde_json::from_value::<crate::fork::bot_skills::read::Request>(params);
        let valid = self.turn.is_some() && !self.stop_requested && !waiting && self.cfg.context.is_none();
        // A tarefa pertence à conexão: nunca sobrevive ao seu fechamento, nem
        // consulta o ator para recapturar Prepared, Ticket ou sessão sucessora.
        let (prepared, hub, acp) = (self.profile_skills.clone(), self.hub.clone(), conn.acp.clone());
        conn.skill_reads.spawn(async move {
            let result = match (valid, prepared, request) {
                (true, Some(prepared), Ok(request)) => prepared.deliver(&hub, &acp, id.clone(), &request).await,
                _ => Err(anyhow!("skills_read_unavailable")),
            };
            if result.is_err() {
                let _ = acp.respond_error(id, -32602, "skills_read_unavailable").await;
            }
        });
    }

    pub(super) fn prepare_profile_skills(&mut self, fresh: bool) -> Result<()> {
        if self.cfg.context.is_some() {
            return Ok(());
        }
        let ticket = self.dispatch.as_ref().ok_or_else(|| anyhow!("admission_dispatch_missing"))?;
        if let Some(prepared) = &self.profile_skills {
            ensure!(prepared.owns(ticket), "skills_preparation_origin_changed");
            return self.check_profile_origin();
        }
        let visible = crate::fork::sessoes::exposure::visible(self.cfg.session_owner(), self.slot().as_deref());
        self.profile_skills =
            Some(Prepared::capture(&self.hub.store, &self.cfg.id, ticket, &self.lane, visible, fresh)?);
        Ok(())
    }
    pub(super) fn check_profile_origin(&self) -> Result<()> {
        if self.cfg.context.is_some() {
            return Ok(());
        }
        if let Some(prepared) = &self.profile_skills {
            ensure!(
                self.dispatch.as_ref().is_some_and(|ticket| prepared.owns(ticket)),
                "skills_preparation_origin_changed"
            );
            let visible = crate::fork::sessoes::exposure::visible(self.cfg.session_owner(), self.slot().as_deref());
            prepared.check_destination(&self.cfg.id, &self.lane, &visible)?;
            prepared.check_origin(&self.hub.store.db.locked())?;
        }
        Ok(())
    }
    pub(super) fn pending_profile_binding(&self) -> Result<Option<BindingRef>> {
        self.check_profile_origin()?;
        self.profile_skills
            .as_ref()
            .map(|prepared| prepared.pending_binding(&self.hub.store))
            .transpose()
            .map(Option::flatten)
    }
    pub(super) fn bind_profile_skills(&mut self, binding: &BindingRef) -> Result<()> {
        self.check_profile_origin()?;
        if let Some(prepared) = &mut self.profile_skills {
            prepared.bind(&self.hub.store, binding)?;
        }
        Ok(())
    }
    pub(super) fn complete_profile_skills(&mut self, binding: &BindingRef, acp: &str) -> Result<()> {
        if let Some(prepared) = &mut self.profile_skills {
            prepared.complete(&self.hub.store, binding, acp)?;
        }
        self.check_profile_origin()
    }
    pub(super) fn clear_profile_skills(&mut self, ticket: &Ticket) {
        if self.profile_skills.as_ref().is_some_and(|prepared| prepared.owns(ticket)) {
            if let Some(conn) = &self.conn {
                conn.acp.cancel_scoped();
            }
            self.profile_skills = None;
        }
    }
    pub(super) fn project_profile_skills(&self) -> Result<Option<String>> {
        self.check_profile_origin()?;
        project(&self.hub.store, &self.cfg, self.profile_skills.as_ref())
    }
    pub(super) async fn render_profile(&self) -> Result<(String, RenderedProfile)> {
        self.check_profile_origin()?;
        let (hub, cfg, prepared) = (self.hub.clone(), self.cfg.clone(), self.profile_skills.clone());
        let instructions = self.conn.as_ref().map_or(Instructions::FirstMessage, Conn::instructions);
        let query = self.turn_text.clone();
        let pair = tokio::task::spawn_blocking(move || -> Result<_> {
            let skills = project(&hub.store, &cfg, prepared.as_ref())?;
            Ok((
                crate::chat::memory::current_revision(&hub.store, cfg.memory_id()),
                context::render_with_skills(&hub.store, &cfg, instructions, skills, query.as_deref()),
            ))
        })
        .await??;
        self.check_profile_origin()?;
        Ok(pair)
    }
}

impl Conn {
    /// Cancela e recolhe os donos de I/O antes de aguardar o mutex de stdin.
    pub(super) async fn kill(mut self) {
        self.skill_reads.shutdown().await;
        self.acp.kill().await;
    }
}
