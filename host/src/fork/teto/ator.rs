//! MX-145 E3: ganchos do ator. A conferência do teto roda DENTRO da transação do
//! permit (`teto::decide_on` via `ClaimHooks::gate` em `send_admitted`): a mesma
//! leitura que autoriza o despacho grava a espera durável e apaga a reserva —
//! nenhum gasto ou mudança de configuração entra entre a decisão e a permissão.
//!
//! Cada trabalho travado tem UMA linha durável em `donna_ceiling_holds` (chave
//! `(bot, trabalho)`); a lista em memória é só o espelho dela, com o que não é
//! serializável (canal de resposta do pedido, turno de grupo). Toda saída de um
//! item — retomada, cancelamento, aviso de perda — confirma primeiro o efeito
//! durável dele e só então apaga a linha DELE; efeito que falha conserva a
//! espera. O restore reconstrói a lista na ordem da tabela; a fila restaurada e
//! o re-drive do agendador não duplicam o que já está parado aqui.
//!
//! A retomada D72 (assinatura) parada pelo teto continua sendo da espera D72:
//! o teto só segura o `retry_waiting` e, ao liberar, devolve a ele.
use super::{Actor, Lane, Queued, fork_admission::Pending};
use crate::{
    LockExt,
    fork::{
        activity_code::ActivityKind,
        bot_admission::{Origin, Ticket},
        teto,
    },
    hub::BotStatus,
    store::EntryKind,
};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Reavaliação do teto com trabalho estacionado: cadência da sonda de geração.
const CEILING_RECHECK: Duration = Duration::from_secs(15);
/// Fallback: mesmo com a sonda idêntica, a avaliação completa roda no mínimo
/// neste intervalo (estado mudado fora da sonda só mantém a espera, não a abre).
const CEILING_PROBE_STALE: Duration = Duration::from_secs(60);

/// Retentativa do que o dono já decidiu (cancelamento, liquidação, reescrita do
/// lote) quando o efeito durável falhou: independe do teto abrir.
const CEILING_SETTLE_RETRY: Duration = Duration::from_secs(1);

const STOPPED_ACTIVITY: &str = "Parado pelo teto de gasto.";

/// Trabalho de um turno travado pelo teto, fora da fila e fora da admissão.
/// Cada variante sabe se reconstituir: itens de fila voltam à fila; a captura de
/// setup (humano/recuperação) volta como `Pending::Setup`; `Waiting` devolve a
/// retomada à espera D72 que a possui; `Dead` marca pedido ou turno de grupo cujo
/// canal morreu no reinício — vira aviso durável na retomada; `Settled` é o que
/// já teve o efeito confirmado e só falta apagar a linha (marca `settled` na linha:
/// sobrevive a restart e sai no próximo ciclo, mesmo com o teto travado).
pub(super) enum CeilingHeld {
    Routine { id: String },
    Request { ask: Box<crate::chat::team::BotRequest> },
    Group { turn: Box<crate::chat::group::GroupTurn> },
    Setup { lane: Lane, entries: Vec<String>, text: String, hidden: bool },
    Waiting { lane: Lane, entries: Vec<String> },
    Dead { what: &'static str },
    Settled,
}

/// Espelho em memória de uma linha durável.
pub(super) struct Held {
    /// Chave da linha (`teto::work_key`).
    work: String,
    /// Origem gravada (`routine`, `human`, ...): a política por origem é reaplicada a cada reavaliação.
    origin: String,
    item: CeilingHeld,
    /// O dono cancelou (marca `cancelling` na linha, gravada ANTES do efeito): o
    /// efeito é retentado a cada ciclo e após restart; nunca volta à origem.
    cancelling: bool,
    /// Lote humano com entrada parada cuja reescrita falhou: a linha (e a ordem)
    /// fica; a reescrita é retentada e, até valer, a captura não é liberada.
    stale: bool,
}

impl Held {
    fn new(work: String, origin: String, item: CeilingHeld) -> Self {
        Self { work, origin, item, cancelling: false, stale: false }
    }

    /// Pode voltar à origem: nem decisão de cancelamento pendente nem só a linha.
    fn releasable(&self) -> bool {
        !self.cancelling && !matches!(self.item, CeilingHeld::Settled)
    }

    /// Ainda há efeito decidido pelo dono a confirmar.
    fn unsettled(&self) -> bool {
        self.cancelling || self.stale || matches!(self.item, CeilingHeld::Settled)
    }
}

pub(super) type CeilingHolds = VecDeque<Held>;

/// Restauração das esperas que falhou no boot; guarda a recuperação de lane que
/// só pode decidir depois de saber o que está parado.
#[derive(Default)]
pub(super) struct PendingRestore {
    pub interrupted: Option<Lane>,
}

/// Intervalo entre releituras das esperas quando o boot não conseguiu lê-las.
const CEILING_RESTORE_RETRY: Duration = Duration::from_secs(1);

/// O que tirar das esperas num cancelamento (Parar, recomeço, cancelamento de pedido/grupo).
pub(super) enum CeilingDrop<'a> {
    /// Parar a principal: rotinas, pedidos e capturas das entradas canceladas.
    StopMain {
        entries: &'a [String],
    },
    /// Parar com entradas específicas (outra conversa ou uma mensagem).
    Entries(&'a [String]),
    /// Recomeço da principal: rotinas, pedidos e capturas da principal.
    PrimaryRestart {
        main: &'a Lane,
        entries: &'a [String],
    },
    Ask {
        id: &'a str,
    },
    Group {
        chat: &'a str,
    },
}

impl CeilingDrop<'_> {
    /// Entradas humanas canceladas por este pedido (as demais do lote sobrevivem).
    fn entries(&self) -> &[String] {
        match self {
            Self::StopMain { entries } | Self::Entries(entries) | Self::PrimaryRestart { entries, .. } => entries,
            Self::Ask { .. } | Self::Group { .. } => &[],
        }
    }
}

