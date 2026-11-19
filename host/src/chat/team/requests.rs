//! Admission, capacity and the wait graph for bot requests.

use crate::LockExt;
use anyhow::{Result, bail};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use tokio::sync::watch;

use super::{BotRequest, MAX_PENDING_MESSAGES};
use crate::fork::cadeias::modelo::Recibo;

const MAX_REQUEST_HOPS: u8 = 8;
pub(super) const MAX_PENDING_MESSAGES_PER_SENDER: usize = 16;

pub(super) struct Request {
    from: String,
    to: String,
    /// Sessões PR5: a conversa e a sessão visível de quem pediu; encerrar ou recomeçar cancela só os pedidos dela.
    lane: crate::store::Lane,
    cancel: watch::Sender<bool>,
}

/// The wait graph includes queued requests, not only running recipients.
/// Reserving an edge and checking for cycles happen under the same lock.
#[derive(Default)]
pub struct Requests(pub(super) Mutex<RequestState>);

#[derive(Default)]
pub(super) struct RequestState {
    pub(super) pending: HashMap<String, Request>,
    pub(super) active: HashMap<String, u8>,
    /// Recibo D220 do turno atual (repasse herdado ou Run de Rotina); lido com os saltos.
    pub(super) chains: HashMap<String, Recibo>,
    pub(super) messages: usize,
    pub(super) messages_by_sender: HashMap<String, usize>,
}

impl Requests {
    #[cfg(test)]
    pub(crate) fn test_pending_counts(&self) -> (usize, usize) {
        let state = self.0.locked();
        (state.pending.len(), state.messages)
    }

    /// Sessões PR5 (testes do ator): um pedido pendente feito a partir de `lane`.
    #[cfg(test)]
    pub(crate) fn begin_for_test(
        &self,
        id: &str,
        from: &str,
        to: &str,
        lane: &crate::store::Lane,
    ) -> Result<watch::Receiver<bool>> {
        self.begin_in(id, from, to, lane).map(|(rx, _, _)| rx)
    }

    #[cfg(test)]
    pub(crate) fn clear_for_test(&self) {
        self.0.locked().pending.clear();
    }

    #[cfg(test)]
    pub(super) fn reserve_message(&self, from: &str) -> Result<u8> {
        self.reserve_message_in(from).map(|(hops, _)| hops)
    }

    pub(super) fn reserve_message_in(&self, from: &str) -> Result<(u8, Option<Recibo>)> {
        let mut state = self.0.locked();
        let hops = state.next_hop(from)?;
        let chain = state.chains.get(from).cloned();
        if state.messages >= MAX_PENDING_MESSAGES {
            bail!("too many pending bot messages");
        }
        let count = state.messages_by_sender.entry(from.to_owned()).or_default();
        if *count >= MAX_PENDING_MESSAGES_PER_SENDER {
            bail!("too many pending bot messages from this bot (limit {MAX_PENDING_MESSAGES_PER_SENDER})");
        }
        *count += 1;
        state.messages += 1;
        Ok((hops, chain))
    }

    pub(super) fn release_message(&self, from: &str) {
        let mut state = self.0.locked();
        state.messages -= 1;
        if let Some(count) = state.messages_by_sender.get_mut(from) {
            *count -= 1;
            if *count == 0 {
                state.messages_by_sender.remove(from);
            }
        }
    }

    #[cfg(test)]
    pub(super) fn begin(&self, id: &str, from: &str, to: &str) -> Result<(watch::Receiver<bool>, u8)> {
        self.begin_in(id, from, to, &crate::store::Lane::main(from)).map(|(rx, hops, _)| (rx, hops))
    }

