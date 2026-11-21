//! R2/P2: saveRoutine só publica candidato após revalidar o bot na Tx de escrita.
//! A checagem antecipada mantém o erro do fluxo; não autoriza o commit.
use super::{
    Context, EntryKind, Hub, Lane, LockExt, MAX_FINISHED_RUNS, Result, Routines, State, Store, Value, duplicate_body,
    duplicate_source, guard_live_runs, hook_set, inherit_external_release, json, public, public_run, save_in,
};

pub(super) struct ManualRun<'a> {
    pub(super) id: &'a str,
    pub(super) event: Value,
    pub(super) delivery: Option<String>,
    pub(super) test: bool,
}

// Um replay pode reencontrar uma rotina editada depois da chamada original.
// Nunca devolva texto armazenado por outra origem numa criação classificada interna.
fn creation_reply(routine: &super::Routine) -> Value {
    json!({"routine":{"id":routine.id,"botId":routine.bot_id,"enabled":routine.enabled,"externalReleasePending":routine.external_creation && routine.external_release_pending.is_some() && !routine.enabled,
        "nextRunAt":if routine.enabled { routine.next_runs.iter().flatten().min().copied() } else { None }}})
}

/// MX-106: o modelo pode pausar, editar e apagar, mas ligar uma rotina criada a partir de conteúdo de
/// fora é gesto do dono no app (origem humana). A transição é conferida antes do commit.
pub(super) fn ensure_no_model_release(before: &State, after: &State) -> Result<()> {
    for routine in after.routines.iter().filter(|routine| routine.external_creation && routine.enabled) {
        let old = before.routines.iter().find(|old| old.id == routine.id && old.bot_id == routine.bot_id);
        anyhow::ensure!(old.is_some_and(|old| !old.deleted && old.enabled), super::EXTERNAL_RELEASE);
    }
    Ok(())
}

/// Heartbeat: o modelo não declara, não altera (declaração, nome, horário, tempo limite, chave de webhook), não apaga nem liga uma checagem
/// silenciosa; isso é gesto do dono no app. Pausar é permitido.
pub(super) fn ensure_no_model_heartbeat(before: &State, after: &State) -> Result<()> {
    let same = |a: &super::Routine, b: &super::Routine| a.id == b.id && a.bot_id == b.bot_id;
    for old in before.routines.iter().filter(|old| old.heartbeat.is_some() && !old.deleted) {
        let kept = after.routines.iter().find(|r| same(r, old)).is_some_and(|r| {
            !r.deleted
                && r.heartbeat == old.heartbeat
                && r.name == old.name
                && r.webhook_key == old.webhook_key
                && r.triggers == old.triggers
                && r.timeout_seconds == old.timeout_seconds
                && (old.enabled || !r.enabled)
        });
        if !kept {
            tracing::warn!(routine = %old.id, bot = %old.bot_id, "o modelo tentou alterar, apagar ou ligar uma checagem silenciosa; recusado");
        }
        anyhow::ensure!(kept, crate::fork::heartbeat::SO_O_DONO);
    }
    for routine in after.routines.iter().filter(|r| !r.deleted && r.heartbeat.is_some()) {
        let declared_before = before.routines.iter().any(|old| same(old, routine) && old.heartbeat.is_some());
        if !declared_before {
            tracing::warn!(routine = %routine.id, bot = %routine.bot_id, "o modelo tentou declarar uma checagem silenciosa; recusado");
        }
        anyhow::ensure!(declared_before, crate::fork::heartbeat::SO_O_DONO);
    }
    Ok(())
}

pub(super) fn retain_history(next: &mut State) {
    let mut kept = 0;
    next.runs.reverse();
    next.runs.retain(|run| {
        if run.status.active() || run.report_pending {
            return true;
        }
        kept += 1;
        kept <= MAX_FINISHED_RUNS
    });
    next.runs.reverse();
    next.routines.retain(|r| !r.deleted || next.runs.iter().any(|run| run.routine_id == r.id && run.status.active()));
}

