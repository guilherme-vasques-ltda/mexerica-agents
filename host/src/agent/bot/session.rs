// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! The bot's ACP sessions: starting its agent, new/load, routine runs' sessions.

use super::fast_mode::Listing;
use super::launch::{claude_meta, launch_commands, select_model, start_agent, start_mexerica, system_prompt};
use super::{Actor, Adapter, Conn, Instructions};
use crate::LockExt;
use crate::agent::acp::{self, Incoming};
use crate::agent::own_memory;
use crate::chat::{context, memory};
use crate::fork::activity_code::ActivityKind;
use crate::store::run_session_key;
use anyhow::{Context as _, Result, anyhow, bail};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

impl Actor {
    /// Which session the running turn uses: a routine run has one of its own (by its slot), and so does each group
    /// the bot is in (D229-6: what is said in a group never reaches the bot's own conversation, nor the other way
    /// around); everything else uses the main one.
    pub(super) fn slot(&self) -> Option<String> {
        self.active_routine
            .as_deref()
            .map(crate::routines::session_slot)
            .or_else(|| self.active_group.as_ref().map(|group| group_slot(&group.lane.chat)))
            .or_else(|| self.lane.session.as_deref().map(extra_slot))
            .or_else(|| crate::fork::spaces::session_slot(&self.cfg))
    }

    pub(super) fn session_for(&mut self, slot: Option<&str>) -> Option<String> {
        let Some(slot) = slot else { return self.session_id.clone() };
        if let Some(sid) = self.run_sessions.get(slot) {
            return Some(sid.clone());
        }
        let sid = self.hub.store.kv_get(&run_session_key(&self.cfg.id, slot));
        let sid = sid.filter(|s| !s.is_empty())?;
        self.run_sessions.insert(slot.to_owned(), sid.clone());
        Some(sid)
    }

    pub(super) fn set_session(&mut self, slot: Option<&str>, sid: Option<&str>) {
        if sid.is_none() {
            self.hub.pontes.revogar_slot(&self.cfg.id, slot);
        }
        if sid.is_none()
            && let Some(old) = self.session_for(slot)
            && let Err(error) = crate::fork::sessoes::bindings::revoke(&self.hub.store, &self.cfg.id, Some(&old))
        {
            tracing::error!(%error, "falha ao revogar vínculo de sessão");
            // Sem revogação persistida, o processo antigo não pode continuar recebendo ferramentas.
            self.stop_requested = true;
            return; // A revogação precisa confirmar antes de esquecer o slot.
        }
        let Some(slot) = slot else {
            self.session_id = sid.map(str::to_owned);
            if let Err(error) = self.hub.store.set_session(&self.cfg.id, sid) {
                tracing::warn!(bot = %self.cfg.id, error = format!("{error:#}"), "couldn't store the session");
            }
            return;
        };
        let key = run_session_key(&self.cfg.id, slot);
        let stored = if let Some(s) = sid {
            self.run_sessions.insert(slot.to_owned(), s.to_owned());
            self.hub.store.kv_set(&key, s)
        } else {
            self.run_sessions.remove(slot);
            self.hub.store.kv_delete(&key)
        };
        if let Err(error) = stored {
            tracing::warn!(bot = %self.cfg.id, error = format!("{error:#}"), "couldn't store the routine run's session");
        }
    }

    /// Ativa identidade e exposição juntas; falha impede o turno, como no vínculo visível do PR 2.
    fn activate_binding(&self, binding: &crate::fork::sessoes::bindings::BindingRef, sid: &str) -> Result<()> {
        crate::fork::sessoes::bindings::activate_for_config(&self.hub.store, binding, sid, &self.cfg)
            .inspect_err(|_| self.hub.set_runtime(&self.cfg.id, |runtime| runtime.memory_exposure_error = true))
            .context("não foi possível ativar o vínculo da sessão")
    }

    /// Slots of the routine runs that have a session (stored or loaded).
    pub(super) fn run_slots(&self) -> Vec<String> {
        let prefix = run_session_key(&self.cfg.id, "");
        let mut slots: Vec<String> = self.run_sessions.keys().cloned().collect();
        slots.extend(
            self.hub.store.kv_prefix(&prefix).into_iter().filter_map(|k| k.strip_prefix(&prefix).map(str::to_owned)),
        );
        slots.sort();
        slots.dedup();
        slots
    }

