// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Commands from clients and the queue of turns waiting for the bot.

use super::{Actor, CANCEL_GRACE, Cmd, NoticeStyle, Queued, RoutineCompletion};
use crate::LockExt;
use crate::agent::own_memory;
use crate::chat::memory;
use crate::fork::activity_code::ActivityKind;
use crate::hub::BotStatus;
use crate::market::requests;
use crate::store::{EntryKind, Lane};
use anyhow::{Result, anyhow};
use serde_json::json;
use std::time::Instant;
use tokio::sync::mpsc;

impl Actor {
    pub(super) async fn check_antigravity_turn(&mut self) {
        if self.antigravity_active.as_ref().is_some_and(|grant| !grant.current(&self.hub, &self.cfg)) {
            tracing::warn!(reason = "antigravity_authorization_changed", "turno interativo encerrado");
            self.stop_antigravity().await;
        }
    }

    pub(super) async fn stop_antigravity(&mut self) {
        self.antigravity_active = None;
        self.stop_requested = true;
        if let Some(conn) = self.conn.take() {
            if let Some(sid) = &self.turn_session {
                let _ = tokio::time::timeout(
                    std::time::Duration::from_millis(250),
                    conn.acp.notify("session/cancel", json!({"sessionId":sid})),
                )
                .await;
            }
            conn.kill().await;
        }
        self.finish_turn(&Ok(json!({"stopReason":"cancelled"})));
    }

