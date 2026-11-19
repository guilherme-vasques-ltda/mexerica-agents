use super::{
    CANCEL_RETRY, LEASE, MAX_RESULT, MAX_STDIO, POLL_WAIT,
    jobs::{self, Phase},
    registry,
};
use crate::LockExt;
use crate::agent::bot::Cmd;
use crate::api::devices::Caller;
use crate::hub::Hub;
use crate::store::{Scope, now_ms};
use anyhow::{Result, anyhow, bail, ensure};
use serde_json::{Value, json};
use std::{collections::HashMap, ops::ControlFlow, sync::Arc, time::Duration};

fn arg<'a>(b: &'a Value, k: &str) -> Result<&'a str> {
    b[k].as_str().ok_or_else(|| anyhow!("invalid_arguments: {k} obrigatório"))
}
fn computer(hub: &Hub, caller: &Caller) -> Result<registry::Computer> {
    let Caller::Device { key, scopes, .. } = caller else {
        bail!("not_allowed_here: só executor");
    };
    ensure!(scopes == &[Scope::Executor], "not_allowed_here: só credencial de executor");
    hub.store
        .computers()?
        .into_iter()
        .find(|c| c.device_key == *key)
        .ok_or_else(|| anyhow!("removed: computador removido"))
}
fn owned(hub: &Hub, c: &registry::Computer, b: &Value) -> Result<jobs::Job> {
    let j = jobs::find(hub, arg(b, "jobId")?)?;
    ensure!(j.computer_id == c.id, "unknown_job: trabalho de outro computador");
    ensure!(
        j.lease_token.as_deref().is_some_and(|t| crate::remote::crypto::ct_eq(
            t.as_bytes(),
            b["leaseToken"].as_str().unwrap_or_default().as_bytes()
        )),
        "lease_lost: lease inválido"
    );
    Ok(j)
}
fn presence(hub: &Hub, c: &mut registry::Computer, b: &Value) -> Result<()> {
    let p = &b["presence"];
    ensure!(
        p.is_object()
            && p["appConnected"].is_boolean()
            && p["locked"].is_boolean()
            && p["stopped"].is_boolean()
            && p["idleSeconds"].as_u64().is_some(),
        "invalid_arguments: presença incompleta"
    );
    let old = registry::state(hub, c, now_ms());
    c.presence = p.clone();
    c.last_seen_at = Some(now_ms());
    if b["capabilities"].is_object() || b["capabilities"].is_array() {
        c.capabilities = b["capabilities"].clone();
    }
    if old != registry::state(hub, c, now_ms()) {
        c.state_since = Some(now_ms());
    }
    hub.store.save_computer(c)
}
fn require_hello(hub: &Hub, c: &registry::Computer, b: &Value) -> Result<()> {
    ensure!(
        hub.computers.sessions.locked().get(&c.id).is_some_and(|s| Some(s.boot.as_str()) == b["bootId"].as_str()),
        "hello_required: envie executorHello"
    );
    Ok(())
}
fn empty_poll() -> Value {
    json!({"jobs":[],"cancel":[],"registerRoutines": Vec::<jobs::RoutineSpec>::new(),"revokeRoutines":[]})
}

