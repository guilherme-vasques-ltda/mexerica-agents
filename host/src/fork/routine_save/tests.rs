#![allow(clippy::unwrap_used)]
//! R2/P2: fixa a ordem lookup -> exclusão -> commit pendente, sem sleep/evals.
use super::*;
use crate::{remote::identity::Identity, store::BotConfig};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    time::Duration,
};

struct Fixture {
    hub: Arc<Hub>,
    bot: String,
    dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("donnabot-save-delete-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let bot = uuid::Uuid::new_v4().to_string();
        let store = Store::open(Path::new(":memory:")).unwrap();
        let cfg: BotConfig = serde_json::from_value(json!({
            "id":bot,"name":"Fixture","backend":"fixture","cwd":dir,"notify":false
        }))
        .unwrap();
        store.save_bot(&cfg).unwrap();
        let hub = Hub::new(store, "fixture".into(), Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
        hub.routines.restore(&hub.store).unwrap();
        Self { hub, bot, dir }
    }

    fn persisted(&self) -> String {
        self.hub.store.kv_read("routines.v1").unwrap().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).unwrap();
    }
}

fn body() -> Value {
    json!({"name":"Rotina fixture","instruction":"Somente fixture sintética","triggers":[{"type":"webhook"}]})
}

#[test]
fn gestao_nao_libera_rotina_externa_pendente() {
    let f = Fixture::new();
    let original = f.hub.routines.save(&f.hub, &f.bot, &body()).unwrap();
    let id = original["routine"]["id"].as_str().unwrap();
    f.hub
        .routines
        .change_bound(&f.hub.store, Some(&f.bot), |state| {
            let routine = state.routines.iter_mut().find(|routine| routine.id == id).unwrap();
            routine.enabled = false;
            routine.external_creation = true;
            routine.external_release_pending = Some(crate::routines::ExternalRelease::Pending);
            Ok(())
        })
        .unwrap();
    let before = f.persisted();
    let mut committed = false;
    let mut edited = body();
    edited["id"] = json!(id);
    edited["enabled"] = json!(true);
    let result = f.hub.routines.save_team(&f.bot, &[edited], |_, _| {
        committed = true;
        Ok(())
    });
    assert!(result.is_err());
    assert!(!committed);
    assert_eq!(f.persisted(), before);
    assert_eq!(f.hub.routines.list(&f.bot)["routines"][0]["externalReleasePending"], true);
    f.hub.routines.set_enabled(&f.hub.store, &f.bot, id, true).unwrap();
    assert_eq!(f.hub.routines.list(&f.bot)["routines"][0]["enabled"], true);
    assert_eq!(f.hub.routines.list(&f.bot)["routines"][0]["externalReleasePending"], false);
}

#[test]
fn duplicacao_direta_preserva_marca_externa_e_exige_nova_liberacao() {
    let f = Fixture::new();
    let original = f.hub.routines.save(&f.hub, &f.bot, &body()).unwrap();
    let id = original["routine"]["id"].as_str().unwrap();
    f.hub
        .routines
        .change_bound(&f.hub.store, Some(&f.bot), |state| {
            let routine = state.routines.iter_mut().find(|routine| routine.id == id).unwrap();
            routine.external_creation = true;
            routine.external_release_pending = None;
            Ok(())
        })
        .unwrap();

    let copy = f.hub.routines.duplicate_in_origin(&f.hub, &f.bot, id, None, || {}).unwrap();
    let copy_id = copy["routine"]["id"].as_str().unwrap();
    assert_eq!(copy["routine"]["enabled"], false);
    let persisted: Value = serde_json::from_str(&f.persisted()).unwrap();
    let stored = persisted["routines"].as_array().unwrap().iter().find(|routine| routine["id"] == copy_id).unwrap();
    assert_eq!(stored["externalCreation"], true);
    assert_eq!(stored["externalReleasePending"], "Pending");
    assert_eq!(copy["routine"]["externalReleasePending"], true);
    let listed = f.hub.routines.list(&f.bot);
    let listed_copy = listed["routines"].as_array().unwrap().iter().find(|routine| routine["id"] == copy_id).unwrap();
    assert_eq!(listed_copy["externalReleasePending"], true);
}

