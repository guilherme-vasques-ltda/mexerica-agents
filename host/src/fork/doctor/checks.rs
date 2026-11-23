//! Coleta passiva e somente leitura dos sinais de saúde do Hub e do Space.
use super::model::{
    DoctorActionKind, DoctorArea, DoctorCheck, DoctorCheckStatus, DoctorOverallStatus, HubDoctorReport,
};
use crate::{
    LockExt,
    fork::{
        engine_accounts::{Billing, LaunchReference, Registry, State as AccountState, managed},
        fallback_contas::estado::Estado as FallbackState,
        projetos::model::{Project, ProjectState, TaskState},
        retornos::pendentes::{Filtro, JANELA_MS, Pedido, ler_na},
        spaces::PERSONAL,
    },
    hub::Hub,
    store::{Store, now_ms},
};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use std::collections::BTreeSet;

pub(super) fn collect(hub: &Hub, space: &str) -> Result<HubDoctorReport> {
    let now = now_ms();
    let (ai_engine, approvals, channels, routines, delegation) = {
        let db = hub.store.db.locked();
        let tx = db.unchecked_transaction()?;
        crate::fork::spaces::require_space(&tx, space)?;
        let roster = roster_ids(&tx, space)?;
        let ai_engine = check_ai_engine(&tx, space, now);
        let approvals = check_approvals(&tx, &hub.host_id, space, &roster);
        let channels = check_channels(&tx, &hub.host_id, space, &roster);
        let routines = check_routines(&tx, &roster);
        let delegation = check_delegation(&tx, space, &roster, now);
        tx.commit()?;
        (ai_engine, approvals, channels, routines, delegation)
    };
    let mcps = check_mcps(&hub.store, space);
    let checks = vec![
        DoctorCheck {
            id: "hub".into(),
            area: DoctorArea::Hub,
            status: DoctorCheckStatus::Ok,
            code: "hub_ok".into(),
            count: None,
            action_kind: DoctorActionKind::None,
        },
        ai_engine,
        approvals,
        channels,
        mcps,
        routines,
        delegation,
    ];
    let overall = compute_overall(&checks);
    Ok(HubDoctorReport {
        version: 1,
        hub_id: hub.host_id.clone(),
        space_id: space.to_owned(),
        collected_at: now,
        overall,
        checks,
    })
}

pub(super) fn compute_overall(checks: &[DoctorCheck]) -> DoctorOverallStatus {
    if checks.iter().any(|c| c.status == DoctorCheckStatus::Error) {
        return DoctorOverallStatus::Error;
    }
    let waiting = |c: &DoctorCheck| {
        c.area == DoctorArea::Approvals && c.status == DoctorCheckStatus::Warning && c.code == "approvals_waiting"
    };
    if checks.iter().any(|c| c.status != DoctorCheckStatus::Ok && !waiting(c)) {
        return DoctorOverallStatus::Degraded;
    }
    if checks.iter().any(waiting) {
        return DoctorOverallStatus::WaitingUser;
    }
    DoctorOverallStatus::Healthy
}

