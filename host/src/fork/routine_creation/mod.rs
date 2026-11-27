//! D544/D547: criação própria, cota durável e revisão informativa pelo dono.
//! A rotina nasce ativa; o cartão não é uma aprovação nem uma fila de execução.
use crate::{
    LockExt,
    api::devices::Caller,
    fork::{control_tower::Observation, sessoes::ExecutionRef},
    hub::Hub,
    routines::Routine,
    store::{BotConfig, Entry, EntryKind, Lane, entry_on, insert_visible_on, now_ms},
};
use anyhow::{Context, Result, bail, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) mod external_grant;

pub(crate) const POLICY: &str = "donnaRoutineCreationPolicy";
pub(crate) const SAVE: &str = "donnaRoutineCreationPolicySave";
pub(crate) const REVIEW: &str = "donnaRoutineCreationReview";
pub(crate) const DEFAULT_LIMIT: u32 = 10;
const WINDOW: i64 = 86_400_000;
const MAX_LIMIT: u32 = 1000;

pub(crate) fn handles(method: &str) -> bool {
    matches!(method, POLICY | SAVE | REVIEW)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Policy {
    version: u8,
    routine_creation_limit_24h: u32,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Ledger {
    timestamps: Vec<i64>,
    blocked_at: Option<i64>,
}

fn key(domain: &str, space: &str, bot: &str) -> String {
    format!("donna.routine-creation.{domain}.v1.{space}.{bot}")
}
fn read(c: &Connection, key: &str) -> Result<Option<String>> {
    Ok(c.query_row("SELECT v FROM kv WHERE k=?1", [key], |r| r.get(0)).optional()?)
}
fn write(c: &Connection, key: &str, value: &impl Serialize) -> Result<()> {
    c.execute(
        "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
        params![key, serde_json::to_string(value)?],
    )?;
    Ok(())
}
fn policy(c: &Connection, space: &str, bot: &str) -> Result<Policy> {
    let value = read(c, &key("policy", space, bot))?
        .map(|raw| serde_json::from_str::<Policy>(&raw))
        .transpose()?
        .unwrap_or(Policy { version: 1, routine_creation_limit_24h: DEFAULT_LIMIT });
    ensure!(value.version == 1 && value.routine_creation_limit_24h <= MAX_LIMIT, "routine_creation_policy_unavailable");
    Ok(value)
}
fn ledger(c: &Connection, space: &str, bot: &str, now: i64) -> Result<Ledger> {
    let mut value = read(c, &key("quota", space, bot))?
        .map(|raw| serde_json::from_str::<Ledger>(&raw))
        .transpose()?
        .unwrap_or_default();
    ensure!(value.timestamps.iter().all(|at| *at >= 0), "routine_creation_quota_unavailable");
    // Relógio voltando não devolve cota: timestamps futuros continuam contando.
    value.timestamps.retain(|at| *at > now.saturating_sub(WINDOW));
    Ok(value)
}

/// MX-106/D125: a criação veio de um contexto marcado como externo. A decisão é do Hub, nunca do texto
/// do modelo: sessão visível e ACP da execução, lidas na mesma transação da gravação.
fn external_context(c: &Connection, bot: &str, execution: &ExecutionRef) -> Result<bool> {
    let session = crate::fork::sessoes::bindings::assert_execution_live(c, execution)?;
    let binding = crate::fork::sessoes::bindings::resolve(c, execution.acp_binding().context("chamada vencida")?, bot)?;
    let acp = binding.acp_session.as_deref().unwrap_or_default();
    Ok(crate::fork::sessoes::exposure::context_marked(c, bot, &session.id, acp))
}

pub(crate) fn bot_on(c: &Connection, space: &str, bot: &str) -> Result<BotConfig> {
    let raw: Option<String> =
        c.query_row("SELECT config FROM bots WHERE id=?1 AND deleted=0", [bot], |r| r.get(0)).optional()?;
    let cfg: BotConfig = serde_json::from_str(&raw.context("routine_creation_unavailable")?)?;
    ensure!(cfg.id == bot && cfg.space_id == space && !cfg.is_group(), "routine_creation_unavailable");
    let exists: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM kv,json_each(kv.v,'$.spaces') WHERE kv.k='donna.spaces.v1' AND json_extract(value,'$.id')=?1)", [space], |r| r.get(0))?;
    ensure!(exists, "routine_creation_unavailable");
    Ok(cfg)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Card {
    version: u8,
    pub(crate) space_id: String,
    pub(crate) bot_id: String,
    pub(crate) routine_id: String,
    name: String,
    bot_name: String,
    created_at: i64,
    pub(crate) decision: String,
}

pub(crate) struct Invocation {
    key: String,
    digest: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    digest: String,
    space_id: String,
    bot_id: String,
    routine_id: String,
}
impl Invocation {
    pub(crate) fn new(execution: &ExecutionRef, bot: &str, raw: &str, args: &Value) -> Result<Self> {
        ensure!(
            !raw.is_empty() && raw.len() <= 256 && raw.bytes().all(|b| b.is_ascii_graphic()),
            "routine_creation_invocation_invalid"
        );
        let material = format!(
            "routine-creation-v1\n{}\n{}\n{bot}\n{raw}",
            execution.acp_binding().context("chamada vencida")?,
            execution.turn_token()
        );
        let mut canonical = args.clone();
        canonical.sort_all_objects();
        Ok(Self {
            key: format!(
                "donna.routine-creation.receipt.v1.{}",
                crate::agent::registry::sha256_hex(material.as_bytes())
            ),
            digest: crate::agent::registry::sha256_hex(&serde_json::to_vec(&canonical)?),
        })
    }
}
pub(crate) fn replay_on(
    c: &Connection,
    invocation: Option<&Invocation>,
    space: &str,
    bot: &str,
) -> Result<Option<String>> {
    let Some(invocation) = invocation else { return Ok(None) };
    let Some(raw) = read(c, &invocation.key)? else { return Ok(None) };
    let receipt: Receipt = serde_json::from_str(&raw)?;
    ensure!(
        receipt.digest == invocation.digest && receipt.space_id == space && receipt.bot_id == bot,
        "routine_creation_invocation_conflict"
    );
    Ok(Some(receipt.routine_id))
}

pub(crate) enum Creation {
    Created(Vec<Entry>),
    QuotaExceeded,
}
/// Chamado na mesma IMMEDIATE do JSON de Rotinas. Qualquer falha desfaz tudo.
pub(crate) fn create_on(
    c: &Connection,
    space: &str,
    routine: &mut Routine,
    execution: &ExecutionRef,
    human_message: Option<&str>,
    invocation: Option<&Invocation>,
    now: i64,
) -> Result<Creation> {
    let cfg = bot_on(c, space, &routine.bot_id)?;
    routine.creation_origin =
        crate::fork::cadeias::descendencia_rotina::capture_creation_on(c, &cfg.id, space, execution, human_message)?;
    // A marca herdada de uma duplicação sobrevive: OR em vez de recalcular.
    routine.external_creation = routine.external_creation || external_context(c, &cfg.id, execution)?;
    routine.external_release_pending = routine.external_creation.then_some(crate::routines::ExternalRelease::Pending);
    if routine.external_creation {
        // MX-106: a liberação é um gesto do dono no app.
        routine.enabled = false;
    }
    record_creation_on(c, space, &cfg, routine, invocation, now)
}

pub(crate) fn record_creation_on(
    c: &Connection,
    space: &str,
    cfg: &BotConfig,
    routine: &Routine,
    invocation: Option<&Invocation>,
    now: i64,
) -> Result<Creation> {
    let policy = policy(c, space, &cfg.id)?;
    let mut quota = ledger(c, space, &cfg.id, now)?;
    if quota.timestamps.len() >= policy.routine_creation_limit_24h as usize {
        quota.blocked_at = Some(now);
        write(c, &key("quota", space, &cfg.id), &quota)?;
        return Ok(Creation::QuotaExceeded);
    }
    quota.timestamps.push(now);
    quota.blocked_at = None;
    write(c, &key("quota", space, &cfg.id), &quota)?;
    let card = Card {
        version: 1,
        space_id: space.into(),
        bot_id: cfg.id.clone(),
        routine_id: routine.id.clone(),
        name: routine.name.clone(),
        bot_name: cfg.name.clone(),
        created_at: now,
        decision: "pending".into(),
    };
    // Aviso ao dono na conversa principal do próprio bot, nunca em outro Space/grupo.
    let lane = Lane::main(&cfg.id);
    let when = crate::routines::creation_schedule(routine);
    let state = if routine.external_creation {
        "Ela nasceu pausada porque a criação veio de conteúdo de fora e está pendente da sua liberação: nada roda até você ligar no Mission Board ou em Rotinas."
    } else if routine.enabled {
        "Ela está ativa."
    } else {
        "Ela está pausada."
    };
    let action = if crate::environment::Environment::is_open() {
        "Para mudar o agendamento, abra Rotinas."
    } else if routine.external_creation {
        "Você pode ligar ou cancelar no Mission Board."
    } else {
        "Você pode manter ou cancelar no Mission Board."
    };
    let data = json!({"text":format!("{} criou a rotina “{}”. {}. {state} {action}", cfg.name, routine.name, when),"style":"info","routineId":routine.id,"routineCreation":card});
    let (marker, entry) =
        insert_visible_on(c, &format!("routine-created:{}", routine.id), &lane, EntryKind::Notice, 0, &data)?;
    if let Some(invocation) = invocation {
        write(
            c,
            &invocation.key,
            &Receipt {
                digest: invocation.digest.clone(),
                space_id: space.into(),
                bot_id: cfg.id.clone(),
                routine_id: routine.id.clone(),
            },
        )?;
    }
    Ok(Creation::Created(marker.into_iter().chain([entry]).collect()))
}

pub(crate) fn quota_error() -> anyhow::Error {
    anyhow::anyhow!(
        "routine_creation_limit: limite de criações deste bot em 24 horas atingido. Nenhuma rotina foi criada. O dono pode criar pelo app ou ajustar o limite em Trabalho que ninguém pediu."
    )
}
pub(crate) fn changed(hub: &Hub, space: &str) {
    let _ = hub.events.send(json!({"type":crate::fork::control_tower::EVENT,"spaceId":space,"rosterChanged":true}));
}
pub(crate) fn publish(hub: &Hub, mut entries: Vec<Entry>) {
    entries.sort_by_key(|e| e.rev);
    for entry in entries {
        let _ = hub.events.send(json!({"type":"entry","rev":entry.rev,"entry":entry}));
    }
}

pub(crate) fn card_on(c: &Connection, space: &str, entry_id: &str) -> Result<(Entry, Card)> {
    let entry = entry_on(c, entry_id)?.context("routine_review_unavailable")?;
    ensure!(entry.kind == EntryKind::Notice.as_str(), "routine_review_unavailable");
    let card: Card = serde_json::from_value(entry.data["routineCreation"].clone())
        .map_err(|_| anyhow::anyhow!("routine_review_unavailable"))?;
    ensure!(
        card.version == 1
            && card.space_id == space
            && entry.bot_id == card.bot_id
            && entry.session_id == card.bot_id
            && entry.id == format!("routine-created:{}", card.routine_id),
        "routine_review_unavailable"
    );
    bot_on(c, space, &card.bot_id)?;
    Ok((entry, card))
}
pub(crate) fn settle_on(c: &Connection, entry: &Entry, mut card: Card, choice: &str) -> Result<Entry> {
    card.decision = choice.into();
    let mut data = entry.data.clone();
    data["routineCreation"] = serde_json::to_value(&card)?;
    data["text"] = format!(
        "{} criou a rotina “{}”. {} pelo dono.",
        card.bot_name,
        card.name,
        match choice {
            "kept" => "Mantida",
            "enabled" => "Ligada",
            "external_granted" => "Concessão para evento registrada",
            _ => "Cancelada",
        }
    )
    .into();
    let rev = crate::store::next_rev(c)?;
    ensure!(
        c.execute(
            "UPDATE entries SET data=?2,rev=?3,updated_at=?4 WHERE id=?1 AND rev=?5",
            params![entry.id, data.to_string(), rev, now_ms(), entry.rev]
        )? == 1,
        "routine_review_conflict"
    );
    entry_on(c, &entry.id)?.context("routine_review_unavailable")
}

pub(crate) fn touch_review(c: &Connection, entry: &Entry) -> Result<Entry> {
    let rev = crate::store::next_rev(c)?;
    ensure!(
        c.execute(
            "UPDATE entries SET rev=?2,updated_at=?3 WHERE id=?1 AND rev=?4",
            params![entry.id, rev, now_ms(), entry.rev]
        )? == 1,
        "routine_review_conflict"
    );
    entry_on(c, &entry.id)?.context("routine_review_unavailable")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Review {
    entry_id: String,
    entry_rev: i64,
    routine_id: String,
    bot_id: String,
    bot_name: String,
    name: String,
    created_at: i64,
    when: String,
    next_run_at: Option<i64>,
    status: &'static str,
    external_release_pending: bool,
    routine_revision: String,
}

pub(crate) fn routine_revision(routine: &Routine) -> Result<String> {
    Ok(crate::agent::registry::sha256_hex(&serde_json::to_vec(routine)?))
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QuotaBlock {
    bot_id: String,
    bot_name: String,
    limit: u32,
    used: usize,
    retry_at: i64,
    reason: String,
    how_to_increase: &'static str,
    settings_available: bool,
}
#[derive(Serialize)]
pub(crate) struct Reviews {
    items: Vec<Review>,
    blocked: Vec<QuotaBlock>,
}

fn routine_on(c: &Connection, bot: &str, id: &str) -> Result<Option<Routine>> {
    let raw: Option<String> = c.query_row("SELECT value FROM kv,json_each(kv.v,'$.routines') WHERE kv.k='routines.v1' AND json_extract(value,'$.id')=?1 AND json_extract(value,'$.botId')=?2 AND json_extract(value,'$.deleted')=0", params![id,bot], |r| r.get(0)).optional()?;
    raw.map(|raw| serde_json::from_str(&raw).map_err(Into::into)).transpose()
}
pub(crate) fn projection(c: &Connection, space: &str, now: i64) -> Result<Observation<Reviews>> {
    let mut items = Vec::new();
    let ids = c.prepare("SELECT e.id FROM entries e JOIN bots b ON b.id=json_extract(e.data,'$.routineCreation.botId') WHERE e.kind='notice' AND json_extract(e.data,'$.routineCreation.version')=1 AND json_extract(e.data,'$.routineCreation.spaceId')=?1 AND json_extract(e.data,'$.routineCreation.decision')='pending' AND b.deleted=0 AND json_extract(b.config,'$.spaceId')=?1 ORDER BY e.created_at DESC,e.id")?.query_map([space], |r| r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let paused: bool =
        c.query_row("SELECT COALESCE((SELECT json_extract(v,'$.paused') FROM kv WHERE k='routines.v1'),0)", [], |r| {
            r.get(0)
        })?;
    for id in ids {
        let (entry, card) = card_on(c, space, &id)?;
        let Some(routine) = routine_on(c, &card.bot_id, &card.routine_id)? else { continue };
        let cfg = bot_on(c, space, &card.bot_id)?;
        items.push(Review {
            entry_id: entry.id,
            entry_rev: entry.rev,
            routine_id: card.routine_id,
            bot_id: cfg.id,
            bot_name: cfg.name,
            name: routine.name.clone(),
            created_at: card.created_at,
            when: crate::routines::creation_schedule(&routine),
            next_run_at: if routine.enabled { routine.next_runs.iter().flatten().min().copied() } else { None },
            status: if routine.enabled && !paused { "active" } else { "paused" },
            external_release_pending: routine.external_release_pending.is_some()
                && routine.external_creation
                && !routine.enabled,
            routine_revision: routine_revision(&routine)?,
        });
    }
    let bots = c.prepare("SELECT id FROM bots WHERE deleted=0 AND COALESCE(json_extract(config,'$.kind'),'agent')='agent' AND json_extract(config,'$.spaceId')=?1 ORDER BY id")?.query_map([space], |r| r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut blocked = Vec::new();
    for bot in bots {
        let quota = ledger(c, space, &bot, now)?;
        let limit = policy(c, space, &bot)?.routine_creation_limit_24h;
        if quota.blocked_at.is_none() || quota.timestamps.len() < limit as usize {
            continue;
        }
        let cfg = bot_on(c, space, &bot)?;
        let mut times = quota.timestamps.clone();
        times.sort_unstable();
        let retry_at = if limit == 0 {
            0
        } else {
            times.get(times.len().saturating_sub(limit as usize)).copied().unwrap_or(now).saturating_add(WINDOW)
        };
        blocked.push(QuotaBlock { bot_id: cfg.id, bot_name: cfg.name, limit, used: quota.timestamps.len(),
            retry_at,
            reason: format!("Este bot atingiu o limite de {limit} criações pelo bot em 24 horas. Nenhuma rotina nova foi criada."),
            how_to_increase: "O dono pode aumentar Criações de rotinas em 24 horas no cartão Trabalho que ninguém pediu (D547). Este app ainda não oferece esse ajuste. Criar uma rotina diretamente pelo app não gasta esta cota.",
            settings_available: false });
    }
    Ok(Observation::Available {
        source: "routineCreation",
        coverage: "ownBotsInSpacePersistentCreationReviewsAndQuota",
        value: Reviews { items, blocked },
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PolicyRead {
    space_id: String,
    bot_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PolicySave {
    space_id: String,
    bot_id: String,
    routine_creation_limit_24h: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviewRequest {
    space_id: String,
    entry_id: String,
    entry_rev: i64,
    choice: String,
    routine_revision: Option<String>,
    external_grant: Option<external_grant::Contract>,
}

#[derive(Clone, Copy)]
pub(crate) struct ReviewProof<'a> {
    pub(crate) routine_revision: Option<&'a str>,
    pub(crate) external_grant: Option<&'a external_grant::Contract>,
}

pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    crate::fork::control_tower::require(hub, caller)?;
    let _guard = hub.store.team_lock.lock().await;
    crate::fork::control_tower::require(hub, caller)?;
    match method {
        POLICY => {
            let request: PolicyRead =
                serde_json::from_value(body).context("invalid_field: consulta do limite inválida")?;
            let db = hub.store.db.locked();
            bot_on(&db, &request.space_id, &request.bot_id)?;
            let policy = policy(&db, &request.space_id, &request.bot_id)?;
            let quota = ledger(&db, &request.space_id, &request.bot_id, now_ms())?;
            let result = json!({"spaceId":request.space_id,"botId":request.bot_id,"routineCreationLimit24h":policy.routine_creation_limit_24h,"used24h":quota.timestamps.len(),"settingsAvailable":false});
            drop(db);
            crate::fork::control_tower::require(hub, caller)?;
            Ok(result)
        }
        SAVE => {
            let request: PolicySave = serde_json::from_value(body).context("invalid_field: limite inválido")?;
            ensure!(request.routine_creation_limit_24h <= MAX_LIMIT, "invalid_field: limite deve estar entre 0 e 1000");
            crate::fork::donna_fixa::pedido::gravar(&hub.store, |c| {
                bot_on(c, &request.space_id, &request.bot_id)?;
                write(
                    c,
                    &key("policy", &request.space_id, &request.bot_id),
                    &Policy { version: 1, routine_creation_limit_24h: request.routine_creation_limit_24h },
                )
            })?;
            changed(hub, &request.space_id);
            Ok(
                json!({"spaceId":request.space_id,"botId":request.bot_id,"routineCreationLimit24h":request.routine_creation_limit_24h}),
            )
        }
        REVIEW => {
            let request: ReviewRequest = serde_json::from_value(body).context("invalid_field: revisão inválida")?;
            let decision = match request.choice.as_str() {
                "keep" => "kept",
                "cancel" => "cancelled",
                "enable" => "enabled",
                "grantExternal" => "external_granted",
                "revokeExternal" => "external_revoked",
                _ => bail!("invalid_field: escolha inválida"),
            };
            // Local é origem de transporte, não prova de gesto humano.
            if matches!(decision, "external_granted" | "external_revoked") {
                ensure!(matches!(caller, Caller::Local), "routine_external_owner_required");
            }
            hub.routines.review_creation(
                hub,
                &request.space_id,
                &request.entry_id,
                request.entry_rev,
                decision,
                ReviewProof {
                    routine_revision: request.routine_revision.as_deref(),
                    external_grant: request.external_grant.as_ref(),
                },
            )
        }
        _ => bail!("unknown method"),
    }
}

#[cfg(test)]
mod tests;