    /// Session workspace and MCP servers. Memory is readable without writable additionalDirectories.
    fn session_params(&self, servers: &Value, session: Option<&str>) -> Value {
        let mut params = json!({"cwd": self.cfg.cwd, "mcpServers": servers});
        crate::fork::native_scheduling::apply_session(&mut params, &self.cfg.backend);
        if let Some(sid) = session {
            params["sessionId"] = sid.into();
        }
        params
    }

    /// ACP connectors, team collaboration, and optional team management and computer control.
    /// `slot` is the session the servers are for (`None`: the main one); each internal server carries it, so a call
    /// from a group's or a routine's session is told apart from the owner's own conversation.
    pub(super) fn mcp_servers(
        &self,
        slot: Option<&str>,
        binding: &crate::fork::sessoes::bindings::BindingRef,
    ) -> Result<(Value, crate::fork::conectores::Definitions)> {
        let catchup = self.active_routine.as_deref().is_some_and(|id| self.hub.routines.run_is_catchup(id));
        let offered = crate::fork::spaces::tool_servers::decide(
            &self.hub.store,
            &self.cfg,
            self.active_routine.is_some(),
            catchup,
        );
        let Ok(exe) = std::env::current_exe() else {
            tracing::warn!("can't locate donnabot-host for MCP servers");
            return Ok((json!([]), crate::fork::conectores::Definitions::default()));
        };
        let port = self.hub.port;
        let tool_binding = self.cfg.context.as_ref().map(|_| uuid::Uuid::new_v4().to_string());
        let mut servers: Vec<Value> =
            (if offered.connectors.is_empty() { Vec::new() } else { crate::market::connectors(&self.hub.store)? })
                .iter()
                // Marketplace: conta de outro Space nunca vira servidor MCP deste bot, mesmo que a lista a cite.
                .filter(|c| offered.connectors.contains(&c.id) && c.space == self.cfg.space_id)
                .map(|c| {
                    crate::fork::spaces::connector_context::definition(
                        &self.hub,
                        &self.cfg,
                        tool_binding.as_deref(),
                        c,
                        &exe,
                        port,
                        &self.cfg.id,
                        &self.lane.chat,
                        self.active_routine.as_deref(),
                        binding.id.as_str(),
                    )
                })
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect();
        let curated = if offered.curated {
            crate::fork::conectores::definitions(
                &self.hub,
                &self.cfg.id,
                &exe,
                &self.lane,
                &self.conn.as_ref().ok_or_else(|| anyhow!("agent not running"))?.curated,
            )?
        } else {
            crate::fork::conectores::Definitions::default()
        };
        servers.extend(curated.servers.iter().cloned());
        for name in offered.builtin {
            let mut definition = json!({
                "name": name, "command": exe,
                "args": ["mcp", name, "--bot", self.cfg.id, "--port", port.to_string()],
                "env": [
                    {"name":"DONNABOT_SPACE_ID", "value":self.cfg.space_id},
                    {"name":crate::guard_hub::tool_origin::SESSION_SLOT_ENV,
                        "value":crate::guard_hub::tool_origin::slot_value(slot)},
                ],
            });
            if let Some(binding) = &tool_binding {
                definition["env"]
                    .as_array_mut()
                    .ok_or_else(|| anyhow!("invalid_mcp_environment"))?
                    .push(json!({"name":crate::fork::spaces::tool_context::BINDING_ENV,"value":binding}));
            }
            servers.push(definition);
            if name == "memory" {
                // Leitura sem aquecer o cache: a definição não altera a sessão do ator.
                let session = match slot {
                    None => self.session_id.clone(),
                    Some(slot) => self
                        .run_sessions
                        .get(slot)
                        .cloned()
                        .or_else(|| self.hub.store.kv_get(&run_session_key(&self.cfg.id, slot)))
                        .filter(|sid| !sid.is_empty()),
                };
                let capability = crate::fork::memory_auto::transport::issue(
                    &self.hub,
                    &self.cfg,
                    &self.lane.chat,
                    slot,
                    session.as_deref(),
                )?;
                if let Some(env) = servers.last_mut().and_then(|s| s["env"].as_array_mut()) {
                    env.push(json!({"name":crate::fork::memory_auto::transport::ENV,"value":capability}));
                }
            }
            if let Some(env) = servers.last_mut().and_then(|s| s["env"].as_array_mut()) {
                env.push(json!({"name":crate::fork::sessoes::bindings::ENV,"value":binding.id.as_str()}));
            }
        }
        crate::fork::sessoes::contexto_ferramenta::issue(&self.hub, &self.cfg, slot, binding, &mut servers)?;
        crate::fork::ponte_credencial::provisionar(&self.hub, &self.cfg.id, slot, binding, &mut servers)?;
        let servers = Value::Array(servers);
        crate::fork::spaces::tool_context::prepare(&self.hub, &self.cfg, &servers)?;
        Ok((servers, curated))
    }