    pub(super) fn begin_in(
        &self,
        id: &str,
        from: &str,
        to: &str,
        lane: &crate::store::Lane,
    ) -> Result<(watch::Receiver<bool>, u8, Option<Recibo>)> {
        let mut state = self.0.locked();
        let hops = state.next_hop(from)?;
        let chain = state.chains.get(from).cloned();
        let requests = &mut state.pending;
        if from == to {
            bail!("a bot cannot ask itself");
        }
        if requests.len() >= 64 {
            bail!("too many pending bot requests");
        }
        if requests.values().any(|r| r.from == from && r.to == to) {
            bail!("already waiting for this bot");
        }
        let mut pending = vec![to];
        let mut seen = HashSet::new();
        while let Some(bot) = pending.pop() {
            if bot == from {
                bail!("this request would make bots wait for each other; finish the current request first");
            }
            if seen.insert(bot) {
                pending.extend(requests.values().filter(|r| r.from == bot).map(|r| r.to.as_str()));
            }
        }
        let (cancel, rx) = watch::channel(false);
        let request = Request { from: from.to_owned(), to: to.to_owned(), lane: lane.clone(), cancel };
        requests.insert(id.to_owned(), request);
        Ok((rx, hops, chain))
    }

    pub fn cancel_from(&self, bot: &str) {
        let mut state = self.0.locked();
        state.active.remove(bot);
        state.chains.remove(bot);
        for request in state.pending.values().filter(|r| r.from == bot) {
            request.cancel.send_replace(true);
        }
    }

    /// Sessões PR5: fim do turno de `lane` (conclusão, parada por sessão, falha do preparo). Limpa o estado do turno
    /// do bot (saltos e recibo) e cancela só os pedidos feitos a partir dessa lane; os de outras sessões continuam.
    /// O `stop` global segue com `cancel_from`.
    pub(crate) fn cancel_turn_lane(&self, bot: &str, lane: &crate::store::Lane) {
        let mut state = self.0.locked();
        state.active.remove(bot);
        state.chains.remove(bot);
        for request in state.pending.values().filter(|r| r.from == bot && r.lane == *lane) {
            request.cancel.send_replace(true);
        }
    }

    /// Sessões PR5: cancela só os pedidos que `bot` fez a partir de `lane` (o turno em andamento não é tocado).
    pub(crate) fn cancel_from_session(&self, bot: &str, lane: &crate::store::Lane) {
        let state = self.0.locked();
        for request in state.pending.values().filter(|r| r.from == bot && r.lane == *lane) {
            request.cancel.send_replace(true);
        }
    }

    /// D543: pedidos originados nas conversas que serão apagadas, inclusive extras.
    pub(crate) fn pending_from_chats(&self, bot: &str, chats: &[String], all: bool) -> bool {
        self.0.locked().pending.values().any(|r| r.from == bot && (all || chats.contains(&r.lane.chat)))
    }

    /// Sessões PR5 (P4): há pedido pendente feito por `bot` a partir de `lane`?
    pub(crate) fn pending_from_session(&self, bot: &str, lane: &crate::store::Lane) -> bool {
        self.0.locked().pending.values().any(|r| r.from == bot && r.lane == *lane)
    }

    #[cfg(test)]
    pub fn start_turn(&self, bot: &str, request: Option<&BotRequest>) {
        self.start_turn_in(bot, request, None);
    }

    /// O pedido herdado prevalece; sem ele, o recibo real da Run de Rotina do turno.
    pub(crate) fn start_turn_in(&self, bot: &str, request: Option<&BotRequest>, routine: Option<&Recibo>) {
        let mut state = self.0.locked();
        state.active.insert(bot.to_owned(), request.map_or(0, |r| r.hops));
        match request.and_then(|r| r.chain.as_ref()).or(routine) {
            Some(chain) => state.chains.insert(bot.to_owned(), chain.clone()),
            None => state.chains.remove(bot),
        };
    }

    pub fn cancel_bot(&self, bot: &str) {
        let mut state = self.0.locked();
        state.active.remove(bot);
        state.chains.remove(bot);
        for request in state.pending.values().filter(|r| r.from == bot || r.to == bot) {
            request.cancel.send_replace(true);
        }
    }
}

