use super::{PERSONAL, binding, label};
use crate::{
    LockExt,
    api::devices::Caller,
    hub::{BotStatus, Hub},
    store::{Lane, Store},
};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EngineSelection {
    pub backend: String,
    pub model: Option<String>,
    pub model_options: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: String,
    pub space_id: String,
    pub bot_id: String,
    pub title: String,
    pub revision: i64,
    pub primary: bool,
    pub closed: bool,
    pub state_revision: i64,
    pub read_revision: i64,
    pub engine_account_id: Option<String>,
    pub engine_selection: Option<EngineSelection>,
    pub continuity: Option<Value>,
    pub account_revision: Option<i64>,
    #[serde(skip)]
    pub account_stamp: Option<String>,
}
const COLS: &str = "id,space_id,bot_id,title,revision,is_primary,closed,read_revision,engine_account_id,engine_selection,account_revision,account_stamp,continuity,MAX(revision,read_revision,state_revision)";
fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Conversation> {
    let selection: Option<String> = r.get(9)?;
    let selection = selection
        .map(|s| {
            serde_json::from_str(&s)
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(e)))
        })
        .transpose()?;
    let continuity: Option<String> = r.get(12)?;
    let continuity = continuity
        .map(|s| {
            serde_json::from_str(&s)
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(12, rusqlite::types::Type::Text, Box::new(e)))
        })
        .transpose()?;
    Ok(Conversation {
        id: r.get(0)?,
        space_id: r.get(1)?,
        bot_id: r.get(2)?,
        title: r.get(3)?,
        revision: r.get(4)?,
        primary: r.get(5)?,
        closed: r.get(6)?,
        state_revision: r.get(13)?,
        read_revision: r.get(7)?,
        engine_account_id: r.get(8)?,
        engine_selection: selection,
        continuity,
        account_revision: r.get(10)?,
        account_stamp: r.get(11)?,
    })
}
pub(super) fn read_on(c: &rusqlite::Connection, space: &str, id: &str) -> Result<Conversation> {
    super::store::require_space(c, space)?;
    let value = c
        .query_row(
            &format!("SELECT {COLS} FROM donna_space_conversations WHERE id=?1 AND space_id=?2"),
            params![id, space],
            row,
        )
        .optional()?
        .ok_or_else(|| anyhow!("conversation_not_found"))?;
    ensure!(
        c.query_row("SELECT EXISTS(SELECT 1 FROM bots WHERE id=?1 AND deleted=0)", [&value.bot_id], |r| r
            .get::<_, bool>(0))?,
        "conversation_bot_unavailable"
    );
    Ok(value)
}
impl Store {
    pub fn space_conversation(&self, space: &str, id: &str) -> Result<Conversation> {
        self.require_space(space)?;
        let c = self.db.locked();
        read_on(&c, space, id)
    }

    /// Mesma leitura na `Connection` recebida (validadores Connection-only do VG03).
    pub(crate) fn space_conversation_on(c: &rusqlite::Connection, space: &str, id: &str) -> Result<Conversation> {
        read_on(c, space, id)
    }
    pub fn space_conversations(&self, space: &str) -> Result<Vec<Conversation>> {
        self.require_space(space)?;
        let c = self.db.locked();
        Ok(c.prepare(&format!("SELECT {COLS} FROM donna_space_conversations WHERE space_id=?1 AND EXISTS(SELECT 1 FROM bots b WHERE b.id=bot_id AND b.deleted=0) ORDER BY is_primary DESC,revision,id"))?.query_map([space],row)?.collect::<rusqlite::Result<_>>()?)
    }
}
pub(super) fn arg<'a>(b: &'a Value, k: &str) -> Result<&'a str> {
    b[k].as_str().ok_or_else(|| anyhow!("{k}_required"))
}
pub(super) fn get(hub: &Hub, b: &Value) -> Result<Conversation> {
    hub.store.space_conversation(b["spaceId"].as_str().unwrap_or(PERSONAL), arg(b, "conversationId")?)
}
pub(crate) fn context(c: &Conversation) -> Value {
    json!({"version":1,"spaceId":c.space_id,"conversationId":c.id,"botId":c.bot_id,"conversationRevision":c.revision})
}
pub(super) fn entry_value(c: &Conversation, e: &crate::store::Entry) -> Result<Value> {
    let mut v = serde_json::to_value(e)?;
    v["spaceId"] = c.space_id.clone().into();
    v["conversationId"] = c.id.clone().into();
    Ok(v)
}
pub(crate) fn emit(hub: &Hub, c: &Conversation) {
    crate::fork::runtime_audiencia::screen::changed(hub, &c.bot_id);
    let _=hub.events.send(json!({"type":"conversation","spaceId":c.space_id,"rev":c.state_revision.max(c.revision).max(c.read_revision),"conversation":c}));
}
fn expected(b: &Value) -> Result<i64> {
    b["expectedRevision"].as_i64().ok_or_else(|| anyhow!("expected_revision_required"))
}
pub(super) fn require_revision(c: &Conversation, b: &Value) -> Result<()> {
    if c.revision != expected(b)? {
        return Err(crate::remote::cloud::Conflict("conversation_revision_conflict").into());
    }
    Ok(())
}
fn source(hub: &Hub, c: &Conversation) -> Result<crate::store::BotConfig> {
    hub.store.bot(&c.bot_id)?.filter(|r| !r.deleted).map(|r| r.config).ok_or_else(|| anyhow!("bot_not_found"))
}

