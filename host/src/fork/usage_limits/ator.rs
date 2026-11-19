//! Controles da C4 no ator D72; este arquivo é submódulo de waiting.
use super::{Actor, Deserialize, LockExt, Phase, Result, Serialize, Value, WaitingTurn, json, now_ms, reset_label};
use crate::fork::usage_limits::{self as control, Target};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct UsageControl {
    pub(super) id: String,
    pub(super) manual: bool,
    pub(super) policy_paused: bool,
    pub(super) ready: bool,
    uncertain_at: Option<i64>,
}
impl Default for UsageControl {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            manual: false,
            policy_paused: false,
            ready: false,
            uncertain_at: None,
        }
    }
}

impl Actor {
    pub(in crate::agent::bot) fn capture_usage_uncertainty(&mut self, permitted: bool) {
        if let Some(w) = self.waiting.as_mut()
            && w.phase == Phase::Retrying
            && permitted
        {
            w.usage_control.uncertain_at = Some(now_ms());
        }
    }

    /// Um bloqueio durante os awaits conserva o prompt preparado quando a reserva continua viva.
    pub(in crate::agent::bot) fn hold_usage_prompt(&mut self, params: Value, entries: Vec<String>, session: String) {
        if self.dispatch.is_some() && self.waiting.as_ref().is_some_and(|w| w.phase == Phase::Retrying) {
            self.pending_admission =
                Some(crate::agent::bot::fork_admission::Pending::Prompt { params, entries, session });
            self.queue_retry_at = Some(std::time::Instant::now() + std::time::Duration::from_secs(5));
        }
    }

    /// Persiste também a geração criada ao ler checkpoints de versões anteriores.
    pub(in crate::agent::bot) fn restore_usage_control(&mut self) {
        if let Err(error) = self.apply_usage_policy() {
            self.pause_waiting_switch(&error);
            return;
        }
        self.save_waiting_logged();
    }

    /// Só publicação: recupera falha/crash mesmo depois do tombstone encerrar a espera ativa.
    pub(in crate::agent::bot) fn reconcile_usage_notices(&self) {
        match control::pending_notices(&self.hub.store, &self.cfg.id) {
            Ok(notices) => {
                for saved in notices {
                    if let Err(error) = self.publish_usage_cancelled(&saved) {
                        tracing::error!(%error, "aviso de desistência permanece pendente");
                    }
                }
            }
            Err(error) => tracing::error!(%error, "não foi possível ler avisos de desistência pendentes"),
        }
    }

    /// Ausência/erro da política nunca autoriza uma tentativa. Não muda pausas por incerteza/autoridade.
    pub(in crate::agent::bot) fn apply_usage_policy(&mut self) -> Result<bool> {
        let policy = control::settings(&self.hub.store)?;
        let Some(w) = self.waiting.as_mut() else { return Ok(false) };
        if !matches!(w.phase, Phase::Waiting | Phase::Paused) {
            return Ok(false);
        }
        let before = (w.phase, w.usage_control.policy_paused, w.usage_control.ready);
        let sleeping = policy.snooze && w.resume_at.is_some_and(|at| at > now_ms());
        if w.phase == Phase::Waiting && !policy.auto_resume && !w.usage_control.manual {
            w.usage_control.policy_paused = true;
            if !sleeping {
                w.phase = Phase::Paused;
            }
        }
        if w.usage_control.policy_paused {
            if policy.auto_resume || w.usage_control.manual {
                w.phase = Phase::Waiting;
                w.usage_control.policy_paused = false;
            } else {
                w.phase = if sleeping { Phase::Waiting } else { Phase::Paused };
            }
        }
        w.usage_control.ready = w.resume_at.is_some_and(|at| at <= now_ms());
        Ok(before != (w.phase, w.usage_control.policy_paused, w.usage_control.ready))
    }