    /// Makes sure the agent runs and `slot`'s session (main, or a routine run's) is live;
    /// sets `turn_session`.
    pub(super) async fn ensure_session(&mut self, slot: Option<&str>) -> Result<()> {
        let result = self.ensure_session_inner(slot).await.and_then(|()| {
            let conn = self.conn.as_ref().ok_or_else(|| anyhow!("agent not running"))?;
            let sid = self.turn_session.as_deref().ok_or_else(|| anyhow!("no session"))?;
            conn.curated.check(&self.hub, sid)
        });
        if result.is_err() {
            self.hub.pontes.revogar_slot(&self.cfg.id, slot);
        }
        if result.is_err() && self.conn.as_ref().is_some_and(|conn| conn.curated.failed()) {
            // Setup parcial ou transporte perdido não pode deixar loaded com ferramentas inválidas.
            if let Some(conn) = self.conn.take() {
                conn.kill().await;
            }
        } else if let Some(conn) = &self.conn {
            conn.curated.clear_failed();
        }
        result
    }

    async fn ensure_session_inner(&mut self, slot: Option<&str>) -> Result<()> {
        if self.cfg.context.is_some() || self.cfg.space_id != crate::fork::spaces::PERSONAL {
            anyhow::ensure!(self.cfg.fork_editor.engine_account_id.is_some(), "space_account_selection_required");
        }
        if self.cfg.context.is_some() {
            crate::fork::spaces::execution_stamp(&self.hub.store, &self.cfg)?;
        }
        crate::fork::spaces::prepare_workspace(&self.cfg)?;
        memory::engine::check_workspace(&self.cfg)?;
        let selected = crate::fork::engine_accounts::managed::resolve_for_bot(&self.hub.store, &self.cfg)?;
        let revision = selected.as_ref().map(|launch| launch.revision);
        // Modelo de um plano de código (chave colada): perfil e chave próprios do plano, em vez do ambiente do dono.
        let plano = if selected.is_none() {
            crate::fork::planos_codigo::ambiente_do_bot(&self.hub, &self.cfg).await?
        } else {
            None
        };
        let login_profile = match &plano {
            Some(plano) => plano.carimbo.clone(),
            None if selected.is_none() => {
                crate::fork::terminal_login::stamp(&self.hub.store, &self.cfg.backend, &self.cfg.space_id)?
            }
            None => String::new(),
        };
        if self.cfg.context.is_some() {
            let key = format!("donna.space.engine.{}", self.cfg.session_owner());
            let mut stamp = json!({"account":self.cfg.fork_editor.engine_account_id,"revision":revision,"backend":self.cfg.backend,"model":self.cfg.model,"modelOptions":self.cfg.model_options});
            if !login_profile.is_empty() {
                stamp["loginProfile"] = login_profile.clone().into();
            }
            let stamp = stamp.to_string();
            let previous = self.hub.store.kv_read(&key)?;
            if previous.as_deref() != Some(&stamp) {
                if let Some(conn) = self.conn.take() {
                    conn.kill().await;
                }
                self.set_session(slot, None);
                self.hub.store.kv_set(&key, &stamp)?;
            }
            self.account_revision = revision;
        } else {
            // Conta, revisão e motor com que as sessões do bot foram abertas, guardados entre reinícios (D229-6). Só o
            // número da revisão não basta: contas diferentes começam com a mesma, e a seleção pode ter sido salva sem
            // que o ator a recebesse antes de um reinício.
            let mut stamp = account_stamp(&self.cfg, revision);
            if !login_profile.is_empty() {
                stamp.push(':');
                stamp.push_str(&login_profile);
            }
            // Uma sessão extra tem carimbo próprio (`account.stamp.<bot>/session:<id>`), porque pode ter escolha
            // fixada diferente do Fixo atual do bot (PR4): trocar uma sessão não invalida as demais. D372 (A): a
            // principal com seleção efetiva própria também, no mesmo domínio (`.../session:<bot>`); rotina e canal
            // seguem o carimbo do bot. Na primeira vez, compara com o carimbo do bot, com que a ACP pode ter sido aberta.
            let extra = slot.filter(|slot| slot.starts_with(EXTRA_SLOT));
            let principal_own =
                crate::fork::sessoes::admissao::principal_owns_stamp(&self.hub.store.db.locked(), &self.cfg)?;
            let own_slot = extra
                .map(str::to_owned)
                .or_else(|| (slot.is_none() && principal_own).then(|| extra_slot(&self.cfg.id)));
            let key = own_slot.as_deref().map_or_else(
                || account_stamp_key(&self.cfg.id),
                |slot| format!("{}/{slot}", account_stamp_key(&self.cfg.id)),
            );
            let own = self.hub.store.kv_read(&key)?;
            let previous = match &own {
                None if own_slot.is_some() => self.hub.store.kv_read(&account_stamp_key(&self.cfg.id))?,
                own => own.clone(),
            };
            // Sem carimbo guardado e sem conta gerenciada não há sessão de outra conta a invalidar, como antes.
            let changed = previous.as_deref().map_or(selected.is_some(), |previous| previous != stamp);
            if changed {
                if let Some(conn) = self.conn.take() {
                    conn.kill().await;
                }
                if let Some(slot) = extra {
                    // A sessão visível continua a mesma: a ACP nova leva o resumo só dela.
                    if let Some(old) = self.session_for(Some(slot)) {
                        self.carry_extra_handoff(Some(slot), &old)?;
                    }
                    self.set_session(Some(slot), None);
                } else if own_slot.is_some() {
                    // Só a principal mudou: rotina e canal conservam as sessões deles.
                    self.forget_session();
                } else {
                    for slot in self.run_slots() {
                        if !slot.starts_with(EXTRA_SLOT) {
                            self.set_session(Some(&slot), None);
                        }
                    }
                    // A principal com seleção própria não pertence ao carimbo do bot: conserva a sessão dela.
                    if !principal_own {
                        self.forget_session();
                    }
                }
            }
            if own.as_deref() != Some(stamp.as_str())
                && let Err(error) = self.hub.store.kv_set(&key, &stamp)
            {
                tracing::warn!(bot = %self.cfg.id, error = format!("{error:#}"), "couldn't store the account stamp");
            }
            self.account_revision = revision;
        }
        if !self.cfg.connectors.is_empty()
            || (selected.is_none() && self.hub.store.kv_read(&format!("agent-env:{}", self.cfg.backend))?.is_some())
        {
            crate::market::vault::unlock(self.hub.clone()).await?;
        }
        if crate::fork::spaces::tool_context::needs_rebind(&self.hub, &self.cfg)?
            && let Some(conn) = self.conn.take()
        {
            conn.kill().await;
        }
        // An agent that can't resume keeps its session; its next new session gets the new tools.
        if std::mem::take(&mut self.tools_changed)
            && self.conn.as_ref().is_some_and(|c| c.sessions.load)
            && let Some(c) = self.conn.take()
        {
            c.kill().await;
        }
        if self.conn.is_none() {
            self.hub.pontes.revogar_bot(&self.cfg.id);
            crate::fork::sessoes::bindings::revoke(&self.hub.store, &self.cfg.id, None)?;
            let (hub, id) = (self.hub.clone(), self.cfg.id.clone());
            // O motor interno não tem candidatos de comando: o adaptador é em processo.
            let interno = self.cfg.backend == crate::fork::motor_mexerica::ENGINE_ID;
            let mut candidates = if interno {
                Vec::new()
            } else {
                launch_commands(&self.cfg, move |msg: &str, kind| {
                    let msg = msg.to_owned();
                    hub.set_runtime(&id, |r| r.set_activity(msg, kind));
                })
                .await?
            };
            self.hub.set_runtime(&self.id(), |r| r.set_activity("Starting agent…", Some(ActivityKind::StartingAgent)));
            // Keys saved from an "environment variable" sign-in. O motor
            // interno lê a chave do cofre pelo canal fd 3, nunca do ambiente.
            let mut env = if selected.is_some() || interno || plano.is_some() {
                Vec::new()
            } else {
                crate::agent::auth::env(&self.hub.store, &self.cfg.backend)?
            };
            if selected.is_some() && self.cfg.backend == "codex" {
                env.push(("CODEX_CONFIG".into(), "{}".into()));
            }
            own_memory::apply(&self.cfg, &mut env, &mut candidates).await;
            crate::fork::planos_codigo::aplicar_ambiente(
                &mut env,
                &self.cfg.backend,
                &crate::agent::harness::compile(),
                plano.as_ref(),
            );
            if selected.is_none() && plano.is_none() {
                env.extend(crate::fork::terminal_login::runtime_env(
                    &self.hub.store,
                    &self.cfg.backend,
                    &self.cfg.space_id,
                )?);
            }
            // Referência validada pelo Hub por último, depois dos filtros e do harness.
            if let Some(launch) = &selected {
                env.extend(launch.env.clone());
            }
            let mut last_err = None;
            let mut spawned = None;
            if interno {
                match start_mexerica(&self.hub, &[]).await {
                    Ok(conn) => spawned = Some(conn),
                    Err(e) => last_err = Some(e),
                }
            } else {
                for (i, command) in candidates.iter().enumerate() {
                    let fallback_left = i + 1 < candidates.len();
                    // A local CLI too old for ACP just hangs; don't make the user wait long before the fallback.
                    let budget = Duration::from_secs(if fallback_left { 20 } else { 180 });
                    match start_agent(command, &self.cfg.cwd, &env, budget).await {
                        Ok(conn) => {
                            spawned = Some(conn);
                            break;
                        }
                        Err(e) => {
                            tracing::info!(command, error = format!("{e:#}"), "agent failed to start");
                            last_err = Some(e);
                        }
                    }
                }
            }
            if let Some(mut conn) = spawned {
                let revogador = crate::fork::ponte_credencial::Revogador::new(&self.hub, &self.cfg.id);
                let _ = conn.ponte.replace(revogador);
                let backend = match conn.adapter {
                    Adapter::Codex => "codex",
                    Adapter::Claude => "claude",
                    Adapter::Other | Adapter::Mexerica => self.cfg.backend.as_str(),
                };
                crate::fork::memory_auto::transport::new_generation(&self.hub, &self.cfg, &self.lane.chat, backend)?;
                crate::agent::codex_audit::publish_main(&self.hub, &self.cfg.id, &conn.config_warnings);
                self.conn = Some(conn);
            }
            if self.conn.is_none() {
                return Err(last_err.unwrap_or_else(|| anyhow!("agent failed to start")));
            }
        }
        let acp = self.session_for(slot);
        if !self.hub.pontes.tem_slot(&self.cfg.id, slot)
            && let (Some(conn), Some(sid)) = (self.conn.as_mut(), acp.as_deref())
        {
            conn.loaded.remove(sid);
        }
        let loaded = acp.as_ref().is_some_and(|sid| self.conn.as_ref().is_some_and(|c| c.loaded.contains_key(sid)));
        let visible = crate::fork::sessoes::exposure::visible(self.cfg.session_owner(), slot);
        let binding = if loaded {
            let binding = crate::fork::sessoes::bindings::for_acp(
                &self.hub.store,
                &self.cfg.id,
                acp.as_deref().unwrap_or_default(),
            )?;
            anyhow::ensure!(
                binding.session_ref.id == visible && binding.session_ref.chat_id == self.lane.chat,
                "chamada vencida: slot de outra sessão"
            );
            binding
        } else if let Some(binding) = self.pending_profile_binding()? {
            binding
        } else {
            if let Some(sid) = acp.as_deref() {
                crate::fork::sessoes::bindings::revoke(&self.hub.store, &self.cfg.id, Some(sid))?;
            }
            crate::fork::sessoes::bindings::issue_in(
                &self.hub.store,
                &self.cfg.id,
                &self.lane.chat,
                &visible,
                slot,
                None,
            )?
        };
        let instructions = self.conn.as_ref().map_or(Instructions::FirstMessage, Conn::instructions);
        let mut rebuild_history = self.cfg.context.is_some();
        let current = self.session_for(slot);
        let current = if let Some(sid) = current {
            let loaded = self.conn.as_ref().is_some_and(|c| c.loaded.contains_key(&sid));
            if !loaded && crate::fork::spaces::needs_history_rebuild(&self.hub.store, &self.cfg, &sid)? {
                rebuild_history = true;
                self.set_session(slot, None);
                None
            } else {
                Some(sid)
            }
        } else {
            None
        };
        let Some(conn) = self.conn.as_ref() else { bail!("agent not running") };
        if let Some(sid) = current {
            crate::fork::spaces::bind_session(
                &self.hub.store,
                &self.cfg.id,
                self.cfg.context.as_ref().map_or(self.cfg.id.as_str(), |c| c.conversation_id.as_str()),
                &sid,
            )?;
            // Também na retomada: uma ACP anterior à migração ou sem vínculo ganha o seu antes do turno.
            if conn.loaded.contains_key(&sid) {
                // Loaded só revalida as definições/handles originais, sem reprovisioná-los.
                crate::fork::memory_auto::transport::assert_bound(&self.hub, &self.cfg, &self.lane.chat, slot, &sid)?;
                self.hub.prepare_session_binding(&binding)?;
                #[cfg(test)]
                crate::fork::sessoes::gateway_actor_tests::reused(&self.hub, &binding).await?;
                self.bind_profile_skills(&binding)?;
                self.activate_binding(&binding, &sid)?;
                self.complete_profile_skills(&binding, &sid)?;
                self.turn_session = Some(sid);
                self.turn_binding = Some(binding);
                return Ok(());
            }
            if !conn.sessions.load {
                tracing::info!(bot = %self.cfg.id, "the agent can't resume sessions; starting a fresh one");
                self.carry_extra_handoff(slot, &sid)?;
                self.set_session(slot, None);
                return Box::pin(self.ensure_session(slot)).await;
            }
            self.bind_profile_skills(&binding)?;
            self.hub.set_runtime(&self.cfg.id, |r| {
                r.set_activity("Resuming session…", Some(ActivityKind::ResumingSession));
            });
            // Read before anything below re-renders the snapshot: an older one means the agent never heard this profile.
            let predates = context::predates(&self.hub.store, self.cfg.session_owner(), &sid);
            self.hub.prepare_session_binding(&binding)?;
            let (servers, curated) = self.mcp_servers(slot, &binding)?;
            #[cfg(test)]
            crate::fork::sessoes::gateway_actor_tests::prepared(&self.hub, &servers).await?;
            let mut params = self.session_params(&servers, Some(&sid));
            if instructions == Instructions::ClaudeMeta {
                params["_meta"] = claude_meta(&crate::agent::harness::compile(), &self.cfg);
            }
            if instructions == Instructions::SystemPrompt {
                params["systemPrompt"] = system_prompt(&self.snapshot(&sid).await?.system);
            }
            let res = self.session_request("session/load", params).await;
            let Some(conn) = self.conn.as_mut() else { return res.map(drop) };
            // The agent replays history as session/update before answering; drop it.
            while let Ok(inc) = conn.rx.try_recv() {
                if let Incoming::Request { id, .. } = inc {
                    let _ = conn.acp.respond_error(id, -32601, "not supported").await;
                }
            }
            match res {
                Ok(res) => {
                    // A resumed session can come back on the agent's last-used model (Cursor
                    // keeps one for all its sessions), so the bot's own is set again.
                    let res = if res.get("configOptions").is_some() || res.get("models").is_some() {
                        select_model(&conn.acp, &res, &sid, &self.cfg).await?
                    } else {
                        res
                    };
                    let res = self.manual_mode(&sid, res).await?;
                    let Some(conn) = self.conn.as_mut() else { bail!("agent not running") };
                    conn.loaded.insert(sid.clone(), Instant::now());
                    self.note_options(&sid, &res, Listing::Session);
                    if predates {
                        self.profile_resend.insert(sid.clone());
                    }
                    crate::fork::memory_auto::transport::bind(
                        &self.hub,
                        &self.cfg,
                        &self.lane.chat,
                        slot,
                        &servers,
                        &sid,
                    )?;
                    self.activate_binding(&binding, &sid)?;
                    self.complete_profile_skills(&binding, &sid)?;
                    self.turn_session = Some(sid.clone());
                    self.turn_binding = Some(binding.clone());
                    self.remember_hub_tools(&servers, slot, &binding)?;
                    let curated = curated.bind(&self.hub, &sid)?;
                    self.conn.as_mut().ok_or_else(|| anyhow!("agent not running"))?.curated.insert(&sid, curated);
                    return Ok(());
                }
                Err(e) => {
                    if e.chain().filter_map(|cause| cause.downcast_ref::<acp::RpcError>()).any(|rpc| {
                        crate::agent::subscription::detect(
                            &self.cfg.backend,
                            rpc.kind.as_deref(),
                            &rpc.message,
                            &rpc.data,
                            crate::store::now_ms(),
                        )
                        .is_some()
                    }) {
                        return Err(e);
                    }
                    crate::fork::memory_auto::transport::revoke_slot(&self.hub, &self.cfg.id, slot)?;
                    tracing::warn!(bot = %self.cfg.id, error = format!("{e:#}"), "session/load failed; starting fresh");
                    self.carry_extra_handoff(slot, &sid)?;
                }
            }
        }
        let mut reconstruction_coverage = None;
        if self.turn_handoff(&self.lane).is_none() {
            let exclude = self.input.as_ref().map_or(&[][..], |i| i.entries.as_slice());
            if rebuild_history
                && let Some((prompt, coverage)) =
                    crate::fork::spaces::history_context(&self.hub.store, &self.cfg, exclude)?
            {
                self.handoff = Some(prompt);
                self.handoff_chat = Some(self.lane.clone());
                reconstruction_coverage = Some(coverage);
            }
        }
        self.bind_profile_skills(&binding)?;
        let (revision, rendered) = self.render_profile().await?;
        self.hub.prepare_session_binding(&binding)?;
        let (servers, curated) = self.mcp_servers(slot, &binding)?;
        #[cfg(test)]
        crate::fork::sessoes::gateway_actor_tests::prepared(&self.hub, &servers).await?;
        let mut params = self.session_params(&servers, None);
        match instructions {
            Instructions::ClaudeMeta => params["_meta"] = claude_meta(&crate::agent::harness::compile(), &self.cfg),
            Instructions::SystemPrompt => params["systemPrompt"] = system_prompt(&rendered.system),
            Instructions::FirstMessage | Instructions::CodexConfig => {}
        }
        let res = self.session_request("session/new", params).await?;
        let Some(conn) = self.conn.as_mut() else { bail!("agent not running") };
        let sid = res["sessionId"].as_str().ok_or_else(|| anyhow!("agent returned no sessionId"))?.to_owned();
        crate::fork::spaces::mark_context_sid(&self.hub.store, &self.cfg, &sid)?;
        crate::fork::spaces::bind_session(
            &self.hub.store,
            &self.cfg.id,
            self.cfg.context.as_ref().map_or(self.cfg.id.as_str(), |c| c.conversation_id.as_str()),
            &sid,
        )?;
        conn.loaded.insert(sid.clone(), Instant::now());
        let options = select_model(&conn.acp, &res, &sid, &self.cfg).await?;
        let options = self.manual_mode(&sid, options).await?;
        self.note_options(&sid, &options, Listing::Session);
        self.activate_binding(&binding, &sid)?;
        self.complete_profile_skills(&binding, &sid)?;
        self.set_session(slot, Some(&sid));
        self.session_fresh = true;
        crate::fork::memory_auto::transport::bind(&self.hub, &self.cfg, &self.lane.chat, slot, &servers, &sid)?;
        self.turn_session = Some(sid.clone());
        self.turn_binding = Some(binding.clone());
        self.remember_hub_tools(&servers, slot, &binding)?;
        context::adopt(&self.hub.store, &self.cfg, &sid, rendered, revision, instructions)?;
        crate::fork::spaces::continuity(&self.hub, &self.cfg, reconstruction_coverage.as_ref())?;
        let curated = curated.bind(&self.hub, &sid)?;
        self.conn.as_mut().ok_or_else(|| anyhow!("agent not running"))?.curated.insert(&sid, curated);
        Ok(())
    }

