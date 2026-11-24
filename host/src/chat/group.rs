// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Group chats (Grok Bot's design): several bots and the user in one conversation.
//!
//! A group is a roster row (`BotKind::Group`) with a member list and its own
//! transcript, but no agent. When the user writes, the group runs a *room turn*:
//! up to [`MAX_ROUNDS`] rounds in which the members answer one at a time, each in
//! its **own** session (a member's history is unified across its chats), told
//! what was said since it last spoke. Everyone answers unless the new messages
//! @-mention someone; a member with nothing to add says `(pass)`, and a round
//! where nobody speaks ends the turn. A newer message in the same lane (or Stop)
//! moves the lane's epoch, which ends a running room turn before its next speaker.

use crate::LockExt;
use crate::agent::bot::Cmd;
use crate::fork::autoridade::Autoridade;
use crate::fork::grupo_entrega::Delivery;
use crate::fork::push_textos::{Corpo, Titulo};
use crate::hub::Hub;
use crate::remote::push::{self, AlertKind};
use crate::store::{BotConfig, BotKind, Entry, EntryKind, Lane, Store};
use anyhow::{Result, anyhow, bail};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

const MAX_ROUNDS: usize = 3;
/// Replies per room turn, across all rounds.
const MAX_REPLIES: usize = 10;
/// Room lines a member is shown per turn.
const HISTORY_LINES: i64 = 24;
/// A bot's line in another member's prompt is clipped (the user's never is).
const LINE_CHARS: usize = 2000;
const PASS: &str = "(pass)";

/// One member's turn in a room, queued on its actor like any other turn.
pub struct GroupTurn {
    pub lane: Lane,
    pub prompt: String,
    /// The messages `prompt` shows (ids, oldest first): a reaction goes only on one the member saw.
    pub shown: Vec<String>,
    /// The room turn that asked; a member skips it once a newer one started (see [`Rooms::current`]).
    pub epoch: u64,
    pub(crate) authority: Option<Autoridade>,
    /// The reply the room sees, or `None` for a pass.
    pub reply: oneshot::Sender<Result<Option<String>>>,
}

/// Room turn epochs per lane and groups blocked until restart, behind the same fence as delivery.
#[derive(Default)]
pub struct Rooms(Mutex<RoomState>);

#[derive(Default)]
struct RoomState {
    epochs: HashMap<Lane, u64>,
    blocked: HashSet<String>,
    recipients: HashMap<String, HashSet<String>>,
}

impl Rooms {
    fn next(&self, hub: &Hub, lane: &Lane, authority: Option<&Autoridade>) -> Result<Option<u64>> {
        let mut state = self.0.locked();
        if state.blocked.contains(&lane.chat) {
            return Ok(None);
        }
        Autoridade::no_efeito(hub, authority, || Ok(Some(Self::advance(&mut state.epochs, lane))))
    }

    fn advance(map: &mut HashMap<Lane, u64>, lane: &Lane) -> u64 {
        let epoch = map.entry(lane.clone()).or_default();
        *epoch += 1;
        *epoch
    }

    #[cfg(test)]
    pub(crate) fn reserve_epoch_for_test(&self, lane: &Lane) -> u64 {
        Self::advance(&mut self.0.locked().epochs, lane)
    }

    pub(crate) fn block(&self, group: &str) {
        self.0.locked().blocked.insert(group.to_owned());
    }

    pub(crate) fn blocked(&self, group: &str) -> bool {
        self.0.locked().blocked.contains(group)
    }

    pub fn current(&self, lane: &Lane, epoch: u64) -> bool {
        let state = self.0.locked();
        !state.blocked.contains(&lane.chat) && state.epochs.get(lane).copied() == Some(epoch)
    }

    pub(crate) fn while_unblocked<T>(&self, group: &str, effect: impl FnOnce() -> T) -> Option<T> {
        let state = self.0.locked();
        (!state.blocked.contains(group)).then(effect)
    }

