//! DON-907 (D431): as quatro ações por mensagem na fila — remover, editar, mover e mandar agora.
//!
//! Padrão do #444 (`newSession`/`sessionClose`): a RPC captura a autoridade ORIGINAL antes de qualquer espera; o ator
//! confere o trabalho em memória (a mensagem ainda está na fila, não foi despachada) e grava com `with_db`, que
//! reconfere a autoridade na mesma Tx, antes do efeito. Pedido atrasado (já despachada, revisão velha, autoridade
//! encerrada) é recusado sem efeito. Tudo é por sessão: só a fila da `lane` da mensagem muda; a interrupção do
//! "Mandar agora" para só o turno dessa `lane` (ou uma compactação), sem revogar pontes de outros slots.
use super::{Actor, NoticeStyle, Queued, session::EXTRA_SLOT};
use crate::LockExt;
use crate::api::devices::Caller;
use crate::chat::team::MAX_MESSAGE_BYTES;
use crate::fork::autoridade::Autoridade;
use crate::hub::Hub;
use crate::store::{Entry, EntryKind, Lane, QUEUE_ORDER, Store, entry_on, next_rev, now_ms};
use anyhow::{Context, Result, anyhow, ensure};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::sync::Arc;

/// D431 provisória: "Mandar agora" com resposta em andamento interrompe só essa resposta (os motores ACP não aceitam
/// injetar no meio do turno). Com `false`, a mensagem só passa para a frente da fila e espera a resposta acabar.
pub(super) const MANDAR_AGORA_INTERROMPE: bool = true;

pub(super) const INTERROMPIDA: &str = "Resposta interrompida para atender a próxima mensagem.";
const COMPACTACAO_ENCERRADA: &str = "Compactação encerrada para atender a próxima mensagem.";
const CONTROL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

pub enum Acao {
    Remove,
    Edit { text: String, expected: i64 },
    Move { position: usize, expected: i64 },
    SendNow,
}

pub struct Pedido {
    pub entry_id: String,
    pub acao: Acao,
    pub autoridade: Autoridade,
    pub reply: tokio::sync::oneshot::Sender<Result<Value>>,
}

/// `queuedRemove` / `queuedEdit` / `queuedMove` / `queuedSendNow`. Sem resposta do ator em 10 s:
/// `queued_control_pending` (resultado incerto; o app relê as entradas pelo stream).
pub(crate) async fn rpc(hub: &Arc<Hub>, caller: &Caller, method: &str, b: &Value) -> Result<Value> {
    let autoridade = Autoridade::capturar(hub, caller)?;
    let field = |k: &str| b[k].as_str().filter(|v| !v.is_empty()).ok_or_else(|| anyhow!("`{k}` is required"));
    let (bot, entry_id) = (field("botId")?, field("entryId")?.to_owned());
    let expected = || b["expectedRevision"].as_i64().context("`expectedRevision` is required");
    let acao = match method {
        "queuedRemove" => Acao::Remove,
        "queuedEdit" => {
            Acao::Edit { text: b["text"].as_str().unwrap_or_default().trim().to_owned(), expected: expected()? }
        }
        "queuedMove" => Acao::Move {
            position: usize::try_from(b["position"].as_u64().context("`position` is required")?)?,
            expected: expected()?,
        },
        "queuedSendNow" => Acao::SendNow,
        _ => anyhow::bail!("unknown queued action"),
    };
    let (reply, result) = tokio::sync::oneshot::channel();
    let pedido = Box::new(Pedido { entry_id, acao, autoridade, reply });
    // Numa conversa de Space o `botId` é a conversa; a fila é a do ator dono dela. A Tx reconfere a posse.
    let owner: Option<String> = rusqlite::OptionalExtension::optional(hub.store.db.locked().query_row(
        "SELECT bot_id FROM donna_space_conversations WHERE id=?1 AND id<>bot_id",
        [bot],
        |r| r.get(0),
    ))?;
    hub.send_cmd(owner.as_deref().unwrap_or(bot), super::Cmd::QueuedControl(pedido))?;
    tokio::time::timeout(CONTROL_TIMEOUT, result).await.map_err(|_| anyhow!("queued_control_pending"))??
}