pub(super) async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, b: Value) -> Result<Option<Value>> {
    if method == "donnaConversations" {
        return Ok(Some(list(hub, &b)?));
    }
    if !method.starts_with("donnaConversation") {
        return Ok(None);
    }
    if method == "donnaConversationStartupOpen" {
        return Ok(Some(super::startup::open(hub, caller, b)?));
    }
    if method == "donnaConversationStartupLookup" {
        return Ok(Some(super::startup::lookup(hub, caller, b)?));
    }
    if method == "donnaConversationCreate" {
        super::super::engine_setup::require_local(caller)?;
        let _guard = hub.store.team_lock.lock().await;
        return Ok(Some(create(hub, &b)?));
    }
    if method == "donnaConversationHistory" {
        return Ok(Some(history(hub, &get(hub, &b)?, &b, caller.is_web_chat())?));
    }
    let _guard = hub.store.travar_equipe().await;
    let conversation = get(hub, &b)?;
    if let Some(value) = super::resources::call(hub, caller, method, &conversation, &b).await? {
        return Ok(Some(value));
    }
    let value = match method {
        "donnaConversationRetry" => {
            require_revision(&conversation, &b)?;
            ensure!(conversation.id != conversation.bot_id, "use_legacy_primary_retry");
            let (reply, result) = tokio::sync::oneshot::channel();
            hub.send_cmd(
                &conversation.bot_id,
                crate::agent::bot::Cmd::ContextRetry {
                    chat: conversation.id,
                    revision: conversation.revision,
                    entry_id: arg(&b, "entryId")?.to_owned(),
                    reply,
                },
            )?;
            tokio::time::timeout(std::time::Duration::from_secs(10), result)
                .await
                .map_err(|_| anyhow!("context_control_pending"))???
        }
        "donnaConversationStop" | "donnaConversationNewSession" => {
            require_revision(&conversation, &b)?;
            let entry_id = if method == "donnaConversationStop" { Some(arg(&b, "entryId")?.to_owned()) } else { None };
            control(hub, &conversation, entry_id, method == "donnaConversationNewSession").await?
        }
        "donnaConversationUpdate" => {
            super::super::engine_setup::require_local(caller)?;
            update(hub, conversation, &b)?
        }
        "donnaConversationRead" => mark_read(hub, conversation)?,
        "donnaConversationSend" => send(hub, caller, &conversation, &b)?,
        "donnaConversationChoiceAnswer" => choice_answer(hub, caller, &conversation, &b)?,
        _ => return Err(anyhow!("unknown_conversation_method")),
    };
    Ok(Some(value))
}
fn list(hub: &Hub, b: &Value) -> Result<Value> {
    let space = b["spaceId"].as_str().unwrap_or(PERSONAL);
    let mut conversations = hub.store.space_conversations(space)?;
    if let Some(bot) = b["botId"].as_str() {
        let source = hub.store.bot(bot)?.ok_or_else(|| anyhow!("bot_not_found"))?;
        ensure!(
            source.config.space_id == space || hub.store.team_settings()?.primary_bot_id.as_deref() == Some(bot),
            "cross_space_reference"
        );
        conversations.retain(|c| c.bot_id == bot);
    }
    // Só Pessoal apresenta a principal global. Space legado sem vínculo não tem principal.
    // A execução contextual legada continua disponível pelas conversas existentes.
    let principal_id = if space == PERSONAL {
        hub.store.team_settings()?.primary_bot_id
    } else {
        hub.store.spaces()?.spaces.into_iter().find(|s| s.id == space).and_then(|s| s.principal_bot_id)
    };
    let principal = principal_id
        .map(|id| hub.store.bot(&id))
        .transpose()?
        .flatten()
        .filter(|r| !r.deleted && !r.config.is_group() && r.config.space_id == space)
        .map(|r| json!({"id":r.config.id,"name":r.config.name,"avatarColor":r.config.avatar_color,
            "avatarShape":r.config.avatar_shape,"avatarConfig":r.config.avatar_config,"mexeriquinha":r.config.mexeriquinha}));
    Ok(json!({"spaceId":space,"conversations":conversations,"principal":principal}))
}
fn create(hub: &Hub, b: &Value) -> Result<Value> {
    let space = arg(b, "spaceId")?;
    hub.store.require_space(space)?;
    let bot = arg(b, "botId")?;
    let cfg = hub
        .store
        .bot(bot)?
        .filter(|r| !r.deleted && !r.config.is_group())
        .ok_or_else(|| anyhow!("bot_not_found"))?
        .config;
    ensure!(
        cfg.space_id == space || hub.store.team_settings()?.primary_bot_id.as_deref() == Some(bot),
        "cross_space_reference"
    );
    let title = if b.get("title").is_some() { label(&b["title"])? } else { "Nova conversa".into() };
    let mut db = hub.store.db.locked();
    let tx = db.transaction()?;
    let count: i64 =
        tx.query_row("SELECT COUNT(*) FROM donna_space_conversations WHERE space_id=?1", [space], |r| r.get(0))?;
    ensure!(count < 1024, "conversation_limit");
    let primary: bool = tx.query_row(
        "SELECT NOT EXISTS(SELECT 1 FROM donna_space_conversations WHERE space_id=?1 AND bot_id=?2)",
        params![space, bot],
        |r| r.get(0),
    )?;
    let id = uuid::Uuid::new_v4().to_string();
    let rev = crate::store::next_rev(&tx)?;
    tx.execute(
        "INSERT INTO donna_space_conversations(id,space_id,bot_id,title,revision,is_primary) VALUES(?1,?2,?3,?4,?5,?6)",
        params![id, space, bot, title, rev, primary],
    )?;
    crate::fork::sessoes::ensure_principal(&tx, &id)?;
    tx.commit()?;
    drop(db);
    let c = hub.store.space_conversation(space, &id)?;
    super::notice::ensure(hub, &cfg, &c.id, &c.space_id)?;
    emit(hub, &c);
    Ok(json!({"conversation":c}))
}
fn update(hub: &Hub, mut c: Conversation, b: &Value) -> Result<Value> {
    require_revision(&c, b)?;
    ensure!(
        !crate::agent::bot::read_waiting_checkpoint(&hub.store, &c.bot_id)?
            .as_ref()
            .is_some_and(crate::agent::bot::checkpoint_waiting_active),
        "conversation_busy"
    );
    ensure!(
        !matches!(hub.runtime(&c.bot_id).status, BotStatus::Working | BotStatus::NeedsInput | BotStatus::Waiting),
        "conversation_busy"
    );
    ensure!(
        b.as_object().is_some_and(|o| o.keys().all(|k| matches!(
            k.as_str(),
            "spaceId"
                | "conversationId"
                | "expectedRevision"
                | "title"
                | "closed"
                | "engineAccountId"
                | "engineSelection"
                | "expectedAccountRevision"
        ))),
        "invalid_conversation_field"
    );
    if let Some(value) = b.get("title") {
        c.title = label(value)?;
    }
    if let Some(value) = b.get("closed") {
        let closed = value.as_bool().ok_or_else(|| anyhow!("invalid_closed"))?;
        ensure!(!c.primary || !closed, "primary_conversation_immutable");
        c.closed = closed;
    }
    if b.get("engineAccountId").is_some() || b.get("engineSelection").is_some() {
        ensure!(c.id != c.bot_id, "use_bot_editor_for_primary_binding");
        ensure!(
            !matches!(hub.runtime(&c.bot_id).status, BotStatus::Working | BotStatus::NeedsInput | BotStatus::Waiting),
            "conversation_busy"
        );
        binding::select(&hub.store, &source(hub, &c)?, &mut c, b)?;
    }
    let mut db = hub.store.db.locked();
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let current: i64 = tx.query_row(
        "SELECT revision FROM donna_space_conversations WHERE id=?1 AND space_id=?2",
        params![c.id, c.space_id],
        |r| r.get(0),
    )?;
    ensure!(current == expected(b)?, "conversation_revision_conflict");
    c.revision = crate::store::next_rev(&tx)?;
    let selection = c.engine_selection.as_ref().map(serde_json::to_string).transpose()?;
    tx.execute("UPDATE donna_space_conversations SET title=?2,closed=?3,engine_account_id=?4,revision=?5,engine_selection=?6,account_revision=?7,account_stamp=?8 WHERE id=?1",params![c.id,c.title,c.closed,c.engine_account_id,c.revision,selection,c.account_revision,c.account_stamp])?;
    // G1b: fechar e cancelar os cartões de escolha commitam juntos.
    let cancelled = if c.closed { crate::fork::genui::flow::cancel_on(&tx, &c.id, None)? } else { Vec::new() };
    tx.commit()?;
    drop(db);
    crate::fork::genui::flow::publish_committed(hub, cancelled);
    if c.closed {
        crate::fork::owner_question::withdraw_chat(hub, &c.id, None);
    }
    c.state_revision = c.revision;
    emit(hub, &c);
    Ok(json!({"conversation":c}))
}
fn history(hub: &Hub, c: &Conversation, b: &Value, web: bool) -> Result<Value> {
    let before = b["beforeSeq"].as_i64().unwrap_or(i64::MAX);
    if before != i64::MAX {
        let owner: Option<String> = hub
            .store
            .db
            .locked()
            .query_row("SELECT bot_id FROM entries WHERE seq=?1", [before], |r| r.get(0))
            .optional()?;
        ensure!(owner.as_deref() == Some(&c.id), "invalid_conversation_cursor");
    }
    let limit = b["limit"].as_i64().unwrap_or(100).clamp(1, 500);
    let mut entries = if let Some(query) = b["query"].as_str() {
        ensure!(query.len() <= 4096, "query_too_long");
        let terms = query.split_whitespace().take(8).map(str::to_lowercase).collect::<Vec<_>>();
        let from = Some(before.saturating_sub(1));
        let mut values = if web {
            hub.store.search_web_messages(&c.id, &terms, from, limit + 1)?
        } else {
            hub.store.search_messages(&c.id, &terms, from, limit + 1)?
        };
        values.reverse();
        values
    } else if web {
        hub.store.web_history(&c.id, None, before, limit + 1)?
    } else {
        hub.store.history(&c.id, before, limit + 1)?
    };
    let more = entries.len() > usize::try_from(limit)?;
    if more {
        entries.remove(0);
    }
    let entries = entries.iter().map(|e| entry_value(c, e)).collect::<Result<Vec<_>>>()?;
    Ok(json!({"context":context(c),"entries":entries,"hasMore":more}))
}
fn mark_read(hub: &Hub, mut c: Conversation) -> Result<Value> {
    let mut db = hub.store.db.locked();
    let tx = db.transaction()?;
    let rev = crate::store::next_rev(&tx)?;
    tx.execute("UPDATE donna_space_conversations SET read_revision=?2 WHERE id=?1", params![c.id, rev])?;
    if c.primary && c.id == c.bot_id {
        tx.execute("UPDATE bots SET read_rev=?2,rev=?2 WHERE id=?1", params![c.id, rev])?;
    }
    tx.commit()?;
    drop(db);
    c.read_revision = rev;
    c.state_revision = rev;
    emit(hub, &c);
    Ok(json!({"conversation":c}))
}
fn send(hub: &Arc<Hub>, caller: &Caller, c: &Conversation, b: &Value) -> Result<Value> {
    let text = arg(b, "text")?.trim();
    let nonce = arg(b, "clientNonce")?;
    let revision = expected(b)?;
    ensure!(!nonce.is_empty() && nonce.len() <= 128, "invalid_nonce");
    ensure!(!text.is_empty() && text.len() <= 1024 * 1024, "invalid_message");
    let attachments = b["attachments"].as_array().cloned().unwrap_or_default();
    let intent =
        json!({"hubId":hub.host_id,"revision":revision,"text":text,"attachments":attachments,"replyTo":b["replyTo"]});
    if let Some(entry) = hub.store.find_by_nonce(&c.id, nonce) {
        ensure!(entry.data["intent"] == intent, "nonce_intent_conflict");
        return Ok(json!({"context":entry.data["spaceContext"],"entry":entry_value(c,&entry)?,"replayed":true}));
    }
    require_revision(c, b)?;
    ensure!(c.id != c.bot_id, "use_legacy_primary_send");
    ensure!(!c.closed, "conversation_closed");
    let original = source(hub, c)?;
    ensure!(!original.is_group(), "use_channel_send");
    let cfg = super::execution_config(&hub.store, &original, &c.id)?;
    let stamp = if cfg.context.is_some() { Some(binding::execution_stamp(&hub.store, &cfg)?) } else { None };
    let mut data = json!({"text":text,"clientNonce":nonce,"status":"queued","spaceContext":context(c),"intent":intent,"executionBinding":stamp});
    if let Some(reply) = b["replyTo"].as_str() {
        data["replyTo"] = crate::chat::replies::quote(&hub.store, &c.id, reply)?;
    }
    let mut resolved = Vec::new();
    for upload in &attachments {
        let id = upload.as_str().ok_or_else(|| anyhow!("invalid_upload_id"))?;
        resolved.push(super::uploads::resolve(&hub.store, c, &cfg, id)?.1);
    }
    data["attachments"] = json!(resolved);
    data["contentOrigin"] = crate::guard_hub::origem::capture(caller, b, &data);
    crate::fork::idioma_app::carimbar(&hub.store, caller, &mut data); // D395
    // MB-1b: responde as perguntas pendentes da conversa na mesma transação da mensagem.
    let entry = match crate::fork::owner_question::add_human_message_no_turno(hub, &Lane::main(&c.id), &data)? {
        crate::fork::owner_question::Human::New(entry) => entry,
        crate::fork::owner_question::Human::Replayed(entry) => {
            return Ok(json!({"context":entry.data["spaceContext"],"entry":entry_value(c,&entry)?,"replayed":true}));
        }
    };
    let prompt = crate::chat::prompt::of_entry(hub, &cfg, &entry.data);
    hub.send_cmd(
        &c.bot_id,
        crate::agent::bot::Cmd::Send { lane: Lane::main(&c.id), entry_id: entry.id.clone(), text: prompt },
    )?;
    Ok(json!({"context":context(c),"entry":entry_value(c,&entry)?,"replayed":false,"queued":true}))
}