    fn queue_member(&self, group: &str, member: &str, send: impl FnOnce() -> Result<bool>) -> Result<Option<bool>> {
        let mut state = self.0.locked();
        if state.blocked.contains(group) {
            return Ok(None);
        }
        let queued = send()?;
        if queued {
            state.recipients.entry(group.to_owned()).or_default().insert(member.to_owned());
        }
        Ok(Some(queued))
    }

    /// Stop: ends the room turn in every lane of `group` and the member turns in it.
    pub fn stop(&self, hub: &Hub, group: &str) {
        let _ = self.stop_after(hub, group, || Ok(()));
    }

    /// Stop under the epochs' lock: either the cancellation is saved and the epochs advance, or the room is
    /// blocked until restart. In both cases member cancellations are queued before any new turn can start.
    pub(crate) fn stop_after(&self, hub: &Hub, group: &str, cancel: impl FnOnce() -> Result<()>) -> Result<()> {
        let mut state = self.0.locked();
        let result = cancel();
        if result.is_ok() {
            for (lane, epoch) in &mut state.epochs {
                if lane.chat == group {
                    *epoch += 1;
                }
            }
        } else {
            state.blocked.insert(group.to_owned());
        }
        if state.epochs.keys().any(|lane| lane.chat == group) {
            let mut members = state.recipients.remove(group).unwrap_or_default();
            match hub.store.bot(group) {
                Ok(Some(row)) => members.extend(row.config.members),
                Ok(None) => (),
                Err(error) => tracing::warn!(group, %error, "não foi possível ler os membros da sala ao parar"),
            }
            for member in members {
                let _ = hub.send_cmd(&member, Cmd::CancelGroup { chat: group.to_owned() });
            }
        }
        result
    }
}

/// Existing, distinct agent bots (not groups, not the group itself).
pub fn valid_members(store: &Store, group_id: &str, ids: &[String]) -> Result<Vec<String>> {
    let mut out: Vec<String> = vec![];
    for id in ids.iter().map(|i| i.trim()) {
        if id.is_empty() || id == group_id || out.iter().any(|o| o == id) {
            continue;
        }
        match store.bot(id)? {
            Some(row) if !row.deleted && row.config.kind == BotKind::Agent => out.push(id.to_owned()),
            Some(row) if row.config.is_group() => bail!("a group can't be a member of another group"),
            _ => {}
        }
    }
    if out.is_empty() {
        bail!("a group needs at least one bot");
    }
    Ok(out)
}

/// A group whose members are exactly `members`.
pub fn with_members(store: &Store, members: &[String]) -> Result<Option<String>> {
    let mut wanted = members.to_vec();
    wanted.sort();
    Ok(store.bots()?.into_iter().find_map(|row| {
        let mut have = row.config.members.clone();
        have.sort();
        (!row.deleted && row.config.is_group() && have == wanted).then_some(row.config.id)
    }))
}

/// The user wrote in a group lane: start a room turn there (ending one still running).
/// `delivery` tracks the user's message through the first round (`fork::grupo_entrega`).
pub(crate) fn start(hub: &Arc<Hub>, group: &BotConfig, lane: Lane, delivery: Delivery) {
    let _ = start_authorized(hub, group, lane, delivery, None);
}

pub(crate) fn start_authorized(
    hub: &Arc<Hub>,
    group: &BotConfig,
    lane: Lane,
    delivery: Delivery,
    authority: Option<Autoridade>,
) -> Result<()> {
    let epoch = match hub.groups.next(hub, &lane, authority.as_ref()) {
        Ok(Some(epoch)) => epoch,
        Ok(None) => {
            tracing::warn!(group = group.id, "sala bloqueada até o próximo reinício; nada é entregue");
            return Ok(());
        }
        Err(error) => {
            delivery.interrupt(hub, &group.id)?;
            return Err(error);
        }
    };
    let (hub, group_id) = (hub.clone(), group.id.clone());
    tokio::spawn(async move {
        if let Err(error) = run(&hub, &group_id, &lane, epoch, delivery.clone(), authority.as_ref()).await {
            if error.is::<crate::fork::autoridade::AutorizacaoEncerrada>()
                || error.is::<crate::fork::autoridade::EpochDaRequisicaoEncerrado>()
            {
                let _ = delivery.interrupt(&hub, &group_id);
            }
            tracing::warn!(group = group_id, error = format!("{error:#}"), "room turn failed");
        }
    });
    Ok(())
}

