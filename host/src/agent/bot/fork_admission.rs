//! Gancho do produtor VG-03a no ator existente. Não publica controles ao app/bot.
use super::Actor;
use crate::{
    LockExt,
    fork::{
        activity_code::ActivityKind,
        bot_admission::{self as admission, Origin, Phase},
    },
    hub::BotStatus,
    store::Lane,
};
use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

pub(super) enum Pending {
    Recovery { lane: Lane, legacy: Option<String> },
    Setup { lane: Lane, entries: Vec<String>, text: String, hidden: bool },
    Prompt { params: Value, entries: Vec<String>, session: String },
}

/// Motivo público de uma falha de preparo. Só código de lista fechada, id opaco da conta e motor: nunca a
/// mensagem do erro, caminho, token nem e-mail. O app monta a frase a partir daqui.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Reason {
    pub(super) code: &'static str,
    pub(super) account_id: Option<String>,
    pub(super) backend: Option<String>,
}

const REASON_CODES: &[&str] = &[
    "account_unavailable",
    "account_disconnected",
    "account_not_found",
    "account_incompatible",
    "cli_unavailable",
    "model_unavailable",
    "model_option_unavailable",
    "setup_in_progress",
    "space_account_selection_required",
    "engine_unavailable",
    "routing_selection_unavailable",
    "engine_choice_execution_mismatch",
    "engine_not_ready",
    "folder_unavailable",
    "quota_near_limit",
    "quota_exhausted",
    "fallback_no_proof",
    "setup_failed",
];

