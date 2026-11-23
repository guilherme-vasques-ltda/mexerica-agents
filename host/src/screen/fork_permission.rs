//! D511: metadados de espera e concessão temporária; nunca guarda ações de tela.
use super::Screen;
use crate::{
    LockExt,
    fork::{
        donna_fixa::pedido::Origem,
        sessoes::bindings::{self, ExecutionRef},
        tela_por_computador as access,
    },
    hub::{BotStatus, Hub, Runtime},
    store::{Entry, Store, now_ms},
};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

pub(crate) const WAIT: Duration = Duration::from_secs(270);
const MAX_NOW: Duration = Duration::from_secs(900);
const ACCOUNT_APP: &str = "Para liberar a tela, abra o app conectado à sua conta.";

#[derive(Default)]
pub(super) struct State {
    pending: HashMap<String, Pending>,
    grants: HashMap<String, Grant>,
}
struct Pending {
    bot: String,
    execution: ExecutionRef,
    runtime: Runtime,
    generation: i64,
    waiters: HashMap<String, tokio::time::Instant>,
    deadline: Instant,
}
#[derive(Clone)]
pub(crate) struct Grant {
    pub(super) card: String,
    pub(super) revision: i64,
    bot: String,
    execution: ExecutionRef,
    generation: i64,
    runtime: Runtime,
    started: Instant,
    last_use: Instant,
    started_at: i64,
    last_use_at: i64,
    owner: Option<crate::fork::autoridade::Autoridade>,
}
impl Grant {
    fn within_deadlines(&self) -> bool {
        self.started.elapsed() < MAX_NOW
            && self.last_use.elapsed() < super::AGENT_HOLD
            && now_ms().saturating_sub(self.started_at) < 900_000
            && now_ms().saturating_sub(self.last_use_at) < 20_000
    }
    pub(super) fn owner(&self) -> Option<&crate::fork::autoridade::Autoridade> {
        self.owner.as_ref()
    }
    /// Connection-only: o recibo respondido e a geração ainda são os mesmos.
    pub(crate) fn check_on(&self, db: &Connection, bot: &str, target: &str) -> Result<()> {
        ensure!(self.bot == bot && self.within_deadlines(), access::DENIED);
        ensure!(crate::fork::spaces::config_generation_on(db, bot)? == self.generation, access::DENIED);
        bindings::assert_execution_live(db, &self.execution)?;
        let entry = Store::entry_on(db, &self.card)?.ok_or_else(|| anyhow!(access::DENIED))?;
        ensure!(
            entry.rev == self.revision
                && entry.data["status"] == "answered"
                && entry.data["selected"] == "now"
                && entry.data["screenAccess"]["target"] == target
                && entry.data["screenAccess"]["effect"] == true,
            access::DENIED
        );
        Ok(())
    }
}
impl State {
    pub(super) fn has(&self, bot: &str) -> bool {
        self.grants.contains_key(bot)
    }
    pub(super) fn grant(
        &self,
        bot: &str,
        execution: &ExecutionRef,
        generation: i64,
        runtime: &Runtime,
    ) -> Option<&Grant> {
        self.grants.get(bot).filter(|g| {
            g.execution == *execution
                && g.generation == generation
                && same_runtime(runtime, &g.runtime, execution)
                && g.within_deadlines()
        })
    }
    pub(super) fn claimed(&mut self, bot: &str) {
        self.grants.retain(|id, _| id == bot);
        if let Some(g) = self.grants.get_mut(bot) {
            g.last_use = Instant::now();
            g.last_use_at = now_ms();
        }
    }
    pub(super) fn revoke(&mut self, bot: &str) {
        self.grants.remove(bot);
    }
}
/// Projeção sem poder de autorizar: só oferece Encerrar se o recibo original segue válido.
pub(crate) fn can_end(hub: &Hub, bot: &str) -> bool {
    let snapshot = {
        let control = hub.screen.control.locked();
        control.permissions.grants.get(bot).cloned()
    };
    let Some(snapshot) = snapshot else { return false };
    hub.permission_runtime(bot, |runtime| {
        let runtime = runtime.ok_or_else(|| anyhow!(access::DENIED))?;
        let check = |db: &Connection| {
            let control = hub.screen.control.locked();
            let grant = control
                .permissions
                .grant(bot, &snapshot.execution, snapshot.generation, runtime)
                .ok_or_else(|| anyhow!(access::DENIED))?;
            ensure!(grant.card == snapshot.card && grant.revision == snapshot.revision, access::DENIED);
            let row = Store::bot_on(db, bot)?.ok_or_else(|| anyhow!(access::DENIED))?;
            ensure!(!row.deleted && row.config.computer, access::DENIED);
            grant.check_on(db, bot, &access::host_target(hub))
        };
        if let Some(owner) = snapshot.owner() { owner.with_db(hub, None, check) } else { check(&hub.store.db.locked()) }
    })
    .is_ok()
}