/// G1b: resposta do dono ao cartão de escolha. Mesma audiência do envio (guard comum, permissão
/// Control e revisão da conversa); a autoridade da pergunta é conferida pelo Hub no CAS.
fn choice_answer(hub: &Hub, caller: &Caller, c: &Conversation, b: &Value) -> Result<Value> {
    require_revision(c, b)?;
    let target = crate::fork::genui::flow::Target {
        chat: &c.id,
        space: &c.space_id,
        bot: &c.bot_id,
        primary: c.id == c.bot_id,
        closed: c.closed,
        app_language: crate::fork::idioma_app::do_chamador(&hub.store, caller),
    };
    let answered = crate::fork::genui::flow::answer(hub, &target, b)?;
    Ok(json!({
        "context": context(c),
        "question": answered.question.data["genuiChoice"],
        "entry": entry_value(c, &answered.response)?,
        "replayed": answered.replayed,
    }))
}

pub(crate) async fn control(hub: &Arc<Hub>, c: &Conversation, entry_id: Option<String>, reset: bool) -> Result<Value> {
    let (reply, result) = tokio::sync::oneshot::channel();
    hub.send_cmd(
        &c.bot_id,
        crate::agent::bot::Cmd::ContextControl { chat: c.id.clone(), revision: c.revision, entry_id, reset, reply },
    )?;
    tokio::time::timeout(std::time::Duration::from_secs(10), result)
        .await
        .map_err(|_| anyhow!("context_control_pending"))??
}
pub async fn legacy_control(hub: &Arc<Hub>, space: &str, chat: &str, reset: bool) -> Result<Value> {
    let _guard = hub.store.team_lock.lock().await;
    let c = hub.store.space_conversation(space, chat)?;
    // Sessões PR5: recomeçar a principal só por `legacy_restart`, com a autoridade original.
    anyhow::ensure!(!(reset && c.id == c.bot_id), "not_allowed: autoridade original ausente");
    control(hub, &c, None, reset).await
}

/// `newSession` legado. Na principal do Pessoal, recomeça só a principal no ator, com a autoridade ORIGINAL capturada
/// na entrada da RPC e levada explícita no comando (Sessões PR5); numa conversa de Space, o caminho de sempre.
pub(crate) async fn legacy_restart(
    hub: &Arc<Hub>,
    space: &str,
    chat: &str,
    autoridade: crate::fork::autoridade::Autoridade,
) -> Result<Value> {
    let _guard = hub.store.team_lock.lock().await;
    let c = hub.store.space_conversation(space, chat)?;
    if c.id != c.bot_id {
        return control(hub, &c, None, true).await;
    }
    let (reply, result) = tokio::sync::oneshot::channel();
    let autoridade = Box::new(autoridade);
    let command = crate::agent::bot::Cmd::PrimaryNewSession { chat: c.id, revision: c.revision, autoridade, reply };
    hub.send_cmd(&c.bot_id, command)?;
    tokio::time::timeout(std::time::Duration::from_secs(10), result)
        .await
        .map_err(|_| anyhow!("context_control_pending"))??
}