/// Id/nome opaco curto e sem caracteres livres; qualquer outra coisa é omitida em vez de copiada.
fn opaque(value: Option<&str>) -> Option<String> {
    value
        .filter(|v| {
            !v.is_empty()
                && v.len() <= 64
                && v.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
        .map(str::to_owned)
}

/// Classifica o erro interno do preparo por lista fechada. O código é o prefixo `^[a-z_]+` seguido de `:` ou do
/// fim; o resto da mensagem é descartado. Desconhecido ou vazio: `setup_failed`.
pub(super) fn reason_from_error(error: &anyhow::Error, account_id: Option<&str>, backend: &str) -> Reason {
    let typed = error.chain().any(|c| c.downcast_ref::<super::launch::EngineNotReady>().is_some());
    let code = if typed {
        "engine_not_ready"
    } else {
        error
            .chain()
            .find_map(|cause| {
                let text = cause.to_string();
                let end = text.find(|c: char| !(c.is_ascii_lowercase() || c == '_')).unwrap_or(text.len());
                let (code, rest) = text.split_at(end);
                (rest.is_empty() || rest.starts_with(':')).then_some(code)?;
                REASON_CODES.iter().copied().find(|known| *known == code)
            })
            .unwrap_or("setup_failed")
    };
    Reason { code, account_id: opaque(account_id), backend: opaque(Some(backend)) }
}

impl Reason {
    fn json(&self) -> Value {
        let mut value = json!({"code": self.code});
        if let Some(id) = &self.account_id {
            value["accountId"] = json!(id);
        }
        if let Some(backend) = &self.backend {
            value["backend"] = json!(backend);
        }
        value
    }
}

/// ~2 s de espera pelo lock do produtor de cadeias depois do permit (ver `send_admitted`).
const MARK_STARTED_BUSY_RETRIES: usize = 80;

impl Actor {
    pub(super) fn accepts_completion(&self, ticket: &admission::Ticket) -> bool {
        self.dispatch.as_ref() == Some(ticket)
    }

    pub(super) fn admission_blocked(&self) -> bool {
        if self.restart_admission || self.pending_admission.is_some() || self.ceiling_unrestored.is_some() {
            return true;
        }
        match admission::paused(&self.hub.store, &self.cfg.id) {
            Ok(paused) => paused,
            Err(error) => {
                tracing::error!(%error, "admissão indisponível; nenhum próximo turno será enviado");
                true
            }
        }
    }

    pub(super) fn reserve_dispatch(&mut self) -> Result<()> {
        let origin = self.turn_origin();
        let token = self
            .hub
            .runtime(&self.cfg.id)
            .preparing_turn_token
            .ok_or_else(|| anyhow!("admission_execution_missing"))?;
        let ids = self.input.as_ref().map_or(&[][..], |input| input.entries.as_slice());
        if let Some(ticket) = &self.dispatch {
            return admission::reuse_reserved(
                &self.hub.store,
                &self.cfg,
                ticket,
                &self.lane.chat,
                &token,
                &origin,
                ids,
            );
        }
        self.dispatch =
            Some(admission::reserve_for_entries(&self.hub.store, &self.cfg, &self.lane.chat, &token, &origin, ids)?);
        Ok(())
    }

    pub(super) fn cancel_pending_admission(&mut self) -> Result<bool> {
        let Some(pending) = self.pending_admission.as_ref() else { return Ok(false) };
        if let Pending::Recovery { legacy: Some(legacy), .. } = pending
            && admission::decode(legacy)?.is_none()
        {
            admission::clear_legacy_captured(&self.hub.store, &self.cfg.id, legacy)?;
        }
        self.pending_admission = None;
        Ok(true)
    }

    /// A limpeza conserva a captura em memória se SQLite falhar. Outro turno
    /// permanece bloqueado até a mesma cerca poder ser concluída.
    pub(super) fn finish_dispatch(&mut self) {
        let Some(ticket) = self.dispatch.as_ref() else { return };
        self.dispatch_finishing = Some(ticket.clone());
        self.retry_finish_dispatch();
    }

    fn retry_finish_dispatch(&mut self) {
        let Some(ticket) = self.dispatch_finishing.clone() else { return };
        match admission::finish(&self.hub.store, &self.cfg.id, &ticket) {
            Ok(()) => {
                if self.dispatch.as_ref() == Some(&ticket) {
                    self.clear_profile_skills(&ticket);
                    self.dispatch = None;
                }
                self.dispatch_finishing = None;
            }
            Err(error) => tracing::error!(%error, "não foi possível concluir a cerca de despacho"),
        }
    }

    pub(super) fn defer_setup(&mut self, lane: Lane, entries: &[String], text: &str, hidden: bool) {
        self.pending_admission = Some(Pending::Setup { lane, entries: entries.to_vec(), text: text.into(), hidden });
        self.queue_retry_at = Some(Instant::now() + Duration::from_secs(5));
        self.publish_pending_admission();
    }

    /// Falha humana anterior à permissão conserva queued à frente da fila;
    /// terminal descarta o preparo agregado antes de devolver as remanescentes.
    /// A fase canônica distingue setup de efeito permitido/possivelmente enviado.
    pub(super) fn defer_unpermitted_human(&mut self, error: &anyhow::Error) -> Result<bool> {
        if self.waiting.is_some() {
            return Ok(false);
        }
        let Some(input) = self.input.as_ref() else { return Ok(false) };
        if input.hidden || input.transient || input.routine.is_some() || input.entries.is_empty() {
            return Ok(false);
        }
        let Some(ticket) = self.dispatch.as_ref() else { return Ok(false) };
        let Some(record) = admission::current(&self.hub.store, &self.cfg.id)? else { return Ok(false) };
        if record.ticket() != *ticket || record.phase != Phase::Reserved {
            return Ok(false);
        }
        let entries = input.entries.clone();
        let text = input.text.clone();
        let (account_id, backend) = self.candidate_engine();
        self.setup_reason = Some(reason_from_error(error, account_id.as_deref(), &backend));
        self.defer_setup(self.lane.clone(), &entries, &text, false);
        if !self.settle_human_setup()? {
            self.reconcile_setup_entries(&entries)?;
        }
        Ok(true)
    }

    /// Primeiro liquida a coleta do preparo anterior. Mesmo após crash/falha SQL,
    /// nenhum novo preparo ocorre antes do abandono durável e da checagem do teto.
    fn settle_human_setup(&mut self) -> Result<bool> {
        let Some(ticket) = self.dispatch.clone() else { return Ok(false) };
        let record =
            admission::current(&self.hub.store, &self.cfg.id)?.ok_or_else(|| anyhow!("admission_dispatch_missing"))?;
        anyhow::ensure!(record.ticket() == ticket && record.phase == Phase::Reserved, "admission_dispatch_conflict");
        if record.origin != Origin::Human || record.entry_ids.is_empty() || record.setup_ready || self.waiting.is_some()
        {
            return Ok(false);
        }
        crate::fork::memory_auto::queue::abandon(&self.hub, &self.cfg.id)?;
        // Motor retirado da oferta: a recusa é definitiva, então o dono já recebe a mensagem do
        // motor, sem esperar três tentativas que não mudam nada nem copiar outro texto de erro
        // interno (que não vai ao histórico por poder carregar segredo).
        let (_, backend) = self.candidate_engine();
        let retirado = crate::fork::motores_retirados::retirado(&backend);
        if record.setup_attempts < 3 && !retirado {
            return Ok(false);
        }
        let message = if retirado {
            crate::fork::motores_retirados::msg_lancar(&backend)
        } else {
            "Não foi possível preparar o pedido após três tentativas. Confira o erro e reenvie o pedido.".to_owned()
        };
        // Sem erro guardado (restart do Hub), o motivo é `setup_failed`.
        let reason = self.setup_reason.clone().unwrap_or_else(|| {
            let (account_id, backend) = self.candidate_engine();
            reason_from_error(&anyhow!("setup_failed"), account_id.as_deref(), &backend)
        });
        let reason = reason.json();
        // A cerca permanece até todas as gravações terminarem. Repetição após
        // falha parcial/restart só conclui a publicação; nunca abre outro preparo.
        for id in &record.entry_ids {
            let entry = crate::store::Store::entry_on(&self.hub.store.db.locked(), id)?;
            if let Some(mut entry) = entry
                && entry.bot_id == record.conversation_id
                && entry.kind == crate::store::EntryKind::User.as_str()
                && entry.data["status"] == "queued"
            {
                entry.data["status"] = json!("failed");
                entry.data["error"] = json!(message);
                entry.data["reason"] = reason.clone();
                self.hub.set_entry(id, &entry.data).ok_or_else(|| anyhow!("admission_setup_failure_not_saved"))?;
            }
        }
        self.hub.add_entry_once(
            &format!("setup-failed-{}", record.attempt),
            &self.lane,
            crate::store::EntryKind::Notice,
            self.turn.unwrap_or(0),
            &json!({"text":message,"style":"error","author":self.cfg.id,"reason":reason}),
        )?;
        self.setup_reason = None;
        let queued = self.hub.store.queued_user_entries_for_identity(&self.cfg.id)?;
        self.clear_setup_entries(&record.entry_ids, &queued)?;
        self.hub.set_memory_runtime(&self.cfg.id, |r| {
            r.status = BotStatus::Error;
            r.set_activity(message, Some(ActivityKind::SetupFailed));
        });
        Ok(true)
    }

    /// Conta e motor da configuração CANDIDATA desta lane: a resolução do Space mais a seleção
    /// efetiva. `self.cfg` só troca depois do preparo validar — citá-la aqui apontaria a conta e o
    /// motor da lane ANTERIOR (ex.: Space sem conta escolhida acusaria a conta do Pessoal). Se a
    /// leitura falhar, omite a conta em vez de citar a errada.
    fn candidate_engine(&self) -> (Option<String>, String) {
        let probe = self
            .hub
            .store
            .bot(&self.cfg.id)
            .map(|bot| bot.filter(|b| !b.deleted).map(|b| b.config))
            .and_then(|bot| bot.ok_or_else(|| anyhow!("bot_not_found")))
            .and_then(|original| crate::fork::spaces::execution_config(&self.hub.store, &original, &self.lane.chat))
            .and_then(|cfg| captured_engine(&self.hub, &cfg, &self.lane));
        match probe {
            Ok(cfg) => (cfg.fork_editor.engine_account_id, cfg.backend),
            Err(error) => {
                tracing::warn!(%error, "conta/motor da lane ilegível; motivo de setup sem conta");
                (None, self.input.as_ref().map_or_else(|| self.cfg.backend.clone(), |input| input.backend.clone()))
            }
        }
    }

    /// A validação de Space pode ter tornado só parte do lote terminal. Não
    /// reutilizar seu texto agregado nem manter terminal como bloqueio da fila.
    fn reconcile_setup_entries(&mut self, ids: &[String]) -> Result<bool> {
        if self.waiting.is_some()
            || self.input.as_ref().is_none_or(|input| {
                input.hidden || input.transient || input.routine.is_some() || input.entries.is_empty()
            })
        {
            return Ok(true);
        }
        let entries = self.hub.store.queued_user_entries_for_identity(&self.cfg.id)?;
        let expected = self.lane.session.as_deref().unwrap_or(&self.lane.chat);
        if ids.iter().all(|id| {
            entries
                .iter()
                .any(|entry| &entry.id == id && entry.bot_id == self.lane.chat && entry.session_id == expected)
        }) {
            return Ok(true);
        }
        self.clear_setup_entries(ids, &entries)?;
        Ok(false)
    }

    fn clear_setup_entries(&mut self, ids: &[String], entries: &[crate::store::Entry]) -> Result<()> {
        // Recusa Permitted e captura sucessora. Só libera após as gravações.
        self.release_setup_reservation()?;
        // Sessões PR5: a captura do preparo terminal cai (o cartão segue durável); só passagem legada volta ao buffer.
        let pr5 = self.session_card.take().map(|card| crate::chat::handoff::as_context(&card.card.text));
        if self.handoff.is_none()
            && let Some(input) = &self.input
            && input.handoff.is_some()
            && input.handoff != pr5
        {
            self.handoff.clone_from(&input.handoff);
            self.handoff_chat = Some(input.lane());
        }
        self.pending_admission = None;
        self.queue_retry_at = None;
        self.turn = None;
        self.input = None;
        self.turn_text = None;
        self.turn_entries.clear();
        self.turn_memory_revision.clear();
        self.turn_session = None;
        self.turn_binding = None;
        self.announce = None;
        // Sessões PR5: só os pedidos da lane do preparo que falhou.
        self.hub.team.cancel_turn_lane(&self.cfg.id, &self.lane);
        self.hub.set_memory_runtime(&self.cfg.id, |r| {
            r.status = BotStatus::Idle;
            r.preparing_turn_token = None;
            r.clear_activity();
            r.started_at = None;
            r.resume_at = None;
            r.lane = None;
            r.message = None;
            r.session = None;
        });
        self.requeue_humans_from_snapshot(ids, entries, None);
        self.retry_queue_restore();
        Ok(())
    }

    fn release_setup_reservation(&mut self) -> Result<()> {
        if let Some(ticket) = self.dispatch.as_ref() {
            admission::release_reserved(&self.hub.store, &self.cfg.id, ticket)?;
            let ticket = ticket.clone();
            self.clear_profile_skills(&ticket);
            self.dispatch = None;
        }
        Ok(())
    }

    /// Compor ainda não envia. Erro conserva os insumos do primeiro prompt.
    pub(super) async fn compose_reserved(&mut self, sid: &str, text: &str) -> Result<String> {
        let handoff = self.handoff.clone();
        let fresh = self.session_fresh;
        let resend = self.profile_resend.contains(sid);
        // Sessões PR5: se o cartão capturado mudar antes do permit, a recomposição volta a este estado do perfil.
        self.note_card_composition(fresh, resend);
        let result = self.compose(sid, text).await;
        if result.is_err() {
            self.handoff = handoff;
            self.session_fresh = fresh;
            if resend {
                self.profile_resend.insert(sid.into());
            }
        }
        result
    }

    fn publish_pending_admission(&self) {
        self.hub.set_memory_runtime(&self.cfg.id, |r| {
            r.status = BotStatus::Waiting;
            r.set_activity("Aguardando admissão do próximo turno.", Some(ActivityKind::WaitingAdmission));
            r.started_at = None;
            r.message = None;
        });
    }

    /// O ACP só recebe o prompt depois de validar contexto original e permissão
    /// durável. Nenhum erro de gravação vira falha/cancelamento do pedido humano.
    pub(super) async fn send_admitted(
        &mut self,
        mut params: Value,
        entries: Vec<String>,
        session: String,
        done_tx: &mpsc::UnboundedSender<super::PromptDone>,
    ) -> Result<()> {
        let hub = self.hub.clone();
        let account_guard =
            if self.cfg.context.is_some() { Some(crate::fork::engine_accounts::MUTATION.lock().await) } else { None };
        if let Some(ticket) = &self.dispatch
            && !admission::prepared_memory_matches(&hub.store, &self.cfg, ticket)?
        {
            for id in &entries {
                if let Some(mut entry) = hub.store.entry(id)
                    && entry.bot_id == self.lane.chat
                    && entry.kind == crate::store::EntryKind::User.as_str()
                    && entry.data["status"] == "queued"
                {
                    entry.data["status"] = json!("failed");
                    entry.data["error"] = json!("A política de memória mudou. Confira antes de reenviar.");
                    hub.set_entry(id, &entry.data).ok_or_else(|| anyhow!("memory_policy_failure_not_saved"))?;
                }
            }
            return Err(anyhow!("admission_memory_policy_changed"));
        }
        crate::fork::spaces::validate_entries(&hub, &self.cfg, &entries)?;
        // F6-B, regra (k): sob uma troca de conta, a prova é refeita imediatamente antes do envio.
        self.fallback_reprovar().await?;
        if self.active_group.is_some() {
            // Grupo transitório revogado não fica preso em retry de persistência.
            crate::fork::spaces::validate_dispatch_on(&hub.store.db.locked(), &self.cfg, &self.lane.chat, &entries)?;
        }
        if self.active_request.as_ref().is_some_and(crate::chat::team::BotRequest::reply_closed)
            || self.active_group.as_ref().is_some_and(|g| g.reply.is_closed() || !hub.groups.current(&g.lane, g.epoch))
        {
            self.stop_requested = true;
            self.finish_turn(&Ok(json!({"stopReason":"cancelled"})));
            return Ok(());
        }
        // R1/MX-145: pedido já decidido segue como o canal fechado acima; `running`
        // só é gravado DENTRO da Tx do permit, depois que o teto deixou passar.
        if let Some(request) = self.active_request.as_ref()
            && !request.still_open(&hub.store)?
        {
            self.stop_requested = true;
            self.finish_turn(&Ok(json!({"stopReason":"cancelled"})));
            return Ok(());
        }
        if let Some(id) = &self.active_routine {
            anyhow::ensure!(
                self.routine_attempt
                    .as_ref()
                    .is_some_and(|receipt| receipt.run_id == *id && receipt.bot_id == self.cfg.id),
                "routine_attempt_missing"
            );
        }
        let ticket = self.dispatch.clone().ok_or_else(|| anyhow!("admission_dispatch_missing"))?;
        let checkpoint = self.waiting.as_ref().map(serde_json::to_string).transpose()?;
        for id in &entries {
            if let Some(entry) = hub.store.entry(id) {
                crate::chat::memory::observe_input(&hub, &self.cfg.id, &entry.data)?;
            }
        }
        #[cfg(test)]
        if let Some(before_permit) = self.before_permit.take() {
            before_permit(&hub);
        }
        if self.sync_usage_control().await {
            self.hold_usage_prompt(params, entries, session);
            return Ok(());
        }
        if let Err(error) = admission::mark_human_setup_ready(&hub.store, &self.cfg.id, &ticket) {
            tracing::error!(%error, "preparo não confirmado; prompt conservado sem envio");
            self.pending_admission = Some(Pending::Prompt { params, entries, session });
            self.queue_retry_at = Some(Instant::now() + Duration::from_secs(5));
            self.publish_pending_admission();
            return Ok(());
        }
        let binding = self.turn_binding.clone();
        // PR4: uma extra nunca despacha fora de turno humano.
        let human = !entries.is_empty()
            && self.active_routine.is_none()
            && self.active_request.is_none()
            && self.active_group.is_none()
            && self.input.as_ref().is_some_and(|input| !input.hidden && !input.transient);
        anyhow::ensure!(
            human || self.lane.session.is_none() || self.context_compaction.is_some(),
            "session_admission_unavailable"
        );
        self.context_compaction_guard()?;
        let (cfg, lane) = (self.cfg.clone(), self.lane.clone());
        let turn = crate::fork::sessoes::admissao::Turn {
            cfg: &cfg,
            lane: &lane,
            entries: &entries,
            token: &ticket.execution_token,
            account_revision: self.account_revision,
        };
        // Sessões PR5: o cartão capturado que este prompt leva é consumido na MESMA Tx do queued → sent (CAS da rev).
        let card = self.session_card_for_permit();
        let mut consumed = None;
        let mut dispatch = |c: &rusqlite::Connection, original: &crate::fork::sessoes::bindings::BindingRef| {
            let class = crate::fork::sessoes::admissao::dispatch(c, &turn, original)?;
            if let Some((lane, card)) = &card {
                consumed = Some(crate::chat::handoff::consume_on(c, lane, card)?);
            }
            Ok(class)
        };
        let mut resume =
            |c: &rusqlite::Connection, checkpoint: &str| crate::fork::sessoes::admissao::resume(c, &turn, checkpoint);
        let dispatch: crate::fork::bot_admission::DispatchHook<'_> = &mut dispatch;
        let resume: crate::fork::bot_admission::ResumeHook<'_> = &mut resume;
        // MX-145 E3: o teto trava DENTRO da Tx do permit — decisão e autorização
        // na mesma leitura; o corpo da espera leva os insumos para retomar depois.
        // O pedido entre bots só vira `running` (cerca+1) na MESMA Tx em que o teto
        // passou: parado pelo teto ele continua `accepted` — o restart o fecha como
        // `restart_before_start`, nunca como possivelmente enviado.
        let hold_body = self.ceiling_hold_body(&ticket, &entries);
        let ceiling_bot = self.cfg.id.clone();
        let request_record = self.active_request.as_ref().and_then(crate::chat::team::BotRequest::unrecorded_id);
        let mut request_fence = None;
        let mut request_decided = false;
        let mut ceiling = None;
        let mut ceiling_gate = |c: &rusqlite::Connection, record: &crate::fork::bot_admission::Dispatch| {
            let decision = crate::fork::teto::decide_on(c, &ceiling_bot, &record.origin, &hold_body)?;
            ceiling = Some(decision);
            if decision.verdict == crate::fork::teto::Verdict::Pass
                && let Some(id) = &request_record
            {
                let Some(fence) = crate::fork::retornos::despachar_na(c, id)? else {
                    request_decided = true;
                    return Err(anyhow!("retorno_decidido"));
                };
                request_fence = Some(fence);
            }
            Ok(decision.verdict)
        };
        let permitted = match hub.permit_bot_dispatch(
            &self.cfg,
            &ticket,
            checkpoint.as_deref(),
            &entries,
            self.lane.session.as_deref(),
            // Encaixe PR4: BindingRef original do setup; recibo/CAS no Dispatch, checkpoint original no Resume.
            // O permit decide pela origem capturada; turno humano sem consumidor falha fechado lá.
            crate::fork::bot_admission::ClaimHooks {
                binding: binding.as_ref(),
                dispatch: Some(dispatch),
                resume: Some(resume),
                gate: Some(&mut ceiling_gate),
            },
        ) {
            Ok(permitted) => permitted,
            // O cartão mudou depois da composição: a Tx foi desfeita; recompõe no mesmo preparo, sem prompt velho.
            Err(error) if error.chain().any(<dyn std::error::Error>::is::<crate::chat::handoff::CardChanged>) => {
                self.recompose_after_card_change(&session, &entries);
                return Ok(());
            }
            // O pedido foi decidido (cancelado/interrompido) entre a leitura e a Tx: não envia.
            Err(_) if request_decided => {
                self.stop_requested = true;
                self.finish_turn(&Ok(json!({"stopReason":"cancelled"})));
                return Ok(());
            }
            Err(error) => {
                // A Tx foi desfeita: uma decisão de espera tomada nela não vale.
                ceiling = None;
                tracing::error!(%error, "permissão não persistida; prompt permanece reservado");
                self.queue_retry_at = Some(Instant::now() + Duration::from_secs(5));
                false
            }
        };
        if permitted && let Some(entry) = consumed {
            self.session_card_consumed(&entry);
        }
        if permitted && let (Some(fence), Some(request)) = (request_fence, self.active_request.as_mut()) {
            request.dispatch_recorded(fence);
        }
        if let Some(decision) = ceiling
            && (decision.changed && permitted)
        {
            // Aviso novo (ou espera de fundo apagada) na passagem: o app relê.
            crate::fork::teto::emit(&hub);
        }
        if !permitted {
            // A Tx do permit gravou a espera do teto e apagou a reserva quando a
            // conferência travou: o trabalho sai da admissão, não de `pending`.
            if let Some(decision) = ceiling
                && decision.verdict == crate::fork::teto::Verdict::Hold
            {
                self.park_ceiling_hold(&hold_body, &ticket, &entries, decision.changed);
                return Ok(());
            }
            if self.sync_usage_control().await {
                self.hold_usage_prompt(params, entries, session);
                return Ok(());
            }
            self.pending_admission = Some(Pending::Prompt { params, entries, session });
            // O tempo de execução da rotina ainda não começou. Prazos de asks e
            // epochs dos grupos continuam os mesmos e são conferidos no retry.
            self.routine_deadline = None;
            self.publish_pending_admission();
            return Ok(());
        }
        // O recibo original atravessou o setup/Pending no ator. O produtor de
        // cadeias usa a mesma Store, em outro commit: não prometer uma Tx comum.
        if let Some(id) = &self.active_routine {
            let receipt = self.routine_attempt.as_ref().ok_or_else(|| anyhow!("routine_attempt_missing"))?;
            // `StoreBusy` vem do try_lock do produtor antes de qualquer gravação: repetir só
            // essa confirmação local, com prazo curto, não reenvia prompt nem cria recibo.
            // Qualquer outro erro, ou o prazo esgotado, segue para Interrupted sem replay.
            let mut started = hub.routines.mark_started(&hub.store, &self.cfg.id, id, receipt);
            for _ in 0..MARK_STARTED_BUSY_RETRIES {
                let busy = started.as_ref().is_err_and(|error| {
                    error.chain().any(|cause| {
                        cause.downcast_ref::<crate::fork::atividades::modelo::CoreError>()
                            == Some(&crate::fork::atividades::modelo::CoreError::StoreBusy)
                    })
                });
                if !busy {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
                started = hub.routines.mark_started(&hub.store, &self.cfg.id, id, receipt);
            }
            if let Err(error) = started {
                // Permitted já é durável: falha posterior não pode reabrir setup
                // nem fabricar outro recibo. Conservar cerca até Interrupted persistir.
                self.finish_routine(crate::fork::rotina_resultado::Observed::interrupted(),
                    Some(format!("A permissão foi confirmada, mas o início da rotina não pôde ser confirmado. Confira o histórico; nenhuma ação foi repetida. {error:#}")), None);
                self.turn = None;
                self.input = None;
                hub.team.cancel_from(&self.cfg.id);
                if self.routine_completion.is_none() {
                    self.finish_dispatch();
                }
                hub.set_memory_runtime(&self.cfg.id, |runtime| {
                    runtime.preparing_turn_token = None;
                    runtime.status = BotStatus::Error;
                    runtime.set_activity(
                        "Rotina interrompida após a permissão; confira o histórico.",
                        Some(ActivityKind::RoutineInterrupted),
                    );
                    runtime.started_at = None;
                    runtime.lane = None;
                    runtime.message = None;
                    runtime.session = None;
                });
                return Ok(());
            }
            self.routine_deadline = Some(Instant::now() + hub.routines.timeout(id));
        }
        // A permissão pertence ao mesmo turno do Hub, inclusive depois da espera.
        hub.activate_bot_turn(&self.cfg.id, &ticket.execution_token)?;
        if let Some(prepared) = &mut self.profile_skills {
            prepared.activate(&hub)?;
            params["_meta"]["mexericaSkillsReadCandidate"] = prepared.descriptor();
        }
        if let Some(request) = self.active_request.as_mut() {
            request.mark_started(&self.hub);
        }
        let acp = self.conn.as_ref().ok_or_else(|| anyhow!("agent not running"))?.acp.clone();
        // Depois do commit a pausa não revoga esta tentativa. Não há outro await
        // antes de entrar no envio, nem lock SQLite atravessando o await.
        self.context_compaction_guard()?;
        self.cost_dispatch(&session)?;
        let answer = acp.send("session/prompt", params).await?;
        self.mudo_armar();
        drop(account_guard);
        if let Err(error) = self.checkpoint_waiting_session() {
            tracing::warn!(%error, "consumo do handoff será salvo novamente antes de outra tentativa");
        }
        if !self.turn_memory_revision.is_empty()
            && let Err(error) = crate::chat::memory::mark_announced(
                &hub.store,
                self.cfg.session_owner(),
                &session,
                &self.turn_memory_revision,
            )
        {
            tracing::warn!(%error, "não foi possível confirmar o aviso de memória");
        }
        let done_tx = done_tx.clone();
        tokio::spawn(async move {
            let _ = done_tx.send(super::PromptDone { ticket, result: answer.response().await });
        });
        Ok(())
    }

    pub(super) async fn retry_admission(&mut self, done_tx: &mpsc::UnboundedSender<super::PromptDone>) {
        if self.pending_admission.is_some()
            && (self.active_request.as_ref().is_some_and(crate::chat::team::BotRequest::reply_closed)
                || self
                    .active_group
                    .as_ref()
                    .is_some_and(|g| g.reply.is_closed() || !self.hub.groups.current(&g.lane, g.epoch)))
        {
            self.pending_admission = None;
            self.stop_requested = true;
            self.finish_turn(&Ok(json!({"stopReason":"cancelled"})));
            return;
        }
        if self.queue_retry_at.is_some_and(|at| Instant::now() < at) {
            return;
        }
        if self.restart_admission {
            if let Err(error) = self.restore_dispatch() {
                tracing::error!(%error, "recuperação da admissão bloqueada; evidência preservada");
                return;
            }
            if self.restart_admission {
                return;
            }
        }
        // MX-145: esperas do teto ainda não lidas — nada é admitido até a releitura valer.
        if self.retry_ceiling_restore(done_tx).await {
            return;
        }
        self.retry_finish_dispatch();
        if admission::paused(&self.hub.store, &self.cfg.id).unwrap_or(true) {
            return;
        }
        // Trabalho estacionado pelo teto fica fora da admissão e da fila; a
        // reavaliação é espaçada e devolve o trabalho à origem quando abrir.
        if self.pending_admission.is_none() && !self.ceiling_holds.is_empty() {
            if self.ceiling_next_check.is_some_and(|at| Instant::now() < at) {
                return;
            }
            self.retry_ceiling_holds(done_tx).await;
            return;
        }
        let Some(pending) = self.pending_admission.take() else { return };
        self.queue_retry_at = None;
        let result = match pending {
            Pending::Recovery { lane, legacy } => {
                let cleared = match legacy {
                    Some(raw) if admission::decode(&raw).is_ok_and(|record| record.is_none()) => {
                        admission::clear_legacy_captured(&self.hub.store, &self.cfg.id, &raw)
                    }
                    _ => Ok(()), // Reserva canônica já foi tratada por restore_dispatch.
                };
                match cleared {
                    Ok(()) => {
                        if self.active_routine.is_some() {
                            self.recover_routine().await;
                        } else {
                            self.resume_interrupted(lane);
                        }
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            }
            Pending::Setup { lane, entries, text, hidden } => {
                match self.settle_human_setup() {
                    Ok(true) => return,
                    Ok(false) => {}
                    Err(error) => {
                        tracing::error!(%error, "liquidação do preparo bloqueada; captura preservada");
                        self.defer_setup(lane, &entries, &text, hidden);
                        return;
                    }
                }
                match self.reconcile_setup_entries(&entries) {
                    Ok(false) => return,
                    Err(error) => {
                        tracing::warn!(%error, "elegibilidade do setup será conferida novamente");
                        self.defer_setup(lane, &entries, &text, hidden);
                        return;
                    }
                    Ok(true) => {}
                }
                let token = self.hub.runtime(&self.cfg.id).preparing_turn_token;
                // Preservar o fluxo anterior das origens fora de P1 e de D72.
                if (hidden
                    || self.waiting.is_some()
                    || self.active_routine.is_some()
                    || self.active_request.is_some()
                    || self.active_group.is_some())
                    && let Err(error) = self.release_setup_reservation()
                {
                    tracing::error!(%error, "retry de setup bloqueado; captura preservada");
                    self.defer_setup(lane, &entries, &text, hidden);
                    return;
                }
                if let Err(error) = self.start_prepared_turn(lane, &entries, &text, hidden, done_tx, token).await {
                    self.start_failed(&error);
                    self.requeue_unreserved_humans(&entries);
                }
                Ok(())
            }
            Pending::Prompt { params, entries, session } => {
                match self.reconcile_setup_entries(&entries) {
                    Ok(false) => return,
                    Err(error) => {
                        tracing::warn!(%error, "elegibilidade do prompt será conferida novamente");
                        self.pending_admission = Some(Pending::Prompt { params, entries, session });
                        self.queue_retry_at = Some(Instant::now() + Duration::from_secs(5));
                        return;
                    }
                    Ok(true) => {}
                }
                self.send_admitted(params, entries, session, done_tx).await
            }
        };
        if let Err(error) = result {
            self.start_failed(&error);
        }
    }

    /// Chamado antes de `restore_waiting`/`resume_interrupted`. Ler pausa precede
    /// consumir evidência; resultado Completing continua publicável sem IA.
    pub(super) fn restore_dispatch(&mut self) -> Result<()> {
        let Some(record) = admission::current(&self.hub.store, &self.cfg.id)? else {
            self.restart_admission = false;
            return Ok(());
        };
        let paused = admission::paused(&self.hub.store, &self.cfg.id)?;
        self.dispatch = Some(record.ticket());
        if paused {
            return Ok(());
        }
        if record.phase == Phase::Reserved && matches!(&record.origin, Origin::Human) && !record.entry_ids.is_empty() {
            let entries = self.hub.store.queued_user_entries_for_identity(&self.cfg.id)?;
            let original = self
                .hub
                .store
                .bot(&self.cfg.id)?
                .filter(|bot| !bot.deleted)
                .ok_or_else(|| anyhow!("bot_not_found"))?
                .config;
            let cfg = crate::fork::spaces::execution_config(&self.hub.store, &original, &record.conversation_id)?;
            anyhow::ensure!(
                record.memory_policy.as_ref() == Some(&admission::MemoryPolicy::of_config(&cfg)),
                "admission_memory_policy_changed"
            );
            if record.setup_attempts == 3 && !record.setup_ready {
                let first = crate::store::Store::entry_on(&self.hub.store.db.locked(), &record.entry_ids[0])?;
                let lane = Lane {
                    chat: record.conversation_id.clone(),
                    session: first.filter(|e| e.session_id != record.conversation_id).map(|e| e.session_id),
                };
                self.lane = lane.clone();
                let captured = captured_account(&self.hub, &cfg, &lane)?;
                self.hub.prepare_captured_turn(
                    &self.cfg.id,
                    Some(&record.execution_token),
                    captured.as_deref(),
                    |_| {},
                )?;
                self.defer_setup(lane, &record.entry_ids, "", false);
                self.settle_human_setup()?;
                self.restart_admission = false;
                return Ok(());
            }
            let batch = record
                .entry_ids
                .iter()
                .map(|id| entries.iter().find(|entry| &entry.id == id))
                .collect::<Option<Vec<_>>>();
            if let Some(batch) = batch {
                let first = batch.first().ok_or_else(|| anyhow!("admission_input_missing"))?;
                anyhow::ensure!(
                    batch
                        .iter()
                        .all(|entry| entry.bot_id == record.conversation_id && entry.session_id == first.session_id),
                    "admission_session_conflict"
                );
                let lane = Lane {
                    chat: record.conversation_id.clone(),
                    session: (first.session_id != record.conversation_id).then(|| first.session_id.clone()),
                };
                // Principal e extra retomam a mesma reserva, com o token original.
                {
                    let text = batch
                        .iter()
                        .map(|entry| crate::chat::prompt::of_entry(&self.hub, &cfg, &entry.data))
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    let captured = captured_account(&self.hub, &cfg, &lane)?;
                    let token = Some(record.execution_token.as_str());
                    self.hub.prepare_captured_turn(&self.cfg.id, token, captured.as_deref(), |runtime| {
                        runtime.lane = Some(lane.clone());
                        runtime.message = None;
                        runtime.session = None;
                    })?;
                    self.lane = lane.clone();
                    self.input = Some(super::waiting::TurnInput {
                        chat: lane.chat.clone(),
                        entries: record.entry_ids.clone(),
                        text: text.clone(),
                        hidden: false,
                        handoff: None,
                        routine: None,
                        transient: false,
                        backend: cfg.backend.clone(),
                        model: cfg.model.clone(),
                        command: cfg.command.clone(),
                        session: None,
                        context_binding: cfg
                            .context
                            .as_ref()
                            .map(|_| crate::fork::spaces::execution_stamp(&self.hub.store, &cfg))
                            .transpose()?,
                        visible_session: lane.session.clone(),
                    });
                    self.defer_setup(lane, &record.entry_ids, &text, false);
                    self.restart_admission = false;
                    return Ok(());
                }
            }
        }
        if record.phase == Phase::Permitted {
            let checkpoint = super::waiting::read_waiting_checkpoint(&self.hub.store, &self.cfg.id)?;
            let waiting_owns = checkpoint.as_ref().is_some_and(|w| w["phase"] != "finished");
            if waiting_owns {
                let owns = checkpoint.as_ref().is_some_and(|w| {
                    w["executionToken"].as_str() == Some(record.execution_token.as_str())
                        && serde_json::from_value::<Option<admission::MemoryPolicy>>(w["memoryPolicy"].clone())
                            .is_ok_and(|policy| policy.is_some() && policy == record.memory_policy)
                });
                if !owns {
                    return Err(anyhow!("admission_checkpoint_execution_conflict"));
                }
                // Espera/pausa D72 já persistida comprova que não há prompt vivo.
                // Retrying/Completing ficam com restore_waiting/finish_turn.
                if checkpoint.as_ref().is_some_and(|w| w["phase"] == "waiting" || w["phase"] == "paused") {
                    admission::finish(&self.hub.store, &self.cfg.id, &record.ticket())?;
                    self.clear_profile_skills(&record.ticket());
                    self.dispatch = None;
                }
                self.restart_admission = false;
                return Ok(());
            }
            let completed_routine =
                matches!(&record.origin, Origin::Routine { id } if self.hub.routines.admission_finished(id));
            if !completed_routine {
                // O aviso vai para a sessão do turno interrompido (extra ou principal), nunca para outra.
                let first = match record.entry_ids.first() {
                    Some(id) => crate::store::Store::entry_on(&self.hub.store.db.locked(), id)?,
                    None => None,
                };
                let lane = Lane {
                    chat: record.conversation_id.clone(),
                    session: first.filter(|e| e.session_id != record.conversation_id).map(|e| e.session_id),
                };
                self.hub.add_entry_once(
                    &format!("dispatch-interrupted-{}", record.attempt), &lane,
                    crate::store::EntryKind::Notice, 0,
                    &json!({"text":"O Hub reiniciou após permitir este turno. Pode haver efeito já realizado; confira o histórico antes de reenviar. Nenhuma ação foi repetida automaticamente.","style":"info","author":self.cfg.id}),
                )?;
            }
        }
        admission::finish(&self.hub.store, &self.cfg.id, &record.ticket())?;
        self.clear_profile_skills(&record.ticket());
        self.dispatch = None;
        self.restart_admission = false;
        Ok(())
    }
}

/// D372: a configuração da execução recuperada de um turno humano, pela seleção EFETIVA da lane (sem promover a
/// próxima): conta E motor, para a guarda de setup do preparo; canal/Space seguem a configuração recebida.
fn captured_engine(
    hub: &crate::hub::Hub,
    cfg: &crate::store::BotConfig,
    lane: &Lane,
) -> Result<crate::store::BotConfig> {
    let mut probe = cfg.clone();
    crate::fork::sessoes::admissao::apply_choice(&hub.store.db.locked(), lane, &mut probe)?;
    Ok(probe)
}

fn captured_account(hub: &crate::hub::Hub, cfg: &crate::store::BotConfig, lane: &Lane) -> Result<Option<String>> {
    Ok(captured_engine(hub, cfg, lane)?.fork_editor.engine_account_id)
}

#[cfg(test)]
mod reason_tests {
    use super::{REASON_CODES, reason_from_error};
    use crate::agent::bot::launch::EngineNotReady;
    use anyhow::anyhow;

    #[test]
    fn motivo_so_sai_da_lista_fechada() {
        for code in REASON_CODES {
            for text in [format!("{code}: texto interno"), (*code).to_string()] {
                let reason = reason_from_error(&anyhow!(text), Some("acc-1"), "codex");
                assert_eq!(reason.code, *code);
                assert_eq!(reason.account_id.as_deref(), Some("acc-1"));
                assert_eq!(reason.backend.as_deref(), Some("codex"));
            }
        }
        // Contexto por cima do código: a cadeia é percorrida.
        let chained = anyhow!("account_unavailable: reconecte a conta selecionada.").context("preparo falhou");
        assert_eq!(reason_from_error(&chained, None, "claude").code, "account_unavailable");
        // Caminho legado tipado, qualquer que seja o texto original.
        let legacy = anyhow::Error::new(EngineNotReady("codex login --token sk-SEGREDO /tmp/motor".into()));
        assert_eq!(reason_from_error(&legacy, None, "codex").code, "engine_not_ready");
    }

    #[test]
    fn motivo_desconhecido_ou_vazio_e_setup_failed_sem_vazar() {
        for text in [
            "",
            "codigo_inventado: x",
            "Account_unavailable: maiúscula",
            "account_unavailable com espaço",
            "falha em /tmp/motor/.cli/auth.json para usuario@exemplo.invalid token=sk-abc",
            "sk-abc123: usuario@exemplo.invalid",
        ] {
            let reason = reason_from_error(&anyhow!(text.to_string()), None, "claude");
            assert_eq!(reason.code, "setup_failed", "{text}");
            let shown = format!("{:?}{}", reason, reason.json());
            for secret in ["sk-abc", "usuario@", "/tmp/motor", "auth.json", "token="] {
                assert!(!shown.contains(secret), "{secret} vazou em {shown}");
            }
        }
        // Id/motor fora do formato opaco são omitidos, nunca copiados.
        let reason = reason_from_error(&anyhow!("account_not_found"), Some("usuario@exemplo.invalid"), "/bin/sh -c x");
        assert_eq!((reason.account_id, reason.backend), (None, None));
        let json = reason_from_error(&anyhow!("model_unavailable: x"), Some("a1"), "claude").json();
        assert_eq!(json, serde_json::json!({"code":"model_unavailable","accountId":"a1","backend":"claude"}));
    }
}
