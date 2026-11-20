// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Starting a turn and wrapping it up: final reply, notices, replies to waiters, alerts and memory.

use super::{Actor, Conn, Done, Instructions, NoticeStyle, Queued, Reacted, Seg};
use crate::LockExt;
use crate::agent::acp::{self, Incoming};
use crate::analytics::{self, TurnOutcome, TurnSource};
use crate::chat::context::{self, Snapshot};
use crate::chat::memory;
use crate::fork::activity_code::ActivityKind;
use crate::fork::push_textos::{Corpo, Titulo};
use crate::hub::BotStatus;
use crate::remote::push::{self, AlertKind};
use crate::store::{BotConfig, Lane, now_ms};
use anyhow::{Context, Result, anyhow};
use serde_json::json;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Leads a `<bot-profile>` sent again after a compaction, so the agent drops an older copy it may still hold.
const PROFILE_AGAIN: &str =
    "This is your full, current profile and memory; it replaces any earlier bot profile in this conversation.";

impl Actor {
    /// This session's frozen instructions (reads memory files when re-rendering).
    pub(super) async fn snapshot(&self, session: &str) -> Result<Snapshot> {
        let (hub, cfg, session) = (self.hub.clone(), self.cfg.clone(), session.to_owned());
        let instructions = self.conn.as_ref().map_or(Instructions::FirstMessage, Conn::instructions);
        self.check_profile_origin()?;
        let prepared = self.profile_skills.clone();
        // The owner's words orient the opening only when it is over its cap; the snapshot stays frozen after.
        let query = self.turn_text.clone();
        let snapshot = tokio::task::spawn_blocking(move || {
            context::resolve_with_skills(
                &hub.store,
                &cfg,
                &session,
                instructions,
                || super::fork_profile_skills::project(&hub.store, &cfg, prepared.as_ref()),
                query.as_deref(),
            )
        })
        .await??;
        self.check_profile_origin()?;
        Ok(snapshot)
    }

    /// The prompt for `text` in session `sid`: the turn's header line, the turn context, then its profile, a handoff
    /// or a memory change, and the text; a profile update comes after the text.
    pub(super) async fn compose(&mut self, sid: &str, text: &str) -> Result<String> {
        let instructions = self.conn.as_ref().map_or(Instructions::FirstMessage, Conn::instructions);
        // D6: a sessão fica com a pessoa da conversa (ou marcada sem pessoa); na primeira vez, a memória inteira
        // chega com a nota dela.
        memory::bind_profile_person(&self.hub.store, self.cfg.session_owner(), sid, self.cfg.person.as_deref())?;
        let mut snapshot = self.snapshot(sid).await?;
        let header_turn = self.active_group.is_some() || self.active_request.is_some() || self.active_routine.is_some();
        let (head, text) = split_head(header_turn, text);
        let mut prompt = text.to_owned();
        // Um resumo da conversa privada só entra no turno dessa conversa; continua guardado para ela se outro vier antes.
        // Sessões PR5: o cartão capturado da lane vem do banco e continua na captura até o permit.
        if let Some(card) = self.has_session_card(&self.lane).then(|| self.turn_handoff(&self.lane)).flatten() {
            prompt = format!("{card}\n\n{prompt}");
        } else if self.handoff_belongs(&self.lane)
            && let Some(handoff) = self.handoff.take()
        {
            prompt = format!("{handoff}\n\n{prompt}");
        }
        // A compaction may have dropped the profile the session's first message carried.
        let resend = instructions.inline_context() && self.profile_resend.remove(sid);
        if self.session_fresh || resend {
            // The instructions reaching the agent now hold memory as it was when they were rendered.
            memory::opened(&self.hub.store, self.cfg.session_owner(), sid, &snapshot.memory)?;
        }
        if let Some((notice, revision)) =
            memory::change_notice(&self.hub.store, &self.cfg, sid, self.turn_text.as_deref())
        {
            prompt = format!("{notice}\n\n{prompt}");
            self.turn_memory_revision = revision;
        }
        if std::mem::take(&mut self.session_fresh) && instructions.inline_context() {
            prompt = format!("<bot-profile>\n{}\n</bot-profile>\n\n{prompt}", snapshot.system);
        } else if resend {
            context::mark_announced(
                &self.hub.store,
                self.cfg.session_owner(),
                &snapshot,
                snapshot.system_profile.clone(),
            )?;
            snapshot.announced.clone_from(&snapshot.system_profile);
            prompt = format!("<bot-profile>\n{PROFILE_AGAIN}\n\n{}\n</bot-profile>\n\n{prompt}", snapshot.system);
        }
        // D539: perfil do dono por turno; nunca em Canal, onde pode haver desconhecidos.
        prompt = format!(
            "{}{prompt}",
            crate::fork::memory::profile::turn(&self.hub.store, &self.cfg, self.active_group.is_some())
        );
        let skills = self.project_profile_skills()?;
        if let Some((update, text)) =
            context::profile_update_with_skills(&self.hub.store, &snapshot, &self.cfg, skills.as_deref())
        {
            prompt = format!("{prompt}\n\n{update}");
            self.announce = Some((snapshot, text));
        }
        // D395: o idioma da resposta do Berga, por turno.
        let entries = self.input.as_ref().map_or(&[][..], |input| input.entries.as_slice());
        prompt = format!(
            "{}{prompt}",
            crate::fork::idioma_app::turno(&self.hub.store, &self.cfg, entries, self.active_group.is_some())
        );
        // D371: regras do dono para delegar, por turno; nunca em turno de Canal.
        prompt = format!(
            "{}{prompt}",
            crate::fork::delegacao::turno(&self.hub.store, &self.cfg, self.active_group.is_some())
        );
        prompt = format!(
            "{}{prompt}",
            if self.cfg.context.is_some() {
                String::new()
            } else {
                crate::computers::context::block(&self.hub, &self.cfg, self.active_routine.is_some())
            }
        );
        Ok(turn_layout(head, &context::turn_context(), &prompt))
    }