#[tokio::test]
async fn pending_save_cannot_recreate_routine_or_hook_after_hub_deletion() {
    let f = Fixture::new();
    // O fluxo normal continua aceito antes da exclusão, com hook real persistido.
    f.hub.routines.save(&f.hub, &f.bot, &body()).unwrap();
    assert_eq!(f.hub.routines.hooks().len(), 1);
    let (checked_tx, checked_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let saving = f.hub.clone();
    let bot = f.bot.clone();
    let pending = std::thread::spawn(move || {
        // É o mesmo caminho usado por Routines::save; a seam só fixa a janela.
        saving.routines.save_bound(&saving, &bot, &body(), || {
            checked_tx.send(()).unwrap();
            resume_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        })
    });
    checked_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    // O worker já validou o bot ativo, mas ainda não tomou state.
    f.hub.delete_bot(&f.bot).unwrap();
    assert!(f.hub.store.bot(&f.bot).unwrap().unwrap().deleted);
    let committed = f.persisted();
    let state = f.hub.routines.list(&f.bot);
    let hooks = f.hub.routines.hooks();
    let hook_updates = f.hub.routines.subscribe_hooks();
    let mut events = f.hub.events.subscribe();
    resume_tx.send(()).unwrap();
    let error = pending.join().unwrap().unwrap_err();
    assert_eq!(error.to_string(), "routine requires an existing agent bot");
    assert_eq!(f.persisted(), committed);
    assert_eq!(f.hub.routines.list(&f.bot), state);
    assert_eq!(serde_json::to_value(f.hub.routines.hooks()).unwrap(), serde_json::to_value(hooks).unwrap());
    assert!(!hook_updates.has_changed().unwrap());
    assert!(events.try_recv().is_err());
    assert!(f.hub.routines.hooks().is_empty());
    let restored = Routines::default();
    restored.restore(&f.hub.store).unwrap();
    assert_eq!(restored.list(&f.bot), state);
}

#[test]
fn direct_store_deletion_after_state_reservation_is_rechecked_in_write_transaction() {
    let f = Fixture::new();
    let before = f.persisted();
    let state = f.hub.routines.list(&f.bot);
    let hook_updates = f.hub.routines.subscribe_hooks();
    let error = f
        .hub
        .routines
        .change_bound(&f.hub.store, Some(&f.bot), |candidate| {
            // O candidato foi montado sob state; uma exclusão direta não usa essa trava.
            save_in(candidate, &f.bot, &body())?;
            // Esta closure só prepara State; ainda não há DB/Tx do save adquirido.
            f.hub.store.delete_bot(&f.bot)?;
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.to_string(), "routine requires an existing agent bot");
    assert!(f.hub.store.bot(&f.bot).unwrap().unwrap().deleted);
    assert_eq!(f.persisted(), before);
    assert_eq!(f.hub.routines.list(&f.bot), state);
    assert!(f.hub.routines.hooks().is_empty());
    assert!(!hook_updates.has_changed().unwrap());
}

#[test]
fn disparo_mcp_idempotente_conserva_recibo_original_e_replay_revogado_nao_ganha_autoridade() {
    use crate::{fork::sessoes::bindings, routines::tool_origin::ToolOrigin};
    let f = Fixture::new();
    let saved = f.hub.routines.save(&f.hub, &f.bot, &body()).unwrap();
    let id = saved["routine"]["id"].as_str().unwrap();
    let binding = bindings::issue(&f.hub.store, &f.bot, &f.bot, &f.bot, None).unwrap();
    bindings::activate(&f.hub.store, &binding, "acp-manual").unwrap();
    let entry =
        f.hub.store.insert_entry(&Lane::main(&f.bot), EntryKind::User, 1, &json!({"text":"Teste solicitado"})).unwrap();
    f.hub
        .start_bot_turn(&f.bot, |runtime| {
            runtime.lane = Some(Lane::main(&f.bot));
            runtime.session = Some("acp-manual".into());
            runtime.message = Some(entry.id);
        })
        .unwrap();
    let execution = bindings::capture(&f.hub, &f.bot, binding.id.as_str()).unwrap();
    let origin = ToolOrigin::capture(&f.hub, &f.bot, &execution, true).unwrap();
    let before = f.persisted();
    assert!(f.hub.routines.duplicate_direct(&f.hub, &f.bot, id, Some(&origin)).is_err());
    assert_eq!(f.persisted(), before);
    let enqueue = || {
        f.hub.routines.enqueue_in_origin(
            &f.hub.store,
            &f.bot,
            id,
            json!({"source":"test"}),
            Some("entrega-original".into()),
            true,
            Some(&origin),
        )
    };
    let first = enqueue().unwrap();
    assert_eq!(enqueue().unwrap(), first);
    let counts = || {
        let db = f.hub.store.db.locked();
        db.query_row(
            "SELECT COUNT(*),SUM(used),SUM(linked),(SELECT COUNT(*) FROM donna_chain_admissions) FROM donna_chain_roots",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?)),
        )
        .unwrap()
    };
    assert_eq!(counts(), (1, 1, 1, 1));
    assert_eq!(f.hub.routines.list(&f.bot)["runs"].as_array().unwrap().len(), 1);
    assert!(first["run"].get("_chain").is_none());
    let before = f.persisted();
    bindings::revoke(&f.hub.store, &f.bot, Some("acp-manual")).unwrap();
    assert!(enqueue().unwrap_err().to_string().contains("chamada vencida"));
    assert_eq!(f.persisted(), before);
    assert_eq!(counts(), (1, 1, 1, 1), "replay usa o mesmo recibo, sem nova tentativa nem autoridade");
}