impl RequestState {
    fn next_hop(&self, from: &str) -> Result<u8> {
        let hops = *self.active.get(from).ok_or_else(|| anyhow::anyhow!("the requesting bot is no longer working"))?;
        if hops >= MAX_REQUEST_HOPS {
            bail!("bot request hop limit ({MAX_REQUEST_HOPS}) reached; ask the user before starting another chain");
        }
        Ok(hops + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhausted_chain_rejects_messages_and_asks_without_reserving_capacity() {
        let requests = Requests::default();
        requests.0.locked().active.insert("a".into(), MAX_REQUEST_HOPS);
        let message = requests.reserve_message("a");
        assert!(message.unwrap_err().to_string().contains("hop limit"));
        let ask = requests.begin("ask", "a", "b");
        assert!(ask.unwrap_err().to_string().contains("hop limit"));
        let state = requests.0.locked();
        assert_eq!(state.messages, 0);
        assert!(state.pending.is_empty());
        drop(state);
        requests.start_turn("a", None);
        assert_eq!(requests.reserve_message("a").unwrap(), 1);
    }

    #[test]
    fn last_allowed_handoff_is_admitted() {
        let requests = Requests::default();
        requests.0.locked().active.insert("a".into(), MAX_REQUEST_HOPS - 1);
        assert_eq!(requests.reserve_message("a").unwrap(), MAX_REQUEST_HOPS);
        let (_, hops) = requests.begin("ask", "a", "b").unwrap();
        assert_eq!(hops, MAX_REQUEST_HOPS);
    }

    #[test]
    fn sender_quota_leaves_capacity_for_other_bots() {
        let requests = Requests::default();
        for bot in ["a", "b"] {
            requests.start_turn(bot, None);
        }
        for _ in 0..MAX_PENDING_MESSAGES_PER_SENDER {
            requests.reserve_message("a").unwrap();
        }
        let result = requests.reserve_message("a");
        assert!(result.unwrap_err().to_string().contains("from this bot (limit 16)"));
        assert_eq!(requests.0.locked().messages, MAX_PENDING_MESSAGES_PER_SENDER);
        requests.reserve_message("b").unwrap();
        // Ask capacity is independent of message quotas.
        requests.begin("ask", "a", "b").unwrap();
        requests.release_message("a");
        requests.reserve_message("a").unwrap();
        for _ in 0..MAX_PENDING_MESSAGES_PER_SENDER {
            requests.release_message("a");
        }
        requests.release_message("b");
        let state = requests.0.locked();
        assert_eq!(state.messages, 0);
        assert!(state.messages_by_sender.is_empty());
    }

    #[test]
    fn global_capacity_still_bounds_messages_from_multiple_senders() {
        let requests = Requests::default();
        for bot in ["a", "b", "c", "d"] {
            requests.start_turn(bot, None);
            for _ in 0..MAX_PENDING_MESSAGES_PER_SENDER {
                requests.reserve_message(bot).unwrap();
            }
        }
        assert_eq!(requests.0.locked().messages, MAX_PENDING_MESSAGES);
        requests.start_turn("e", None);
        let result = requests.reserve_message("e");
        assert_eq!(result.unwrap_err().to_string(), "too many pending bot messages");
        assert!(!requests.0.locked().messages_by_sender.contains_key("e"));
        requests.release_message("a");
        requests.reserve_message("e").unwrap();
        assert_eq!(requests.0.locked().messages, MAX_PENDING_MESSAGES);
    }

    #[test]
    fn concurrent_reservations_cannot_exceed_the_sender_quota() {
        let requests = Requests::default();
        requests.start_turn("a", None);
        let barrier = std::sync::Barrier::new(MAX_PENDING_MESSAGES);
        let admitted = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..MAX_PENDING_MESSAGES)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        requests.reserve_message("a").is_ok()
                    })
                })
                .collect();
            handles.into_iter().map(|handle| handle.join().unwrap()).filter(|admitted| *admitted).count()
        });
        assert_eq!(admitted, MAX_PENDING_MESSAGES_PER_SENDER);
        assert_eq!(requests.0.locked().messages, admitted);
        for _ in 0..admitted {
            requests.release_message("a");
        }
        assert!(requests.0.locked().messages_by_sender.is_empty());
    }
}
