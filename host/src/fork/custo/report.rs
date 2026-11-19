use super::{Read, ledger::Tokens};
use crate::{LockExt, hub::Hub, store::now_ms};
use anyhow::{Result, anyhow, ensure};
use chrono::{Datelike, TimeZone, Utc};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
};

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Totals {
    turns: u64,
    unpriced_turns: u64,
    input_tokens: Option<u64>,
    cached_read_tokens: Option<u64>,
    cached_write_tokens: Option<u64>,
    output_tokens: Option<u64>,
    costs: Vec<Cost>,
    /// MX-159: estimado por tokens × preço de API (`source` `estimado`),
    /// separado do informado do motor — nunca somados sem distinção.
    estimated_costs: Vec<Cost>,
}
#[derive(Clone, Serialize)]
struct Cost {
    amount: f64,
    currency: String,
    source: &'static str,
}

impl Totals {
    fn add(&mut self, row: &Row) -> Result<()> {
        fn sum(a: &mut Option<u64>, b: Option<i64>) -> Result<()> {
            if let Some(b) = b {
                let b = u64::try_from(b).map_err(|_| anyhow!("invalid_turn_tokens"))?;
                *a = Some(a.unwrap_or(0).checked_add(b).ok_or_else(|| anyhow!("cost_tokens_overflow"))?);
            }
            Ok(())
        }
        self.turns += 1;
        self.unpriced_turns += u64::from(row.amount.is_none());
        sum(&mut self.input_tokens, row.tokens.input)?;
        sum(&mut self.cached_read_tokens, row.tokens.read)?;
        sum(&mut self.cached_write_tokens, row.tokens.write)?;
        sum(&mut self.output_tokens, row.tokens.output)?;
        if let (Some(amount), Some(currency)) = (row.amount, &row.currency) {
            let (bag, source) = if row.source == "estimado" {
                (&mut self.estimated_costs, "estimado")
            } else {
                (&mut self.costs, "motor")
            };
            if let Some(cost) = bag.iter_mut().find(|c| &c.currency == currency) {
                cost.amount += amount;
                ensure!(cost.amount.is_finite(), "cost_overflow");
            } else {
                bag.push(Cost { amount, currency: currency.clone(), source });
            }
            bag.sort_by(|a, b| a.currency.cmp(&b.currency));
        }
        Ok(())
    }
}

