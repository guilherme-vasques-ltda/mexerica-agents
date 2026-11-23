use super::model::{ConversationRef, Observation, Open, Pending, Question, TowerBot, TowerSnapshot, Usage, Work};
use crate::{
    LockExt,
    fork::{runtime_audiencia, sessoes::store as sessions},
    hub::{BotStatus, Hub},
    store::{BotConfig, now_ms},
};
use anyhow::{Result, ensure};
use rusqlite::{Connection, params};

pub(super) struct Conversation {
    pub id: String,
    pub bot_id: String,
    pub revision: i64,
    pub closed: bool,
}
impl Conversation {
    fn reference(&self) -> ConversationRef {
        ConversationRef { conversation_id: self.id.clone(), revision: self.revision }
    }
}

pub(super) fn conversations(c: &Connection, space: &str) -> Result<Vec<Conversation>> {
    Ok(c.prepare(
        "SELECT c.id,c.bot_id,c.revision,c.closed FROM donna_space_conversations c
         JOIN bots b ON b.id=c.bot_id AND b.deleted=0
         WHERE c.space_id=?1 ORDER BY c.is_primary DESC,c.revision,c.id",
    )?
    .query_map([space], |r| {
        Ok(Conversation { id: r.get(0)?, bot_id: r.get(1)?, revision: r.get(2)?, closed: r.get(3)? })
    })?
    .collect::<rusqlite::Result<_>>()?)
}

