//! Adapter compilado como filho de `routines`: acesso ao estado privado sem API pública nova.
use super::{EntryKind, Hub, Lane, Result, Routine, Routines, Run, State, Status, Store, Value, json, now_ms};
use crate::LockExt;
use crate::fork::cadeias::{
    admissao,
    modelo::{Escopo, Fase, Origem, Pedido, PreparacaoNaoPersistida, Recibo, Recusa, Tipo},
};
use anyhow::{Context, ensure};
use rusqlite::Connection;

/// Desfecho da confirmação de início de uma execução de rotina.
pub(crate) enum Inicio {
    /// Recibo vencedor do CAS: o despacho foi confirmado e a execução está em andamento.
    Iniciada,
    /// A rotina virou «só quando eu mandar» depois da reserva automática: a execução
    /// foi cancelada na mesma passada, sem despacho — nada chega ao motor.
    Cancelada,
}

/// Candidato do produtor único, ainda sem recibo nem efeito persistido.
pub(super) struct Reserva {
    run: Run,
    pedido: Pedido,
}

impl Reserva {
    fn com_recibo(mut self, receipt: Recibo) -> Run {
        self.run.id.clone_from(&receipt.run_id);
        self.run.chain = Some(receipt);
        self.run
    }

    /// Só a Connection da guarda MCP; IDs/digest/ocorrência já foram preparados.
    pub(super) fn admitir_na(self, c: &Connection) -> Result<Run> {
        let Reserva { mut run, mut pedido } = self;
        let receipt = if matches!(pedido.escopo, Escopo::RotinaDescendente { .. }) {
            crate::fork::cadeias::descendencia_rotina::admit_on(c, &mut pedido)??
        } else {
            crate::fork::cadeias::store::admit(c, &pedido)??
        };
        run.id.clone_from(&receipt.run_id);
        run.chain = Some(receipt);
        Ok(run)
    }
}

pub(super) fn reservar(
    store: &Store,
    routine: &Routine,
    event: Value,
    delivery: Option<String>,
    occurrence: String,
    source: Origem,
) -> Result<Run> {
    let reservation = preparar(store, routine, event, delivery, occurrence, source)?;
    let receipt = admissao::admitir(store, &reservation.pedido)?;
    Ok(reservation.com_recibo(receipt))
}

fn preparar(
    store: &Store,
    routine: &Routine,
    event: Value,
    delivery: Option<String>,
    occurrence: String,
    source: Origem,
) -> Result<Reserva> {
    let run = super::new_run(routine, event, delivery);
    let intent = admissao::digest(&(&routine.bot_id, &routine.id, &run.event, &run.delivery_id))?;
    if let Some(origin) = &routine.creation_origin {
        let pedido = crate::fork::cadeias::descendencia_rotina::prepare(
            origin,
            &routine.bot_id,
            &routine.id,
            &run.id,
            occurrence,
            intent,
        );
        return Ok(Reserva { run, pedido });
    }
    let mut request = admissao::pedido(Tipo::Raiz, occurrence, intent, &routine.bot_id, &run.id, None);
    request.origem = source;
    request.escopo = Escopo::ConversaDoBot {
        bot_id: routine.bot_id.clone(),
        space_id: store.bot(&routine.bot_id)?.map(|row| row.config.space_id),
    };
    request.rotina = Some(routine.id.clone());
    Ok(Reserva { run, pedido: request })
}

pub(super) fn manual(store: &Store, routine: &Routine, event: Value, delivery: Option<String>) -> Result<Run> {
    let reservation = preparar_manual(store, routine, event, delivery)?;
    let receipt = admissao::admitir(store, &reservation.pedido)?;
    Ok(reservation.com_recibo(receipt))
}

pub(super) fn preparar_manual(
    store: &Store,
    routine: &Routine,
    event: Value,
    delivery: Option<String>,
) -> Result<Reserva> {
    let occurrence = match &delivery {
        Some(id) => serde_json::to_string(&("manual-delivery", &routine.id, id))?,
        None => admissao::manual_pendente(store, &routine.id)?
            .unwrap_or_else(|| format!("manual:{}:{}", routine.id, uuid::Uuid::new_v4())),
    };
    preparar(store, routine, event, delivery, occurrence, Origem::Manual)
}