fn lane_of(entry: &Entry) -> Lane {
    Lane { chat: entry.bot_id.clone(), session: (entry.session_id != entry.bot_id).then(|| entry.session_id.clone()) }
}

/// O slot ACP da `lane`, como no preparo do turno: extra, conversa de Space ou a principal (`None`).
fn slot_of(bot: &str, lane: &Lane) -> Option<String> {
    lane.session
        .as_ref()
        .map(|s| format!("{EXTRA_SLOT}{s}"))
        .or_else(|| (lane.chat != bot).then(|| format!("conversation:{}", lane.chat)))
}

/// A entrada humana `queued` deste bot (ou de uma conversa dele), lida na Tx; nada mais passa.
fn queued_on(c: &Connection, bot: &str, id: &str) -> Result<Entry> {
    let entry = entry_on(c, id)?.ok_or_else(|| anyhow!("queued_entry_not_found"))?;
    let mine: bool = c.query_row(
        "SELECT ?2=?1 OR EXISTS(SELECT 1 FROM donna_space_conversations WHERE id=?2 AND bot_id=?1 AND closed=0)",
        params![bot, entry.bot_id],
        |r| r.get(0),
    )?;
    ensure!(mine, "cross_conversation_reference");
    ensure!(
        entry.kind == EntryKind::User.as_str() && entry.data["status"] == "queued",
        "queued_entry_dispatched: a mensagem já saiu da fila"
    );
    Ok(entry)
}

/// MX-77: o aviso DO DESTINATÁRIO de um pedido `ask_bot`/`message_bot` ainda `queued` — o par do
/// remetente mora na conversa dele. Só o aviso deste bot na principal passa; devolve o `delegationId`
/// que liga o par. Pedidos nunca moram em sessão extra nem em conversa de Space.
fn queued_request_on(c: &Connection, bot: &str, id: &str) -> Result<(Entry, String)> {
    let entry = entry_on(c, id)?.ok_or_else(|| anyhow!("queued_entry_not_found"))?;
    ensure!(entry.bot_id == bot && entry.session_id == bot, "cross_conversation_reference");
    ensure!(
        entry.kind == EntryKind::Notice.as_str()
            && entry.data["status"] == "queued"
            && entry.data["botMessage"]["targetBotId"] == bot
            && entry.data["delegationId"].is_string(),
        "queued_entry_dispatched: a mensagem já saiu da fila"
    );
    let delegation = entry.data["delegationId"].as_str().unwrap_or_default().to_owned();
    Ok((entry, delegation))
}

/// Os dois avisos do pedido (remetente e destinatário), na mesma `delegationId`.
fn request_pair_on(c: &Connection, delegation: &str) -> Result<Vec<Entry>> {
    let cols = crate::store::ENTRY_COLS;
    let sql = format!("SELECT {cols} FROM entries WHERE json_extract(data,'$.delegationId')=?1");
    Ok(c.prepare(&sql)?.query_map([delegation], crate::store::row_entry)?.collect::<rusqlite::Result<_>>()?)
}

/// O nome de exibição do bot na Tx; sem cadastro legível, o próprio id (a linha nunca fica sem origem).
fn request_name_on(c: &Connection, id: &str) -> Result<String> {
    Ok(Store::bot_on(c, id)?.map_or_else(|| id.to_owned(), |b| b.config.name))
}