fn same_runtime(current: &Runtime, original: &Runtime, execution: &ExecutionRef) -> bool {
    matches!(current.status, BotStatus::Working | BotStatus::NeedsInput)
        && current.memory_turn_token.as_deref() == Some(execution.turn_token())
        && current.memory_turn_token == original.memory_turn_token
        && current.session == original.session
        && current.lane == original.lane
        && current.message == original.message
        && current.started_at == original.started_at
}
impl Screen {
    pub(crate) fn permission_runtime_changed(&self, bot: &str, current: &Runtime) {
        let mut control = self.control.locked();
        if control.permissions.grants.get(bot).is_some_and(|g| !same_runtime(current, &g.runtime, &g.execution)) {
            control.permissions.revoke(bot);
        }
        for pending in control.permissions.pending.values_mut().filter(|p| p.bot == bot) {
            if !same_runtime(current, &pending.runtime, &pending.execution) {
                pending.waiters.clear();
            }
        }
    }
}
struct Waiting {
    hub: Weak<Hub>,
    card: String,
    counted: bool,
    id: String,
    deadline: tokio::time::Instant,
}
impl Drop for Waiting {
    fn drop(&mut self) {
        if self.counted
            && let Some(hub) = self.hub.upgrade()
        {
            let mut c = hub.screen.control.locked();
            if let Some(p) = c.permissions.pending.get_mut(&self.card) {
                p.waiters.remove(&self.id);
            }
        }
    }
}

