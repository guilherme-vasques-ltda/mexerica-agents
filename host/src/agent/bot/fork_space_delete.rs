//! D543: a decisão sobre trabalho vivo pertence ao ator, como no D545.
use super::{Actor, Queued};

impl Actor {
    pub(super) fn space_delete_busy(&self, chats: &[String], all: bool) -> bool {
        let belongs = |chat: &str| all || chats.iter().any(|c| c == chat);
        ((self.turn.is_some() || self.pending_admission.is_some() || self.finishing.is_some())
            && belongs(&self.lane.chat))
            || self.waiting_lane().is_some_and(|lane| belongs(&lane.chat))
            || self.queue.iter().any(|q| match q {
                Queued::User { lane, .. } => belongs(&lane.chat),
                Queued::Group(turn) => belongs(&turn.lane.chat),
                Queued::RestoreUser { entry_id, .. } => {
                    all || self.hub.store.entry(entry_id).is_some_and(|entry| belongs(&entry.bot_id))
                }
                // Rotinas e pedidos sem lane executam na principal do dono.
                Queued::Routine(_) | Queued::BotRequest(_) => belongs(&self.cfg.id),
                Queued::Compact { manual, .. } => {
                    belongs(manual.as_ref().map_or(self.cfg.id.as_str(), |m| m.target.lane.chat.as_str()))
                }
            })
            || self.ceiling_holds_belong(&belongs)
            || self.hub.team.pending_from_chats(&self.cfg.id, chats, all)
    }
}