/// MX-77: reescreve o pedido nos dois avisos — texto novo do dono, marca `edited`, heading e linha de
/// status refeitos como na admissão. O contrato do bot (`botMessage.contract`) não muda.
fn edited_request_data(entry: &Entry, recipient: bool, ask: bool, source: &str, target: &str, text: &str) -> Value {
    let mut data = entry.data.clone();
    let heading = match (recipient, ask) {
        (true, true) => format!("Request from {source}: {text}"),
        (true, false) => format!("Message from {source}: {text}"),
        (false, true) => format!("Asked {target}: {text}"),
        (false, false) => format!("Messaged {target}: {text}"),
    };
    let status = if ask {
        "Waiting for a reply…".to_owned()
    } else {
        format!("Queued. Outcome will appear in {target}'s chat.")
    };
    data["heading"] = heading.clone().into();
    data["text"] = format!("{heading}\n{status}").into();
    if let Some(message) = data.get_mut("botMessage").and_then(Value::as_object_mut) {
        message.insert("text".into(), text.into());
        message.insert("edited".into(), true.into());
    }
    data
}

/// As entradas na fila da mesma sessão, na ordem guardada.
fn lane_queue_on(c: &Connection, lane: &Lane) -> Result<Vec<Entry>> {
    let sql = format!(
        "SELECT {} FROM entries WHERE bot_id=?1 AND COALESCE(session,bot_id)=?2 AND kind='user'
         AND json_extract(data,'$.status')='queued' ORDER BY {QUEUE_ORDER}",
        crate::store::ENTRY_COLS
    );
    Ok(c.prepare(&sql)?
        .query_map(params![lane.chat, lane.session_id()], crate::store::row_entry)?
        .collect::<rusqlite::Result<_>>()?)
}

fn write_on(c: &Connection, id: &str, data: &Value) -> Result<Entry> {
    c.execute(
        "UPDATE entries SET rev=?2, data=?3, updated_at=?4 WHERE id=?1",
        params![id, next_rev(c)?, data.to_string(), now_ms()],
    )?;
    entry_on(c, id)?.ok_or_else(|| anyhow!("queued_entry_not_found"))
}

/// Põe `id` na posição `position` da fila da sessão: as chaves já usadas por ela são redistribuídas na ordem nova,
/// então a posição relativa às outras sessões e às mensagens que chegarem depois não muda.
fn move_on(c: &Connection, lane: &Lane, id: &str, position: usize) -> Result<(Vec<String>, Vec<Entry>)> {
    let mut queue = lane_queue_on(c, lane)?;
    let keys: Vec<i64> = queue.iter().map(|e| crate::store::queue_key(e).0).collect();
    let from = queue.iter().position(|e| e.id == id).ok_or_else(|| anyhow!("queued_entry_not_found"))?;
    let entry = queue.remove(from);
    queue.insert(position.min(queue.len()), entry);
    let mut changed = Vec::new();
    for (entry, key) in queue.iter().zip(keys) {
        if crate::store::queue_key(entry).0 != key {
            let mut data = entry.data.clone();
            data["queueOrder"] = key.into();
            changed.push(write_on(c, &entry.id, &data)?);
        }
    }
    Ok((queue.into_iter().map(|e| e.id).collect(), changed))
}

impl Actor {
    /// A entrada já foi entregue a um turno (em andamento, em admissão ou esperando a assinatura).
    fn dispatched(&self, id: &str) -> bool {
        let id = id.to_owned();
        self.input.as_ref().is_some_and(|input| input.entries.contains(&id))
            || matches!(
                &self.pending_admission,
                Some(
                    super::fork_admission::Pending::Setup { entries, .. }
                        | super::fork_admission::Pending::Prompt { entries, .. }
                ) if entries.contains(&id)
            )
    }