    fn remember_hub_tools(
        &mut self,
        servers: &Value,
        slot: Option<&str>,
        binding: &crate::fork::sessoes::bindings::BindingRef,
    ) -> Result<()> {
        if let Some(session) = &self.turn_session {
            crate::fork::spaces::tool_context::activate(&self.hub, &self.cfg, servers, session)?;
            crate::fork::sessoes::contexto_ferramenta::activate(&self.hub, servers, slot, binding, session)?;
            let servers = &crate::fork::sessoes::contexto_ferramenta::without_seal(servers);
            let known = std::env::current_exe().map_or_else(
                |_| std::collections::HashSet::new(),
                |exe| {
                    crate::guard_hub::tool_origin::internal_servers(
                        servers,
                        &exe,
                        &self.cfg.id,
                        self.hub.port,
                        &self.cfg.space_id,
                        slot,
                        crate::fork::spaces::tool_context::definition_binding(servers),
                        Some(binding.id.as_str()),
                    )
                },
            );
            self.hub_tool_servers.insert(session.clone(), known);
        }
        Ok(())
    }

    /// A session request the agent doesn't answer in time stops the agent: a hung one
    /// would otherwise hold the bot, and its Stop, forever.
    pub(super) async fn session_request(&mut self, method: &str, params: Value) -> Result<Value> {
        let acp = self.conn.as_ref().ok_or_else(|| anyhow!("agent not running"))?.acp.clone();
        if let Ok(result) = tokio::time::timeout(SESSION_TIMEOUT, acp.request(method, params)).await {
            return result;
        }
        if let Some(c) = self.conn.take() {
            c.kill().await;
        }
        let message = format!("the agent didn't answer {method} within {}s", SESSION_TIMEOUT.as_secs());
        Err(acp::Failure::error(acp::FailureKind::Timeout, message))
    }