    /// Atualiza o aviso somente após a desistência durável. Nunca anuncia ausência de efeitos em voo.
    pub(in crate::agent::bot) fn publish_usage_cancelled(&self, saved: &Value) -> Result<()> {
        let checkpoint = &saved["checkpoint"];
        let Some(id) = checkpoint["notice"].as_str() else { return Ok(()) };
        let Some(mut entry) = self.hub.store.entry(id) else { return Ok(()) };
        let started = saved["alreadyStarted"] == true;
        entry.data["text"] = json!(if started {
            "A retomada já tinha sido autorizada. A parada foi solicitada; confira os efeitos antes de continuar."
        } else if saved["resumed"] == true {
            "A retomada automática foi cancelada. Você pediu para retomar este pedido manualmente."
        } else {
            "Não vou retomar este pedido. O pedido e a sessão continuam guardados."
        });
        entry.data["style"] = json!("info");
        entry.data["subscriptionWait"] = json!({
            "waitId":checkpoint["usageControl"]["id"],"cancelled":true,
            "retry":!started && saved["resumed"] != true,"alreadyStarted":started,
            "resumeAt":checkpoint["resumeAt"],"autoResume":false,"snoozed":false,
            "ready":checkpoint["resumeAt"].as_i64().is_some_and(|at| at <= now_ms())
        });
        self.hub.set_entry(id, &entry.data).ok_or_else(|| anyhow::anyhow!("usage_wait_notice_not_saved"))?;
        Ok(())
    }

    /// Chamado no tick, na intenção do dono e depois do preparo. True bloqueia este envio.
    pub(in crate::agent::bot) async fn sync_usage_control(&mut self) -> bool {
        let Some(w) = self.waiting.as_ref() else { return false };
        if w.phase == Phase::Completing || w.phase == Phase::Finished || self.finishing.is_some() {
            return false;
        }
        let cancellation = control::cancellation(&self.hub.store, &w.usage_control.id);
        match cancellation {
            Err(error) => {
                tracing::error!(%error,"controle da espera ilegível; envio bloqueado");
                return true;
            }
            Ok(Some(saved)) => {
                let permitted = crate::fork::bot_admission::current(&self.hub.store, &self.cfg.id)
                    .map(|d| d.is_some_and(|d| d.phase == crate::fork::bot_admission::Phase::Permitted));
                match permitted {
                    Err(error) => tracing::error!(%error,"não foi possível conferir o despacho cancelado"),
                    Ok(true) => self.cancel_turn().await,
                    Ok(false) => {
                        // Finalização local reutiliza a barreira/tombstone D72 e preserva a fila.
                        self.pending_admission = None;
                        self.queue_retry_at = None;
                        self.stop_requested = true;
                        self.finish_turn(&Ok(json!({"stopReason":"cancelled"})));
                    }
                }
                if let Err(error) = self.publish_usage_cancelled(&saved) {
                    tracing::error!(%error,"aviso da desistência será reconciliado");
                }
                return true;
            }
            Ok(None) => {}
        }
        // Desligado durante o preparo: liquida só a reserva ainda sem efeito e conserva a espera.
        if self.waiting.as_ref().is_some_and(|w| w.phase == Phase::Retrying && !w.usage_control.manual)
            && control::settings(&self.hub.store).is_ok_and(|p| !p.auto_resume)
        {
            match crate::fork::bot_admission::current(&self.hub.store, &self.cfg.id) {
                Ok(record)
                    if record.as_ref().is_none_or(|r| r.phase == crate::fork::bot_admission::Phase::Reserved) =>
                {
                    self.pending_admission = None;
                    self.queue_retry_at = None;
                    if let Some(w) = self.waiting.as_mut() {
                        w.phase = Phase::Paused;
                        w.usage_control.policy_paused = true;
                    }
                    self.save_waiting_logged();
                    self.publish_waiting();
                }
                _ => return true,
            }
        }
        match self.apply_usage_policy() {
            Ok(changed) => {
                if changed {
                    self.save_waiting_logged();
                    self.publish_waiting();
                }
            }
            Err(error) => {
                tracing::error!(%error,"preferências de limite ilegíveis; envio bloqueado");
                return true;
            }
        }
        // A Tx do permit repete esta leitura: cobre o clique depois deste ponto.
        let checkpoint = self.waiting.as_ref().map(serde_json::to_string).transpose();
        match checkpoint {
            Ok(checkpoint) => !control::allow_on(&self.hub.store.db.locked(), checkpoint.as_deref()).unwrap_or(false),
            Err(_) => true,
        }
    }

