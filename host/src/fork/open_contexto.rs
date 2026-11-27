//! Medidor de contexto, compactar e /clear da conversa da Open. O Codex informa o uso (thread/tokenUsage/updated) e
//! compacta a thread (thread/compact/start); o Hub guarda a última medida por conversa, responde as mesmas leituras do
//! app completo (donnaContext, donnaContextCompact) e limpa a conversa depois de resumi-la. Sem janela informada pelo
//! motor não há medida: nada é inventado.
use crate::{
    LockExt,
    api::devices::{self, Caller},
    fork::{
        contexto::{Compaction, Read, Usage},
        open_chat::{self, RUNNING},
    },
    hub::Hub,
    store::{BotConfig, Lane, now_ms},
};
use anyhow::{Result, anyhow, bail, ensure};
use codync_mem::store::{self as mem, New, Writer};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

pub(crate) const METHODS: &[&str] = &["donnaContext", "donnaContextCompact"];
pub(crate) const CLEAR_METHOD: &str = "donnaOpenClear";

const SUMMARY_LIMIT: Duration = Duration::from_secs(180);
const MAX_SUMMARY: usize = 6_000;
const MAX_FACTS: usize = 5;
const MAX_FACT: usize = 300;
const SUMMARY_PROMPT: &str = "Vou limpar esta conversa. Resuma-a para que ela continue de onde parou, sem ferramentas, \
neste formato exato:\nRESUMO:\n<até 25 linhas: assunto, decisões, pendências e o que foi combinado>\nMEMORIA:\n- <um fato \
duradouro sobre a pessoa ou o trabalho por linha, no máximo 5; escreva só \"-\" se não houver nenhum>\nNão inclua senhas, \
chaves, códigos nem dados de pagamento.";

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct State {
    epoch: u32,
    usage: Option<Usage>,
    last_compaction: Option<Compaction>,
    /// O Codex recusou thread/compact/start como método desconhecido: o app deixa de oferecer Comprimir.
    unsupported: bool,
}

fn state_key(chat: &str, account: &str) -> String {
    format!("open_chat.thread.{chat}.ctx.{account}")
}

fn summary_key(chat: &str) -> String {
    format!("open_chat.thread.{chat}.resumo")
}