impl Routines {
    pub(super) fn change_bound<T>(
        &self,
        store: &Store,
        bot: Option<&str>,
        update: impl FnOnce(&mut State) -> Result<T>,
    ) -> Result<T> {
        self.change_in_origin(store, bot, None, update)
    }

    pub(super) fn change_in_origin<T>(
        &self,
        store: &Store,
        bot: Option<&str>,
        origin: Option<&super::tool_origin::ToolOrigin>,
        update: impl FnOnce(&mut State) -> Result<T>,
    ) -> Result<T> {
        let mut guard = self.state.locked();
        let mut next = guard.clone();
        let result = update(&mut next)?;
        if origin.is_some() {
            ensure_no_model_release(&guard, &next)?;
            ensure_no_model_heartbeat(&guard, &next)?;
        }
        retain_history(&mut next);
        let raw = serde_json::to_string(&next)?;
        // A época avança antes do commit: uma checagem admitida antes não segura a decisão durante a gravação, e a
        // próxima admissão só acontece com o estado já trocado (ela espera este mesmo lock).
        self.state.epoca.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        #[cfg(test)]
        crate::fork::sessoes::call_tests::before_effect(store, "routine_commit");
        if origin.is_some() {
            store.guard_routines_commit_in_origin(bot, &raw, guard_live_runs(&next), origin)?;
        } else {
            match bot {
                Some(bot) => store.guard_routines_commit_for_bot(bot, &raw, guard_live_runs(&next))?,
                None => store.guard_routines_commit(&raw, guard_live_runs(&next))?,
            }
        }
        let hooks_changed = hook_set(&guard) != hook_set(&next);
        *guard = next;
        drop(guard);
        if hooks_changed {
            self.hooks_rev.send_modify(|v| *v += 1);
        }
        Ok(result)
    }

    /// MCP: nenhuma raiz é confirmada antes da guarda original e do JSON da fila.
    pub(super) fn enqueue_bound_chain(
        &self,
        store: &Store,
        bot: &str,
        origin: &super::tool_origin::ToolOrigin,
        request: ManualRun<'_>,
    ) -> Result<Value> {
        let mut guard = self.state.locked();
        let mut next = guard.clone();
        anyhow::ensure!(!next.paused, super::hooks::Refused::Paused);
        let routine = next
            .routines
            .iter()
            .find(|r| r.id == request.id && r.bot_id == bot && !r.deleted)
            .context("routine not found")?;
        anyhow::ensure!(!routine.external_creation || routine.enabled, super::EXTERNAL_RELEASE);
        anyhow::ensure!(routine.enabled || request.test, super::hooks::Refused::Paused);
        let previous = request
            .delivery
            .as_ref()
            .and_then(|key| {
                next.runs.iter().find(|run| run.routine_id == request.id && run.delivery_id.as_ref() == Some(key))
            })
            .cloned();
        if previous.is_none() {
            anyhow::ensure!(
                !next.runs.iter().any(|run| run.routine_id == request.id && run.status.active()),
                super::hooks::Refused::Busy
            );
        }
        // IDs, ocorrência, digest e cadastro são preparados sem escrita e antes da Tx.
        let reservation = if previous.as_ref().is_none_or(|run| run.chain.is_some()) {
            Some(super::cadeias::preparar_manual(store, routine, request.event, request.delivery)?)
        } else {
            None
        };
        #[cfg(test)]
        crate::fork::sessoes::call_tests::before_effect(store, "routine_commit");
        let result = store.guard_routines_change_in_origin(Some(bot), Some(origin), |c| {
            let admitted = reservation.map(|reservation| reservation.admitir_na(c)).transpose()?;
            let result = if let Some(previous) = previous {
                if let Some(admitted) = admitted {
                    anyhow::ensure!(
                        admitted.id == previous.id,
                        "Não foi possível conferir a execução original desta ocorrência"
                    );
                }
                json!({"run":public_run(&previous)})
            } else {
                let run = admitted.context("Tentativa da rotina não encontrada")?;
                let receipt = run.chain.as_ref().context("Tentativa da rotina não encontrada")?;
                anyhow::ensure!(
                    !crate::fork::cadeias::store::vinculada(c, receipt)?,
                    crate::fork::cadeias::modelo::Recusa::ReciboInvalido
                );
                let result = json!({"run":public_run(&run)});
                next.runs.push(run);
                result
            };
            retain_history(&mut next);
            let live_runs = guard_live_runs(&next).map(|(bot, run)| (bot.to_owned(), run.to_owned())).collect();
            Ok((result, serde_json::to_string(&next)?, live_runs))
        })?;
        let hooks_changed = hook_set(&guard) != hook_set(&next);
        *guard = next;
        drop(guard);
        if hooks_changed {
            self.hooks_rev.send_modify(|v| *v += 1);
        }
        Ok(result)
    }