struct Row {
    bot: String,
    chat: String,
    session: String,
    parent: Option<String>,
    tokens: Tokens,
    amount: Option<f64>,
    currency: Option<String>,
    account: Option<String>,
    model: Option<String>,
    source: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Conversation {
    bot_id: String,
    chat_id: String,
    session_id: String,
    title: String,
    parent_session_id: Option<String>,
    totals: Totals,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BotSummary {
    bot_id: String,
    name: String,
    totals: Totals,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountSummary {
    account_id: Option<String>,
    totals: Totals,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelSummary {
    model: Option<String>,
    totals: Totals,
}
#[derive(Clone)]
struct Session {
    chat: String,
    title: String,
    parent: Option<String>,
}

pub(crate) fn month(value: Option<&str>) -> Result<(String, i64, i64)> {
    let value = value.map_or_else(|| Utc::now().format("%Y-%m").to_string(), str::to_owned);
    ensure!(value.len() == 7 && value.is_ascii() && value.as_bytes()[4] == b'-', "invalid_cost_month");
    let year: i32 = value[..4].parse().map_err(|_| anyhow!("invalid_cost_month"))?;
    let m: u32 = value[5..].parse().map_err(|_| anyhow!("invalid_cost_month"))?;
    ensure!((1970..=9998).contains(&year), "invalid_cost_month");
    let start = Utc.with_ymd_and_hms(year, m, 1, 0, 0, 0).single().ok_or_else(|| anyhow!("invalid_cost_month"))?;
    let end = if m == 12 {
        Utc.with_ymd_and_hms(year + 1, 1, 1, 0, 0, 0)
    } else {
        Utc.with_ymd_and_hms(year, m + 1, 1, 0, 0, 0)
    }
    .single()
    .ok_or_else(|| anyhow!("invalid_cost_month"))?;
    Ok((format!("{:04}-{:02}", start.year(), start.month()), start.timestamp_millis(), end.timestamp_millis()))
}

pub(super) fn read(hub: &Hub, request: &Read) -> Result<Value> {
    let path = hub.store.db.locked().path().map(PathBuf::from).ok_or_else(|| anyhow!("cost_storage_unavailable"))?;
    let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    // Uma fotografia coerente; conexão própria somente leitura, sem prender a trava do ator.
    let tx = c.unchecked_transaction()?;
    report(&tx, &hub.host_id, request)
}

const COLS: &str = "bot_id,chat_id,session_id,parent_session_id,input_tokens,cached_read_tokens,cached_write_tokens,output_tokens,amount,currency,account_id,model,source";
fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    Ok(Row {
        bot: r.get(0)?,
        chat: r.get(1)?,
        session: r.get(2)?,
        parent: r.get(3)?,
        tokens: Tokens { input: r.get(4)?, read: r.get(5)?, write: r.get(6)?, output: r.get(7)?, invalid: false },
        amount: r.get(8)?,
        currency: r.get(9)?,
        account: r.get(10)?,
        model: r.get(11)?,
        source: r.get(12)?,
    })
}
fn sessions(c: &Connection) -> Result<HashMap<String, Session>> {
    let result = c
        .prepare("SELECT id,chat_id,title,parent_session FROM donna_bot_sessions")?
        .query_map([], |r| Ok((r.get(0)?, Session { chat: r.get(1)?, title: r.get(2)?, parent: r.get(3)? })))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(result)
}
fn conversation(bot: &str, chat: &str, id: &str, sessions: &HashMap<String, Session>) -> Conversation {
    let info = sessions.get(id).filter(|s| s.chat == chat);
    // MX-145: linhas de fundo têm sessão sintética; o título diz o que são.
    let title = info.map_or_else(
        || {
            id.strip_prefix(super::BACKGROUND_SESSION_PREFIX)
                .map(|job| format!("Trabalho de fundo ({job})"))
                .unwrap_or_default()
        },
        |s| s.title.clone(),
    );
    Conversation {
        bot_id: bot.into(),
        chat_id: chat.into(),
        session_id: id.into(),
        title,
        parent_session_id: info.and_then(|s| s.parent.clone()),
        totals: Totals::default(),
    }
}

pub(super) fn report(c: &Connection, hub_id: &str, request: &Read) -> Result<Value> {
    let (month, start, end) = month(request.month.as_deref())?;
    ensure!(
        request.bot_id.is_some() || (request.session_id.is_none() && request.conversation_id.is_none()),
        "invalid_cost_request"
    );
    let rows = c
        .prepare(&format!("SELECT {COLS} FROM donna_turn_usage WHERE at>=?1 AND at<?2"))?
        .query_map(params![start, end], row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let sessions = sessions(c)?;
    let mut total = Totals::default();
    let mut bots: BTreeMap<String, BotSummary> = BTreeMap::new();
    let mut conversations: BTreeMap<(String, String, String), Conversation> = BTreeMap::new();
    let mut accounts: BTreeMap<Option<String>, Totals> = BTreeMap::new();
    let mut models: BTreeMap<Option<String>, Totals> = BTreeMap::new();
    for row in &rows {
        total.add(row)?;
        accounts.entry(row.account.clone()).or_default().add(row)?;
        models.entry(row.model.clone()).or_default().add(row)?;
        if !bots.contains_key(&row.bot) {
            let name: Option<String> = c
                .query_row("SELECT json_extract(config,'$.name') FROM bots WHERE id=?1", [&row.bot], |r| r.get(0))
                .optional()?
                .flatten();
            bots.insert(
                row.bot.clone(),
                BotSummary {
                    bot_id: row.bot.clone(),
                    name: name.unwrap_or_else(|| row.bot.clone()),
                    totals: Totals::default(),
                },
            );
        }
        if let Some(bot) = bots.get_mut(&row.bot) {
            bot.totals.add(row)?;
        }
        let mut id = Some(row.session.clone());
        let mut seen = HashSet::new();
        while let Some(sid) = id {
            ensure!(seen.insert(sid.clone()), "invalid_cost_parent_cycle");
            let entry = conversations
                .entry((row.bot.clone(), row.chat.clone(), sid.clone()))
                .or_insert_with(|| conversation(&row.bot, &row.chat, &sid, &sessions));
            entry.totals.add(row)?;
            id = if sid == row.session {
                row.parent.clone()
            } else {
                sessions.get(&sid).filter(|s| s.chat == row.chat).and_then(|s| s.parent.clone())
            };
        }
    }
    let mut bots: Vec<_> = bots.into_values().collect();
    let mut conversations: Vec<_> = conversations.into_values().collect();
    let mut accounts: Vec<_> =
        accounts.into_iter().map(|(account_id, totals)| AccountSummary { account_id, totals }).collect();
    let mut models: Vec<_> = models.into_iter().map(|(model, totals)| ModelSummary { model, totals }).collect();
    bots.sort_by(|a, b| compare(&a.totals, &b.totals).then(a.bot_id.cmp(&b.bot_id)));
    conversations.sort_by(|a, b| compare(&a.totals, &b.totals).then(a.session_id.cmp(&b.session_id)));
    accounts.sort_by(|a, b| compare(&a.totals, &b.totals).then(a.account_id.cmp(&b.account_id)));
    models.sort_by(|a, b| compare(&a.totals, &b.totals).then(a.model.cmp(&b.model)));
    let helpers: Vec<_> = conversations.iter().filter(|r| r.parent_session_id.is_some()).cloned().collect();
    let selected = request.bot_id.as_deref().map(|bot| -> Result<Conversation> {
        let chat = request.conversation_id.as_deref().unwrap_or(bot);
        let sid = request.session_id.as_deref().unwrap_or(chat);
        // Nunca deduz chat de sessionId. A referência é conferida pela conversa e pelo bot explícitos.
        let owner: Option<String> = c.query_row("SELECT id FROM bots WHERE id=?1 UNION SELECT bot_id FROM donna_space_conversations WHERE id=?2 AND bot_id=?1",
            params![bot,chat],|r|r.get(0)).optional()?;
        ensure!(owner.as_deref()==Some(bot) && (chat==bot || c.query_row("SELECT EXISTS(SELECT 1 FROM donna_space_conversations WHERE id=?1 AND bot_id=?2)",params![chat,bot],|r|r.get::<_,bool>(0))?),"unknown_cost_conversation");
        // spaceId é a seleção do transporte; se informado, a conversa tem que pertencer àquele Space.
        if let Some(space) = &request.space_id {
            let valid = if chat==bot { space==crate::fork::spaces::PERSONAL }
                else { c.query_row("SELECT EXISTS(SELECT 1 FROM donna_space_conversations WHERE id=?1 AND space_id=?2)",params![chat,space],|r|r.get(0))? };
            ensure!(valid,"unknown_cost_conversation");
        }
        // MX-145: a sessão sintética de fundo (`background:<job>`) não está em donna_bot_sessions;
        // vale só na principal do bot e só se o ledger tem linha dela para esse bot.
        let background = sid.starts_with(super::BACKGROUND_SESSION_PREFIX) && chat==bot
            && c.query_row("SELECT EXISTS(SELECT 1 FROM donna_turn_usage WHERE bot_id=?1 AND chat_id=?2 AND session_id=?3)",
                params![bot,chat,sid],|r|r.get::<_,bool>(0))?;
        ensure!(background || sessions.get(sid).is_some_and(|s|s.chat==chat),"unknown_cost_session");
        let mut selected = conversation(bot,chat,sid,&sessions);
        let query = format!("WITH RECURSIVE family(id) AS (SELECT ?1 UNION SELECT s.id FROM donna_bot_sessions s JOIN family f ON s.parent_session=f.id WHERE s.chat_id=?2)
            SELECT {COLS} FROM donna_turn_usage WHERE bot_id=?3 AND chat_id=?2 AND session_id IN (SELECT id FROM family)");
        let rows = c.prepare(&query)?.query_map(params![sid,chat,bot],row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        for row in rows { selected.totals.add(&row)?; }
        Ok(selected)
    }).transpose()?;
    Ok(json!({"hubId":hub_id,"measuredAt":now_ms(),"month":month,"total":total,
        "bots":bots,"conversations":conversations,"helpers":helpers,"accounts":accounts,"models":models,
        "conversation":selected}))
}

// Não converte moedas: entre moedas diferentes o critério é uso de tokens.
fn rank(t: &Totals) -> u64 {
    t.input_tokens
        .unwrap_or(0)
        .saturating_add(t.cached_read_tokens.unwrap_or(0))
        .saturating_add(t.cached_write_tokens.unwrap_or(0))
        .saturating_add(t.output_tokens.unwrap_or(0))
}

/// Informado + estimado somados por moeda só para ORDENAR a listagem (a soma
/// nunca é exibida: os campos continuam separados).
fn merged(t: &Totals) -> Vec<(String, f64)> {
    let mut m = BTreeMap::<&str, f64>::new();
    for c in t.costs.iter().chain(&t.estimated_costs) {
        *m.entry(c.currency.as_str()).or_default() += c.amount;
    }
    m.into_iter().map(|(c, a)| (c.to_owned(), a)).collect()
}

fn compare(a: &Totals, b: &Totals) -> std::cmp::Ordering {
    match (merged(a).as_slice(), merged(b).as_slice()) {
        ([(ca, aa)], [(cb, bb)]) => ca.cmp(cb).then_with(|| bb.total_cmp(aa)),
        ([_], _) => std::cmp::Ordering::Less,
        (_, [_]) => std::cmp::Ordering::Greater,
        _ => rank(b).cmp(&rank(a)),
    }
}