    /// Troca de ACP de uma sessão extra sem `session/load`: a ACP nova começa com o resumo só dessa sessão visível,
    /// e a sessão visível continua a mesma. A principal tem caminho próprio (rollover e espera, `turn.rs`).
    fn carry_extra_handoff(&mut self, slot: Option<&str>, acp: &str) -> Result<()> {
        let Some(visible) = slot.and_then(|slot| slot.strip_prefix(EXTRA_SLOT)) else { return Ok(()) };
        if self.turn_handoff(&self.lane).is_none() {
            let (text, person) =
                crate::chat::handoff::build_in_with_person(&self.hub.store, &self.cfg.id, visible, acp)?;
            if person.is_some() && person == self.cfg.person {
                self.handoff = Some(text);
                self.handoff_chat = Some(self.lane.clone());
            }
        }
        Ok(())
    }

    /// Drops the main ACP session so the next turn starts a fresh one.
    pub(super) fn forget_session(&mut self) {
        let slot = crate::fork::spaces::session_slot(&self.cfg);
        self.set_session(slot.as_deref(), None);
    }
}

fn account_stamp_key(bot: &str) -> String {
    format!("account.stamp.{bot}")
}

/// The managed account the bot's sessions were opened with: account id, its revision and the backend.
fn account_stamp(cfg: &crate::store::BotConfig, revision: Option<u64>) -> String {
    json!({"account": cfg.fork_editor.engine_account_id, "revision": revision, "backend": cfg.backend}).to_string()
}

pub(super) const EXTRA_SLOT: &str = "session:";

/// An extra session's ACP slot: `session:<id>`, stored like a routine run's (`fork::sessoes::exposure::visible`).
fn extra_slot(session: &str) -> String {
    format!("{EXTRA_SLOT}{session}")
}

/// The member's own session for a group: `group:<group id>`, stored like a routine run's.
fn group_slot(group: &str) -> String {
    format!("group:{group}")
}

/// How long `session/new` and `session/load` may take.
const SESSION_TIMEOUT: Duration = Duration::from_secs(180);
