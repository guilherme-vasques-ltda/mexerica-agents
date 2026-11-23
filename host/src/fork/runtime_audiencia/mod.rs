//! Projeção de runtime para a audiência do `BotJSON`; não altera estado nem concede acesso.
use crate::{
    LockExt,
    hub::{BotStatus, Hub, Runtime},
    store::BotConfig,
};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;

pub(crate) mod live_activity;
pub(crate) mod screen;

/// Vocabulário externo: indisponibilidade não é um sexto estado do Runtime.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum Status {
    Idle,
    Working,
    NeedsInput,
    Waiting,
    Error,
    Unavailable,
}

impl From<BotStatus> for Status {
    fn from(status: BotStatus) -> Self {
        match status {
            BotStatus::Idle => Self::Idle,
            BotStatus::Working => Self::Working,
            BotStatus::NeedsInput => Self::NeedsInput,
            BotStatus::Waiting => Self::Waiting,
            BotStatus::Error => Self::Error,
        }
    }
}

/// Sem membro ativo selecionado, repouso agregado exige observação de todos os membros.
/// O agregador chama sob a mesma trava usada para escolher o runtime real.
pub(crate) fn group_idle_observed(group: &BotConfig, runtime: &HashMap<String, Runtime>) -> bool {
    !group.members.is_empty() && group.members.iter().all(|member| runtime.contains_key(member))
}

/// Única escrita dos seis campos de runtime em snapshots completos/incrementais e eventos.
pub(crate) fn project(hub: &Hub, cfg: &BotConfig, rt: Option<&Runtime>, bot: &mut Value) {
    project_in_space(hub, cfg, &cfg.space_id, rt, bot);
}

/// Consumidores efêmeros usam a mesma redação canônica, no contexto solicitado.
pub(crate) fn project_in_space(hub: &Hub, cfg: &BotConfig, space: &str, rt: Option<&Runtime>, bot: &mut Value) {
    let Some(rt) = rt else {
        redact(bot);
        return;
    };
    // MB-1b: só as perguntas cuja conversa a regra canônica autoriza neste Space (texto, ids e contagem).
    // O canal não herda as perguntas do membro: elas aparecem no cartão do próprio bot e no transcript.
    let questions: Vec<_> = rt
        .pending_questions
        .iter()
        .filter(|q| !cfg.is_group() && audience_in_space(hub, cfg, space, Some(&q.chat_id), false).unwrap_or(false))
        .collect();
    let idle =
        rt.status == BotStatus::Idle && rt.activity.is_empty() && rt.started_at.is_none() && rt.resume_at.is_none();
    if !audience_in_space(hub, cfg, space, rt.lane.as_ref().map(|l| l.chat.as_str()), idle).unwrap_or(false) {
        redact(bot);
        bot["pendingQuestions"] = json!(questions);
        return;
    }
    bot["pendingQuestions"] = json!(questions);
    bot["status"] = json!(Status::from(rt.shown_status(!questions.is_empty())));
    bot["activity"] = rt.activity.clone().into();
    // MX-147: código opcional e estável; vai junto do texto e sai junto na redação.
    bot["activityCode"] =
        rt.activity_code.as_ref().and_then(|code| serde_json::to_value(code).ok()).unwrap_or(Value::Null);
    bot["startedAt"] = rt.started_at.into();
    bot["resumeAt"] = rt.resume_at.into();
    bot["workingChat"] = rt.lane.as_ref().map(|l| l.chat.clone()).into();
}

/// Revalida o vínculo atual de uma conversa capturada, inclusive ao invalidar o
/// contexto anterior. A lane atual pode ter mudado; a audiência continua canônica.
pub(crate) fn permits_conversation_in_space(hub: &Hub, cfg: &BotConfig, space: &str, chat: &str) -> bool {
    conversation_in_space(hub, cfg, space, chat).unwrap_or(false)
}

pub(crate) fn conversation_in_space(hub: &Hub, cfg: &BotConfig, space: &str, chat: &str) -> Result<bool> {
    audience_in_space(hub, cfg, space, Some(chat), false)
}

/// A fila e o catch-up podem sobreviver à mudança de Runtime ou audiência.
/// Só conserva o runtime que ainda coincide com a projeção atual, sem trocar revisão/configuração.
pub(crate) fn revalidate_event(hub: &Hub, space: &str, bot: &mut Value) {
    if bot["deleted"] == true {
        bot["pendingQuestions"] = json!([]);
        return;
    }
    let live = bot["id"].as_str().and_then(|id| hub.store.bot(id).ok().flatten()).filter(|row| !row.deleted);
    // MB-1b S3: as perguntas são revalidadas pela conversa de cada uma, pela MESMA projeção do snapshot deste
    // Space, independentemente da disponibilidade do runtime (que continua redigido sem revelar a atividade).
    let questions = live.as_ref().map_or_else(
        || json!([]),
        |row| {
            let mut fresh = json!({});
            project_in_space(hub, &row.config, space, hub.observed_runtime(&row.config.id).as_ref(), &mut fresh);
            fresh["pendingQuestions"].take()
        },
    );
    let row = live.filter(|row| row.config.space_id == space);
    let known = matches!(bot["status"].as_str(), Some("idle" | "working" | "needsInput" | "waiting" | "error"));
    if !known
        || !row.as_ref().is_some_and(|row| {
            // Vale para individuais e grupos: vínculo antigo aberto não prova estado atual.
            let current = hub.bot_json(row);
            ["status", "activity", "activityCode", "startedAt", "resumeAt", "workingChat"]
                .iter()
                .all(|key| bot[*key] == current[*key])
        })
    {
        redact(bot);
    }
    bot["pendingQuestions"] = questions;
}

