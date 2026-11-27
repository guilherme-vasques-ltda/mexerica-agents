use crate::{
    LockExt,
    fork::{donna_fixa::pedido::Origem, permission_card, skill_catalog, skill_selection, spaces::PERSONAL},
    hub::Hub,
    store::{BotConfig, Entry, EntryKind, Lane, Store},
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex, Weak},
    time::Duration,
};
use tokio::{sync::oneshot, time::Instant};

pub(crate) const CARD_KIND: &str = "open_hub_action";
const ALLOW: &str = "allow";
const DENY: &str = "deny";
const WAIT: Duration = Duration::from_secs(120);

pub(super) fn catalog() -> Vec<Value> {
    [
        ("list_catalog_skills", "Lista skills de texto permitidas no catálogo Pessoal.", json!({}), json!([])),
        ("install_skill", "Pede aprovação local para instalar uma skill do catálogo, sem ligá-la.",
            json!({"id":{"type":"string"},"digest":{"type":"string"}}), json!(["id","digest"])),
        ("enable_skill", "Pede aprovação local para ligar ao seu bot uma skill permitida já instalada.",
            json!({"id":{"type":"string"}}), json!(["id"])),
        ("save_routine", "Pede aprovação local para criar ou editar sua rotina. Informe id apenas ao editar. Use cron com fuso IANA, interval em segundos, once em milissegundos Unix ou manual:true sem gatilhos.",
            json!({
                "id":{"type":"string"}, "name":{"type":"string"}, "instruction":{"type":"string"},
                "timeoutSeconds":{"type":"integer","minimum":1,"maximum":86400},
                "triggers":{"type":"array","items":{"type":"object","properties":{
                    "type":{"enum":["cron","once","interval"]},"expression":{"type":"string"},"timeZone":{"type":"string"},
                    "seconds":{"type":"integer"},"at":{"type":"integer"}
                },"required":["type"],"additionalProperties":false}}, "manual":{"type":"boolean"}
            }), json!(["name","instruction"])),
        ("pause_routine", "Pede aprovação local para pausar uma rotina deste bot.",
            json!({"id":{"type":"string"}}), json!(["id"])),
    ]
    .into_iter()
    .map(|(name, description, properties, required)| json!({
        "type":"function", "name":name, "description":description,
        "inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}
    }))
    .collect()
}

pub(super) fn offered(name: &str) -> bool {
    matches!(name, "list_catalog_skills" | "install_skill" | "enable_skill" | "save_routine" | "pause_routine")
}

