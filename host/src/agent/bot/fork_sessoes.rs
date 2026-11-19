//! Sessões PR5 no ator: encerrar uma extra, o fechamento automático (P4), o recomeço só da principal e o cartão de
//! continuação consumido no permit. O ator é quem inicia turnos: a decisão sobre o trabalho em memória e a Tx que
//! reconfere o durável acontecem sem `await` entre elas. Travas na ordem da base: `emit_lock` → (Cloud →) DB; a
//! autoridade ORIGINAL chega explícita no comando e grava por `with_db`; eventos só depois do commit.
//!
//! Isolamento: tudo aqui é por sessão. Nada chama `cancel_turn` (que revoga as pontes do bot inteiro) nem
//! `team.cancel_from` (que cancela os pedidos de todas as sessões do bot); só as pontes do slot e os pedidos da lane.
use super::{Actor, Queued, session::EXTRA_SLOT};
use crate::LockExt;
use crate::chat::handoff;
use crate::fork::autoridade::Autoridade;
use crate::fork::sessoes::{
    encerrar::{self, ExtraState},
    store::SessionRow,
};
use crate::store::{Entry, EntryKind, Lane};
use anyhow::{Result, ensure};
use serde_json::{Value, json};

/// Captura PR5 do preparo do turno: o cartão próprio da lane (recomeço da principal ou continuação de uma extra), lido
/// do banco pelo preparo canônico. É dado, não autoridade. A fonte continua sendo o cartão em rascunho e a chave da
/// lane: a captura pode ser descartada antes do permit sem perder nada; o permit consome exatamente esta revisão.
pub(super) struct SessionCard {
    pub lane: Lane,
    pub card: handoff::Pr5Card,
    /// `session_fresh` e o reenvio de perfil antes de compor o prompt que leva o cartão: se o cartão mudar antes do
    /// permit, a recomposição volta a levar o perfil inicial.
    pub composed_from: Option<(bool, bool)>,
}

impl Actor {
    fn extra_lane(&self, session: &str) -> Lane {
        Lane { chat: self.cfg.id.clone(), session: Some(session.to_owned()) }
    }

    /// O turno em andamento (rodando, em admissão ou concluindo) é desta `lane`.
    pub(super) fn running_in(&self, lane: &Lane) -> bool {
        (self.turn.is_some() || self.pending_admission.is_some() || self.finishing.is_some()) && self.lane == *lane
    }

    /// Há trabalho desta `lane` em memória: turno, espera, conclusão, fila ou pedido `ask_bot` feito por ela.
    pub(super) fn lane_busy(&self, lane: &Lane) -> bool {
        self.running_in(lane)
            || self.waiting_lane().as_ref() == Some(lane)
            || self.queue.iter().any(|q| matches!(q, Queued::User { lane: l, .. } if l == lane))
            || self.hub.team.pending_from_session(&self.cfg.id, lane)
    }

    /// Para o turno ou a espera de `lane`, se forem dela, sem tocar pontes de outros slots. Devolve se havia algo.
    /// Uma conclusão durável (`Completing`) não é parada: ela só publica o que já foi produzido, sem prompt novo.
    pub(super) async fn stop_lane(&mut self, lane: &Lane, slot: Option<&str>) -> bool {
        let running = self.running_in(lane);
        let waiting = self.waiting_lane().as_ref() == Some(lane) && !self.waiting_completing();
        if running || waiting {
            self.hub.pontes.revogar_slot(&self.cfg.id, slot);
        }
        if running || (waiting && self.waiting_for_reset()) {
            self.cancel_turn_kept_bridges().await;
        } else if waiting {
            self.clear_waiting();
        }
        running || waiting
    }

    /// `sessionClose`: fecha na Tx da guarda com a autoridade ORIGINAL (nunca recapturada; revogada → nada muda) e só
    /// depois solta o trabalho em memória desta sessão. Irmãs e principal continuam.
    pub(super) async fn session_close(&mut self, id: &str, expected: i64, autoridade: &Autoridade) -> Result<Value> {
        let chat = self.cfg.id.clone();
        crate::fork::donna_fixa::pedido::antes_de_gravar(&self.hub.store);
        let closed = {
            let _emit = self.hub.emit_lock.locked();
            let mut closed = autoridade.with_db(&self.hub, None, |c| encerrar::close_on(c, &chat, id, expected))?;
            encerrar::publish_locked(&self.hub, Some(&closed.session), &mut closed.entries);
            closed
        };
        crate::fork::owner_question::refresh(&self.hub, [&chat]);
        let stop = self.release_session(id, &closed.cancelled).await;
        Ok(json!({
            "session": closed.session,
            "cancelled": {"entryIds": closed.cancelled},
            "stopRequested": stop,
        }))
    }