    pub(super) async fn on_cmd(&mut self, cmd: Cmd, done_tx: &mpsc::UnboundedSender<super::PromptDone>) -> bool {
        match cmd {
            Cmd::ForkContext { request, reply } => self.context_call(request, reply, done_tx).await,
            Cmd::ContextTeam { context, name, args, reply } => {
                let hub = self.hub.clone();
                // Stop é ordenado pelo mesmo ator; updateBot publica sob esta barreira.
                let _boundary = hub.store.memory_boundary.locked();
                let result = (|| {
                    anyhow::ensure!(!reply.is_closed() && !self.stop_requested, "team_admission_cancelled");
                    context.check_actor(&hub, &self.cfg, &self.lane, self.turn_session.as_deref())?;
                    crate::chat::team::admit(&hub, &self.cfg.id, &name, &args, &context)
                })();
                let _ = reply.send(result);
            }
            Cmd::ContextTool { context, command } => {
                let checked = context
                    .check_actor(&self.hub, &self.cfg, &self.lane, self.turn_session.as_deref())
                    .and_then(|()| {
                        anyhow::ensure!(!self.stop_requested, "turn_stopping");
                        Ok(())
                    });
                let execution = context.turn.execution.clone();
                match *command {
                    Cmd::SendToUser { text, execution: _, reply } => {
                        let _ = reply.send(checked.and_then(|()| self.send_to_user(text, execution.as_ref())));
                    }
                    Cmd::React { emoji, to, execution: _, reply } => {
                        let _ =
                            reply.send(checked.and_then(|()| self.react(&emoji, to.as_deref(), execution.as_ref())));
                    }
                    Cmd::AskOwner { text, execution: _, reply } => {
                        let _ = reply.send(checked.and_then(|()| self.ask_owner(text, execution.as_ref())));
                    }
                    Cmd::ShowChoice { invocation, payload, reply } => {
                        let _ = reply.send(checked.and_then(|()| self.show_choice(&context, &invocation, &payload)));
                    }
                    Cmd::ShowChart { invocation, payload, reply } => {
                        let _ = reply.send(checked.and_then(|()| self.show_chart(&context, &invocation, &payload)));
                    }
                    Cmd::SendFile { path, name, cancelled, execution: _, reply } => match checked {
                        Ok(()) => self.begin_file(path, name, cancelled, execution, reply, Some(context)),
                        Err(error) => {
                            let _ = reply.send(Err(error));
                        }
                    },
                    _ => tracing::warn!("comando contextual de ferramenta recusado"),
                }
            }
            Cmd::PrimaryNewSession { chat, revision, autoridade, reply } => {
                // Pedido expirado (`context_control_pending`): o recomeço tardio não roda.
                if reply.is_closed() {
                    return true;
                }
                let result = self.primary_new_session(&chat, revision, &autoridade).await;
                let _ = reply.send(result);
            }

            Cmd::SessionClose { session, expected, autoridade, reply } => {
                // Quem pediu já recebeu `session_control_pending`: o fechamento tardio não roda (resultado incerto
                // para o app, que relê `sessions`).
                if reply.is_closed() {
                    return true;
                }
                let result = self.session_close(&session, expected, &autoridade).await;
                let _ = reply.send(result);
            }
            Cmd::SpaceDeleteGuard { chats, all, reply, release } => {
                if !reply.is_closed() {
                    let busy = self.space_delete_busy(&chats, all);
                    if reply.send(busy).is_ok() && !busy {
                        // Nenhum comando/turno começa entre a decisão e o commit.
                        // Drop do remetente libera também em erro/cancelamento.
                        let _ = release.await;
                    }
                }
            }
            Cmd::SessionDelete { request, autoridade, reply } => {
                if reply.is_closed() {
                    return true;
                }
                let result = self.session_delete(&request, &autoridade);
                let _ = reply.send(result);
            }
            Cmd::SessionIdleClose { session } => {
                self.session_idle_close(&session);
            }
            Cmd::QueuedControl(pedido) => {
                // Quem pediu já recebeu `queued_control_pending`: o efeito tardio não roda.
                if pedido.reply.is_closed() {
                    return true;
                }
                let super::fork_fila::Pedido { entry_id, acao, autoridade, reply } = *pedido;
                let result = self.queued_control(entry_id, acao, &autoridade, done_tx).await;
                let _ = reply.send(result);
            }
            Cmd::ContextControl { chat, revision, entry_id, reset, reply } => {
                let result = self.context_control(&chat, revision, entry_id.as_deref(), reset).await;
                let _ = reply.send(result);
            }
            Cmd::RefreshTools => {
                self.tools_changed = true;
            }
            Cmd::Routine(id) => {
                // MX-145: re-drive do agendador após restart de uma rotina parada pelo teto.
                if self.ceiling_holds_work(&format!("routine:{id}")) {
                    return true;
                }
                // MX-263: a execução que espera a assinatura pertence à espera D72;
                // o re-drive do agendador não a reenfileira nem a duplica.
                if self.waiting.as_ref().is_some_and(|w| w.active() && w.routine_id() == Some(id.as_str())) {
                    return true;
                }
                self.queue.push_back(Queued::Routine(id));
                if self.turn.is_none() {
                    self.next_in_queue(done_tx).await;
                }
            }
            Cmd::Send { lane, entry_id, text } => {
                if self
                    .hub
                    .store
                    .entry(&entry_id)
                    .is_none_or(|entry| entry.kind == EntryKind::User.as_str() && entry.data["status"] != "queued")
                {
                    return true;
                }
                // O startup pode ter recuperado o SQLite enquanto este comando aguardava no canal.
                if self.queue_has_user(&entry_id)
                    || self.input.as_ref().is_some_and(|input| input.entries.contains(&entry_id))
                    || self.ceiling_holds_entry(&entry_id)
                {
                    return true;
                }
                if self.waiting_active()
                    && let Some(mut entry) = self.hub.store.entry(&entry_id)
                {
                    entry.data["subscriptionQueued"] = json!(true);
                    self.hub.set_entry(&entry_id, &entry.data);
                }
                if self.hub.store.entry(&entry_id).is_some_and(|entry| entry.kind == EntryKind::User.as_str()) {
                    self.compaction_reset(&lane);
                }
                self.queue.push_back(Queued::User { lane, entry_id, text });
                if self.turn.is_none() {
                    self.next_in_queue(done_tx).await;
                }
            }
            Cmd::Group(turn) => {
                self.queue.push_back(Queued::Group(turn));
                if self.turn.is_none() {
                    self.next_in_queue(done_tx).await;
                }
            }
            Cmd::CancelGroup { chat } => {
                // Dropping a queued turn's reply tells the room it won't come.
                self.queue.retain(|q| !matches!(q, Queued::Group(g) if g.lane.chat == chat));
                if let Err(error) = self.drop_ceiling_holds(&super::fork_teto::CeilingDrop::Group { chat: &chat }) {
                    tracing::error!(%error, "cancelamento de espera do teto não durável; item segue parado");
                }
                if self.active_group.as_ref().is_some_and(|g| g.lane.chat == chat) {
                    self.cancel_turn().await;
                }
            }
            Cmd::BotRequest(ask) => {
                self.queue.push_back(Queued::BotRequest(ask));
                if self.turn.is_none() {
                    self.next_in_queue(done_tx).await;
                }
            }
            Cmd::CancelAsk { id } => {
                self.queue.retain(|q| !matches!(q, Queued::BotRequest(a) if a.expects_reply() && a.id == id));
                if let Err(error) = self.drop_ceiling_holds(&super::fork_teto::CeilingDrop::Ask { id: &id }) {
                    tracing::error!(%error, "cancelamento de espera do teto não durável; item segue parado");
                }
                if self.active_request.as_ref().is_some_and(|a| a.expects_reply() && a.id == id) {
                    if self.pending_admission.is_some() {
                        self.cancel_turn().await;
                        return true;
                    }
                    self.hub.team.cancel_from(&self.cfg.id);
                    self.stop_requested = true;
                    // The request has expired. Kill only its process so a harness
                    // ignoring cooperative cancellation cannot block the queue.
                    if let Some(c) = self.conn.take() {
                        c.kill().await;
                    }
                }
            }
            Cmd::SendToUser { text, execution, reply } => {
                let _ = reply.send(self.send_to_user(text, execution.as_ref()));
            }
            Cmd::RoutineResult { execution, declaration, reply } => {
                let _ = reply.send(self.routine_result(&execution, declaration));
            }
            Cmd::React { emoji, to, execution, reply } => {
                let _ = reply.send(self.react(&emoji, to.as_deref(), execution.as_ref()));
            }
            Cmd::AskOwner { text, execution, reply } => {
                let _ = reply.send(self.ask_owner(text, execution.as_ref()));
            }
            Cmd::ShowChoice { reply, .. } | Cmd::ShowChart { reply, .. } => {
                let _ = reply.send(Err(anyhow!(crate::fork::genui::flow::MAIN_REFUSAL)));
            }
            Cmd::SendFile { path, name, cancelled, execution, reply } => {
                self.begin_file(path, name, cancelled, execution, reply, None);
            }
            Cmd::UsageWaitChanged { wait_id } => {
                self.sync_usage_control().await;
                if let Ok(Some(saved)) = crate::fork::usage_limits::cancellation(&self.hub.store, &wait_id)
                    && let Err(error) = self.publish_usage_cancelled(&saved)
                {
                    tracing::error!(%error, "não foi possível publicar a desistência");
                }
                self.next_in_queue(done_tx).await;
            }
            Cmd::CeilingChanged => {
                self.queue_retry_at = None;
                self.ceiling_next_check = None;
                self.ceiling_probe = None;
                self.retry_admission(done_tx).await;
            }
            Cmd::UsageWaitResume { target, reply, origem } => {
                let result = self.resume_usage_wait(&target, origem.as_ref());
                let admitted = result.is_ok();
                let _ = reply.send(result);
                if admitted {
                    self.retry_waiting(done_tx).await;
                }
            }
            Cmd::RetrySubscription { entry_id } => {
                self.retry_subscription(&entry_id);
                self.retry_waiting(done_tx).await;
            }
            Cmd::ContextRetry { chat, revision, entry_id, reply } => {
                let result = self.context_retry(&chat, revision, &entry_id);
                // A resposta só confirma a transição durável; o consumidor é este ator.
                let admitted = result.is_ok();
                let _ = reply.send(result);
                if admitted {
                    self.retry_waiting(done_tx).await;
                }
            }
            Cmd::Stop => self.stop().await,
            Cmd::Permission { entry_id, option_id, reply, origem } => {
                self.check_antigravity_turn().await;
                let _ = reply.send(self.answer_permission_from(&entry_id, option_id, origem.as_ref()).await);
            }
            Cmd::Approve { title, detail, execution, reply } => {
                if let Some(execution) = execution
                    && crate::fork::sessoes::bindings::recheck(&self.hub, &execution).is_ok()
                {
                    self.ask_approval(&title, &detail, execution, reply);
                }
            }
            Cmd::Reconfigure(cfg) => {
                self.check_antigravity_turn().await;
                let mut cfg = *cfg;
                if self.cfg.context.is_some() {
                    match crate::fork::spaces::execution_config(&self.hub.store, &cfg, &self.lane.chat) {
                        Ok(contextual) => cfg = contextual,
                        Err(error) => {
                            tracing::warn!(%error,"context reconfiguration deferred until next turn");
                            return true;
                        }
                    }
                }
                // Compara com a mesma sessão do último turno. D372: a conversa com seleção própria não muda com o
                // padrão do bot, e conserva a do turno capturado; a troca da conversa vale no próximo turno.
                // Rotina e canal em andamento seguem o padrão do bot (domínio dele): não aplicam a seleção da principal.
                let own_domain = self.active_routine.is_none() && self.active_group.is_none();
                let applied = if own_domain {
                    crate::fork::sessoes::admissao::apply_choice(&self.hub.store.db.locked(), &self.lane, &mut cfg)
                } else {
                    Ok(false)
                };
                match applied {
                    Ok(true) => crate::fork::sessoes::admissao::keep_turn_engine(&self.cfg, &mut cfg),
                    Ok(false) => {}
                    Err(error) => {
                        tracing::warn!(%error, "session reconfiguration deferred until next turn");
                        return true;
                    }
                }
                // The harness reads its own memory's switch when it starts: an agent that can resume its session
                // restarts into it, one that can't starts afresh.
                let memory_changed = own_memory::kept(&cfg) != own_memory::kept(&self.cfg);
                let resumes = self.conn.as_ref().is_none_or(|c| c.sessions.load);
                let restart = cfg.fork_editor != self.cfg.fork_editor
                    || cfg.backend != self.cfg.backend
                    || cfg.command != self.cfg.command
                    || cfg.cwd != self.cfg.cwd
                    || cfg.model != self.cfg.model
                    || cfg.model_options != self.cfg.model_options
                    || (memory_changed && !resumes);
                self.tools_changed |= cfg.connectors != self.cfg.connectors
                    || cfg.computer != self.cfg.computer
                    || cfg.manage_team != self.cfg.manage_team
                    || memory_changed;
                // Live sessions switch before their next turn (see `fast_mode`).
                let fast_changed = cfg.fast_mode != self.cfg.fast_mode;
                // Name, description and skill changes reach the agent as a profile update
                // on the next message (see `context`).
                let switch_waiting = self.waiting_for_reset()
                    && self.cfg.context.is_none()
                    && (cfg.backend != self.cfg.backend
                        || cfg.model != self.cfg.model
                        || cfg.command != self.cfg.command);
                // DON-907 C9a: a linha de troca e, entre provedores, a passagem; antes de esquecer a sessão.
                self.engine_switched(&cfg, switch_waiting);
                if cfg.backend != self.cfg.backend || cfg.fork_editor != self.cfg.fork_editor {
                    // IDs ACP pertencem ao motor anterior; o transcript do Hub continua preservado.
                    // Extras têm carimbo próprio e escolha fixada; a troca do bot não as invalida (PR4).
                    for slot in self.run_slots() {
                        if !slot.starts_with(super::session::EXTRA_SLOT) {
                            self.set_session(Some(&slot), None);
                        }
                    }
                    if !switch_waiting {
                        self.forget_session();
                    }
                }
                self.cfg = cfg;
                if switch_waiting {
                    if let Err(error) = self.switch_waiting_model() {
                        self.pause_waiting_switch(&error);
                    }
                    self.restart_agent().await;
                    self.retry_waiting(done_tx).await;
                }
                if fast_changed {
                    self.fast_mode_changed();
                }
                if memory_changed {
                    // Claude Code's notes come or go on the memory screen.
                    memory::screen_changed(&self.hub, &self.cfg.id);
                }
                // The running turn finishes first: it may be the bot changing its own model (`update_bot`).
                if restart && self.turn.is_some() {
                    self.restart_after_turn = true;
                } else if restart {
                    self.restart_agent().await;
                }
            }
            Cmd::Shutdown => {
                // O texto e as ferramentas que o motor já mandou vão para o disco; pedidos dele (aprovação, leitura)
                // ficam sem resposta e o motor é encerrado. O corte é marcado depois, sem ator vivo.
                let queued = self.conn.as_ref().map_or(0, |c| c.rx.len());
                for _ in 0..queued {
                    let Some(incoming) = self.conn.as_mut().and_then(|c| c.rx.try_recv().ok()) else { continue };
                    let crate::agent::acp::Incoming::Notification { method, params } = incoming else {
                        if let crate::agent::acp::Incoming::Request { method, .. } = incoming {
                            tracing::warn!(
                                bot = self.cfg.id,
                                method,
                                "pedido do motor sem resposta: o Hub está desligando"
                            );
                        }
                        continue;
                    };
                    if method == "session/update"
                        && self.turn.is_some()
                        && !self.waiting_for_reset()
                        && !self.completing()
                        && params["sessionId"].as_str() == self.turn_session.as_deref()
                        && matches!(
                            params["update"]["sessionUpdate"].as_str(),
                            Some("agent_message_chunk" | "tool_call" | "tool_call_update")
                        )
                    {
                        self.on_update(&params["update"]);
                    }
                }
                if !self.completing() {
                    self.flush(true);
                }
                return false;
            }
        }
        true
    }

