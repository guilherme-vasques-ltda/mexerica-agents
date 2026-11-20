// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Durable bot routines, shared by API clients and the built-in MCP server.
mod calendar;
mod check;
pub use check::Checked;
mod editor;
pub use editor::{ScheduleDraft, preview as schedule_preview};
pub mod hooks;
use hooks::Refused;
#[path = "../fork/bot_deletion/routines.rs"]
mod bot_deletion;
#[path = "../fork/routine_save/routines.rs"]
mod bound_save;
#[path = "../fork/cadeias/rotinas.rs"]
mod cadeias;
#[path = "../fork/heartbeat/routines.rs"]
mod checagem;
mod pause;
#[path = "../fork/rotina_resultado/routines.rs"]
mod resultado;
mod runner;
mod schedule;
#[path = "../fork/routine_save/origin.rs"]
pub(crate) mod tool_origin;
pub use runner::start;
pub use schedule::Trigger;

use crate::{
    LockExt,
    hub::Hub,
    store::{EntryKind, Lane, Store, now_ms},
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

const STATE_KEY: &str = "routines.v1";
const MAX_FINISHED_RUNS: usize = 1000;
/// Teto de rotinas não apagadas no Hub, valendo para criação e duplicação.
const MAX_ROUTINES: usize = 1000;
/// A duplicação parou no teto, em pt-BR leigo.
const DUPLICATE_LIMIT: &str = "Limite de 1000 rotinas atingido. Apague uma rotina para duplicar.";
/// MX-106: liberar uma rotina de conteúdo de fora é gesto do dono no app, nunca do modelo.
pub(crate) const EXTERNAL_RELEASE: &str =
    "routine_external_release: esta rotina nasceu de conteúdo de fora; só o dono liga pelo app e nada foi alterado.";
fn default_timeout() -> u64 {
    3600
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub(crate) enum ExternalRelease {
    Pending,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Routine {
    pub id: String,
    pub bot_id: String,
    pub name: String,
    pub instruction: String,
    pub triggers: Vec<Trigger>,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub next_runs: Vec<Option<i64>>,
    pub webhook_key: String,
    pub deleted: bool,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    schedule_generation: u64,
    /// Procedência emitida pelo Hub; nunca recebida do corpo editável/MCP.
    #[serde(default, rename = "_creationOrigin")]
    pub(crate) creation_origin: Option<crate::fork::cadeias::descendencia_rotina::RoutineOrigin>,
    /// MX-106/D125: o Hub marcou a criação como vinda de conteúdo de fora. Persistente, nunca editável
    /// pelo modelo; só o app (origem humana) liga uma rotina ainda pausada por esta marca.
    #[serde(default)]
    pub(crate) external_creation: bool,
    /// Ainda aguarda a primeira liberação humana; não é sinônimo de pausa posterior.
    #[serde(default)]
    pub(crate) external_release_pending: Option<ExternalRelease>,
    /// Checagem silenciosa (Heartbeat): declarada só pelo dono, pelo app; nunca recebida do modelo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) heartbeat: Option<crate::fork::heartbeat::Declaracao>,
    /// O que a checagem sabe entre uma leitura e outra (marcador, pendências já avisadas, última checagem).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) heartbeat_state: Option<crate::fork::heartbeat::Estado>,
}

/// Campos editáveis e identidade; metadados do agendador não são edições do usuário.
pub(crate) fn same_team_routines(a: &[Routine], b: &[Routine]) -> bool {
    a.len() == b.len()
        && a.iter().all(|a| {
            b.iter().any(|b| {
                a.id == b.id
                    && a.bot_id == b.bot_id
                    && a.name == b.name
                    && a.instruction == b.instruction
                    && a.triggers == b.triggers
                    && a.enabled == b.enabled
                    && a.timeout_seconds == b.timeout_seconds
                    && a.webhook_key == b.webhook_key
                    && a.deleted == b.deleted
                    && a.heartbeat == b.heartbeat
            })
        })
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Pending,
    Starting,
    Running,
    Recovering,
    Succeeded,
    Failed,
    Interrupted,
    Cancelled,
}

impl Status {
    fn active(self) -> bool {
        matches!(self, Self::Pending | Self::Starting | Self::Running | Self::Recovering)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools, reason = "marcadores independentes de pendência, atraso e origem da execução")]
pub struct Run {
    pub id: String,
    pub routine_id: String,
    pub bot_id: String,
    pub status: Status,
    pub created_at: i64,
    pub finished_at: Option<i64>,
    pub detail: Option<String>,
    pub event: Value,
    pub delivery_id: Option<String>,
    #[serde(default)]
    pub started_at: Option<i64>,
    #[serde(default)]
    report_pending: bool,
    #[serde(default)]
    result: Option<String>,
    #[serde(default, rename = "_chain")]
    chain: Option<crate::fork::cadeias::modelo::Recibo>,
    /// I-023: aditivos, lidos com tolerância (`fork::rotina_resultado`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    declaration: Option<crate::fork::rotina_resultado::Lenient<crate::fork::rotina_resultado::Declaration>>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    declaration_conflict: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    outcome: Option<crate::fork::rotina_resultado::Lenient<crate::fork::rotina_resultado::RunOutcome>>,
    /// MX-110: ocorrência perdida por indisponibilidade do Hub (`due_at` anterior
    /// ao último `available_at`). Atrasada executa só leitura/aviso.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub catchup: bool,
    /// Retentativas limitadas de execução catch-up após falha.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub attempts: u64,
    /// Execução gravada por uma checagem silenciosa: nunca passou pela IA; o texto do aviso sai do histórico ao publicar.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    heartbeat: bool,
    /// O texto do aviso da checagem, só até a publicação. Nunca vai às listagens (`public_run`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    heartbeat_texto: Option<String>,
}

#[allow(clippy::trivially_copy_pass_by_ref, reason = "serde skip_serializing_if exige fn(&u64) -> bool")]
fn is_zero(v: &u64) -> bool {
    *v == 0
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct State {
    #[serde(default)]
    paused: bool,
    /// Último instante em que o Hub pôde executar ocorrências (boot/despausa).
    /// Ocorrência com `at < available_at` é atrasada por indisponibilidade —
    /// critério verificável, sem tolerância arbitrária (MX-110).
    #[serde(default)]
    available_at: i64,
    routines: Vec<Routine>,
    runs: Vec<Run>,
}

fn guard_live_runs(state: &State) -> impl Iterator<Item = (&str, &str)> {
    state
        .runs
        .iter()
        .filter(|run| matches!(run.status, Status::Starting | Status::Running | Status::Recovering))
        .map(|run| (run.bot_id.as_str(), run.id.as_str()))
}

pub struct Routines {
    state: Mutex<State>,
    active: Mutex<HashSet<String>>,
    scheduler: Mutex<Option<tokio::task::JoinHandle<()>>>,
    stopping: std::sync::atomic::AtomicBool,
    /// Bumped whenever the webhook set the relay registers (`hooks()`) changes.
    hooks_rev: tokio::sync::watch::Sender<u64>,
    /// Fontes que cada bot pode ler nas checagens silenciosas: (bot, nome). Vazio até um conector registrar uma.
    fontes: Mutex<std::collections::HashMap<(String, String), Arc<dyn crate::fork::heartbeat::Fonte>>>,
    /// Checagens em andamento, para que a mesma rotina nunca tenha duas rodadas sobrepostas.
    checando: Mutex<HashSet<String>>,
    /// Falhas transitórias injetadas em `prepare_attempt` nos testes.
    #[cfg(test)]
    pub(crate) fail_next: std::sync::atomic::AtomicUsize,
    /// Falhas transitórias injetadas em `mark_waiting` nos testes.
    #[cfg(test)]
    pub(crate) fail_mark: std::sync::atomic::AtomicUsize,
}

impl Default for Routines {
    fn default() -> Self {
        Self {
            state: Mutex::default(),
            active: Mutex::default(),
            scheduler: Mutex::default(),
            stopping: std::sync::atomic::AtomicBool::default(),
            hooks_rev: tokio::sync::watch::channel(0).0,
            fontes: Mutex::default(),
            checando: Mutex::default(),
            #[cfg(test)]
            fail_next: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            fail_mark: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

#[cfg(test)]
impl Routines {
    /// Consome o contador: a próxima operação injetada falha uma vez por unidade.
    pub(crate) fn injected_failure(&self) -> bool {
        self.fail_next
            .try_update(std::sync::atomic::Ordering::SeqCst, std::sync::atomic::Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
    }

    pub(crate) fn injected_mark_failure(&self) -> bool {
        self.fail_mark
            .try_update(std::sync::atomic::Ordering::SeqCst, std::sync::atomic::Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
    }
}

/// One routine's public webhook as the cloud stores it.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct Hook {
    pub id: String,
    pub key: String,
    pub enabled: bool,
}

fn hook_set(state: &State) -> Vec<Hook> {
    state
        .routines
        .iter()
        .filter(|r| !r.deleted && r.triggers.iter().any(|t| matches!(t, Trigger::Webhook | Trigger::Event { .. })))
        .map(|r| Hook { id: r.id.clone(), key: r.webhook_key.clone(), enabled: r.enabled && !state.paused })
        .collect()
}

fn webhook_key() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

/// Uma única rotina validada no estado candidato; só o chamador confirma o SQLite.
fn save_in(s: &mut State, bot: &str, body: &Value) -> Result<Routine> {
    let Checked { name, instruction, triggers, timeout } = Checked::parse(body)?;
    let now = now_ms();
    let id = body["id"].as_str().map_or_else(|| uuid::Uuid::new_v4().to_string(), str::to_owned);
    let updated = body["id"].is_string();
    let old = check::replaced(s, bot, body, &triggers, now)?;
    let heartbeat = match body.get("heartbeat") {
        None => old.as_ref().and_then(|r| r.heartbeat.clone()),
        Some(Value::Null) => None,
        Some(declaration) => Some(crate::fork::heartbeat::Declaracao::parse(declaration)?),
    };
    if heartbeat.is_some() && triggers.iter().any(|t| !matches!(t, Trigger::Interval { .. } | Trigger::Cron { .. })) {
        bail!(crate::fork::heartbeat::SO_NO_HORARIO);
    }
    if heartbeat.is_some()
        && let Some(old) = old.as_ref().filter(|old| old.heartbeat.is_none())
    {
        // Converter uma rotina comum: sem origem de bot (a descendente tem agenda e orçamento próprios) e sem
        // execução já reservada, que ainda poderia chegar à IA.
        anyhow::ensure!(
            old.creation_origin.is_none(),
            "Uma rotina criada pelo bot não vira checagem silenciosa; crie uma nova."
        );
        anyhow::ensure!(
            !s.runs.iter().any(|run| run.routine_id == old.id
                && matches!(run.status, Status::Starting | Status::Running | Status::Recovering)),
            "Esta rotina está em execução; espere terminar para convertê-la em checagem silenciosa."
        );
        cancel_pending(s, &old.id);
    }
    // Uma checagem silenciosa nasce pausada: só liga quando o dono diz `enabled: true`, de propósito.
    let explicit_enable = old.as_ref().is_some_and(|r| r.heartbeat.is_some()) || body["enabled"] == true;
    let instruction = if heartbeat.is_some() { crate::fork::heartbeat::INSTRUCAO.to_owned() } else { instruction };
    if !updated && s.routines.iter().filter(|r| !r.deleted).count() >= MAX_ROUTINES {
        bail!("routine limit reached");
    }
    let schedule_generation = old
        .as_ref()
        .map_or(0, |r| r.schedule_generation)
        .checked_add(u64::from(
            old.as_ref().is_some_and(|r| r.triggers != triggers || (body["enabled"] == true && !r.enabled)),
        ))
        .context("Não foi possível atualizar a agenda desta rotina")?;
    let r = Routine {
        id: id.clone(),
        bot_id: bot.to_owned(),
        name,
        instruction,
        enabled: (heartbeat.is_none() || explicit_enable)
            && body["enabled"].as_bool().unwrap_or_else(|| old.as_ref().is_none_or(|r| r.enabled)),
        created_at: old.as_ref().map_or(now, |r| r.created_at),
        updated_at: now,
        next_runs: if let Some(old) = &old
            && old.triggers == triggers
            && (body["enabled"] != true || old.enabled)
        {
            old.next_runs.clone()
        } else {
            triggers.iter().map(|t| t.next(now)).collect::<Result<_>>()?
        },
        triggers,
        webhook_key: old.as_ref().map_or_else(webhook_key, |r| r.webhook_key.clone()),
        deleted: false,
        timeout_seconds: timeout.unwrap_or_else(|| old.as_ref().map_or_else(default_timeout, |r| r.timeout_seconds)),
        last_error: None,
        schedule_generation,
        creation_origin: old.as_ref().and_then(|r| r.creation_origin.clone()),
        external_creation: old.as_ref().is_some_and(|r| r.external_creation),
        external_release_pending: old
            .as_ref()
            .and_then(|r| r.external_release_pending)
            .filter(|_| body["enabled"] != true),
        heartbeat_state: old
            .as_ref()
            .and_then(|r| r.heartbeat_state.clone())
            .filter(|_| old.as_ref().is_some_and(|r| r.heartbeat == heartbeat)),
        heartbeat,
    };
    if !(1..=86_400).contains(&r.timeout_seconds) {
        bail!("timeoutSeconds must be between 1 and 86400");
    }
    if !r.enabled {
        cancel_pending(s, &id);
    }
    s.routines.retain(|r| r.id != id);
    s.routines.push(r.clone());
    Ok(r)
}

/// O corpo da cópia a partir da rotina salva: nome com sufixo, o resto igual e sem
/// identidade. O segredo do webhook não viaja no corpo: `save_in` gera um novo na
/// criação, e o original continua válido.
fn duplicate_body(source: &Routine) -> Value {
    json!({
        "name": format!("{} (cópia)", source.name),
        "instruction": source.instruction,
        "triggers": source.triggers,
        "enabled": false,
        "timeoutSeconds": source.timeout_seconds,
        "heartbeat": source.heartbeat,
    })
}

/// A cópia herda a marca de conteúdo de fora da origem e nasce pendente de
/// liberação pelo dono, mesmo que a origem já tenha sido liberada.
fn inherit_external_release(target: &mut Routine, source: &Routine) {
    if source.external_creation {
        target.external_creation = true;
        target.external_release_pending = Some(ExternalRelease::Pending);
    }
}

/// Localiza a rotina do bot e confere o teto em pt-BR antes de duplicar.
fn duplicate_source(s: &State, bot: &str, id: &str) -> Result<Routine> {
    let source =
        s.routines.iter().find(|r| r.id == id && r.bot_id == bot && !r.deleted).context("routine not found")?.clone();
    if s.routines.iter().filter(|r| !r.deleted).count() >= MAX_ROUTINES {
        bail!("{DUPLICATE_LIMIT}");
    }
    Ok(source)
}

/// Duplica a rotina do bot como criação: grava a cópia desligada, sem herdar
/// execuções, erros ou agenda calculada. Só os testes usam direto; produção passa
/// pela fila de criação em `duplicate_in_origin`.
#[cfg(test)]
fn duplicate_in(s: &mut State, bot: &str, id: &str) -> Result<Routine> {
    let source = duplicate_source(s, bot, id)?;
    let created = save_in(s, bot, &duplicate_body(&source))?;
    let stored = s.routines.iter_mut().find(|r| r.id == created.id).context("rotina duplicada não foi gravada")?;
    inherit_external_release(stored, &source);
    Ok(stored.clone())
}

impl Routines {
    pub fn restore(&self, store: &Store) -> Result<()> {
        let mut state = match store.kv_read(STATE_KEY)? {
            Some(raw) => serde_json::from_str::<State>(&raw).context("reading routines")?,
            None => State::default(),
        };
        cadeias::restaurar(store, &mut state)?;
        // Toda ocorrência devida antes deste instante nasceu de indisponibilidade
        // (processo fora do ar) e é candidata a catch-up (MX-110).
        state.available_at = now_ms();
        let mut active = self.active.locked();
        active.clear();
        for run in &mut state.runs {
            if matches!(run.status, Status::Starting | Status::Running | Status::Recovering)
                && crate::fork::bot_admission::routine_uncertain(store, &run.bot_id, &run.id)?
            {
                run.status = Status::Interrupted;
                run.finished_at = Some(now_ms());
                run.detail = Some("O Hub reiniciou após permitir o despacho; pode haver efeito já realizado. Confira antes de tentar de novo.".into());
                run.report_pending = true;
                run.event = Value::Null;
                continue;
            }
            match run.status {
                Status::Starting => run.status = Status::Pending,
                Status::Running | Status::Recovering => {
                    run.status = Status::Recovering;
                    run.finished_at = None;
                    active.insert(run.bot_id.clone());
                }
                _ => {}
            }
        }
        store.guard_routines_commit(&serde_json::to_string(&state)?, guard_live_runs(&state))?;
        *self.state.locked() = state;
        self.hooks_rev.send_modify(|v| *v += 1);
        Ok(())
    }

    /// Só distingue o legado sem recibo; não reserva, consome orçamento ou autoriza IA.
    pub(crate) fn recovery_without_receipt(&self, bot: &str, id: &str) -> bool {
        self.state
            .locked()
            .runs
            .iter()
            .any(|run| run.id == id && run.bot_id == bot && run.status == Status::Recovering && run.chain.is_none())
    }

    /// Resultado durável já concluído não precisa de nova IA no boot.
    pub(crate) fn admission_finished(&self, id: &str) -> bool {
        self.state.locked().runs.iter().any(|r| r.id == id && !r.status.active())
    }

    /// The webhook routines the relay registers with the cloud.
    pub fn hooks(&self) -> Vec<Hook> {
        hook_set(&self.state.locked())
    }

    pub fn subscribe_hooks(&self) -> tokio::sync::watch::Receiver<u64> {
        self.hooks_rev.subscribe()
    }

    fn change<T>(&self, store: &Store, update: impl FnOnce(&mut State) -> Result<T>) -> Result<T> {
        self.change_bound(store, None, update)
    }

    /// Snapshot privado da D67; inclui chaves, nunca vai para o chat/modelo.
    pub(crate) fn team_snapshot(&self, bot: &str) -> Vec<Routine> {
        self.state.locked().routines.iter().filter(|r| r.bot_id == bot && !r.deleted).cloned().collect()
    }

    /// Compensa uma criação ainda sem aviso, inclusive se o scheduler já enfileirou uma rotina.
    pub(crate) fn discard_team_creation<T>(&self, bot: &str, commit: impl FnOnce(&str) -> Result<T>) -> Result<T> {
        let mut state = self.state.locked();
        let mut next = state.clone();
        next.routines.retain(|r| r.bot_id != bot);
        for run in next.runs.iter_mut().filter(|r| r.bot_id == bot && r.status.active()) {
            run.status = Status::Cancelled;
            run.finished_at = Some(now_ms());
            run.report_pending = false;
        }
        let result = commit(&serde_json::to_string(&next)?)?;
        let hooks_changed = hook_set(&state) != hook_set(&next);
        *state = next;
        drop(state);
        if hooks_changed {
            self.hooks_rev.send_modify(|v| *v += 1);
        }
        Ok(result)
    }

    /// O callback persiste perfil, rotinas e aviso juntos. Só depois do commit troca o estado em memória.
    pub(crate) fn restore_team<T>(
        &self,
        bot: &str,
        expected: &[Routine],
        before: &[Routine],
        commit: impl FnOnce(&str) -> Result<T>,
    ) -> Result<T> {
        let mut state = self.state.locked();
        let current: Vec<_> = state.routines.iter().filter(|r| r.bot_id == bot && !r.deleted).cloned().collect();
        if !same_team_routines(&current, expected) || state.runs.iter().any(|r| r.bot_id == bot && r.status.active()) {
            bail!("routines changed or have active runs; cannot undo");
        }
        let mut next = state.clone();
        next.routines.retain(|r| r.bot_id != bot);
        for routine in before {
            let mut restored = routine.clone();
            // Desfazer uma edição não desfaz o que a checagem já avisou: o progresso mais recente fica.
            if let Some(atual) = current.iter().find(|r| r.id == restored.id)
                && atual.heartbeat == restored.heartbeat
            {
                restored.heartbeat_state.clone_from(&atual.heartbeat_state);
            }
            restored.next_runs = restored.triggers.iter().map(|t| t.next(now_ms())).collect::<Result<_>>()?;
            restored.schedule_generation = current
                .iter()
                .find(|r| r.id == restored.id)
                .map_or(restored.schedule_generation, |r| r.schedule_generation.max(restored.schedule_generation))
                .checked_add(1)
                .context("Não foi possível atualizar a agenda desta rotina")?;
            next.routines.push(restored);
        }
        let result = commit(&serde_json::to_string(&next)?)?;
        let hooks_changed = hook_set(&state) != hook_set(&next);
        *state = next;
        drop(state);
        if hooks_changed {
            self.hooks_rev.send_modify(|v| *v += 1);
        }
        Ok(result)
    }

    /// Consulta só identidade e status, sem serializar histórico, payloads ou resultados.
    pub(crate) fn guard_run_live(&self, bot: &str, run: &str) -> bool {
        self.state.locked().runs.iter().any(|r| {
            r.id == run
                && r.bot_id == bot
                && matches!(r.status, Status::Starting | Status::Running | Status::Recovering)
        })
    }

    #[cfg(test)]
    pub(crate) fn chain_reserved_for_test(&self, bot: &str) -> bool {
        self.active.locked().contains(bot)
    }

    pub fn list(&self, bot: &str) -> Value {
        let state = self.state.locked();
        json!({"routines": state.routines.iter().filter(|r| r.bot_id == bot && !r.deleted).map(public).collect::<Vec<_>>(),
            "runs": state.runs.iter().rev().filter(|r| r.bot_id == bot).take(200).map(public_run).collect::<Vec<_>>()})
    }

    pub fn save(&self, hub: &Hub, bot: &str, body: &Value) -> Result<Value> {
        self.save_bound(hub, bot, body, || {})
    }

    pub fn duplicate(&self, hub: &Hub, bot: &str, id: &str) -> Result<Value> {
        self.duplicate_in_origin(hub, bot, id, None, || {})
    }

    /// Duplica a rotina do bot pela fila de criação, com cota, aviso e replay.
    pub(super) fn duplicate_in_origin(
        &self,
        hub: &Hub,
        bot: &str,
        id: &str,
        origin: Option<&tool_origin::ToolOrigin>,
        after_lookup: impl FnOnce(),
    ) -> Result<Value> {
        after_lookup();
        if origin.is_some_and(tool_origin::ToolOrigin::is_creation) {
            return self.save_in_origin(hub, bot, &json!({"duplicateOf": id}), origin, || {});
        }
        self.duplicate_direct(hub, bot, id, origin)
    }

    /// O callback confirma perfil, rotinas e snapshot juntos antes da troca em memória.
    pub(crate) fn save_team<T>(
        &self,
        bot: &str,
        bodies: &[Value],
        commit: impl FnOnce(&str, &[Routine]) -> Result<T>,
    ) -> Result<(T, Vec<Value>)> {
        anyhow::ensure!(!bodies.iter().any(|body| body.get("heartbeat").is_some()), crate::fork::heartbeat::SO_O_DONO);
        let mut state = self.state.locked();
        let mut next = state.clone();
        let saved =
            bodies.iter().map(|body| save_in(&mut next, bot, body).map(|r| public(&r))).collect::<Result<_>>()?;
        bound_save::ensure_no_model_release(&state, &next)?;
        bound_save::ensure_no_model_heartbeat(&state, &next)?;
        let after: Vec<_> = next.routines.iter().filter(|r| r.bot_id == bot && !r.deleted).cloned().collect();
        let result = commit(&serde_json::to_string(&next)?, &after)?;
        let hooks_changed = hook_set(&state) != hook_set(&next);
        *state = next;
        drop(state);
        if hooks_changed {
            self.hooks_rev.send_modify(|v| *v += 1);
        }
        Ok((result, saved))
    }

    pub fn set_enabled(&self, store: &Store, bot: &str, id: &str, enabled: bool) -> Result<Value> {
        self.set_enabled_in_origin(store, bot, id, enabled, None)
    }

    fn set_enabled_in_origin(
        &self,
        store: &Store,
        bot: &str,
        id: &str,
        enabled: bool,
        origin: Option<&tool_origin::ToolOrigin>,
    ) -> Result<Value> {
        self.change_in_origin(store, origin.map(|_| bot), origin, |s| {
            let r = s
                .routines
                .iter_mut()
                .find(|r| r.id == id && r.bot_id == bot && !r.deleted)
                .context("routine not found")?;
            if enabled && !r.enabled {
                r.next_runs = r.triggers.iter().map(|t| t.next(now_ms())).collect::<Result<_>>()?;
                r.schedule_generation =
                    r.schedule_generation.checked_add(1).context("Não foi possível atualizar a agenda desta rotina")?;
            }
            r.enabled = enabled;
            if enabled {
                r.external_release_pending = None;
            }
            r.updated_at = now_ms();
            if !enabled {
                for run in &mut s.runs {
                    if run.routine_id == id && run.status == Status::Pending {
                        cancel_run(run);
                    }
                }
            }
            Ok(json!({"routine":public(r)}))
        })
    }

    pub fn remove(&self, store: &Store, bot: &str, id: &str) -> Result<Value> {
        self.remove_in_origin(store, bot, id, None)
    }

    fn remove_in_origin(
        &self,
        store: &Store,
        bot: &str,
        id: &str,
        origin: Option<&tool_origin::ToolOrigin>,
    ) -> Result<Value> {
        self.change_in_origin(store, origin.map(|_| bot), origin, |s| remove_in(s, bot, id))
    }

    pub fn enqueue(
        &self,
        store: &Store,
        bot: &str,
        id: &str,
        event: Value,
        delivery: Option<String>,
        test: bool,
    ) -> Result<Value> {
        self.enqueue_in_origin(store, bot, id, event, delivery, test, None)
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "conserva a origem MCP até o commit sem alterar os produtores existentes"
    )]
    fn enqueue_in_origin(
        &self,
        store: &Store,
        bot: &str,
        id: &str,
        event: Value,
        delivery: Option<String>,
        test: bool,
        origin: Option<&tool_origin::ToolOrigin>,
    ) -> Result<Value> {
        anyhow::ensure!(
            !self
                .state
                .locked()
                .routines
                .iter()
                .any(|r| r.id == id && r.bot_id == bot && !r.deleted && r.heartbeat.is_some()),
            crate::fork::heartbeat::SO_NO_HORARIO
        );
        if let Some(result) = self.enqueue_descendant(
            store,
            bot,
            origin,
            &bound_save::ManualRun { id, event: event.clone(), delivery: delivery.clone(), test },
        )? {
            return Ok(result);
        }
        if let Some(origin) = origin {
            return self.enqueue_bound_chain(store, bot, origin, bound_save::ManualRun { id, event, delivery, test });
        }
        self.change(store, |s| {
            if s.paused {
                bail!(Refused::Paused);
            }
            let r =
                s.routines.iter().find(|r| r.id == id && r.bot_id == bot && !r.deleted).context("routine not found")?;
            if !r.enabled && !test {
                bail!(Refused::Paused);
            }
            if let Some(key) = &delivery
                && let Some(run) =
                    s.runs.iter().find(|run| run.routine_id == id && run.delivery_id.as_ref() == Some(key))
            {
                if run.chain.is_some() {
                    let reserved = cadeias::manual(store, r, event.clone(), delivery.clone())?;
                    anyhow::ensure!(
                        reserved.id == run.id,
                        "Não foi possível conferir a execução original desta ocorrência"
                    );
                }
                return Ok(json!({"run":public_run(run)}));
            }
            if s.runs.iter().any(|run| run.routine_id == id && run.status.active()) {
                bail!(Refused::Busy);
            }
            let run = cadeias::manual(store, r, event, delivery)?;
            // O JSON pode ter retido só o histórico recente; um recibo já ligado nunca vira trabalho novo.
            let receipt = run.chain.as_ref().context("Tentativa da rotina não encontrada")?;
            anyhow::ensure!(
                !crate::fork::cadeias::admissao::vinculada(store, receipt)?,
                crate::fork::cadeias::modelo::Recusa::ReciboInvalido
            );
            let result = json!({"run":public_run(&run)});
            s.runs.push(run);
            Ok(result)
        })
    }

    /// The routine's webhook: the public URL through the configured cloud (when it's on), the
    /// local one, and the key. `rotate` replaces the key first, cutting off the old one.
    pub fn credentials(&self, hub: &Hub, bot: &str, id: &str, rotate: bool) -> Result<Value> {
        self.credentials_in_origin(hub, bot, id, rotate, None)
    }

    fn credentials_in_origin(
        &self,
        hub: &Hub,
        bot: &str,
        id: &str,
        rotate: bool,
        origin: Option<&tool_origin::ToolOrigin>,
    ) -> Result<Value> {
        if rotate {
            self.change_in_origin(&hub.store, origin.map(|_| bot), origin, |s| {
                let r = s
                    .routines
                    .iter_mut()
                    .find(|r| r.id == id && r.bot_id == bot && !r.deleted)
                    .context("routine not found")?;
                r.webhook_key = webhook_key();
                r.updated_at = now_ms();
                Ok(())
            })?;
        }
        let key = {
            let s = self.state.locked();
            s.routines
                .iter()
                .find(|r| r.id == id && r.bot_id == bot && !r.deleted)
                .context("routine not found")?
                .webhook_key
                .clone()
        };
        let cloud = hub.cloud.status();
        let url = crate::remote::cloud::url(&hub.store)
            .map(|base| format!("{}/v1/hooks/{}/{id}", base.trim_end_matches('/'), hub.identity.computer_id()));
        Ok(json!({
            "url": url,
            "localUrl": format!("http://127.0.0.1:{}/hooks/routines/{id}", hub.port),
            "key": key,
            "connected": url.is_some() && cloud.connected,
        }))
    }

    /// One delivery from the local endpoint or the cloud: checks the key or signature, then
    /// queues a run when the event matches. Retries of a delivery id return its first run.
    pub fn receive(&self, hub: &Hub, id: &str, headers: &hooks::Headers, body: &[u8]) -> Result<Value, Refused> {
        let r = self.state.locked().routines.iter().find(|r| r.id == id && !r.deleted).cloned();
        let Some(r) = r.filter(|r| hooks::authorized(&r.webhook_key, headers, body)) else {
            return Err(Refused::Unauthorized);
        };
        let delivery = hooks::delivery_id(headers)?;
        let Some(event) = hooks::event(headers, body)? else {
            return Ok(json!({"ping": true}));
        };
        if !r.enabled {
            return Err(Refused::Paused);
        }
        if !r.triggers.iter().any(|t| matches!(t, Trigger::Webhook) || t.matches(&event)) {
            return Ok(json!({"ignored": "event does not match this routine"}));
        }
        if self.state.locked().paused {
            return Err(Refused::Paused);
        }
        crate::fork::webhook_chain::admit(hub, &r)?;
        self.enqueue(&hub.store, &r.bot_id, id, event, delivery, false).map_err(|e| match e.downcast::<Refused>() {
            Ok(refused) => refused,
            Err(e) => Refused::Invalid(format!("{e:#}")),
        })
    }
}

fn required<'a>(body: &'a Value, key: &str) -> Result<&'a str> {
    body[key].as_str().with_context(|| format!("{key} is required"))
}
fn public(r: &Routine) -> Value {
    json!({"id":r.id,"botId":r.bot_id,"name":r.name,"instruction":r.instruction,"triggers":r.triggers,"enabled":r.enabled,"createdAt":r.created_at,"updatedAt":r.updated_at,
        "timeoutSeconds":r.timeout_seconds,"lastError":r.last_error,"heartbeat":r.heartbeat,"heartbeatStatus":r.heartbeat_state.as_ref().map(crate::fork::heartbeat::Estado::public),"externalReleasePending":r.external_creation && r.external_release_pending.is_some() && !r.enabled,
        "triggerDescriptions":r.triggers.iter().map(Trigger::description).collect::<Vec<_>>(),"agenda":calendar::agenda(&r.triggers),"nextRunAt": if r.enabled {r.next_runs.iter().flatten().min().copied()} else {None}})
}
fn public_run(run: &Run) -> Value {
    json!({"id":run.id,"routineId":run.routine_id,"botId":run.bot_id,"status":run.status,
        "createdAt":run.created_at,"finishedAt":run.finished_at,"detail":run.detail,"event":run.event,
        "deliveryId":run.delivery_id,"startedAt":run.started_at,"reportPending":run.report_pending,"result":run.result,
        "outcome":crate::fork::rotina_resultado::public(run.outcome.as_ref()),"declaration":run.declaration,
        "declarationConflict":run.declaration_conflict})
}

fn new_run(r: &Routine, event: Value, delivery_id: Option<String>) -> Run {
    Run {
        id: uuid::Uuid::new_v4().to_string(),
        routine_id: r.id.clone(),
        bot_id: r.bot_id.clone(),
        status: Status::Pending,
        created_at: now_ms(),
        finished_at: None,
        detail: None,
        event,
        delivery_id,
        started_at: None,
        report_pending: false,
        result: None,
        chain: None,
        declaration: None,
        declaration_conflict: false,
        outcome: None,
        catchup: false,
        attempts: 0,
        heartbeat: false,
        heartbeat_texto: None,
    }
}

fn remove_in(s: &mut State, bot: &str, id: &str) -> Result<Value> {
    let r = s.routines.iter_mut().find(|r| r.id == id && r.bot_id == bot && !r.deleted).context("routine not found")?;
    r.deleted = true;
    r.enabled = false;
    r.webhook_key.clear();
    for run in &mut s.runs {
        if run.routine_id == id && run.status == Status::Pending {
            run.status = Status::Cancelled;
            run.finished_at = Some(now_ms());
        }
    }
    Ok(json!({"ok":true}))
}

pub(crate) fn creation_schedule(routine: &Routine) -> String {
    let text = calendar::summary(&routine.triggers, chrono_tz::UTC);
    if routine.triggers.iter().any(|t| matches!(t, Trigger::Once { .. })) { format!("{text} (UTC)") } else { text }
}

fn cancel_run(run: &mut Run) {
    run.status = Status::Cancelled;
    run.finished_at = Some(now_ms());
    run.event = Value::Null;
}

/// Recalcula a agenda de uma rotina pausada, sem dispará-la.
pub(crate) fn enable_for_review(routine: &mut Routine) -> Result<()> {
    anyhow::ensure!(
        routine.external_creation && routine.external_release_pending.is_some() && !routine.enabled && !routine.deleted,
        "routine_review_unavailable"
    );
    routine.next_runs = routine.triggers.iter().map(|t| t.next(now_ms())).collect::<Result<_>>()?;
    routine.schedule_generation =
        routine.schedule_generation.checked_add(1).context("Não foi possível atualizar a agenda desta rotina")?;
    routine.enabled = true;
    routine.external_release_pending = None;
    routine.updated_at = now_ms();
    Ok(())
}
fn cancel_pending(state: &mut State, id: &str) {
    for run in &mut state.runs {
        if run.routine_id == id && run.status == Status::Pending {
            cancel_run(run);
        }
    }
}

/// The name of a run's own agent session, which it resumes after a restart.
pub fn session_slot(run_id: &str) -> String {
    format!("routine-{run_id}")
}

impl Routines {
    /// MX-110: a execução é catch-up? Usado pelo gate de permissões e pelas tools.
    pub(crate) fn run_is_catchup(&self, run_id: &str) -> bool {
        self.state.locked().runs.iter().any(|r| r.id == run_id && r.catchup)
    }
}

/// The notice a run posts in the chat when it begins; the run's turn is the notice's.
pub fn notice_id(run_id: &str) -> String {
    format!("routine-{run_id}")
}

#[cfg(test)]
pub fn call(hub: &Arc<Hub>, bot: &str, name: &str, args: &Value) -> Result<Value> {
    call_in_origin(hub, bot, name, args, None)
}

#[cfg(test)]
pub(crate) fn call_in_execution(
    hub: &Arc<Hub>,
    bot: &str,
    name: &str,
    args: &Value,
    execution: &crate::fork::sessoes::ExecutionRef,
) -> Result<Value> {
    call_invoked_in_execution(hub, bot, name, args, execution, None)
}

pub(crate) fn call_invoked_in_execution(
    hub: &Arc<Hub>,
    bot: &str,
    name: &str,
    args: &Value,
    execution: &crate::fork::sessoes::ExecutionRef,
    invocation: Option<&str>,
) -> Result<Value> {
    if name == "save_routine" || name == "duplicate_routine" {
        if name == "duplicate_routine" {
            anyhow::ensure!(args["id"].is_string(), "invalid_field: id de rotina inválido");
            let origin = tool_origin::ToolOrigin::capture_creation(
                hub,
                bot,
                execution,
                invocation,
                &json!({"duplicateOf": args["id"]}),
            )?;
            return call_in_origin(hub, bot, name, args, Some(&origin));
        }
        if args.get("id").is_none() {
            let origin = tool_origin::ToolOrigin::capture_creation(hub, bot, execution, invocation, args)?;
            return call_in_origin(hub, bot, name, args, Some(&origin));
        }
        anyhow::ensure!(args["id"].is_string(), "invalid_field: id de rotina inválido");
    }
    let changes =
        matches!(name, "save_routine" | "duplicate_routine" | "set_routine_enabled" | "delete_routine" | "run_routine")
            || (name == "routine_webhook" && args["rotate"] == true);
    let origin = tool_origin::ToolOrigin::capture(hub, bot, execution, changes)?;
    let result = call_in_origin(hub, bot, name, args, Some(&origin))?;
    if !changes || name == "routine_webhook" {
        #[cfg(test)]
        crate::fork::sessoes::call_tests::before_effect(&hub.store, "routine_read");
        origin.recheck(hub, bot)?;
    }
    Ok(result)
}

fn call_in_origin(
    hub: &Arc<Hub>,
    bot: &str,
    name: &str,
    args: &Value,
    origin: Option<&tool_origin::ToolOrigin>,
) -> Result<Value> {
    anyhow::ensure!(
        !matches!(name, "save_routine" | "duplicate_routine") || args.get("heartbeat").is_none(),
        crate::fork::heartbeat::SO_O_DONO
    );
    if origin.is_none() {
        let changes = matches!(name, "save_routine" | "set_routine_enabled" | "delete_routine" | "run_routine")
            || (name == "routine_webhook" && args["rotate"] == true);
        if changes {
            crate::chat::manage::user_turn(hub, bot)?;
        }
    }
    match name {
        "list_routines" => Ok(hub.routines.list(bot)),
        "save_routine" => hub.routines.save_in_origin(hub, bot, args, origin, || {}),
        "duplicate_routine" => hub.routines.duplicate_in_origin(hub, bot, required(args, "id")?, origin, || {}),
        "set_routine_enabled" => hub.routines.set_enabled_in_origin(
            &hub.store,
            bot,
            required(args, "id")?,
            args["enabled"].as_bool().context("enabled is required")?,
            origin,
        ),
        "delete_routine" => hub.routines.remove_in_origin(&hub.store, bot, required(args, "id")?, origin),
        "run_routine" => hub.routines.enqueue_in_origin(
            &hub.store,
            bot,
            required(args, "id")?,
            json!({"source":"test"}),
            None,
            true,
            origin,
        ),
        "routine_webhook" => hub.routines.credentials_in_origin(
            hub,
            bot,
            required(args, "id")?,
            args["rotate"].as_bool().unwrap_or(false),
            origin,
        ),
        _ => bail!("unknown routine tool"),
    }
}

pub const INSTRUCTIONS: &str = "Keep secrets out of routine instructions and ordinary status summaries.\nAntes de usar, chame ajuda_instrucoes para o guia.\nO servidor conserva a execução e a mensagem humana de origem até a mesma transação que grava rotina e fila; chamadas de outra sessão ou revogadas são recusadas como chamada vencida, sem efeito parcial.\nLeituras e chaves de webhook revalidam antes da resposta.\nToda criação pelo MCP, inclusive em turno humano, gasta a cota por bot em 24 horas móveis (padrão 10, ajustável só pelo dono).\nManter não pausa nem renova orçamento; Cancelar preserva o histórico e não desfaz efeitos já admitidos.\nUma tarefa filha herda a cadeia e os limites de execução da origem, sem renovar os 50 nem aceitar recibos do modelo.\nAlterar, pausar, apagar, disparar manualmente ou girar a chave ainda exige pedido do dono na própria sessão.\nSe a cota ou a origem forem recusadas, explique a limitação; nunca contorne criando pelo app, por shell, outra conta, outro bot ou outro agendador.\nNas ocorrências de relógio e testes manuais, o Hub conserva o pedido e conta cada nova tentativa com IA, inclusive retomadas após reinício, até 50 tentativas por pedido.\nAo atingir o limite, informe a interrupção e preserve o histórico; nunca crie o mesmo pedido de novo para contornar o limite.\nRotinas antigas já iniciadas sem registros confiáveis podem aparecer como Retomando (recovering), mas esse estado não confirma saldo de tentativas nem autoriza nova IA.\nAo preparar uma chamada sem recibo confiável, o Hub confirma Interrompida e o motivo antes de qualquer prompt.\nSem sessão salva, encerra sem envio.\nSe a gravação falhar, preserva a reserva e repete a mesma preparação, sem nova IA ou efeito externo.\nA colaboração legada entre bots e a continuação genérica após configurar conexão/login ainda não estão cobertas por esse controle; não anuncie proteção global nem webhooks liberados.\nGatilhos de relógio seguem a pausa global; webhooks locais e públicos recusam execução enquanto o controle de cadeia do Hub estiver indisponível.\nCrie rotinas e recordações como tarefas para você mesmo; nunca use IDs de outro bot ou Space.\nAgendamento só existe pelas Rotinas da Mexerica: use o MCP routines (save_routine, list_routines, set_routine_enabled, delete_routine, run_routine, routine_webhook), respeitando as aprovações e restrições atuais do Hub.\nNunca use agendamento nativo do motor, skills schedule/routines/loop do Claude, rotinas na nuvem do provedor, cron do sistema ou processo em segundo plano como alternativa.\nSe routines faltar, recusar ou falhar, explique a limitação; não agende de outro jeito nem afirme sucesso.\nUse once for a single dated execution; never approximate a one-time task with an annually repeating cron expression.\nDo not claim success if the tool failed.\nA pausa global de rotinas recusa novos disparos, inclusive testes manuais e eventos externos; trabalho já reservado ou em recuperação pode continuar. run_routine executes the real task immediately, including its side effects; use it when the user requested a test or immediate execution, not merely to validate a schedule.\nWebhook triggers: routine_webhook explains how senders deliver; never claim a third-party integration is connected until a real delivery has been verified.\nFor recurring clock schedules, always use a five-field cron trigger with an explicit IANA timezone, including minute/hour frequencies; do not create interval triggers for those requests.";
pub(crate) const INSTRUCTIONS_GUIDE: &str = "Create and manage persistent Mexerica routines when the user asks for scheduled or event-driven work.\nD544/D547: todos os bots, inclusive o Berga, podem criar rotinas e recordações para si sem pedido humano, com execução e origem válidas.\nO Hub avisa o dono e mostra Manter/Cancelar no Mission Board; sem resposta, a tarefa continua ativa.\nCriação a partir de conteúdo de fora (D125: entrada colada, anexo, resposta, resultado MCP, skill ou histórico externo) nasce pausada: o Hub nega ligar ou rodar pelo modelo (routine_external_release) e só o dono libera no cartão do Mission Board ou em Rotinas; o silêncio a mantém pausada.\nCriação direta no app é do dono e não gasta a cota MCP.\nPublicar resultado salvo não consome tentativa.\nExcluir o bot com sucesso confirma Cancelada antes de encerrá-lo.\nO cartão D84 com delta ainda não está implementado.\nA agenda interna de trabalho nasce disponível para todos os bots, inclusive o Berga, sem plugin nem concessão por bot (D538/D544).\nPreserve existing intervals when editing unrelated fields.\nCron steps reset within their field: */7 minutes is not an exact seven-minute duration across hours.\nBefore saving, resolve the task, trigger and intended timezone for calendar/one-time schedules from the conversation; ask only for missing or ambiguous details.\nIf the request is already clear, create it without another confirmation.\nCheck that tools, connector sign-ins and files needed by the task are available; describe any missing setup rather than promising the task will work.\nUse list_routines before creating or editing to avoid duplicates; preserve unrelated triggers, enabled state and instructions.\nA criação retorna o ID, enabled e nextRunAt; descreva a programação fornecida e o estado confirmado, sem afirmar que executou.\nA routine runs locally in a session of its own; the host must be running and awake, its result appears in your chat with the user, and each run ends by declaring its outcome with report_routine_result, only inside that run's own session (no_news stays silent; without a valid declaration the run shows as unconfirmed, and failures the Hub observed always win).
Vários horários no mesmo dia = um gatilho cron por horário, mesmos dias e fuso, numa rotina só; não crie uma rotina por horário.";

/// A sessão de uma execução (`routine-<run>`) também lista `report_routine_result` (I-023).
pub fn tools_for(slot: Option<&str>) -> Value {
    let id = json!({"type":"string"});
    let mut tools = vec![
        json!({"name":"list_routines","description":"Lista as rotinas deste bot e seu histórico, com o estado atual, os motivos de interrupção já confirmados e o desfecho de cada execução (outcome: o que o bot declarou ou a falha que o Hub observou; declaração não prova efeito externo).","inputSchema":{"type":"object","properties":{}}}),
    ];
    tools.push(json!({"name":"save_routine","description":"Create or replace a routine. Omit id to create; include id to update. triggers is an OR list. Use cron for recurring clock schedules; interval is for explicitly requested elapsed-duration semantics or existing schedules: interval {type:interval,seconds}; once {type:once,at:Unix milliseconds}; cron {type:cron,expression:five cron fields,timeZone:IANA zone}; webhook {type:webhook}; event {type:event,source:slack/github/origin/microsoftTeams/linear/sentry/pagerduty/email,event:event name or *,filters:field-to-value map}. Events require a configured webhook sender. Minimum interval is one second. D538/D544: todos os bots, inclusive o Berga, podem criar suas próprias rotinas e recordações (once), sem plugin ou concessão por bot. O bot só acessa suas tarefas, dentro do seu Space; criação dispensa pedido humano, mas gasta cota e gera aviso/cartão ao dono; edição exige pedido do dono na sessão de origem. Criação a partir de conteúdo de fora (D125) nasce pausada e o modelo não liga nem roda: só o dono libera no app. Para criar omita id (null é inválido).","inputSchema":{"type":"object","properties":{"id":id,"name":{"type":"string"},"instruction":{"type":"string"},"enabled":{"type":"boolean"},"timeoutSeconds":{"type":"integer","minimum":1,"maximum":86400},"triggers":{"type":"array","items":{"type":"object"}}},"required":["name","instruction","triggers"]}}));
    tools.push(json!({"name":"duplicate_routine","description":"Duplicate one of this bot's routines. Creates a disabled copy named \"<name> (cópia)\" with the same instruction, clock and event triggers; webhook triggers keep working under a fresh secret and the original keeps working. The copy never inherits runs, errors or computed schedules. Like any creation it spends quota and notifies the owner. Needs the routine id.","inputSchema":{"type":"object","properties":{"id":id},"required":["id"]}}));
    for (name, description) in [
        ("set_routine_enabled", "Pause or resume a routine; enabled boolean is required."),
        ("delete_routine", "Delete a routine and cancel pending runs."),
        (
            "run_routine",
            "Inicia um teste real, inclusive para rotina pausada individualmente; a pausa global recusa novos disparos. O pedido conserva suas tentativas após reinício e é interrompido ao atingir o limite, com motivo no histórico.",
        ),
        (
            "routine_webhook",
            "Get the routine's webhook: its public url (through the Mexerica cloud; null when the cloud is off, then only localUrl on this computer works), localUrl and per-routine key. Senders POST with Authorization: Bearer <key>; for GitHub, set the webhook's content type to application/json and its secret to the key (deliveries are verified by X-Hub-Signature-256) and they become events {source:github, event:<event>.<action>, repo, sender, text, url, payload} that event triggers can filter. The Worker encrypts the event for the Hub and does not queue it. Offline or older Hubs return 503 with Retry-After; if the sender does not retry, the event is lost. A lost ACK is uncertain; reuse a stable delivery ID. Until Hub chain authority is integrated, local and public webhooks refuse execution and show the reason in the routine conversation. Delivery attempts are limited to 30 per minute and 2,000 per day per user across Hubs, and 10,000 per day globally; excess returns 429 with Retry-After. rotate:true replaces the key; the old one stops working at once. This doesn't register a provider subscription: set that up with the user, and report the webhook as awaiting connection until then.",
        ),
    ] {
        let required = if name == "set_routine_enabled" { vec!["id", "enabled"] } else { vec!["id"] };
        let mut properties = json!({"id": id});
        match name {
            "set_routine_enabled" => properties["enabled"] = json!({"type":"boolean"}),
            "routine_webhook" => properties["rotate"] = json!({"type":"boolean"}),
            _ => {}
        }
        tools.push(json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required}}));
    }
    for tool in &mut tools {
        if let Some(description) = tool["description"].as_str() {
            let description = format!(
                "{description} O Hub conserva a execução original; vínculo revogado ou de outra sessão recusa a gravação/fila e a entrega de leituras."
            );
            tool["description"] = json!(description);
        }
    }
    if crate::fork::rotina_resultado::offered(slot) {
        tools.push(crate::fork::rotina_resultado::tool());
    }
    json!(tools)
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn routine() -> Routine {
        Routine {
            id: "routine".into(),
            bot_id: "bot".into(),
            name: "Check".into(),
            instruction: "Check status".into(),
            triggers: vec![Trigger::Interval { seconds: 60 }],
            enabled: true,
            created_at: 0,
            updated_at: 0,
            next_runs: vec![Some(60_000)],
            webhook_key: "secret".into(),
            deleted: false,
            timeout_seconds: default_timeout(),
            last_error: None,
            schedule_generation: 0,
            creation_origin: None,
            external_creation: false,
            external_release_pending: None,
            heartbeat: None,
            heartbeat_state: None,
        }
    }
    #[test]
    fn guide_teaches_one_cron_trigger_per_time_in_a_single_routine() {
        assert!(
            INSTRUCTIONS_GUIDE.contains("um gatilho cron por horário")
                && INSTRUCTIONS_GUIDE.contains("não crie uma rotina por horário"),
            "o bot deve ler um gatilho por horário, numa rotina só"
        );
    }
    #[test]
    fn list_returns_the_ready_portuguese_agenda_beside_descriptions() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let routines = Routines::default();
        routines
            .change(&store, |s| {
                let mut r = routine();
                r.triggers = vec![
                    Trigger::Cron { expression: "0 9 * * *".into(), time_zone: "America/Sao_Paulo".into() },
                    Trigger::Cron { expression: "0 15 * * *".into(), time_zone: "America/Sao_Paulo".into() },
                ];
                r.next_runs = vec![Some(1), Some(2)];
                s.routines.push(r);
                Ok(())
            })
            .unwrap();
        let listed = routines.list("bot");
        assert_eq!(listed["routines"][0]["agenda"], "Todo dia às 9h e às 15h");
        assert_eq!(listed["routines"][0]["triggerDescriptions"].as_array().unwrap().len(), 2);
    }
    fn saved_source(s: &mut State, id: &str, triggers: &Value) -> Routine {
        let body = json!({"name":"Café","instruction":"Avisar no café","triggers":triggers,"timeoutSeconds":120});
        let mut routine = save_in(s, "bot", &body).unwrap();
        s.routines.pop();
        routine.id = id.into();
        routine
    }

    #[test]
    fn duplicate_copies_cron_disabled_with_a_fresh_schedule() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let routines = Routines::default();
        routines
            .change(&store, |s| {
                let mut source = saved_source(
                    s,
                    "src",
                    &json!([
                        {"type":"cron","expression":"0 9 * * *","timeZone":"America/Sao_Paulo"},
                        {"type":"cron","expression":"0 15 * * *","timeZone":"America/Sao_Paulo"},
                    ]),
                );
                source.next_runs = vec![None, None];
                source.last_error = Some("falhou ontem".into());
                s.routines.push(source);
                Ok(())
            })
            .unwrap();
        let copy = routines.change(&store, |s| duplicate_in(s, "bot", "src")).unwrap();
        assert_eq!(copy.name, "Café (cópia)");
        let source = routines.state.locked().routines.iter().find(|r| r.id == "src").unwrap().clone();
        assert_eq!(copy.instruction, source.instruction);
        assert_eq!(copy.triggers, source.triggers);
        assert_eq!(copy.timeout_seconds, 120);
        assert!(!copy.enabled);
        assert_ne!(copy.id, source.id);
        assert_ne!(copy.webhook_key, source.webhook_key);
        assert!(copy.next_runs.iter().all(Option::is_some), "agenda recalculada do zero");
        assert_eq!(copy.last_error, None);
    }

    #[test]
    fn duplicate_inherits_external_release_pending() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let routines = Routines::default();
        routines
            .change(&store, |s| {
                let mut source = saved_source(s, "src", &json!([{"type":"interval","seconds":60}]));
                source.external_creation = true;
                source.external_release_pending = None;
                s.routines.push(source);
                Ok(())
            })
            .unwrap();
        let copy = routines.change(&store, |s| duplicate_in(s, "bot", "src")).unwrap();
        assert!(copy.external_creation);
        assert!(copy.external_release_pending.is_some());
        assert!(!copy.enabled);
    }

    #[test]
    fn duplicate_copies_events_and_rotates_webhook_secrets() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let routines = Routines::default();
        routines
            .change(&store, |s| {
                let mut evented =
                    saved_source(s, "evt", &json!([{"type":"event","source":"github","event":"push","filters":{}}]));
                evented.id = "evt".into();
                s.routines.push(evented);
                let mut hooked = saved_source(s, "hook", &json!([{"type":"webhook"}]));
                hooked.id = "hook".into();
                hooked.webhook_key = "segredo-antigo".into();
                s.routines.push(hooked);
                Ok(())
            })
            .unwrap();
        let event_copy = routines.change(&store, |s| duplicate_in(s, "bot", "evt")).unwrap();
        assert_eq!(event_copy.triggers.len(), 1);
        assert!(matches!(event_copy.triggers[0], Trigger::Event { .. }));
        assert!(!event_copy.enabled);
        let hook_copy = routines.change(&store, |s| duplicate_in(s, "bot", "hook")).unwrap();
        assert_ne!(hook_copy.webhook_key, "segredo-antigo");
        assert_ne!(hook_copy.webhook_key, "");
        assert!(!hook_copy.enabled);
        assert_eq!(hook_copy.next_runs, vec![None]);
        let kept = routines.state.locked().routines.iter().find(|r| r.id == "hook").unwrap().clone();
        assert_eq!(kept.webhook_key, "segredo-antigo", "o original continua válido");
        assert!(kept.enabled);
    }

    #[test]
    fn duplicate_refuses_unknown_routines_other_bots_and_a_full_hub() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let routines = Routines::default();
        routines
            .change(&store, |s| {
                let mut foreign = saved_source(s, "alheia", &json!([{"type":"interval","seconds":60}]));
                foreign.id = "alheia".into();
                foreign.bot_id = "outro".into();
                s.routines.push(foreign);
                Ok(())
            })
            .unwrap();
        for id in ["sumida", "alheia"] {
            let Err(error) = routines.change(&store, |s| duplicate_in(s, "bot", id)) else {
                panic!("duplicação de {id} deveria recusar")
            };
            assert_eq!(format!("{error:#}"), "routine not found");
        }
        routines
            .change(&store, |s| {
                for n in 0..999 {
                    save_in(s, "bot", &json!({"name":format!("R{n}"),"instruction":"x","triggers":[{"type":"interval","seconds":60}]}))?;
                }
                Ok(())
            })
            .unwrap();
        let Err(error) = routines.change(&store, |s| duplicate_in(s, "bot", "alheia")) else {
            panic!("duplicação alheia deveria recusar")
        };
        assert_eq!(format!("{error:#}"), "routine not found");
        let full = routines.change(&store, |s| {
            save_in(
                s,
                "bot",
                &json!({"name":"Cheia","instruction":"x","triggers":[{"type":"interval","seconds":60}]}),
            )?;
            Ok(())
        });
        assert!(full.is_err(), "o teto segue valendo na criação");
        routines
            .change(&store, |s| {
                let mut mine = routine();
                mine.id = "minha".into();
                s.routines.push(mine);
                Ok(())
            })
            .unwrap();
        let Err(error) = routines.change(&store, |s| duplicate_in(s, "bot", "minha")) else {
            panic!("duplicação com o teto cheio deveria recusar")
        };
        assert_eq!(format!("{error:#}"), "Limite de 1000 rotinas atingido. Apague uma rotina para duplicar.");
    }

    #[test]
    fn gateway_run_lookup_is_bound_to_bot_and_checks_status_beyond_the_public_page() {
        let routines = Routines::default();
        let r = routine();
        let mut live = new_run(&r, json!({"payload":"synthetic"}), None);
        live.status = Status::Running;
        let id = live.id.clone();
        let mut state = routines.state.locked();
        state.runs.push(live);
        for _ in 0..201 {
            let mut finished = new_run(&r, Value::Null, None);
            finished.status = Status::Succeeded;
            state.runs.push(finished);
        }
        drop(state);
        assert!(routines.guard_run_live("bot", &id));
        assert!(!routines.guard_run_live("other", &id));
        assert!(!routines.guard_run_live("bot", "missing"));
        for (status, expected) in [
            (Status::Pending, false),
            (Status::Starting, true),
            (Status::Recovering, true),
            (Status::Succeeded, false),
            (Status::Failed, false),
            (Status::Interrupted, false),
            (Status::Cancelled, false),
        ] {
            routines.state.locked().runs[0].status = status;
            assert_eq!(routines.guard_run_live("bot", &id), expected);
        }
    }
    #[test]
    fn pending_runs_survive_restart_but_running_runs_are_not_duplicated() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let routines = Routines::default();
        let r = routine();
        let pending = new_run(&r, Value::Null, None);
        let mut running = new_run(&r, Value::Null, None);
        running.status = Status::Running;
        routines
            .change(&store, |s| {
                s.routines.push(r);
                s.runs.extend([pending, running]);
                Ok(())
            })
            .unwrap();
        let restored = Routines::default();
        restored.restore(&store).unwrap();
        let state = restored.state.locked();
        assert!(state.runs[0].status == Status::Pending);
        assert!(state.runs[1].status == Status::Recovering);
        let id = state.runs[1].id.clone();
        drop(state);
        assert!(restored.prepare_attempt(&store, "bot", &id).is_err());
        let state = restored.state.locked();
        assert!(state.runs[1].status == Status::Interrupted);
    }
    #[test]
    fn team_conflicts_ignore_execution_metadata_but_preserve_editable_changes() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let routines = Routines::default();
        let original = routine();
        let expected = vec![original.clone()];
        let mut executed = original.clone();
        executed.next_runs = vec![Some(now_ms() + 99_000)];
        executed.last_error = Some("transient scheduler error".into());
        executed.updated_at += 1;
        assert!(same_team_routines(&[executed.clone()], &expected), "approval uses editable fields only");
        routines
            .change(&store, |s| {
                s.routines.push(executed);
                Ok(())
            })
            .unwrap();
        routines.restore_team("bot", &expected, &expected, |raw| store.kv_set(STATE_KEY, raw)).unwrap();
        let mut edited = original.clone();
        edited.instruction = "later user edit".into();
        assert!(!same_team_routines(&[edited.clone()], &expected));
        routines
            .change(&store, |s| {
                s.routines = vec![edited];
                Ok(())
            })
            .unwrap();
        assert!(routines.restore_team("bot", &expected, &[], |_| Ok(())).is_err());
    }

    #[test]
    fn creation_rollback_cancels_work_already_queued_by_the_scheduler() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let routines = Routines::default();
        let r = routine();
        routines
            .change(&store, |s| {
                s.runs.push(new_run(&r, Value::Null, None));
                s.routines.push(r);
                Ok(())
            })
            .unwrap();
        routines.discard_team_creation("bot", |raw| store.kv_set(STATE_KEY, raw)).unwrap();
        let state = routines.state.locked();
        assert_eq!(state.routines.len(), 0);
        assert!(state.runs.iter().all(|r| r.status == Status::Cancelled));
    }

    #[test]
    fn pause_cancels_pending_and_resume_recalculates_schedule() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let routines = Routines::default();
        let r = routine();
        routines
            .change(&store, |s| {
                s.runs.push(new_run(&r, Value::Null, None));
                s.routines.push(r);
                Ok(())
            })
            .unwrap();
        routines.set_enabled(&store, "bot", "routine", false).unwrap();
        assert!(routines.state.locked().runs[0].status == Status::Cancelled);
        routines.set_enabled(&store, "bot", "routine", true).unwrap();
        assert!(routines.state.locked().routines[0].next_runs[0].unwrap() > now_ms());
    }
    #[test]
    fn duplicate_delivery_is_idempotent_and_cross_bot_edits_fail() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let routines = Routines::default();
        routines
            .change(&store, |s| {
                s.routines.push(routine());
                Ok(())
            })
            .unwrap();
        let first = routines.enqueue(&store, "bot", "routine", Value::Null, Some("same".into()), false).unwrap();
        let second = routines.enqueue(&store, "bot", "routine", Value::Null, Some("same".into()), false).unwrap();
        assert_eq!(first["run"]["id"], second["run"]["id"]);
        assert!(routines.set_enabled(&store, "other", "routine", false).is_err());
        assert!(routines.remove(&store, "other", "routine").is_err());
        assert!(routines.enqueue(&store, "bot", "routine", Value::Null, None, false).is_err());
    }

    fn cron_trigger(expression: &str, time_zone: &str) -> Value {
        json!({"type":"cron","expression":expression,"timeZone":time_zone})
    }

    /// Só nome/instrução mudaram: a agenda salva e a pendência continuam iguais.
    #[test]
    fn rename_only_keeps_next_runs_and_pending() {
        let mut s = State::default();
        let mut r = routine();
        r.triggers = vec![Trigger::Cron { expression: "0 9 * * *".into(), time_zone: "America/Sao_Paulo".into() }];
        r.next_runs = vec![Some(1_800_000_000_000)];
        let run = new_run(&r, Value::Null, None);
        s.routines.push(r);
        s.runs.push(run);
        let original_triggers = s.routines[0].triggers.clone();
        let body = json!({"id":"routine","name":"Novo nome","instruction":"Check status",
            "triggers":[cron_trigger("0 9 * * *", "America/Sao_Paulo")]});
        let saved = save_in(&mut s, "bot", &body).unwrap();
        assert_eq!(saved.next_runs, vec![Some(1_800_000_000_000)]);
        assert_eq!(saved.triggers, original_triggers);
        assert_eq!(s.runs.len(), 1);
        assert!(s.runs[0].status == Status::Pending);
    }

    /// Trocar o fuso recalcula a próxima ocorrência e não perde a pendência.
    #[test]
    fn changing_timezone_recomputes_next_without_losing_pending() {
        let mut s = State::default();
        let mut r = routine();
        r.triggers = vec![Trigger::Cron { expression: "0 9 * * *".into(), time_zone: "America/Sao_Paulo".into() }];
        r.next_runs = vec![Some(1_800_000_000_000)];
        let run = new_run(&r, Value::Null, None);
        s.routines.push(r);
        s.runs.push(run);
        let body = json!({"id":"routine","name":"Check","instruction":"Check status",
            "triggers":[cron_trigger("0 9 * * *", "America/New_York")]});
        let before = now_ms();
        let saved = save_in(&mut s, "bot", &body).unwrap();
        let next = saved.next_runs[0].unwrap();
        assert!(next >= before, "recalcula a partir de agora");
        assert_ne!(next, 1_800_000_000_000);
        let trigger = Trigger::Cron { expression: "0 9 * * *".into(), time_zone: "America/New_York".into() };
        // `next` carrega os milissegundos do instante do salvamento (o croner
        // preserva a parte sub-segundo da consulta); um minuto antes cai na
        // mesma ocorrência, um milissegundo antes já rola para o dia seguinte.
        assert_eq!(trigger.next(next - 60_000).unwrap(), Some(next), "é ocorrência válida da nova agenda");
        assert_eq!(s.runs.len(), 1);
        assert!(s.runs[0].status == Status::Pending);
    }
}

#[cfg(test)]
#[path = "../fork/heartbeat/testes_rotina.rs"]
mod heartbeat_rotina_tests;