async fn run(
    hub: &Arc<Hub>,
    group_id: &str,
    lane: &Lane,
    epoch: u64,
    mut delivery: Delivery,
    authority: Option<&Autoridade>,
) -> Result<()> {
    let mut delivery_open = true;
    let mut replies = 0;
    let mut last_reply: Option<(String, String, String)> = None;
    // A resumed message gets one round, alone: nobody receives it, or an earlier one, twice.
    let upto = delivery.upto;
    let rounds = if upto.is_some() { 1 } else { MAX_ROUNDS };
    'rounds: for round in 0..rounds {
        let group = hub.store.bot(group_id)?.filter(|r| !r.deleted).ok_or_else(|| anyhow!("group deleted"))?.config;
        let members = members(hub, &group);
        let since_user = since_last_user_message(hub, lane, upto)?;
        let mut speakers = responders(&members, &since_user);
        // Each round starts one member later, so the same bot doesn't always speak first.
        if !speakers.is_empty() {
            let offset = round % speakers.len();
            speakers.rotate_left(offset);
        }
        crate::fork::lider_canal::leader_first(&group, &mut speakers);
        let mut spoke = 0;
        let first = round == 0 && delivery_open;
        for member in &speakers {
            if !hub.groups.current(lane, epoch) {
                // A newer message (which shows this one) or Stop ended the room turn.
                if delivery_open {
                    delivery.confirm(hub, group_id);
                }
                return Ok(());
            }
            // A full room turn still reports its last reply.
            if replies >= MAX_REPLIES {
                break 'rounds;
            }
            // Resumed after a restart: whoever already received the message doesn't get it again.
            if first && delivery.skips(&member.id) {
                continue;
            }
            let (prompt, shown) = member_prompt(hub, &group, &members, member, lane, upto)?;
            let (reply, answer) = oneshot::channel();
            let turn = GroupTurn { lane: lane.clone(), prompt, shown, epoch, authority: authority.cloned(), reply };
            let Some(queued) = hub.groups.queue_member(group_id, &member.id, || {
                if first {
                    delivery.hand_authorized(hub, group_id, &member.id, authority)?;
                }
                let sender = hub.login_command_sender(&member.id);
                let queued = Autoridade::no_efeito(hub, authority, || {
                    Ok(sender.is_ok_and(|sender| sender.send(Cmd::Group(turn)).is_ok()))
                })?;
                if !queued && first {
                    delivery.missed(hub, group_id, &member.id);
                }
                Ok(queued)
            })?
            else {
                return Ok(());
            };
            if !queued {
                continue;
            }
            // A failed, cancelled or overdue member turn counts as a pass; its error shows in the room.
            let answered = tokio::time::timeout(crate::chat::team::ASK_TIMEOUT, answer).await;
            if hub
                .groups
                .while_unblocked(group_id, || {
                    if first {
                        delivery.finish_member(hub, group_id, &member.id);
                    }
                })
                .is_none()
            {
                return Ok(());
            }
            if let Ok(Ok(Ok(Some(text)))) = answered {
                spoke += 1;
                replies += 1;
                last_reply = Some((member.id.clone(), member.name.clone(), text));
            }
        }
        if first {
            delivery.confirm(hub, group_id);
            delivery_open = false;
        }
        if spoke == 0 {
            break;
        }
    }
    if delivery_open {
        delivery.confirm(hub, group_id);
    }
    if let Some((from, name, text)) = last_reply
        && let Some(group) = hub.store.bot(group_id)?.map(|r| r.config)
    {
        push::notify(
            hub,
            &group,
            Some(&from),
            &Titulo::NoGrupo { nome: name.as_str(), grupo: group.name.as_str() },
            &Corpo::Texto(text.as_str()),
            AlertKind::Done,
        );
    }
    Ok(())
}