fn roster_ids(c: &Connection, space: &str) -> Result<Vec<String>> {
    let mut st = c.prepare(
        "SELECT b.id FROM bots b WHERE b.deleted=0
         AND COALESCE(json_extract(b.config,'$.kind'),'agent')='agent'
         AND (json_extract(b.config,'$.spaceId')=?1 OR
           (b.id=(SELECT json_extract(v,'$.primaryBotId') FROM kv WHERE k='team.settings.v1')
            AND EXISTS(SELECT 1 FROM donna_space_conversations c WHERE c.space_id=?1 AND c.bot_id=b.id)))
         ORDER BY b.id",
    )?;
    Ok(st.query_map([space], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
}

fn check_ai_engine(c: &Connection, space: &str, now: i64) -> DoctorCheck {
    match eval_ai_engine(c, space, now) {
        Ok(check) => check,
        Err(_) => DoctorCheck {
            id: "ai_engine".into(),
            area: DoctorArea::AiEngine,
            status: DoctorCheckStatus::Unavailable,
            code: "ai_unavailable".into(),
            count: None,
            action_kind: DoctorActionKind::ConnectAi,
        },
    }
}

fn eval_ai_engine(c: &Connection, space: &str, now: i64) -> Result<DoctorCheck> {
    let registry = Registry::load_db(c)?;
    let fallback = FallbackState::carregar(c)?;

    let relevant: Vec<_> = if space == PERSONAL {
        registry.accounts.iter().collect()
    } else {
        let mut linked = Vec::new();
        for account in &registry.accounts {
            if managed::linked_to_space(c, &account.id, space)? {
                linked.push(account);
            }
        }
        linked
    };

    let mut ready_count: u64 = 0;
    let mut connected_count: u64 = 0;
    let mut paused_connected_count: u64 = 0;
    let mut connected_unpaused_count: u64 = 0;
    let mut rate_limited_count: u64 = 0;

    for account in relevant {
        let enabled = account.managed.as_ref().is_none_or(|m| m.enabled);
        let paused = account.managed.as_ref().is_some_and(|m| m.paused);
        let connected = enabled
            && account.state == AccountState::Connected
            && (account.billing != Billing::Subscription || account.subscription_verified());
        if connected && let LaunchReference::VaultProfile { slot } = &account.launch {
            let present: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM kv WHERE k=?1)", [slot], |r| r.get(0))?;
            if !present {
                continue;
            }
        }
        let rate_limited = fallback.descansando(&account.id, now).is_some();

        if connected {
            connected_count += 1;
            if paused {
                paused_connected_count += 1;
            } else {
                connected_unpaused_count += 1;
                if rate_limited {
                    rate_limited_count += 1;
                } else {
                    ready_count += 1;
                }
            }
        }
    }

    if ready_count > 0 {
        return Ok(DoctorCheck {
            id: "ai_engine".into(),
            area: DoctorArea::AiEngine,
            status: DoctorCheckStatus::Ok,
            code: "ai_ok".into(),
            count: Some(ready_count),
            action_kind: DoctorActionKind::None,
        });
    }
    if connected_count == 0 {
        return Ok(DoctorCheck {
            id: "ai_engine".into(),
            area: DoctorArea::AiEngine,
            status: DoctorCheckStatus::Error,
            code: "ai_no_active_account".into(),
            count: Some(0),
            action_kind: DoctorActionKind::ConnectAi,
        });
    }
    if connected_unpaused_count == 0 {
        return Ok(DoctorCheck {
            id: "ai_engine".into(),
            area: DoctorArea::AiEngine,
            status: DoctorCheckStatus::Error,
            code: "ai_all_paused".into(),
            count: Some(paused_connected_count),
            action_kind: DoctorActionKind::ConnectAi,
        });
    }
    Ok(DoctorCheck {
        id: "ai_engine".into(),
        area: DoctorArea::AiEngine,
        status: DoctorCheckStatus::Warning,
        code: "ai_rate_limited".into(),
        count: Some(rate_limited_count),
        action_kind: DoctorActionKind::ConnectAi,
    })
}

fn check_approvals(c: &Connection, hub_id: &str, space: &str, roster: &[String]) -> DoctorCheck {
    match eval_approvals(c, hub_id, space, roster) {
        Ok(check) => check,
        Err(_) => DoctorCheck {
            id: "approvals".into(),
            area: DoctorArea::Approvals,
            status: DoctorCheckStatus::Unavailable,
            code: "approvals_unavailable".into(),
            count: None,
            action_kind: DoctorActionKind::ReviewWaiting,
        },
    }
}

fn eval_approvals(c: &Connection, hub_id: &str, space: &str, roster: &[String]) -> Result<DoctorCheck> {
    let roster_json = serde_json::to_string(roster)?;
    let mut entry_ids: BTreeSet<String> = BTreeSet::new();

    // 1. Cartões de permissão direta pendentes em sessões abertas do catálogo neste Space.
    let mut stmt = c.prepare(
        "SELECT e.id FROM entries e
         JOIN donna_space_conversations c ON c.id=e.bot_id AND c.space_id=?1 AND c.closed=0
         JOIN donna_bot_sessions s ON s.id=COALESCE(e.session,e.bot_id) AND s.chat_id=c.id AND s.state='open'
         WHERE e.kind='permission' AND json_valid(e.data) AND json_extract(e.data,'$.status')='pending'
           AND EXISTS(SELECT 1 FROM json_each(?2) WHERE value=c.bot_id)",
    )?;
    for id in stmt.query_map(params![space, roster_json], |r| r.get::<_, String>(0))? {
        entry_ids.insert(id?);
    }

    // 2. Pausas de permissão delegada (deduplicadas pelo mesmo entry_id/card_id).
    let mut deleg_stmt = c.prepare(
        "SELECT p.card_id
         FROM donna_delegated_permission_checkpoints p
         JOIN donna_session_requests r ON r.id=p.request_id
         JOIN entries e ON e.id=p.card_id
         JOIN donna_space_conversations c ON c.id=e.bot_id AND c.bot_id=r.target_bot_id
         JOIN donna_bot_sessions s ON s.id=COALESCE(e.session,e.bot_id) AND s.chat_id=c.id
         JOIN bots source ON source.id=r.requester_bot_id AND source.deleted=0
         JOIN bots target ON target.id=r.target_bot_id AND target.deleted=0
         WHERE p.space_id=?1 AND c.space_id=?1 AND c.closed=0 AND s.state='open' AND s.principal=1 AND s.id=c.id
           AND r.schema_version=1 AND r.kind='inter_bot' AND r.state='running' AND r.end_reason IS NULL
           AND NOT EXISTS(SELECT 1 FROM donna_session_results result WHERE result.request_id=r.id)
           AND r.requester_bot_id<>r.target_bot_id AND r.mode IN ('wait','async') AND r.attempt=1 AND p.attempt=r.attempt AND p.fence=r.fence
           AND r.target_chat_id=c.id AND r.target_session_id=s.id AND e.kind='permission'
           AND e.rev>=p.card_revision AND p.card_revision>0 AND p.created_at>=0
           AND CASE WHEN json_valid(e.data) THEN json_extract(e.data,'$.status')='pending'
                AND json_extract(e.data,'$.author')=r.target_bot_id ELSE 0 END
           AND EXISTS(SELECT 1 FROM json_each(?2) WHERE value=r.requester_bot_id)
           AND EXISTS(SELECT 1 FROM json_each(?2) WHERE value=r.target_bot_id)",
    )?;
    for id in deleg_stmt.query_map(params![space, roster_json], |r| r.get::<_, String>(0))? {
        entry_ids.insert(id?);
    }

    // 3. Perguntas ask_owner pendentes em sessões abertas autorizadas neste Space.
    let mut q_stmt = c.prepare(&format!(
        "SELECT e.id FROM entries e
         JOIN donna_space_conversations c ON c.id=e.bot_id AND c.space_id=?1 AND c.closed=0
         JOIN donna_bot_sessions s ON s.id=COALESCE(e.session,e.bot_id) AND s.chat_id=c.id AND s.state='open'
         WHERE {} AND json_valid(e.data)
           AND EXISTS(SELECT 1 FROM json_each(?2) WHERE value=c.bot_id)",
        crate::fork::owner_question::PENDING_E
    ))?;
    for id in q_stmt.query_map(params![space, roster_json], |r| r.get::<_, String>(0))? {
        entry_ids.insert(id?);
    }

    // 4. Propostas aguardando decisão do dono.
    let proposals_count: i64 = c.query_row(
        "SELECT COUNT(*) FROM donna_task_proposals WHERE hub_id=?1 AND space_id=?2 AND state='pending'",
        params![hub_id, space],
        |r| r.get(0),
    )?;
    ensure!(proposals_count >= 0, "invalid_proposals_count");

    // 5. Tarefas em conferência (review) em projetos ativos deste Space.
    let mut review_count: u64 = 0;
    let mut proj_stmt =
        c.prepare("SELECT project, tasks FROM donna_projects WHERE hub_id=?1 AND space_id=?2 ORDER BY id")?;
    for row in proj_stmt.query_map(params![hub_id, space], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (proj_raw, tasks_raw) = row?;
        let project: Project = serde_json::from_str(&proj_raw)?;
        if project.state != ProjectState::Active {
            continue;
        }
        let tasks: Vec<crate::fork::projetos::model::Task> = serde_json::from_str(&tasks_raw)?;
        for task in tasks {
            if task.state == TaskState::Review {
                review_count += 1;
            }
        }
    }

    let total = (entry_ids.len() as u64)
        .saturating_add(u64::try_from(proposals_count).unwrap_or(0))
        .saturating_add(review_count);

    if total > 0 {
        Ok(DoctorCheck {
            id: "approvals".into(),
            area: DoctorArea::Approvals,
            status: DoctorCheckStatus::Warning,
            code: "approvals_waiting".into(),
            count: Some(total),
            action_kind: DoctorActionKind::ReviewWaiting,
        })
    } else {
        Ok(DoctorCheck {
            id: "approvals".into(),
            area: DoctorArea::Approvals,
            status: DoctorCheckStatus::Ok,
            code: "approvals_ok".into(),
            count: Some(0),
            action_kind: DoctorActionKind::None,
        })
    }
}

fn check_channels(c: &Connection, hub_id: &str, space: &str, roster: &[String]) -> DoctorCheck {
    match eval_channels(c, hub_id, space, roster) {
        Ok(check) => check,
        Err(_) => DoctorCheck {
            id: "channels".into(),
            area: DoctorArea::Channels,
            status: DoctorCheckStatus::Unavailable,
            code: "channels_unavailable".into(),
            count: None,
            action_kind: DoctorActionKind::CheckChannels,
        },
    }
}

fn eval_channels(c: &Connection, hub_id: &str, space: &str, roster: &[String]) -> Result<DoctorCheck> {
    let plugin_state: Option<String> = c
        .query_row(
            "SELECT state FROM donna_plugin_installations WHERE hub_id=?1 AND builtin='whatsapp'",
            [hub_id],
            |r| r.get(0),
        )
        .optional()?;
    if plugin_state.as_deref() != Some("enabled") {
        return Ok(DoctorCheck {
            id: "channels".into(),
            area: DoctorArea::Channels,
            status: DoctorCheckStatus::Ok,
            code: "channels_ok".into(),
            count: Some(0),
            action_kind: DoctorActionKind::None,
        });
    }

    let mode: String = c
        .query_row("SELECT mode FROM donna_whatsapp_hub WHERE id=1", [], |r| r.get(0))
        .optional()?
        .unwrap_or_else(|| "donna_only".to_owned());

    let roster_json = serde_json::to_string(roster)?;
    let mut linked_count: u64 = 0;
    let mut logged_out_count: u64 = 0;
    let mut num_stmt = c.prepare(
        "SELECT state FROM donna_whatsapp_numbers
         WHERE EXISTS(SELECT 1 FROM json_each(?1) WHERE value=bot_id)",
    )?;
    for row in num_stmt.query_map([&roster_json], |r| r.get::<_, String>(0))? {
        match row?.as_str() {
            "linked" => linked_count += 1,
            "logged_out" => logged_out_count += 1,
            _ => {}
        }
    }

    if logged_out_count > 0 {
        return Ok(DoctorCheck {
            id: "channels".into(),
            area: DoctorArea::Channels,
            status: DoctorCheckStatus::Warning,
            code: "channels_whatsapp_logged_out".into(),
            count: Some(logged_out_count),
            action_kind: DoctorActionKind::CheckChannels,
        });
    }

    if linked_count == 0 {
        if space != PERSONAL && mode == "donna_only" {
            return Ok(DoctorCheck {
                id: "channels".into(),
                area: DoctorArea::Channels,
                status: DoctorCheckStatus::Ok,
                code: "channels_ok".into(),
                count: Some(0),
                action_kind: DoctorActionKind::None,
            });
        }
        return Ok(DoctorCheck {
            id: "channels".into(),
            area: DoctorArea::Channels,
            status: DoctorCheckStatus::Warning,
            code: "channels_whatsapp_unlinked".into(),
            count: Some(0),
            action_kind: DoctorActionKind::CheckChannels,
        });
    }

    let uncertain_count: i64 = c.query_row(
        "SELECT COUNT(*) FROM donna_whatsapp_outbox
         WHERE state='uncertain' AND EXISTS(SELECT 1 FROM json_each(?1) WHERE value=bot_id)",
        [&roster_json],
        |r| r.get(0),
    )?;
    ensure!(uncertain_count >= 0, "invalid_outbox_count");
    if uncertain_count > 0 {
        return Ok(DoctorCheck {
            id: "channels".into(),
            area: DoctorArea::Channels,
            status: DoctorCheckStatus::Warning,
            code: "channels_whatsapp_outbox_uncertain".into(),
            count: Some(u64::try_from(uncertain_count).unwrap_or(0)),
            action_kind: DoctorActionKind::CheckChannels,
        });
    }

    Ok(DoctorCheck {
        id: "channels".into(),
        area: DoctorArea::Channels,
        status: DoctorCheckStatus::Ok,
        code: "channels_ok".into(),
        count: Some(linked_count),
        action_kind: DoctorActionKind::None,
    })
}

fn check_mcps(store: &Store, space: &str) -> DoctorCheck {
    match eval_mcps(store, space) {
        Ok(check) => check,
        Err(_) => DoctorCheck {
            id: "mcps".into(),
            area: DoctorArea::Mcps,
            status: DoctorCheckStatus::Unavailable,
            code: "mcps_unavailable".into(),
            count: None,
            action_kind: DoctorActionKind::CheckMcps,
        },
    }
}

fn eval_mcps(store: &Store, space: &str) -> Result<DoctorCheck> {
    let (raw, _) = crate::market::vault::read_unmigrated(store, "connectors")?;
    let connectors: Vec<crate::market::Connector> = raw.map_or_else(|| Ok(Vec::new()), |s| serde_json::from_str(&s))?;
    ensure!(connectors.iter().all(|c| !c.space.is_empty()), "connector_space_unavailable");
    let space_connectors: Vec<_> = connectors.into_iter().filter(|c| c.space == space).collect();
    let total = space_connectors.len() as u64;
    let issues = space_connectors
        .iter()
        .filter(|c| {
            !c.configured()
                || c.missing_catalog_header()
                || c.oauth.as_ref().is_some_and(|o| {
                    !o.signed_in()
                        || c.oauth_binding
                            .as_ref()
                            .is_some_and(super::super::conectores::oauth_binding::Binding::scopes_retained)
                })
        })
        .count() as u64;

    if issues > 0 {
        Ok(DoctorCheck {
            id: "mcps".into(),
            area: DoctorArea::Mcps,
            status: DoctorCheckStatus::Warning,
            code: "mcps_attention_needed".into(),
            count: Some(issues),
            action_kind: DoctorActionKind::CheckMcps,
        })
    } else {
        Ok(DoctorCheck {
            id: "mcps".into(),
            area: DoctorArea::Mcps,
            status: DoctorCheckStatus::Ok,
            code: "mcps_ok".into(),
            count: Some(total),
            action_kind: DoctorActionKind::None,
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoutineStoreRow {
    #[serde(default)]
    paused: bool,
    #[serde(default)]
    routines: Vec<RoutineRow>,
    #[serde(default)]
    runs: Vec<RunRow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoutineRow {
    id: String,
    bot_id: String,
    enabled: bool,
    #[serde(default)]
    deleted: bool,
    #[serde(default)]
    last_error: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunRow {
    routine_id: String,
    status: String,
    #[serde(default)]
    created_at: i64,
}

fn check_routines(c: &Connection, roster: &[String]) -> DoctorCheck {
    match eval_routines(c, roster) {
        Ok(check) => check,
        Err(_) => DoctorCheck {
            id: "routines".into(),
            area: DoctorArea::Routines,
            status: DoctorCheckStatus::Unavailable,
            code: "routines_unavailable".into(),
            count: None,
            action_kind: DoctorActionKind::CheckRoutines,
        },
    }
}

fn eval_routines(c: &Connection, roster: &[String]) -> Result<DoctorCheck> {
    let raw: Option<String> = c.query_row("SELECT v FROM kv WHERE k='routines.v1'", [], |r| r.get(0)).optional()?;
    let Some(raw) = raw else {
        return Ok(DoctorCheck {
            id: "routines".into(),
            area: DoctorArea::Routines,
            status: DoctorCheckStatus::Ok,
            code: "routines_ok".into(),
            count: Some(0),
            action_kind: DoctorActionKind::None,
        });
    };
    let state: RoutineStoreRow = serde_json::from_str(&raw)?;
    let space_routines: Vec<&RoutineRow> =
        state.routines.iter().filter(|r| !r.deleted && roster.iter().any(|id| id == &r.bot_id)).collect();
    let total = space_routines.len() as u64;

    let mut failed_count: u64 = 0;
    let mut paused_count: u64 = 0;
    for routine in &space_routines {
        let latest_run = state.runs.iter().filter(|run| run.routine_id == routine.id).max_by_key(|run| run.created_at);
        let run_failed = latest_run.is_some_and(|run| run.status == "failed");
        if routine.last_error.is_some() || run_failed {
            failed_count += 1;
        }
        if !routine.enabled {
            paused_count += 1;
        }
    }

    if failed_count > 0 {
        return Ok(DoctorCheck {
            id: "routines".into(),
            area: DoctorArea::Routines,
            status: DoctorCheckStatus::Warning,
            code: "routines_failed".into(),
            count: Some(failed_count),
            action_kind: DoctorActionKind::CheckRoutines,
        });
    }
    if state.paused && total > 0 {
        return Ok(DoctorCheck {
            id: "routines".into(),
            area: DoctorArea::Routines,
            status: DoctorCheckStatus::Warning,
            code: "routines_paused".into(),
            count: Some(total),
            action_kind: DoctorActionKind::CheckRoutines,
        });
    }
    if paused_count > 0 {
        return Ok(DoctorCheck {
            id: "routines".into(),
            area: DoctorArea::Routines,
            status: DoctorCheckStatus::Warning,
            code: "routines_paused".into(),
            count: Some(paused_count),
            action_kind: DoctorActionKind::CheckRoutines,
        });
    }
    Ok(DoctorCheck {
        id: "routines".into(),
        area: DoctorArea::Routines,
        status: DoctorCheckStatus::Ok,
        code: "routines_ok".into(),
        count: Some(total),
        action_kind: DoctorActionKind::None,
    })
}

fn check_delegation(c: &Connection, space: &str, roster: &[String], now: i64) -> DoctorCheck {
    match eval_delegation(c, space, roster, now) {
        Ok(check) => check,
        Err(_) => DoctorCheck {
            id: "delegation".into(),
            area: DoctorArea::Delegation,
            status: DoctorCheckStatus::Unavailable,
            code: "delegation_unavailable".into(),
            count: None,
            action_kind: DoctorActionKind::OpenTorre,
        },
    }
}

fn eval_delegation(c: &Connection, space: &str, roster: &[String], now: i64) -> Result<DoctorCheck> {
    if space != PERSONAL {
        return Ok(DoctorCheck {
            id: "delegation".into(),
            area: DoctorArea::Delegation,
            status: DoctorCheckStatus::Ok,
            code: "delegation_ok".into(),
            count: Some(0),
            action_kind: DoctorActionKind::None,
        });
    }
    let roster_refs: Vec<&str> = roster.iter().map(String::as_str).collect();
    let manter =
        |p: &Pedido| p.sem_sucesso() && roster_refs.contains(&p.de.as_str()) && roster_refs.contains(&p.para.as_str());
    let lidos = ler_na(c, &Filtro { desde: now - JANELA_MS, limite: 100, manter: &manter })?;
    if lidos.total > 0 {
        Ok(DoctorCheck {
            id: "delegation".into(),
            area: DoctorArea::Delegation,
            status: DoctorCheckStatus::Warning,
            code: "inter_bot_stalled".into(),
            count: Some(lidos.total as u64),
            action_kind: DoctorActionKind::OpenTorre,
        })
    } else {
        Ok(DoctorCheck {
            id: "delegation".into(),
            area: DoctorArea::Delegation,
            status: DoctorCheckStatus::Ok,
            code: "delegation_ok".into(),
            count: Some(0),
            action_kind: DoctorActionKind::None,
        })
    }
}