impl Actor {
    /// Origem do turno em preparo, a mesma classificação gravada na reserva.
    pub(super) fn turn_origin(&self) -> Origin {
        if let Some(id) = &self.active_routine {
            Origin::Routine { id: id.clone() }
        } else if let Some(request) = &self.active_request {
            if request.expects_reply() { Origin::Ask } else { Origin::Message }
        } else if self.active_group.is_some() {
            Origin::Group
        } else if self.compacting.is_some() {
            Origin::Compaction
        } else if self.input.as_ref().is_some_and(|input| input.hidden) {
            Origin::Recovery
        } else {
            Origin::Human
        }
    }

    /// Corpo da espera durável: identidade da reserva, origem e insumos do setup
    /// — o suficiente para revalidar e reconstituir o trabalho ainda não enviado.
    /// `waiting` marca a retomada D72: no boot ela volta para a espera D72.
    pub(super) fn ceiling_hold_body(&self, ticket: &Ticket, entries: &[String]) -> Value {
        let origin = self.turn_origin();
        let detail = match &origin {
            Origin::Routine { id } => json!({"routineId": id}),
            Origin::Ask | Origin::Message => self.active_request.as_ref().map_or_else(
                || json!({}),
                |a| json!({"requestId": a.id, "entryId": a.entry_id, "expectsReply": a.expects_reply()}),
            ),
            Origin::Group => {
                self.active_group.as_ref().map_or_else(|| json!({}), |g| json!({"chat": g.lane.chat, "epoch": g.epoch}))
            }
            _ => json!({}),
        };
        let mut body = json!({
            "attempt": ticket.attempt,
            "executionToken": ticket.execution_token,
            "origin": teto::origin_kind(&origin),
            "originDetail": detail,
            "lane": {"chat": self.lane.chat, "session": self.lane.session},
            "setup": {
                "entries": entries,
                "text": self.input.as_ref().map(|i| i.text.clone()).unwrap_or_default(),
                "hidden": self.input.as_ref().is_some_and(|i| i.hidden),
            },
            "waiting": self.waiting_retrying(),
        });
        body["work"] = json!(teto::work_key(&body));
        body
    }

    /// O trabalho com essa chave já está parado aqui (dedupe de restore/re-drive).
    pub(super) fn ceiling_holds_work(&self, work: &str) -> bool {
        self.ceiling_holds.iter().any(|h| h.work == work)
    }

    /// A entrada humana já está numa captura parada pelo teto: a fila restaurada
    /// e um `Send` repetido não a duplicam.
    pub(super) fn ceiling_holds_entry(&self, entry: &str) -> bool {
        self.ceiling_holds
            .iter()
            .any(|h| matches!(&h.item, CeilingHeld::Setup { entries, .. } if entries.iter().any(|e| e == entry)))
    }

    /// A retomada D72 está parada pelo teto: `retry_waiting` espera a liberação.
    pub(super) fn ceiling_holds_waiting(&self) -> bool {
        self.ceiling_holds.iter().any(|h| matches!(h.item, CeilingHeld::Waiting { .. }))
    }