/// O SQL limita a audiência antes de construir identidade, título, atividade ou contagens.
pub(super) fn roster(c: &Connection, space: &str) -> Result<Vec<BotConfig>> {
    let rows = c
        .prepare(
            "SELECT b.config,b.avatar_config FROM bots b WHERE b.deleted=0
         AND COALESCE(json_extract(b.config,'$.kind'),'agent')='agent'
         AND (json_extract(b.config,'$.spaceId')=?1 OR
           (b.id=(SELECT json_extract(v,'$.primaryBotId') FROM kv WHERE k='team.settings.v1')
            AND EXISTS(SELECT 1 FROM donna_space_conversations c WHERE c.space_id=?1 AND c.bot_id=b.id)))
         ORDER BY b.id",
        )?
        .query_map([space], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    // Mesmo decodificador do roster canônico, na Connection já autorizada; sem novo lock do Store.
    rows.into_iter().map(|(profile, avatar)| crate::fork_avatars::restored(&profile, avatar.as_deref())).collect()
}

pub(super) fn snapshot(hub: &Hub, space: &str) -> Result<TowerSnapshot> {
    let db = hub.store.db.locked();
    let tx = db.unchecked_transaction()?;
    let raw: String = tx.query_row("SELECT v FROM kv WHERE k='donna.spaces.v1'", [], |r| r.get(0))?;
    let spaces: crate::fork::spaces::Snapshot = serde_json::from_str(&raw)?;
    ensure!(spaces.spaces.iter().any(|s| s.id == space), "space_not_found");
    let conversations = conversations(&tx, space)?;
    // Materializa somente dados do Space sob a Connection atual. Soltar DB antes
    // de observar runtime e chamar a projeção canônica, que revalida vínculos.
    let mut rows = Vec::new();
    for cfg in roster(&tx, space)? {
        let open = conversations
            .iter()
            .find(|c| c.bot_id == cfg.id && !c.closed)
            .map(|c| -> Result<Open> {
                let session_ref = sessions::get(&tx, &c.id)?
                    .filter(|s| s.principal && s.chat_id == c.id && s.state == "open")
                    .map(|s| sessions::SessionRef { id: s.id, chat_id: s.chat_id });
                Ok(Open {
                    label: "Abrir conversa do bot",
                    bot_id: cfg.id.clone(),
                    conversation: c.reference(),
                    session_ref,
                })
            })
            .transpose()?;
        let pending = pending(&tx, space, &cfg.id).unwrap_or(Observation::Unavailable);
        rows.push((cfg, open, pending));
    }
    let roster_ids: Vec<&str> = rows.iter().map(|(cfg, ..)| cfg.id.as_str()).collect();
    let delegated_permissions =
        super::delegated_permissions::project(&tx, space, &roster_ids).unwrap_or_else(|error| {
            tracing::warn!(error = %error, "permissões delegadas indisponíveis no Mission Board");
            Observation::Unavailable
        });
    let inter_bot_requests = crate::fork::retornos::pendentes::torre_na(&tx, space, &roster_ids)?;
    let roster: Vec<String> = roster_ids.iter().map(|id| (*id).to_owned()).collect();
    let routine_creations = crate::fork::routine_creation::projection(&tx, space, now_ms()).unwrap_or_else(|error| {
        tracing::warn!(error = format!("{error:#}"), "revisões de rotinas indisponíveis no Mission Board");
        Observation::Unavailable
    });
    let account_switches =
        crate::fork::fallback_contas::torre::projection(&tx, space, now_ms()).unwrap_or_else(|error| {
            tracing::warn!(error = format!("{error:#}"), "trocas de conta indisponíveis no Mission Board");
            Observation::Unavailable
        });
    tx.commit()?;
    drop(db);
    // MB-3: falha na coluna lateral não derruba o Board; vira "Não informado".
    let what_happened = crate::fork::resumo_diario::torre(hub, space, &roster).unwrap_or_else(|error| {
        tracing::warn!(error = format!("{error:#}"), "resumo do dia indisponível no Mission Board");
        Observation::Unavailable
    });
    let mut bots = Vec::new();
    for (cfg, open, pending) in rows {
        let (work, questions) = work(hub, &cfg, space, &conversations);
        let title = (cfg.space_id == space).then_some(cfg.title);
        bots.push(TowerBot {
            bot_id: cfg.id,
            name: cfg.name,
            title,
            avatar_color: cfg.avatar_color,
            avatar_shape: cfg.avatar_shape,
            avatar_config: cfg.avatar_config,
            mexeriquinha: cfg.mexeriquinha,
            runtime: work,
            open,
            pending,
            questions,
            queue: Observation::Unavailable,
            usage: Usage {
                daily_bot_tokens: Observation::Unavailable,
                subscription_limit: Observation::Unavailable,
                context_tokens: Observation::Unavailable,
            },
        });
    }
    Ok(TowerSnapshot {
        version: 1,
        hub_id: hub.host_id.clone(),
        snapshot_id: uuid::Uuid::new_v4().to_string(),
        collected_at: now_ms(),
        space_id: space.into(),
        bots,
        delegated_permissions,
        inter_bot_requests,
        what_happened,
        routine_creations,
        account_switches,
        attention: super::attention::project(hub, space, true).map_or(Observation::Unavailable, |value| {
            Observation::Available { source: "donnaAttention", coverage: "authorizedSpace", value }
        }),
    })
}

fn work(
    hub: &Hub,
    cfg: &BotConfig,
    space: &str,
    conversations: &[Conversation],
) -> (Observation<Work>, Observation<Vec<Question>>) {
    let runtime = hub.observed_runtime(&cfg.id);
    let mut observed = serde_json::json!({});
    runtime_audiencia::project_in_space(hub, cfg, space, runtime.as_ref(), &mut observed);
    let questions = questions(runtime.is_some(), &observed, conversations);
    (runtime_work(cfg, &observed, conversations), questions)
}

pub(super) fn attention_questions(
    hub: &Hub,
    cfg: &BotConfig,
    space: &str,
    conversations: &[Conversation],
) -> Observation<Vec<Question>> {
    work(hub, cfg, space, conversations).1
}

/// MB-1b: somente o que a projeção canônica já autorizou, em conversa aberta capturada nesta coleta.
fn questions(
    observed_runtime: bool,
    observed: &serde_json::Value,
    conversations: &[Conversation],
) -> Observation<Vec<Question>> {
    if !observed_runtime {
        return Observation::Unavailable;
    }
    let value = observed["pendingQuestions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|q| {
            let chat = q["chatId"].as_str()?;
            let captured = conversations.iter().find(|c| c.id == chat && !c.closed)?;
            Some(Question {
                entry_id: q["entryId"].as_str()?.to_owned(),
                conversation: captured.reference(),
                session_id: q["sessionId"].as_str().map(str::to_owned),
                text: q["text"].as_str()?.to_owned(),
                asked_at: q["askedAt"].as_i64()?,
            })
        })
        .collect();
    Observation::Available {
        source: "hub.runtime.pendingQuestions",
        coverage: "ownerQuestionsInAuthorizedConversations",
        value,
    }
}

