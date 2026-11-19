//! Autoridade de execução emitida pelo Hub, distinta do Context legado do 03.
use super::operacao::digest;
use crate::{
    hub::{BotStatus, Hub, Runtime},
    store::Lane,
};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ContextoExecucao {
    pub hub: String,
    pub channel: String,
    pub bot: String,
    pub lane: Lane,
    pub acp_session: String,
    /// Legado ausente permanece legível, mas nunca autoriza execução.
    pub session_binding: Option<String>,
    pub turn: i64,
    pub run: String,
    pub message: String,
    pub config_digest: String,
}
impl ContextoExecucao {
    pub(super) fn capture(hub: &Hub, channel: &str, bot: &str, runtime: &Runtime, c: &Connection) -> Result<Self> {
        let lane = runtime.lane.clone().ok_or_else(|| anyhow!("Sem conversa ativa"))?;
        let message = runtime.message.clone().ok_or_else(|| anyhow!("Exige turno humano"))?;
        let acp_session = runtime.session.clone().ok_or_else(|| anyhow!("Sem sessão ACP ativa"))?;
        let run = runtime.memory_turn_token.clone().ok_or_else(|| anyhow!("Sem execução humana ativa"))?;
        let binding = crate::fork::sessoes::bindings::for_acp_on(c, bot, &acp_session)?;
        let (raw, turn): (String, i64) = c.query_row(
            "SELECT b.config,e.turn FROM bots b JOIN entries e ON e.id=?2 WHERE b.id=?1 AND b.deleted=0",
            params![bot, message],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let this = Self {
            hub: hub.host_id.clone(),
            channel: channel.into(),
            bot: bot.into(),
            lane,
            acp_session,
            session_binding: Some(binding.id.as_str().into()),
            turn,
            run,
            message,
            config_digest: digest(&serde_json::from_str::<Value>(&raw)?),
        };
        this.check(hub, runtime, c)?;
        Ok(this)
    }
    pub(super) fn check(&self, hub: &Hub, runtime: &Runtime, c: &Connection) -> Result<()> {
        ensure!(self.hub == hub.host_id && self.lane.chat == self.bot, "Conversa incorreta");
        ensure!(matches!(runtime.status, BotStatus::Working | BotStatus::NeedsInput), "Turno encerrado");
        ensure!(
            runtime.lane.as_ref() == Some(&self.lane)
                && runtime.session.as_ref() == Some(&self.acp_session)
                && runtime.message.as_ref() == Some(&self.message)
                && runtime.memory_turn_token.as_ref() == Some(&self.run)
                && !runtime.memory_exposure_error,
            "Contexto revogado"
        );
        let binding = self.session_binding.as_deref().ok_or_else(|| anyhow!("Vínculo original ausente"))?;
        let binding = crate::fork::sessoes::bindings::resolve(c, binding, &self.bot)?;
        crate::fork::sessoes::bindings::assert_binding_live(c, &binding)?;
        ensure!(
            binding.acp_session.as_deref() == Some(self.acp_session.as_str())
                && binding.session_ref.chat_id == self.lane.chat
                && binding.session_ref.id == self.lane.session_id(),
            "Vínculo de sessão incorreto"
        );
        let (raw, deleted): (String, bool) =
            c.query_row("SELECT config,deleted FROM bots WHERE id=?1", [&self.bot], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let config: Value = serde_json::from_str(&raw)?;
        ensure!(
            !deleted
                && digest(&config) == self.config_digest
                && config["spaceId"].as_str().unwrap_or(crate::fork::spaces::PERSONAL) == crate::fork::spaces::PERSONAL
                && config["kind"] != "group",
            "Perfil ou espaço mudou"
        );
        let entry = crate::store::conectores::human_entry(c, &self.message)?;
        ensure!(
            entry.is_user_authored()
                && entry.bot_id == self.bot
                && entry.turn == self.turn
                && entry.session_id == self.lane.session_id(),
            "Origem não humana ou sessão incorreta"
        );
        let bound: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM donna_session_acp WHERE bot=?1 AND acp_session=?2 AND session=?3)",
            params![self.bot, self.acp_session, self.lane.session_id()],
            |r| r.get(0),
        )?;
        ensure!(bound, "Vínculo de exposição ACP indisponível");
        Ok(())
    }
}