    /// Estaciona o trabalho travado fora de `pending_admission`: origens isentas
    /// seguem atendidas. A linha durável já está gravada (na Tx do permit); o
    /// evento sai só se ela mudou. A retomada D72 volta a ser só a espera D72
    /// (turno e entrada continuam dela, como depois de um boot).
    pub(super) fn park_ceiling_hold(&mut self, body: &Value, ticket: &Ticket, entries: &[String], changed: bool) {
        let text = self.input.as_ref().map(|i| i.text.clone()).unwrap_or_default();
        let hidden = self.input.as_ref().is_some_and(|i| i.hidden);
        let lane = self.lane.clone();
        let d72 = body["waiting"] == true && self.waiting_retrying();
        let setup = || CeilingHeld::Setup { lane: lane.clone(), entries: entries.to_vec(), text: text.clone(), hidden };
        let item = if d72 {
            CeilingHeld::Waiting { lane: lane.clone(), entries: entries.to_vec() }
        } else {
            match self.turn_origin() {
                Origin::Routine { .. } => {
                    self.active_routine.take().map_or_else(setup, |id| CeilingHeld::Routine { id })
                }
                Origin::Ask | Origin::Message => {
                    self.active_request.take().map_or_else(setup, |ask| CeilingHeld::Request { ask: Box::new(ask) })
                }
                Origin::Group => {
                    self.active_group.take().map_or_else(setup, |turn| CeilingHeld::Group { turn: Box::new(turn) })
                }
                _ => setup(),
            }
        };
        let mut held = Held::new(
            body["work"].as_str().unwrap_or_default().to_owned(),
            body["origin"].as_str().unwrap_or_default().to_owned(),
            item,
        );
        // Trabalho que o dono já cancelou e voltou pela admissão (re-drive): o
        // permit o segurou sem tocar a linha; aqui ele segue cancelado.
        held.cancelling =
            teto::cancelled(&self.hub.store.db.locked(), &self.cfg.id, &held.work).unwrap_or_else(|error| {
                tracing::error!(%error, "intenção de cancelar ilegível; a espera fica parada");
                false
            });
        // Mesmo trabalho travando de novo: substitui no lugar (a linha também é a mesma).
        if let Some(slot) = self.ceiling_holds.iter_mut().find(|h| h.work == held.work) {
            held.cancelling |= slot.cancelling;
            *slot = held;
        } else {
            self.ceiling_holds.push_back(held);
        }
        // O preparo de skills pertence ao ticket morto na Tx: sem a limpeza a
        // retomada com reserva nova cai em `skills_preparation_origin_changed`.
        self.clear_profile_skills(ticket);
        // A reserva morreu na Tx do permit; libera turno, fila e orçamento de preparo.
        self.dispatch = None;
        self.routine_deadline = None;
        if d72 {
            self.ceiling_hold_waiting();
        } else {
            self.turn = None;
            self.input = None;
        }
        self.ceiling_next_check = Some(Instant::now() + CEILING_RECHECK);
        self.ceiling_stopped_runtime(Some(ticket.execution_token.as_str()));
        if changed {
            teto::emit(&self.hub);
        }
    }

    /// Publica "parado pelo teto". O preparo do ticket descartado sai junto
    /// (senão Desfazer e a instalação do motor veem o bot ocupado); um preparo
    /// sucessor, com outro token, fica.
    fn ceiling_stopped_runtime(&self, dropped: Option<&str>) {
        self.hub.set_memory_runtime(&self.cfg.id, |r| {
            r.status = BotStatus::Waiting;
            r.set_activity(STOPPED_ACTIVITY, Some(ActivityKind::CeilingStopped));
            r.started_at = None;
            r.message = None;
            if dropped.is_some() && r.preparing_turn_token.as_deref() == dropped {
                r.preparing_turn_token = None;
            }
        });
    }

    /// Sem espera, sem turno: a linha de atividade "parado pelo teto" sai (só ela —
    /// uma espera de assinatura, que também é `Waiting`, fica intocada).
    fn ceiling_clear_runtime(&self) {
        if self.ceiling_holds.is_empty() && self.turn.is_none() && self.pending_admission.is_none() {
            self.hub.set_memory_runtime(&self.cfg.id, |r| {
                if r.status == BotStatus::Waiting && r.activity == STOPPED_ACTIVITY {
                    r.status = BotStatus::Idle;
                    r.clear_activity();
                }
            });
        }
    }

    /// Um trabalho só volta com o ator livre: nada em admissão, despacho ou
    /// conclusão, nem turno, espera D72 ou item de fila autônomo à frente — assim
    /// cada liberação passa pelo caminho normal na ordem em que travou. A retomada
    /// D72 é a exceção: o turno que ocupa o ator é o dela.
    fn ceiling_ready(&self, item: &CeilingHeld) -> bool {
        if self.pending_admission.is_some()
            || self.dispatch.is_some()
            || self.routine_completion.is_some()
            || self.finishing.is_some()
        {
            return false;
        }
        match item {
            CeilingHeld::Waiting { .. } => true,
            _ => {
                self.turn.is_none()
                    && self.waiting.is_none()
                    && !self
                        .queue
                        .iter()
                        .any(|q| matches!(q, Queued::Routine(_) | Queued::BotRequest(_) | Queued::Group(_)))
            }
        }
    }