    /// Depois do commit: tira a sessão da fila em memória (inclusive restaurações das entradas canceladas), cancela
    /// só os pedidos dela, para só o turno ou a espera dela e esquece o slot ACP dela.
    async fn release_session(&mut self, id: &str, cancelled: &[String]) -> bool {
        let lane = self.extra_lane(id);
        let slot = format!("{EXTRA_SLOT}{id}");
        self.queue.retain(|q| match q {
            Queued::User { lane: l, .. } => *l != lane,
            Queued::RestoreUser { entry_id, .. } => !cancelled.contains(entry_id),
            _ => true,
        });
        self.hub.team.cancel_from_session(&self.cfg.id, &lane);
        if self.handoff_chat.as_ref() == Some(&lane) {
            self.handoff = None;
            self.handoff_chat = None;
        }
        if self.has_session_card(&lane) {
            self.session_card = None;
        }
        let running = self.running_in(&lane);
        let stopped = self.stop_lane(&lane, Some(&slot)).await;
        // A ACP ociosa fecha quando o motor anuncia; o slot é esquecido sempre (os vínculos já caíram na Tx).
        if !running
            && let Some(sid) = self.session_for(Some(&slot))
            && self.conn.as_ref().is_some_and(|c| c.sessions.close && c.loaded.contains_key(&sid))
        {
            self.close_session(&sid).await;
        }
        self.set_session(Some(&slot), None);
        stopped
    }

    /// P4: candidata da varredura (produtor do sistema, sem Authority, só `idle`). Sessão com trabalho em memória
    /// fica aberta; a Tx relê a configuração, o limite, o escopo e todas as pendências duráveis e fecha sem cancelar.
    pub(super) fn session_idle_close(&mut self, id: &str) {
        let lane = self.extra_lane(id);
        if self.lane_busy(&lane) {
            return;
        }
        let chat = self.cfg.id.clone();
        let closed = {
            let _emit = self.hub.emit_lock.locked();
            match idle_close(&self.hub.store, &chat, id) {
                Ok(Some(session)) => {
                    encerrar::publish_locked(&self.hub, Some(&session), &mut []);
                    true
                }
                Ok(None) => false,
                Err(error) => {
                    tracing::warn!(error = format!("{error:#}"), session = id, "arquivamento automático adiado");
                    false
                }
            }
        };
        if closed {
            // Sem turno, espera ou fila desta sessão: só o slot ACP ocioso fica para trás.
            self.set_session(Some(&format!("{EXTRA_SLOT}{id}")), None);
        }
    }

    /// A espera em andamento é de uma extra encerrada (inclusive antes de reiniciar o Hub): termina sem turno, com o
    /// aviso. Só o estado canônico `Closed` descarta; erro de leitura adia, sem destruir o checkpoint. Conclusão
    /// durável segue para a publicação canônica (dados já produzidos, sem IA).
    pub(super) fn drop_closed_waiting(&mut self) -> bool {
        if self.waiting_completing() {
            return false;
        }
        let Some(lane) = self.waiting_lane() else { return false };
        let Some(id) = lane.session.clone() else { return false };
        let state = encerrar::extra_state_on(&self.hub.store.db.locked(), &lane.chat, &id);
        match state {
            Ok(ExtraState::Open) => false,
            Ok(ExtraState::Closed) => {
                if self.clear_waiting() {
                    let data = json!({"text": encerrar::CLOSED_BEFORE_REPLY, "style": "info", "author": self.cfg.id});
                    let _ = self.hub.add_entry(&lane, EntryKind::Notice, self.hub.store.max_turn(&lane.chat), &data);
                }
                true
            }
            Err(error) => {
                let error = format!("{error:#}");
                tracing::warn!(error, session = id, "retomada adiada: estado da sessão ilegível");
                true
            }
        }
    }