pub(super) async fn call(
    hub: &Arc<Hub>,
    lane: &Lane,
    turn: i64,
    name: &str,
    args: &Value,
    revalidate: impl Fn() -> Result<BotConfig>,
) -> Result<Value> {
    ensure!(crate::environment::Environment::is_open() && lane.session.is_none(), "open_tools_local_turn_required");
    let cfg = revalidate()?;
    ensure!(cfg.id == lane.chat && !cfg.is_group() && !cfg.hidden, "open_tools_local_turn_required");
    let current = || {
        let row = revalidate()?;
        ensure!(
            row.id == cfg.id && row.space_id == cfg.space_id && !row.hidden && !row.is_group(),
            "open_tools_local_turn_required"
        );
        Ok(())
    };
    if name == "list_catalog_skills" {
        fields(args, &[])?;
        ensure!(cfg.space_id == PERSONAL, "skills_personal_only");
        let result = skill_catalog::open_catalog(&hub.store, current)?;
        current()?;
        return Ok(result);
    }
    let (title, operation, detail) = match name {
        "install_skill" => {
            fields(args, &["id", "digest"])?;
            ensure!(cfg.space_id == PERSONAL, "skills_personal_only");
            let (id, digest) = (required(args, "id")?, required(args, "digest")?);
            let catalog = skill_catalog::open_catalog(&hub.store, current)?;
            let item = catalog["items"]
                .as_array()
                .and_then(|items| {
                    items.iter().find(|item| item["id"] == id && item["digest"] == digest && item["installed"] == false)
                })
                .context("open_tools_skill_catalog_changed")?;
            (
                "Instalar skill do catálogo",
                Operation::Install(json!({"catalogId":id,"expectedDigest":digest})),
                json!({"skill":item["name"],"id":id,"digest":digest}),
            )
        }
        "enable_skill" => {
            fields(args, &["id"])?;
            ensure!(cfg.space_id == PERSONAL, "skills_personal_only");
            let id = required(args, "id")?;
            let installation = skill_catalog::open_skill_installation(&hub.store, id, current)?;
            let version = hub
                .store
                .bot(&cfg.id)?
                .and_then(|row| skill_selection::valid_version(row.skills_version))
                .context("open_tools_skill_selection_unavailable")?;
            ("Ligar skill instalada", Operation::Enable { id, installation, version }, json!({"skill":id,"bot":cfg.id}))
        }
        "save_routine" | "pause_routine" => {
            let (body, expected, detail) = routine_intent(hub, &cfg, name, args)?;
            let title = if name == "pause_routine" {
                "Pausar rotina"
            } else if expected.is_some() {
                "Editar rotina"
            } else {
                "Criar rotina"
            };
            (title, Operation::Routine { body, expected }, detail)
        }
        _ => bail!("open_tools_unknown_tool"),
    };
    current()?;
    let approved = approve(hub, lane, turn, title, &serde_json::to_string_pretty(&detail)?).await?;
    let _team = hub.store.travar_equipe().await;
    let check_db = |db: &rusqlite::Connection| {
        check_card(db, &approved)?;
        let bot = Store::bot_on(db, &cfg.id)?.context("open_tools_local_turn_required")?;
        ensure!(
            !bot.deleted && !bot.config.hidden && !bot.config.is_group() && bot.config.space_id == cfg.space_id,
            "open_tools_local_turn_required"
        );
        Ok(())
    };
    current()?;
    match operation {
        Operation::Install(body) => {
            let result = skill_catalog::install_open_approved(&hub.store, &body, || {
                current()?;
                check_db(&hub.store.db.locked())
            })?;
            Ok(json!({"id":result["skill"]["id"],"installed":true}))
        }
        Operation::Enable { id, installation, version } => {
            skill_selection::enable_open(hub, &cfg.id, id, version, installation, current, check_db)?;
            Ok(json!({"id":id,"enabled":true}))
        }
        Operation::Routine { body, expected } => {
            hub.routines.save_open_action(hub, &cfg, &body, expected, current, check_db)
        }
    }
}

enum Operation<'a> {
    Install(Value),
    Enable { id: &'a str, installation: String, version: String },
    Routine { body: Value, expected: Option<i64> },
}

fn fields(value: &Value, allowed: &[&str]) -> Result<()> {
    ensure!(
        value.as_object().is_some_and(|object| object.keys().all(|key| allowed.contains(&key.as_str()))),
        "open_tools_invalid_arguments"
    );
    Ok(())
}

fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key].as_str().filter(|s| !s.trim().is_empty()).context("open_tools_invalid_arguments")
}

fn routine_intent(hub: &Hub, cfg: &BotConfig, name: &str, args: &Value) -> Result<(Value, Option<i64>, Value)> {
    let pause = name == "pause_routine";
    if pause {
        fields(args, &["id"])?;
        required(args, "id")?;
    } else {
        fields(args, &["id", "name", "instruction", "manual", "triggers", "timeoutSeconds"])?;
        required(args, "name")?;
        required(args, "instruction")?;
        ensure!(args.get("manual").is_none_or(Value::is_boolean), "open_tools_invalid_arguments");
        ensure!(
            args.get("timeoutSeconds").is_none_or(|value| value.as_u64().is_some()),
            "open_tools_invalid_arguments"
        );
    }
    let listed;
    let existing = if args.get("id").is_some() {
        let id = required(args, "id")?;
        listed = hub.routines.list(&cfg.id);
        let routine = listed["routines"]
            .as_array()
            .and_then(|items| items.iter().find(|item| item["id"] == id))
            .context("open_tools_routine_not_found")?;
        ensure!(routine["heartbeat"].is_null(), "open_tools_routine_heartbeat_unavailable");
        Some(routine)
    } else {
        None
    };
    let mut body = args.clone();
    if pause {
        let old = existing.context("open_tools_routine_not_found")?;
        body = json!({
            "id":old["id"],"name":old["name"],"instruction":old["instruction"],"triggers":old["triggers"],
            "timeoutSeconds":old["timeoutSeconds"],"manual":old["manual"],"enabled":false
        });
    }
    if let Some(triggers) = body.get("triggers") {
        for trigger in triggers.as_array().context("open_tools_invalid_arguments")? {
            match required(trigger, "type")? {
                "cron" => fields(trigger, &["type", "expression", "timeZone"])?,
                "once" => fields(trigger, &["type", "at"])?,
                "interval" => fields(trigger, &["type", "seconds"])?,
                _ => bail!("open_tools_routine_trigger_unavailable"),
            }
        }
    }
    crate::fork::open_rotinas::check_save(&body)?;
    let checked = hub.routines.check(&cfg.id, &body)?;
    let expected = existing.map(|old| old["updatedAt"].as_i64().context("open_tools_routine_changed")).transpose()?;
    let detail = if pause {
        json!({"id":body["id"],"name":checked.name,"enabled":false})
    } else {
        json!({
            "id":body["id"], "name":checked.name,"instruction":checked.instruction,
            "triggers":checked.triggers,"manual":checked.triggers.is_empty(),
            "timeoutSeconds":checked.timeout.or_else(|| existing.and_then(|old| old["timeoutSeconds"].as_u64())),
            "enabled":existing.is_none_or(|old| old["enabled"] == true),
        })
    };
    Ok((body, expected, detail))
}

