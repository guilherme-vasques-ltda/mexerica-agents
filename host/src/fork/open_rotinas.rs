//! Rotinas na Open: recusa de gatilhos de rede no salvamento e execução das pendentes pelo turno só de texto.
use crate::LockExt;
use crate::fork::open_chat;
use crate::fork::rotina_resultado::{End, Observed, Reason};
use crate::hub::Hub;
use anyhow::{Result, ensure};
use serde_json::Value;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};

/// A Open só agenda por relógio: gatilho de webhook ou de evento depende de entrada externa, e a checagem
/// silenciosa depende de fontes que a Open não tem. Confere os gatilhos já interpretados, venham de `triggers`
/// ou do rascunho `schedule` do editor (inclusive `keep`, que reaproveita os gatilhos enviados).
pub(crate) fn check_save(body: &Value) -> Result<()> {
    ensure!(body.get("heartbeat").is_none_or(Value::is_null), "open_routine_heartbeat_unavailable");
    let triggers = match (body.get("schedule"), body.get("triggers")) {
        (Some(draft), _) => serde_json::from_value::<crate::routines::ScheduleDraft>(draft.clone())
            .ok()
            .and_then(|draft| draft.triggers().ok())
            .and_then(|triggers| serde_json::to_value(triggers).ok())
            .ok_or_else(|| anyhow::anyhow!("open_routine_trigger_invalid"))?,
        (None, Some(triggers)) => triggers.clone(),
        (None, None) => return Ok(()),
    };
    let Some(triggers) = triggers.as_array() else { anyhow::bail!("open_routine_trigger_invalid") };
    for trigger in triggers {
        ensure!(
            matches!(trigger.get("type").and_then(Value::as_str), Some(kind) if kind != "webhook" && kind != "event"),
            "open_routine_trigger_unavailable"
        );
    }
    Ok(())
}

pub(crate) fn start(hub: &Arc<Hub>) {
    let weak = Arc::downgrade(hub);
    tokio::spawn(async move {
        let running = Arc::new(Mutex::new(HashSet::<String>::new()));
        let mut clock = tokio::time::interval(Duration::from_secs(1));
        clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            clock.tick().await;
            let Some(hub) = weak.upgrade() else { break };
            if hub.routines.stopping() {
                break;
            }
            for (bot, run) in hub.routines.pending_open_runs() {
                if !open_chat::open_busy(&[bot.as_str()]).is_empty() || !running.locked().insert(run.clone()) {
                    continue;
                }
                let (hub, running) = (hub.clone(), running.clone());
                tokio::spawn(async move {
                    execute(&hub, &bot, &run).await;
                    running.locked().remove(&run);
                });
            }
        }
    });
}