/// Sem await na criação: uma Tx decide o único pendente do par, com a execução original.
fn request(
    hub: &Arc<Hub>,
    attempt: &super::fork_observation::Attempt,
    _reason: Option<&str>,
) -> Result<(Entry, Waiting)> {
    attempt.check_waiting()?;
    let waiting_id = uuid::Uuid::new_v4().to_string();
    let deadline = tokio::time::Instant::now() + WAIT;
    let _boundary = hub.store.memory_boundary.locked();
    let _emit = hub.emit_lock.locked();
    let (entry, entries, counted, fresh) = hub.conector_fence(&attempt.bot, |runtime| {
        ensure!(same_runtime(runtime, &attempt.runtime, &attempt.execution), access::DENIED);
        let mut db = hub.store.db.locked();
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        bindings::assert_execution_live(&tx, &attempt.execution)?;
        ensure!(crate::fork::spaces::config_generation_on(&tx, &attempt.bot)? == attempt.generation, access::DENIED);
        let target = access::host_target(hub);
        let denied: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM donna_screen_access_audit WHERE bot_id=?1 AND target=?2 AND choice='deny' AND (at>?3 OR execution=?4))",
            params![attempt.bot, target, now_ms()-3_600_000, attempt.execution.turn_token()], |r| r.get(0))?;
        ensure!(!denied, "O dono recusou a tela deste computador. Não vou pedir de novo neste turno nem por uma hora; ele pode ligar em Configurações.");
        let existing: Option<String> = tx.query_row("SELECT id FROM entries WHERE kind='permission' AND json_extract(data,'$.toolKind')='screen_access' AND json_extract(data,'$.author')=?1 AND json_extract(data,'$.screenAccess.target')=?2 AND json_extract(data,'$.status')='pending' ORDER BY seq DESC LIMIT 1",
            params![attempt.bot, target], |r| r.get(0)).optional()?;
        ensure!(
            existing.as_ref().is_none_or(|id| !hub.store.expiration_failed(id)),
            "O pedido de tela anterior não pôde ser encerrado."
        );
        let mut events = vec![];
        let existing = existing.map(|id| Store::entry_on(&tx, &id)).transpose()?.flatten();
        let mut control = hub.screen.control.locked();
        let existing = if let Some(mut old) = existing {
            if old.data["screenAccess"]["expiresAt"].as_i64().is_none_or(|until| until <= now_ms())
                || control.permissions.pending.get(&old.id).is_some_and(|p| Instant::now() >= p.deadline) {
                old.data["status"] = json!("expired");
                if let Some(closed) = Store::update_entry_on(&tx, &old.id, &old.data, None)? { events.push(closed); }
                None
            } else { Some(old) }
        } else { None };
        let entry = if let Some(existing) = existing { existing } else {
            let row = Store::bot_on(&tx, &attempt.bot)?.ok_or_else(|| anyhow!(access::DENIED))?;
            ensure!(!row.deleted && row.config.computer && !row.config.is_group(), access::DENIED);
            let lane = runtime.lane.as_ref().ok_or_else(|| anyhow!(access::DENIED))?;
            let turn: i64 = tx.query_row("SELECT COALESCE(MAX(turn),0) FROM entries WHERE bot_id=?1 AND COALESCE(session,bot_id)=?2", params![lane.chat, lane.session_id()], |r| r.get(0))?;
            let data = json!({"author":attempt.bot,"status":"pending","toolKind":"screen_access","voiceApproval":"app_only",
                "title":"Pode ver a tela deste computador?", "detail":access::WARNING,
                "screenAccess":{"target":target,"generation":attempt.generation,"execution":attempt.execution.turn_token(),"expiresAt":now_ms()+600_000},
                "options":[{"optionId":"always","name":"Permitir sempre","kind":"allow_always"},
                    {"optionId":"now","name":"Permitir agora","kind":"allow_once"},{"optionId":"deny","name":"Não permitir","kind":"reject_once"}]});
            let (marker, entry) = Store::insert_permission_card_on(&tx, &row.config, lane, turn, &data, &attempt.execution)?;
            events.extend(marker); events.push(entry.clone()); entry
        };
        tx.commit()?;
        let fresh = events.iter().any(|e| e.id == entry.id);
        let scope = entry.data["screenAccess"]["execution"].as_str() == Some(attempt.execution.turn_token())
            && entry.data["screenAccess"]["generation"] == attempt.generation
            && runtime.lane.as_ref().is_some_and(|lane| lane.chat == entry.bot_id && lane.session_id() == entry.session_id);
        let mut counted = false;
        if scope && (fresh || control.permissions.pending.contains_key(&entry.id)) {
            let p = control.permissions.pending.entry(entry.id.clone()).or_insert_with(|| Pending {
                bot:attempt.bot.clone(),execution:attempt.execution.clone(),runtime:attempt.runtime.clone(),generation:attempt.generation,waiters:HashMap::new(),
                deadline: Instant::now()+Duration::from_millis(remaining(&entry)),
            });
            counted = p.execution == attempt.execution && p.generation == attempt.generation;
            if counted { p.waiters.retain(|_,until|*until>tokio::time::Instant::now());p.waiters.insert(waiting_id.clone(),deadline); }
        }
        Ok((entry,events,counted,fresh))
    })?;
    for e in entries {
        let _ = hub.events.send(json!({"type":"entry","rev":e.rev,"entry":e}));
    }
    let waiting = Waiting { hub: Arc::downgrade(hub), card: entry.id.clone(), counted, id: waiting_id, deadline };
    if fresh {
        schedule_expiry(hub, &entry);
    }
    Ok((entry, waiting))
}

fn remaining(entry: &Entry) -> u64 {
    let until = entry.data["screenAccess"]["expiresAt"].as_i64().unwrap_or(0);
    until.saturating_sub(now_ms()).clamp(0, 600_000).cast_unsigned()
}
fn schedule_expiry(hub: &Arc<Hub>, entry: &Entry) {
    let weak = Arc::downgrade(hub);
    let id = entry.id.clone();
    let delay = remaining(entry);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(delay)).await;
        if let Some(hub) = weak.upgrade() {
            let _ = expire_due(&hub, &id, true);
        }
    });
}
/// Recupera apenas o prazo do cartão persistido; nunca execução, origem ou Agora.
pub(crate) fn restore_pending(hub: &Arc<Hub>) -> Result<()> {
    let cards = {
        let db = hub.store.db.locked();
        let mut stmt=db.prepare("SELECT id FROM entries WHERE kind='permission' AND json_extract(data,'$.toolKind')='screen_access' AND json_extract(data,'$.status')='pending'")?;
        let ids = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        ids.into_iter().map(|id| Store::entry_on(&db, &id)).collect::<rusqlite::Result<Vec<_>>>()?
    };
    for entry in cards.into_iter().flatten() {
        schedule_expiry(hub, &entry);
    }
    Ok(())
}