struct Pending {
    hub: Weak<Hub>,
    card: Entry,
    answer: oneshot::Sender<Entry>,
    deadline: Instant,
}

static PENDING: LazyLock<Mutex<HashMap<String, Pending>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) fn app_response(body: &Value) -> bool {
    body.get("source").is_none_or(|source| source.is_null() || source == "app")
}

pub(super) fn respond(
    hub: &Arc<Hub>,
    entry: &Entry,
    revision: i64,
    body: &Value,
    option: Option<&str>,
    origin: Option<&Origem>,
) -> Result<Value> {
    ensure!(
        crate::environment::Environment::is_open()
            && matches!(origin, Some(Origem::Local))
            && app_response(body)
            && entry.kind == "permission"
            && entry.data["toolKind"] == CARD_KIND,
        "open_tools_local_app_required"
    );
    let mut pending = PENDING.locked();
    let current = pending.get(&entry.id).context("open_tools_approval_not_pending")?;
    ensure!(
        Weak::ptr_eq(&current.hub, &Arc::downgrade(hub))
            && revision == current.card.rev
            && matches!(option, None | Some(ALLOW | DENY)),
        "open_tools_approval_stale"
    );
    let mut data = current.card.data.clone();
    data["status"] = if option.is_some() { json!("answered") } else { json!("cancelled") };
    data["selected"] = json!(option);
    let confirmed = permission_card::commit_checked(hub, &entry.id, &data, None, origin, |db| {
        ensure!(Instant::now() < current.deadline, "open_tools_approval_expired");
        check_card(db, &current.card)
    })?
    .context("open_tools_approval_stale")?;
    if let Some(current) = pending.remove(&entry.id) {
        let _ = current.answer.send(confirmed);
    }
    Ok(json!({}))
}

fn check_card(db: &rusqlite::Connection, expected: &Entry) -> Result<()> {
    ensure!(
        Store::entry_on(db, &expected.id)?
            .is_some_and(|entry| entry.rev == expected.rev && entry.data == expected.data),
        "open_tools_approval_stale"
    );
    Ok(())
}

struct Waiting<'a> {
    hub: &'a Hub,
    card: Entry,
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        if PENDING.locked().remove(&self.card.id).is_some() {
            let mut closed = self.card.data.clone();
            closed["status"] = json!("cancelled");
            let _ = permission_card::commit_checked(self.hub, &self.card.id, &closed, None, None, |db| {
                check_card(db, &self.card)
            });
        }
    }
}