    pub(super) async fn stop(&mut self) {
        // Congela o alcance do Stop antes de aguardar o encerramento do turno.
        self.cancel_queued = self
            .queue
            .iter()
            .filter_map(|queued| match queued {
                Queued::User { entry_id, .. } | Queued::RestoreUser { entry_id, .. } => Some(entry_id.clone()),
                _ => None,
            })
            .collect();
        match self.hub.store.queued_user_entries_for_identity(&self.cfg.id) {
            Ok(entries) => self.cancel_queued.extend(entries.into_iter().map(|entry| entry.id)),
            Err(error) => tracing::error!(%error, "não foi possível ler a fila durável ao parar"),
        }
        self.cancel_queued.sort();
        self.cancel_queued.dedup();
        self.hub.team.cancel_from(&self.cfg.id);
        if let Err(error) = self.hub.routines.cancel_queued(&self.hub.store, &self.cfg.id) {
            tracing::error!(error = format!("{error:#}"), "couldn't cancel queued routines");
        }
        if self.active_routine.is_none() {
            self.hub.routines.release(&self.cfg.id);
        }
        // Queued messages are dropped too: Stop means "stop everything".
        self.queue.clear();
        for entry_id in &self.cancel_queued {
            if let Some(mut e) = self.hub.store.entry(entry_id) {
                e.data["status"] = "cancelled".into();
                self.hub.set_entry(entry_id, &e.data);
            }
        }
        self.cancel_turn().await;
    }