pub async fn call(
    hub: &Arc<Hub>,
    caller: &Caller,
    method: &str,
    b: Value,
    execution: Option<&crate::fork::sessoes::ExecutionRef>,
) -> Result<ControlFlow<Value, Value>> {
    let answer = match method {
        "computersCall" => super::tools::call(hub, &b, execution).await,
        "computersList" => registry::list(hub, None)?,
        "computerEnrollCode" => super::pairing::issue(hub),
        "computerJob" => jobs::response(hub, &jobs::find(hub, arg(&b, "jobId")?)?),
        "computerRename" => {
            let _gate = hub.computers.gate.locked();
            let mut c = registry::resolve(hub, Some(arg(&b, "computerId")?), None)?;
            let name = arg(&b, "label")?.trim();
            ensure!(!name.is_empty() && name.chars().count() <= 100, "invalid_arguments: rótulo inválido");
            c.label = name.into();
            hub.store.save_computer(&c)?;
            json!({})
        }
        "computerRemove" => {
            let c = registry::resolve(hub, Some(arg(&b, "computerId")?), None)?;
            hub.revoke_device(&c.device_key)?;
            jobs::sweep(hub, now_ms())?;
            hub.computers.wake();
            json!({})
        }
        "computerBots" => {
            let before = registry::bots(&hub.store)?;
            if let Some(ids) = b.get("bots") {
                let ids: Vec<String> = serde_json::from_value(ids.clone())?;
                for id in &ids {
                    ensure!(
                        hub.store.bot(id)?.is_some_and(|r| !r.deleted && !r.config.is_group()),
                        "invalid_arguments: bot inválido"
                    );
                }
                hub.store.kv_set("computers.bots", &serde_json::to_string(&ids)?)?;
                for id in before.iter().chain(&ids).filter(|id| before.contains(id) != ids.contains(id)) {
                    let _ = hub.send_cmd(id, Cmd::RefreshTools);
                }
                hub.computers.wake();
                json!({"bots":ids})
            } else {
                json!({"bots":before})
            }
        }
        "executorHello" => hello(hub, caller, &b)?,
        "executorPoll" => poll(hub, caller, &b).await?,
        "executorUpdate" => update(hub, caller, &b)?,
        "executorResult" => result(hub, caller, &b)?,
        "executorAudit" => {
            let _gate = hub.computers.gate.locked();
            let c = computer(hub, caller)?;
            let records = b["records"]
                .as_array()
                .filter(|v| v.len() <= 50)
                .ok_or_else(|| anyhow!("too_large: até 50 registros"))?;
            json!({"ackSeq":hub.store.computer_audit(&c.id,records)?})
        }
        "executorBye" => {
            let _gate = hub.computers.gate.locked();
            let mut c = computer(hub, caller)?;
            match arg(&b, "reason")? {
                "sleep" => c.presence["asleep"] = json!(true),
                "quit" => c.presence["appConnected"] = json!(false),
                "stop" => c.presence["stopped"] = json!(true),
                "revoke" => {
                    hub.revoke_device(&c.device_key)?;
                }
                _ => bail!("invalid_arguments: motivo inválido"),
            }
            c.state_since = Some(now_ms());
            hub.store.save_computer(&c)?;
            hub.computers.sessions.locked().remove(&c.id);
            hub.computers.wake();
            json!({})
        }
        _ => return Ok(ControlFlow::Continue(b)),
    };
    Ok(ControlFlow::Break(answer))
}
fn hello(hub: &Hub, caller: &Caller, b: &Value) -> Result<Value> {
    let _gate = hub.computers.gate.locked();
    let mut c = computer(hub, caller)?;
    ensure!(b["proto"] == 1, "proto_unsupported: protocolo não suportado");
    let boot = arg(b, "bootId")?;
    ensure!(!boot.is_empty() && boot.len() <= 128, "invalid_arguments: bootId inválido");
    ensure!(b["platform"] == c.platform, "invalid_arguments: plataforma mudou");
    let journal = b["journal"]
        .as_array()
        .filter(|v| v.len() <= 50)
        .ok_or_else(|| anyhow!("too_large: diário de até 50 itens"))?;
    validate_routines(b)?;
    let previous = hub.computers.sessions.locked().get(&c.id).cloned();
    let mut reconcile = Vec::new();
    for record in journal {
        let id = arg(record, "jobId")?;
        let action = if let Ok(mut j) = jobs::find(hub, id) {
            ensure!(j.computer_id == c.id, "unknown_job: diário de outro computador");
            if j.state == Phase::Running
                && (record["state"] == "unknown" || c.last_boot_id.as_deref().is_some_and(|last| last != boot))
            {
                jobs::mark_unknown(&mut j, now_ms());
                jobs::save(hub, &j)?;
            }
            // A started job must retain any real result in the helper's outbox,
            // even after Stop. Its pending cancellation goes through the poll.
            if j.started_at.is_none() && (j.cancel_requested || j.state == Phase::Cancelled) {
                "cancelled"
            } else if j.result.is_none() {
                "resend"
            } else {
                "forget"
            }
        } else {
            "forget"
        };
        reconcile.push(json!({"jobId":id,"action":action}));
    }
    c.app_version = b["version"].as_str().map(str::to_owned);
    c.os_version = b["osVersion"].as_str().map(str::to_owned);
    c.arch = b["arch"].as_str().map(str::to_owned);
    // Bye discards the session, but waking in the same boot must preserve Stop.
    c.last_boot_id = Some(boot.into());
    presence(hub, &mut c, b)?;
    let poll = previous.map_or(0, |s| s.poll.wrapping_add(1));
    // A new connection must retry pending Stop requests immediately: a previous
    // poll may have completed on the Hub while its response was lost.
    hub.computers
        .sessions
        .locked()
        .insert(c.id.clone(), super::Session { boot: boot.into(), poll, cancel_sent_at: HashMap::new() });
    hub.computers.wake();
    Ok(
        json!({"computerId":c.id,"label":c.label,"serverTimeMs":now_ms(),"limits":{"pollWaitMs":POLL_WAIT,"leaseMs":LEASE,"maxResult":MAX_RESULT},"reconcile":reconcile,"registerRoutines": Vec::<jobs::RoutineSpec>::new(),"revokeRoutines":[]}),
    )
}
fn validate_routines(b: &Value) -> Result<()> {
    if let Some(r) = b.get("routines") {
        let rs = r.as_array().filter(|r| r.len() <= 50).ok_or_else(|| anyhow!("too_large: até 50 rotinas"))?;
        for r in rs {
            ensure!(
                r["id"].is_string()
                    && r["digest"].is_string()
                    && matches!(r["state"].as_str(), Some("registered" | "denied"))
                    && r["at"].as_i64().is_some(),
                "invalid_arguments: relato de rotina inválido"
            );
        }
    }
    Ok(())
}
async fn poll(hub: &Hub, caller: &Caller, b: &Value) -> Result<Value> {
    let mut changed = hub.computers.watch();
    let (id, generation) = {
        let _gate = hub.computers.gate.locked();
        let mut c = computer(hub, caller)?;
        require_hello(hub, &c, b)?;
        validate_routines(b)?;
        presence(hub, &mut c, b)?;
        jobs::sweep_locked(hub, now_ms())?;
        if let Some(id) = b["activeJob"].as_str() {
            let mut j = jobs::find(hub, id)?;
            ensure!(j.computer_id == c.id, "unknown_job: trabalho de outro computador");
            if j.state.active() && j.state != Phase::Queued {
                j.lease_until = Some(now_ms() + LEASE);
                hub.store.save_computer_job(&j)?;
            }
        }
        let mut sessions = hub.computers.sessions.locked();
        let s = sessions.get_mut(&c.id).ok_or_else(|| anyhow!("hello_required: envie executorHello"))?;
        s.poll = s.poll.wrapping_add(1);
        let generation = s.poll;
        hub.computers.wake();
        (c.id, generation)
    };
    // A pending active poll must return with enough margin for the next
    // request to renew the 15 s lease, even while approval is still open.
    let max_wait = if b["activeJob"].is_string() { 10_000 } else { POLL_WAIT };
    let wait = b["waitMs"].as_u64().unwrap_or(POLL_WAIT).min(max_wait);
    let end = tokio::time::Instant::now() + Duration::from_millis(wait);
    loop {
        let mut next_wake = end;
        {
            let _gate = hub.computers.gate.locked();
            let c = computer(hub, caller)?;
            if hub
                .computers
                .sessions
                .locked()
                .get(&id)
                .is_none_or(|s| s.poll != generation || Some(s.boot.as_str()) != b["bootId"].as_str())
            {
                return Ok(empty_poll());
            }
            jobs::sweep_locked(hub, now_ms())?;
            let mut all = hub.store.computer_jobs()?;
            let mut cancel = Vec::new();
            {
                let mut sessions = hub.computers.sessions.locked();
                let s = sessions.get_mut(&id).ok_or_else(|| anyhow!("hello_required: envie executorHello"))?;
                s.cancel_sent_at.retain(|id, _| all.iter().any(|j| j.id == *id && j.cancel_requested));
                let now = tokio::time::Instant::now();
                for j in all.iter_mut().filter(|j| j.computer_id == id && j.cancel_requested) {
                    if j.started_at.is_some() {
                        if let Some(sent) = s.cancel_sent_at.get(&j.id) {
                            let retry_at = *sent + Duration::from_millis(CANCEL_RETRY);
                            if retry_at > now {
                                next_wake = next_wake.min(retry_at);
                                continue;
                            }
                        }
                        // Keep the durable request until result/deadline. The
                        // session timestamp limits retries without assuming delivery.
                        s.cancel_sent_at.insert(j.id.clone(), now);
                    } else {
                        j.cancel_requested = false;
                        hub.store.save_computer_job(j)?;
                    }
                    cancel.push(j.id.clone());
                }
            }
            let active = all.iter().any(|j| {
                j.computer_id == id
                    && matches!(j.state, Phase::Delivered | Phase::Accepted | Phase::AwaitingApproval | Phase::Running)
            });
            let allowed = crate::fork::atividades::revisao::estado::work_allowed_on(&hub.store.db.locked()).is_ok();
            if allowed
                && !active
                && registry::state(hub, &c, now_ms()) == "online"
                && let Some(mut j) = all.into_iter().find(|j| j.computer_id == id && j.state == Phase::Queued)
            {
                j.state = Phase::Delivered;
                j.lease_token = Some(crate::remote::crypto::b64(&crate::remote::crypto::random::<32>()));
                j.lease_until = Some(now_ms() + LEASE);
                jobs::save(hub, &j)?;
                return Ok(
                    json!({"jobs":[jobs::wire(&j,now_ms())?],"cancel":cancel,"registerRoutines": Vec::<jobs::RoutineSpec>::new(),"revokeRoutines":[]}),
                );
            }
            if !cancel.is_empty() {
                return Ok(
                    json!({"jobs":[],"cancel":cancel,"registerRoutines": Vec::<jobs::RoutineSpec>::new(),"revokeRoutines":[]}),
                );
            }
        }
        if tokio::time::Instant::now() >= end {
            return Ok(empty_poll());
        }
        let _ = tokio::time::timeout_at(next_wake, changed.changed()).await;
    }
}
fn update(hub: &Hub, caller: &Caller, b: &Value) -> Result<Value> {
    let _gate = hub.computers.gate.locked();
    let mut c = computer(hub, caller)?;
    jobs::sweep_locked(hub, now_ms())?;
    let mut j = owned(hub, &c, b)?;
    ensure!(j.state != Phase::Cancelled && !j.cancel_requested, "job_cancelled: trabalho cancelado");
    ensure!(now_ms() < j.start_by && j.state != Phase::Expired, "job_expired: nada foi executado");
    ensure!(j.lease_until.is_some_and(|t| t > now_ms()), "lease_lost: lease expirado");
    match arg(b, "state")? {
        "accepted" => {
            ensure!(matches!(j.state, Phase::Delivered | Phase::Accepted), "lease_lost: estado inválido");
            j.state = Phase::Accepted;
        }
        "awaiting_approval" => {
            ensure!(matches!(j.state, Phase::Accepted | Phase::AwaitingApproval), "lease_lost: estado inválido");
            j.state = Phase::AwaitingApproval;
        }
        "start" => {
            crate::fork::atividades::revisao::estado::work_allowed_on(&hub.store.db.locked())?;
            ensure!(
                matches!(j.state, Phase::Accepted | Phase::AwaitingApproval) && jobs::origin_alive(hub, &j),
                "job_cancelled: origem encerrada ou trabalho já iniciado"
            );
            ensure!(registry::state(hub, &c, now_ms()) == "online", "job_cancelled: computador indisponível");
            j.state = Phase::Running;
            j.started_at = Some(now_ms());
            c.last_used_at = Some(now_ms());
        }
        _ => bail!("invalid_arguments: estado inválido"),
    }
    j.lease_until = Some(now_ms() + LEASE);
    jobs::save_with_computer(hub, &j, (j.state == Phase::Running).then_some(&c))?;
    Ok(json!({"go":true}))
}
fn result(hub: &Hub, caller: &Caller, b: &Value) -> Result<Value> {
    ensure!(serde_json::to_vec(b)?.len() <= MAX_RESULT, "too_large: resultado serializado acima de 40 KiB");
    ensure!(
        b["stdout"].as_str().map_or(0, str::len) + b["stderr"].as_str().map_or(0, str::len) <= MAX_STDIO,
        "too_large: stdout e stderr acima de 24 KiB"
    );
    ensure!(
        matches!(b["decidedBy"].as_str(), Some("click" | "policy" | "routine" | "none"))
            && b["auditSeq"].as_u64().is_some(),
        "invalid_arguments: decisão e auditSeq obrigatórios"
    );
    let _gate = hub.computers.gate.locked();
    let c = computer(hub, caller)?;
    jobs::sweep_locked(hub, now_ms())?;
    let mut j = owned(hub, &c, b)?;
    if j.result.is_some() && j.state != Phase::Unknown {
        return Ok(json!({"ack":true}));
    }
    let outcome = arg(b, "outcome")?;
    let phase = match outcome {
        "done" => Phase::Done,
        "failed" => Phase::Failed,
        "cancelled" => Phase::Cancelled,
        "denied" => Phase::Denied,
        "expired" => Phase::Expired,
        "refused" => Phase::Refused,
        "unknown" => Phase::Unknown,
        _ => bail!("invalid_arguments: resultado inválido"),
    };
    ensure!(
        !matches!(phase, Phase::Done | Phase::Failed | Phase::Unknown) || j.started_at.is_some(),
        "job_cancelled: start não confirmado"
    );
    let ended_before_start = j.started_at.is_none() && matches!(j.state, Phase::Cancelled | Phase::Expired);
    ensure!(
        !ended_before_start || matches!(phase, Phase::Cancelled | Phase::Expired | Phase::Refused | Phase::Denied),
        "job_cancelled: trabalho encerrado antes de start"
    );
    if j.state == Phase::Unknown {
        j.unknown_reported = true;
    }
    ensure!(
        !matches!(phase, Phase::Denied | Phase::Expired | Phase::Refused) || j.started_at.is_none(),
        "invalid_arguments: trabalho iniciado exige resultado de execução"
    );
    if phase == Phase::Unknown {
        j.unknown_reported = true;
    }
    if ended_before_start {
        // A delayed denial still forbids another window in this turn. It can
        // tighten a terminal state, never reopen the start barrier.
        if phase == Phase::Denied {
            j.state = Phase::Denied;
            j.outcome = Some("denied".into());
            j.error_code = Some("denied".into());
        } else {
            j.outcome = Some(if j.state == Phase::Expired { "expired".into() } else { "cancelled".into() });
        }
    } else {
        j.state = phase;
        j.outcome = Some(if phase == Phase::Unknown { "unknown_outcome".into() } else { outcome.into() });
        j.error_code = b["code"].as_str().map(str::to_owned);
    }
    j.finished_at = Some(now_ms());
    j.cancel_requested = false;
    // Store only the result contract, never reflect its lease token through the MCP.
    let mut clean = b.clone();
    if let Some(map) = clean.as_object_mut() {
        map.remove("leaseToken");
        map.remove("jobId");
    }
    j.result = Some(clean);
    if j.kind == "fs.read" || j.kind == "fs.list" || (j.kind == "program" && j.spec["program"] == "spark") {
        hub.computers.mark_external(&j.bot_id);
    }
    jobs::save(hub, &j)?;
    Ok(json!({"ack":true}))
}

/// Additional fork permissions; None defers unchanged upstream method policy.
pub fn permit(caller: &Caller, method: &str) -> Option<bool> {
    match caller {
        Caller::Local if super::EXECUTOR_METHODS.contains(&method) => Some(false),
        Caller::Device { scopes, .. } if super::EXECUTOR_METHODS.contains(&method) => {
            Some(scopes == &[Scope::Executor])
        }
        Caller::Device { scopes, .. } if scopes.contains(&Scope::Executor) => Some(false),
        Caller::Device { .. } if super::LOCAL_METHODS.contains(&method) => Some(false),
        _ => None,
    }
}