pub(super) fn agenda(store: &Store, routine: &Routine, anchor: (i64, usize)) -> Result<Run> {
    let occurrence = serde_json::to_string(&("schedule", &routine.id, routine.schedule_generation, anchor))?;
    let run = reservar(store, routine, json!({"source":"schedule"}), None, occurrence, Origem::Agenda)?;
    ensure!(
        !admissao::vinculada(store, run.chain.as_ref().context("Tentativa da rotina não encontrada")?)?,
        Recusa::ReciboInvalido
    );
    Ok(run)
}

/// Não resolve cadastro/Space de novo: a procedência privada fixa o contexto.
pub(super) fn preparar_descendente(
    routine: &Routine,
    event: Value,
    delivery: Option<String>,
    occurrence: String,
) -> Result<Reserva> {
    let origin = routine.creation_origin.as_ref().context("procedência descendente ausente")?;
    let run = super::new_run(routine, event, delivery);
    let intent = admissao::digest(&(&routine.bot_id, &routine.id, &run.event, &run.delivery_id))?;
    let pedido = crate::fork::cadeias::descendencia_rotina::prepare(
        origin,
        &routine.bot_id,
        &routine.id,
        &run.id,
        occurrence,
        intent,
    );
    Ok(Reserva { run, pedido })
}

fn recusa_descendente(mut run: Run, routine: &mut Routine, reason: Recusa) -> Run {
    let detail = reason.to_string();
    interromper(&mut run, &detail);
    routine.enabled = false;
    routine.last_error = Some(detail);
    run
}

/// Reserva nova ou recusa terminal; replay já vinculado nunca recria trabalho,
/// mesmo depois da retenção do JSON. Somente SQL de domínio na Tx do chamador.
pub(super) fn admitir_descendente_na(
    c: &Connection,
    routine: &mut Routine,
    reservation: Reserva,
) -> Result<Option<Run>> {
    let candidate = reservation.run.clone();
    match reservation.admitir_na(c) {
        Ok(run) => {
            let receipt = run.chain.as_ref().context("recibo descendente ausente")?;
            if crate::fork::cadeias::store::vinculada(c, receipt)? {
                return Ok(None);
            }
            Ok(Some(run))
        }
        Err(error) => match error.downcast_ref::<Recusa>() {
            Some(reason) => Ok(Some(recusa_descendente(candidate, routine, *reason))),
            None => Err(error),
        },
    }
}

impl Routines {
    /// A filha nunca abre reserva órfã: avanço, admissão, vínculo e JSON numa Tx.
    pub(super) fn schedule_descendants_due(&self, store: &Store, now: i64) -> Result<()> {
        let mut guard = self.state.locked();
        if guard.paused
            || !guard.routines.iter().any(|r| {
                r.creation_origin.is_some()
                    && !r.deleted
                    && r.enabled
                    && r.next_runs.iter().flatten().any(|at| *at <= now)
            })
        {
            return Ok(());
        }
        let mut next = guard.clone();
        let mut reservations = Vec::new();
        for (index, routine) in next.routines.iter_mut().enumerate() {
            if routine.creation_origin.is_none() || routine.deleted || !routine.enabled {
                continue;
            }
            let anchor = routine
                .next_runs
                .iter()
                .enumerate()
                .filter_map(|(index, at)| at.filter(|at| *at <= now).map(|at| (at, index)))
                .min();
            let mut fired = false;
            for (trigger, next_at) in routine.triggers.iter().zip(&mut routine.next_runs) {
                if let Some(at) = *next_at
                    && at <= now
                {
                    match trigger.advance(at, now) {
                        Ok(future) => {
                            fired = true;
                            *next_at = future;
                        }
                        Err(error) => {
                            routine.enabled = false;
                            routine.last_error = Some(format!("Agendamento inválido: {error:#}"));
                            break;
                        }
                    }
                }
            }
            if fired
                && routine.enabled
                && !next.runs.iter().any(|r| r.routine_id == routine.id && r.status == Status::Pending)
            {
                let occurrence = serde_json::to_string(&(
                    "schedule",
                    &routine.id,
                    routine.schedule_generation,
                    anchor.context("Ocorrência sem horário devido")?,
                ))?;
                let mut reservation = preparar_descendente(routine, json!({"source":"schedule"}), None, occurrence)?;
                reservation.run.catchup = next.available_at > 0 && anchor.is_some_and(|(at, _)| at < next.available_at);
                reservations.push((index, reservation));
            }
        }
        store.guard_routines_change_in_origin(None, None, |c| {
            for (index, reservation) in reservations {
                if let Some(run) = admitir_descendente_na(c, &mut next.routines[index], reservation)? {
                    next.runs.push(run);
                }
            }
            super::bound_save::retain_history(&mut next);
            let live = super::guard_live_runs(&next).map(|(bot, run)| (bot.to_owned(), run.to_owned())).collect();
            Ok(((), serde_json::to_string(&next)?, live))
        })?;
        let hooks_changed = super::hook_set(&guard) != super::hook_set(&next);
        *guard = next;
        drop(guard);
        if hooks_changed {
            self.hooks_rev.send_modify(|v| *v += 1);
        }
        Ok(())
    }