async fn approve(hub: &Arc<Hub>, lane: &Lane, turn: i64, title: &str, detail: &str) -> Result<Entry> {
    ensure!(
        lane.session.is_none()
            && detail.encode_utf16().count() <= 2000
            && !detail.contains([
                '\u{061c}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}', '\u{202e}',
                '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}'
            ]),
        "open_tools_invalid_approval"
    );
    let (answer, received) = oneshot::channel();
    let deadline = Instant::now() + WAIT;
    let card = {
        let mut pending = PENDING.locked();
        let (card, _) = hub.add_entry_once(
            &uuid::Uuid::new_v4().to_string(),
            lane,
            EntryKind::Permission,
            turn,
            &json!({
                "author": lane.chat, "toolKind": CARD_KIND, "title": title, "detail": detail, "status": "pending",
                "options": [
                    {"optionId": ALLOW, "kind": "allow_once", "name": "Allow once"},
                    {"optionId": DENY, "kind": "reject_once", "name": "Deny"}
                ]
            }),
        )?;
        pending.insert(card.id.clone(), Pending { hub: Arc::downgrade(hub), card: card.clone(), answer, deadline });
        card
    };
    let waiting = Waiting { hub, card };
    let received = tokio::time::timeout_at(deadline, received).await.context("open_tools_approval_expired")??;
    if received.data["selected"] != ALLOW {
        bail!("open_tools_approval_denied");
    }
    let snapshot = permission_card::PendingContext::capture(&waiting.card, None);
    snapshot.answered(&received, ALLOW)?;
    Ok(received)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn fixture() -> (Arc<Hub>, tempfile::TempDir) {
        let mut builder = tempfile::Builder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(std::fs::Permissions::from_mode(0o700));
        }
        let directory = builder.tempdir().unwrap();
        let store = Store::open(&directory.path().join("hub.db")).unwrap();
        let bot: BotConfig = serde_json::from_value(json!({"id":"bot","name":"Bot","backend":"codex"})).unwrap();
        store.save_bot(&bot).unwrap();
        let hub = Hub::new(
            store,
            "open-tool-actions".into(),
            crate::remote::identity::Identity::load_or_create(directory.path()).unwrap(),
            "test".into(),
            0,
        );
        (hub, directory)
    }

    async fn next_card(hub: &Hub, events: &mut tokio::sync::broadcast::Receiver<Value>) -> Entry {
        loop {
            let event = tokio::time::timeout(Duration::from_secs(5), events.recv()).await.unwrap().unwrap();
            if event["entry"]["kind"] == "permission"
                && let Some(card) = event["entry"]["id"].as_str().and_then(|id| hub.store.entry(id))
                && card.data["status"] == "pending"
            {
                return card;
            }
        }
    }

    async fn accept_next(hub: &Arc<Hub>, events: &mut tokio::sync::broadcast::Receiver<Value>) {
        let card = next_card(hub, events).await;
        crate::api::dispatch(
            hub,
            &crate::api::devices::Caller::Local,
            "respondPermission",
            json!({
                "source":"app","botId":card.bot_id,"entryId":card.id,"entryRev":card.rev,"optionId":ALLOW
            }),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn approval_accepts_only_the_live_card_local_app_and_exact_revision() {
        if !crate::environment::Environment::is_open() {
            return;
        }
        let (hub, _directory) = fixture();
        let lane = Lane::main("bot");
        let mut events = hub.events.subscribe();
        let (answer, ()) = tokio::join!(approve(&hub, &lane, 7, "Teste", "Detalhe"), async {
            let card = next_card(&hub, &mut events).await;
            let local = Some(&Origem::Local);
            let app = json!({"source":"app"});
            assert!(respond(&hub, &card, card.rev, &app, Some(ALLOW), None).is_err());
            assert!(respond(&hub, &card, card.rev, &json!({"source":"voice"}), Some(ALLOW), local).is_err());
            assert!(respond(&hub, &card, card.rev + 1, &app, Some(ALLOW), local).is_err());
            assert!(respond(&hub, &card, card.rev, &app, Some("always"), local).is_err());
            respond(&hub, &card, card.rev, &app, Some(ALLOW), local).unwrap();
            assert!(respond(&hub, &card, card.rev, &app, Some(ALLOW), local).is_err());
        });
        let answer = answer.unwrap();
        assert_eq!(answer.turn, 7);
        assert_eq!(answer.data["selected"], ALLOW);
    }

    #[tokio::test]
    async fn dropping_approval_closes_its_card_without_an_effect() {
        let (hub, _directory) = fixture();
        let lane = Lane::main("bot");
        let mut events = hub.events.subscribe();
        let mut waiting = Box::pin(approve(&hub, &lane, 7, "Teste", "Detalhe"));
        let card = tokio::select! {
            _ = &mut waiting => panic!("aprovação concluída sem resposta"),
            card = next_card(&hub, &mut events) => card,
        };
        drop(waiting);
        assert_eq!(hub.store.entry(&card.id).unwrap().data["status"], "cancelled");
        assert!(!PENDING.locked().contains_key(&card.id));
    }

    #[test]
    fn routine_refuses_cross_bot_target_extra_fields_and_network_trigger() {
        let (hub, _directory) = fixture();
        let cfg = hub.store.bot("bot").unwrap().unwrap().config;
        for args in [
            json!({"name":"Teste","instruction":"Avisar","triggers":[{"type":"webhook"}]}),
            json!({"name":"Teste","instruction":"Avisar","manual":true,"botId":"outro"}),
            json!({"name":"Teste","instruction":"Avisar","manual":true,"id":"outro"}),
            json!({"name":"Teste","instruction":"Avisar","manual":true,"enabled":true}),
        ] {
            assert!(routine_intent(&hub, &cfg, "save_routine", &args).is_err());
        }
        let args = json!({
            "name":"Teste","instruction":"Avisar","triggers":[{"type":"cron","expression":"0 9 * * *","timeZone":"UTC"}]
        });
        assert!(routine_intent(&hub, &cfg, "save_routine", &args).is_ok());
    }

    #[tokio::test]
    async fn routine_creation_uses_shared_quota_and_stale_turn_never_saves() {
        if !crate::environment::Environment::is_open() {
            return;
        }
        let (hub, _directory) = fixture();
        hub.store
            .kv_set(
                &format!("donna.routine-creation.policy.v1.{PERSONAL}.bot"),
                r#"{"version":1,"routineCreationLimit24h":1}"#,
            )
            .unwrap();
        let lane = Lane::main("bot");
        let alive = std::cell::Cell::new(true);
        let revalidate = || {
            ensure!(alive.get(), "turn_cancelled");
            Ok(hub.store.bot("bot")?.unwrap().config)
        };
        for (turn, stop) in [(1, true), (2, false), (3, false)] {
            alive.set(true);
            let mut events = hub.events.subscribe();
            let args = json!({"name":format!("Rotina {turn}"),"instruction":"Avisar","manual":true});
            let (result, ()) = tokio::join!(call(&hub, &lane, turn, "save_routine", &args, revalidate), async {
                let card = next_card(&hub, &mut events).await;
                if stop {
                    alive.set(false);
                }
                respond(&hub, &card, card.rev, &json!({}), Some(ALLOW), Some(&Origem::Local)).unwrap();
            });
            match turn {
                1 => assert_eq!(result.unwrap_err().to_string(), "turn_cancelled"),
                2 => assert_eq!(result.unwrap()["routine"]["name"], "Rotina 2"),
                _ => assert!(result.unwrap_err().to_string().starts_with("routine_creation_limit:")),
            }
        }
        assert_eq!(hub.routines.list("bot")["routines"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn catalog_install_does_not_enable_the_skill_and_enable_requires_a_second_card() {
        if !crate::environment::Environment::is_open() {
            return;
        }
        let (hub, _directory) = fixture();
        let lane = Lane::main("bot");
        let revalidate = || Ok(hub.store.bot("bot")?.unwrap().config);
        let catalog = call(&hub, &lane, 1, "list_catalog_skills", &json!({}), revalidate).await.unwrap();
        let skill = catalog["items"].as_array().unwrap().iter().find(|item| item["id"] == "grilling").unwrap();
        let mut events = hub.events.subscribe();
        let install = json!({"id":skill["id"],"digest":skill["digest"]});
        let (installed, ()) =
            tokio::join!(call(&hub, &lane, 1, "install_skill", &install, revalidate), accept_next(&hub, &mut events));
        assert_eq!(installed.unwrap(), json!({"id":"grilling","installed":true}));
        assert!(revalidate().unwrap().skills.is_empty());
        let enable = json!({"id":"grilling"});
        let (enabled, ()) =
            tokio::join!(call(&hub, &lane, 1, "enable_skill", &enable, revalidate), accept_next(&hub, &mut events));
        assert_eq!(enabled.unwrap(), json!({"id":"grilling","enabled":true}));
        assert_eq!(revalidate().unwrap().skills, ["grilling"]);
    }
}
