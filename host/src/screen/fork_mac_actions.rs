use super::{fork_mac_consent::Consent, fork_mac_identity::macos::Xpc, fork_observation::Attempt};
use crate::{
    LockExt,
    api::devices::Caller,
    fork::{donna_fixa::pedido::Origem, permission_card, sessoes::bindings},
    hub::Hub,
    store::{Entry, EntryKind, Lane, Store},
};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex, Weak},
};
use tokio::{
    sync::oneshot,
    time::{Duration, Instant},
};

pub(crate) const CARD: &str = "mac_local_screen_action";
pub(crate) const CLICK_CARD: &str = "mac_local_screen_click";
const DENIED: &str = "A ação de tela não foi autorizada para este turno.";
const WAIT: Duration = Duration::from_secs(120);

fn monotonic_ns() -> Result<u64> {
    let mut time = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: clock_gettime writes to a valid timespec and does not retain it.
    ensure!(unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC_RAW, &raw mut time) } == 0, DENIED);
    ensure!((0..1_000_000_000).contains(&time.tv_nsec), DENIED);
    u64::try_from(time.tv_sec)?
        .checked_mul(1_000_000_000)
        .and_then(|ns| ns.checked_add(time.tv_nsec.cast_unsigned()))
        .context(DENIED)
}

fn before_deadline(deadline_ns: u64) -> Result<()> {
    ensure!(monotonic_ns()? < deadline_ns, DENIED);
    Ok(())
}

fn capture_request(name: &str, args: &Value) -> Result<()> {
    ensure!(name == "screenshot" && args.as_object().is_some_and(serde_json::Map::is_empty), DENIED);
    Ok(())
}

const RETURN_WINDOW: Duration = Duration::from_secs(10);
const CLICK_LEASE_NS: u64 = 500_000_000;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClickTarget {
    bundle_id: String,
    window: String,
    button: String,
}

impl ClickTarget {
    fn parse(args: Value) -> Result<Self> {
        let target: Self = serde_json::from_value(args).context("Informe app, janela e botão exatos.")?;
        ensure!(
            target.bundle_id.len() <= 120
                && target.bundle_id.split('.').count() >= 2
                && target
                    .bundle_id
                    .split('.')
                    .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
                && label(&target.window)
                && label(&target.button),
            "O aplicativo, a janela ou o botão não têm uma identificação válida."
        );
        Ok(target)
    }
}

fn label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 120
        && value.trim() == value
        && !value.chars().any(|c| c.is_control()
            || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '‹' | '›'))
}

pub(crate) async fn catalog(hub: &Arc<Hub>, caller: &Caller) -> Value {
    let Caller::Ponte { bot, sessao, .. } = caller else { return json!({"tools":[]}) };
    let allowed = || -> Result<()> {
        ensure!(hub.screen.enabled(), DENIED);
        crate::fork::ponte_credencial::conferir(hub, caller, "computerTools", &json!({}))?;
        let db = hub.store.db.locked();
        ensure!(bindings::resolve(&db, sessao, bot)?.slot.is_none(), DENIED);
        bot_consent(&db, bot)?;
        Ok(())
    };
    let Some(xpc) = hub.screen.mac_xpc.get().filter(|_| allowed().is_ok()) else {
        return json!({"tools":[]});
    };
    if helper_target(xpc).await.is_err() || allowed().is_err() {
        return json!({"tools":[]});
    }

    json!({"tools": [
        {"name":"screenshot","description":"Capture the main display, with the local owner's approval. Returns an image and the logical display size in points.",
         "inputSchema":{"type":"object","properties":{},"additionalProperties":false},"annotations":{"readOnlyHint":true}},
        {"name":"click","description":"Press one uniquely named native button in the exact frontmost window on the main display. The owner must approve and return to that window. No coordinate or keyboard input.",
         "inputSchema":{"type":"object","properties":{"bundleId":{"type":"string","maxLength":120},"window":{"type":"string","maxLength":120},"button":{"type":"string","maxLength":120}},
                        "required":["bundleId","window","button"],"additionalProperties":false}}
    ]})
}