    /// Gancho único app/MCP: false/None só para rotina independente. Não altera
    /// o percurso legado; origem MCP e JSON são reconferidos na mesma IMMEDIATE.
    pub(super) fn enqueue_descendant(
        &self,
        store: &Store,
        bot: &str,
        origin: Option<&super::tool_origin::ToolOrigin>,
        request: &super::bound_save::ManualRun<'_>,
    ) -> Result<Option<Value>> {
        let mut guard = self.state.locked();
        let Some(index) = guard.routines.iter().position(|r| r.id == request.id && r.bot_id == bot && !r.deleted)
        else {
            return Ok(None);
        };
        if guard.routines[index].creation_origin.is_none() {
            return Ok(None);
        }
        ensure!(!guard.paused, super::hooks::Refused::Paused);
        ensure!(!guard.routines[index].external_creation || guard.routines[index].enabled, super::EXTERNAL_RELEASE);
        ensure!(guard.routines[index].enabled || request.test, super::hooks::Refused::Paused);
        let previous = request
            .delivery
            .as_ref()
            .and_then(|key| {
                guard.runs.iter().find(|r| r.routine_id == request.id && r.delivery_id.as_ref() == Some(key))
            })
            .cloned();
        ensure!(
            previous.is_some() || !guard.runs.iter().any(|r| r.routine_id == request.id && r.status.active()),
            super::hooks::Refused::Busy
        );
        let occurrence = match &request.delivery {
            Some(id) => serde_json::to_string(&("manual-delivery", request.id, id))?,
            None => format!("manual:{}:{}", request.id, uuid::Uuid::new_v4()),
        };
        let reservation =
            preparar_descendente(&guard.routines[index], request.event.clone(), request.delivery.clone(), occurrence)?;
        let mut next = guard.clone();
        #[cfg(test)]
        crate::fork::sessoes::call_tests::before_effect(store, "routine_commit");
        let result = store.guard_routines_change_in_origin(Some(bot), origin, |c| {
            let value = if let Some(previous) = previous {
                if previous.chain.is_some() {
                    let replay = reservation.admitir_na(c)?;
                    ensure!(replay.id == previous.id, Recusa::ReciboInvalido);
                }
                json!({"run":super::public_run(&previous)})
            } else {
                let run = admitir_descendente_na(c, &mut next.routines[index], reservation)?
                    .context("Esta ocorrência já foi admitida; nenhum trabalho foi recriado.")?;
                let value = json!({"run":super::public_run(&run)});
                next.runs.push(run);
                value
            };
            super::bound_save::retain_history(&mut next);
            let live = super::guard_live_runs(&next).map(|(bot, run)| (bot.to_owned(), run.to_owned())).collect();
            Ok((value, serde_json::to_string(&next)?, live))
        })?;
        let hooks_changed = super::hook_set(&guard) != super::hook_set(&next);
        *guard = next;
        drop(guard);
        if hooks_changed {
            self.hooks_rev.send_modify(|v| *v += 1);
        }
        Ok(Some(result))
    }
}

fn interromper(run: &mut Run, detail: &str) {
    run.status = Status::Interrupted;
    run.detail = Some(detail.into());
    run.finished_at = Some(now_ms());
    run.report_pending = true;
    run.event = Value::Null;
}