    pub(super) async fn queued_control(
        &mut self,
        id: String,
        acao: Acao,
        autoridade: &Autoridade,
        done_tx: &tokio::sync::mpsc::UnboundedSender<super::PromptDone>,
    ) -> Result<Value> {
        // MX-77: pedido de outro bot ainda na fila do ator — remover (recusar), editar e mandar agora,
        // sem reordenar. O par do remetente recebe a mesma decisão.
        if self.queue.iter().any(|q| matches!(q, Queued::BotRequest(a) if a.entry_id == id)) {
            return self.request_control(id, acao, autoridade, done_tx).await;
        }
        // Pedido atrasado: a mensagem já saiu da fila do ator (despachada, removida ou de outro bot).
        ensure!(
            self.queue_has_user(&id) && !self.dispatched(&id),
            "queued_entry_dispatched: a mensagem já saiu da fila"
        );
        let bot = self.cfg.id.clone();
        crate::fork::donna_fixa::pedido::antes_de_gravar(&self.hub.store);
        let (lane, order, changed) = {
            let _emit = self.hub.emit_lock.locked();
            let result = autoridade.with_db(&self.hub, None, |c| {
                let entry = queued_on(c, &bot, &id)?;
                let lane = lane_of(&entry);
                match &acao {
                    Acao::Remove => {
                        let mut data = entry.data.clone();
                        data["status"] = "cancelled".into();
                        Ok((lane, None, vec![write_on(c, &id, &data)?]))
                    }
                    Acao::Edit { text, expected } => {
                        ensure!(entry.rev == *expected, "queued_revision_conflict: a mensagem mudou; releia a fila");
                        let has_files = entry.data["attachments"].as_array().is_some_and(|a| !a.is_empty());
                        ensure!(!text.is_empty() || has_files, "empty message");
                        let mut data = entry.data.clone();
                        data["text"] = text.clone().into();
                        if let Some(pasted) = data["pasted"].as_array_mut() {
                            pasted.retain(|p| p.as_str().is_some_and(|p| text.contains(p)));
                        }
                        Ok((lane, None, vec![write_on(c, &id, &data)?]))
                    }
                    Acao::Move { position, expected } => {
                        ensure!(entry.rev == *expected, "queued_revision_conflict: a mensagem mudou; releia a fila");
                        let (order, changed) = move_on(c, &lane, &id, *position)?;
                        Ok((lane, Some(order), changed))
                    }
                    Acao::SendNow => {
                        // A marca durável faz o reinício do Hub manter a prioridade e o envio separado (R1-02).
                        let mut data = entry.data.clone();
                        data["sendNow"] = now_ms().into();
                        let mut changed = vec![write_on(c, &id, &data)?];
                        let (order, moved) = move_on(c, &lane, &id, 0)?;
                        changed.retain(|e| moved.iter().all(|m| m.id != e.id));
                        changed.extend(moved);
                        Ok((lane, Some(order), changed))
                    }
                }
            });
            let (lane, order, mut changed) = result?;
            crate::fork::sessoes::encerrar::publish_locked(&self.hub, None, &mut changed);
            (lane, order, changed)
        };
        // Depois do commit, o mesmo efeito na fila em memória do ator.
        match &acao {
            Acao::Remove => {
                self.queue.retain(
                    |q| !matches!(q, Queued::User { entry_id, .. } | Queued::RestoreUser { entry_id, .. } if *entry_id == id),
                );
                self.send_now.retain(|marked| *marked != id);
            }
            Acao::Edit { .. } => {
                // O prompt é reconstruído da entrada no contexto original, como na restauração do boot.
                for q in &mut self.queue {
                    if matches!(q, Queued::User { entry_id, .. } if *entry_id == id) {
                        *q = Queued::RestoreUser { entry_id: id.clone(), retry_at: None };
                    }
                }
                self.retry_queue_restore();
            }
            Acao::Move { .. } | Acao::SendNow => self.reorder_lane(&lane, order.as_deref().unwrap_or_default()),
        }
        let mut stopped = false;
        if matches!(acao, Acao::SendNow) {
            if let Some(at) = self.queue.iter().position(
                |q| matches!(q, Queued::User { entry_id, .. } | Queued::RestoreUser { entry_id, .. } if *entry_id == id),
            ) && let Some(item) = self.queue.remove(at)
            {
                self.queue.push_front(item);
            }
            if !self.send_now.contains(&id) {
                self.send_now.push(id.clone());
            }
            stopped = self.send_now_interrupt(&lane).await;
            if self.turn.is_none() {
                self.next_in_queue(done_tx).await;
            }
        }
        Ok(json!({"entries": changed, "stopRequested": stopped}))
    }