    /// Reavaliação com folga: a sonda O(1) decide se a conferência completa roda;
    /// a política por origem é reaplicada a CADA espera (desmarcar
    /// `applyToConversations` solta a fala do dono e mantém o autônomo). Abertas
    /// voltam à origem uma por vez, na ordem; ainda travadas só regravam se a
    /// assinatura mudou.
    pub(super) async fn retry_ceiling_holds(
        &mut self,
        done_tx: &tokio::sync::mpsc::UnboundedSender<super::PromptDone>,
    ) {
        // O que o dono já decidiu (cancelar, liquidar, reescrever o lote) vale
        // antes de qualquer retomada e independe do teto abrir.
        self.settle_ceiling_holds();
        // Ator ocupado: nada pode voltar agora; confere de novo no próximo tick.
        if !self.ceiling_holds.iter().any(|h| h.releasable() && self.ceiling_ready(&h.item)) {
            self.schedule_ceiling_settle();
            return;
        }
        {
            let db = self.hub.store.db.locked();
            if let Ok(probe) = teto::probe_on(&db)
                && self
                    .ceiling_probe
                    .as_ref()
                    .is_some_and(|(seen, at)| *seen == probe && at.elapsed() < CEILING_PROBE_STALE)
            {
                self.ceiling_next_check = Some(Instant::now() + CEILING_RECHECK);
                return;
            }
        }
        let evaluated = (|| -> anyhow::Result<_> {
            let db = self.hub.store.db.locked();
            let tx = db.unchecked_transaction()?;
            let probe = teto::probe_on(&tx).ok();
            let eval = teto::evaluate_on(&tx, teto::settings_on(&tx)?)?;
            let mut changed = eval.changed;
            // Ainda travadas: a linha acompanha o estado (assinatura nova => `at` novo).
            if let Some(blocked) = &eval.gate.blocked {
                for held in &self.ceiling_holds {
                    if !teto::covers(&held.origin, &eval.settings) {
                        continue;
                    }
                    if let Some(mut body) = teto::read_hold(&tx, &self.cfg.id, &held.work)? {
                        body["reason"] = json!(blocked.reason);
                        body["metrics"] = json!(blocked.metrics);
                        body["signature"] = json!(eval.gate.signature);
                        body["at"] = json!(crate::store::now_ms());
                        changed |= teto::write_hold(&tx, &self.cfg.id, &held.work, "turn", &body)?;
                    }
                }
            }
            tx.commit()?;
            Ok((probe, eval, changed))
        })();
        let (probe, eval, changed) = match evaluated {
            Ok(evaluated) => evaluated,
            Err(error) => {
                // Indecisão fecha o escopo coberto: permanece parado, sem prompt.
                tracing::error!(%error, "decisão do teto indisponível; espera conservada");
                self.ceiling_next_check = Some(Instant::now() + CEILING_RECHECK);
                return;
            }
        };
        self.ceiling_probe = probe.map(|p| (p, Instant::now()));
        let open = |origin: &str| eval.gate.blocked.is_none() || !teto::covers(origin, &eval.settings);
        let released = self.release_ceiling_holds(&open, done_tx).await;
        if changed || released {
            teto::emit(&self.hub);
        }
    }

    /// Devolve UM trabalho por ciclo — o primeiro aberto e pronto, na ordem em
    /// que travaram — ao seu caminho original (item de fila, `Pending::Setup`,
    /// espera D72) ou grava o aviso durável do que não pode ser retomado. Os
    /// seguintes saem nos próximos ciclos, quando o ator voltar a ficar livre.
    async fn release_ceiling_holds(
        &mut self,
        open: &(dyn Fn(&str) -> bool + Sync),
        done_tx: &tokio::sync::mpsc::UnboundedSender<super::PromptDone>,
    ) -> bool {
        let mut released = false;
        let mut requeue = None;
        // Cancelada/liquidada nunca volta (e não segura as outras); lote à espera da
        // reescrita segura a vez dele — a ordem de chegada vale.
        let ready = self
            .ceiling_holds
            .iter()
            .position(|h| h.releasable() && open(&h.origin) && self.ceiling_ready(&h.item))
            .filter(|&index| !self.ceiling_holds[index].stale);
        if let Some(index) = ready
            && let Some(held) = self.ceiling_holds.remove(index)
        {
            match self.release_ceiling_hold(held) {
                Ok(back) => {
                    released = true;
                    requeue = back;
                }
                Err(held) => self.ceiling_holds.insert(index, *held),
            }
        }
        self.ceiling_next_check = (!self.ceiling_holds.is_empty())
            .then(|| Instant::now() + if released { Duration::ZERO } else { CEILING_RECHECK });
        self.schedule_ceiling_settle();
        if released {
            // A próxima sai assim que o ator liberar: sem cache da sonda.
            self.ceiling_probe = None;
        }
        self.ceiling_clear_runtime();
        if let Some(item) = requeue {
            self.queue.push_front(item);
            self.next_in_queue(done_tx).await;
        }
        released
    }

    /// Efeito durável primeiro (aviso, fechamento do pedido, espera D72 devida),
    /// depois a linha; só então o efeito em memória. Falha devolve o item intacto.
    fn release_ceiling_hold(&mut self, held: Held) -> Result<Option<Queued>, Box<Held>> {
        let lost = match &held.item {
            CeilingHeld::Request { ask } if ask.reply_closed() => Some(("Pedido", "o canal da resposta expirou")),
            CeilingHeld::Group { turn }
                if turn.reply.is_closed() || !self.hub.groups.current(&turn.lane, turn.epoch) =>
            {
                Some(("Turno de grupo", "a sala seguiu sem ele"))
            }
            CeilingHeld::Dead { what } => Some((*what, "o canal não sobrevive ao reinício")),
            _ => None,
        };
        if let Some((what, why)) = lost
            && !self.ceiling_lost_notice(&held.work, what, why)
        {
            return Err(Box::new(held));
        }
        let durable = match &held.item {
            CeilingHeld::Request { ask } if lost.is_some() => self.ceiling_request_settled(ask),
            CeilingHeld::Waiting { .. } => self
                .ceiling_resume_waiting()
                .map_err(|error| {
                    tracing::error!(%error, "retomada D72 não gravada; espera do teto conservada");
                })
                .is_ok(),
            _ => true,
        };
        if !durable || !self.clear_ceiling_row(&held.work) {
            return Err(Box::new(held));
        }
        Ok(match held.item {
            CeilingHeld::Routine { id } => Some(Queued::Routine(id)),
            CeilingHeld::Request { ask } if lost.is_none() => Some(Queued::BotRequest(*ask)),
            CeilingHeld::Request { ask } => {
                ceiling_request_refused(*ask);
                None
            }
            CeilingHeld::Group { turn } if lost.is_none() => Some(Queued::Group(*turn)),
            CeilingHeld::Setup { lane, entries, text, hidden } => {
                self.pending_admission = Some(Pending::Setup { lane, entries, text, hidden });
                None
            }
            CeilingHeld::Group { .. }
            | CeilingHeld::Dead { .. }
            | CeilingHeld::Waiting { .. }
            | CeilingHeld::Settled => None,
        })
    }