struct Waiting {
    hub: Weak<Hub>,
    attempt: Arc<Attempt>,
    card: Entry,
    deadline_ns: u64,
    answer: oneshot::Sender<(Entry, u64)>,
}
static PENDING: LazyLock<Mutex<HashMap<String, Waiting>>> = LazyLock::new(Mutex::default);

struct Cancel<'a>(&'a Hub, &'a Entry);
impl Drop for Cancel<'_> {
    fn drop(&mut self) {
        if PENDING.locked().remove(&self.1.id).is_some() {
            let _ = commit(self.0, self.1, None, None, |_| Ok(()));
        }
    }
}

fn commit(
    hub: &Hub,
    card: &Entry,
    option: Option<&str>,
    origin: Option<&Origem>,
    check: impl FnOnce(&rusqlite::Connection) -> Result<()>,
) -> Result<Entry> {
    let mut updated = card.data.clone();
    updated["status"] = if option.is_some() { json!("answered") } else { json!("cancelled") };
    updated["selected"] = json!(option);
    let _emit = hub.emit_lock.locked();
    let (saved, ()) = permission_card::commit_original(hub, &card.id, origin, |db| {
        check(db)?;
        check_card(db, card)?;
        Ok((updated, None, ()))
    })?;
    let saved = saved.context(DENIED)?;
    let _ = hub.events.send(json!({"type":"entry","rev":saved.rev,"entry":saved}));
    Ok(saved)
}

fn check_card(db: &rusqlite::Connection, card: &Entry) -> Result<()> {
    ensure!(Store::entry_on(db, &card.id)?.is_some_and(|row| row.rev == card.rev && row.data == card.data), DENIED);
    Ok(())
}

fn bot_consent(db: &rusqlite::Connection, bot: &str) -> Result<Consent> {
    let row = Store::bot_on(db, bot)?.context(DENIED)?;
    ensure!(!row.deleted && !row.config.hidden && !row.config.is_group(), DENIED);
    Consent::read(db, bot)?.context(DENIED)
}

fn check_turn(db: &rusqlite::Connection, attempt: &Attempt) -> Result<Entry> {
    ensure!(Some(bot_consent(db, &attempt.bot)?) == attempt.private_consent, DENIED);
    ensure!(crate::fork::spaces::config_generation_on(db, &attempt.bot)? == attempt.generation, DENIED);
    bindings::assert_execution_live(db, &attempt.execution)?;
    let message = attempt.runtime.message.as_deref().context(DENIED)?;
    let entry = Store::entry_on(db, message)?.context(DENIED)?;
    ensure!(
        entry.kind == "user"
            && entry.bot_id == attempt.bot
            && entry.session_id == attempt.bot
            && entry.data["contentOrigin"]["ingress"]["caller"] == "local"
            && entry.data["contentOrigin"]["hubOrigin"].is_null(),
        DENIED
    );
    Ok(entry)
}

pub(crate) fn respond(
    hub: &Arc<Hub>,
    entry: &Entry,
    revision: i64,
    source: &Value,
    option: Option<&str>,
    origin: Option<&Origem>,
) -> Result<Value> {
    ensure!(
        super::fork_mac_local::is_private(hub)
            && matches!(origin, Some(Origem::Local))
            && source == "app"
            && matches!(option, None | Some("allow" | "deny")),
        DENIED
    );
    let waiting = {
        let mut pending = PENDING.locked();
        let current = pending.get(&entry.id).context(DENIED)?;
        ensure!(
            Weak::ptr_eq(&current.hub, &Arc::downgrade(hub))
                && current.card.rev == revision
                && current.card.data == entry.data
                && before_deadline(current.deadline_ns).is_ok(),
            DENIED
        );
        pending.remove(&entry.id).context(DENIED)?
    };
    let saved = (|| {
        let answered_at = monotonic_ns()?;
        if option == Some("allow") {
            waiting.attempt.check_waiting()?;
        }
        commit(hub, &waiting.card, option, origin, |db| {
            before_deadline(waiting.deadline_ns)?;
            if option == Some("allow") {
                check_turn(db, &waiting.attempt)?;
            }
            Ok(())
        })
        .map(|saved| (saved, answered_at))
    })();
    let saved = match saved {
        Ok(saved) => saved,
        Err(error) => {
            let _ = commit(hub, &waiting.card, None, None, |_| Ok(()));
            return Err(error);
        }
    };
    let _ = waiting.answer.send(saved);
    Ok(json!({}))
}