/// Antes da conversão de estados legados e do spawn dos atores.
pub(super) fn restaurar(store: &Store, state: &mut State) -> Result<()> {
    for run in state.runs.iter_mut().filter(|run| run.status.active()) {
        let inherited = state
            .routines
            .iter()
            .find(|r| r.id == run.routine_id && r.bot_id == run.bot_id)
            .and_then(|r| r.creation_origin.as_ref());
        if run.chain.is_none() && inherited.is_some() {
            interromper(
                run,
                "Não foi possível conferir a procedência desta execução descendente. Nenhuma nova cadeia foi criada.",
            );
            continue;
        }
        if run.chain.is_none() {
            // Normalizar não é retomar IA nem encerrar trabalho: uma exclusão
            // ainda pode cancelar esta Run na sua própria transação. Sem recibo,
            // prepare_attempt recusa a retomada sem inventar orçamento passado.
            if matches!(run.status, Status::Running | Status::Recovering) {
                continue;
            }
            if run.started_at.is_some() {
                interromper(
                    run,
                    "Não foi possível conferir as tentativas anteriores da rotina. A execução foi interrompida; o histórico foi preservado.",
                );
                continue;
            }
            let mut request = admissao::pedido(
                Tipo::Raiz,
                format!("legacy-run:{}", run.id),
                admissao::digest(&(&run.bot_id, &run.routine_id, &run.id))?,
                &run.bot_id,
                &run.id,
                None,
            );
            request.origem = Origem::Legada;
            request.escopo = Escopo::ConversaDoBot {
                bot_id: run.bot_id.clone(),
                space_id: store.bot(&run.bot_id)?.map(|row| row.config.space_id),
            };
            request.rotina = Some(run.routine_id.clone());
            run.chain = Some(admissao::admitir(store, &request)?);
        }
        let receipt = run.chain.as_ref().context("Tentativa da rotina não encontrada")?;
        ensure!(receipt.run_id == run.id && receipt.bot_id == run.bot_id, Recusa::ReciboInvalido);
        if let Some(origin) = inherited {
            match admissao::contexto(store, receipt) {
                Ok(context) if context.escopo == origin.scope(&run.bot_id, &run.routine_id) => {}
                Ok(_) => {
                    interromper(
                        run,
                        "A procedência da rotina não corresponde à cadeia salva. Nenhuma tentativa foi iniciada.",
                    );
                    continue;
                }
                Err(error) if error.downcast_ref::<Recusa>().is_some() => {
                    interromper(run, "Não foi possível conferir a cadeia descendente. Nenhuma tentativa foi iniciada.");
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
        match admissao::fase(store, receipt)? {
            Fase::EnvioPossivel => run.status = Status::Recovering,
            Fase::Concluida => interromper(
                run,
                "A confirmação da rotina foi interrompida. Confira o histórico; nenhuma tentativa foi repetida automaticamente.",
            ),
            Fase::Reservada => {}
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PreparationRequirement {
    Dispatch,
    LegacyWithoutReceipt,
}

impl Routines {
    /// O recibo viaja em variável local através dos awaits de preparação do ator.
    pub(crate) fn prepare_attempt(&self, store: &Store, bot: &str, id: &str) -> Result<Recibo> {
        #[cfg(test)]
        if self.injected_failure() {
            return Err(crate::fork::atividades::modelo::CoreError::StoreBusy.into());
        }
        self.prepare_with_requirement(store, bot, id, PreparationRequirement::Dispatch)?
            .context("Não foi possível conferir a tentativa desta rotina")
    }

    /// Negativa apenas: nunca prepara um recibo, mesmo se a consulta anterior envelheceu.
    pub(crate) fn reject_legacy_recovery(&self, store: &Store, bot: &str, id: &str) -> Result<()> {
        let _ = self.prepare_with_requirement(store, bot, id, PreparationRequirement::LegacyWithoutReceipt)?;
        Ok(())
    }

    fn prepare_with_requirement(
        &self,
        store: &Store,
        bot: &str,
        id: &str,
        requirement: PreparationRequirement,
    ) -> Result<Option<Recibo>> {
        let mut preparada = false;
        let receipt = self
            .change(store, |state| {
                let run = state
                    .runs
                    .iter_mut()
                    .find(|run| run.id == id && run.bot_id == bot)
                    .context("Execução de rotina não encontrada")?;
                ensure!(matches!(run.status, Status::Starting | Status::Recovering), Recusa::ReciboInvalido);
                if requirement == PreparationRequirement::Dispatch
                    && run.event["source"] != "test"
                    && state.routines.iter().any(|r| r.id == run.routine_id && r.triggers.is_empty())
                {
                    // A rotina virou «só quando eu mandar» depois de esta execução automática ser reservada.
                    crate::routines::cancel_run(run);
                    preparada = true;
                    return Ok(None);
                }
                if requirement == PreparationRequirement::LegacyWithoutReceipt {
                    ensure!(run.status == Status::Recovering && run.chain.is_none(), Recusa::ReciboInvalido);
                }
                let Some(previous) = run.chain.as_ref() else {
                    interromper(
                        run,
                        "Não foi possível conferir as tentativas anteriores da rotina. A execução foi interrompida; o histórico foi preservado.",
                    );
                    // Confirmar a interrupção antes de devolver o erro. Err dentro
                    // de change descartaria o candidato e deixaria a Run ativa.
                    preparada = true;
                    return Ok(None);
                };
                ensure!(previous.run_id == id && previous.bot_id == bot, Recusa::ReciboInvalido);
                let next = match admissao::fase(store, previous)? {
                    Fase::Reservada => previous.clone(),
                    Fase::EnvioPossivel => {
                        let contexto = admissao::contexto(store, previous)?;
                        let mut request = admissao::pedido(
                            Tipo::Continuacao,
                            format!("resume:{}", previous.admission_id),
                            admissao::digest(&("routine-resume", id, &previous.admission_id))?,
                            bot,
                            id,
                            Some(previous.clone()),
                        );
                        request.escopo = contexto.escopo;
                        request.rotina = contexto.rotina;
                        admissao::admitir(store, &request)?
                    }
                    Fase::Concluida => return Err(Recusa::ReciboInvalido.into()),
                };
                run.chain = Some(next.clone());
                preparada = true;
                Ok(Some(next))
            })
            .map_err(|error| {
                // Só erros posteriores ao callback são falhas ao salvar. Recusas
                // de recibo, contexto ou limite não autorizam repetir preparação.
                if preparada { error.context(PreparacaoNaoPersistida) } else { error }
            })?;
        if receipt.is_none() {
            self.release(bot);
        }
        Ok(receipt)
    }

    /// Só é sucesso para o vencedor do CAS. Retry SQL não concede novo envio.
    pub(crate) fn mark_started(&self, store: &Store, bot: &str, id: &str, receipt: &Recibo) -> Result<Inicio> {
        self.change(store, |state| {
            let run = state
                .runs
                .iter_mut()
                .find(|run| run.id == id && run.bot_id == bot)
                .context("Execução de rotina não encontrada")?;
            ensure!(
                matches!(run.status, Status::Starting | Status::Recovering | Status::Running)
                    && run.chain.as_ref() == Some(receipt),
                Recusa::ReciboInvalido
            );
            // «Só quando eu mandar»: a rotina pode ter virado manual entre o
            // preparo e esta confirmação. A execução automática é cancelada na
            // MESMA passada que conferiu o recibo, antes do despacho — nada
            // chega ao motor e o ator encerra a tentativa sem falha.
            if run.event["source"] != "test"
                && state.routines.iter().any(|r| r.id == run.routine_id && r.triggers.is_empty())
            {
                crate::routines::cancel_run(run);
                return Ok(Inicio::Cancelada);
            }
            admissao::despachar(store, receipt)?;
            run.status = Status::Running;
            run.started_at.get_or_insert_with(now_ms);
            Ok(Inicio::Iniciada)
        })
    }

    /// Serializa decisão e aviso com reserva/vínculo; estado ocupado adia para o próximo tick.
    pub(super) fn publish_chain_orphans(hub: &Hub) -> Result<()> {
        // A lista de candidatas vem antes de qualquer trava de Rotinas: a espera curta pelo banco não segura o estado.
        let orphans = admissao::orfas(&hub.store)?;
        let state = match hub.routines.state.try_lock() {
            Ok(state) => state,
            Err(std::sync::TryLockError::WouldBlock) => return Ok(()),
            Err(std::sync::TryLockError::Poisoned(_)) => {
                anyhow::bail!("Não foi possível conferir o estado das rotinas para publicar avisos.");
            }
        };
        for (chain, bot, routine) in orphans {
            // Sob o estado a conferência não espera: banco ocupado adia tudo para o próximo tick.
            match admissao::orfa_pendente(&hub.store, &chain) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(error) if crate::fork::atividades::store::is_store_busy(&error) => return Ok(()),
                Err(error) => return Err(error),
            }
            // Cada leitura solta DB antes de add_entry_once: state -> emit_lock -> DB, sem reentrada.
            hub.add_entry_once(
                &format!("routine-chain-orphan-{chain}"), &Lane::main(&bot), EntryKind::Notice, 0,
                &json!({"text":"Esta ocorrência da rotina não foi iniciada porque sua gravação foi interrompida. O histórico foi preservado.",
                    "style":"error","routineId":routine}),
            )?;
        }
        drop(state);
        Ok(())
    }
}

#[cfg(test)]
#[path = "rotinas_testes.rs"]
mod testes;