/// O hello e os bots preparados no catch-up também precisam de comprovação no consumo.
pub(crate) fn revalidate_snapshot(hub: &Hub, space: &str, mut event: Value) -> Option<Value> {
    if event["type"] == "hello" {
        event["screen"] = screen::state(hub, space);
        return Some(event);
    }
    crate::fork::spaces::filter_event(hub, space, event)
}

fn redact(bot: &mut Value) {
    bot["pendingQuestions"] = json!([]);
    bot["status"] = json!(Status::Unavailable);
    bot["activity"] = json!("");
    bot["activityCode"] = Value::Null;
    bot["startedAt"] = Value::Null;
    bot["resumeAt"] = Value::Null;
    bot["workingChat"] = Value::Null;
}

fn config(c: &Connection, id: &str) -> Result<Option<BotConfig>> {
    let raw: Option<String> =
        c.query_row("SELECT config FROM bots WHERE id=?1 AND deleted=0", [id], |r| r.get(0)).optional()?;
    raw.map(|raw| serde_json::from_str(&raw).map_err(Into::into)).transpose()
}

/// Runtime é clonado antes desta leitura; nunca conserva a trava de runtime ao acessar DB.
/// Toda comprovação canônica usa a mesma Connection, sem Store/Cloud recursivos.
/// O contexto solicitado é separado da identidade nativa; não forja configuração/vínculo.
fn audience_in_space(
    hub: &Hub,
    cfg: &BotConfig,
    requested_space: &str,
    chat: Option<&str>,
    idle: bool,
) -> Result<bool> {
    let c = hub.store.db.locked();
    let Some(current) = config(&c, &cfg.id)? else { return Ok(false) };
    if current.space_id != cfg.space_id || current.is_group() != cfg.is_group() {
        return Ok(false);
    }
    let raw: String = c.query_row("SELECT v FROM kv WHERE k='donna.spaces.v1'", [], |r| r.get(0))?;
    let spaces: crate::fork::spaces::Snapshot = serde_json::from_str(&raw)?;
    if !spaces.spaces.iter().any(|s| s.id == requested_space) {
        return Ok(false);
    }
    // Sem lane, só há evidência de repouso limpo na conversa própria; erro/relógio não provam contexto.
    let chat = match chat {
        Some(chat) => chat,
        None if idle && requested_space == cfg.space_id => &cfg.id,
        None => return Ok(false),
    };
    let conversation: Option<(String, String, bool)> = c
        .query_row("SELECT space_id,bot_id,closed FROM donna_space_conversations WHERE id=?1", [chat], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?;
    let Some((space, owner, closed)) = conversation else { return Ok(false) };
    if closed || space != requested_space {
        return Ok(false);
    }
    if requested_space != current.space_id {
        // A mesma regra canônica das conversas adicionais: somente a principal atual,
        // na conversa que realmente a referencia. Omissão/corrupção não concede vínculo.
        let raw: Option<String> =
            c.query_row("SELECT v FROM kv WHERE k='team.settings.v1'", [], |r| r.get(0)).optional()?;
        let settings = raw.map(|raw| serde_json::from_str::<crate::store::TeamSettings>(&raw)).transpose()?;
        if cfg.is_group() || owner != cfg.id || settings.and_then(|s| s.primary_bot_id).as_deref() != Some(&cfg.id) {
            return Ok(false);
        }
    }
    if cfg.is_group() {
        if chat != cfg.id || owner != cfg.id {
            return Ok(false);
        }
        // O agregado usa esta lista e sua ordem para escolher o primeiro membro ativo.
        // Inclusão, remoção ou reorder exigem outro snapshot, mesmo quando o antigo era Idle.
        if cfg.members != current.members {
            return Ok(false);
        }
        for member in &current.members {
            if !config(&c, member)?.is_some_and(|m| !m.is_group() && m.space_id == space) {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    if owner == cfg.id {
        return Ok(true);
    }
    // Uma lane de grupo exige a associação atual; igualdade de Space sozinha não é vínculo.
    Ok(config(&c, chat)?.is_some_and(|group| {
        owner == group.id && group.is_group() && group.space_id == space && group.members.contains(&cfg.id)
    }))
}

#[cfg(test)]
mod tests;