    /// MX-77: remover (recusar), editar ou mandar agora um pedido de bot ainda na fila do ator.
    /// A Tx confere o aviso do destinatário e grava; depois o item em memória recebe o efeito — remover
    /// encerra o pedido avisando o remetente, editar remonta o prompt e mandar agora interrompe o turno
    /// da principal como o "Mandar agora" de mensagem. `Move` é recusado: pedido de bot não se reordena.
    async fn request_control(
        &mut self,
        id: String,
        acao: Acao,
        autoridade: &Autoridade,
        done_tx: &tokio::sync::mpsc::UnboundedSender<super::PromptDone>,
    ) -> Result<Value> {
        let at = self
            .queue
            .iter()
            .position(|q| matches!(q, Queued::BotRequest(a) if a.entry_id == id))
            .ok_or_else(|| anyhow!("queued_entry_dispatched: a mensagem já saiu da fila"))?;
        let ask = matches!(self.queue.get(at), Some(Queued::BotRequest(a)) if a.expects_reply());
        let bot = self.cfg.id.clone();
        crate::fork::donna_fixa::pedido::antes_de_gravar(&self.hub.store);
        let (lane, changed) = {
            let _emit = self.hub.emit_lock.locked();
            let result = autoridade.with_db(&self.hub, None, |c| {
                let (entry, delegation) = queued_request_on(c, &bot, &id)?;
                let lane = lane_of(&entry);
                match &acao {
                    Acao::Remove | Acao::SendNow => Ok((lane, Vec::new())),
                    Acao::Move { .. } => anyhow::bail!("queued_move_unsupported: pedidos de bot não mudam de lugar"),
                    Acao::Edit { text, expected } => {
                        ensure!(entry.rev == *expected, "queued_revision_conflict: a mensagem mudou; releia a fila");
                        ensure!(!text.is_empty() && text.len() <= MAX_MESSAGE_BYTES, "empty message");
                        let source = request_name_on(c, entry.data["sourceBotId"].as_str().unwrap_or(&bot))?;
                        let target = request_name_on(c, &bot)?;
                        let mut changed = Vec::new();
                        for other in request_pair_on(c, &delegation)? {
                            let recipient = other.bot_id == bot;
                            changed.push(write_on(
                                c,
                                &other.id,
                                &edited_request_data(&other, recipient, ask, &source, &target, text),
                            )?);
                        }
                        Ok((lane, changed))
                    }
                }
            });
            let (lane, mut changed) = result?;
            crate::fork::sessoes::encerrar::publish_locked(&self.hub, None, &mut changed);
            (lane, changed)
        };
        // Depois do commit, o mesmo efeito na fila em memória do ator.
        match &acao {
            Acao::Remove => {
                if let Some(at) = self.queue.iter().position(|q| matches!(q, Queued::BotRequest(a) if a.entry_id == id))
                    && let Some(Queued::BotRequest(request)) = self.queue.remove(at)
                {
                    request.reject_by_owner();
                }
            }
            Acao::Edit { text, .. } => {
                if let Some(at) = self.queue.iter().position(|q| matches!(q, Queued::BotRequest(a) if a.entry_id == id))
                    && let Some(Queued::BotRequest(request)) = self.queue.get_mut(at)
                {
                    request.edit_text(text);
                }
            }
            Acao::Move { .. } => unreachable!("o Move já foi recusado na Tx"),
            Acao::SendNow => {}
        }
        let mut stopped = false;
        if matches!(acao, Acao::SendNow) {
            if let Some(at) = self.queue.iter().position(|q| matches!(q, Queued::BotRequest(a) if a.entry_id == id))
                && let Some(item) = self.queue.remove(at)
            {
                self.queue.push_front(item);
            }
            stopped = self.send_now_interrupt(&lane).await;
            if self.turn.is_none() {
                self.next_in_queue(done_tx).await;
            }
        }
        Ok(json!({"entries": changed, "stopRequested": stopped}))
    }