pub(super) async fn authorize(
    mut attempt: super::fork_observation::Attempt,
    reason: Option<&str>,
) -> Result<Arc<super::fork_observation::Attempt>> {
    let hub = attempt.hub.upgrade().ok_or_else(|| anyhow!(access::DENIED))?;
    attempt.take_grant(&hub);
    let persistent = access::allowed_on(&hub.store.db.locked(), &attempt.bot, &access::host_target(&hub), None).is_ok();
    if persistent || attempt.grant.is_some() {
        attempt.verify()?;
        return Ok(Arc::new(attempt));
    }
    // Erros de origem, turno, configuração ou transporte não criam pedido.
    attempt.check_waiting()?;
    let mut events = hub.events.subscribe();
    let (entry, waiting) = request(&hub, &attempt, reason)?;
    let until = waiting.deadline;
    loop {
        ensure!(
            tokio::time::Instant::now() < until,
            "aguardando o dono; o cartão continua disponível por até 10 minutos"
        );
        let current = hub.store.entry(&entry.id).ok_or_else(|| anyhow!(access::DENIED))?;
        if current.data["status"] != "pending" {
            ensure!(
                current.data["status"] == "answered" && current.data["screenAccess"]["effect"] == true,
                "O dono não liberou a tela deste computador."
            );
            if current.data["selected"] == "always" {
                ensure!(
                    waiting.counted,
                    "A chave foi ligada para as próximas chamadas; esta espera já pertence a outra execução."
                );
                // Só adota a alteração produzida pelo commit deste cartão, nunca um off→on sucessor.
                ensure!(current.data["screenAccess"]["generation"] == attempt.generation, access::DENIED);
                attempt.generation += 1;
            }
            if current.data["selected"] == "now" {
                attempt.grant = Some((current.id.clone(), current.rev));
            }
            attempt.verify()?;
            return Ok(Arc::new(attempt));
        }
        attempt.check_waiting()?;
        ensure!(
            current.data["screenAccess"]["expiresAt"].as_i64().is_some_and(|t| t > now_ms()),
            "O pedido de tela expirou sem liberação."
        );
        tokio::select! {
            () = tokio::time::sleep_until(until) => return Err(anyhow!("aguardando o dono; o cartão continua disponível por até 10 minutos")),
            () = tokio::time::sleep(Duration::from_millis(250)) => {},
            _ = events.recv() => {},
        }
    }
}

#[cfg(test)]
fn expire(hub: &Hub, id: &str) -> Result<()> {
    expire_due(hub, id, false)
}
fn expire_due(hub: &Hub, id: &str, elapsed_timer: bool) -> Result<()> {
    let _emit = hub.emit_lock.locked();
    let mut db = hub.store.db.locked();
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let Some(mut entry) = Store::entry_on(&tx, id)? else { return Ok(()) };
    let mut control = hub.screen.control.locked();
    let elapsed = elapsed_timer
        || entry.data["screenAccess"]["expiresAt"].as_i64().is_some_and(|t| t <= now_ms())
        || control.permissions.pending.get(id).is_some_and(|p| Instant::now() >= p.deadline);
    if entry.data["status"] != "pending" {
        control.permissions.pending.remove(id);
        return Ok(());
    }
    if !elapsed {
        return Ok(());
    }
    entry.data["status"] = json!("expired");
    let updated = Store::update_entry_on(&tx, id, &entry.data, None)?;
    tx.commit()?;
    control.permissions.pending.remove(id);
    drop(control);
    drop(db);
    if let Some(e) = updated {
        let _ = hub.events.send(json!({"type":"entry","rev":e.rev,"entry":e}));
    }
    Ok(())
}