    fn clear_ceiling_row(&self, work: &str) -> bool {
        let db = self.hub.store.db.locked();
        match teto::clear_hold(&db, &self.cfg.id, work) {
            Ok(_) => true,
            Err(error) => {
                tracing::error!(%error, "espera do teto não apagada; conservada");
                false
            }
        }
    }

    /// Pedido nunca enviado: o registro durável fecha como não iniciado ANTES de a
    /// espera sair. Falha conserva a espera (o fechamento é idempotente).
    fn ceiling_request_settled(&self, ask: &crate::chat::team::BotRequest) -> bool {
        let Some(id) = ask.unrecorded_id() else { return true };
        match crate::fork::retornos::encerrar(&self.hub.store, &id, crate::fork::retornos::Decisao::NaoIniciado) {
            Ok(_) => true,
            Err(error) => {
                tracing::error!(%error, "pedido parado pelo teto não fechado; espera conservada");
                false
            }
        }
    }

    /// Aviso durável e idempotente (Notice por trabalho) gravado ANTES de apagar a
    /// evidência: o dono vê no chat o que o teto segurou e não foi retomado.
    fn ceiling_lost_notice(&self, work: &str, what: &str, why: &str) -> bool {
        tracing::warn!(what, why, "trabalho parado pelo teto não retomado");
        let lane = Lane::main(&self.cfg.id);
        let result = self.hub.store.alloc_turn(&lane.chat).and_then(|turn| {
            self.hub.add_entry_once(
                &format!("ceiling-lost-{}-{work}", self.cfg.id),
                &lane,
                EntryKind::Notice,
                turn,
                &json!({"text": format!("{what} parado pelo teto de gasto não foi retomado: {why}. Nada foi enviado."),
                    "style": "info", "author": self.cfg.id}),
            )
        });
        match result {
            Ok(_) => true,
            Err(error) => {
                tracing::error!(%error, "aviso de trabalho perdido não gravado; espera conservada");
                false
            }
        }
    }

    /// Cancelamentos do dono valem também para o que está parado pelo teto: a
    /// mesma ação que tira itens da fila tira a espera correspondente. O efeito
    /// durável (rotina cancelada, pedido fechado) vem ANTES de apagar a linha; se
    /// ele falhar, a espera fica. Parar só parte de um lote humano reescreve a
    /// captura com as entradas que sobram, no mesmo lugar da ordem.
    /// `Err` quando um trabalho parado não teve NEM a intenção de cancelar gravada
    /// NEM o efeito confirmado: quem pediu recebe o erro (nada de cancelamento
    /// durável anunciado) e o item fica bloqueado para retomada, com retentativa.
    pub(super) fn drop_ceiling_holds(&mut self, what: &CeilingDrop<'_>) -> anyhow::Result<()> {
        let matches = |held: &Held| match (what, &held.item) {
            (
                CeilingDrop::StopMain { .. } | CeilingDrop::PrimaryRestart { .. },
                CeilingHeld::Routine { .. }
                | CeilingHeld::Request { .. }
                | CeilingHeld::Dead { .. }
                | CeilingHeld::Settled,
            ) => true,
            (
                CeilingDrop::StopMain { entries } | CeilingDrop::Entries(entries),
                CeilingHeld::Setup { entries: held, .. } | CeilingHeld::Waiting { entries: held, .. },
            ) => held.iter().any(|e| entries.contains(e)),
            (
                CeilingDrop::PrimaryRestart { main, entries },
                CeilingHeld::Setup { lane, entries: held, .. } | CeilingHeld::Waiting { lane, entries: held },
            ) => lane == *main || held.iter().any(|e| entries.contains(e)),
            (CeilingDrop::Ask { id }, CeilingHeld::Request { ask }) => ask.expects_reply() && ask.id == *id,
            (CeilingDrop::Group { chat }, CeilingHeld::Group { turn }) => turn.lane.chat == *chat,
            _ => false,
        };
        let mut keep = VecDeque::new();
        let mut changed = false;
        let mut undurable = false;
        for mut held in std::mem::take(&mut self.ceiling_holds) {
            if !matches(&held) {
                keep.push_back(held);
                continue;
            }
            // Lote humano: as entradas que não foram paradas continuam paradas pelo
            // teto, com o texto delas, NO LUGAR (mesma linha, mesmo `seq`).
            let survivors = self.ceiling_survivors(&held, what);
            if !survivors.is_empty() {
                match self.ceiling_shrink_setup(&held, &survivors) {
                    Ok(shrunk) => {
                        keep.push_back(shrunk);
                        changed = true;
                    }
                    Err(error) => {
                        // A linha e a ordem ficam; a reescrita é retentada no ciclo
                        // seguinte e, até valer, a captura não sai com a entrada parada.
                        tracing::warn!(%error, "captura não reescrita; retentada no lugar");
                        held.stale = true;
                        keep.push_back(held);
                    }
                }
                continue;
            }
            // Intenção durável ANTES do efeito: falhou o efeito, a liberação nunca
            // devolve este trabalho e o ciclo (inclusive após restart) retenta.
            held.cancelling = true;
            let recorded = self.record_ceiling_cancel(&held.work);
            changed = true;
            if let Some(rest) = self.finish_ceiling_cancel(held) {
                // Nem intenção nem efeito duráveis: só a memória segura o item.
                undurable |= !recorded && rest.cancelling;
                keep.push_back(rest);
            }
        }
        self.ceiling_holds = keep;
        if self.ceiling_holds.is_empty() {
            self.ceiling_next_check = None;
        }
        self.schedule_ceiling_settle();
        if changed {
            self.ceiling_clear_runtime();
            teto::emit(&self.hub);
        }
        anyhow::ensure!(
            !undurable,
            "ceiling_cancel_not_durable: o cancelamento do trabalho parado pelo teto não pôde ser gravado; ele segue parado enquanto o Hub estiver ligado, mas um reinício o devolve à espera — tente Parar de novo"
        );
        Ok(())
    }