#[test]
fn duplicate_via_tool_origin_creates_disabled_copy_with_fresh_secret() {
    use crate::fork::sessoes::bindings;
    let f = Fixture::new();
    let saved = f.hub.routines.save(&f.hub, &f.bot, &body()).unwrap();
    let id = saved["routine"]["id"].as_str().unwrap().to_owned();
    let binding = bindings::issue(&f.hub.store, &f.bot, &f.bot, &f.bot, None).unwrap();
    bindings::activate(&f.hub.store, &binding, "acp-duplicate").unwrap();
    let entry = f
        .hub
        .store
        .insert_entry(&Lane::main(&f.bot), EntryKind::User, 1, &json!({"text":"Duplique a rotina"}))
        .unwrap();
    f.hub
        .start_bot_turn(&f.bot, |runtime| {
            runtime.lane = Some(Lane::main(&f.bot));
            runtime.session = Some("acp-duplicate".into());
            runtime.message = Some(entry.id);
        })
        .unwrap();
    let execution = bindings::capture(&f.hub, &f.bot, binding.id.as_str()).unwrap();
    let copy =
        super::super::call_in_execution(&f.hub, &f.bot, "duplicate_routine", &json!({"id": id}), &execution).unwrap();
    let copy_id = copy["routine"]["id"].as_str().unwrap();
    assert_ne!(copy_id, id.as_str());
    // A resposta da criação traz só identidade e estado; o resto vem da leitura.
    let stored = f.hub.routines.list(&f.bot)["routines"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == copy_id)
        .unwrap()
        .clone();
    assert_eq!(stored["name"], "Rotina fixture (cópia)");
    assert_eq!(stored["enabled"], false);
    assert_eq!(stored["triggers"], saved["routine"]["triggers"]);
    let hooks = f.hub.routines.hooks();
    assert_eq!(hooks.len(), 2);
    let old_key = hooks.iter().find(|h| h.id == id).unwrap().key.clone();
    let new_key = hooks.iter().find(|h| h.id == copy_id).unwrap().key.clone();
    assert_ne!(new_key, old_key, "o segredo novo não reaproveita o original");
    let listed = f.hub.routines.list(&f.bot)["routines"].clone();
    assert_eq!(listed.as_array().unwrap().len(), 2);
    assert!(listed.as_array().unwrap().iter().any(|r| r["id"] == id && r["enabled"] == true));
    assert!(
        super::super::call_in_execution(&f.hub, &f.bot, "duplicate_routine", &json!({"id": "sumida"}), &execution)
            .is_err()
    );
    // Direto, sem origem MCP: mesma cópia desligada, sem gastar cota.
    let direct = f.hub.routines.duplicate_in_origin(&f.hub, &f.bot, &id, None, || {}).unwrap();
    assert_eq!(direct["routine"]["name"], "Rotina fixture (cópia)");
    assert_eq!(direct["routine"]["enabled"], false);
    assert_ne!(direct["routine"]["id"].as_str().unwrap(), id.as_str());
}