/// Ramo do respond existente: revisão, origem original e toda a decisão no commit #567.
pub(crate) fn respond(hub: &Arc<Hub>, entry: &Entry, option: Option<&str>, origin: Option<&Origem>) -> Result<Value> {
    let option = option.unwrap_or("deny");
    ensure!(matches!(option, "always" | "now" | "deny"), "Opção de tela inválida.");
    let bot = entry.data["author"].as_str().ok_or_else(|| anyhow!(access::DENIED))?;
    let target = access::host_target(hub);
    let owner = match origin {
        Some(Origem::Dono(_, original)) => Some(original.clone()),
        Some(Origem::Local) => None,
        _ => return Err(anyhow!("Responda pelo app do dono.")),
    };
    let boundary = hub.store.memory_boundary.locked();
    let emit = hub.emit_lock.locked();
    let (updated, mut control) = hub.permission_runtime(bot, |runtime| {
        crate::fork::permission_card::commit_original(hub, &entry.id, origin, |db| {
            let current = Store::entry_on(db, &entry.id)?.ok_or_else(|| anyhow!(access::DENIED))?;
            ensure!(current.kind == "permission" && current.data["status"] == "pending" && current.rev == entry.rev && current.data == entry.data, "A revisão deste cartão mudou. Atualize a conversa antes de responder.");
            ensure!(current.data["screenAccess"]["target"] == target, "tela ainda indisponível neste computador");
            ensure!(current.data["screenAccess"]["expiresAt"].as_i64().is_some_and(|t| t > now_ms()), "O pedido de tela expirou sem liberação.");
            let generation = if option == "deny" { -1 } else {
                let row = Store::bot_on(db, bot)?.ok_or_else(|| anyhow!(access::DENIED))?;
                ensure!(!row.deleted && !row.config.is_group(), access::DENIED);
                if current.bot_id != bot {
                    let room=Store::bot_on(db,&current.bot_id)?.ok_or_else(||anyhow!("A sala deste cartão não está mais disponível."))?;
                    ensure!(!room.deleted && room.config.is_group() && room.config.space_id == row.config.space_id
                        && room.config.members.iter().any(|member|member==bot),"O bot deste cartão não participa mais desta sala.");
                }
                let generation=crate::fork::spaces::config_generation_on(db,bot)?;
                ensure!(current.data["screenAccess"]["generation"] == generation, "Este pedido de tela foi revogado. Faça um novo pedido.");
                generation
            };
            let control = hub.screen.control.locked();
            ensure!(control.permissions.pending.get(&entry.id).is_none_or(|p| Instant::now() < p.deadline), "O pedido de tela expirou sem liberação.");
            // TODO D330-F5: Caller::Local só nega. Opção B do dono até separar o motor;
            // o app do Mac via túnel SSH e processos do servidor compartilham esta origem (L2).
            if option != "deny" {
                ensure!(matches!(origin, Some(Origem::Local)) || owner.as_ref().is_some_and(super::super::fork::autoridade::Autoridade::outro_aparelho), ACCOUNT_APP);
                if option == "always" {
                    ensure!(control.active_agent().is_none(), "Pare o uso da tela pelos bots antes de ligar esta opção.");
                } else {
                    ensure!(control.active_agent().is_none() || owner.as_ref().is_some_and(super::super::fork::autoridade::Autoridade::outro_aparelho), ACCOUNT_APP);
                }
            }
            let live = control.permissions.pending.get(&entry.id).is_some_and(|p| p.waiters.values().any(|until|*until>tokio::time::Instant::now())
                && p.generation == generation && runtime.is_some_and(|r| same_runtime(r,&p.runtime,&p.execution))
                && bindings::assert_execution_live(db,&p.execution).is_ok());
            let effect = option == "always" || (option == "now" && live);
            if option == "always" {
                db.execute("INSERT OR IGNORE INTO donna_screen_access(bot_id,target) VALUES(?1,?2)", params![bot,target])?;
            }
            db.execute("INSERT INTO donna_screen_access_audit(bot_id,target,enabled,at,choice,card_id,effect,execution) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![bot,target,i64::from(effect),now_ms(),option,entry.id,i64::from(effect),current.data["screenAccess"]["execution"].as_str()])?;
            let mut data = current.data;
            data["status"] = json!("answered"); data["selected"] = json!(option); data["screenAccess"]["effect"] = json!(effect);
            Ok((data,None,control))
        })
    })?;
    let updated = updated.ok_or_else(|| anyhow!(access::DENIED))?;
    if option == "now" && updated.data["screenAccess"]["effect"] == true {
        if let Some(p) = control.permissions.pending.get(&entry.id) {
            let grant = Grant {
                card: entry.id.clone(),
                revision: updated.rev,
                bot: bot.into(),
                execution: p.execution.clone(),
                generation: p.generation,
                runtime: p.runtime.clone(),
                started: Instant::now(),
                last_use: Instant::now(),
                started_at: now_ms(),
                last_use_at: now_ms(),
                owner,
            };
            control.permissions.grants.insert(bot.into(), grant);
        }
    } else if option == "deny" || option == "always" {
        control.permissions.revoke(bot);
    }
    control.permissions.pending.remove(&entry.id);
    drop(control);
    let _ = hub.events.send(json!({"type":"entry","rev":updated.rev,"entry":updated}));
    drop(emit);
    drop(boundary);
    hub.emit_bot(bot);
    Ok(json!({}))
}

#[cfg(test)]
mod tests;