fn members(hub: &Hub, group: &BotConfig) -> Vec<BotConfig> {
    group
        .members
        .iter()
        .filter_map(|id| hub.store.bot(id).ok().flatten())
        .filter(|r| !r.deleted)
        .map(|r| r.config)
        .collect()
}

/// The room as members see it (`Store::messages_after`, filtered before the limit): messages held for the owner's
/// review stay out, and a resumed message is all the room shows.
fn visible(hub: &Hub, lane: &Lane, after: i64, limit: i64, upto: Option<i64>) -> Result<Vec<Entry>> {
    let c = hub.store.db.locked();
    let mut st = c.prepare(&format!(
        "SELECT {} FROM entries WHERE bot_id = ?1 AND seq > ?2 AND (?4 IS NULL OR seq = ?4)
           AND (kind = 'user' OR (kind = 'agent' AND json_extract(data, '$.final') = 1))
           AND COALESCE(json_extract(data, '$.{}'), 0) != 1
         ORDER BY seq DESC LIMIT ?3",
        crate::store::ENTRY_COLS,
        crate::fork::grupo_entrega::GROUP_REVIEW
    ))?;
    let mut rows: Vec<Entry> = st
        .query_map(rusqlite::params![lane.chat, after, limit, upto], crate::store::row_entry)?
        .collect::<rusqlite::Result<_>>()?;
    rows.reverse();
    Ok(rows)
}

/// A quote of a message the room can't show (held for review, or still pending) stays out of the prompt too.
fn quote_note(hub: &Hub, me: &str, reply: &serde_json::Value) -> Option<String> {
    let quoted = reply["id"].as_str().and_then(|id| hub.store.entry(id))?;
    if crate::fork::grupo_entrega::unproven(&quoted) {
        return None;
    }
    crate::chat::replies::note(hub, me, reply)
}

/// The lane's messages since the user last wrote there, that message included.
fn since_last_user_message(hub: &Hub, lane: &Lane, upto: Option<i64>) -> Result<Vec<Entry>> {
    let recent = visible(hub, lane, 0, HISTORY_LINES, upto)?;
    let start = recent.iter().rposition(|e| e.kind == EntryKind::User.as_str()).unwrap_or(0);
    Ok(recent[start..].to_vec())
}

/// Who answers: the members @-mentioned or replied to since the user's last message, or
/// everyone when nobody (or `@all` / `@everyone`) is.
fn responders<'a>(members: &'a [BotConfig], messages: &[Entry]) -> Vec<&'a BotConfig> {
    let texts: Vec<&str> = messages.iter().filter_map(|e| e.data["text"].as_str()).collect();
    let everyone = texts.iter().any(|t| mentions(t, "all") || mentions(t, "everyone"));
    let replied = |m: &BotConfig| messages.iter().any(|e| e.data["replyTo"]["author"] == m.id.as_str());
    let named: Vec<&BotConfig> = members
        .iter()
        .filter(|m| replied(m) || texts.iter().any(|t| handles(&m.name).iter().any(|h| mentions(t, h))))
        .collect();
    if everyone || named.is_empty() { members.iter().collect() } else { named }
}

/// Ways to @-mention a bot: its full name, without spaces, or its first word (lowercased).
fn handles(name: &str) -> Vec<String> {
    let lower = name.trim().to_lowercase();
    let mut out = vec![lower.clone(), lower.split_whitespace().collect::<String>()];
    if let Some(first) = lower.split_whitespace().next() {
        out.push(first.to_owned());
    }
    out.retain(|h| !h.is_empty());
    out.dedup();
    out
}