/// D538/D544: chamadas MCP reais, banco em memória e nenhum motor externo.
#[tokio::test]
async fn d544_rotinas_proprias_sem_plugin_e_isoladas_por_space() {
    use crate::{
        api::{devices::Caller, dispatch},
        fork::{sessoes::bindings, spaces::PERSONAL},
    };
    let f = Fixture::new();
    f.hub.store.set_team_settings(&json!({"primaryBotId":f.bot})).unwrap();
    let initial = f.hub.store.spaces().unwrap();
    let created = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaSpaceCreate",
        json!({
            "hubId":f.hub.host_id,"expectedRevision":initial.revision,"name":"Trabalho",
            "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}}
        }),
    )
    .await
    .unwrap();
    let work =
        created["spaces"].as_array().unwrap().iter().find(|s| s["id"] != PERSONAL).unwrap()["id"].as_str().unwrap();
    let other = uuid::Uuid::new_v4().to_string();
    let cfg: BotConfig =
        serde_json::from_value(json!({"id":other,"name":"Trabalho","backend":"fixture","spaceId":work,"cwd":f.dir}))
            .unwrap();
    f.hub.store.save_bot(&cfg).unwrap();
    let execution_for = |bot: &str, session: &str| {
        let binding = bindings::issue(&f.hub.store, bot, bot, bot, None).unwrap();
        bindings::activate(&f.hub.store, &binding, session).unwrap();
        let entry = f
            .hub
            .store
            .insert_entry(&Lane::main(bot), EntryKind::User, 1, &json!({"text":"Crie minhas tarefas"}))
            .unwrap();
        f.hub
            .start_bot_turn(bot, |r| {
                r.lane = Some(Lane::main(bot));
                r.session = Some(session.into());
                r.message = Some(entry.id);
            })
            .unwrap();
        bindings::capture(&f.hub, bot, binding.id.as_str()).unwrap()
    };
    let other_execution = execution_for(&other, "acp-work-d544");
    let foreign = super::super::call_in_execution(&f.hub, &other, "save_routine", &body(), &other_execution).unwrap();
    let foreign_id = foreign["routine"]["id"].as_str().unwrap();
    let execution = execution_for(&f.bot, "acp-d544");
    let call = |name: &str, args: Value| super::super::call_in_execution(&f.hub, &f.bot, name, &args, &execution);
    let count: i64 =
        f.hub.store.db.locked().query_row("SELECT COUNT(*) FROM donna_plugin_installations", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 0, "nasce disponível, sem instalação nem concessão");
    let once = call("save_routine", json!({"botId":other,"spaceId":work,"name":"Recordação","instruction":"Lembrar o dono","triggers":[{"type":"once","at":crate::store::now_ms()+86_400_000}]})).unwrap();
    assert_eq!(once["routine"]["botId"], f.bot, "campos do modelo não escolhem o destino");
    let once_id = once["routine"]["id"].as_str().unwrap();
    // Mesmo com configuração legada desligada, o agendamento continua disponível.
    f.hub.store.db.locked().execute("INSERT INTO donna_plugin_installations(id,version,hub_id,builtin,state,generation,revision) VALUES(?1,1,?2,'scatty','disabled',1,1)", rusqlite::params![uuid::Uuid::new_v4().to_string(),f.hub.host_id]).unwrap();
    let recurring = call("save_routine", json!({"name":"Revisão","instruction":"Revisar minhas tarefas","triggers":[{"type":"cron","expression":"0 9 * * 1","timeZone":"America/Sao_Paulo"}]})).unwrap();
    assert_eq!(recurring["routine"]["botId"], f.bot);
    assert_eq!(call("list_routines", json!({})).unwrap()["routines"].as_array().unwrap().len(), 2);
    for (name, mut args) in [
        ("save_routine", body()),
        ("delete_routine", json!({})),
        ("set_routine_enabled", json!({"enabled":false})),
        ("run_routine", json!({})),
        ("routine_webhook", json!({"rotate":true})),
    ] {
        args["id"] = json!(foreign_id);
        args["spaceId"] = json!(work);
        args["botId"] = json!(other);
        assert!(call(name, args).is_err(), "{name}: ID/campos alheios não mudam o bot executor");
    }
    let stored = &f.hub.routines.list(&other)["routines"][0];
    assert_eq!(stored["id"], foreign["routine"]["id"]);
    assert_eq!(stored["botId"], foreign["routine"]["botId"]);
    assert_eq!(stored["enabled"], foreign["routine"]["enabled"]);
    assert!(foreign["routine"].get("instruction").is_none());
    for (space, expected) in [(PERSONAL, 2), (work, 1)] {
        let view = f.hub.routines.overview(&f.hub.store, &json!({"spaceId":space,"from":crate::store::now_ms(),"to":crate::store::now_ms()+86_400_000,"timeZone":"UTC"})).unwrap();
        assert_eq!(view["routines"].as_array().unwrap().len(), expected);
        assert!(
            view["routines"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["botId"] == if space == PERSONAL { f.bot.as_str() } else { other.as_str() })
        );
    }
    call("set_routine_enabled", json!({"id":once_id,"enabled":false})).unwrap();
    call("delete_routine", json!({"id":once_id})).unwrap();
    let before = f.persisted();
    bindings::revoke(&f.hub.store, &f.bot, Some("acp-d544")).unwrap();
    assert!(call("save_routine", body()).is_err());
    assert_eq!(f.persisted(), before, "sessão revogada não grava");
}