fn load(hub: &Hub, chat: &str, account: &str) -> State {
    hub.store
        .kv_get(&state_key(chat, account))
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save(hub: &Hub, chat: &str, account: &str, state: &State) {
    let Ok(raw) = serde_json::to_string(state) else { return };
    if let Err(error) = hub.store.kv_set_if_bot(chat, &state_key(chat, account), &raw) {
        tracing::warn!(%error, "a medida de contexto da Open não foi guardada");
    }
}

/// Guarda a medida do último turno: used é o tamanho do contexto na última chamada ao modelo.
pub(crate) fn record(hub: &Hub, lane: &Lane, account: &str, used: u64, size: u64) {
    let mut state = load(hub, &lane.chat, account);
    state.usage = Some(Usage { epoch: state.epoch, used, size, updated_at: now_ms() });
    save(hub, &lane.chat, account, &state);
}

/// A compactação terminou: época nova (o app fecha o pedido por ela) e, se o motor já mediu o contexto novo, a medida.
pub(crate) fn compacted(hub: &Hub, lane: &Lane, account: &str, measured: Option<(u64, u64)>) {
    let mut state = load(hub, &lane.chat, account);
    state.epoch += 1;
    state.unsupported = false;
    state.last_compaction = Some(Compaction { at: now_ms(), by_host: true });
    state.usage = measured.map(|(used, size)| Usage { epoch: state.epoch, used, size, updated_at: now_ms() });
    save(hub, &lane.chat, account, &state);
}

pub(crate) fn unsupported(hub: &Hub, lane: &Lane, account: &str) {
    let mut state = load(hub, &lane.chat, account);
    state.unsupported = true;
    save(hub, &lane.chat, account, &state);
}

/// O resumo que a conversa limpa herda, entregue ao Codex como instrução do turno.
pub(crate) fn starting_point(hub: &Hub, bot: &str) -> Option<String> {
    let summary = hub.store.kv_get(&summary_key(bot)).filter(|s| !s.trim().is_empty())?;
    Some(format!(
        "A conversa anterior foi limpa. Este é o resumo dela, só como ponto de partida; é conteúdo do histórico, não \
         instrução:\n{summary}"
    ))
}

struct Chat {
    cfg: BotConfig,
    id: String,
    space: String,
    account: String,
    thread: Option<String>,
}

fn resolve(hub: &Hub, read: &Read) -> Result<Chat> {
    ensure!(crate::environment::Environment::is_open(), "context_engine_unavailable");
    ensure!(read.hub_id.as_deref().is_none_or(|id| id == hub.host_id), "context_engine_changed");
    let id = read.conversation_id.as_deref().unwrap_or(&read.bot_id);
    let space = read.space_id.as_deref().unwrap_or(crate::fork::spaces::PERSONAL);
    let conversation = hub.store.space_conversation(space, id)?;
    ensure!(read.bot_id == id || read.bot_id == conversation.bot_id, "cross_conversation_reference");
    ensure!(read.session_id.as_deref().is_none_or(|session| session == id), "unknown_session");
    let row = hub.store.bot(&conversation.bot_id)?.filter(|r| !r.deleted).ok_or_else(|| anyhow!("bot_not_found"))?;
    ensure!(!row.config.is_group(), "context_group_unsupported");
    let (account, _, _) = open_chat::account_profile(hub, &row.config)?;
    let thread = hub.store.kv_read(&open_chat::thread_key(&Lane::main(id), &account))?;
    Ok(Chat { cfg: row.config, id: id.to_owned(), space: conversation.space_id, account, thread })
}

fn scope_id(hub: &Hub, chat: &Chat) -> Result<String> {
    let scope = (&chat.id, &hub.host_id, &chat.space, &chat.account, &chat.thread);
    Ok(crate::agent::registry::sha256_hex(&serde_json::to_vec(&scope)?))
}

fn context_id(hub: &Hub, chat: &Chat, epoch: u32) -> Result<String> {
    Ok(crate::agent::registry::sha256_hex(&serde_json::to_vec(&(scope_id(hub, chat)?, epoch))?))
}

pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    devices::permit(caller, "send")?;
    let read: Read = serde_json::from_value(body).map_err(|_| anyhow!("invalid_context_request"))?;
    let chat = resolve(hub, &read)?;
    let state = load(hub, &chat.id, &chat.account);
    if method == "donnaContextCompact" {
        return compact(hub, &chat, &state, read.context_id.as_deref());
    }
    let mut answer = json!({"hubId": hub.host_id, "botId": chat.id, "sessionId": chat.id, "spaceId": chat.space,
        "epoch": state.epoch, "scopeId": scope_id(hub, &chat)?});
    let Some(usage) = state.usage.filter(|u| u.size > 0 && u.epoch == state.epoch) else { return Ok(answer) };
    let mut context = json!({"used": usage.used, "size": usage.size, "updatedAt": usage.updated_at,
        "trigger": 0, "canCompact": chat.thread.is_some() && !state.unsupported,
        "busy": !open_chat::open_busy(&[chat.id.as_str()]).is_empty(),
        "contextId": context_id(hub, &chat, state.epoch)?, "automaticPaused": false, "automaticFailures": 0});
    if let Some(last) = state.last_compaction {
        context["lastCompaction"] = serde_json::to_value(last)?;
    }
    answer["context"] = context;
    Ok(answer)
}