    /// Efeito do cancelamento (rotina cancelada, pedido fechado como não iniciado)
    /// e depois a linha. Efeito falhou: o item segue `cancelling` (memória e
    /// intenção gravada). Efeito valeu e a linha não saiu: vira `Settled` — a
    /// intenção gravada continua até a linha sair (mesmo comando, por gatilho).
    fn finish_ceiling_cancel(&mut self, held: Held) -> Option<Held> {
        let durable = match &held.item {
            CeilingHeld::Routine { id } => self
                .hub
                .routines
                .cancel_held(&self.hub.store, &self.cfg.id, id)
                .map_err(|error| {
                    tracing::error!(%error, "rotina parada pelo teto não cancelada; intenção conservada");
                })
                .is_ok(),
            CeilingHeld::Request { ask } => self.ceiling_request_settled(ask),
            _ => true,
        };
        if !durable {
            return Some(held);
        }
        let Held { work, origin, item, .. } = held;
        // Soltar o turno de grupo avisa a sala que a resposta não vem; a espera D72
        // é parada pelo próprio cancelamento do turno.
        if let CeilingHeld::Request { ask } = item {
            ceiling_request_refused(*ask);
        }
        if self.clear_ceiling_row(&work) {
            return None;
        }
        Some(Held::new(work, origin, CeilingHeld::Settled))
    }

    /// Grava a intenção de cancelar (INSERT próprio, independente da linha). Falha
    /// é logada; o item já se comporta como cancelado em memória e o ciclo retenta.
    fn record_ceiling_cancel(&self, work: &str) -> bool {
        match teto::record_cancel(&self.hub.store.db.locked(), &self.cfg.id, work) {
            Ok(()) => true,
            Err(error) => {
                tracing::error!(%error, "intenção de cancelar não gravada; retentada no ciclo");
                false
            }
        }
    }

    /// Confirma o que o dono já decidiu, a cada ciclo e independente do teto:
    /// cancelamento pendente retenta o efeito, liquidação apaga a linha, lote com
    /// entrada parada retenta a reescrita no lugar.
    fn settle_ceiling_holds(&mut self) {
        if !self.ceiling_holds.iter().any(Held::unsettled) {
            return;
        }
        let mut keep = VecDeque::new();
        let mut changed = false;
        for mut held in std::mem::take(&mut self.ceiling_holds) {
            if matches!(held.item, CeilingHeld::Settled) {
                self.record_ceiling_cancel(&held.work);
                if self.clear_ceiling_row(&held.work) {
                    changed = true;
                } else {
                    keep.push_back(held);
                }
            } else if held.cancelling {
                self.record_ceiling_cancel(&held.work);
                match self.finish_ceiling_cancel(held) {
                    Some(rest) => {
                        changed |= matches!(rest.item, CeilingHeld::Settled);
                        keep.push_back(rest);
                    }
                    None => changed = true,
                }
            } else if held.stale {
                let survivors = self.ceiling_live_entries(&held);
                if survivors.is_empty() {
                    held.cancelling = true;
                    self.record_ceiling_cancel(&held.work);
                    if let Some(rest) = self.finish_ceiling_cancel(held) {
                        keep.push_back(rest);
                    }
                    changed = true;
                    continue;
                }
                match self.ceiling_shrink_setup(&held, &survivors) {
                    Ok(shrunk) => {
                        keep.push_back(shrunk);
                        changed = true;
                    }
                    Err(error) => {
                        tracing::warn!(%error, "captura ainda não reescrita; retentada");
                        keep.push_back(held);
                    }
                }
            } else {
                keep.push_back(held);
            }
        }
        self.ceiling_holds = keep;
        if changed {
            self.ceiling_clear_runtime();
            teto::emit(&self.hub);
        }
    }