    /// Preparo canônico do primeiro turno humano de `lane` (fila e preparo restaurado de uma reserva): substitui a
    /// captura pelo cartão PR5 próprio da lane, lido do banco agora, mesmo com outra passagem em memória. Não escreve
    /// no buffer legado nem consome nada. Erro de leitura impede o turno (nunca segue sem o cartão).
    pub(super) fn capture_session_card(&mut self, lane: &Lane) -> Result<()> {
        let card = handoff::capture_on(&self.hub.store.db.locked(), lane)?;
        self.session_card = card.map(|card| SessionCard { lane: lane.clone(), card, composed_from: None });
        Ok(())
    }

    /// O texto de passagem efetivo de um turno em `lane`: o cartão PR5 capturado para ela, enquadrado pelo Hub como
    /// contexto recuperado (`handoff::as_context`, D125: dado mesmo depois de editado); sem ele, o buffer legado
    /// (rollover, reconstrução, espera) só se pertencer a ela. Único critério de `TurnInput`, composição, contexto
    /// herdado, checkpoint e troca de motor.
    pub(super) fn turn_handoff(&self, lane: &Lane) -> Option<String> {
        if let Some(card) = self.session_card.as_ref().filter(|card| card.lane == *lane) {
            return Some(handoff::as_context(&card.card.text));
        }
        self.handoff.clone().filter(|_| self.handoff_belongs(lane))
    }

    /// A lane do turno tem cartão PR5 capturado (a composição o usa sem tirar do buffer legado).
    pub(super) fn has_session_card(&self, lane: &Lane) -> bool {
        self.session_card.as_ref().is_some_and(|card| card.lane == *lane)
    }

    /// Antes de compor o prompt que leva o cartão: guarda o estado do perfil inicial para uma recomposição.
    pub(super) fn note_card_composition(&mut self, fresh: bool, resend: bool) {
        let lane = self.lane.clone();
        if let Some(card) = self.session_card.as_mut().filter(|card| card.lane == lane) {
            card.composed_from = Some((fresh, resend));
        }
    }

    /// O cartão a consumir na Tx deste permit: só se o turno é da lane da captura e o insumo capturado do turno leva
    /// exatamente o texto dela.
    pub(super) fn session_card_for_permit(&self) -> Option<(Lane, handoff::Pr5Card)> {
        let card = self.session_card.as_ref()?;
        let input = self.input.as_ref()?;
        let carried = input.handoff.as_deref() == Some(handoff::as_context(&card.card.text).as_str());
        (card.lane == self.lane && input.lane() == card.lane && carried).then(|| (card.lane.clone(), card.card.clone()))
    }

    /// Depois do commit do permit que consumiu o cartão: publica a mudança e solta a captura.
    pub(super) fn session_card_consumed(&mut self, entry: &Entry) {
        self.session_card = None;
        let _emit = self.hub.emit_lock.locked();
        let _ = self.hub.events.send(json!({"type":"entry","rev":entry.rev,"entry":entry}));
    }

    /// O cartão mudou entre a composição e o permit (editado, re-marcado ou consumido): a Tx foi desfeita. A captura
    /// cai, o perfil inicial volta a valer e o turno volta ao MESMO preparo (entradas, lane, reserva e token), que relê
    /// o cartão atual e recompõe. Não é falha de abertura nem tentativa nova.
    pub(super) fn recompose_after_card_change(&mut self, sid: &str, entries: &[String]) {
        if let Some((fresh, resend)) = self.session_card.take().and_then(|card| card.composed_from) {
            self.session_fresh = fresh;
            if resend {
                self.profile_resend.insert(sid.to_owned());
            }
        }
        let Some(input) = self.input.as_ref() else { return };
        let (lane, text, hidden) = (input.lane(), input.text.clone(), input.hidden);
        self.defer_setup(lane, entries, &text, hidden);
        self.queue_retry_at = Some(std::time::Instant::now());
    }