    /// Whether the pending handoff is for a turn in `chat` of the conversation it was taken for: never a group turn, a
    /// request from another bot or a routine run (D229-6).
    pub(super) fn handoff_belongs(&self, lane: &Lane) -> bool {
        self.active_group.is_none()
            && self.active_request.is_none()
            && self.active_routine.is_none()
            && self.handoff_chat.as_ref() == Some(lane)
    }

    /// A configuração de execução de um turno que começa em `lane` e se ele promoveu a seleção nova do dono (D372).
    /// DON-907 C9a: com a promoção, é o ponto único da troca da conversa principal, antes de fechar a conexão antiga.
    pub(super) fn turn_config(
        &mut self,
        lane: &Lane,
        entry_ids: &[String],
        hidden: bool,
        prepared_token: Option<&str>,
    ) -> Result<(BotConfig, bool)> {
        let original =
            self.hub.store.bot(&self.cfg.id)?.filter(|b| !b.deleted).ok_or_else(|| anyhow!("bot_not_found"))?.config;
        let mut cfg = crate::fork::spaces::execution_config(&self.hub.store, &original, &lane.chat)?;
        // D372: a conversa (principal ou extra do Pessoal) roda na própria seleção efetiva, capturada aqui para o
        // turno todo; canal e Space usam a fonte canônica. Só um turno humano NOVO (sem token preparado, sem espera,
        // sem ticket já reservado, não oculto, com entradas, fora de rotina/pedido/canal) promove a próxima seleção do
        // dono, na mesma Connection e sem await antes de capturar. Retry de setup/admissão, recuperação, retomada e os
        // demais domínios usam a efetiva.
        let new_human = prepared_token.is_none()
            && self.waiting.is_none()
            && self.dispatch.is_none()
            && !hidden
            && !entry_ids.is_empty()
            && self.active_request.is_none()
            && self.active_group.is_none()
            && self.active_routine.is_none();
        // Rotina e canal têm slot próprio e seguem o padrão do bot (domínio dele), nunca a seleção da principal.
        let own_domain = self.active_routine.is_none() && self.active_group.is_none();
        // C9a (R7): a fonte antiga da troca é a seleção EFETIVA desta conversa antes da promoção, lida na mesma
        // Connection; nunca `self.cfg`, que é a da última lane do ator (extra, rotina ou canal).
        let (promoted, previous) = if new_human {
            let db = self.hub.store.db.locked();
            let mut previous = cfg.clone();
            crate::fork::sessoes::admissao::apply_choice(&db, lane, &mut previous)?;
            (crate::fork::sessoes::admissao::start_choice(&db, lane, &mut cfg)?, previous)
        } else {
            if own_domain {
                crate::fork::sessoes::admissao::apply_choice(&self.hub.store.db.locked(), lane, &mut cfg)?;
            }
            (false, cfg.clone())
        };
        if promoted
            && *lane == Lane::main(&self.cfg.id)
            && super::fork_troca_motor::switch_kind(&previous, &cfg).is_some()
        {
            // Durante a troca, o ator é o da principal: configuração anterior dela (rótulo "de", sessão a esquecer e
            // passagem), restaurada depois para a decisão de fechar a conexão da última lane.
            let last = std::mem::replace(&mut self.cfg, previous);
            self.engine_switched_on_turn(&cfg, lane);
            self.cfg = last;
        }
        Ok((cfg, promoted))
    }

    /// Starts a turn in `lane` for the given user entries (none for a hidden turn).
    pub(super) async fn start_turn(
        &mut self,
        lane: Lane,
        entry_ids: &[String],
        text: &str,
        hidden: bool,
        done_tx: &mpsc::UnboundedSender<super::PromptDone>,
    ) -> Result<()> {
        self.start_prepared_turn(lane, entry_ids, text, hidden, done_tx, None).await
    }