    /// Algo decidido pelo dono ainda não confirmado: retenta logo, sem esperar a
    /// cadência da reavaliação do teto.
    fn schedule_ceiling_settle(&mut self) {
        if self.ceiling_holds.iter().any(Held::unsettled) {
            let soon = Instant::now() + CEILING_SETTLE_RETRY;
            self.ceiling_next_check = Some(self.ceiling_next_check.map_or(soon, |at| at.min(soon)));
        }
    }

    /// Entradas de uma captura humana que continuam na fila durável (as paradas
    /// pelo dono saem).
    fn ceiling_live_entries(&self, held: &Held) -> Vec<String> {
        let CeilingHeld::Setup { entries, hidden: false, .. } = &held.item else { return Vec::new() };
        entries
            .iter()
            .filter(|id| self.hub.store.entry(id).is_some_and(|e| e.data["status"] == "queued"))
            .cloned()
            .collect()
    }

    /// Entradas de uma captura humana que este cancelamento NÃO parou e que
    /// continuam na fila durável; vazio = a captura inteira sai.
    fn ceiling_survivors(&self, held: &Held, what: &CeilingDrop<'_>) -> Vec<String> {
        let CeilingHeld::Setup { lane, entries, hidden: false, .. } = &held.item else { return Vec::new() };
        if matches!(what, CeilingDrop::PrimaryRestart { main, .. } if lane == *main) {
            return Vec::new();
        }
        let dropped = what.entries();
        entries
            .iter()
            .filter(|id| !dropped.contains(id))
            .filter(|id| self.hub.store.entry(id).is_some_and(|e| e.data["status"] == "queued"))
            .cloned()
            .collect()
    }

    /// Reescreve a captura com as sobreviventes e o texto de cada uma (o mesmo
    /// que a fila comporia), na mesma linha — a ordem de chegada se conserva.
    fn ceiling_shrink_setup(&self, held: &Held, survivors: &[String]) -> anyhow::Result<Held> {
        let CeilingHeld::Setup { lane, hidden, .. } = &held.item else { anyhow::bail!("ceiling_hold_not_setup") };
        let original = self
            .hub
            .store
            .bot(&self.cfg.id)?
            .filter(|bot| !bot.deleted)
            .ok_or_else(|| anyhow::anyhow!("bot_not_found"))?
            .config;
        let cfg = crate::fork::spaces::execution_config(&self.hub.store, &original, &lane.chat)?;
        let mut texts = Vec::new();
        for id in survivors {
            let entry = self.hub.store.entry(id).ok_or_else(|| anyhow::anyhow!("entry_not_found"))?;
            texts.push(crate::chat::prompt::of_entry(&self.hub, &cfg, &entry.data));
        }
        let text = texts.join("\n\n");
        let db = self.hub.store.db.locked();
        let mut body =
            teto::read_hold(&db, &self.cfg.id, &held.work)?.ok_or_else(|| anyhow::anyhow!("ceiling_hold_missing"))?;
        body["setup"]["entries"] = json!(survivors);
        body["setup"]["text"] = json!(text);
        let work = teto::work_key(&body);
        body["work"] = json!(work);
        teto::rework_hold(&db, &self.cfg.id, &held.work, &work, &body)?;
        Ok(Held::new(
            work,
            held.origin.clone(),
            CeilingHeld::Setup { lane: lane.clone(), entries: survivors.to_vec(), text, hidden: *hidden },
        ))
    }