    /// Recomeçar a principal (`primaryRestart`, interno; o `newSession` legado chega aqui com a autoridade ORIGINAL).
    /// Numa Tx da guarda, só da principal: CAS da conversa, fila, perguntas, escolhas, checkpoint de espera e o cartão
    /// de passagem na chave legada (R5 #3, R6 #4). Autoridade revogada → nada muda. Depois do commit: o turno ou a
    /// espera só se forem da principal, os pedidos que ela fez, rotinas e pedidos de bot na fila (vivem na principal)
    /// e o slot dela. Extras trabalhando, na fila, em espera ou com pedidos próprios não são tocadas.
    pub(super) async fn primary_restart(
        &mut self,
        chat: &str,
        revision: i64,
        autoridade: &Autoridade,
    ) -> Result<Value> {
        ensure!(chat == self.cfg.id, "primary_conversation_required");
        let space = crate::fork::spaces::chat_space(&self.hub, chat)?;
        let main = Lane::main(chat);
        // O resumo lê pelo Store: montado antes das travas, da ACP da principal que vai ser esquecida.
        let (text, person) = match self.session_id.as_deref() {
            Some(sid) => {
                let (text, person) = handoff::build_in_with_person(&self.hub.store, chat, chat, sid)?;
                (Some(text), person)
            }
            None => (None, None),
        };
        crate::fork::donna_fixa::pedido::antes_de_gravar(&self.hub.store);
        let (conversation, entries, card) = {
            let _emit = self.hub.emit_lock.locked();
            let (conversation, mut entries, card) = autoridade.with_db(&self.hub, None, |c| {
                let conversation = crate::store::Store::space_conversation_on(c, &space, chat)?;
                ensure!(
                    conversation.bot_id == chat && conversation.revision == revision,
                    "conversation_revision_conflict"
                );
                // Um rascunho pendente na principal vira o do recomeço, sem duplicar (`restart_on`).
                let (entries, card) = encerrar::restart_on(c, chat, text.as_deref(), person.as_deref())?;
                Ok((conversation, entries, card))
            })?;
            let mut published = entries.clone();
            if let Some((marker, card)) = &card {
                published.extend(marker.iter().cloned().chain([card.clone()]));
            }
            encerrar::publish_locked(&self.hub, None, &mut published);
            entries.retain(|e| e.kind == EntryKind::User.as_str());
            (conversation, entries, card.map(|(_, card)| card))
        };
        crate::fork::owner_question::refresh(&self.hub, [&self.cfg.id]);
        let ids: Vec<String> = entries.iter().map(|e| e.id.clone()).collect();
        self.queue.retain(|q| match q {
            Queued::User { lane, .. } => *lane != main,
            Queued::RestoreUser { entry_id, .. } => !ids.contains(entry_id),
            Queued::Routine(_) | Queued::BotRequest(_) => false,
            _ => true,
        });
        self.hub.routines.cancel_queued(&self.hub.store, &self.cfg.id)?;
        // MX-145: o que o teto segurou da principal sai junto (rotina cancelada, pedido fechado).
        let ceiling =
            self.drop_ceiling_holds(&super::fork_teto::CeilingDrop::PrimaryRestart { main: &main, entries: &ids });
        self.hub.team.cancel_from_session(&self.cfg.id, &main);
        let stopped = self.stop_lane(&main, None).await;
        self.set_session(None, None);
        if self.handoff_chat.as_ref() == Some(&main) {
            self.handoff = None;
            self.handoff_chat = None;
        }
        // A captura anterior do recomeço da principal (de outro cartão) não vale mais; o próximo preparo relê.
        if self.has_session_card(&main) {
            self.session_card = None;
        }
        if self.lane == main {
            self.announce = None;
        }
        // O dreamer pertence à identidade/Space original; turnos adicionais não o alimentam.
        let _ = self.dreamer.send(crate::chat::memory::DreamEvent::Flush);
        // O resto do recomeço valeu; o cancelamento não durável de algo parado pelo teto não é anunciado.
        ceiling?;
        Ok(json!({
            "context": crate::fork::spaces::conversation_context(&conversation),
            "stopRequested": stopped,
            "entryIds": ids,
            "handoff": card,
        }))
    }
}

/// A Tx IMMEDIATE do produtor do sistema (sem Authority, só `idle`), sob o `emit_lock` do chamador.
fn idle_close(store: &crate::store::Store, chat: &str, id: &str) -> Result<Option<SessionRow>> {
    let mut db = store.db.locked();
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let closed = encerrar::close_idle_on(&tx, chat, id, crate::store::now_ms())?;
    tx.commit()?;
    Ok(closed)
}