    /// Reordena só os itens da fila em memória que são da `lane`, nas posições que eles já ocupavam.
    fn reorder_lane(&mut self, lane: &Lane, order: &[String]) {
        let of_lane = |q: &Queued| match q {
            Queued::User { lane: l, entry_id, .. } => (l == lane).then_some(entry_id.clone()),
            Queued::RestoreUser { entry_id, .. } => order.contains(entry_id).then_some(entry_id.clone()),
            _ => None,
        };
        let slots: Vec<usize> =
            self.queue.iter().enumerate().filter(|(_, q)| of_lane(q).is_some()).map(|(i, _)| i).collect();
        let mut items: Vec<Queued> = slots.iter().rev().filter_map(|&i| self.queue.remove(i)).collect();
        items.reverse();
        let rank = |q: &Queued| of_lane(q).and_then(|id| order.iter().position(|o| *o == id)).unwrap_or(usize::MAX);
        items.sort_by_key(rank);
        for (at, item) in slots.into_iter().zip(items) {
            self.queue.insert(at, item);
        }
    }

    /// D431 provisória (ver [`MANDAR_AGORA_INTERROMPE`]): para só a resposta da própria sessão, ou uma compactação,
    /// com o aviso na conversa. Pontes de outros slots ficam (`stop_lane`).
    async fn send_now_interrupt(&mut self, lane: &Lane) -> bool {
        if !MANDAR_AGORA_INTERROMPE {
            return false;
        }
        let compacting = self.turn.is_some() && self.compacting.is_some();
        let running = self.lane.clone();
        if !(compacting || self.running_in(lane) || self.waiting_lane().as_ref() == Some(lane)) {
            return false;
        }
        let target = if compacting { running } else { lane.clone() };
        if self.turn.is_some() && !self.stop_requested {
            self.notice(if compacting { COMPACTACAO_ENCERRADA } else { INTERROMPIDA }, NoticeStyle::Info);
        }
        let slot = slot_of(&self.cfg.id, &target);
        self.stop_lane(&target, slot.as_deref()).await
    }

    /// O lote do "Mandar agora" leva só a mensagem escolhida; o resto da fila continua esperando. Só a marca do
    /// item em `next` sai; a de outra entrada marcada fica (R1-01).
    pub(super) fn take_send_now(&mut self, next: usize) -> bool {
        let Some(Queued::User { entry_id, .. }) = self.queue.get(next) else { return false };
        let Some(at) = self.send_now.iter().position(|marked| marked == entry_id) else { return false };
        self.send_now.remove(at);
        true
    }

    /// Uma marcada não entra no lote de outra mensagem: ela sai sozinha quando chegar a vez dela.
    pub(super) fn send_now_marked(&self, id: &str) -> bool {
        self.send_now.iter().any(|marked| marked == id)
    }

    /// Boot: as marcadas por "Mandar agora" (`data.sendNow`) voltam para a frente, a mais recente primeiro, como o
    /// `push_front` fez antes do reinício, e recuperam o envio separado (R1-02).
    pub(super) fn restore_send_now(&mut self, entries: &[Entry]) {
        let mut marked: Vec<(i64, &str)> =
            entries.iter().filter_map(|e| e.data["sendNow"].as_i64().map(|at| (at, e.id.as_str()))).collect();
        marked.sort_unstable();
        for (_, id) in marked {
            let Some(at) = self.queue.iter().position(
                |q| matches!(q, Queued::User { entry_id, .. } | Queued::RestoreUser { entry_id, .. } if entry_id == id),
            ) else {
                continue;
            };
            if let Some(item) = self.queue.remove(at) {
                self.queue.push_front(item);
            }
            if !self.send_now_marked(id) {
                self.send_now.push(id.to_owned());
            }
        }
    }
}