fn runtime_work(cfg: &BotConfig, observed: &serde_json::Value, conversations: &[Conversation]) -> Observation<Work> {
    // Só converte o vocabulário já autorizado pelo produtor; nunca classifica a audiência.
    let status = match observed["status"].as_str() {
        Some("idle") => BotStatus::Idle,
        Some("working") => BotStatus::Working,
        Some("needsInput") => BotStatus::NeedsInput,
        Some("waiting") => BotStatus::Waiting,
        Some("error") => BotStatus::Error,
        _ => return Observation::Unavailable,
    };
    let chat = observed["workingChat"].as_str();
    let Some(captured) = conversations.iter().find(|c| c.id == chat.unwrap_or(&cfg.id) && !c.closed) else {
        return Observation::Unavailable;
    };
    Observation::Available {
        source: "hub.runtime_audiencia",
        coverage: "currentAuthorizedConversationOrObservedIdle",
        value: Work {
            status,
            activity: (status == BotStatus::Working)
                .then(|| observed["activity"].as_str().filter(|s| !s.is_empty()).map(str::to_owned))
                .flatten(),
            activity_code: (status == BotStatus::Working)
                .then(|| observed["activityCode"].clone())
                .filter(|code| !code.is_null()),
            started_at: (status == BotStatus::Working).then(|| observed["startedAt"].as_i64()).flatten(),
            resume_at: (status == BotStatus::Waiting).then(|| observed["resumeAt"].as_i64()).flatten(),
            reason: match status {
                BotStatus::NeedsInput => Some("Precisa de você; confira a conversa autorizada."),
                BotStatus::Waiting => Some("Aguardando; confira a conversa autorizada."),
                BotStatus::Error => Some("Erro; confira a conversa autorizada."),
                _ => None,
            },
            conversation: chat.map(|_| captured.reference()),
        },
    }
}

fn pending(c: &Connection, space: &str, bot: &str) -> Result<Observation<Vec<Pending>>> {
    let captured: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_space_conversations c
         JOIN donna_bot_sessions s ON s.chat_id=c.id AND s.state='open'
         WHERE c.space_id=?1 AND c.bot_id=?2 AND c.closed=0)",
        params![space, bot],
        |r| r.get(0),
    )?;
    if !captured {
        return Ok(Observation::Unavailable);
    }
    // Somente IDs originais e únicos: nenhum texto/comando/argumento da aprovação.
    // A sessão é conferida no catálogo; não inferida da Lane nem do ID ACP.
    let rows = c
        .prepare(&format!(
            "SELECT e.id,c.id,c.revision FROM entries e
         JOIN donna_space_conversations c ON c.id=e.bot_id AND c.space_id=?1 AND c.bot_id=?2 AND c.closed=0
         JOIN donna_bot_sessions s ON s.id=COALESCE(e.session,e.bot_id) AND s.chat_id=c.id AND s.state='open'
         WHERE {} ORDER BY e.id",
            super::PENDING_PERMISSION
        ))?
        .query_map(params![space, bot], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let value = rows
        .into_iter()
        .map(|(entry_id, chat, revision)| {
            let session_ref = sessions::session_ref_of_entry(c, &chat, &entry_id)?;
            Ok(Pending {
                entry_id,
                session_ref,
                kind: "permission",
                status: "pending",
                conversation: ConversationRef { conversation_id: chat, revision },
            })
        })
        .collect::<Result<_>>()?;
    Ok(Observation::Available {
        source: "entries.permission",
        coverage: "openCatalogSessionsPermissionCardsInBotConversations",
        value,
    })
}