    pub(super) async fn start_prepared_turn(
        &mut self,
        lane: Lane,
        entry_ids: &[String],
        text: &str,
        hidden: bool,
        done_tx: &mpsc::UnboundedSender<super::PromptDone>,
        prepared_token: Option<String>,
    ) -> Result<()> {
        self.clear_finished_before_turn()?;
        self.lane = lane.clone();
        // Sessões PR5: ponto canônico da captura do cartão da lane (recomeço da principal, continuação de uma extra) no
        // turno humano novo — o lote da fila e o preparo restaurado de uma reserva (`Pending::Setup`) passam aqui. Fora
        // dele (rotina, pedido, canal, oculto, retomada) não há captura. Consumo só na Tx do permit.
        if !hidden
            && !entry_ids.is_empty()
            && self.waiting.is_none()
            && self.active_request.is_none()
            && self.active_group.is_none()
            && self.active_routine.is_none()
        {
            self.capture_session_card(&lane)?;
        } else {
            self.session_card = None;
        }
        // F6-B: perto do limite ou esgotada, a conversa do dono passa para a próxima conta antes de capturar o turno.
        self.fallback_prepare(&lane, entry_ids, hidden, prepared_token.as_deref()).await;
        let (cfg, promoted) = self.turn_config(&lane, entry_ids, hidden, prepared_token.as_deref())?;
        if cfg.context != self.cfg.context
            || crate::agent::own_memory::kept(&cfg) != crate::agent::own_memory::kept(&self.cfg)
            || cfg.fork_editor != self.cfg.fork_editor
            || cfg.backend != self.cfg.backend
            || cfg.model != self.cfg.model
            || cfg.model_options != self.cfg.model_options
        {
            if let Some(conn) = self.conn.take() {
                conn.kill().await;
            }
            // A passagem desta conversa (troca promovida agora ou cartão tirado pela fila para esta mensagem) segue.
            // O buffer legado desta lane: o rollover que a fila tirou para esta mensagem (`handoff_card`) também segue.
            // O de outra lane não é apagado pela troca desta; a captura PR5 não está no buffer.
            if self.handoff_belongs(&lane) && !promoted && self.handoff_card.is_none() {
                self.handoff = None;
            }
            self.announce = None;
        }
        if cfg.context.is_some() || cfg.space_id != crate::fork::spaces::PERSONAL {
            anyhow::ensure!(cfg.fork_editor.engine_account_id.is_some(), "space_account_selection_required");
        }
        crate::fork::spaces::validate_entries(&self.hub, &cfg, entry_ids)?;
        // D372: a velocidade é da conversa. A troca do dono (promovida agora) libera uma recusa de Fast, como a edição
        // do bot fazia (`Cmd::Reconfigure`).
        let fast_changed = cfg.fast_mode != self.cfg.fast_mode;
        self.cfg = cfg;
        if fast_changed || (promoted && self.cfg.fast_mode) {
            self.fast_mode_changed();
        }
        if self.active_routine.as_deref() != self.routine_attempt.as_ref().map(|receipt| receipt.run_id.as_str()) {
            self.routine_attempt = None;
        }
        if self.routine_attempt.is_none()
            && let Some(id) = &self.active_routine
        {
            let (hub, bot, id) = (self.hub.clone(), self.cfg.id.clone(), id.clone());
            self.routine_attempt =
                Some(tokio::task::spawn_blocking(move || hub.routines.prepare_attempt(&hub.store, &bot, &id)).await??);
        }
        // Captura palavras e IDs originais antes de qualquer operação que possa atingir o limite.
        if self.waiting.is_none() {
            let slot = self.slot();
            let session = self.session_for(slot.as_deref());
            self.input = Some(super::waiting::TurnInput {
                chat: lane.chat.clone(),
                entries: entry_ids.to_vec(),
                text: text.to_owned(),
                hidden,
                handoff: self.turn_handoff(&lane),
                routine: self.active_routine.clone(),
                transient: self.active_request.is_some() || self.active_group.is_some(),
                backend: self.cfg.backend.clone(),
                model: self.cfg.model.clone(),
                command: self.cfg.command.clone(),
                session,
                context_binding: self
                    .cfg
                    .context
                    .as_ref()
                    .map(|_| crate::fork::spaces::execution_stamp(&self.hub.store, &self.cfg))
                    .transpose()?,
                visible_session: lane.session.clone(),
            });
        }
        self.turn_limit = None;
        // A routine run keeps the turn its notice took when it began (`Routines::begin`).
        let run_turn = self
            .active_routine
            .as_ref()
            .and_then(|id| self.hub.store.entry(&crate::routines::notice_id(id)))
            .map(|e| e.turn)
            .filter(|t| *t > 0);
        let turn = match entry_ids.last().and_then(|id| self.hub.store.entry(id)) {
            Some(entry) => entry.turn,
            None => match run_turn {
                Some(turn) => turn,
                // Grupo e rotina abrem um turno novo: número reservado de forma atômica no chat (D229-6).
                None if self.active_group.is_some() || self.active_routine.is_some() => {
                    self.hub.store.alloc_turn(&lane.chat)?
                }
                // Turno oculto (compactação) continua anexado ao turno corrente do chat.
                None => self.hub.store.max_turn(&lane.chat),
            },
        };
        self.lane = lane.clone();
        self.turn = Some(turn);
        // Only the user's own words feed memory: not another bot's, a room's or a routine's.
        let from_user = owner_words(
            hidden,
            NonOwner {
                request: self.active_request.is_some(),
                group: self.active_group.is_some(),
                routine: self.active_routine.is_some(),
            },
        );
        self.turn_text = from_user.then(|| self.input.as_ref().map_or(text, |i| i.text.as_str()).to_owned());
        self.turn_entries = if from_user { entry_ids.to_vec() } else { Vec::new() };
        self.turn_memory_revision.clear();
        self.announce = None;
        self.stop_requested = false;
        self.cost_turn = None;
        self.cancel_queued.clear();
        self.cancel_deadline = None;
        self.hub.team.start_turn_in(&self.cfg.id, self.active_request.as_ref(), self.routine_attempt.as_ref());
        self.exit_tail = None;
        self.seg = Seg::None;
        self.tools.clear();
        self.plan_entry = None;
        self.last_text = None;
        self.sent.clear();
        self.reacted = Reacted::No;
        self.files.summary = None;
        self.files.cancel();
        let restored = self.restored_execution_token(&self.cfg)?.or(prepared_token);
        // D372: a guarda de setup confere a conta desta execução capturada, não o padrão atual do bot.
        let captured = self.cfg.fork_editor.engine_account_id.clone();
        let (activity_text, kind) = match (self.compacting.is_some(), hidden) {
            (true, _) => ("Summarizing the earlier conversation…", ActivityKind::Summarizing),
            (false, true) => ("Picking up where it left off…", ActivityKind::ResumingWork),
            (false, false) => ("Starting…", ActivityKind::Starting),
        };
        let saved = self.hub.prepare_captured_turn(&self.id(), restored.as_deref(), captured.as_deref(), |r| {
            r.resume_at = None;
            r.set_activity(activity_text, Some(kind));
            r.started_at = Some(now_ms());
            r.session = None;
            r.lane = Some(lane.clone());
            r.message = if from_user { entry_ids.last().cloned() } else { None };
        })?;
        let fresh_dispatch = self.dispatch.is_none();
        if let Err(error) = self.reserve_dispatch() {
            tracing::error!(%error, "reserva não persistida; turno conservado antes do envio");
            self.defer_setup(lane, entry_ids, text, hidden);
            return Ok(());
        }
        if from_user && self.waiting.is_none() && !entry_ids.is_empty() {
            let ticket = self.dispatch.as_ref().ok_or_else(|| anyhow!("admission_dispatch_missing"))?;
            if let Err(error) = crate::fork::bot_admission::begin_human_setup(&self.hub.store, &self.cfg.id, ticket) {
                tracing::error!(%error, "orçamento de preparo não reservado; nenhum preparo será iniciado");
                self.defer_setup(lane, entry_ids, text, hidden);
                return Ok(());
            }
        }
        // Mesmo se Reconfigure ainda estiver na fila, este turno adota o perfil
        // que foi confirmado na preparação; Desfazer recusa também essa reserva.
        self.cfg = crate::store::restore_team_profile(&self.cfg, &saved);
        self.cfg.permission = crate::store::restrictive(self.cfg.permission, saved.permission);
        // O snapshot de equipe não pode recolocar o prompt pessoal em um contexto estrangeiro.
        if self.cfg.context.as_ref().is_some_and(|c| c.foreign_identity) {
            self.cfg = crate::fork::spaces::execution_config(&self.hub.store, &saved, &self.lane.chat)?;
        }

        // D6: a pessoa comprovada da conversa decide a nota "Sobre você"; sessão de outra pessoa não é reaproveitada.
        self.bind_profile_person(from_user, entry_ids)?;
        self.prepare_profile_skills(fresh_dispatch)?;

        let memory_backend = self.conn.as_ref().map_or(self.cfg.backend.as_str(), |c| match c.adapter {
            super::Adapter::Codex => "codex",
            super::Adapter::Claude => "claude",
            super::Adapter::Other | super::Adapter::Mexerica => self.cfg.backend.as_str(),
        });
        crate::fork::memory_auto::queue::capture_engine(&self.hub, &self.cfg, &self.lane, memory_backend)?;

        // Memory files changed since (in an editor, say) reach the opening or the next notice.
        if self.cfg.context.is_none() {
            let (hub, cfg) = (self.hub.clone(), self.cfg.clone());
            let _ = tokio::task::spawn_blocking(move || memory::files::scan(&hub, &cfg)).await;
        }
        // F6-B: sem conta livre na fila, o preparo para aqui (a reserva do despacho já existe e a fila tenta de novo).
        self.fallback_parar()?;
        let slot = self.slot();
        if let Some(deadline) = self.routine_deadline {
            tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), self.ensure_session(slot.as_deref()))
                .await
                .context("routine session setup timed out")??;
        } else {
            self.ensure_session(slot.as_deref()).await?;
        }
        if self.cfg.context.is_none() && self.waiting.is_some() && self.handoff.is_none() {
            // session/load indisponível: uma sessão nova recebe o mesmo handoff do rollover.
            if let Some(old) = self
                .input
                .as_ref()
                .and_then(|input| input.session.clone())
                .filter(|old| Some(old) != self.turn_session.as_ref())
            {
                let (text, person) =
                    crate::chat::handoff::build_in_with_person(&self.hub.store, &self.cfg.id, &self.cfg.id, &old)?;
                if person.is_some() && person == self.cfg.person {
                    self.handoff = Some(text);
                    self.handoff_chat = Some(self.lane.clone());
                }
            }
        }
        if let Some(sid) = self.turn_session.as_deref() {
            if self.session_fresh {
                let inherited = self.turn_handoff(&self.lane).is_some();
                self.hub.store.memory_session_start(&self.cfg.id, sid, inherited)?;
                // Sessões PR5 (D125): a ACP nova de uma extra exposta (por herança da continuação ou troca de ACP)
                // nasce marcada como externa; o resumo é dado, não instrução do dono.
                crate::fork::sessoes::exposure::carry(&self.hub.store, &self.cfg.id, &self.lane, sid)?;
                self.hub.set_runtime(&self.cfg.id, |r| r.memory_exposure_error = false);
            }
            self.hub.store.memory_session_turn(&self.cfg.id, &self.lane.chat, sid, turn)?;
            self.hub.observe_memory_session_inputs(&self.cfg.id, sid, entry_ids)?;
            let mut memory_turns = vec![turn];
            for entry in entry_ids.iter().filter_map(|id| self.hub.store.entry(id)) {
                if !memory_turns.contains(&entry.turn) {
                    memory_turns.push(entry.turn);
                }
            }
            let origin = if self.active_routine.is_some() {
                "routine"
            } else if self.active_group.is_some() {
                "group"
            } else if self.active_request.is_some()
                || !from_user
                || entry_ids.is_empty()
                || entry_ids.iter().any(|id| {
                    self.hub.store.entry(id).is_none_or(|entry| {
                        !entry.is_user_authored()
                            || !entry.data["sourceBotId"].is_null()
                            || !entry.data["runId"].is_null()
                    })
                })
            {
                "other"
            } else {
                "owner"
            };
            self.hub.store.memory_turn_begin(&self.cfg.id, &self.lane.chat, sid, &memory_turns, origin)?;
            if origin == "owner" {
                crate::fork::memory_auto::begin(&self.hub, &self.cfg, &self.lane, sid, entry_ids)?;
            }
        }
        let session = self.turn_session.clone();
        if let Some(input) = self.input.as_mut() {
            input.session.clone_from(&session);
        }
        crate::fork::memory::gateway::bind(&self.hub, &self.id(), session.as_deref())?;
        if let Some(sid) = session {
            self.sync_fast_mode(&sid).await;
        }
        if let Err(error) = self.checkpoint_waiting_session() {
            self.wait_for_checkpoint(&error);
            return Ok(());
        }
        if hidden && self.session_fresh {
            // A new session must never silently repeat an interrupted routine, and has nothing to compact.
            self.compacting = None;
            self.raw_prompt = false;
            self.finish_routine(
                crate::fork::rotina_resultado::Observed::interrupted(),
                Some("The agent could not reload this run's saved session; inspect before retrying".into()),
                None,
            );
            self.turn = None;
            if self.routine_completion.is_none() {
                self.finish_dispatch();
            }
            self.hub.team.cancel_from(&self.cfg.id);
            self.hub.set_memory_runtime(&self.id(), |r| {
                r.preparing_turn_token = None;
                r.status = BotStatus::Idle;
                r.clear_activity();
                r.started_at = None;
                r.lane = None;
                r.message = None;
                r.session = None;
            });
            return Ok(());
        }
        // Session-start chatter (banners, command lists) isn't part of the reply.
        // Some adapters (pi-acp) send it on a timer right after session/new.
        if self.session_fresh {
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        if let Some(conn) = self.conn.as_mut() {
            while let Ok(inc) = conn.rx.try_recv() {
                if let Incoming::Request { id, .. } = inc {
                    let _ = conn.acp.respond_error(id, -32601, "not supported").await;
                }
            }
        }
        let sid = self.turn_session.clone().ok_or_else(|| anyhow!("no session"))?;
        self.cost_begin(&sid)?;
        let prompt = if std::mem::take(&mut self.raw_prompt) {
            text.to_owned()
        } else {
            self.compose_reserved(&sid, text).await?
        };
        let params = json!({
            "sessionId": sid,
            "prompt": [{"type": "text", "text": prompt}],
        });
        self.send_admitted(params, entry_ids.to_vec(), sid, done_tx).await?;
        Ok(())
    }

    pub(super) fn subscription_limit_notice(&self) -> String {
        if let Some(aviso) = &self.fallback.aviso_limite {
            aviso.clone()
        } else if self.active_routine.is_some() {
            "Limite da assinatura atingido; esta rotina tenta de novo no próximo horário.".into()
        } else if self.compacting.is_some() {
            "Limite da assinatura atingido; a compactação foi encerrada. A fila está liberada.".into()
        } else {
            "Limite da assinatura atingido; este turno foi encerrado. Tente de novo quando a assinatura liberar.".into()
        }
    }

    /// Ferramentas e permissões não continuam vivas depois do fim ou bloqueio ACP.
    pub(super) fn close_turn_work(&mut self, failed: bool) {
        for (id, _) in self.perms.drain() {
            if let Some(mut entry) = self.hub.store.entry(&id) {
                entry.data["status"] = "expired".into();
                self.hub.set_entry(&id, &entry.data);
            }
        }
        for entry_id in self.tools.values() {
            if let Some(mut entry) = self.hub.store.entry(entry_id)
                && matches!(entry.data["status"].as_str(), Some("pending" | "in_progress"))
            {
                entry.data["status"] = if failed { "failed" } else { "completed" }.into();
                self.hub.set_entry(entry_id, &entry.data);
            }
        }
    }

    pub(super) fn finish_turn(&mut self, done: &Done) {
        if self.turn.is_none() {
            return;
        }
        if let Some(conn) = self.conn.as_mut() {
            conn.compaction_usage = super::CompactionUsage::Idle;
        }
        self.cost_finish(done);
        let limit = (!self.stop_requested).then(|| self.result_limit(done)).flatten();
        let limited = limit.is_some();
        self.fallback.aviso_limite = None;
        // F6-B (regra (b)): com reserva, a conta esgotada descansa e o pedido segue na próxima, sem espera D72.
        let fallback_trocou = limit.is_some_and(|limit| self.fallback_on_limit(limit.resets_at));
        if let Some(limit) = limit
            && !fallback_trocou
            && self.wait_on_limit(limit)
        {
            return;
        }
        if let Err(error) = self.checkpoint_completion(done) {
            tracing::error!(%error, "não foi possível publicar a conclusão; checkpoint será repetido");
            self.defer_completion(done);
            return;
        }
        if let (Some(conn), Some(sid)) = (self.conn.as_mut(), &self.turn_session)
            && let Some(used) = conn.loaded.get_mut(sid)
        {
            *used = Instant::now();
        }
        self.files.cancel();
        let stopped = self.stop_requested || matches!(&done, Ok(v) if v["stopReason"] == "cancelled");
        // Turno cortado: o trecho aberto vai inteiro para o disco ainda com `streaming`, que só a marca de interrompida
        // tira; se ela falhar, a varredura sem ator marca depois.
        let cut_early = stopped || done.is_err() || limited;
        let sealed = if cut_early {
            self.flush(true);
            !matches!(self.seg, Seg::Open { dirty: true, .. })
        } else {
            self.close_seg()
        };
        self.close_turn_work(stopped || done.is_err() || limited);
        let mut final_text = None;
        // R1: a entrada `agent` que originou a resposta, só referência (sem copiar o texto).
        let mut final_entry = None;
        // I-023: só a resposta final que o Hub de fato gravou com `final: true` conta como resposta visível.
        let mut visible_reply = false;
        let routine_id = self.active_routine.clone();
        let routine = routine_id.is_some();
        let grouped = self.active_group.is_some() || routine;
        let delegated = self.active_request.as_ref().is_some_and(crate::chat::team::BotRequest::expects_reply);
        let sent = std::mem::take(&mut self.sent);
        let reacted = std::mem::take(&mut self.reacted);
        if !sent.is_empty() {
            // The bot talked through send_message: what it wrote as its reply stays in the trace.
            self.last_text = None;
            final_text = Some(sent.join("\n\n"));
        } else if !matches!(reacted, Reacted::No) {
            // A reaction (or one it held back) was its whole reply: what it wrote stays in the trace too.
            self.last_text = None;
        }
        let last_text = self.last_text.take();
        // Fim que ficou só na memória (selagem que falhou em algum trecho do turno): o disco ainda diz `streaming`.
        let unsealed =
            last_text.as_ref().and_then(|id| self.hub.store.entry(id)).is_some_and(|e| e.data["streaming"] == true);
        let cut = cut_early || !sealed || unsealed;
        // Resposta cortada que vai à conversa: só a marca tira o `streaming`; se ela falhar, a varredura marca depois.
        let shows_cut = cut && !delegated && last_text.is_some();
        if shows_cut {
            if let Some(id) = &last_text {
                self.mark_interrupted(id);
            }
        } else {
            self.close_seg();
        }
        self.seg = Seg::None;
        if let Some(id) = last_text.filter(|_| !cut)
            && let Some(mut e) = self.hub.store.entry(&id)
        {
            let text = e.data["text"].as_str().map(str::to_owned);
            // A pass stays in the trace: neither a room nor the user sees it (an ask's reply goes back as it is).
            if delegated || !text.as_deref().is_none_or(crate::chat::group::is_pass) {
                // An ask's answer belongs to its requesting bot and exchange notice.
                // Independent messages still report to the user in this chat.
                if !delegated {
                    e.data["final"] = true.into();
                    let saved = self.hub.set_entry(&id, &e.data).is_some();
                    if !saved && self.waiting.is_some() {
                        self.defer_completion(done);
                        return;
                    }
                    visible_reply = saved && text.as_deref().is_some_and(|t| !t.trim().is_empty());
                }
                if text.is_some() {
                    final_entry = Some(id);
                }
                final_text = text;
            }
        }
        if final_text.is_none() {
            final_text = self.files.summary.take();
        }
        let stop_reason = match &done {
            Ok(v) => v["stopReason"].as_str().unwrap_or("end_turn").to_owned(),
            Err(_) => "error".into(),
        };
        let raw_stop_reason = done.as_ref().ok().and_then(|v| v["stopReason"].as_str()).map(str::to_owned);
        let mut invited = false;
        let limit_notice = self.subscription_limit_notice();
        let notice = match (&done, stop_reason.as_str()) {
            _ if limited && !stopped => self.try_notice(&limit_notice, NoticeStyle::Info),
            (Err(e), _) if !self.stop_requested => {
                let tail = self.exit_tail.take().filter(|t| !t.is_empty());
                if self.invite_sign_in(e) {
                    invited = true;
                    Ok(())
                } else {
                    let detail = tail.map(|t| format!("\n\n{}", acp::truncate(&t, 1200))).unwrap_or_default();
                    self.try_notice(&format!("The agent failed: {e}{detail}"), NoticeStyle::Error)
                }
            }
            (_, "cancelled") | (Err(_), _) => self.try_notice("Stopped.", NoticeStyle::Info),
            (_, "max_tokens") => self.try_notice("The agent hit its output limit.", NoticeStyle::Info),
            (_, "max_turn_requests") => {
                self.try_notice("The agent hit its step limit for this turn.", NoticeStyle::Info)
            }
            (_, "refusal") => self.try_notice("The agent declined to continue.", NoticeStyle::Info),
            _ => Ok(()),
        };
        if let Err(error) = notice {
            tracing::error!(%error, "não foi possível publicar o aviso final");
            if self.waiting.is_some() {
                self.defer_completion(done);
                return;
            }
        }
        let failure = done.as_ref().err().filter(|_| !self.stop_requested);
        let failed = failure.is_some() || limited;
        let reply = if stopped {
            Err(anyhow!("recipient was stopped; partial work may have happened"))
        } else if limited {
            Err(anyhow!("{limit_notice}"))
        } else if stop_reason != "end_turn" {
            Err(anyhow!("recipient did not finish ({stop_reason}); partial work may have happened"))
        } else {
            final_text
                .clone()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| anyhow!("recipient finished without a text reply"))
        };
        if routine {
            use crate::fork::rotina_resultado::{End, Observed, Reason};
            let (end, detail) = if self.routine_timed_out {
                (
                    End::Failed(Reason::Timeout),
                    Some("Routine exceeded its execution time limit; its agent process was stopped".into()),
                )
            } else if stopped {
                (End::Stopped, Some("Stopped by user".into()))
            } else if limited {
                (End::Failed(Reason::Limit), Some(limit_notice.clone()))
            } else if failed {
                let reason = if invited { Reason::SignInRequested } else { Reason::AgentError };
                (
                    End::Failed(reason),
                    Some(format!("Routine did not finish ({stop_reason}); inspect the run conversation")),
                )
            } else if raw_stop_reason.as_deref().is_some_and(|r| r != "end_turn") {
                (
                    End::Failed(Reason::EngineStop),
                    Some(format!("Routine did not finish ({stop_reason}); inspect the run conversation")),
                )
            } else {
                (End::Finished(raw_stop_reason.clone()), None)
            };
            // Encerramento imposto pela espera pausada é terminal: o catch-up
            // do MX-110 não reenfileira a run.
            let terminal = std::mem::take(&mut self.routine_terminal);
            self.finish_routine(Observed { end, visible_reply, terminal }, detail, final_text.clone());
            if self.routine_completion.is_some() && self.waiting.is_some() {
                self.defer_completion(done);
                return;
            }
        }
        if !self.clear_waiting() {
            self.defer_completion(done);
            return;
        }
        self.active_routine = routine_id;
        self.report_turn(stopped, failure, &stop_reason);
        self.after_turn(done);
        self.active_routine = None;
        memory::files::scan_after_turn(&self.hub, self.cfg.clone());
        let message_failed =
            !stopped && reply.is_err() && self.active_request.as_ref().is_some_and(|r| !r.expects_reply());
        // Proveniência capturada antes de limpar runtime/token abaixo.
        let token = self.hub.runtime(&self.cfg.id).memory_turn_token;
        let produtor = crate::fork::retornos::Produtor::de(self.turn_session.clone(), token);
        let fim = crate::fork::retornos::fim_do_turno(
            stopped,
            limited,
            failure.is_some(),
            &stop_reason,
            final_text.as_deref(),
            final_entry,
            produtor,
        );
        self.complete_request(reply, stopped, fim);
        self.complete_group(if limited && !stopped {
            Err(anyhow!("{limit_notice}"))
        } else if stopped || failed || stop_reason != "end_turn" {
            Err(anyhow!("turn did not complete ({stop_reason})"))
        } else {
            Ok(final_text.clone())
        });
        self.turn = None;
        if self.routine_completion.is_none() {
            self.finish_dispatch();
        }
        // The message reached the agent: the profile update it carried is now known to it.
        if let (Ok(_), Some((snapshot, announced))) = (&done, self.announce.take())
            && let Err(error) = context::mark_announced(&self.hub.store, self.cfg.session_owner(), &snapshot, announced)
        {
            tracing::warn!(bot = %self.cfg.id, error = format!("{error:#}"), "couldn't record the profile update");
        }
        if self.cfg.context.is_none()
            && self.cfg.space_id == crate::fork::spaces::PERSONAL
            && let (Some(user), Some(agent), "end_turn") = (self.turn_text.take(), &final_text, stop_reason.as_str())
        {
            let turn = crate::chat::naming::Turn { user, agent: agent.clone(), at: now_ms() };
            if let Err(mpsc::error::SendError(memory::DreamEvent::Turn(turn))) =
                self.dreamer.send(memory::DreamEvent::Turn(turn))
                && let Err(error) = crate::chat::naming::record_pending(&self.hub.store, &self.cfg.id, turn)
            {
                tracing::warn!(bot = %self.cfg.id, %error, "turno de nomeação não persistido");
            }
        }
        {
            let _boundary = self.hub.store.memory_boundary.locked();
            if let Some(sid) = self.turn_session.as_deref()
                && !self.hub.runtime(&self.id()).memory_exposure_error
                && let Err(error) = self.hub.store.memory_turn_finish(&self.id(), sid)
            {
                tracing::error!(%error, "não foi possível concluir a proteção de memória");
            }
            // Fechar o runtime na mesma cerca impede uma exposição tardia de usar a sessão já liberada.
            self.hub.set_runtime(&self.id(), |r| {
                r.memory_turn_token = None;
                r.preparing_turn_token = None;
                r.status = if failed { BotStatus::Error } else { BotStatus::Idle };
                r.resume_at = None;
                r.clear_activity();
                r.started_at = None;
                r.lane = None;
                r.message = None;
                r.session = None;
            });
        }
        // Sessões PR5: só os pedidos da lane deste turno; os de outras sessões do bot continuam.
        self.hub.team.cancel_turn_lane(&self.cfg.id, &self.lane);
        // A room turn's news comes from the group once the room is done (see `group`).
        let more = self.queue.iter().any(|q| matches!(q, Queued::User { .. } | Queued::RestoreUser { .. }));
        // It held back its only answer, a reaction: it said nothing, so there is nothing to tell either.
        let held_back = final_text.is_none() && matches!(reacted, Reacted::Refused);
        if !self.stop_requested
            && !delegated
            && !grouped
            && let Some(kind) = turn_alert(failed || message_failed, &stop_reason, more || held_back)
        {
            let corpo = match final_text.as_deref() {
                Some(text) => Corpo::Texto(text),
                None => match (kind, &reacted) {
                    (AlertKind::Done, Reacted::With(emoji)) => Corpo::Reagiu(emoji.as_str()),
                    (AlertKind::Done, _) => Corpo::Terminou,
                    _ => Corpo::NaoTerminou,
                },
            };
            push::notify(&self.hub, &self.cfg, None, &Titulo::Nome(&self.cfg.name), &corpo, kind);
        }
    }
}