    /// Stops the running turn only (queued ones stay).
    pub(super) async fn cancel_turn(&mut self) {
        self.hub.pontes.revogar_bot(&self.cfg.id);
        self.cancel_turn_kept_bridges().await;
    }

    /// Sessões PR5: para o turno ou a espera em andamento sem revogar pontes de outros slots; quem chama já revogou
    /// as do slot da própria sessão.
    pub(super) async fn cancel_turn_kept_bridges(&mut self) {
        let pending = match self.cancel_pending_admission() {
            Ok(pending) => pending,
            Err(error) => {
                tracing::error!(%error, "não foi possível salvar a parada anterior ao despacho");
                return;
            }
        };
        if pending {
            self.stop_requested = true;
            self.finish_turn(&Ok(json!({"stopReason":"cancelled"})));
            return;
        }
        if self.waiting_for_reset() {
            self.stop_requested = true;
            self.finish_turn(&Ok(json!({"stopReason": "cancelled"})));
            return;
        }
        if self.turn.is_none() {
            return;
        }
        self.stop_requested = true;
        if self.profile_skills.is_some()
            && let Some(ticket) = &self.dispatch
            && let Err(error) = crate::fork::bot_admission::request_stop(&self.hub.store, &self.cfg.id, ticket)
        {
            // Não continuar com uma entrega cujo registro não pôde ser revogado.
            tracing::error!(%error, "não foi possível registrar parada da leitura de skills");
            if let Some(c) = self.conn.take() {
                c.kill().await;
            }
        }
        if let Some(c) = &self.conn {
            c.acp.cancel_scoped();
        }
        if self.cfg.context.is_some() {
            self.hub.set_memory_runtime(&self.cfg.id, |_| {});
        }
        self.cancel_deadline.get_or_insert_with(|| Instant::now() + CANCEL_GRACE);
        self.files.cancel();
        let ids: Vec<String> = self.perms.keys().cloned().collect();
        for id in ids {
            let _ = self.answer_permission(&id, None).await;
        }
        if let (Some(c), Some(sid)) = (&mut self.conn, &self.turn_session) {
            if c.skill_reads.is_empty() {
                let _ = c.acp.notify("session/cancel", json!({"sessionId": sid})).await;
            } else {
                // O cancelamento não pode esperar atrás do pipe que está cancelando.
                let (acp, sid) = (c.acp.clone(), sid.clone());
                c.skill_reads.spawn(async move {
                    let _ = acp.notify("session/cancel", json!({"sessionId": sid})).await;
                });
            }
        }
    }

    /// Reinicia o processo, preservando os IDs principal e de rotinas para session/load.
    async fn restart_agent(&mut self) {
        self.files.cancel();
        if let Some(c) = self.conn.take() {
            c.kill().await;
        }
    }

    /// As entradas já são duráveis; reconstrói o prompt completo (anexos/citações incluídos).
    pub(super) fn restore_waiting_queue(&mut self) -> Result<()> {
        // Armar antes do I/O: erro não deixa o boot depender de fila já preenchida.
        self.queue_restore_retry_at = Some(Instant::now() + std::time::Duration::from_secs(5));
        self.persist_subscription_refusals()?;
        let entries = self.hub.store.queued_user_entries_for_identity(&self.cfg.id)?;
        let ids = entries
            .iter()
            .filter(|entry| !self.input.as_ref().is_some_and(|input| input.entries.contains(&entry.id)))
            // MX-145: a captura parada pelo teto já carrega estas entradas.
            .filter(|entry| !self.ceiling_holds_entry(&entry.id))
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>();
        self.requeue_humans_from_snapshot(&ids, &entries, None);
        self.restore_send_now(&entries);
        self.queue_restore_retry_at = None;
        self.retry_queue_restore();
        Ok(())
    }

