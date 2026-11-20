// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Bot and chat methods: roster, messages, uploads, memory, routines and the bots' own MCP calls.

use super::devices::Caller;
use super::str_arg;
use crate::LockExt;
use crate::agent::bot::Cmd;
use crate::hub::Hub;
use crate::store::{EntryKind, Lane};
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::ops::ControlFlow;
use std::sync::Arc;

pub(super) async fn call(
    hub: &Arc<Hub>,
    caller: &Caller,
    method: &str,
    b: Value,
    engine_authority: Option<&crate::fork::engine_setup::access::Authority>,
    execution: Option<&crate::fork::sessoes::ExecutionRef>,
) -> Result<ControlFlow<Value, Value>> {
    // I-023: o resultado da execução não espera o ator segurando o `team_lock`; E1 é a captura do vínculo.
    if method == "routineCall" && b["name"] == crate::fork::rotina_resultado::TOOL {
        let execution = execution.ok_or_else(|| anyhow!("chamada vencida: execução ausente"))?;
        let bot = str_arg(&b, "botId")?;
        return Ok(ControlFlow::Break(
            crate::fork::rotina_resultado::call(hub, bot, &b["arguments"], execution).await?,
        ));
    }
    // Edições de rotinas não podem entrar no snapshot de uma mudança concorrente da Donna.
    let _routine_guard = if matches!(
        method,
        "saveRoutine"
            | "duplicateRoutine"
            | "setRoutineEnabled"
            | "deleteRoutine"
            | "runRoutine"
            | "routineWebhook"
            | "routineCall"
            | "setRoutinesPaused"
    ) {
        let guard = hub.store.team_lock.lock().await;
        crate::chat::manage::recover_live(hub);
        Some(guard)
    } else {
        None
    };
    crate::fork::sessoes::bindings::recheck_optional(hub, execution)?;
    let read_result = matches!(method, "memoryCall" | "teamCall")
        && matches!(b["name"].as_str(), Some("search_memory" | "read_memory" | "list_bots" | "my_team_chain"));
    let result = match method {
        "routinesPause" => hub.routines.pause_state(),
        "setRoutinesPaused" => {
            hub.routines.set_paused(&hub.store, b["paused"].as_bool().context("paused is required")?)?
        }
        "routineSchedule" => crate::routines::Routines::schedule_preview_pt(&b, crate::store::now_ms())?,
        "routines" => hub.routines.list(str_arg(&b, "botId")?),
        "routinesOverview" => hub.routines.overview(&hub.store, &b)?,
        "routineTeams" => crate::routines::Routines::teams_in_space(
            &hub.store,
            b["spaceId"].as_str().unwrap_or(crate::fork::spaces::PERSONAL),
        )?,
        "saveRoutine" => hub.routines.save(hub, str_arg(&b, "botId")?, &b)?,
        "duplicateRoutine" => hub.routines.duplicate(hub, str_arg(&b, "botId")?, str_arg(&b, "id")?)?,
        "setRoutineEnabled" => hub.routines.set_enabled(
            &hub.store,
            str_arg(&b, "botId")?,
            str_arg(&b, "id")?,
            b["enabled"].as_bool().context("enabled is required")?,
        )?,
        "deleteRoutine" => hub.routines.remove(&hub.store, str_arg(&b, "botId")?, str_arg(&b, "id")?)?,
        // Banco ocupado por outra tarefa do Hub: espera curta e repete (a reserva manual é idempotente).
        "runRoutine" => {
            let (bot, id) = (str_arg(&b, "botId")?, str_arg(&b, "id")?);
            crate::fork::atividades::store::retry_store_busy(|| {
                hub.routines.enqueue(&hub.store, bot, id, json!({"source":"test"}), None, true)
            })
            .await?
        }
        "routineWebhook" => hub.routines.credentials(
            hub,
            str_arg(&b, "botId")?,
            str_arg(&b, "id")?,
            b["rotate"].as_bool().unwrap_or(false),
        )?,
        "routineCall" => crate::routines::call_invoked_in_execution(
            hub,
            str_arg(&b, "botId")?,
            str_arg(&b, "name")?,
            &b["arguments"],
            execution.ok_or_else(|| anyhow!("chamada vencida: execução ausente"))?,
            b.get("invocation").map(|v| v.as_str().context("routine_creation_invocation_invalid")).transpose()?,
        )?,
        "memoryCall" => {
            let (bot, name) = (str_arg(&b, "botId")?, str_arg(&b, "name")?);
            if matches!(name, "search_history" | "open_history") {
                // Só o servidor da sessão principal do bot lê o histórico privado (D229-6).
                crate::chat::memory::history_call_in_execution(
                    hub,
                    bot,
                    name,
                    &b["arguments"],
                    b["sessionSlot"].as_str(),
                    execution,
                )
                .await?
            } else {
                crate::chat::memory::tool_call_in_execution(hub, bot, name, &b["arguments"], execution).await?
            }
        }
        "chatCall" => {
            let (bot, name) = (str_arg(&b, "botId")?, str_arg(&b, "name")?);
            if crate::chat::profile::TOOLS.contains(&name) {
                crate::chat::profile::call(hub, bot, name, &b["arguments"]).await?
            } else {
                crate::chat::outbox::call(hub, bot, name, &b["arguments"], execution).await?
            }
        }
        "teamCall" => {
            crate::chat::team::call_in_execution(
                hub,
                str_arg(&b, "botId")?,
                str_arg(&b, "name")?,
                &b["arguments"],
                execution,
            )
            .await?
        }
        "taskCall" => {
            crate::fork::tarefas::call_in_execution(
                hub,
                str_arg(&b, "botId")?,
                str_arg(&b, "name")?,
                &b["arguments"],
                execution,
            )
            .await?
        }
        "manageCall" => {
            crate::chat::manage::call_in_execution(
                hub,
                str_arg(&b, "botId")?,
                str_arg(&b, "name")?,
                &b["arguments"],
                execution,
            )
            .await?
        }
        "history" => {
            let bot = str_arg(&b, "botId")?;
            let before = b["beforeSeq"].as_i64().unwrap_or(i64::MAX);
            let limit = b["limit"].as_i64().unwrap_or(100).clamp(1, 500);
            let session = crate::fork::sessoes::api::history_session(hub, bot, &b)?;
            json!({"entries": hub.store.history_in_session(bot, session, before, limit)?})
        }
        // Notices between a bot and one peer (the newest 200), for the read-only bot conversation sheet.
        "botConversation" => {
            json!({"entries": hub.store.bot_conversation(str_arg(&b, "botId")?, str_arg(&b, "peerId")?, 200)?})
        }
        "teamSettings" => {
            let value = serde_json::to_value(hub.store.team_settings()?)?;
            if let Some(authority) = engine_authority {
                authority.require_current(hub)?;
            }
            value
        }
        "setTeamSettings" => {
            let _guard = hub.store.team_lock.lock().await;
            crate::chat::manage::recover_live(hub);
            // D397: o app não escolhe nem troca a principal.
            crate::fork::donna_fixa::check_primary_change(&hub.store, &b)?;
            let tower_change = crate::fork::control_tower::primary_change(hub, &b)?;
            for cfg in hub.store.set_team_settings(&b)? {
                if let Err(error) = hub.send_cmd(&cfg.id, Cmd::Reconfigure(Box::new(cfg.clone()))) {
                    tracing::warn!(%error, bot = %cfg.id, "team settings saved; actor reconfiguration pending restart");
                }
                if let Err(error) = hub.send_cmd(&cfg.id, Cmd::RefreshTools) {
                    tracing::warn!(%error, bot = %cfg.id, "team tools refresh pending restart");
                }
                hub.emit_bot(&cfg.id);
            }
            tower_change.publish(hub);
            serde_json::to_value(hub.store.team_settings()?)?
        }
        "undoTeamChange" => crate::chat::manage::undo(hub, str_arg(&b, "entryId")?).await?,
        "createBot" => {
            let owner = crate::fork::autoridade::Autoridade::capturar(hub, caller)?;
            crate::fork_avatars::validate_patch(&b)?;
            let mut b = b;
            // c1: condição e autoridade originais antes de qualquer fila.
            let skills = crate::fork::skill_selection::CreationSelection::capture(hub, caller, &mut b)?;
            // MX-264/C: `skills` ausente/nulo deixa o Hub aplicar os padrões de
            // bots novos; uma lista (mesmo vazia) é escolha explícita do dono.
            let skills_explicitos = b.get("skills").is_some_and(|v| !v.is_null());
            let guard = hub.store.team_lock.clone().lock_owned().await;
            crate::chat::manage::recover_live(hub);
            // Only a bot's `create_bot` records who made one.
            b["createdBy"] = Value::Null;
            let permission_chosen = b.get("permission").is_some() && b["permissionChosenByUser"] == true;
            b["permissionChosenByUser"] = permission_chosen.into();
            if !permission_chosen {
                b["permission"] = serde_json::to_value(hub.store.team_settings()?.permission)?;
            }
            if let Some(authority) = engine_authority {
                authority.require_current(hub)?;
            }
            let cfg = crate::agent::new_bot::with_defaults(hub, b).await?;
            let authority = engine_authority.cloned();
            let hub = hub.clone();
            json!({"bot": tokio::task::spawn_blocking(move || {
                let _guard = guard;
                // MX-264/C: padrão "novos bots já vêm com…" (só skills; D91 segue
                // para conectores/contas). Sem seleção explícita, soma os padrões
                // instalados; bots de ferramenta/Times não passam por aqui.
                let (cfg, skills) = crate::fork::extensoes_em_todos::padroes_novos_bot(
                    &hub, &owner, cfg, skills, skills_explicitos,
                );
                hub.create_bot_authorized(cfg, authority.as_ref(), skills.as_ref(), Some(&owner))
            })
            .await??})
        }
        "updateBot" => {
            // c1: intenção tipada de uma célula; captura antes de team_lock, sem recaptura.
            if let Some(selection) = crate::fork::skill_selection::Selection::capture(hub, caller, &b)? {
                let guard = hub.store.team_lock.clone().lock_owned().await;
                crate::chat::manage::recover_live(hub);
                let hub = hub.clone();
                return Ok(ControlFlow::Break(json!({"bot": tokio::task::spawn_blocking(move || {
                    let _guard = guard;
                    selection.commit(&hub)
                })
                .await??})));
            }
            // D368: autoridade ORIGINAL antes da fila; a escrita a reconfere dentro da transação.
            let owner = crate::fork::autoridade::Autoridade::capturar(hub, caller)?;
            let guard = hub.store.team_lock.clone().lock_owned().await;
            crate::chat::manage::recover_live(hub);
            if let Some(authority) = engine_authority {
                authority.require_current(hub)?;
            }
            let authority = engine_authority.cloned();
            let hub = hub.clone();
            json!({"bot": tokio::task::spawn_blocking(move || { let _guard = guard; hub.update_bot_by_user_authorized(&b, authority.as_ref(), Some(&owner)) }).await??})
        }
        // The roster as the user dragged it, top first.
        "reorderBots" => {
            let guard = hub.store.team_lock.clone().lock_owned().await;
            crate::chat::manage::recover_live(hub);
            let ids: Vec<String> = serde_json::from_value(b["ids"].clone()).context("ids is required")?;
            let hub = hub.clone();
            tokio::task::spawn_blocking(move || {
                let _guard = guard;
                hub.reorder_bots(&ids)
            })
            .await??;
            json!({})
        }
        "deleteBot" => {
            let _guard = hub.store.team_lock.lock().await;
            crate::chat::manage::recover_live(hub);
            hub.delete_bot(str_arg(&b, "botId")?)?;
            json!({})
        }
        "markRead" => {
            hub.mark_read(str_arg(&b, "botId")?)?;
            json!({})
        }
        // One chunk of a file for a later `send` (`attachments`); base64 `data` at `offset`.
        "upload" => {
            use base64::Engine as _;
            let row = hub.store.bot(str_arg(&b, "botId")?)?.filter(|r| !r.deleted);
            let root = crate::chat::uploads::root(&row.ok_or_else(|| anyhow!("unknown bot"))?.config);
            let (id, name) = (str_arg(&b, "uploadId")?.to_owned(), str_arg(&b, "name")?.to_owned());
            let data =
                base64::engine::general_purpose::STANDARD.decode(str_arg(&b, "data")?).context("invalid data")?;
            let (offset, done) = (b["offset"].as_u64().unwrap_or(0), b["done"] == true);
            let attachment = tokio::task::spawn_blocking(move || {
                crate::chat::uploads::append(&root, &id, &name, offset, &data, done)
            })
            .await??;
            json!({"attachment": attachment})
        }
        // A sent file, back in chunks (chat previews on other devices).
        "readUpload" => {
            use base64::Engine as _;
            let row = hub.store.bot(str_arg(&b, "botId")?)?.filter(|r| !r.deleted);
            let root = crate::chat::uploads::root(&row.ok_or_else(|| anyhow!("unknown bot"))?.config);
            let id = str_arg(&b, "uploadId")?.to_owned();
            let offset = b["offset"].as_u64().unwrap_or(0);
            let (data, size) =
                tokio::task::spawn_blocking(move || crate::chat::uploads::read(&root, &id, offset, 384 * 1024))
                    .await??;
            json!({"data": base64::engine::general_purpose::STANDARD.encode(data), "size": size})
        }
        "send" => {
            let bot = str_arg(&b, "botId")?;
            let text = b["text"].as_str().unwrap_or_default().trim();
            let uploads: Vec<&str> =
                b["attachments"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            if text.is_empty() && uploads.is_empty() {
                bail!("empty message");
            }
            let nonce = b["clientNonce"].as_str().unwrap_or_default();
            if !nonce.is_empty()
                && let Some(e) = hub.store.find_by_nonce(bot, nonce)
            {
                return Ok(ControlFlow::Break(json!({"entry": e})));
            }
            let row = hub.store.bot(bot)?.filter(|r| !r.deleted).ok_or_else(|| anyhow!("unknown bot"))?;
            // Open: a conversa só de texto reserva o turno antes de gravar a mensagem.
            let open_turn = if crate::environment::Environment::is_open() {
                Some(crate::fork::open_chat::begin(hub, &row.config)?)
            } else {
                None
            };
            // `replyTo`: the message of this chat it answers (`chat::replies`).
            let reply = match b["replyTo"].as_str().filter(|id| !id.is_empty()) {
                Some(id) => Some(crate::chat::replies::quote(&hub.store, bot, id)?),
                None => None,
            };
            let lane = crate::fork::sessoes::api::send_lane(&hub.store.db.locked(), bot, &b)?;
            let mut attachments = Vec::new();
            if !uploads.is_empty() {
                let root = crate::chat::uploads::root(&row.config);
                for id in uploads {
                    attachments.push(crate::chat::uploads::resolve(&root, id)?.1);
                }
            }
            let turn = hub.store.alloc_turn(bot)?;
            // A group has no agent of its own to queue behind: its message is simply sent.
            let status = if row.config.is_group() { "sent" } else { "queued" };
            let mut data = json!({"text": text, "clientNonce": nonce, "status": status, "attachments": attachments});
            crate::computers::registry::remember_source(&hub.store, &b, &mut data);
            // A fila D72 precisa sobreviver até ao crash entre salvar a entrada e entregá-la ao ator.
            if !row.config.is_group()
                && crate::agent::bot::read_waiting_checkpoint(&hub.store, bot)?
                    .as_ref()
                    .is_some_and(crate::agent::bot::checkpoint_waiting_active)
            {
                data["subscriptionQueued"] = json!(true);
            }
            // What the user pasted rather than typed: memory never takes it as the user's own words.
            let pasted: Vec<&str> = b["pasted"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|p| !p.is_empty() && text.contains(p))
                .collect();
            if !pasted.is_empty() {
                data["pasted"] = json!(pasted);
            }
            if let Some(reply) = reply {
                data["replyTo"] = reply;
            }
            data["contentOrigin"] = crate::guard_hub::origem::capture(caller, &b, &data);
            crate::fork::idioma_app::carimbar(&hub.store, caller, &mut data); // D395
            // MB-1b: a mensagem humana responde, na mesma transação, as perguntas pendentes desta conversa.
            let e = crate::fork::owner_question::add_human_message(hub, &lane, turn, &data)
                .ok_or_else(|| anyhow!("couldn't save the message"))?;
            crate::fork::sessoes::api::title_first_message(hub, &lane, text);
            // Before the turn starts, so its reply lands unread.
            if let Err(error) = hub.mark_read(bot) {
                tracing::warn!(%error, bot, "couldn't mark bot read after send");
            }
            if let Some(ticket) = open_turn {
                crate::fork::open_chat::run(hub, ticket, &row.config, lane, turn, e.id.clone(), text.to_owned());
            } else if row.config.is_group() {
                // Each member reads it in its room prompt (`chat::group`).
                crate::chat::group::start(hub, &row.config, lane);
            } else {
                // `/skill …` reaches the agent as "read this skill first"; the chat keeps what was typed.
                let prompt = crate::chat::prompt::of_entry(hub, &row.config, &e.data);
                hub.send_cmd(bot, Cmd::Send { lane, entry_id: e.id.clone(), text: prompt })?;
            }
            json!({"entry": e})
        }
        // A finished voice call, as a line in the chat ("Voice chat · 0:16").
        "logCall" => {
            let bot = str_arg(&b, "botId")?;
            hub.store.bot(bot)?.filter(|r| !r.deleted).ok_or_else(|| anyhow!("unknown bot"))?;
            let seconds = b["seconds"].as_u64().unwrap_or(0);
            let data = json!({"text": "Voice chat", "callSeconds": seconds});
            let e = hub.add_entry(&Lane::main(bot), EntryKind::Notice, hub.store.max_turn(bot), &data);
            json!({"entry": e})
        }
        "retrySubscription" => {
            let bot = str_arg(&b, "botId")?;
            let entry_id = str_arg(&b, "entryId")?.to_owned();
            hub.send_cmd(bot, Cmd::RetrySubscription { entry_id })?;
            json!({})
        }
        "stop" => {
            let bot = str_arg(&b, "botId")?;
            match hub.store.bot(bot)? {
                _ if crate::environment::Environment::is_open() => crate::fork::open_chat::stop(bot),
                Some(row) if row.config.is_group() => hub.groups.stop(hub, bot),
                _ => {
                    crate::fork::spaces::legacy_control(
                        hub,
                        b["spaceId"].as_str().unwrap_or(crate::fork::spaces::PERSONAL),
                        bot,
                        false,
                    )
                    .await?;
                }
            }
            json!({})
        }
        // DON-907 (D431): ações por mensagem na fila; autoridade ORIGINAL capturada na entrada (`agent::bot::fork_fila`).
        "queuedRemove" | "queuedEdit" | "queuedMove" | "queuedSendNow" => {
            crate::agent::bot::queued_rpc(hub, caller, method, &b).await?
        }
        "newSession" => {
            // Sessões PR5: autoridade ORIGINAL capturada aqui, antes de qualquer espera, e levada até a Tx do ator.
            let autoridade = crate::fork::autoridade::Autoridade::capturar(hub, caller)?;
            crate::fork::spaces::legacy_restart(
                hub,
                b["spaceId"].as_str().unwrap_or(crate::fork::spaces::PERSONAL),
                str_arg(&b, "botId")?,
                autoridade,
            )
            .await?;
            json!({})
        }
        "memory" | "saveMemory" | "pinMemory" | "archiveMemory" | "forgetMemory" | "clearMemory" | "undoDream"
        | "answerMemory" | "undoMemory" | "memoryFile" | "saveMemoryFile" | "deleteMemoryFile"
        | "restoreMemoryFile" => crate::chat::memory::call(hub, method, &b).await?,
        "editHandoff" | "keepConversation" => crate::chat::handoff::call(hub, method, &b)?,
        "react" => {
            let e = hub.react(str_arg(&b, "entryId")?, str_arg(&b, "emoji")?)?;
            json!({"entry": e})
        }
        "respondPermission" => crate::fork::permission_card::respond(hub, &b).await?,
        _ => return Ok(ControlFlow::Continue(b)),
    };
    if read_result {
        crate::fork::sessoes::bindings::recheck_optional(hub, execution)?;
    }
    Ok(ControlFlow::Break(result))
}