impl Actor {
    /// `turn_finished`, while the turn's start time is still in the runtime.
    pub(super) fn report_turn(&self, stopped: bool, failure: Option<&anyhow::Error>, stop_reason: &str) {
        let outcome = if stopped {
            TurnOutcome::Stopped
        } else if failure.is_some() {
            TurnOutcome::Failed
        } else if stop_reason == "end_turn" {
            TurnOutcome::Done
        } else {
            TurnOutcome::Incomplete
        };
        let source = if self.active_routine.is_some() {
            TurnSource::Routine
        } else if self.active_group.is_some() {
            TurnSource::Group
        } else if self.active_request.is_some() {
            TurnSource::Bot
        } else {
            TurnSource::User
        };
        let started = self.hub.runtime(&self.id()).started_at.unwrap_or_else(now_ms);
        let ms = now_ms() - started;
        analytics::turn_finished(&self.hub, &self.cfg.backend, outcome, stop_reason, failure, source, ms);
    }
}

/// The turn producers that are never the owner's own words: another bot's request, a group or channel
/// turn, a routine run.
#[derive(Clone, Copy, Debug, Default)]
struct NonOwner {
    request: bool,
    group: bool,
    routine: bool,
}

/// Whether the turn's words are the owner's own — the only text that may orient the opening's cut and feed
/// memory evidence: never a routine's, a group's or channel's, another bot's request, or a hidden turn's.
fn owner_words(hidden: bool, non_owner: NonOwner) -> bool {
    !hidden && !non_owner.request && !non_owner.group && !non_owner.routine
}