    /// O callback interno fica antes de state/DB; permite fixar deterministicamente
    /// a janela entre lookup e commit no teste. Produção passa callback vazio.
    pub(super) fn save_bound(&self, hub: &Hub, bot: &str, body: &Value, after_lookup: impl FnOnce()) -> Result<Value> {
        self.save_in_origin(hub, bot, body, None, after_lookup)
    }

    pub(super) fn duplicate_direct(
        &self,
        hub: &Hub,
        bot: &str,
        id: &str,
        origin: Option<&super::tool_origin::ToolOrigin>,
    ) -> Result<Value> {
        anyhow::ensure!(origin.is_none(), "duplicação direta exige origem do dono");
        let row = hub
            .store
            .bot(bot)?
            .filter(|b| !b.deleted && !b.config.is_group())
            .context("routine requires an existing agent bot")?;
        let routine = self.change_in_origin(&hub.store, Some(bot), origin, |s| {
            let source = duplicate_source(s, &row.config.id, id)?;
            let created = save_in(s, &row.config.id, &duplicate_body(&source))?;
            let stored = s
                .routines
                .iter_mut()
                .find(|routine| routine.id == created.id)
                .context("rotina duplicada não foi gravada")?;
            inherit_external_release(stored, &source);
            Ok(stored.clone())
        })?;
        let lane = origin.map_or_else(|| Lane::main(bot), super::tool_origin::ToolOrigin::lane);
        hub.add_entry_in_execution(
            &lane,
            EntryKind::Notice,
            0,
            &json!({"text":format!("Created routine: {}", routine.name), "routineId": routine.id, "style":"info"}),
            origin.map(|origin| &origin.execution),
        );
        Ok(json!({"routine":public(&routine)}))
    }