/// `@handle` in `text`, not followed by more of a Latin word (`@al` isn't `@alice`).
/// CJK names are often followed straight by the message, so only ASCII letters end a match early.
fn mentions(text: &str, handle: &str) -> bool {
    let lower = text.to_lowercase();
    let needle = format!("@{handle}");
    lower
        .match_indices(&needle)
        .any(|(i, _)| lower[i + needle.len()..].chars().next().is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_')))
}

fn member_prompt(
    hub: &Hub,
    group: &BotConfig,
    members: &[BotConfig],
    me: &BotConfig,
    lane: &Lane,
    upto: Option<i64>,
) -> Result<(String, Vec<String>)> {
    let peers: Vec<&BotConfig> = members.iter().filter(|m| m.id != me.id).collect();
    let peer_names = if peers.is_empty() {
        "just the user".to_owned()
    } else {
        peers.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")
    };
    let about = group.description.trim();
    let room = if about.is_empty() { format!("\"{}\"", group.name) } else { format!("\"{}\" — {about}", group.name) };
    // The turn's first line says what kind of turn this is, so the header comes before the instructions.
    let leader = crate::fork::lider_canal::header(group, members, me);
    let header = format!("[Channel: \"{}\" - with {peer_names}{leader}]", group.name);
    let mut out = vec![
        header,
        format!("You are {}, one participant in a channel ({room}) with the user and {peer_names}.", me.name),
    ];
    if !peers.is_empty() {
        out.push("Other participants in the room:".into());
        for p in &peers {
            let about =
                if p.description.trim().is_empty() { String::new() } else { format!(" ({})", p.description.trim()) };
            out.push(format!("- {}{about}", p.name));
        }
    }
    out.extend(crate::fork::lider_canal::guidance(group, members, me));
    out.push(format!(
        "You have your full toolkit here. Do the work first, then answer: your final reply of this turn is \
         the one message the room sees. Keep it short and conversational. @-mention another participant only \
         when you need them to act. If you have nothing new worth adding, reply exactly \"{PASS}\". Never \
         reveal private one-on-one context. Reactions are rare: react(emoji, to) only where a person would, \
         never to a question or a request, `to` naming whose newest message (\"user\" or a participant). With \
         nothing to add, a reaction and \"{PASS}\" agrees without a message."
    ));
    out.push(String::new());

    let seen = hub.store.last_spoke(lane, &me.id);
    let shown = visible(hub, lane, seen, HISTORY_LINES, upto)?;
    let lines: Vec<String> = shown.iter().map(|e| line(hub, group, e, me)).collect();
    if lines.is_empty() {
        out.push("No new messages in the room since your last turn.".into());
    } else {
        out.push("New messages in the room (oldest first):".into());
        out.extend(lines);
    }
    out.push(String::new());
    out.push(format!(
        "It's your turn, {}. Reply to the room if you have something worth adding, or reply exactly \"{PASS}\" if you don't.",
        me.name
    ));
    Ok((out.join("\n"), shown.into_iter().map(|e| e.id).collect()))
}

fn line(hub: &Hub, group: &BotConfig, e: &Entry, me: &BotConfig) -> String {
    let text = e.data["text"].as_str().unwrap_or_default();
    let reacted = crate::chat::reactions::describe(hub, e);
    if e.kind == EntryKind::User.as_str() {
        // As in the member's own chat (its skills), with the files the group keeps.
        let root = crate::chat::uploads::root(group);
        let attachments = e.data["attachments"].as_array().map_or(&[][..], Vec::as_slice);
        let reply = quote_note(hub, &me.id, &e.data["replyTo"]).map(|n| format!("{n} "));
        return format!(
            "User: {}{}{reacted}",
            reply.unwrap_or_default(),
            crate::chat::prompt::of_message(&hub.store, &me.skills, &root, text, attachments)
        );
    }
    let author = e.data["author"].as_str().unwrap_or_default();
    let text = crate::agent::acp::truncate(text, LINE_CHARS);
    if author == me.id {
        format!("{} (you): {text}{reacted}", me.name)
    } else {
        format!("{}: {text}{reacted}", hub.bot_name(author))
    }
}

/// A member's final text is a pass (the room doesn't see it).
pub fn is_pass(text: &str) -> bool {
    let t = text.trim().trim_matches(|c: char| c == '"' || c == '.' || c == '*').to_lowercase();
    t.is_empty() || t == PASS || t == "pass"
}

#[cfg(test)]
mod tests;