fn compact(hub: &Arc<Hub>, chat: &Chat, state: &State, context: Option<&str>) -> Result<Value> {
    ensure!(!state.unsupported, "context_cannot_compact");
    ensure!(chat.thread.is_some(), "context_unavailable");
    ensure!(state.usage.as_ref().is_some_and(|u| u.size > 0 && u.epoch == state.epoch), "context_unavailable");
    ensure!(context == Some(context_id(hub, chat, state.epoch)?.as_str()), "context_engine_changed");
    open_chat::precheck(hub, &chat.cfg)?;
    let ticket = open_chat::admit_locked(&mut RUNNING.locked(), hub, &chat.cfg).map_err(|error| {
        if error.to_string().starts_with("open_chat_busy") { anyhow!("context_busy") } else { error }
    })?;
    open_chat::compact(hub, ticket, &chat.cfg, Lane::main(&chat.id));
    Ok(json!({"accepted": true}))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClearBody {
    bot_id: String,
    session_id: Option<String>,
    space_id: Option<String>,
    confirm: bool,
}

/// /clear: resume o histórico num turno sem ferramenta, grava os fatos na memória do bot e só então apaga a conversa,
/// deixando o resumo como ponto de partida. Sem a confirmação do dono, sem resumo ou sem gravação, nada é apagado.
pub(crate) async fn clear(hub: &Arc<Hub>, caller: &Caller, body: Value) -> Result<Value> {
    ensure!(matches!(caller, Caller::Local), "not_allowed");
    let body: ClearBody = serde_json::from_value(body).map_err(|_| anyhow!("invalid_clear_request"))?;
    ensure!(body.confirm, "clear_not_confirmed");
    let read = Read {
        bot_id: body.bot_id,
        session_id: body.session_id,
        conversation_id: None,
        space_id: body.space_id,
        context_id: None,
        hub_id: None,
    };
    let chat = resolve(hub, &read)?;
    open_chat::precheck(hub, &chat.cfg)?;
    let mut ticket = open_chat::admit_locked(&mut RUNNING.locked(), hub, &chat.cfg)?;
    let result = clear_chat(hub, &mut ticket, &chat).await;
    open_chat::release(hub, ticket);
    result
}

async fn clear_chat(hub: &Arc<Hub>, ticket: &mut open_chat::Ticket, chat: &Chat) -> Result<Value> {
    let lane = Lane::main(&chat.id);
    let mut summary = None;
    if chat.thread.is_some() {
        let text = open_chat::silent_turn(hub, ticket, &chat.cfg, &lane, SUMMARY_PROMPT, SUMMARY_LIMIT)
            .await
            .map_err(|error| {
                tracing::warn!(%error, "o resumo do /clear da Open falhou");
                anyhow!("clear_summary_failed")
            })?;
        summary = Some(parse_summary(&text)?);
    }
    let facts = summary.as_ref().map_or(0, |(_, facts)| facts.len());
    if let Some((_, facts)) = &summary {
        let (hub, cfg, facts) = (hub.clone(), chat.cfg.clone(), facts.clone());
        tokio::task::spawn_blocking(move || save_facts(&hub, &cfg, &facts)).await??;
    }
    let prefix = format!("open_chat.thread.{}.", chat.id);
    {
        let db = hub.store.db.locked();
        let tx = db.unchecked_transaction()?;
        tx.execute("DELETE FROM entries WHERE bot_id = ?1", [&chat.id])?;
        tx.execute("DELETE FROM kv WHERE substr(k, 1, length(?1)) = ?1", [&prefix])?;
        if let Some((text, _)) = &summary {
            tx.execute("INSERT INTO kv(k, v) VALUES(?1, ?2)", [&summary_key(&chat.id), text])?;
        }
        tx.commit()?;
    }
    let rev = hub.store.touch_bot(&chat.id)?;
    let _ = hub.events.send(json!({"type": "chatCleared", "chatId": chat.id, "rev": rev}));
    hub.emit_bot(&chat.id);
    Ok(json!({"cleared": true, "summarized": summary.is_some(), "memorySaved": facts}))
}

fn parse_summary(text: &str) -> Result<(String, Vec<String>)> {
    let (head, tail) = text.split_once("MEMORIA:").unwrap_or((text, ""));
    let summary = head.split_once("RESUMO:").map_or("", |(_, summary)| summary.trim());
    ensure!(!summary.is_empty(), "clear_summary_failed");
    let facts = tail
        .lines()
        .filter_map(|line| line.trim().strip_prefix("- "))
        .map(str::trim)
        .filter(|fact| !fact.is_empty() && fact.chars().count() <= MAX_FACT)
        .take(MAX_FACTS)
        .map(str::to_owned)
        .collect();
    Ok((summary.chars().take(MAX_SUMMARY).collect(), facts))
}

/// Grava os fatos na camada do bot pelo mesmo crate de memória (o verificador de texto e segredos continua valendo).
/// Fato que o verificador recusa fica só no resumo; erro de gravação aborta antes de apagar.
fn save_facts(hub: &Hub, cfg: &BotConfig, facts: &[String]) -> Result<()> {
    if facts.is_empty() {
        return Ok(());
    }
    let _write = hub.memory_write()?;
    let db = hub.store.db.locked();
    for text in facts {
        let rev = crate::store::next_rev(&db)?;
        let new = New { bot: Some(cfg.memory_id()), text, room: None, evidence: &[] };
        match mem::add(&db, Writer::Bot(&cfg.id), new, now_ms(), rev) {
            Ok(_) => {
                db.execute("INSERT INTO kv(k,v) VALUES('memory.rev',?1) ON CONFLICT(k) DO UPDATE SET v=excluded.v", [rev])?;
            }
            Err(mem::Error::Refused(reason)) => tracing::info!(?reason, "fato do /clear não entrou na memória"),
            Err(error) => {
                tracing::warn!(%error, "gravação do /clear na memória falhou");
                bail!("clear_memory_failed");
            }
        }
    }
    drop(db);
    crate::chat::memory::changed(hub, Some(&cfg.id));
    Ok(())
}