    pub(super) async fn next_in_queue(&mut self, done_tx: &mpsc::UnboundedSender<super::PromptDone>) {
        // DON-907 C9a: a passagem de uma troca de provedor feita durante o turno que acabou, antes de qualquer envio.
        if self.turn.is_none() && self.restart_after_turn {
            self.engine_switch_after_turn();
        }
        if self.queue_restore_retry_at.is_some() || self.admission_blocked() || self.dispatch.is_some() {
            return;
        }
        if self.queue_retry_at.is_some_and(|at| Instant::now() < at) {
            return;
        }
        self.queue_retry_at = None;
        self.retry_queue_restore();
        if self.turn.is_none() && std::mem::take(&mut self.restart_after_turn) {
            self.restart_agent().await;
        }
        let main = Lane::main(&self.cfg.id);
        while self.turn.is_none()
            && self.waiting.is_none()
            && self.routine_completion.is_none()
            && self.queue_retry_at.is_none()
            && self.queue_restore_retry_at.is_none()
            && !self.queue.is_empty()
        {
            // Só origem ainda inválida fica estacionada; elegíveis conservam a
            // ordem original. A posição não muda durante o retry por entrada.
            // Conversa pausada por efeito incerto também fica: as outras passam à frente.
            let Some(next) = self.queue.iter().position(|q| match q {
                Queued::RestoreUser { .. } => false,
                Queued::User { lane, .. } => !self.conversation_held(&lane.chat),
                Queued::Routine(_) | Queued::BotRequest(_) => !self.conversation_held(&main.chat),
                Queued::Group(_) | Queued::Compact { .. } => true,
            }) else {
                return;
            };
            match self.queue.get(next) {
                Some(Queued::Routine(_)) => {
                    let Some(Queued::Routine(id)) = self.queue.remove(next) else { unreachable!("item é uma rotina") };
                    self.active_routine = Some(id.clone());
                    self.routine_timed_out = false;
                    if self.hub.routines.recovery(&self.cfg.id).is_some_and(|run| run.id == id) {
                        self.routine_deadline = Some(Instant::now() + self.hub.routines.timeout(&id));
                        self.recover_routine().await;
                        continue;
                    }
                    let (hub, bot, run) = (self.hub.clone(), self.cfg.id.clone(), id.clone());
                    let prepared = tokio::task::spawn_blocking(move || hub.routines.begin(&hub, &bot, &run))
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|result| result);
                    match prepared {
                        Ok(Some(prepared)) => {
                            self.routine_deadline = Some(Instant::now() + prepared.timeout);
                            let result = self.start_turn(prepared.lane, &[], &prepared.prompt, false, done_tx).await;
                            if let Err(error) = result {
                                self.start_failed(&error);
                            }
                        }
                        Ok(None) => {
                            self.active_routine = None;
                            self.hub.routines.release(&self.cfg.id);
                        }
                        Err(error) => {
                            if self.defer_chain_preparation(&error) {
                                continue;
                            }
                            self.finish_routine(
                                crate::fork::rotina_resultado::Observed::failed(
                                    crate::fork::rotina_resultado::Reason::StartFailed,
                                ),
                                Some(format!("Could not start routine: {error:#}")),
                                None,
                            );
                        }
                    }
                    continue;
                }
                Some(Queued::BotRequest(_)) => {
                    let Some(Queued::BotRequest(ask)) = self.queue.remove(next) else {
                        unreachable!("item é um pedido")
                    };
                    if ask.reply_closed() {
                        continue;
                    }
                    let ids = [ask.entry_id.clone()];
                    let prompt = ask.prompt.clone();
                    self.active_request = Some(ask);
                    let result = self.start_turn(main.clone(), &ids, &prompt, false, done_tx).await;
                    if let Err(e) = result {
                        self.start_failed(&e);
                    }
                    continue;
                }
                Some(Queued::Group(_)) => {
                    let Some(Queued::Group(turn)) = self.queue.remove(next) else {
                        unreachable!("item é um turno de grupo")
                    };
                    // A newer message in the room (or Stop) ended the room turn that queued this.
                    if turn.reply.is_closed() || !self.hub.groups.current(&turn.lane, turn.epoch) {
                        continue;
                    }
                    // Sala pausada para este membro: a rodada conta como passe, sem prompt.
                    if self.conversation_held(&turn.lane.chat) {
                        let _ = turn.reply.send(Err(anyhow!("conversation_paused")));
                        continue;
                    }
                    let (lane, prompt) = (turn.lane.clone(), turn.prompt.clone());
                    self.active_group = Some(turn);
                    let result = self.start_turn(lane, &[], &prompt, false, done_tx).await;
                    if let Err(e) = result {
                        self.start_failed(&e);
                    }
                    continue;
                }
                Some(Queued::Compact { .. }) => {
                    let Some(Queued::Compact { idle, manual }) = self.queue.remove(next) else {
                        unreachable!("item é uma compactação")
                    };
                    self.start_requested_compaction(idle, manual, done_tx).await;
                    continue;
                }
                _ => {}
            }
            // Messages sent while the agent worked go out together as its next turn
            // without crossing a bot request or another lane: each gets its own reply.
            let Some(Queued::User { lane, .. }) = self.queue.get(next) else { continue };
            // Uma extra roda em série no próprio slot (`session:<id>`); a admissão exige escolha fixada (PR4).
            let lane = lane.clone();
            // Extra que a classificação canônica não admitiria (ex.: restaurada no boot, conta trocada antes do
            // despacho) fica na fila, como na VG: sem reserva, sem pendência de admissão e sem virar prompt da principal.
            let held = match lane.session {
                Some(_) => crate::fork::sessoes::api::extra_admissible(&self.hub.store.db.locked(), &lane).err(),
                None => None,
            };
            if let Some(error) = held {
                tracing::warn!(%error, "sessão extra sem seleção roteável; fila conservada");
                self.hub.set_memory_runtime(&self.cfg.id, |r| {
                    r.status = BotStatus::Waiting;
                    r.set_activity("Aguardando execução nesta sessão.", Some(ActivityKind::WaitingSession));
                    r.started_at = None;
                    r.lane = Some(lane);
                });
                return;
            }
            let mut batch = Vec::new();
            let solo = self.take_send_now(next)
                || self.queue.get(next).is_some_and(
                    |q| matches!(q, Queued::User { entry_id, .. } if self.hub.antigravity_grants.contains(entry_id)),
                );
            let mut first = true;
            while let Some(Queued::User { lane: l, entry_id, .. }) = self.queue.get(next)
                && *l == lane
                && (first || (!self.send_now_marked(entry_id) && !self.hub.antigravity_grants.contains(entry_id)))
            {
                first = false;
                if let Some(Queued::User { entry_id, text, .. }) = self.queue.remove(next)
                    && self.hub.store.entry(&entry_id).is_some_and(|entry| entry.data["status"] == "queued")
                {
                    batch.push((entry_id, text));
                }
                if solo {
                    break;
                }
            }
            if batch.is_empty() {
                continue;
            }
            let ids: Vec<String> = batch.iter().map(|(id, _)| id.clone()).collect();
            let text = batch.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n\n");
            // A handoff waiting in the chat starts a new conversation with this message.
            if lane == main && self.handoff.is_none() {
                self.handoff = self.take_handoff();
                self.handoff_chat = Some(lane.clone());
            }
            if let Err(e) = self.start_turn(lane, &ids, &text, false, done_tx).await {
                let no_replay = crate::agent::backends::is_antigravity(&self.cfg.backend)
                    || ids.iter().any(|id| {
                        self.hub
                            .store
                            .entry(id)
                            .is_some_and(|entry| entry.data[crate::fork::antigravity::ENTRY_MARKER] == true)
                    });
                if no_replay {
                    for id in &ids {
                        if let Some(mut entry) = self.hub.store.entry(id)
                            && entry.data["status"] == "queued"
                        {
                            entry.data["status"] = json!("failed");
                            entry.data["error"] = json!(e.to_string());
                            self.hub.set_entry(id, &entry.data);
                        }
                    }
                } else {
                    self.stop_subscription_retry(&ids, &e);
                }
                self.start_failed(&e);
                if !no_replay {
                    self.requeue_unreserved_humans(&ids);
                }
            }
        }
    }

    /// Repete somente preparação sem envio, na fila e com a reserva originais.
    fn defer_chain_preparation(&mut self, error: &anyhow::Error) -> bool {
        let persistence = error.downcast_ref::<crate::fork::cadeias::modelo::PreparacaoNaoPersistida>().is_some();
        if !persistence
            && !error.chain().any(|cause| {
                cause.downcast_ref::<crate::fork::atividades::modelo::CoreError>()
                    == Some(&crate::fork::atividades::modelo::CoreError::StoreBusy)
            })
        {
            return false;
        }
        // MX-263: a preparação falhou dentro da retomada de uma espera D72 — a
        // espera continua dona da execução e tenta de novo pelo próprio ciclo.
        // Reenfileirar tratá-la-ia como recuperação legada e interromperia a run.
        if self.rollback_routine_wait_retry() {
            return true;
        }
        let Some(id) = self.active_routine.take() else { return false };
        if persistence {
            tracing::error!(run = %id, error = format!("{error:#}"), "não foi possível salvar a preparação da rotina; reserva mantida");
        }
        self.queue.push_front(Queued::Routine(id));
        self.queue_retry_at = Some(Instant::now() + std::time::Duration::from_secs(1));
        self.turn = None;
        self.input = None;
        self.routine_deadline = None;
        self.hub.set_memory_runtime(&self.id(), |r| {
            r.status = BotStatus::Error;
            r.set_activity(
                if persistence {
                    "Não foi possível salvar a preparação da rotina. Aguardando nova tentativa."
                } else {
                    "Aguardando para salvar a tentativa da rotina."
                },
                Some(ActivityKind::RoutineSetupFailed),
            );
            r.started_at = None;
            r.lane = None;
            r.session = None;
        });
        true
    }

    pub(super) fn start_failed(&mut self, e: &anyhow::Error) {
        if self.defer_chain_preparation(e) {
            return;
        }
        // Retenção deliberada da conversa pausada: o aviso da pausa já está nela. O turno termina sem aviso de erro e o
        // bot volta a ficar ocioso. Rotina segue o caminho de falha, para não ser registrada como concluída.
        if self.active_routine.is_none()
            && e.chain().any(<dyn std::error::Error>::is::<crate::fork::bot_admission::ConversationPaused>)
        {
            self.finish_turn(&Err(crate::fork::bot_admission::ConversationPaused.into()));
            return;
        }
        // Conservar RpcError tipado: hint de uso não transforma uma falha comum em limite.
        let rpc = e.chain().find_map(|cause| cause.downcast_ref::<crate::agent::acp::RpcError>());
        let done = Err(if e.is::<crate::fork::antigravity::ModeUnconfirmed>() {
            crate::fork::antigravity::ModeUnconfirmed.into()
        } else {
            rpc.map_or_else(
                || anyhow!("{e:#}"),
                |rpc| {
                    crate::agent::acp::RpcError {
                        code: rpc.code,
                        message: rpc.message.clone(),
                        kind: rpc.kind.clone(),
                        data: rpc.data.clone(),
                    }
                    .into()
                },
            )
        });
        if self.result_limit(&done).is_some() && self.turn.is_some() {
            self.finish_turn(&done);
            return;
        }
        if self.waiting.is_some() {
            self.finish_turn(&done);
            return;
        }
        match self.defer_unpermitted_human(e) {
            Ok(true) => {
                self.invite_sign_in(e);
                tracing::warn!(
                    error = format!("{e:#}"),
                    "falha anterior à permissão tratada pela elegibilidade durável"
                );
                return;
            }
            Err(error) => {
                // Não decidir replay nem apagar a captura se sua fase ficou ilegível.
                tracing::error!(%error, "fase de despacho indisponível; setup conservado");
                if let Some(input) = self.input.clone() {
                    self.defer_setup(self.lane.clone(), &input.entries, &input.text, input.hidden);
                    return;
                }
            }
            Ok(false) => {}
        }
        if self.turn.is_some() {
            match crate::fork::memory_auto::queue::active_job(&self.hub, &self.cfg.id) {
                Ok(Some(_)) => {
                    // O job já existe, mesmo se compose/validação falhou antes de session/prompt.
                    // Completing conserva a falha e adia só a publicação se a transação falhar.
                    self.finish_turn(&done);
                    return;
                }
                Err(error) => {
                    tracing::error!(%error, "não foi possível verificar o job; conclusão será repetida sem prompt");
                    self.defer_completion(&done);
                    return;
                }
                Ok(None) => {}
            }
        }
        if self.turn.is_some() {
            self.report_turn(false, Some(e), "start_failed");
        }
        // I-023: o convite publicado (não a palavra "auth") decide `signInRequested`; o convite não muda.
        let invited = self.invite_sign_in(e);
        let reason = if invited {
            crate::fork::rotina_resultado::Reason::SignInRequested
        } else {
            crate::fork::rotina_resultado::Reason::StartFailed
        };
        let subscription_refused = e.downcast_ref::<crate::fork::assinatura_pessoal::Refusal>().is_some();
        let mut observed = crate::fork::rotina_resultado::Observed::failed(reason);
        let anthropic_refused = e.downcast_ref::<crate::fork::anthropic_automation::Refusal>().copied();
        observed.terminal = subscription_refused || anthropic_refused.is_some();
        let anthropic_notice = anthropic_refused.map(|reason| reason.to_string());
        self.finish_routine(
            observed,
            Some(anthropic_notice.clone().unwrap_or_else(|| format!("Could not start routine: {e:#}"))),
            None,
        );
        self.complete_request(
            Err(anthropic_refused.map_or_else(|| anyhow!("couldn't start recipient: {e:#}"), anyhow::Error::from)),
            false,
            crate::fork::retornos::Fim::FalhaInicio,
        );
        let room_refusal = self.active_group.is_some() && subscription_refused;
        if room_refusal
            && self
                .hub
                .add_entry(
                    &self.lane,
                    EntryKind::Notice,
                    self.hub.store.max_turn(&self.lane.chat),
                    &json!({"text":e.to_string(),"style":"error","author":self.cfg.id}),
                )
                .is_none()
        {
            tracing::warn!("não foi possível publicar a recusa no canal");
        }
        self.complete_group(Err(anyhow!("couldn't start: {e:#}")));
        if !invited && !room_refusal {
            let text = e
                .downcast_ref::<crate::fork::assinatura_pessoal::Refusal>()
                .map_or_else(|| format!("Couldn't start the agent: {e:#}"), ToString::to_string);
            self.notice(
                if e.is::<crate::fork::antigravity::ModeUnconfirmed>() {
                    crate::fork::antigravity::MODE_UNCONFIRMED
                } else {
                    anthropic_notice.as_deref().unwrap_or(&text)
                },
                NoticeStyle::Error,
            );
        }
        self.turn = None;
        self.input = None;
        if self.routine_completion.is_none() {
            self.finish_dispatch();
        }
        self.hub.set_memory_runtime(&self.id(), |r| {
            r.status = BotStatus::Error;
            r.preparing_turn_token = None;
            r.resume_at = None;
            r.set_activity("Couldn't start", Some(ActivityKind::StartFailed));
            r.started_at = None;
            r.lane = None;
            r.message = None;
            r.session = None;
        });
        self.hub.team.cancel_from(&self.cfg.id);
    }

    /// When `e` asks for signing in: marks the harness signed out and puts a sign-in card in the
    /// chat, which sends this turn's messages again once the user has signed in. Whether it did.
    pub(super) fn invite_sign_in(&self, e: &anyhow::Error) -> bool {
        let backend = &self.cfg.backend;
        if crate::agent::backends::is_antigravity(backend) {
            return false;
        }
        let Some(h) = crate::agent::backends::harness(backend).filter(|_| crate::agent::acp::needs_sign_in(e)) else {
            return false;
        };
        crate::agent::backends::set_signed_in(backend, Some(false));
        let invited =
            requests::invite_agent_sign_in(&self.hub, &self.cfg.id, &self.lane, backend, h.name, &self.turn_entries);
        invited
            .inspect_err(
                |error| tracing::warn!(bot = %self.cfg.id, error = format!("{error:#}"), "couldn't ask to sign in"),
            )
            .is_ok()
    }

    /// O término R1 é gravado antes do oneshot/avisos; sem gravação, falha segura no canal.
    pub(super) fn complete_request(
        &mut self,
        result: Result<String>,
        cancelled: bool,
        fim: crate::fork::retornos::Fim,
    ) {
        if let Some(ask) = self.active_request.take() {
            let result = match ask.record_end(&self.hub.store, &self.cfg.id, fim) {
                crate::fork::retornos::Registro::ErroDeGravacao => {
                    Err(anyhow!("{}", crate::fork::retornos::ERRO_DE_GRAVACAO))
                }
                crate::fork::retornos::Registro::Decidido => Err(anyhow!("retorno_pedido_ja_decidido")),
                crate::fork::retornos::Registro::CorpoRetido => Err(anyhow!("retorno_corpo_retido")),
                crate::fork::retornos::Registro::Gravado | crate::fork::retornos::Registro::Legado => result,
            };
            ask.complete(result, cancelled);
        }
    }

    pub(super) fn complete_group(&mut self, result: Result<Option<String>>) {
        if let Some(turn) = self.active_group.take() {
            let _ = turn.reply.send(result);
        }
    }

    pub(super) fn finish_routine(
        &mut self,
        observed: crate::fork::rotina_resultado::Observed,
        detail: Option<String>,
        text: Option<String>,
    ) {
        if let Some(id) = self.active_routine.take() {
            self.routine_attempt = None;
            self.routine_completion = Some(RoutineCompletion { id, observed, detail, text });
            self.routine_deadline = None;
            self.retry_routine_completion();
        }
    }

    /// If SQLite is temporarily unavailable, keep the result and block this
    /// actor's next turn until the durable completion has been saved.
    pub(super) fn retry_routine_completion(&mut self) -> bool {
        let Some(completion) = &self.routine_completion else {
            return true;
        };
        match self.hub.routines.complete(
            &self.hub,
            &completion.id,
            &completion.observed,
            completion.detail.clone(),
            completion.text.clone(),
        ) {
            Ok(()) => {
                // A finished run's session is never resumed.
                let slot = crate::routines::session_slot(&completion.id);
                self.routine_completion = None;
                self.set_session(Some(&slot), None);
                true
            }
            Err(error) => {
                tracing::error!(
                    error = format!("{error:#}"),
                    run = completion.id,
                    "couldn't save routine result; will retry"
                );
                false
            }
        }
    }

    /// A negativa legada continua no produtor #245, inclusive seu rollback/retry.
    /// Uma cadeia confiável incerta não passa por `prepare_attempt` só para interromper.
    pub(super) async fn recover_routine(&mut self) {
        if self.restart_admission || self.dispatch.is_some() {
            return;
        }
        if crate::fork::bot_admission::paused(&self.hub.store, &self.cfg.id).unwrap_or(true) {
            self.resume_interrupted(Lane::main(&self.cfg.id));
            return;
        }
        if let Some(id) = self.active_routine.clone()
            && self.hub.routines.recovery_without_receipt(&self.cfg.id, &id)
        {
            let (hub, bot) = (self.hub.clone(), self.cfg.id.clone());
            let rejected =
                tokio::task::spawn_blocking(move || hub.routines.reject_legacy_recovery(&hub.store, &bot, &id))
                    .await
                    .map_err(anyhow::Error::from)
                    .and_then(|result| result);
            if let Err(error) = rejected
                && error.downcast_ref::<crate::fork::cadeias::modelo::Recusa>().is_none()
            {
                self.start_failed(&error);
                return;
            }
            // Recusa: a cadeia mudou desde a consulta; não preparar/contar nem enviar.
        }
        self.resume_interrupted(Lane::main(&self.cfg.id));
    }

    /// Legado sem checkpoint D72 não comprova identidade nem permissão de replay.
    pub(super) fn resume_interrupted(&mut self, lane: Lane) {
        if self.restart_admission || self.dispatch.is_some() {
            return;
        }
        if crate::fork::bot_admission::paused(&self.hub.store, &self.cfg.id).unwrap_or(true) {
            if let Ok(legacy) = self.hub.store.kv_read(&crate::fork::bot_admission::key(&self.cfg.id)) {
                self.lane = lane.clone();
                self.turn = Some(self.hub.store.max_turn(&lane.chat));
                self.pending_admission = Some(super::fork_admission::Pending::Recovery { lane, legacy });
            }
            return;
        }
        if let Err(error) = crate::fork::bot_admission::clear_legacy_on(&self.hub.store.db.locked(), &self.cfg.id) {
            tracing::error!(%error, "evidência legada preservada; recuperação bloqueada");
            return;
        }
        self.lane = lane.clone();
        self.turn = None;
        self.input = None;
        self.finish_routine(
            crate::fork::rotina_resultado::Observed::interrupted(),
            Some(
                "A execução anterior não tem checkpoint D72 com identidade comprovada. Confira antes de reenviar."
                    .into(),
            ),
            None,
        );
        self.hub.team.cancel_from(&self.cfg.id);
        self.hub.set_memory_runtime(&self.cfg.id, |r| {
            r.preparing_turn_token = None;
            r.status = BotStatus::Idle;
            r.clear_activity();
            r.started_at = None;
            r.lane = None;
            r.message = None;
            r.session = None;
        });
        if self.hub.add_entry(&lane, EntryKind::Notice, self.hub.store.max_turn(&lane.chat),
            &json!({"text":"O Hub reiniciou durante uma execução sem checkpoint de retomada comprovado. Confira o histórico antes de reenviar; nenhuma ação foi repetida automaticamente.","style":"info","author":self.cfg.id})).is_none()
        {
            tracing::error!("não foi possível salvar o aviso da execução legada");
        }
    }
}