    pub(super) fn save_in_origin(
        &self,
        hub: &Hub,
        bot: &str,
        body: &Value,
        origin: Option<&super::tool_origin::ToolOrigin>,
        after_lookup: impl FnOnce(),
    ) -> Result<Value> {
        let row = hub
            .store
            .bot(bot)?
            .filter(|b| !b.deleted && !b.config.is_group())
            .context("routine requires an existing agent bot")?;
        after_lookup();
        if let Some(origin) = origin.filter(|origin| origin.is_creation()) {
            return self.save_bot_created(hub, &row.config, body, origin);
        }
        let routine = self.change_in_origin(&hub.store, Some(bot), origin, |s| save_in(s, &row.config.id, body))?;
        let updated = body["id"].is_string();
        let lane = origin.map_or_else(|| Lane::main(bot), super::tool_origin::ToolOrigin::lane);
        hub.add_entry_in_execution(&lane, EntryKind::Notice, 0, &json!({"text":format!("{} routine: {}", if updated {"Updated"} else {"Created"}, routine.name), "routineId": routine.id, "style":"info"}), origin.map(|origin| &origin.execution));
        Ok(json!({"routine":public(&routine)}))
    }
    fn save_bot_created(
        &self,
        hub: &Hub,
        cfg: &crate::store::BotConfig,
        body: &Value,
        origin: &super::tool_origin::ToolOrigin,
    ) -> Result<Value> {
        use crate::fork::routine_creation::{self as creation, Creation};
        anyhow::ensure!(body.get("id").is_none(), "invalid_field: criação não aceita id");
        // Intercalação de teste ocorre antes do lock de emissão: a troca de sessão
        // publica seu próprio evento. A origem já foi capturada e a Tx a reconfere.
        #[cfg(test)]
        crate::fork::sessoes::call_tests::before_effect(&hub.store, "routine_commit");
        // Mantém a ordem global de eventos antes de state/DB, como os outros produtores de entradas.
        let _emit = hub.emit_lock.locked();
        let mut guard = self.state.locked();
        let mut next = guard.clone();
        let replay = crate::fork::routine_creation::replay_on(
            &hub.store.db.locked(),
            origin.invocation(),
            &cfg.space_id,
            &cfg.id,
        )?;
        // Duplicação: a origem é lida aqui, depois do replay e sob o lock do estado
        // até o commit. Retry devolve a cópia já criada; sem TOCTOU com edição.
        let source = if replay.is_none() {
            match body.get("duplicateOf").and_then(Value::as_str) {
                Some(source_id) => {
                    let found = duplicate_source(&next, &cfg.id, source_id)?;
                    anyhow::ensure!(found.heartbeat.is_none(), crate::fork::heartbeat::SO_O_DONO);
                    Some(found)
                }
                None => None,
            }
        } else {
            None
        };
        let owned;
        let effective = match &source {
            Some(found) => {
                owned = duplicate_body(found);
                &owned
            }
            None => body,
        };
        let mut candidate = if replay.is_some() { None } else { Some(save_in(&mut next, &cfg.id, effective)?) };
        // P1: a cópia herda a marca de conteúdo de fora da origem antes de `create_on`,
        // que preserva (OR) em vez de recalcular só pelo contexto atual.
        if let (Some(found), Some(candidate)) = (&source, candidate.as_mut()) {
            inherit_external_release(candidate, found);
        }
        let mut entries = Vec::new();
        let mut quota_exceeded = false;
        let result = hub.store.guard_routines_change_in_origin(Some(&cfg.id), Some(origin), |c| {
            creation::bot_on(c, &cfg.space_id, &cfg.id)?;
            if let Some(id) = creation::replay_on(c, origin.invocation(), &cfg.space_id, &cfg.id)? {
                crate::fork::cadeias::descendencia_rotina::capture_creation_on(
                    c,
                    &cfg.id,
                    &cfg.space_id,
                    &origin.execution,
                    origin.human_message(),
                )?;
                next = guard.clone();
                let previous = next
                    .routines
                    .iter()
                    .find(|r| r.id == id && r.bot_id == cfg.id && !r.deleted)
                    .context("routine_creation_already_settled: a rotina desta chamada não será recriada")?;
                let result = creation_reply(previous);
                let live = guard_live_runs(&next).map(|(bot, run)| (bot.to_owned(), run.to_owned())).collect();
                return Ok((result, serde_json::to_string(&next)?, live));
            }
            let routine = candidate.as_mut().context("routine_creation_replay_changed")?;
            match creation::create_on(
                c,
                &cfg.space_id,
                routine,
                &origin.execution,
                origin.human_message(),
                origin.invocation(),
                crate::store::now_ms(),
            )? {
                Creation::Created(committed) => {
                    entries = committed;
                    let target = next
                        .routines
                        .iter_mut()
                        .find(|r| r.id == routine.id)
                        .context("routine creation candidate missing")?;
                    *target = routine.clone();
                }
                Creation::QuotaExceeded => {
                    quota_exceeded = true;
                    next = guard.clone();
                }
            }
            retain_history(&mut next);
            let live = guard_live_runs(&next).map(|(bot, run)| (bot.to_owned(), run.to_owned())).collect();
            Ok((creation_reply(routine), serde_json::to_string(&next)?, live))
        })?;
        let hooks_changed = hook_set(&guard) != hook_set(&next);
        *guard = next;
        drop(guard);
        if hooks_changed {
            self.hooks_rev.send_modify(|v| *v += 1);
        }
        creation::publish(hub, entries);
        creation::changed(hub, &cfg.space_id);
        if quota_exceeded {
            return Err(creation::quota_error());
        }
        Ok(result)
    }

