// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Agent → client: `session/update`s, permission requests and streamed text mapped onto entries.

use super::fast_mode::Listing;
use super::{Actor, Reacted, Seg, SegKind};
use crate::LockExt;
use crate::agent::acp::{self, Incoming};
use crate::fork::activity_code::ActivityKind;
use crate::fork::push_textos::{Corpo, Titulo};
use crate::fork::sessoes::{ExecutionRef, bindings};
use crate::hub::BotStatus;
use crate::remote::push::{self, AlertKind};
use crate::store::EntryKind;
use anyhow::bail;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

/// Who waits for an approval card's answer.
pub(super) enum Answer {
    /// The agent's `session/request_permission`, by its JSON-RPC id.
    Agent(Value),
    /// A Codync tool (`Cmd::Approve`): true when the user allowed it.
    Codync(oneshot::Sender<bool>),
}

/// As opções já filtradas do cartão são a autoridade para qualquer resposta do cliente.
pub(super) struct Pending {
    answer: Answer,
    options: Vec<String>,
    context: crate::fork::permission_card::PendingContext,
    execution: ExecutionRef,
    policy: PolicyRequest,
    reject_options: Vec<String>,
}

struct PolicyRequest {
    params: Value,
    recorded: Option<Value>,
    adapter: super::Adapter,
    origin: crate::fork::rules::avaliacao::Origem,
}

impl PolicyRequest {
    fn check(
        &self,
        store: &crate::store::Store,
        bot: &str,
        permission: crate::store::Permission,
    ) -> anyhow::Result<()> {
        use crate::fork::rules::gate::{GateOutcome, GateRequest, decide};
        if decide(
            store,
            &GateRequest {
                params: &self.params,
                recorded: self.recorded.as_ref(),
                bot_id: bot,
                adapter: self.adapter,
                permission,
                origin: self.origin,
            },
        ) == GateOutcome::Deny
        {
            bail!("A política atual recusou esta aprovação. O cartão expirou.");
        }
        Ok(())
    }
}

fn check_context_permission(
    hub: &crate::hub::Hub,
    cfg: &crate::store::BotConfig,
    lane: &crate::store::Lane,
    turn: Option<i64>,
    session: Option<&str>,
    id: &str,
    pending: &crate::fork::permission_card::AnsweredContext,
) -> anyhow::Result<()> {
    let entry = hub.store.entry(id).ok_or_else(|| anyhow::anyhow!("card_not_found"))?;
    pending.check(&entry, &cfg.id, lane, turn, session)?;
    let original = hub.store.bot(&cfg.id)?.ok_or_else(|| anyhow::anyhow!("bot_not_found"))?.config;
    crate::fork::spaces::tool_context::card_config(hub, &entry, original)?;
    Ok(())
}

fn mark_delivery_error(hub: &crate::hub::Hub, entry_id: &str, error: &anyhow::Error) {
    if let Some(mut entry) = hub.store.entry(entry_id) {
        entry.data["status"] =
            json!(if error.to_string().contains("política atual recusou") { "expired" } else { "cancelled" });
        entry.data["selected"] = Value::Null;
        entry.data["detail"] = json!(format!("{}\n{}", error, entry.data["detail"].as_str().unwrap_or_default()));
        hub.set_entry(entry_id, &entry.data);
    }
}

impl Actor {
    pub(super) async fn on_incoming(&mut self, inc: Option<Incoming>) {
        if matches!(inc, Some(Incoming::Notification { .. } | Incoming::Request { .. })) {
            self.mudo_sinal();
        }
        match inc {
            None | Some(Incoming::Closed { .. }) => {
                let tail = match inc {
                    Some(Incoming::Closed { stderr_tail }) => stderr_tail,
                    _ => String::new(),
                };
                self.conn = None;
                // The in-flight prompt request resolves with an error; finish_turn reports it with this output.
                self.exit_tail = Some(tail);
            }
            Some(Incoming::Notification { method, params }) => {
                // Any session's, turn or not: the agent changed an option itself.
                if method == "session/update"
                    && params["update"]["sessionUpdate"] == "config_option_update"
                    && let Some(sid) = params["sessionId"].as_str()
                {
                    self.note_options(sid, &params["update"], Listing::Update);
                } else if method == "session/update"
                    && params["sessionId"].as_str() == self.turn_session.as_deref()
                    && self.turn.is_some()
                    && !self.waiting_for_reset()
                {
                    self.on_update(&params["update"]);
                }
            }
            Some(Incoming::Request { id, method, params }) => {
                let Some(conn) = &self.conn else { return };
                let acp = conn.acp.clone();
                if method == "session/request_permission" && self.turn.is_some() && !self.waiting_for_reset() {
                    self.on_permission(id, params).await;
                } else if method == crate::fork::bot_skills::read::METHOD {
                    self.on_skill_read(id, params);
                } else {
                    let _ = acp.respond_error(id, -32601, &format!("{method} is not supported by Donnabot")).await;
                }
            }
        }
    }