/// A finished turn alerts once the user's work is done: not when it's `quiet` (their next message
/// is already queued, or it said nothing on purpose). A turn that ended short of `end_turn` always
/// reports the failure.
fn turn_alert(failed: bool, stop_reason: &str, quiet: bool) -> Option<AlertKind> {
    if failed || !matches!(stop_reason, "end_turn" | "cancelled") {
        return Some(AlertKind::Failed);
    }
    (stop_reason == "end_turn" && !quiet).then_some(AlertKind::Done)
}

/// A group, routine or bot-request turn opens with a header line of its own (`[Channel: …]`, `Scheduled routine: …`,
/// the request's own sentence). Any other turn is all body.
fn split_head(header_turn: bool, text: &str) -> (Option<&str>, &str) {
    match (header_turn, text.split_once('\n')) {
        (true, Some((head, body))) => (Some(head), body.trim_start_matches('\n')),
        (true, None) => (Some(text), ""),
        (false, _) => (None, text),
    }
}

/// The header line stays first, since it says what kind of turn this is; the turn context follows it.
fn turn_layout(head: Option<&str>, context: &str, rest: &str) -> String {
    [head.unwrap_or_default(), context, rest]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cabecalho_do_turno_vem_antes_do_contexto() {
        let context = "<system-reminder>Data e hora atuais.</system-reminder>";
        let (head, body) = split_head(true, "[Channel: \"Casa\" - with Ana]\n\nYou are Bia.");
        assert_eq!(
            turn_layout(head, context, body),
            format!("[Channel: \"Casa\" - with Ana]\n\n{context}\n\nYou are Bia.")
        );
        let (head, body) = split_head(true, "Scheduled routine: Resumo");
        assert_eq!(turn_layout(head, context, body), format!("Scheduled routine: Resumo\n\n{context}"));
        let (head, body) = split_head(false, "oi\nlinha");
        assert_eq!(turn_layout(head, context, body), format!("{context}\n\noi\nlinha"));
    }

    #[test]
    fn only_the_owners_own_words_count_as_the_request() {
        assert!(owner_words(false, NonOwner::default()), "a plain owner turn");
        assert!(!owner_words(true, NonOwner::default()), "a hidden turn");
        for non_owner in [
            NonOwner { request: true, ..NonOwner::default() },
            NonOwner { group: true, ..NonOwner::default() },
            NonOwner { routine: true, ..NonOwner::default() },
        ] {
            assert!(!owner_words(false, non_owner), "{non_owner:?}");
        }
    }

    #[test]
    fn only_finished_tasks_alert() {
        assert_eq!(turn_alert(false, "end_turn", false), Some(AlertKind::Done));
        assert_eq!(turn_alert(false, "end_turn", true), None, "next message queued, or a reaction held back");
        assert_eq!(turn_alert(false, "cancelled", false), None);
        assert_eq!(turn_alert(false, "max_turn_requests", false), Some(AlertKind::Failed));
        assert_eq!(turn_alert(true, "error", true), Some(AlertKind::Failed));
    }
}