async fn execute(hub: &Arc<Hub>, bot: &str, run: &str) {
    // A vez do bot é reservada antes da execução: ocupado por outra rotina ou por uma mensagem do dono, a execução
    // continua pendente e volta no próximo ciclo, sem virar falha.
    let admitted = (|| {
        let cfg = hub
            .store
            .bot(bot)?
            .filter(|row| !row.deleted)
            .ok_or_else(|| anyhow::anyhow!("open_chat_cancelled"))?
            .config;
        open_chat::precheck(hub, &cfg, "")?;
        let ticket = open_chat::admit_locked(&mut open_chat::RUNNING.locked(), hub, &cfg)?;
        Ok::<_, anyhow::Error>((cfg, ticket))
    })();
    if matches!(&admitted, Err(error) if error.to_string() == "open_chat_busy") {
        return;
    }
    let claim = {
        let (hub, bot, run) = (hub.clone(), bot.to_owned(), run.to_owned());
        tokio::task::spawn_blocking(move || hub.routines.begin(&hub, &bot, &run)).await
    };
    let claim = match claim {
        Ok(Ok(Some(prepared))) => Ok(prepared),
        Ok(Ok(None)) => {
            if admitted.is_ok() {
                crate::fork::open_chat_fila::finish(hub, bot);
            }
            return;
        }
        Ok(Err(error)) => Err(error),
        Err(error) => Err(anyhow::anyhow!("open_routine_claim_failed: {error}")),
    };
    // Falha na reserva (inclusive depois de gravar `Starting`) é concluída como falha: o turno de IA não começou.
    let outcome = match (claim, admitted) {
        (Ok(prepared), Ok((cfg, ticket))) => {
            open_chat::routine_turn(hub, ticket, &cfg, prepared.lane, prepared.prompt, prepared.timeout).await
        }
        (claim, admitted) => {
            if admitted.is_ok() {
                crate::fork::open_chat_fila::finish(hub, bot);
            }
            Err(claim.err().or(admitted.err()).unwrap_or_else(|| anyhow::anyhow!("open_chat_unavailable")))
        }
    };
    let (observed, detail, text) = match outcome {
        Ok(text) => (
            Observed {
                end: End::Finished(Some("end_turn".into())),
                visible_reply: !text.trim().is_empty(),
                terminal: false,
            },
            None,
            Some(text),
        ),
        Err(error) => {
            let detail = error.to_string().chars().take(200).collect::<String>();
            (Observed::failed(Reason::AgentError), Some(detail), None)
        }
    };
    // Gravar o resultado tenta de novo enquanto o Hub roda: o texto gerado não se perde e a execução sai de `Running`.
    for attempt in 0u32.. {
        let (shared, id, observed, detail, text) =
            (hub.clone(), run.to_owned(), observed.clone(), detail.clone(), text.clone());
        let saved =
            tokio::task::spawn_blocking(move || shared.routines.complete(&shared, &id, &observed, detail, text)).await;
        if matches!(saved, Ok(Ok(()))) {
            return;
        }
        tracing::error!(run, attempt, "resultado da rotina da Open não gravado");
        if hub.routines.stopping() {
            return;
        }
        tokio::time::sleep(Duration::from_secs(2u64.saturating_pow(attempt.min(5)))).await;
    }
}

#[cfg(test)]
mod tests {
    use super::check_save;
    use serde_json::json;

    #[test]
    fn salvar_recusa_gatilho_de_rede_e_aceita_relogio() {
        assert!(check_save(&json!({})).is_ok());
        assert!(
            check_save(&json!({"triggers": [{"type": "interval", "seconds": 60}, {"type": "once", "at": 1}]})).is_ok()
        );
        assert!(
            check_save(&json!({"triggers": [{"type": "cron", "expression": "0 9 * * *", "timeZone": "UTC"}]})).is_ok()
        );
        assert!(check_save(&json!({"triggers": [{"type": "webhook"}]})).is_err());
        assert!(check_save(&json!({"triggers": [{"type": "interval", "seconds": 60}, {"type": "event", "source": "s", "event": "e"}]})).is_err());
        assert!(check_save(&json!({"triggers": [{}]})).is_err());
        assert!(check_save(&json!({"triggers": "webhook"})).is_err());
        assert!(check_save(&json!({"schedule": {"kind": "webhook"}})).is_err());
        assert!(check_save(&json!({"schedule": {"kind": "keep", "original": [{"type": "webhook"}]}})).is_err());
        assert!(
            check_save(&json!({"schedule": {"kind": "keep", "original": [{"type": "interval", "seconds": 60}]}}))
                .is_ok()
        );
        assert!(check_save(&json!({"schedule": {"kind": "cron", "zone": "UTC"}})).is_ok());
        assert!(check_save(&json!({"schedule": {"kind": "manual"}})).is_ok());
        assert!(check_save(&json!({"schedule": "webhook"})).is_err());
        assert!(check_save(&json!({"heartbeat": {}})).is_err());
        assert!(check_save(&json!({"heartbeat": null})).is_ok());
    }
}