    pub(super) fn on_update(&mut self, u: &Value) {
        if matches!(u["kind"].as_str(), Some("fetch" | "search")) {
            self.hub.computers.mark_external(&self.cfg.id);
        }
        let turn = self.turn.unwrap_or(0);
        match u["sessionUpdate"].as_str().unwrap_or_default() {
            "agent_message_chunk" => {
                let t = acp::content_text(&u["content"]);
                self.append(SegKind::Text, &t);
                self.set_activity("Writing a reply…", ActivityKind::WritingReply);
            }
            "agent_thought_chunk" => {
                let t = acp::content_text(&u["content"]);
                self.append(SegKind::Thought, &t);
                self.set_activity("Thinking…", ActivityKind::Thinking);
            }
            "tool_call" => {
                self.close_seg();
                let tool_id = u["toolCallId"].as_str().unwrap_or_default().to_owned();
                let mut data = json!({
                    "toolCallId": tool_id,
                    "title": u["title"].as_str().unwrap_or("Tool"),
                    "toolKind": u["kind"].as_str().unwrap_or("other"),
                    "status": u["status"].as_str().unwrap_or("pending"),
                    "output": "",
                    "diffs": [],
                    "locations": u["locations"].clone(),
                });
                let (adapter, version) = self
                    .conn
                    .as_ref()
                    .map_or((super::Adapter::Other, None), |c| (c.adapter, c.adapter_version.as_deref()));
                crate::guard_hub::tool_origin::record(
                    &mut data,
                    u,
                    self.turn_session.as_ref().and_then(|sid| self.hub_tool_servers.get(sid)),
                    adapter,
                    version,
                );
                self.mark_memory_tool(&data);
                merge_tool_content(&mut data, &u["content"]);
                crate::fork::whatsapp::leitura::redact_step(&mut data, u);
                let title = data["title"].as_str().unwrap_or("Tool").to_owned();
                if let Some(e) = self.add(EntryKind::Tool, turn, data) {
                    self.tools.insert(tool_id, e.id);
                }
                let (activity_text, kind) = super::activity(&title);
                self.set_activity(activity_text, kind);
            }
            "tool_call_update" => {
                let tool_id = u["toolCallId"].as_str().unwrap_or_default();
                let Some(entry_id) = self.tools.get(tool_id).cloned() else {
                    self.mark_memory_tool(&Value::Null);
                    return;
                };
                let Some(mut e) = self.hub.store.entry(&entry_id) else {
                    self.mark_memory_tool(&Value::Null);
                    return;
                };
                for (k, key) in [("title", "title"), ("kind", "toolKind"), ("status", "status")] {
                    if let Some(v) = u[k].as_str() {
                        e.data[key] = v.into();
                    }
                }
                if !u["locations"].is_null() {
                    e.data["locations"] = u["locations"].clone();
                }
                merge_tool_content(&mut e.data, &u["content"]);
                crate::fork::whatsapp::leitura::redact_step(&mut e.data, u);
                let status = e.data["status"].as_str().unwrap_or_default().to_owned();
                let title = e.data["title"].as_str().unwrap_or_default().to_owned();
                let (adapter, version) = self
                    .conn
                    .as_ref()
                    .map_or((super::Adapter::Other, None), |c| (c.adapter, c.adapter_version.as_deref()));
                crate::guard_hub::tool_origin::revise(
                    &mut e.data,
                    u,
                    self.turn_session.as_ref().and_then(|sid| self.hub_tool_servers.get(sid)),
                    adapter,
                    version,
                );
                self.mark_memory_tool(&e.data);
                self.hub.set_entry(&entry_id, &e.data);
                if status == "in_progress" {
                    let (activity_text, kind) = super::activity(&title);
                    self.set_activity(activity_text, kind);
                }
            }
            "compaction_update" => {
                if let Some(conn) = self.conn.as_mut() {
                    conn.reports_compaction = true;
                }
                let id = u["compactionId"].as_str().unwrap_or_default();
                match u["status"].as_str() {
                    Some("in_progress") => self.compaction_began(id),
                    Some("completed") if self.other_compaction_running(id) => {}
                    // Claude's adapter carries the summary on one of its completed updates.
                    Some("completed") => {
                        let summary = acp::content_text(&u["summary"]);
                        self.compacted(id, (!summary.trim().is_empty()).then_some(summary));
                    }
                    _ => {
                        self.compaction_settled(id);
                    }
                }
            }
            "usage_update" => {
                self.cost_observe(u);
                self.note_limit(u);
                let info = &u["_meta"]["_claude/rateLimit"];
                if info.is_object() {
                    crate::usage::ingest_claude_rate_limit(&self.hub, info);
                }
                if let Some(used) = u["used"].as_u64() {
                    self.track_usage(used, u["size"].as_u64().unwrap_or(0));
                }
            }
            "plan" => {
                let data = json!({"entries": u["entries"].clone(), "author": self.cfg.id});
                match &self.plan_entry {
                    Some(id) => {
                        self.hub.set_entry(id, &data);
                    }
                    None => {
                        if let Some(e) = self.add(EntryKind::Plan, turn, data) {
                            self.plan_entry = Some(e.id);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn mark_memory_tool(&self, data: &Value) {
        if !crate::guard_hub::tool_origin::exposes_memory(data) {
            return;
        }
        let Some(sid) = self.turn_session.as_deref() else {
            self.hub.set_runtime(&self.cfg.id, |r| r.memory_exposure_error = true);
            return;
        };
        if let Err(error) =
            self.hub.store.memory_session_mark_with_failure(&self.cfg.id, sid, "ferramenta externa", || {
                self.hub.set_runtime(&self.cfg.id, |r| r.memory_exposure_error = true);
            })
        {
            tracing::error!(%error, "não foi possível marcar a sessão de memória");
        }
    }

    pub(super) async fn on_permission(&mut self, rpc_id: Value, params: Value) {
        if let Err(error) = crate::fork::permission_card::request_session(&params, self.turn_session.as_deref()) {
            if let Some(conn) = &self.conn {
                let _ = conn.acp.respond_error(rpc_id, -32602, &error.to_string()).await;
            }
            return;
        }
        let binding = self
            .turn_session
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("chamada vencida"))
            .and_then(|acp| bindings::for_acp(&self.hub.store, &self.cfg.id, acp))
            .and_then(|binding| bindings::capture(&self.hub, &self.cfg.id, binding.id.as_str()));
        let execution = match binding {
            Ok(execution) => execution,
            Err(error) => {
                if let Some(conn) = &self.conn {
                    let _ = conn.acp.respond_error(rpc_id, -32602, &error.to_string()).await;
                }
                return;
            }
        };
        let codex = self.conn.as_ref().is_some_and(|c| c.adapter == super::Adapter::Codex);
        let options = params["options"].as_array().cloned().unwrap_or_default();
        let options = if codex { crate::agent::engine_permissions::codex_options(options) } else { options };
        let tool = &params["toolCall"];
        let recorded =
            tool["toolCallId"].as_str().and_then(|id| self.tools.get(id)).and_then(|id| self.hub.store.entry(id));
        // Porteiro 13.1b: regras e nível efetivo decidem antes do cartão.
        // Sessão e vínculo já foram conferidos acima; Ask segue para o cartão.
        let outcome = crate::fork::rules::gate::decide(
            &self.hub.store,
            &crate::fork::rules::gate::GateRequest {
                params: &params,
                recorded: recorded.as_ref().map(|entry| &entry.data),
                bot_id: &self.cfg.id,
                adapter: self.conn.as_ref().map_or(super::Adapter::Other, |c| c.adapter),
                permission: self.cfg.permission,
                origin: if self.turn_text.is_some() {
                    crate::fork::rules::avaliacao::Origem::DonoDigitadoAutenticado
                } else {
                    crate::fork::rules::avaliacao::Origem::Derivada
                },
            },
        );
        // MX-110: numa execução catch-up, Allow vira Ask — todo efeito pede
        // cartão ao dono; Deny continua deny. Ferramentas sem pedido de
        // permissão já foram removidas pelo recorte de servidores (decide).
        let catchup = self.active_routine.as_deref().is_some_and(|id| self.hub.routines.run_is_catchup(id));
        let outcome = if catchup && matches!(outcome, crate::fork::rules::gate::GateOutcome::Allow) {
            crate::fork::rules::gate::GateOutcome::Ask
        } else {
            outcome
        };
        match outcome {
            crate::fork::rules::gate::GateOutcome::Allow => {
                // Só `allow_once` representa a decisão única; sem ela, cartão.
                if let Some(option) = options.iter().find(|o| o["kind"] == "allow_once" && o["optionId"].is_string()) {
                    let result = json!({"outcome": {"outcome": "selected", "optionId": option["optionId"]}});
                    if let Some(conn) = &self.conn {
                        // Vínculo e escrita no mesmo commit: revogação entre
                        // checar e responder não concede a permissão.
                        if let Err(error) = conn
                            .acp
                            .respond_checked(rpc_id.clone(), result, &self.hub.store.memory_boundary, || {
                                bindings::recheck(&self.hub, &execution).map(|_| ())
                            })
                            .await
                        {
                            let _ = conn.acp.respond_error(rpc_id, -32602, &error.to_string()).await;
                        }
                    }
                    return;
                }
            }
            crate::fork::rules::gate::GateOutcome::Deny => {
                let result =
                    options.iter().find(|o| o["kind"] == "reject_once" && o["optionId"].is_string()).map_or_else(
                        || json!({"outcome": {"outcome": "cancelled"}}),
                        |o| json!({"outcome": {"outcome": "selected", "optionId": o["optionId"]}}),
                    );
                if let Some(conn) = &self.conn
                    && let Err(error) = conn
                        .acp
                        .respond_checked(rpc_id.clone(), result, &self.hub.store.memory_boundary, || {
                            bindings::recheck(&self.hub, &execution).map(|_| ())
                        })
                        .await
                {
                    let _ = conn.acp.respond_error(rpc_id, -32602, &error.to_string()).await;
                }
                return;
            }
            crate::fork::rules::gate::GateOutcome::Ask => {}
        }
        let memory_server = codex
            && crate::fork::permission_card::memory_tool_request(
                &params,
                recorded.as_ref(),
                self.turn,
                &self.lane.chat,
            );
        let title = crate::fork::permission_card::action_title(tool, recorded.as_ref().map(|entry| &entry.data));
        let mut detail = json!({"output": "", "diffs": []});
        merge_tool_content(&mut detail, &tool["content"]);
        let raw = &tool["rawInput"];
        let command = raw["command"].as_str().map(str::to_owned).or_else(|| {
            raw["command"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "))
        });
        let data = json!({
            "title": title,
            "toolKind": if memory_server { "mcp_memory" } else { tool["kind"].as_str().unwrap_or("other") },
            "voiceApproval": super::voice_approval::policy(self.conn.as_ref().map(|c| c.adapter), tool),
            "command": command,
            "detail": detail["output"],
            "diffs": detail["diffs"],
            "cwd": self.cfg.cwd,
            "options": options.iter().map(|o| json!({"optionId": o["optionId"], "name": o["name"], "kind": o["kind"]})).collect::<Vec<_>>(),
            "status": "pending",
            "selected": Value::Null,
        });
        if let Err(error) = self.show_card(
            &title,
            data,
            Answer::Agent(rpc_id.clone()),
            execution,
            PolicyRequest {
                params: params.clone(),
                recorded: recorded.as_ref().map(|entry| entry.data.clone()),
                adapter: self.conn.as_ref().map_or(super::Adapter::Other, |c| c.adapter),
                origin: if self.turn_text.is_some() {
                    crate::fork::rules::avaliacao::Origem::DonoDigitadoAutenticado
                } else {
                    crate::fork::rules::avaliacao::Origem::Derivada
                },
            },
        ) && let Some(conn) = &self.conn
        {
            let _ = conn.acp.respond_error(rpc_id, -32602, &error.to_string()).await;
        }
    }

    /// `Cmd::Approve`: Allow or Deny on a card, with what the tool will do in its details.
    pub(super) fn ask_approval(
        &mut self,
        title: &str,
        detail: &str,
        execution: ExecutionRef,
        reply: oneshot::Sender<bool>,
    ) {
        if self.turn.is_none() || self.waiting_for_reset() {
            return; // dropping `reply` refuses: no turn waits on it
        }
        let data = json!({
            "title": title, "toolKind": "other", "voiceApproval": "app_only", "command": Value::Null, "detail": detail, "diffs": [],
            "options": [{"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                        {"optionId": "deny", "name": "Deny", "kind": "reject_once"}],
            "status": "pending", "selected": Value::Null,
        });
        let _ = self.show_card(
            title,
            data,
            Answer::Codync(reply),
            execution,
            PolicyRequest {
                params: json!({"toolCall": {"kind": "other", "title": title, "rawInput": {"command": detail}}}),
                recorded: None,
                adapter: super::Adapter::Other,
                origin: crate::fork::rules::avaliacao::Origem::Derivada,
            },
        );
    }

    /// Posts an approval card in the turn's lane and tells the user it needs them.
    fn show_card(
        &mut self,
        title: &str,
        data: Value,
        answer: Answer,
        execution: ExecutionRef,
        policy: PolicyRequest,
    ) -> anyhow::Result<()> {
        #[cfg(test)]
        crate::fork::sessoes::call_tests::before_effect(&self.hub.store, "permission_card_origin");
        let hub = self.hub.clone();
        let _boundary = hub.store.memory_boundary.locked();
        bindings::recheck(&hub, &execution)?;
        let pedido = self
            .active_request
            .as_ref()
            .map(crate::chat::team::BotRequest::permission_checkpoint)
            .transpose()?
            .map(|p| (p.id.to_owned(), p.fence));
        let turn = self.turn.unwrap_or(0);
        self.flush_in_execution(true, Some(&execution))?;
        self.seg = Seg::None;
        let options = data["options"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|o| o["optionId"].as_str().map(str::to_owned))
            .collect();
        // Fail-closed na classificação: só `reject*` dispensa revalidação; qualquer
        // outro kind (inclusive ausente ou desconhecido) é tratado como aprovação.
        let reject_options = data["options"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|o| o["kind"].as_str().is_some_and(|kind| kind.starts_with("reject")))
            .filter_map(|o| o["optionId"].as_str().map(str::to_owned))
            .collect();
        let mut data = data;
        data["author"] = json!(self.cfg.id);
        let pedido = pedido.as_ref().map(|(id, fence)| crate::fork::delegacao::pausa::Pedido { id, fence: *fence });
        let e = hub.add_permission_card(&self.cfg, &self.lane, turn, &data, &execution, pedido.as_ref())?;
        let context = crate::fork::permission_card::PendingContext::capture(&e, self.turn_session.as_deref());
        self.perms.insert(e.id, Pending { answer, options, context, execution, policy, reject_options });
        self.hub.set_runtime(&self.cfg.id, |r| {
            r.status = BotStatus::NeedsInput;
            r.set_activity(
                format!("Needs approval: {title}"),
                Some(ActivityKind::NeedsApproval { title: title.to_owned() }),
            );
        });
        // Tapping it opens where the card is: the group, for a room turn.
        let target = self.hub.store.bot(&self.lane.chat).ok().flatten().map_or_else(|| self.cfg.clone(), |r| r.config);
        push::notify(
            &self.hub,
            &target,
            Some(&self.cfg.id),
            &Titulo::Precisa(&self.cfg.name),
            &Corpo::Texto(title),
            AlertKind::NeedsInput,
        );
        Ok(())
    }

    /// A Codync card whose tool stopped waiting (it timed out, or its call was dropped) expires, buttons and all.
    pub(super) async fn expire_abandoned(&mut self) {
        let gone: Vec<String> = self
            .perms
            .iter()
            .filter(|(_, pending)| matches!(&pending.answer, Answer::Codync(reply) if reply.is_closed()))
            .map(|(id, _)| id.clone())
            .collect();
        for id in gone {
            let _ = self.answer_permission(&id, None).await;
        }
    }

    /// Resposta interna do Hub (expiração, cancelamento), fora de pedido.
    pub(super) async fn answer_permission(&mut self, entry_id: &str, option_id: Option<String>) -> anyhow::Result<()> {
        self.answer_permission_from(entry_id, option_id, None).await
    }

    pub(super) async fn answer_permission_from(
        &mut self,
        entry_id: &str,
        mut option_id: Option<String>,
        origem: Option<&crate::fork::donna_fixa::pedido::Origem>,
    ) -> anyhow::Result<()> {
        let Some(pending) = self.perms.get(entry_id) else {
            if self
                .hub
                .store
                .entry(entry_id)
                .is_some_and(|e| crate::agent::engine_permissions::repeated_answer(&e, option_id.as_deref()))
            {
                return Ok(());
            }
            bail!("Este cartão não está mais pendente. Confira a conversa antes de responder.");
        };
        if option_id.is_some() {
            let entry = self.hub.store.entry(entry_id).ok_or_else(|| anyhow::anyhow!("Cartão não encontrado."))?;
            pending.context.check(&entry, &self.cfg.id, &self.lane, self.turn, self.turn_session.as_deref())?;
            bindings::recheck(&self.hub, &pending.execution)?;
            if self.cfg.context.as_ref().is_some_and(|c| !c.foreign_identity) {
                let original =
                    self.hub.store.bot(&self.cfg.id)?.ok_or_else(|| anyhow::anyhow!("bot_not_found"))?.config;
                crate::fork::spaces::tool_context::card_config(&self.hub, &entry, original)?;
            }
        }
        if option_id.as_ref().is_some_and(|id| !pending.options.contains(id)) {
            bail!("Opção inválida: escolha uma das opções disponíveis neste cartão.");
        }
        let approving = option_id.as_ref().is_some_and(|id| !pending.reject_options.contains(id));
        let policy_error = if approving {
            let _boundary = self.hub.store.memory_boundary.locked();
            pending.policy.check(&self.hub.store, &self.cfg.id, self.cfg.permission).err()
        } else {
            None
        };
        if policy_error.is_some() {
            option_id = None;
        }
        let Some(pending) = self.perms.remove(entry_id) else { return Ok(()) };
        let mut status = if policy_error.is_some() {
            "expired"
        } else if option_id.is_some() {
            "answered"
        } else {
            "cancelled"
        };
        let mut entry = self.hub.store.entry(entry_id).ok_or_else(|| anyhow::anyhow!("Cartão não encontrado."))?;
        entry.data["status"] = json!(status);
        entry.data["selected"] = json!(option_id);
        let execution = option_id.as_ref().map(|_| &pending.execution);
        let committed = crate::fork::permission_card::commit(&self.hub, entry_id, &entry.data, execution, origem);
        let confirmed = match committed {
            Ok(Some(confirmed)) => confirmed,
            // Guarda recusou: nada gravado; a espera continua para quem ainda tem autoridade.
            Err(error) if error.is::<crate::fork::autoridade::AutorizacaoEncerrada>() => {
                self.perms.insert(entry_id.to_owned(), pending);
                return Err(error);
            }
            Ok(None) | Err(_) => anyhow::bail!("chamada vencida: não foi possível confirmar o cartão"),
        };
        let answered = option_id.as_deref().map(|option| pending.context.answered(&confirmed, option)).transpose()?;
        let contextual = answered.filter(|_| self.cfg.context.as_ref().is_some_and(|c| !c.foreign_identity));
        let mut delivery_error = policy_error;
        match pending.answer {
            Answer::Agent(rpc_id) => {
                let outcome = match &option_id {
                    Some(o) => json!({"outcome": {"outcome": "selected", "optionId": o}}),
                    None => json!({"outcome": {"outcome": "cancelled"}}),
                };
                if let Some(conn) = &mut self.conn {
                    let (hub, cfg, lane, turn, session, card_id) = (
                        self.hub.clone(),
                        self.cfg.clone(),
                        self.lane.clone(),
                        self.turn,
                        self.turn_session.clone(),
                        entry_id.to_owned(),
                    );
                    let acp = conn.acp.clone();
                    let needs_policy = approving && option_id.is_some();
                    let policy = pending.policy;
                    let execution = pending.execution;
                    let mut response = Box::pin(async move {
                        acp.respond_checked(rpc_id, outcome, &hub.store.memory_boundary, || {
                            if needs_policy {
                                policy.check(&hub.store, &cfg.id, cfg.permission)?;
                            }
                            if let Some(contextual) = &contextual {
                                bindings::recheck(&hub, &execution)?;
                                check_context_permission(
                                    &hub,
                                    &cfg,
                                    &lane,
                                    turn,
                                    session.as_deref(),
                                    &card_id,
                                    contextual,
                                )?;
                            }
                            Ok(())
                        })
                        .await
                    });
                    let result = if conn.skill_reads.is_empty() {
                        Some(response.await)
                    } else {
                        match futures::poll!(response.as_mut()) {
                            std::task::Poll::Ready(result) => Some(result),
                            std::task::Poll::Pending => {
                                // Conserva a ordem FIFO e revalida na conclusão do frame.
                                let hub = self.hub.clone();
                                let card_id = entry_id.to_owned();
                                conn.skill_reads.spawn(async move {
                                    if let Err(error) = response.await {
                                        mark_delivery_error(&hub, &card_id, &error);
                                    }
                                });
                                None
                            }
                        }
                    };
                    if let Some(Err(error)) = result {
                        status =
                            if error.to_string().contains("política atual recusou") { "expired" } else { "cancelled" };
                        option_id = None;
                        delivery_error = Some(error);
                    }
                }
            }
            Answer::Codync(reply) => {
                // The tool stopped waiting (see `expire_abandoned`): nothing acts on the answer.
                let validation = {
                    let _boundary = self.hub.store.memory_boundary.locked();
                    (|| -> anyhow::Result<()> {
                        if approving && option_id.is_some() {
                            pending.policy.check(&self.hub.store, &self.cfg.id, self.cfg.permission)?;
                        }
                        if let Some(contextual) = &contextual {
                            bindings::recheck(&self.hub, &pending.execution)?;
                            check_context_permission(
                                &self.hub,
                                &self.cfg,
                                &self.lane,
                                self.turn,
                                self.turn_session.as_deref(),
                                entry_id,
                                contextual,
                            )?;
                        }
                        if reply.send(option_id.as_deref() == Some("allow")).is_err() {
                            status = "expired";
                        }
                        Ok(())
                    })()
                };
                if let Err(error) = validation {
                    status =
                        if error.to_string().contains("política atual recusou") { "expired" } else { "cancelled" };
                    option_id = None;
                    delivery_error = Some(error);
                }
            }
        }
        if (status == "expired" || delivery_error.is_some())
            && let Some(mut e) = self.hub.store.entry(entry_id)
        {
            e.data["status"] = status.into();
            e.data["selected"] = option_id.into();
            if let Some(error) = &delivery_error {
                e.data["detail"] = json!(format!("{}\n{}", error, e.data["detail"].as_str().unwrap_or_default()));
            }
            self.hub.set_entry(entry_id, &e.data);
        }
        if self.perms.is_empty() && self.turn.is_some() {
            self.hub.set_runtime(&self.cfg.id, |r| {
                r.status = BotStatus::Working;
                r.set_activity("Continuing…", Some(ActivityKind::Continuing));
            });
        }
        if let Some(error) = delivery_error {
            return Err(error);
        }
        Ok(())
    }

    /// A `send_message`: the user's next bubble, in the turn's lane, right away. Only in the
    /// bot's own chat, including independent messages; a room, an ask or a routine reads
    /// the turn's reply.
    pub(super) fn send_to_user(&mut self, text: String, execution: Option<&ExecutionRef>) -> anyhow::Result<()> {
        bindings::recheck_optional(&self.hub, execution)?;
        let Some(turn) = self.turn.filter(|_| !self.waiting_for_reset()) else { anyhow::bail!("no turn is running") };
        if self.active_request.as_ref().is_some_and(crate::chat::team::BotRequest::expects_reply) {
            anyhow::bail!(
                "send_message isn't available in an ask: return your answer to the requesting bot as your final reply"
            );
        }
        if self.active_group.is_some() || self.active_routine.is_some() {
            anyhow::bail!("send_message isn't available in this turn: write your answer as your reply");
        }
        self.close_seg();
        self.hub
            .add_entry_in_execution(
                &self.lane,
                EntryKind::Agent,
                turn,
                &json!({"text": text, "final": true,"author":self.cfg.id}),
                execution,
            )
            .ok_or_else(|| anyhow::anyhow!("couldn't save the message"))?;
        self.sent.push(text);
        self.set_activity("Working…", ActivityKind::Working);
        Ok(())
    }

    /// `react`: the emoji goes on the user's message this turn answers, or in a room on a message the bot was shown.
    pub(super) fn react(
        &mut self,
        emoji: &str,
        to: Option<&str>,
        execution: Option<&ExecutionRef>,
    ) -> anyhow::Result<()> {
        bindings::recheck_optional(&self.hub, execution)?;
        if self.turn.is_none() || self.waiting_for_reset() {
            anyhow::bail!("no turn is running");
        }
        let target = match (&self.active_group, self.turn_entries.last()) {
            (Some(g), _) => crate::chat::reactions::in_room(&self.hub, &g.lane, &self.cfg.id, to, g.shown)?,
            (None, Some(id)) => id.clone(),
            (None, None) => anyhow::bail!("react works only on a message you're answering in a chat or a channel"),
        };
        let in_chat = self.active_group.is_none();
        if crate::chat::reactions::reacted_lately(&self.hub, &self.lane, &self.cfg.id, &target)? {
            if in_chat {
                self.reacted = Reacted::Refused;
            }
            anyhow::bail!(
                "you reacted in this chat a moment ago and reactions stay rare: no reaction this time. If the message \
                 needs no words, end your turn by replying exactly \"(pass)\"; don't tell the user about this."
            );
        }
        self.hub.bot_react_in_execution(&target, &self.cfg.id, emoji, execution)?;
        if in_chat {
            self.reacted = Reacted::With(emoji.trim().to_owned());
        }
        Ok(())
    }

    // MARK: streaming text segments

    pub(super) fn append(&mut self, kind: SegKind, text: &str) {
        if text.is_empty() {
            return;
        }
        let same = matches!(&self.seg, Seg::Open { kind: k, .. } if *k == kind);
        if !same {
            self.close_seg();
            let (k, data) = match kind {
                SegKind::Text => (EntryKind::Agent, json!({"text": text, "final": false})),
                SegKind::Thought => (EntryKind::Thought, json!({"text": text})),
            };
            if let Some(e) = self.add(k, self.turn.unwrap_or(0), data) {
                if kind == SegKind::Text {
                    self.last_text = Some(e.id.clone());
                }
                self.seg =
                    Seg::Open { kind, entry_id: e.id, buf: text.to_owned(), flushed: Instant::now(), dirty: false };
            }
            return;
        }
        if let Seg::Open { buf, dirty, .. } = &mut self.seg {
            buf.push_str(text);
            *dirty = true;
        }
    }

    /// Persists streamed text at most every 300 ms (or immediately with `force`).
    pub(super) fn flush(&mut self, force: bool) {
        let _ = self.flush_in_execution(force, None);
    }

    fn flush_in_execution(&mut self, force: bool, execution: Option<&ExecutionRef>) -> anyhow::Result<()> {
        if let Seg::Open { kind, entry_id, buf, flushed, dirty } = &mut self.seg
            && *dirty
            && (force || flushed.elapsed() >= Duration::from_millis(300))
        {
            let data = match kind {
                SegKind::Text => json!({"text": buf, "final": false, "author": self.cfg.id}),
                SegKind::Thought => json!({"text": buf, "author": self.cfg.id}),
            };
            let written = self.hub.set_entry_in_execution(entry_id, &data, execution);
            if execution.is_some() && written.is_none() {
                bail!("chamada vencida: segmento não confirmado");
            }
            *flushed = Instant::now();
            *dirty = false;
        }
        Ok(())
    }

    pub(super) fn close_seg(&mut self) {
        self.flush(true);
        self.seg = Seg::None;
    }
}

/// Folds ACP tool-call `content` into `{output, diffs}` on an entry's data.
pub fn merge_tool_content(data: &mut Value, content: &Value) {
    let Some(items) = content.as_array() else { return };
    let mut out = String::new();
    let mut diffs = vec![];
    for item in items {
        match item["type"].as_str() {
            Some("diff") => {
                let path = item["path"].as_str().unwrap_or_default();
                let old = item["oldText"].as_str().unwrap_or_default();
                let new = item["newText"].as_str().unwrap_or_default();
                diffs.push(diff_summary(path, old, new));
            }
            Some("terminal") => {}
            _ => {
                let t = acp::content_text(item);
                if !t.is_empty() {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(&t);
                }
            }
        }
    }
    if !out.is_empty() {
        data["output"] = acp::truncate(&out, 6000).into();
    }
    if !diffs.is_empty() {
        data["diffs"] = diffs.into();
    }
}

/// Changed-region summary: trims the common prefix/suffix and reports the middle.
pub fn diff_summary(path: &str, old: &str, new: &str) -> Value {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..].iter().rev().zip(b[prefix..].iter().rev()).take_while(|(x, y)| x == y).count();
    let removed = &a[prefix..a.len() - suffix];
    let added = &b[prefix..b.len() - suffix];
    let mut unified = String::new();
    for (sign, lines) in [('-', removed), ('+', added)] {
        for l in lines {
            unified.push(sign);
            unified.push_str(l);
            unified.push('\n');
        }
    }
    json!({
        "path": path,
        "added": added.len(),
        "removed": removed.len(),
        "isNew": old.is_empty(),
        "startLine": prefix + 1,
        "patch": acp::truncate(&unified, 6000),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_counts_changed_middle() {
        let d = diff_summary("f", "a\nb\nc\n", "a\nB\nB2\nc\n");
        assert_eq!(d["added"], 2);
        assert_eq!(d["removed"], 1);
        assert_eq!(d["startLine"], 2);
        assert_eq!(d["patch"], "-b\n+B\n+B2\n");
    }

    #[test]
    fn tool_content_merges_text_and_diffs() {
        let mut data = json!({"output": "", "diffs": []});
        merge_tool_content(
            &mut data,
            &json!([
                {"type": "content", "content": {"type": "text", "text": "ok"}},
                {"type": "diff", "path": "/a.rs", "oldText": "", "newText": "x\n"}
            ]),
        );
        assert_eq!(data["output"], "ok");
        assert_eq!(data["diffs"][0]["isNew"], true);
    }
}