    pub(in crate::agent::bot) fn resume_usage_wait(
        &mut self,
        target: &Target,
        origem: Option<&crate::fork::donna_fixa::pedido::Origem>,
    ) -> Result<Value> {
        anyhow::ensure!(target.bot_id == self.cfg.id, "usage_wait_destination_mismatch");
        // D368: o pedido esperou na fila do ator; a autoridade de quem pediu precisa valer agora.
        crate::fork::donna_fixa::pedido::conferir(origem)?;
        if let Some(saved) = control::cancellation(&self.hub.store, &target.wait_id)? {
            anyhow::ensure!(
                self.turn.is_none()
                    && self.waiting.is_none()
                    && self.dispatch.is_none()
                    && self.pending_admission.is_none()
                    && self.finishing.is_none(),
                "usage_wait_bot_busy"
            );
            anyhow::ensure!(
                target.matches(&saved["checkpoint"]) && saved["alreadyStarted"] != true && saved["resumed"] != true,
                "usage_wait_stale_or_uncertain"
            );
            let mut w: WaitingTurn = serde_json::from_value(saved["checkpoint"].clone())?;
            anyhow::ensure!(w.completion.is_none(), "usage_wait_already_completed");
            w.phase = Phase::Waiting;
            w.usage_control = UsageControl { manual: true, ..UsageControl::default() };
            w.notice = None; // ID novo: o clique no aviso anterior nunca controla a nova espera.
            w.retry_at = now_ms();
            w.attempt_count = 0; // Mesmo orçamento do Tentar de novo explícito da D72.
            w.switch_error = None;
            control::resume_archive(&self.hub.store, target, &serde_json::to_value(w)?, origem)?;
            self.stop_requested = false;
            self.restore_waiting();
            let mut consumed = saved;
            consumed["resumed"] = json!(true);
            self.publish_usage_cancelled(&consumed)?;
        } else {
            let w = self.waiting.as_ref().ok_or_else(|| anyhow::anyhow!("usage_wait_stale"))?;
            anyhow::ensure!(
                target.matches(&serde_json::to_value(w)?)
                    && matches!(w.phase, Phase::Waiting | Phase::Paused)
                    && !w.dirty
                    && w.completion.is_none()
                    && self.dispatch.is_none()
                    && self.finishing.is_none(),
                "usage_wait_stale_or_busy"
            );
            let before = self.waiting.clone();
            let w = self.waiting.as_mut().expect("checked");
            w.usage_control.manual = true;
            w.usage_control.policy_paused = false;
            w.phase = Phase::Waiting;
            w.retry_at = now_ms();
            w.attempt_count = 0;
            w.switch_error = None;
            // Conferência e gravação na mesma transação: revogado entre as duas, nada muda.
            let saved = serde_json::to_value(&*w)
                .map_err(anyhow::Error::from)
                .and_then(|raw| control::save_resumed(&self.hub.store, &self.cfg.id, &raw, origem));
            if let Err(error) = saved {
                self.waiting = before;
                return Err(error);
            }
            self.publish_waiting();
        }
        Ok(json!({"retryRequested":true}))
    }

    /// Texto e botões vêm do mesmo estado confirmado. Nenhum alerta é enviado por esta publicação.
    pub(in crate::agent::bot) fn usage_notice(&self, data: &mut Value) {
        let Some(w) = &self.waiting else { return };
        let policy =
            control::settings(&self.hub.store).unwrap_or(control::Settings { auto_resume: false, snooze: false });
        let sleeping = policy.snooze && w.resume_at.is_some_and(|at| at > now_ms());
        let auto = policy.auto_resume || w.usage_control.manual;
        if w.phase == Phase::Paused {
            data["text"] = json!(data["text"].as_str().unwrap_or_default().replace("Tentar de novo", "Retomar agora"));
        }
        let wait = &mut data["subscriptionWait"];
        wait["waitId"] = json!(w.usage_control.id);
        wait["cancelled"] = json!(false);
        wait["autoResume"] = json!(auto);
        wait["snoozed"] = json!(sleeping);
        wait["ready"] = json!(w.usage_control.ready);
        if sleeping && w.usage_control.policy_paused {
            wait["retry"] = json!(false);
        }
        if w.usage_control.policy_paused {
            data["style"] = json!("info");
            data["text"] = json!(if sleeping {
                format!(
                    "Em soneca até o limite voltar, às {}. A retomada automática está desligada; avisarei quando o prazo chegar.",
                    reset_label(w.resume_at.unwrap_or_default())
                )
            } else if w.usage_control.ready {
                "O prazo informado para o limite chegou. Use Retomar agora para tentar continuar.".into()
            } else {
                "Limite da assinatura atingido. A retomada automática está desligada; o pedido está guardado. Use Retomar agora para tentar continuar.".into()
            });
        } else if sleeping && w.phase == Phase::Waiting {
            data["text"] = json!(format!(
                "Em soneca até o limite voltar, às {}. Sem tentativas nem avisos até lá; depois retomo automaticamente.",
                reset_label(w.resume_at.unwrap_or_default())
            ));
        }
    }
}