    /// Restaura as esperas duráveis no boot, na ordem da tabela: rotina volta como
    /// rotina (o re-drive do agendador é ignorado enquanto ela estiver aqui);
    /// retomada D72 volta para a espera D72; humano/recuperação pelas entradas
    /// gravadas (a fila restaurada não as duplica); pedido/grupo sem canal vivo
    /// vira `Dead` — aviso na retomada.
    pub(super) fn restore_ceiling_hold(&mut self) -> anyhow::Result<()> {
        // Espera D72 gravada pela versão anterior (sem `waiting`) é reconhecida pelo
        // checkpoint D72 vivo — volta como `Waiting`, nunca como segunda captura.
        let rows = {
            let db = self.hub.store.db.locked();
            let mut rows = Vec::new();
            for row in teto::turn_holds_of(&db, &self.cfg.id)? {
                let legacy_d72 = teto::legacy_d72(&db, &self.cfg.id, &row.body)?;
                let cancelled = teto::cancelled(&db, &self.cfg.id, &row.work)?;
                rows.push((row, legacy_d72, cancelled));
            }
            rows
        };
        for (row, legacy_d72, cancelled) in rows {
            if self.ceiling_holds_work(&row.work) {
                continue;
            }
            let hold = &row.body;
            let origin = hold["origin"].as_str().unwrap_or_default().to_owned();
            let lane = Lane {
                chat: hold["lane"]["chat"].as_str().unwrap_or_default().to_string(),
                session: hold["lane"]["session"].as_str().map(str::to_owned),
            };
            let entries: Vec<String> = hold["setup"]["entries"]
                .as_array()
                .map(|v| v.iter().filter_map(|e| e.as_str().map(str::to_owned)).collect())
                .unwrap_or_default();
            let item = match origin.as_str() {
                // MX-263: rotina parada DENTRO da espera D72 volta como a espera
                // que a originou — a retomada é dela, nunca de um segundo envio.
                _ if hold["waiting"] == true || legacy_d72 => CeilingHeld::Waiting { lane, entries },
                "routine" => hold["originDetail"]["routineId"]
                    .as_str()
                    .map_or(CeilingHeld::Dead { what: "Rotina" }, |id| CeilingHeld::Routine { id: id.to_string() }),
                "ask" | "message" => CeilingHeld::Dead { what: "Pedido" },
                "group" => CeilingHeld::Dead { what: "Turno de grupo" },
                _ => CeilingHeld::Setup {
                    lane,
                    entries,
                    text: hold["setup"]["text"].as_str().unwrap_or_default().to_string(),
                    hidden: hold["setup"]["hidden"].as_bool().unwrap_or(false),
                },
            };
            let mut restored = Held::new(row.work, origin, item);
            // A intenção gravada antes do restart (ou a marca da versão anterior) vale.
            restored.cancelling = cancelled;
            // Lote com entrada parada cuja reescrita não chegou a valer antes do restart.
            restored.stale = matches!(restored.item, CeilingHeld::Setup { hidden: false, .. })
                && self.ceiling_live_entries(&restored).len()
                    != hold["setup"]["entries"].as_array().map_or(0, Vec::len);
            self.ceiling_holds.push_back(restored);
        }
        if !self.ceiling_holds.is_empty() {
            self.ceiling_next_check = Some(Instant::now());
            self.ceiling_stopped_runtime(None);
        }
        Ok(())
    }

    /// Releitura das esperas que falhou no boot (fail-closed): até valer, o ator
    /// não admite trabalho. Valendo, o que entrou na fila nesse meio-tempo
    /// (re-drive do agendador, fila restaurada) e já está parado aqui sai da fila
    /// — a espera conduz — e a recuperação de lane adiada decide agora. Devolve
    /// se o ator continua fechado.
    pub(super) async fn retry_ceiling_restore(
        &mut self,
        done_tx: &tokio::sync::mpsc::UnboundedSender<super::PromptDone>,
    ) -> bool {
        if self.ceiling_unrestored.is_none() {
            return false;
        }
        if let Err(error) = self.restore_ceiling_hold() {
            tracing::warn!(%error, "esperas do teto ainda ilegíveis; ator segue fechado");
            self.queue_retry_at = Some(Instant::now() + CEILING_RESTORE_RETRY);
            return true;
        }
        let pending = self.ceiling_unrestored.take().unwrap_or_default();
        let works: std::collections::HashSet<String> = self.ceiling_holds.iter().map(|h| h.work.clone()).collect();
        let entries: std::collections::HashSet<String> = self
            .ceiling_holds
            .iter()
            .filter_map(|h| match &h.item {
                CeilingHeld::Setup { entries, .. } | CeilingHeld::Waiting { entries, .. } => Some(entries.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        self.queue.retain(|q| match q {
            Queued::Routine(id) => !works.contains(&format!("routine:{id}")),
            Queued::User { entry_id, .. } | Queued::RestoreUser { entry_id, .. } => !entries.contains(entry_id),
            _ => true,
        });
        if let Some(lane) = pending.interrupted
            && !self.ceiling_holds_recovery(&lane)
        {
            self.resume_interrupted(lane);
        }
        if self.turn.is_none() {
            self.next_in_queue(done_tx).await;
        }
        false
    }

    /// Recuperação da lane já parada pelo teto: o restore dela conduz a retomada.
    pub(super) fn ceiling_holds_recovery(&self, lane: &Lane) -> bool {
        self.ceiling_holds.iter().any(|h| {
            h.origin == "recovery"
                && matches!(&h.item, CeilingHeld::Setup { lane: held, hidden: true, .. } if held == lane)
        })
    }

    /// Exclusão de Space: espera do teto também é trabalho vivo daquela conversa.
    pub(super) fn ceiling_holds_belong(&self, belongs: &dyn Fn(&str) -> bool) -> bool {
        self.ceiling_holds.iter().any(|h| match &h.item {
            CeilingHeld::Setup { lane, .. } | CeilingHeld::Waiting { lane, .. } => belongs(&lane.chat),
            CeilingHeld::Group { turn } => belongs(&turn.lane.chat),
            // Rotinas e pedidos executam na principal do dono.
            CeilingHeld::Routine { .. }
            | CeilingHeld::Request { .. }
            | CeilingHeld::Dead { .. }
            | CeilingHeld::Settled => belongs(&self.cfg.id),
        })
    }
}

/// Quem pediu recebe a recusa (canal vivo) ou o aviso no chat; o registro
/// durável já foi fechado como não iniciado.
fn ceiling_request_refused(ask: crate::chat::team::BotRequest) {
    ask.complete(Err(anyhow::anyhow!("Parado pelo teto de gasto antes do envio; nada foi enviado.")), false);
}