    pub(crate) fn review_creation(
        &self,
        hub: &Hub,
        space: &str,
        entry_id: &str,
        entry_rev: i64,
        decision: &str,
        proof: crate::fork::routine_creation::ReviewProof<'_>,
    ) -> Result<Value> {
        use crate::fork::routine_creation as creation;
        let creation::ReviewProof { routine_revision: expected_routine_revision, external_grant } = proof;
        let _emit = hub.emit_lock.locked();
        let mut guard = self.state.locked();
        let mut next = guard.clone();
        let mut updated = Vec::new();
        let result = hub.store.guard_routines_change_in_origin(None, None, |c| {
            let (entry, card) = creation::card_on(c, space, entry_id)?;
            anyhow::ensure!(decision == "external_granted" || external_grant.is_none(), "routine_review_conflict");
            let settled = if matches!(decision, "external_granted" | "external_revoked") {
                anyhow::ensure!(entry.rev == entry_rev && card.decision != "cancelled", "routine_review_conflict");
                let routine = next
                    .routines
                    .iter()
                    .find(|r| r.id == card.routine_id && r.bot_id == card.bot_id && !r.deleted)
                    .context("routine_review_unavailable")?;
                if decision == "external_granted" {
                    creation::external_grant::record(
                        c,
                        space,
                        &entry,
                        routine,
                        expected_routine_revision,
                        external_grant,
                    )?;
                } else {
                    creation::external_grant::revoke(c, space, routine)?;
                }
                let entry = creation::touch_review(c, &entry)?;
                updated.push(entry.clone());
                entry
            } else if card.decision == decision && decision != "enabled" {
                entry
            } else {
                anyhow::ensure!(card.decision == "pending" && entry.rev == entry_rev, "routine_review_conflict");
                if decision == "cancelled" {
                    if next.routines.iter().any(|r| r.id == card.routine_id && r.bot_id == card.bot_id && !r.deleted) {
                        super::remove_in(&mut next, &card.bot_id, &card.routine_id)?;
                    }
                } else if decision == "enabled" {
                    let routine = next
                        .routines
                        .iter_mut()
                        .find(|r| r.id == card.routine_id && r.bot_id == card.bot_id && !r.deleted)
                        .context("routine_review_unavailable")?;
                    anyhow::ensure!(
                        expected_routine_revision == Some(creation::routine_revision(routine)?.as_str()),
                        "routine_review_conflict"
                    );
                    crate::routines::enable_for_review(routine)?;
                } else {
                    anyhow::ensure!(
                        next.routines.iter().any(|r| r.id == card.routine_id && r.bot_id == card.bot_id && !r.deleted),
                        "routine_review_unavailable"
                    );
                }
                let entry = creation::settle_on(c, &entry, card, decision)?;
                updated.push(entry.clone());
                entry
            };
            let routine_id =
                settled.data["routineCreation"]["routineId"].as_str().context("routine_review_unavailable")?;
            let result = json!({"entryId":entry_id,"routineId":routine_id,"decision":decision,"rev":settled.rev});
            retain_history(&mut next);
            let live = guard_live_runs(&next).map(|(bot, run)| (bot.to_owned(), run.to_owned())).collect();
            Ok((result, serde_json::to_string(&next)?, live))
        })?;
        let hooks_changed = hook_set(&guard) != hook_set(&next);
        *guard = next;
        drop(guard);
        if hooks_changed {
            self.hooks_rev.send_modify(|v| *v += 1);
        }
        creation::publish(hub, updated);
        creation::changed(hub, space);
        Ok(result)
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
