use super::{APPROVAL_BUDGET, EXEC_MAX, GRACE, LONG_APPROVAL, TOOL_WAIT, registry};
use crate::LockExt;
use crate::hub::Hub;
use crate::remote::crypto;
use crate::store::{EntryKind, Lane, now_ms};
use anyhow::{Result, anyhow, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Queued,
    Delivered,
    Accepted,
    AwaitingApproval,
    Running,
    Done,
    Failed,
    Cancelled,
    Denied,
    Expired,
    Refused,
    Unknown,
}
impl Phase {
    pub fn before_start(self) -> bool {
        matches!(self, Self::Queued | Self::Delivered | Self::Accepted | Self::AwaitingApproval)
    }
    pub fn active(self) -> bool {
        self.before_start() || self == Self::Running
    }
}
#[derive(Clone, Debug)]
pub struct Job {
    pub id: String,
    pub computer_id: String,
    pub bot_id: String,
    pub turn_message_id: String,
    pub kind: String,
    pub spec: Value,
    pub idem_key: String,
    pub state: Phase,
    pub lease_token: Option<String>,
    pub lease_until: Option<i64>,
    pub created_at: i64,
    pub start_by: i64,
    pub deadline_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub outcome: Option<String>,
    pub error_code: Option<String>,
    pub result: Option<Value>,
    pub origin_kind: String,
    pub routine_id: Option<String>,
    pub run_id: Option<String>,
    pub scheduled_at: Option<i64>,
    pub origin: Value,
    pub exec_timeout_ms: i64,
    pub notice_id: Option<String>,
    pub cancel_requested: bool,
    pub unknown_reported: bool,
}
/// D84 contract only. Registration requests are not executable queue jobs (spec §9/§10.4).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineSpec {
    pub routine_id: String,
    pub name: String,
    pub bot_id: String,
    pub bot_name: String,
    pub computer_id: String,
    pub triggers: Value,
    pub steps: Value,
    pub digest: String,
    pub confirmed_at: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineOrigin {
    pub id: String,
    pub run_id: String,
    pub name: String,
    pub scheduled_at: i64,
    pub trigger: RoutineTrigger,
    pub approval: RoutineApproval,
    pub digest: String,
    pub if_unavailable: IfUnavailable,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutineTrigger {
    Schedule,
    Test,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutineApproval {
    Preapproved,
    EachRun,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IfUnavailable {
    Skip,
    Continue,
}

pub fn find(hub: &Hub, id: &str) -> Result<Job> {
    hub.store
        .computer_jobs()?
        .into_iter()
        .find(|j| j.id == id)
        .ok_or_else(|| anyhow!("unknown_job: trabalho desconhecido"))
}
pub fn origin_alive(hub: &Hub, j: &Job) -> bool {
    let binding_live = j.origin["sessionBinding"]
        .as_str()
        .is_none_or(|binding| crate::fork::sessoes::bindings::capture(hub, &j.bot_id, binding).is_ok());
    binding_live
        && j.origin_kind == "chat"
        && crate::chat::manage::user_turn(hub, &j.bot_id).is_ok_and(|id| id == j.turn_message_id)
        && hub.runtime(&j.bot_id).lane == Some(Lane::main(&j.bot_id))
        && registry::bots(&hub.store).is_ok_and(|bots| bots.contains(&j.bot_id))
}
pub fn authorize(hub: &Hub, bot: &str) -> Result<String> {
    let row =
        hub.store.bot(bot)?.filter(|r| !r.deleted).ok_or_else(|| anyhow!("not_allowed_here: bot desconhecido"))?;
    ensure!(
        super::offered(&hub.store, &row.config) && hub.runtime(bot).lane == Some(Lane::main(bot)),
        "not_allowed_here: só na conversa principal do dono"
    );
    crate::chat::manage::user_turn(hub, bot)
        .map_err(|_| anyhow!("not_allowed_here: exige turno iniciado pelo dono; rotinas ficam para o PR 1b"))
}
pub fn source(hub: &Hub, message: &str) -> Option<String> {
    hub.store.entry(message)?.data["fromComputer"].as_str().map(str::to_owned)
}

pub struct Request<'a> {
    pub kind: &'a str,
    pub spec: Value,
    pub timeout: i64,
    pub reason: &'a str,
}
#[cfg(test)]
pub fn create(hub: &Hub, bot: &str, message: &str, c: &registry::Computer, request: Request<'_>) -> Result<Job> {
    create_in_execution(hub, bot, message, c, request, None)
}
pub(crate) fn create_in_execution(
    hub: &Hub,
    bot: &str,
    message: &str,
    c: &registry::Computer,
    request: Request<'_>,
    execution: Option<&crate::fork::sessoes::ExecutionRef>,
) -> Result<Job> {
    crate::fork::sessoes::bindings::recheck_optional(hub, execution)?;
    let Request { kind, spec, timeout, reason } = request;
    let _gate = hub.computers.gate.locked();
    crate::fork::atividades::revisao::estado::work_allowed_on(&hub.store.db.locked())?;
    ensure!(authorize(hub, bot)? == message, "not_allowed_here: a conversa seguiu");
    let current = hub
        .store
        .computers()?
        .into_iter()
        .find(|v| v.id == c.id)
        .ok_or_else(|| anyhow!("removed: computador removido"))?;
    let now = now_ms();
    let state = registry::state(hub, &current, now);
    ensure!(
        state == "online",
        "{state}: {} indisponível desde {}; nada foi executado",
        c.label,
        c.state_since.or(c.last_seen_at).unwrap_or(c.enrolled_at)
    );
    // Em disco que não diferencia maiúsculas, `V.xlsx` e `v.xlsx` são o mesmo alvo: a chave de repetição os junta.
    let mut keyed = spec.clone();
    if spec["purpose"] == "spreadsheet" {
        for key in ["path", "folder"] {
            if let Some(text) = spec[key].as_str() {
                keyed[key] = json!(text.to_lowercase());
            }
        }
    }
    let idem =
        crypto::b64(&crypto::sha256(&[serde_json::to_string(&json!([bot, message, c.id, kind, keyed]))?.as_bytes()]));
    let all = hub.store.computer_jobs()?;
    ensure!(
        !all.iter().any(|j| j.bot_id == bot
            && j.turn_message_id == message
            && j.computer_id == c.id
            && j.state == Phase::Denied),
        "denied_before: o dono negou neste computador; não tente de novo sem novo pedido"
    );
    if let Some(j) =
        all.iter().find(|j| j.idem_key == idem && (j.state.active() || j.state == Phase::Unknown || j.unknown_reported))
    {
        bail!("duplicate_pending: trabalho {}; confira computer_job_status antes de repetir", j.id);
    }
    ensure!(
        !all.iter().any(|j| j.computer_id == c.id && j.state.active()),
        "busy: outro trabalho está em andamento em {}",
        c.label
    );
    ensure!((1_000..=EXEC_MAX).contains(&timeout), "invalid_arguments: timeoutSeconds de 1 a 120");
    let local = source(hub, message).as_deref() == Some(c.id.as_str())
        && current.presence["idleSeconds"].as_u64().is_some_and(|idle| idle < 300);
    let budget = if local { APPROVAL_BUDGET } else { LONG_APPROVAL };
    let entry = hub.store.entry(message).ok_or_else(|| anyhow!("not_allowed_here: mensagem removida"))?;
    let permission = hub.store.bot(bot)?.ok_or_else(|| anyhow!("not_allowed_here: bot removido"))?.config.permission;
    let mut j = Job {
        id: format!("cj_{}", ulid()),
        computer_id: c.id.clone(),
        bot_id: bot.into(),
        turn_message_id: message.into(),
        kind: kind.into(),
        spec,
        idem_key: idem,
        state: Phase::Queued,
        lease_token: None,
        lease_until: None,
        created_at: now,
        start_by: now + budget,
        deadline_at: now + budget + timeout + GRACE,
        started_at: None,
        finished_at: None,
        outcome: None,
        error_code: None,
        result: None,
        origin_kind: "chat".into(),
        routine_id: None,
        run_id: None,
        scheduled_at: None,
        origin: json!({"kind":"chat","botId":bot,"botName":hub.bot_name(bot),"botPermission":permission,"turnMessageId":message,"turnAgeMs":now-entry.created_at,"lane":"main","externalContent":hub.computers.external(bot),"reason":reason}),
        exec_timeout_ms: timeout,
        notice_id: None,
        cancel_requested: false,
        unknown_reported: false,
    };
    if let Some(execution) = execution {
        j.origin["sessionBinding"] =
            json!(execution.acp_binding().ok_or_else(|| anyhow!("not_allowed_here: computadores exige origem ACP"))?);
    }
    // Persist before publishing, with the binding checked inside the write transaction.
    hub.store.save_computer_job_in_execution(&j, execution, None)?;
    let summary = j.spec["command"]
        .as_str()
        .or_else(|| j.spec["path"].as_str())
        .or_else(|| j.spec["program"].as_str())
        .unwrap_or(kind)
        .chars()
        .take(200)
        .collect::<String>();
    j.notice_id = hub
        .add_entry(
            &Lane::main(bot),
            EntryKind::Notice,
            entry.turn,
            &json!({"text":format!("Aguardando aprovação no {} · {summary}",c.label),"computerJobId":j.id,"summary":summary}),
        )
        .map(|e| e.id);
    save(hub, &j)?;
    Ok(j)
}
fn ulid() -> String {
    // ULID: 48-bit timestamp + 80 random bits, Crockford base32, no extra dependency.
    const DIGITS: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut bytes = [0; 16];
    bytes[..6].copy_from_slice(&now_ms().to_be_bytes()[2..]);
    bytes[6..].copy_from_slice(&crypto::random::<10>());
    let mut value = u128::from_be_bytes(bytes);
    let mut out = [b'0'; 26];
    for c in out.iter_mut().rev() {
        *c = DIGITS[usize::try_from(value & 31).unwrap_or(0)];
        value >>= 5;
    }
    out.iter().map(|c| char::from(*c)).collect()
}
pub fn save(hub: &Hub, j: &Job) -> Result<()> {
    save_with_computer(hub, j, None)
}
pub(crate) fn save_with_computer(hub: &Hub, j: &Job, computer: Option<&registry::Computer>) -> Result<()> {
    hub.store.save_computer_job_in_execution(j, None, computer)?;
    if let Some(id) = &j.notice_id {
        let text = match j.state {
            Phase::Done => "Permitido · concluído",
            Phase::Denied => "Negado no computador",
            Phase::Expired => "Expirou sem resposta; nada foi executado",
            Phase::Unknown => "Resultado desconhecido; confira antes de repetir",
            Phase::Cancelled => "Cancelado: a conversa seguiu",
            Phase::Running => "Permitido · executando",
            Phase::Failed | Phase::Refused => "Recusado ou falhou",
            _ => "Aguardando aprovação no computador",
        };
        if let Some(mut e) = hub.store.entry(id) {
            let summary = e.data["summary"].as_str().unwrap_or_default();
            let mut text = format!("{text} · {}", label(hub, &j.computer_id));
            if !summary.is_empty() {
                text.push_str(" · ");
                text.push_str(summary);
            }
            e.data["text"] = json!(text);
            e.data["state"] = json!(j.state);
            hub.set_entry(id, &e.data);
        }
    }
    hub.computers.wake();
    Ok(())
}
fn label(hub: &Hub, id: &str) -> String {
    hub.store.computers().ok().and_then(|cs| cs.into_iter().find(|c| c.id == id)).map_or_else(|| id.into(), |c| c.label)
}
fn finish(j: &mut Job, state: Phase, code: Option<&str>, now: i64) {
    j.state = state;
    j.error_code = code.map(str::to_owned);
    j.finished_at = Some(now);
}
pub(super) fn mark_unknown(j: &mut Job, now: i64) {
    j.state = Phase::Unknown;
    j.finished_at.get_or_insert(now);
    j.unknown_reported = true;
    j.error_code = Some("unknown_outcome".into());
    j.cancel_requested = false;
}
pub fn cancel(hub: &Hub, id: &str) -> Result<()> {
    let _gate = hub.computers.gate.locked();
    let mut j = find(hub, id)?;
    if j.state.active() {
        j.cancel_requested = j.lease_token.is_some();
        if j.state.before_start() {
            finish(&mut j, Phase::Cancelled, Some("job_cancelled"), now_ms());
        }
        save(hub, &j)?;
    }
    Ok(())
}
pub fn recover(hub: &Hub) -> Result<()> {
    let _gate = hub.computers.gate.locked();
    for mut j in hub.store.computer_jobs()? {
        if j.state.before_start() {
            j.cancel_requested = j.lease_token.is_some();
            finish(&mut j, Phase::Cancelled, Some("job_cancelled"), now_ms());
            save(hub, &j)?;
        } else if j.state == Phase::Running {
            mark_unknown(&mut j, now_ms());
            save(hub, &j)?;
        } else if j.state == Phase::Unknown && (j.finished_at.is_none() || j.cancel_requested) {
            // Normalize legacy metadata once, preserving any late result/code
            // and the existing notice: its visible state has not changed.
            j.finished_at.get_or_insert_with(now_ms);
            j.cancel_requested = false;
            hub.store.save_computer_job(&j)?;
        }
    }
    Ok(())
}
pub fn sweep(hub: &Hub, now: i64) -> Result<()> {
    let _gate = hub.computers.gate.locked();
    sweep_locked(hub, now)
}
pub(crate) fn sweep_locked(hub: &Hub, now: i64) -> Result<()> {
    let computers = hub.store.computers()?;
    for mut j in hub.store.computer_jobs()? {
        if !j.state.active() {
            continue;
        }
        let before = j.state;
        let cancellation_before = j.cancel_requested;
        if j.state.before_start() && (!origin_alive(hub, &j) || !computers.iter().any(|c| c.id == j.computer_id)) {
            j.cancel_requested = j.lease_token.is_some();
            finish(&mut j, Phase::Cancelled, Some("job_cancelled"), now);
        } else if j.state.before_start() && now >= j.start_by {
            j.cancel_requested = j.lease_token.is_some();
            finish(&mut j, Phase::Expired, Some("approval_expired"), now);
        } else if j.state == Phase::Queued && j.lease_token.is_none() && now - j.created_at >= 10_000 {
            finish(&mut j, Phase::Expired, Some("offline"), now);
        } else if j.state == Phase::Delivered && j.lease_until.is_none_or(|t| t <= now) {
            j.state = Phase::Queued;
            j.lease_until = None;
        } else if matches!(j.state, Phase::Accepted | Phase::AwaitingApproval) && j.lease_until.is_none_or(|t| t <= now)
        {
            j.cancel_requested = true;
            finish(&mut j, Phase::Expired, Some("lease_lost"), now);
        } else if j.state == Phase::Running && now >= j.deadline_at {
            mark_unknown(&mut j, now);
        }
        if before != j.state || cancellation_before != j.cancel_requested {
            save(hub, &j)?;
        }
    }
    hub.store.clean_computer_jobs(now)?;
    Ok(())
}

pub fn wire(j: &Job, now: i64) -> Result<Value> {
    if j.origin_kind == "routine" {
        let _: RoutineOrigin = serde_json::from_value(j.origin["routine"].clone())?;
    }
    let mut origin = j.origin.clone();
    if let Some(origin) = origin.as_object_mut() {
        origin.remove("sessionBinding");
    }
    Ok(
        json!({"jobId":j.id,"leaseToken":j.lease_token,"kind":j.kind,"approvalMs":(j.start_by-now).max(0),"startMs":(j.start_by-now).max(0),"execTimeoutMs":j.exec_timeout_ms,"origin":origin,"spec":j.spec}),
    )
}
pub fn response(hub: &Hub, j: &Job) -> Value {
    let mut result = j.result.clone().unwrap_or_else(|| json!({}));
    let outcome = j.outcome.as_deref().unwrap_or(match j.state {
        Phase::Unknown => "unknown_outcome",
        Phase::Queued | Phase::Delivered | Phase::Accepted | Phase::AwaitingApproval => "awaiting_approval",
        Phase::Running => "running",
        Phase::Cancelled => "cancelled",
        Phase::Expired => "expired",
        Phase::Denied => "denied",
        Phase::Refused => "refused",
        Phase::Failed => "failed",
        Phase::Done => "done",
    });
    result["computer"] = json!({"id":j.computer_id,"name":label(hub,&j.computer_id)});
    result["jobId"] = json!(j.id);
    result["outcome"] = json!(outcome);
    result["state"] = json!(j.state);
    result["decidedBy"] = json!(match result["decidedBy"].as_str() {
        Some("click") => "janela",
        Some("routine") => "rotina aprovada",
        Some("policy") if j.kind == "program" => "programa liberado",
        Some("policy") => "pasta liberada",
        _ => "nenhuma",
    });
    if let Some(code) = &j.error_code {
        result["code"] = json!(code);
    }
    if j.state.before_start() {
        result["approveBy"] = json!(j.start_by);
        result["message"] =
            json!("Aguardando aprovação no computador indicado; chame computer_job_status com waitSeconds=240");
    }
    if j.state == Phase::Unknown {
        result["message"] = json!(
            "Resultado desconhecido: o comando pode ter rodado. Confira o estado antes de repetir; use computer_job_status."
        );
    }
    if j.state == Phase::Running {
        result["message"] = json!(
            "O comando está rodando neste computador; consulte computer_job_status com waitSeconds=240 até receber o resultado."
        );
    }
    result
}
pub async fn wait(hub: &Hub, id: &str, wait_ms: u64) -> Result<Value> {
    let end = tokio::time::Instant::now() + Duration::from_millis(wait_ms.min(TOOL_WAIT));
    let mut changed = hub.computers.watch();
    loop {
        sweep(hub, now_ms())?;
        let j = find(hub, id)?;
        if !j.state.active() || tokio::time::Instant::now() >= end {
            return Ok(response(hub, &j));
        }
        let _ =
            tokio::time::timeout_at(end.min(tokio::time::Instant::now() + Duration::from_secs(1)), changed.changed())
                .await;
    }
}
pub struct CancelOnDrop {
    hub: Arc<Hub>,
    id: String,
    armed: bool,
}
impl CancelOnDrop {
    pub fn new(hub: Arc<Hub>, id: String) -> Self {
        Self { hub, id, armed: true }
    }
    pub fn disarm(&mut self) {
        self.armed = false;
    }
}
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if self.armed
            && let Err(error) = cancel(&self.hub, &self.id)
        {
            tracing::warn!(%error,"computer call cancellation failed");
        }
    }
}