async fn helper_target(xpc: &Xpc) -> Result<(String, u32)> {
    let call = xpc.send(&json!({"method":"status"}))?;
    let reply = tokio::task::spawn_blocking(move || call.wait(3)).await??;
    let status = &reply["result"];
    ensure!(
        status["capture"] == true && status["input"] == true,
        "Conceda Gravação de Tela e Acessibilidade ao helper."
    );
    let instance = status["instance"].as_str().context(DENIED)?;
    uuid::Uuid::parse_str(instance).context(DENIED)?;
    let display = u32::try_from(status["display"].as_u64().context(DENIED)?)?;
    ensure!(display != 0, DENIED);
    Ok((instance.to_owned(), display))
}

pub(crate) async fn call(hub: &Arc<Hub>, attempt: Attempt, name: &str, args: Value) -> Result<Value> {
    let click = if name == "click" {
        Some(ClickTarget::parse(args)?)
    } else {
        capture_request(name, &args)?;
        None
    };
    let xpc = hub.screen.mac_xpc.get().context("O helper autenticado não está disponível.")?;
    ensure!(attempt.private_consent.is_some(), DENIED);
    let (instance, display) = helper_target(xpc).await?;
    let attempt = Arc::new(attempt);
    let (anchor, lane) = attempt.with_private_screen_send(
        |_| Ok(()),
        || Ok((attempt.runtime.message.clone().context(DENIED)?, attempt.runtime.lane.clone().context(DENIED)?)),
    )?;
    ensure!(lane.chat == attempt.bot && lane.session.is_none(), DENIED);
    let turn = {
        let db = hub.store.db.locked();
        let entry = check_turn(&db, &attempt)?;
        ensure!(entry.id == anchor, DENIED);
        entry.turn
    };
    let id = uuid::Uuid::new_v4().to_string();
    let (send, receive) = oneshot::channel();
    let deadline = Instant::now() + WAIT;
    let deadline_ns = monotonic_ns()?.checked_add(u64::try_from(WAIT.as_nanos())?).context(DENIED)?;
    let (kind, title, token) = if let Some(target) = click {
        let prepare = json!({
            "method":"clickPrepare","instance":instance,"display":display,"deadlineNs":deadline_ns.to_string(),
            "bundleId":target.bundle_id,"window":target.window,"button":target.button,
        });
        let pending = attempt.with_private_screen_send(
            |db| {
                ensure!(hub.screen.enabled(), DENIED);
                check_turn(db, &attempt).map(|_| ())
            },
            || xpc.send(&prepare),
        )?;
        let reply = tokio::task::spawn_blocking(move || pending.wait(3)).await??;
        let token = reply["result"]["token"]
            .as_str()
            .and_then(|token| uuid::Uuid::parse_str(token).ok())
            .context("Não encontrei um botão nativo único na janela ativa indicada.")?;
        let title = format!("{}\n{}\n{}", target.bundle_id, target.window, target.button);
        (CLICK_CARD, title, Some(token))
    } else {
        (CARD, format!("screenshot(display={display})"), None)
    };
    let card = {
        let mut pending = PENDING.locked();
        let (card, _) = hub.add_entry_once(
            &id,
            &Lane { chat: attempt.bot.clone(), session: None },
            EntryKind::Permission,
            turn,
            &json!({
                "toolKind": kind, "author": attempt.bot, "title": title,
                "status":"pending",
                "options":[
                    {"optionId":"allow","kind":"allow_once","name":"Allow once"},
                    {"optionId":"deny","kind":"reject_once","name":"Deny"}
                ]
            }),
        )?;
        pending.insert(
            id,
            Waiting {
                hub: Arc::downgrade(hub),
                attempt: attempt.clone(),
                card: card.clone(),
                deadline_ns,
                answer: send,
            },
        );
        card
    };
    let _cancel = Cancel(hub, &card);
    let (approved, answered_at) = tokio::time::timeout_at(deadline, receive).await??;
    ensure!(approved.data["status"] == "answered" && approved.data["selected"] == "allow", DENIED);
    let request = json!({
        "method":"screenshot","instance":instance,"display":display,"deadlineNs":deadline_ns.to_string()
    });
    let check = |db: &rusqlite::Connection| {
        before_deadline(deadline_ns)?;
        ensure!(hub.screen.enabled(), DENIED);
        check_card(db, &approved)?;
        check_turn(db, &attempt)?;
        Ok(())
    };
    if let Some(token) = token {
        let limit = deadline_ns.min(answered_at.checked_add(u64::try_from(RETURN_WINDOW.as_nanos())?).context(DENIED)?);
        let wait_error = "O prazo venceu sem voltar à janela aprovada. Nenhum botão foi acionado.";
        loop {
            before_deadline(limit).context(wait_error)?;
            let probe = json!({
                "method":"clickReady","instance":instance,"display":display,"token":token,"deadlineNs":limit.to_string()
            });
            let pending = attempt.with_private_screen_send(check, || xpc.send(&probe))?;
            let reply = tokio::task::spawn_blocking(move || pending.wait(1))
                .await?
                .context("O helper não confirmou a janela a tempo. Nenhum botão foi acionado.")?;
            match reply["result"]["state"].as_str() {
                Some("waiting") => tokio::time::sleep(Duration::from_millis(125)).await,
                Some("ready") => break,
                _ => anyhow::bail!("A janela, o botão ou as permissões mudaram. Nenhum botão foi acionado."),
            }
        }
        let pending = attempt.with_private_screen_send(check, || {
            before_deadline(limit).context(wait_error)?;
            let lease = limit.min(monotonic_ns()?.checked_add(CLICK_LEASE_NS).context(DENIED)?);
            xpc.send(&json!({
                "method":"clickCommit","instance":instance,"display":display,"token":token,"deadlineNs":lease.to_string()
            }))
        })?;
        let reply = tokio::task::spawn_blocking(move || pending.wait(2))
            .await?
            .context("Não foi possível confirmar o clique. Não vou repetir a ação.")?;
        let text = match reply["result"]["state"].as_str() {
            Some("pressed") => "O botão aprovado foi acionado.",
            Some("refused") => "A janela, o botão ou o prazo mudou. Nenhum botão foi acionado.",
            _ => "Não foi possível confirmar o clique. Não vou repetir a ação.",
        };
        return Ok(json!({"content":[{"type":"text","text":text}]}));
    }
    let sent = attempt.with_private_screen_send(check, || {
        before_deadline(deadline_ns)?;
        xpc.send(&request)
    })?;
    let reply = tokio::time::timeout_at(deadline, tokio::task::spawn_blocking(move || sent.wait(12))).await???;
    let reply = reply.get("result").context("O helper recusou a captura ou as permissões mudaram.")?;
    let data = reply["data"].as_str().context("A captura não trouxe uma imagem.")?;
    let response = json!({"content":[
        {"type":"image","data":data,"mimeType":"image/jpeg"},
        {"type":"text","text":format!("Tela principal: {} × {} pontos.", reply["width"], reply["height"])}
    ]});
    attempt.with_private_screen_send(check, || {
        before_deadline(deadline_ns)?;
        Ok(response)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_capture_without_arguments_is_supported() {
        assert!(capture_request("screenshot", &json!({})).is_ok());
        assert!(capture_request("screenshot", &json!({"display": 1})).is_err());
        assert!(ClickTarget::parse(json!({"x": 1, "y": 1})).is_err());
        assert!(
            ClickTarget::parse(json!({"bundleId":"app.example.invalid", "window":"Janela", "button":"Ok"})).is_ok()
        );
        assert!(
            ClickTarget::parse(json!({"bundleId":"app.example.invalid", "window":"Janela", "button":"Ok › Não"}))
                .is_err()
        );
    }

    #[test]
    fn capture_admission_rejects_expired_deadlines() {
        assert!(before_deadline(monotonic_ns().unwrap()).is_err());
        assert!(before_deadline(monotonic_ns().unwrap() + 1_000_000_000).is_ok());
    }
}
