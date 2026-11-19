// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
use super::*;
use crate::LockExt;
use crate::api::devices::Caller;
use crate::hub::BotStatus;
use crate::remote::identity::Identity;
use crate::store::{EntryKind, Lane, Store};

async fn dispatch(hub: &Arc<Hub>, caller: &Caller, method: &str, mut body: Value) -> Result<Value> {
    if method == "routineCall" {
        let bot = body["botId"].as_str().unwrap();
        let binding = crate::fork::sessoes::bindings::fixture_binding(&hub.store, bot);
        hub.set_runtime(bot, |r| {
            r.session = Some("fixture-acp".into());
            r.memory_turn_token = Some("fixture-turn".into());
        });
        body["sessionBinding"] = json!(binding.id.as_str());
    }
    crate::api::dispatch(hub, caller, method, body).await
}

fn typed_request(text: &str) -> Value {
    let mut data = json!({"text":text});
    let body = json!({"contentOrigin":{"schema":1,"draftId":uuid::Uuid::new_v4().to_string(),
        "revision":1,"textLength":text.encode_utf16().count(),
        "deviceId":uuid::Uuid::new_v4().to_string(),"channel":"desktopComposer","interaction":"send",
        "segments":[{"start":0,"end":text.encode_utf16().count(),"kind":"typed"}]}});
    data["contentOrigin"] = crate::guard_hub::origem::capture(&Caller::Local, &body, &data);
    data
}

struct Fixture {
    hub: Arc<Hub>,
    dir: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        Self::in_dir(std::env::temp_dir().join(format!("donnabot-manage-{}", uuid::Uuid::new_v4())))
    }
    fn in_dir(dir: std::path::PathBuf) -> Self {
        std::fs::create_dir_all(dir.join("lead")).unwrap();
        std::fs::create_dir_all(dir.join("workspaces/scout")).unwrap();
        let store = Store::open(&dir.join("test.db")).unwrap();
        store
            .kv_set(
                "team.settings.v1",
                &json!({"primaryBotId":"lead","permission":"auto","workspaceRoot":dir.join("workspaces")}).to_string(),
            )
            .unwrap();
        for (id, manage) in [("lead", true), ("dev", false)] {
            let cfg: BotConfig = serde_json::from_value(json!({"id": id, "name": id, "backend": "claude",
                "cwd": dir.join("lead"), "manageTeam": manage, "connectors": ["github"], "skills": ["review"]}))
            .unwrap();
            store.save_bot(&cfg).unwrap();
        }
        let hub = Hub::new(store, "manage-test".into(), Identity::load_or_create(&dir).unwrap(), "test".into(), 0);
        hub.start().unwrap();
        let e = hub
            .store
            .insert_entry(&Lane::main("lead"), EntryKind::User, 1, &typed_request("Gerencie meu time"))
            .unwrap();
        hub.set_runtime("lead", |r| {
            r.lane = Some(Lane::main("lead"));
            r.status = BotStatus::Working;
            r.message = Some(e.id);
        });
        Self { hub, dir }
    }
    async fn call(&self, tool: &str, args: Value) -> Result<Value> {
        call(&self.hub, "lead", tool, &args).await
    }
    fn changes(&self) -> Vec<crate::store::Entry> {
        self.hub
            .store
            .history("lead", i64::MAX, 100)
            .unwrap()
            .into_iter()
            .filter(|e| e.data["teamChange"].is_object())
            .collect()
    }
    async fn create(&self) -> Value {
        self.call("create_bot", json!({"name":"Scout", "folder":self.dir.join("workspaces/scout")})).await.unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn gestao_auto_nao_pode_liberar_rotina_externa_de_outro_bot() {
    let f = Fixture::new();
    let mut routine = json!({"name":"Externa", "instruction":"Fixture", "triggers":[{"type":"webhook"}]});
    let saved = f.hub.routines.save(&f.hub, "dev", &routine).unwrap();
    let id = saved["routine"]["id"].as_str().unwrap();
    let mut state: Value = serde_json::from_str(&f.hub.store.kv_read("routines.v1").unwrap().unwrap()).unwrap();
    let stored = state["routines"].as_array_mut().unwrap().iter_mut().find(|r| r["id"] == id).unwrap();
    stored["enabled"] = json!(false);
    stored["externalCreation"] = json!(true);
    stored["externalReleasePending"] = json!("Pending");
    f.hub.store.kv_set("routines.v1", &state.to_string()).unwrap();
    f.hub.routines.restore(&f.hub.store).unwrap();
    let before = f.hub.store.kv_read("routines.v1").unwrap();
    routine["id"] = json!(id);
    routine["enabled"] = json!(true);
    assert!(f.call("update_bot", json!({"botId":"dev", "name":"Changed", "routines":[routine]})).await.is_err());
    assert_eq!(f.hub.store.kv_read("routines.v1").unwrap(), before);
    assert_eq!(agent_bot(&f.hub, "dev").unwrap().name, "dev");
    assert_eq!(f.hub.routines.list("dev")["routines"][0]["externalReleasePending"], true);
}

#[tokio::test]
async fn only_the_configured_primary_can_manage_even_with_a_legacy_flag() {
    let f = Fixture::new();
    f.hub.store.sem_donna_fixa(); // D366: exercita a troca/exclusão legada, sem Donna fixa.
    // Simula um banco upstream, anterior à restrição de save_bot.
    f.hub.store.kv_set("team.settings.v1", r#"{"primaryBotId":"dev","permission":"auto"}"#).unwrap();
    assert!(f.call("list_team", json!({})).await.is_err());
}

#[tokio::test]
async fn user_api_is_the_only_way_to_assign_the_primary_and_capability() {
    let f = Fixture::new();
    f.hub.store.sem_donna_fixa(); // D366: exercita a troca/exclusão legada, sem Donna fixa.
    let saved = f.hub.update_bot(&json!({"id":"dev", "manageTeam":true, "name":"Renamed"})).unwrap();
    assert_eq!(saved["manageTeam"], false);
    assert_eq!(saved["name"], "Renamed");
    assert!(call(&f.hub, "dev", "list_team", &json!({})).await.is_err());
    dispatch(&f.hub, &Caller::Local, "setTeamSettings", json!({"primaryBotId":"dev", "permission":"auto"}))
        .await
        .unwrap();
    assert!(!agent_bot(&f.hub, "lead").unwrap().manage_team);
    assert!(agent_bot(&f.hub, "dev").unwrap().manage_team);
    let remote = Caller::Device { key: "phone".into(), scopes: vec![crate::store::Scope::Control], autoridade: None };
    assert!(dispatch(&f.hub, &remote, "setTeamSettings", json!({"primaryBotId":"lead"})).await.is_err());
}

#[tokio::test]
async fn new_bots_inherit_skills_without_connectors_accounts_or_management() {
    let f = Fixture::new();
    let mut source = agent_bot(&f.hub, "lead").unwrap();
    *f.hub.store.secret_key.lock().unwrap() = Some(zeroize::Zeroizing::new(vec![42; 32]));
    let custom =
        serde_json::from_value(json!({"id":"exemplo-mcp","name":"Exemplo MCP","url":"https://example.test/mcp"}))
            .unwrap();
    crate::market::save_connectors(&f.hub.store, &[custom]).unwrap();
    source.connectors.extend(["exemplo-mcp".into(), "ferramenta-mail".into()]);
    source.logins = vec!["mail-account".into()];
    source.backend = "creator-only".into();
    source.browser_preference = crate::fork::browser::BrowserPreference::Computer;
    f.hub.store.save_bot_as_owner(&source).unwrap();
    let made = f.create().await;
    let cfg = agent_bot(&f.hub, made["bot"]["id"].as_str().unwrap()).unwrap();
    assert!(!cfg.manage_team);
    assert_eq!(cfg.browser_preference, crate::fork::browser::BrowserPreference::Integrated);
    assert_ne!(cfg.backend, "creator-only", "Donna does not copy the creator's engine");
    assert_eq!(cfg.model, None);
    assert_eq!(cfg.created_by.as_deref(), Some("lead"));
    assert!(!cfg.permission_chosen_by_user);
    assert_eq!(cfg.connectors, Vec::<String>::new());
    assert_eq!(cfg.logins, Vec::<String>::new());
    assert_eq!(agent_bot(&f.hub, "lead").unwrap().connectors, source.connectors);
    assert_eq!(agent_bot(&f.hub, "lead").unwrap().logins, source.logins);
    assert_eq!(cfg.skills, ["review"]);
    assert!(call(&f.hub, &cfg.id, "list_team", &json!({})).await.is_err());
}

#[tokio::test]
async fn bots_cannot_change_the_primary_or_choose_models_harnesses_or_powers() {
    let f = Fixture::new();
    assert!(f.call("update_bot", json!({"botId":"lead", "name":"Other"})).await.is_err());
    assert!(
        crate::chat::profile::call(&f.hub, "lead", "set_instructions", &json!({"instructions":"other"})).await.is_err()
    );
    for field in [
        "model",
        "modelOptions",
        "backend",
        "agent",
        "harness",
        "command",
        "fastMode",
        "manageTeam",
        "permission",
        "permissionChosenByUser",
        "connectors",
        "logins",
        "browserPreference",
    ] {
        for tool in ["create_bot", "update_bot"] {
            let mut args = if tool == "create_bot" {
                json!({"name":"Other", "folder":f.dir.join("workspaces/scout")})
            } else {
                json!({"botId":"dev", "name":"Other"})
            };
            args[field] = if field == "browserPreference" { json!("computer") } else { json!(null) };
            // Nem a escolha explícita de PC nem null escapam da allowlist.
            let error = f.call(tool, args).await.unwrap_err().to_string();
            assert!(error.contains(&format!("field {field} is not allowed")), "{tool}: {error}");
        }
    }
    assert_eq!(agent_bot(&f.hub, "lead").unwrap().name, "lead");
    assert_eq!(agent_bot(&f.hub, "dev").unwrap().name, "dev");
    assert_eq!(f.hub.store.bots().unwrap().len(), 2);
}

#[tokio::test]
async fn autonomy_is_clamped_by_creator_and_global_and_recalculated() {
    let f = Fixture::new();
    let made = f.create().await;
    let id = made["bot"]["id"].as_str().unwrap();
    dispatch(&f.hub, &Caller::Local, "setTeamSettings", json!({"permission":"ask"})).await.unwrap();
    assert_eq!(agent_bot(&f.hub, id).unwrap().permission, Permission::Ask);
    dispatch(&f.hub, &Caller::Local, "setTeamSettings", json!({"permission":"auto"})).await.unwrap();
    assert_eq!(agent_bot(&f.hub, id).unwrap().permission, Permission::Auto);
    f.hub.update_bot_by_user(&json!({"id":"lead", "permission":"ask", "permissionChosenByUser":true})).unwrap();
    dispatch(&f.hub, &Caller::Local, "setTeamSettings", json!({"permission":"auto"})).await.unwrap();
    assert_eq!(agent_bot(&f.hub, id).unwrap().permission, Permission::Ask);
    let source = agent_bot(&f.hub, "lead").unwrap();
    let mut source_ask = source.clone();
    source_ask.permission = Permission::Ask;
    // Ask não persiste o bot sem receber aprovação síncrona.
    assert!(
        create(&f.hub, &source_ask, &json!({"name":"Ask", "folder":f.dir.join("workspaces/scout")}), None)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn explicit_auto_for_the_manager_skips_cards_but_keeps_children_restricted() {
    let f = Fixture::new();
    dispatch(&f.hub, &Caller::Local, "setTeamSettings", json!({"permission":"ask"})).await.unwrap();
    dispatch(
        &f.hub,
        &Caller::Local,
        "updateBot",
        json!({"id":"lead", "permission":"auto", "permissionChosenByUser":true}),
    )
    .await
    .unwrap();
    let made = f.create().await;
    let id = made["bot"]["id"].as_str().unwrap();
    let cfg = agent_bot(&f.hub, id).unwrap();
    assert_eq!(cfg.permission, Permission::Ask);
    assert!(!cfg.permission_chosen_by_user);
    f.call("update_bot", json!({"botId":id, "name":"Changed"})).await.unwrap();
    assert_eq!(f.changes().len(), 2, "ambas as ações em Liberar oferecem Desfazer");
    assert!(!f.hub.store.history("lead", i64::MAX, 100).unwrap().iter().any(|e| e.kind == "permission"));
}

#[tokio::test]
async fn user_autonomy_overrides_general_and_survives_recalculation_undo_and_reopening() {
    let f = Fixture::new();
    let made = f.create().await;
    let id = made["bot"]["id"].as_str().unwrap();
    // Cliente antigo recebe erro claro, sem salvar parcialmente o patch.
    let error = dispatch(&f.hub, &Caller::Local, "updateBot", json!({"id":id, "permission":"auto", "pinned":true}))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("Atualize o app para mudar a autonomia"), "{error}");
    assert!(!agent_bot(&f.hub, id).unwrap().pinned);
    assert!(!agent_bot(&f.hub, id).unwrap().permission_chosen_by_user);
    dispatch(&f.hub, &Caller::Local, "setTeamSettings", json!({"permission":"ask"})).await.unwrap();
    let error = dispatch(&f.hub, &Caller::Local, "updateBot", json!({"id":id, "permission":"auto", "pinned":true}))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("Atualize o app para mudar a autonomia"), "{error}");
    assert!(!agent_bot(&f.hub, id).unwrap().pinned);
    assert_eq!(agent_bot(&f.hub, id).unwrap().permission, Permission::Ask, "perfil atrasado não libera autonomia");
    let error = f.call("update_bot", json!({"botId":id, "permission":"auto"})).await.unwrap_err().to_string();
    assert!(error.contains("field permission is not allowed"), "{error}");
    assert_eq!(agent_bot(&f.hub, id).unwrap().permission, Permission::Ask);
    assert!(!agent_bot(&f.hub, id).unwrap().permission_chosen_by_user);
    dispatch(&f.hub, &Caller::Local, "updateBot", json!({"id":id, "permission":"ask", "permissionChosenByUser":true}))
        .await
        .unwrap();
    dispatch(&f.hub, &Caller::Local, "setTeamSettings", json!({"permission":"auto"})).await.unwrap();
    assert_eq!(agent_bot(&f.hub, id).unwrap().permission, Permission::Ask, "Pedir igual ao herdado é uma escolha");
    dispatch(&f.hub, &Caller::Local, "setTeamSettings", json!({"permission":"ask"})).await.unwrap();

    dispatch(&f.hub, &Caller::Local, "updateBot", json!({"id":id, "permission":"auto", "permissionChosenByUser":true}))
        .await
        .unwrap();
    assert_eq!(agent_bot(&f.hub, id).unwrap().permission, Permission::Auto);
    assert!(agent_bot(&f.hub, id).unwrap().permission_chosen_by_user);
    // Um perfil antigo sem intenção explícita não troca nem apaga a escolha.
    dispatch(&f.hub, &Caller::Local, "updateBot", json!({"id":id, "permission":"ask", "permissionChosenByUser":false}))
        .await
        .unwrap_err();
    assert_eq!(agent_bot(&f.hub, id).unwrap().permission, Permission::Auto);
    assert!(agent_bot(&f.hub, id).unwrap().permission_chosen_by_user);
    for general in ["auto", "ask", "auto"] {
        dispatch(&f.hub, &Caller::Local, "setTeamSettings", json!({"permission":general})).await.unwrap();
        assert_eq!(agent_bot(&f.hub, id).unwrap().permission, Permission::Auto);
    }

    // O Berga ainda pode editar o perfil; Desfazer não reaplica o teto à escolha do usuário.
    f.call("update_bot", json!({"botId":id, "name":"Changed"})).await.unwrap();
    let entry = f.changes().into_iter().find(|e| e.data["text"] == "Bot Scout atualizado").unwrap();
    dispatch(&f.hub, &Caller::Local, "setTeamSettings", json!({"permission":"ask"})).await.unwrap();
    undo(&f.hub, &entry.id).await.unwrap();
    assert_eq!(agent_bot(&f.hub, id).unwrap().name, "Scout");
    let reopened = Store::open(&f.dir.join("test.db")).unwrap();
    let cfg = reopened.bot(id).unwrap().unwrap().config;
    assert_eq!(cfg.permission, Permission::Auto);
    assert!(cfg.permission_chosen_by_user);

    // Pedir explícito também não é elevado quando a geral volta para Liberar.
    dispatch(&f.hub, &Caller::Local, "updateBot", json!({"id":id, "permission":"ask", "permissionChosenByUser":true}))
        .await
        .unwrap();
    dispatch(&f.hub, &Caller::Local, "setTeamSettings", json!({"permission":"auto"})).await.unwrap();
    assert_eq!(agent_bot(&f.hub, id).unwrap().permission, Permission::Ask);
}

#[tokio::test]
async fn auto_notices_are_in_primary_chat_and_updates_can_be_undone_once() {
    let f = Fixture::new();
    f.hub.set_runtime("lead", |r| r.lane = Some(Lane::main("dev")));
    assert!(f.call("update_bot", json!({"botId":"dev", "name":"Não alterar"})).await.is_err());
    assert!(f.changes().is_empty());
    f.hub.set_runtime("lead", |r| r.lane = Some(Lane::main("lead")));
    f.call("update_bot", json!({"botId":"dev", "name":"Changed", "instructions":"new"})).await.unwrap();
    assert_eq!(f.changes().len(), 1);
    assert!(!f.hub.store.history("lead", i64::MAX, 100).unwrap().iter().any(|e| e.kind == "permission"));
    let entry = f.changes().remove(0);
    assert_eq!(entry.data["teamChange"]["status"], "available");
    dispatch(&f.hub, &Caller::Local, "undoTeamChange", json!({"entryId":entry.id})).await.unwrap();
    let cfg = agent_bot(&f.hub, "dev").unwrap();
    assert_eq!(cfg.name, "dev");
    assert_eq!(cfg.description, "");
    assert_eq!(f.hub.store.entry(&entry.id).unwrap().data["teamChange"]["status"], "undone");
    assert!(dispatch(&f.hub, &Caller::Local, "undoTeamChange", json!({"entryId":entry.id})).await.is_err());
}

#[tokio::test]
async fn creation_undo_deletes_only_without_conversations_and_update_undo_preserves_later_edits() {
    let f = Fixture::new();
    let made = f.create().await;
    let id = made["bot"]["id"].as_str().unwrap();
    let entry = f.changes().remove(0);
    dispatch(&f.hub, &Caller::Local, "undoTeamChange", json!({"entryId":entry.id})).await.unwrap();
    assert!(f.hub.store.bot(id).unwrap().unwrap().deleted);
    let made = f.create().await;
    let id = made["bot"]["id"].as_str().unwrap();
    let entry = f.changes().into_iter().find(|e| e.data["teamChange"]["status"] == "available").unwrap();
    f.hub.store.insert_entry(&Lane::main(id), EntryKind::User, 1, &json!({"text":"oi"})).unwrap();
    assert!(dispatch(&f.hub, &Caller::Local, "undoTeamChange", json!({"entryId":entry.id})).await.is_err());
    f.call("update_bot", json!({"botId":"dev", "name":"Changed"})).await.unwrap();
    let entry = f.changes().into_iter().find(|e| e.data["teamChange"]["targetBotId"] == "dev").unwrap();
    f.hub.update_bot(&json!({"id":"dev", "description":"user edit"})).unwrap();
    assert!(dispatch(&f.hub, &Caller::Local, "undoTeamChange", json!({"entryId":entry.id})).await.is_err());
    assert_eq!(agent_bot(&f.hub, "dev").unwrap().description, "user edit");
}

#[tokio::test]
async fn turns_without_a_user_origin_cannot_mutate() {
    let f = Fixture::new();
    f.hub.set_runtime("lead", |r| r.message = None);
    for (tool, args) in [
        ("create_bot", json!({"name":"x", "folder":f.dir.join("workspaces/scout")})),
        ("update_bot", json!({"botId":"dev", "name":"x"})),
    ] {
        let error = f.call(tool, args).await.unwrap_err().to_string();
        assert!(error.contains("precisa de um pedido do dono"), "{error}");
    }
    assert!(f.call("list_team", json!({})).await.is_ok());
    assert_eq!(f.hub.store.bots().unwrap().len(), 2);
    assert_eq!(agent_bot(&f.hub, "dev").unwrap().name, "dev");
}

#[test]
fn a_card_lists_the_routines_before_the_instructions() {
    let fields = setup_patch(&json!({"name": "Scout", "instructions": "Watches the repo."})).unwrap();
    assert_eq!(
        describe(&fields, &["• Scan (Every 30 minutes): Scan it".into()]),
        "Name: Scout\nRoutines:\n• Scan (Every 30 minutes): Scan it\nInstructions:\nWatches the repo."
    );
}

#[tokio::test]
async fn ask_refuses_unanswered_and_oversized_changes_without_writing() {
    let f = Fixture::new();
    f.hub.update_bot_by_user(&json!({"id":"lead", "permission":"ask", "permissionChosenByUser":true})).unwrap();
    let long = f
        .call("create_bot", json!({"name":"x", "folder":f.dir.join("workspaces/scout"), "instructions":"a".repeat(MAX_INSTRUCTIONS_CHARS)}))
        .await
        .unwrap_err();
    assert!(long.to_string().contains("too long for its approval card"));
    // O ator não tem um turno real neste teste: descarta o waiter de aprovação, que recusa a mudança.
    let denied = f.call("update_bot", json!({"botId":"dev", "name":"Denied"})).await.unwrap_err();
    assert!(denied.to_string().contains("didn't allow"));
    assert_eq!(agent_bot(&f.hub, "dev").unwrap().name, "dev");
    assert_eq!(f.hub.store.bots().unwrap().len(), 2);
    assert!(f.changes().is_empty());
}

#[tokio::test]
async fn undo_restores_routines_and_works_after_reopening_the_store() {
    let f = Fixture::new();
    let old = json!({"name":"Scan", "instruction":"old", "triggers":[{"type":"interval", "seconds":1800}]});
    let id = f.hub.routines.save(&f.hub, "dev", &old).unwrap()["routine"]["id"].clone();
    f.call(
        "update_bot",
        json!({"botId":"dev", "name":"Changed", "routines":[
        {"id":id,"name":"Scan", "instruction":"new", "triggers":[{"type":"interval", "seconds":1800}]},
        {"name":"Extra", "instruction":"extra", "triggers":[{"type":"interval", "seconds":3600}]}]}),
    )
    .await
    .unwrap();
    let entry = f.changes().remove(0);
    // Reconstitui um Store/Hub independente com o mesmo SQLite, sem usar estado em memória do anterior.
    f.hub.shutdown().await;
    let store = Store::open(&f.dir.join("test.db")).unwrap();
    let hub = Hub::new(store, "reopened".into(), Identity::load_or_create(&f.dir).unwrap(), "test".into(), 0);
    hub.start().unwrap();
    dispatch(&hub, &Caller::Local, "undoTeamChange", json!({"entryId":entry.id})).await.unwrap();
    assert_eq!(agent_bot(&hub, "dev").unwrap().name, "dev");
    let restored = hub.routines.list("dev");
    assert_eq!(restored["routines"].as_array().unwrap().len(), 1);
    assert_eq!(restored["routines"][0]["id"], id);
    assert_eq!(restored["routines"][0]["instruction"], "old");
    hub.shutdown().await;
}

#[tokio::test]
async fn failed_undo_rolls_back_profile_routines_and_notice_together() {
    let f = Fixture::new();
    f.call(
        "update_bot",
        json!({"botId":"dev", "name":"Changed", "routines":[
        {"name":"Scan", "instruction":"scan", "triggers":[{"type":"interval", "seconds":1800}]}]}),
    )
    .await
    .unwrap();
    let entry = f.changes().remove(0);
    let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER fail_restore BEFORE UPDATE OF v ON kv WHEN NEW.k = 'routines.v1'
        BEGIN SELECT RAISE(ABORT, 'simulated disk failure'); END;",
    )
    .unwrap();
    assert!(dispatch(&f.hub, &Caller::Local, "undoTeamChange", json!({"entryId":entry.id})).await.is_err());
    assert_eq!(agent_bot(&f.hub, "dev").unwrap().name, "Changed");
    assert_eq!(f.hub.routines.list("dev")["routines"].as_array().unwrap().len(), 1);
    assert_eq!(f.hub.store.entry(&entry.id).unwrap().data["teamChange"]["status"], "available");
}

#[test]
fn interrupted_notice_publication_recovers_update_and_creation_after_restart() {
    for create in [false, true] {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let f = runtime.block_on(async {
            let f = Fixture::new();
            let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
            db.execute_batch(
                "CREATE TRIGGER fail_notice BEFORE INSERT ON entries WHEN NEW.bot_id = 'lead' AND NEW.kind = 'notice'
            BEGIN SELECT RAISE(ABORT, 'simulated interrupted publication'); END;",
            )
            .unwrap();
            let (tool, args) = if create {
                ("create_bot", json!({"name":"Scout", "folder":f.dir.join("workspaces/scout")}))
            } else {
                ("update_bot", json!({"botId":"dev", "name":"Changed"}))
            };
            let result = f.call(tool, args).await;
            if create {
                assert!(result.is_err());
            } else {
                let result = result.expect("a saved update must report success even when its notice failed");
                assert_eq!(result["bot"]["name"], "Changed");
                assert!(result["notice"].as_str().unwrap().contains("será publicado na recuperação"));
                assert_eq!(agent_bot(&f.hub, "dev").unwrap().name, "Changed");
                assert_eq!(f.hub.store.pending_team_changes().unwrap().len(), 1);
            }
            db.execute_batch("DROP TRIGGER fail_notice").unwrap();
            f.hub.shutdown().await;
            f
        });
        // Reinício real encerra também os workers de manutenção no pool bloqueante.
        // shutdown() espera os atores, mas não esses jobs; o Drop do runtime os aguarda.
        drop(runtime);
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let store = Store::open(&f.dir.join("test.db")).unwrap();
            recover(&store); // Apenas serve recupera antes de iniciar o Hub.
            let hub = Hub::new(store, "recovered".into(), Identity::load_or_create(&f.dir).unwrap(), "test".into(), 0);
            hub.start().unwrap();
            let entries = hub.store.history("lead", i64::MAX, 100).unwrap();
            if create {
                assert!(entries.iter().all(|e| e.data["teamChange"].is_null()), "failed creation must roll back");
                assert_eq!(hub.store.bots().unwrap().iter().filter(|b| !b.deleted).count(), 2);
                hub.shutdown().await;
                return;
            }
            let entry = entries
                .iter()
                .find(|e| e.data["teamChange"]["status"] == "available")
                .expect("mutation must recover its notice and undo");
            dispatch(&hub, &Caller::Local, "undoTeamChange", json!({"entryId":entry.id})).await.unwrap();
            if create {
                let id = entry.data["teamChange"]["targetBotId"].as_str().unwrap();
                assert!(hub.store.bot(id).unwrap().unwrap().deleted);
            } else {
                assert_eq!(agent_bot(&hub, "dev").unwrap().name, "dev");
            }
            hub.shutdown().await;
        });
    }
}

#[tokio::test]
async fn late_automatic_naming_cannot_rename_a_promoted_primary() {
    let f = Fixture::new();
    f.hub.store.sem_donna_fixa(); // D366: exercita a troca/exclusão legada, sem Donna fixa.
    f.hub.update_bot(&json!({"id":"dev", "autoName":true})).unwrap();
    // O nomeador já terminou a consulta externa, mas ainda não aplicou o resultado.
    dispatch(&f.hub, &Caller::Local, "setTeamSettings", json!({"primaryBotId":"dev"})).await.unwrap();
    crate::chat::naming::apply_name(&f.hub, "dev", "Automatic name").await.unwrap();
    assert_eq!(agent_bot(&f.hub, "dev").unwrap().name, "dev");
}

#[tokio::test]
async fn user_routine_edits_wait_for_management_and_are_never_absorbed_by_undo() {
    let f = Fixture::new();
    f.call("update_bot", json!({"botId":"dev", "name":"Changed"})).await.unwrap();
    let entry = f.changes().remove(0);
    let guard = f.hub.store.team_lock.lock().await;
    let edit = dispatch(
        &f.hub,
        &Caller::Local,
        "saveRoutine",
        json!({"botId":"dev", "name":"User routine",
        "instruction":"keep this", "triggers":[{"type":"interval", "seconds":3600}]}),
    );
    tokio::pin!(edit);
    tokio::select! {
        result = &mut edit => panic!("routine edit escaped management serialization: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(10)) => {}
    }
    assert_eq!(f.hub.routines.list("dev")["routines"], json!([]));
    drop(guard);
    edit.await.unwrap();
    assert!(dispatch(&f.hub, &Caller::Local, "undoTeamChange", json!({"entryId":entry.id})).await.is_err());
    assert_eq!(f.hub.routines.list("dev")["routines"][0]["name"], "User routine");
    assert_eq!(agent_bot(&f.hub, "dev").unwrap().name, "Changed");
}

#[tokio::test]
async fn recovery_never_absorbs_a_concurrent_connector_change_into_undo() {
    let f = Fixture::new();
    let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER fail_notice BEFORE INSERT ON entries WHEN NEW.bot_id = 'lead' AND NEW.kind = 'notice'
        BEGIN SELECT RAISE(ABORT, 'interrupted publication'); END;",
    )
    .unwrap();
    f.call("update_bot", json!({"botId":"dev", "name":"Changed"})).await.unwrap();
    // Como enable_new/complete_connection do marketplace, fora da API updateBot.
    f.hub.update_bot(&json!({"id":"dev", "connectors":["calendar"]})).unwrap();
    db.execute_batch("DROP TRIGGER fail_notice").unwrap();
    recover_live(&f.hub);
    let entry = f.changes().remove(0);
    dispatch(&f.hub, &Caller::Local, "undoTeamChange", json!({"entryId":entry.id})).await.unwrap();
    let cfg = agent_bot(&f.hub, "dev").unwrap();
    assert_eq!(cfg.name, "dev");
    assert_eq!(cfg.connectors, ["calendar"]);
}

#[tokio::test]
async fn canceling_waiter_does_not_release_the_write_or_skip_publication() {
    for creation in [false, true] {
        let f = Fixture::new();
        f.hub.store.sem_donna_fixa(); // D366: promoção legada da principal.
        let source = agent_bot(&f.hub, "lead").unwrap();
        let guard = f.hub.store.team_lock.clone().lock_owned().await;
        let before = (!creation).then(|| agent_bot(&f.hub, "dev").unwrap());
        let pending = undo::prepare(&f.hub, &source, before, vec![], "Mudança em andamento", None).unwrap();
        let hub = f.hub.clone();
        let dir = f.dir.clone();
        let (entered, started) = oneshot::channel();
        let (resume, blocked) = std::sync::mpsc::channel();
        let writer = tokio::spawn(commit_change(hub, guard, pending, move |hub, _pending, _created| {
            entered.send(()).unwrap();
            blocked.recv_timeout(Duration::from_secs(5)).unwrap();
            if creation {
                let cfg = serde_json::from_value(
                    json!({"id":"", "name":"Created", "backend":"codex", "cwd":dir, "createdBy":"lead"}),
                )
                .unwrap();
                hub.create_bot(cfg)
            } else {
                hub.update_bot(&json!({"id":"dev", "name":"Changed"}))
            }
        }));
        started.await.unwrap();
        writer.abort();
        let _ = writer.await;
        let hub = f.hub.clone();
        let mut promotion = tokio::spawn(async move {
            dispatch(&hub, &Caller::Local, "setTeamSettings", json!({"primaryBotId":"dev"})).await
        });
        let waited = tokio::time::timeout(Duration::from_millis(20), &mut promotion).await;
        resume.send(()).unwrap();
        assert!(waited.is_err(), "promotion escaped while the canceled call's writer was still running");
        promotion.await.unwrap().unwrap();
        assert_eq!(f.changes().len(), 1, "the detached writer must finish its durable notice");
        assert_eq!(f.changes()[0].data["teamChange"]["status"], "available");
        if creation {
            assert_eq!(f.hub.store.bots().unwrap().len(), 3);
        } else {
            assert_eq!(agent_bot(&f.hub, "dev").unwrap().name, "Changed");
        }
    }
}

#[tokio::test]
async fn d125_mutacoes_exigem_dono_e_d544_criacao_exige_origem_comprovada() {
    let f = Fixture::new();
    for bot in ["dev", "lead"] {
        let routine = json!({"name":"Scan", "instruction":"original", "triggers":[{"type":"webhook"}]});
        let id = f.hub.routines.save(&f.hub, bot, &routine).unwrap()["routine"]["id"].clone();
        let before = f.hub.store.kv_get("routines.v1");
        for data in [
            None,
            Some(json!({"text":"rotina", "runId":"run"})),
            Some(json!({"text":"webhook", "runId":"hook-run"})),
            Some(json!({"text":"outro bot", "sourceBotId":"other"})),
            Some(json!({"text":"aviso", "from":"host"})),
        ] {
            let message =
                data.map(|data| f.hub.store.insert_entry(&Lane::main(bot), EntryKind::User, 2, &data).unwrap().id);
            f.hub.set_runtime(bot, |r| {
                r.status = BotStatus::Working;
                r.lane = Some(Lane::main(bot));
                r.message = message;
            });
            for (name, args) in [
                ("save_routine", routine.clone()),
                (
                    "save_routine",
                    json!({"id":id,"name":"Scan","instruction":"changed","triggers":[{"type":"webhook"}]}),
                ),
                ("set_routine_enabled", json!({"id":id,"enabled":false})),
                ("delete_routine", json!({"id":id})),
                ("run_routine", json!({"id":id})),
                ("routine_webhook", json!({"id":id,"rotate":true})),
            ] {
                let expected = if name == "save_routine" && args.get("id").is_none() {
                    "conferir a origem da criação"
                } else {
                    "precisa de um pedido do dono"
                };
                let error =
                    dispatch(&f.hub, &Caller::Local, "routineCall", json!({"botId":bot,"name":name,"arguments":args}))
                        .await
                        .unwrap_err();
                assert!(error.to_string().contains(expected), "{bot}/{name}: {error}");
                assert_eq!(f.hub.store.kv_get("routines.v1"), before, "{bot}/{name} alterou a agenda");
            }
            assert!(crate::routines::call(&f.hub, bot, "list_routines", &json!({})).is_ok());
            assert!(crate::routines::call(&f.hub, bot, "routine_webhook", &json!({"id":id})).is_ok());
        }
        let e = f.hub.store.insert_entry(&Lane::main(bot), EntryKind::User, 3, &typed_request("Agende")).unwrap();
        f.hub.set_runtime(bot, |r| r.message = Some(e.id));
        let saved = crate::routines::call(&f.hub, bot, "save_routine", &routine).unwrap();
        let run = crate::routines::call(&f.hub, bot, "run_routine", &json!({"id":saved["routine"]["id"]})).unwrap();
        assert_eq!(run["run"]["status"], "pending");
    }
}

#[tokio::test]
async fn d125_manage_refuses_external_content_even_in_auto() {
    for (kind, data) in [
        (EntryKind::Tool, json!({"status":"completed", "output":"arquivo ou página"})),
        (EntryKind::Tool, json!({"status":"in_progress"})),
        (EntryKind::Tool, json!({"title":"mcp.manage.create_bot","mcpServer":"manage","status":"completed"})),
        (EntryKind::Notice, json!({"sourceBotId":"dev", "text":"resposta do bot"})),
        (EntryKind::User, json!({"text":"resuma", "attachments":[{"id":"synthetic"}]})),
        (EntryKind::User, json!({"text":"resuma", "pasted":["texto externo"]})),
        (EntryKind::User, json!({"text":"resuma", "replyTo":{"id":"synthetic"}})),
        (EntryKind::User, json!({"text":"resuma", "contentOrigin":{"segments":[{"kind":"pasted"}]}})),
        (
            EntryKind::User,
            json!({"text":"resuma", "contentOrigin":{"status":"appReported","segments":[{"kind":"unknown"}]}}),
        ),
    ] {
        let f = Fixture::new();
        let e = f.hub.store.insert_entry(&Lane::main("lead"), kind, 1, &data).unwrap();
        for permission in ["auto", "ask"] {
            f.hub
                .update_bot_by_user(&json!({"id":"lead","permission":permission,"permissionChosenByUser":true}))
                .unwrap();
            for (tool, args) in [
                (
                    "create_bot",
                    json!({"name":"No", "routines":[{"name":"No","instruction":"No","triggers":[{"type":"webhook"}]}]}),
                ),
                (
                    "update_bot",
                    json!({"botId":"dev", "instructions":"No", "routines":[{"name":"No","instruction":"No","triggers":[{"type":"webhook"}]}]}),
                ),
            ] {
                let error = f.call(tool, args).await.unwrap_err().to_string();
                assert!(error.contains("confirmação do dono"), "{permission}/{tool}: {error}");
            }
        }
        assert_eq!(f.hub.store.bots().unwrap().len(), 2);
        assert_eq!(agent_bot(&f.hub, "dev").unwrap().description, "");
        assert_eq!(f.hub.routines.list("dev")["routines"].as_array().unwrap().as_slice(), &[] as &[Value]);
        assert_eq!(std::fs::read_dir(f.dir.join("workspaces")).unwrap().count(), 1);
        assert!(f.changes().is_empty());
        // Apagar a fonte ou começar outro turno não limpa a exposição; nem reiniciar o banco.
        let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
        db.execute("DELETE FROM entries WHERE id=?1", [&e.id]).unwrap();
        let e = f.hub.store.insert_entry(&Lane::main("lead"), EntryKind::User, 3, &typed_request("Altere")).unwrap();
        f.hub.set_runtime("lead", |r| r.message = Some(e.id));
        let reopened = Store::open(&f.dir.join("test.db")).unwrap();
        assert!(reopened.guard_management_exposed("lead", false).unwrap());
        drop(reopened);
        assert!(
            f.call("update_bot", json!({"botId":"dev","name":"No"}))
                .await
                .unwrap_err()
                .to_string()
                .contains("confirmação do dono")
        );
    }
}

#[tokio::test]
async fn d125_manage_honors_gateway_exposure_and_rechecks_before_writing() {
    let f = Fixture::new();
    let source = agent_bot(&f.hub, "lead").unwrap();
    let settings = f.hub.store.team_settings().unwrap();
    let message = user_turn(&f.hub, "lead").unwrap();
    let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
    db.execute("INSERT INTO donna_guard_state(bot,lane,external_exposed) VALUES('lead','*',1)", []).unwrap();
    assert!(
        recheck(&f.hub, &source, &settings, &message, None).unwrap_err().to_string().contains("confirmação do dono")
    );
    assert!(
        f.call("update_bot", json!({"botId":"dev","name":"No"}))
            .await
            .unwrap_err()
            .to_string()
            .contains("confirmação do dono")
    );
    assert_eq!(agent_bot(&f.hub, "dev").unwrap().name, "dev");
    // A exposição de um bot não contamina outro.
    let other = Fixture::new();
    other.create().await;
    other.hub.computers.mark_external("lead");
    assert!(
        other
            .call("update_bot", json!({"botId":"dev","name":"No"}))
            .await
            .unwrap_err()
            .to_string()
            .contains("confirmação do dono")
    );
}

#[tokio::test]
async fn d125_manage_unknown_origin_requires_confirmation_even_in_auto() {
    let f = Fixture::new();
    let e = f.hub.store.insert_entry(&Lane::main("lead"), EntryKind::User, 2, &json!({"text":"Altere"})).unwrap();
    f.hub.set_runtime("lead", |r| r.message = Some(e.id));
    let error = f.call("update_bot", json!({"botId":"dev","name":"No"})).await.unwrap_err().to_string();
    assert!(error.contains("didn't allow"), "sem origem clara, Auto também espera o cartão: {error}");
    assert_eq!(agent_bot(&f.hub, "dev").unwrap().name, "dev");
}

#[tokio::test]
async fn host_authored_user_entries_do_not_authorize_management_or_primary_routines() {
    let f = Fixture::new();
    let routine = json!({"name":"Scan", "instruction":"scan", "triggers":[{"type":"interval", "seconds":1800}]});
    let id = f.hub.routines.save(&f.hub, "lead", &routine).unwrap()["routine"]["id"].clone();
    for data in [
        json!({"text":"[Connection setup completed]", "from":"host"}),
        json!({"text":"other bot", "sourceBotId":"dev"}),
        json!({"text":"routine", "runId":"run"}),
    ] {
        let e = f.hub.store.insert_entry(&Lane::main("lead"), EntryKind::User, 2, &data).unwrap();
        f.hub.set_runtime("lead", |r| r.message = Some(e.id));
        assert!(f.call("create_bot", json!({"name":"No"})).await.is_err());
        assert!(f.call("update_bot", json!({"botId":"dev", "name":"No"})).await.is_err());
        for (name, args) in [
            ("save_routine", routine.clone()),
            ("delete_routine", json!({"id":id})),
            ("set_routine_enabled", json!({"id":id,"enabled":false})),
            ("run_routine", json!({"id":id})),
            ("routine_webhook", json!({"id":id,"rotate":true})),
        ] {
            let expected =
                if name == "save_routine" { "conferir a origem da criação" } else { "precisa de um pedido do dono" };
            let error =
                dispatch(&f.hub, &Caller::Local, "routineCall", json!({"botId":"lead", "name":name, "arguments":args}))
                    .await
                    .unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
        }
        assert!(crate::routines::call(&f.hub, "lead", "list_routines", &json!({})).is_ok());
        assert!(crate::routines::call(&f.hub, "lead", "routine_webhook", &json!({"id":id})).is_ok());
    }
    let e = f.hub.store.insert_entry(&Lane::main("lead"), EntryKind::User, 3, &json!({"text":"Agende isso"})).unwrap();
    f.hub.set_runtime("lead", |r| r.message = Some(e.id));
    // D544: o pedido humano permite à principal agendar seu próprio trabalho, sem plugin.
    let saved = dispatch(
        &f.hub,
        &Caller::Local,
        "routineCall",
        json!({"botId":"lead", "name":"save_routine", "arguments":routine}),
    )
    .await
    .unwrap();
    assert_eq!(saved["routine"]["botId"], "lead");
}

#[tokio::test]
async fn create_folder_policy_applies_to_explicit_and_default_workspaces() {
    let f = Fixture::new();
    for folder in [&f.dir, &f.dir.join("lead"), &crate::service::data_dir()] {
        assert!(f.call("create_bot", json!({"name":"No", "folder":folder})).await.is_err());
    }
    let made = f.call("create_bot", json!({"name":"Private"})).await.unwrap();
    let cwd = made["bot"]["folder"].as_str().unwrap();
    assert!(std::path::Path::new(cwd).starts_with(f.dir.join("workspaces").canonicalize().unwrap()));
    assert!(std::path::Path::new(cwd).is_dir());
}

#[tokio::test]
async fn creation_commits_with_the_expanded_workspace_root() {
    let home = crate::service::home();
    let name = format!("donnabot-manage-{}", uuid::Uuid::new_v4());
    let f = Fixture::in_dir(home.join(&name));
    dispatch(
        &f.hub,
        &Caller::Local,
        "setTeamSettings",
        json!({
            "workspaceRoot":format!("~/{name}/workspaces")
        }),
    )
    .await
    .unwrap();
    let made = f.call("create_bot", json!({"name":"Expandido"})).await.unwrap();
    let cwd = made["bot"]["folder"].as_str().unwrap();
    assert!(std::path::Path::new(cwd).starts_with(f.dir.join("workspaces").canonicalize().unwrap()));
    assert!(std::path::Path::new(cwd).is_dir());
    assert_eq!(agent_bot(&f.hub, made["bot"]["id"].as_str().unwrap()).unwrap().cwd, cwd);
    f.hub.shutdown().await;
}

#[tokio::test]
async fn unsafe_default_root_is_refused_before_approval_without_creating_a_folder() {
    let f = Fixture::new();
    let mut source = agent_bot(&f.hub, "lead").unwrap();
    source.permission = Permission::Ask;
    source.permission_chosen_by_user = true;
    f.hub.store.save_bot_as_owner(&source).unwrap();
    for root in [crate::service::home().join(".projetos"), std::path::PathBuf::from("/var/lib/donnabot-projetos")] {
        f.hub
            .store
            .kv_set("team.settings.v1", &json!({"primaryBotId":"lead", "workspaceRoot":root}).to_string())
            .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(2), f.call("create_bot", json!({"name":"Recusado"})))
            .await
            .expect("a pasta inválida deve ser recusada antes de esperar aprovação");
        assert!(result.is_err());
    }
    assert_eq!(f.hub.store.bots().unwrap().len(), 2);
    assert!(f.changes().is_empty());
    f.hub.shutdown().await;
}

#[tokio::test]
async fn management_can_update_legacy_target_without_changing_its_folder() {
    let f = Fixture::new();
    let mut target = agent_bot(&f.hub, "dev").unwrap();
    target.cwd = "/".into();
    f.hub.store.save_bot(&target).unwrap();
    f.call("update_bot", json!({"botId":"dev", "name":"Renomeado", "hidden":true})).await.unwrap();
    let updated = f.hub.bots_json(0).unwrap().into_iter().find(|bot| bot["id"] == "dev").unwrap();
    assert_eq!(updated["name"], "Renomeado");
    assert_eq!(updated["cwd"], "/");
    assert!(updated["workspaceIssue"].is_string());
    f.hub.shutdown().await;
}

#[tokio::test]
async fn failed_creation_publication_rolls_back_before_retry() {
    let f = Fixture::new();
    let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER fail_notice BEFORE INSERT ON entries WHEN NEW.kind = 'notice'
        BEGIN SELECT RAISE(ABORT, 'publication failed'); END;",
    )
    .unwrap();
    assert!(f.call("create_bot", json!({"name":"Scout"})).await.is_err());
    let folders: Vec<_> =
        std::fs::read_dir(f.dir.join("workspaces")).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(folders, ["scout"], "a falha remove apenas a pasta vazia recém-criada");
    assert!(f.call("create_bot", json!({"name":"Scout", "folder":f.dir.join("workspaces/scout")})).await.is_err());
    assert!(f.dir.join("workspaces/scout").is_dir(), "a pasta que já existia é preservada");
    assert_eq!(f.hub.store.bots().unwrap().iter().filter(|b| !b.deleted).count(), 2);
    assert_eq!(f.hub.store.pending_team_changes().unwrap(), Vec::<(String, String)>::new());
    db.execute_batch("DROP TRIGGER fail_notice").unwrap();
    f.call("create_bot", json!({"name":"Scout"})).await.unwrap();
    assert_eq!(f.hub.store.bots().unwrap().iter().filter(|b| !b.deleted).count(), 3);
    assert_eq!(f.changes().len(), 1);
}

#[tokio::test]
async fn recovery_quarantines_bad_records_without_blocking_startup_or_user_operations() {
    let f = Fixture::new();
    let source = agent_bot(&f.hub, "lead").unwrap();
    undo::prepare(&f.hub, &source, Some(agent_bot(&f.hub, "dev").unwrap()), vec![], "broken publication", None)
        .unwrap();
    f.hub.update_bot(&json!({"id":"dev", "name":"Changed"})).unwrap();
    f.hub.store.kv_set("team.pending.invalid", "invalid JSON").unwrap();
    let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER fail_notice BEFORE INSERT ON entries WHEN NEW.kind = 'notice'
        BEGIN SELECT RAISE(ABORT, 'publication failed'); END;",
    )
    .unwrap();
    f.hub.shutdown().await;
    let store = Store::open(&f.dir.join("test.db")).expect("bad recovery must not block opening the store");
    assert_eq!(store.pending_team_changes().unwrap().len(), 2, "open must leave recovery to serve");
    recover(&store);
    assert_eq!(store.pending_team_changes().unwrap().len(), 1, "valid pending must remain retryable");
    let quarantined: i64 =
        db.query_row("SELECT count(*) FROM kv WHERE k GLOB 'team.quarantine.*'", [], |r| r.get(0)).unwrap();
    assert_eq!(quarantined, 1);
    db.execute_batch("DROP TRIGGER fail_notice").unwrap();
    let hub = Hub::new(store, "quarantine".into(), Identity::load_or_create(&f.dir).unwrap(), "test".into(), 0);
    hub.start().unwrap();
    // Um novo registro ruim em runtime também não impede intenções do usuário.
    hub.store.kv_set("team.pending.live", "invalid JSON").unwrap();
    dispatch(&hub, &Caller::Local, "setTeamSettings", json!({"permission":"auto"})).await.unwrap();
    dispatch(
        &hub,
        &Caller::Local,
        "saveRoutine",
        json!({"botId":"lead", "name":"User",
        "instruction":"User task", "triggers":[{"type":"interval","seconds":3600}]}),
    )
    .await
    .unwrap();
    dispatch(&hub, &Caller::Local, "deleteBot", json!({"botId":"dev"})).await.unwrap();
    assert_eq!(hub.store.pending_team_changes().unwrap(), Vec::<(String, String)>::new());
    hub.shutdown().await;
}

#[tokio::test]
async fn startup_removes_legacy_manager_flags_idempotently_and_primary_switch_refreshes_both() {
    let f = Fixture::new();
    let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
    db.execute("UPDATE bots SET config = json_set(config, '$.manageTeam', json('true')) WHERE id = 'dev'", []).unwrap();
    f.hub.shutdown().await;
    let store = Store::open(&f.dir.join("test.db")).unwrap();
    assert!(!store.bot("dev").unwrap().unwrap().config.manage_team);
    let rev = store.bot("dev").unwrap().unwrap().rev;
    drop(store);
    let store = Store::open(&f.dir.join("test.db")).unwrap();
    assert_eq!(store.bot("dev").unwrap().unwrap().rev, rev);
    // Configs já com os flags esperados: mesmo assim ambas precisam receber Reconfigure/RefreshTools.
    crate::fork::donna_fixa::sem_trava(&db); // D366: troca legada da principal.
    db.execute("UPDATE bots SET config = json_set(config, '$.manageTeam', json(CASE WHEN id = 'dev' THEN 'true' ELSE 'false' END))", []).unwrap();
    let changed = store.set_team_settings(&json!({"primaryBotId":"dev"})).unwrap();
    assert_eq!(changed.len(), 2);
    assert!(changed.iter().any(|c| c.id == "lead"));
    assert!(changed.iter().any(|c| c.id == "dev"));
}

#[tokio::test]
async fn undo_preserves_noneditable_profile_fields_and_allows_error_state() {
    let f = Fixture::new();
    f.call("update_bot", json!({"botId":"dev", "name":"Changed", "instructions":"new"})).await.unwrap();
    let entry = f.changes().remove(0);
    f.hub
        .update_bot(&json!({"id":"dev", "position":9, "connectors":["other"], "skills":["review"],
        "model":"user-model", "backend":"codex", "permission":"ask", "permissionChosenByUser":true, "pinned":true}))
        .unwrap();
    for status in [BotStatus::Working, BotStatus::NeedsInput] {
        f.hub.set_runtime("dev", |r| r.status = status);
        assert!(undo(&f.hub, &entry.id).await.is_err());
    }
    f.hub.set_runtime("dev", |r| r.status = BotStatus::Error);
    undo(&f.hub, &entry.id).await.unwrap();
    let cfg = agent_bot(&f.hub, "dev").unwrap();
    assert_eq!(cfg.name, "dev");
    assert_eq!(cfg.description, "");
    assert_eq!(cfg.position, 9);
    assert_eq!(cfg.connectors, ["other"]);
    assert_eq!(cfg.skills, ["review"]);
    assert_eq!(cfg.model.as_deref(), Some("user-model"));
    assert_eq!(cfg.backend, "codex");
    assert_eq!(cfg.permission, Permission::Ask);
    assert!(cfg.pinned);
}

#[tokio::test]
async fn opening_a_store_does_not_recover_pending_changes_but_serve_recovery_does() {
    let f = Fixture::new();
    let source = agent_bot(&f.hub, "lead").unwrap();
    undo::prepare(&f.hub, &source, Some(agent_bot(&f.hub, "dev").unwrap()), vec![], "Changed", None).unwrap();
    f.hub.update_bot(&json!({"id":"dev", "name":"Changed"})).unwrap();
    f.hub.shutdown().await;
    let store = Store::open(&f.dir.join("test.db")).unwrap();
    assert_eq!(store.pending_team_changes().unwrap().len(), 1);
    assert!(!store.history("lead", i64::MAX, 100).unwrap().iter().any(|e| e.data["teamChange"].is_object()));
    assert_eq!(recover(&store).len(), 1);
    assert_eq!(store.pending_team_changes().unwrap(), Vec::<(String, String)>::new());
    assert_eq!(recover(&store), Vec::<String>::new());
}

#[tokio::test]
async fn stopped_actor_does_not_fail_a_saved_profile_and_routine_update() {
    let f = Fixture::new();
    f.hub.shutdown().await;
    let e = f.hub.store.insert_entry(&Lane::main("lead"), EntryKind::User, 2, &typed_request("Altere")).unwrap();
    f.hub.set_runtime("lead", |r| {
        r.status = BotStatus::Working;
        r.message = Some(e.id);
    });
    let result = f
        .call(
            "update_bot",
            json!({"botId":"dev", "name":"Changed", "routines":[
        {"name":"Scan", "instruction":"scan", "triggers":[{"type":"interval", "seconds":3600}]}]}),
        )
        .await
        .unwrap();
    assert_eq!(result["bot"]["name"], "Changed");
    assert_eq!(result["routines"].as_array().unwrap().len(), 1);
    assert_eq!(f.hub.routines.list("dev")["routines"].as_array().unwrap().len(), 1);
    let state: Value = serde_json::from_str(&f.hub.store.kv_read("routines.v1").unwrap().unwrap()).unwrap();
    assert_eq!(state["routines"][0]["name"], "Scan");
    let entry = f.changes().remove(0);
    undo(&f.hub, &entry.id).await.unwrap();
    assert_eq!(agent_bot(&f.hub, "dev").unwrap().name, "dev");
    assert_eq!(f.hub.routines.list("dev")["routines"], json!([]));
}

#[tokio::test]
async fn mx472_equipe_edicao_e_desfazer_invalidam_concessao() {
    let f = Fixture::new();
    let original = json!({"name":"Evento","instruction":"Original","triggers":[{"type":"event","source":"github","event":"pull_request.opened","filters":{"repo":"org/repo"}}]});
    let id = f.hub.routines.save(&f.hub, "dev", &original).unwrap()["routine"]["id"].as_str().unwrap().to_owned();
    let key = format!("donna.routine-external-grant.v1.{}", crate::agent::registry::sha256_hex(id.as_bytes()));
    let grant = || {
        let state: Value = serde_json::from_str(&f.hub.store.kv_read("routines.v1").unwrap().unwrap()).unwrap();
        let routine: crate::routines::Routine = serde_json::from_value(
            state["routines"].as_array().unwrap().iter().find(|r| r["id"] == id).unwrap().clone(),
        )
        .unwrap();
        let revision = crate::fork::routine_creation::external_grant::authority_revision(&routine).unwrap();
        f.hub.store.kv_set(&key,&json!({"version":1,"grantId":"test","origin":"localTransport","spaceId":"personal","botId":"dev","routineId":id,"entryId":"test","reviewedEntryRevision":1,"authorityRevision":revision,"contract":{"repository":"org/repo","event":"pull_request.opened","action":{"type":"notify","text":"test"}}}).to_string()).unwrap();
    };
    grant();
    let mut changed = original.clone();
    changed["id"] = json!(id);
    changed["instruction"] = json!("Alterada");
    f.call("update_bot", json!({"botId":"dev","routines":[changed]})).await.unwrap();
    assert!(f.hub.store.kv_read(&key).unwrap().is_none());
    let entry = f.changes().remove(0);
    grant();
    undo(&f.hub, &entry.id).await.unwrap();
    assert!(f.hub.store.kv_read(&key).unwrap().is_none());
}

#[tokio::test]
async fn failed_routine_write_rolls_back_the_profile_without_success_notice() {
    let f = Fixture::new();
    let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER fail_routines BEFORE INSERT ON kv WHEN NEW.k = 'routines.v1'
        BEGIN SELECT RAISE(ABORT, 'simulated disk failure'); END;",
    )
    .unwrap();
    assert!(
        f.call(
            "update_bot",
            json!({"botId":"dev", "name":"Changed", "routines":[
        {"name":"Scan", "instruction":"scan", "triggers":[{"type":"interval", "seconds":3600}]}]})
        )
        .await
        .is_err()
    );
    assert_eq!(agent_bot(&f.hub, "dev").unwrap().name, "dev");
    assert_eq!(f.hub.routines.list("dev")["routines"], json!([]));
    assert!(f.changes().is_empty());
    assert_eq!(f.hub.store.pending_team_changes().unwrap(), Vec::<(String, String)>::new());
}

#[tokio::test]
async fn transient_notice_failures_remain_retryable_without_absorbing_later_edits() {
    let f = Fixture::new();
    let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER fail_notice BEFORE INSERT ON entries WHEN NEW.kind = 'notice'
        BEGIN SELECT RAISE(ABORT, 'transient publication failure'); END;",
    )
    .unwrap();
    let saved = f.call("update_bot", json!({"botId":"dev", "name":"Changed"})).await.unwrap();
    assert_eq!(saved["bot"]["name"], "Changed");
    assert!(saved["notice"].as_str().unwrap().contains("será publicado na recuperação"));
    for _ in 0..2 {
        dispatch(&f.hub, &Caller::Local, "setTeamSettings", json!({"permission":"auto"})).await.unwrap();
        assert_eq!(f.hub.store.pending_team_changes().unwrap().len(), 1);
    }
    dispatch(&f.hub, &Caller::Local, "updateBot", json!({"id":"dev", "description":"later user edit"})).await.unwrap();
    db.execute_batch("DROP TRIGGER fail_notice").unwrap();
    recover_live(&f.hub);
    assert_eq!(f.hub.store.pending_team_changes().unwrap(), Vec::<(String, String)>::new());
    let entry = f.changes().remove(0);
    let journal: Value =
        serde_json::from_str(&f.hub.store.kv_read(&format!("team.undo.{}", entry.id)).unwrap().unwrap()).unwrap();
    assert_eq!(journal["after"]["description"], "", "a retry cannot absorb a later user edit");
    assert!(undo(&f.hub, &entry.id).await.is_err());
    assert_eq!(agent_bot(&f.hub, "dev").unwrap().description, "later user edit");
}

#[tokio::test]
async fn undo_commit_reserves_inactive_status_against_a_concurrent_turn_start() {
    let f = Fixture::new();
    let before = agent_bot(&f.hub, "dev").unwrap();
    f.call("update_bot", json!({"botId":"dev", "name":"Changed", "instructions":"instruções que serão desfeitas"}))
        .await
        .unwrap();
    let after = agent_bot(&f.hub, "dev").unwrap();
    let entry = f.changes().remove(0);
    let (attempt_tx, attempt_rx) = std::sync::mpsc::channel();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let mut starter = None;
    f.hub
        .with_inactive_bot("dev", || {
            let hub = f.hub.clone();
            starter = Some(std::thread::spawn(move || {
                attempt_tx.send(()).unwrap();
                let saved = hub.prepare_bot_turn("dev", None, |_| {}).unwrap();
                started_tx.send(saved).unwrap();
            }));
            attempt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert!(matches!(
                started_rx.recv_timeout(Duration::from_millis(20)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ));
            let routines = f.hub.store.kv_read("routines.v1")?.unwrap();
            f.hub.store.commit_team_undo(&entry.id, &after, Some(&before), &routines)?;
            assert!(matches!(started_rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)));
            Ok(())
        })
        .unwrap();
    starter.unwrap().join().unwrap();
    let adopted = started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(adopted.name, "dev", "the waiting turn must adopt the restored profile before its prompt");
    assert_eq!(adopted.description, "", "reverted instructions cannot reach the waiting turn");
    assert_eq!(agent_bot(&f.hub, "dev").unwrap().name, "dev");
    assert_eq!(f.hub.runtime("dev").status, BotStatus::Waiting);
    assert!(f.hub.runtime("dev").preparing_turn_token.is_some());
    assert!(f.hub.runtime("dev").memory_turn_token.is_none());
    assert!(f.hub.with_inactive_bot("dev", || Ok(())).is_err(), "preparo também impede Undo");
    assert_eq!(f.hub.store.entry(&entry.id).unwrap().data["teamChange"]["status"], "undone");
}

#[tokio::test]
async fn snapshot_write_failure_rolls_back_profile_routines_and_creation_together() {
    for creation in [false, true] {
        let f = Fixture::new();
        let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
        db.execute_batch(
            "CREATE TRIGGER fail_snapshot BEFORE UPDATE OF v ON kv
            WHEN NEW.k GLOB 'team.pending.*' AND json_type(NEW.v, '$.saved') = 'object'
            BEGIN SELECT RAISE(ABORT, 'snapshot write failure'); END;",
        )
        .unwrap();
        let mut args = json!({"name":"Changed", "routines":[
            {"name":"Scan", "instruction":"scan", "triggers":[{"type":"interval", "seconds":3600}]}]});
        let tool = if creation {
            "create_bot"
        } else {
            args["botId"] = json!("dev");
            "update_bot"
        };
        assert!(f.call(tool, args).await.is_err());
        assert_eq!(agent_bot(&f.hub, "dev").unwrap().name, "dev");
        assert_eq!(f.hub.store.bots().unwrap().iter().filter(|b| !b.deleted).count(), 2);
        assert_eq!(f.hub.routines.list("dev")["routines"], json!([]));
        assert_eq!(f.hub.store.pending_team_changes().unwrap(), Vec::<(String, String)>::new());
        assert!(f.changes().is_empty());
        dispatch(&f.hub, &Caller::Local, "updateBot", json!({"id":"dev", "description":"later user edit"}))
            .await
            .unwrap();
        db.execute_batch("DROP TRIGGER fail_snapshot").unwrap();
        recover_live(&f.hub);
        assert!(f.changes().is_empty());
        assert_eq!(agent_bot(&f.hub, "dev").unwrap().description, "later user edit");
    }
}

#[tokio::test]
async fn user_creation_inherits_general_unless_the_client_sends_explicit_intent() {
    let f = Fixture::new();
    dispatch(&f.hub, &Caller::Local, "setTeamSettings", json!({"permission":"ask"})).await.unwrap();
    let mut created = Vec::new();
    for (signal, expected, chosen) in
        [(None, Permission::Ask, false), (Some(false), Permission::Ask, false), (Some(true), Permission::Auto, true)]
    {
        let mut body = json!({"name":"Desktop", "description":"", "backend":"codex", "cwd": f.dir.join("lead"),
            "avatarColor":"blue", "avatarShape":"blob", "permission":"auto", "connectors":[], "skills":[], "browserPreference":"computer"});
        if let Some(signal) = signal {
            body["permissionChosenByUser"] = signal.into();
        }
        let made = dispatch(&f.hub, &Caller::Local, "createBot", body).await.unwrap();
        let id = made["bot"]["id"].as_str().unwrap();
        let cfg = agent_bot(&f.hub, id).unwrap();
        assert_eq!(cfg.permission, expected);
        assert_eq!(cfg.permission_chosen_by_user, chosen);
        assert!(cfg.created_by.is_none());
        assert_eq!(made["bot"]["browserPreference"], "computer");
        assert_eq!(cfg.browser_preference, crate::fork::browser::BrowserPreference::Computer);
        assert!(!cfg.computer, "preferência não promove Uso do computador");
        assert!(
            crate::fork::tela_por_computador::allowed_on(
                &f.hub.store.db.locked(),
                id,
                &crate::fork::tela_por_computador::host_target(&f.hub),
                None
            )
            .is_err(),
            "preferência não concede tela D536"
        );
        created.push((id.to_owned(), chosen));
    }
    dispatch(&f.hub, &Caller::Local, "setTeamSettings", json!({"permission":"auto"})).await.unwrap();
    let made = dispatch(
        &f.hub,
        &Caller::Local,
        "createBot",
        json!({"name":"Sem controle", "backend":"codex",
        "cwd": f.dir.join("lead"), "connectors":[], "skills":[], "permissionChosenByUser":true}),
    )
    .await
    .unwrap();
    let cfg = agent_bot(&f.hub, made["bot"]["id"].as_str().unwrap()).unwrap();
    assert_eq!(cfg.permission, Permission::Auto);
    assert!(!cfg.permission_chosen_by_user, "sinal sem permission não é escolha");
    // Reabrir executa migrations: espere os atores do Hub terminarem de gravar.
    f.hub.shutdown().await;
    let reopened = Store::open(&f.dir.join("test.db")).unwrap();
    for (id, chosen) in created {
        let persisted = reopened.bot(&id).unwrap().unwrap().config;
        assert_eq!(persisted.permission_chosen_by_user, chosen);
        assert_eq!(persisted.browser_preference, crate::fork::browser::BrowserPreference::Computer);
        assert!(!persisted.computer);
    }
}

#[tokio::test]
async fn unchosen_primary_requires_approval_when_general_is_ask() {
    let f = Fixture::new();
    dispatch(&f.hub, &Caller::Local, "setTeamSettings", json!({"permission":"ask"})).await.unwrap();
    let source = agent_bot(&f.hub, "lead").unwrap();
    assert!(!source.permission_chosen_by_user);
    assert_eq!(management_permission(&source, &f.hub.store.team_settings().unwrap()), Permission::Ask);
    // Sem turno real, o ator recusa o card; Liberar teria aplicado a edição.
    let error = f.call("update_bot", json!({"botId":"dev", "name":"Denied"})).await.unwrap_err().to_string();
    assert!(error.contains("didn't allow"), "{error}");
    assert_eq!(agent_bot(&f.hub, "dev").unwrap().name, "dev");
    assert!(f.changes().is_empty());
}

#[tokio::test]
async fn avatares_so_usuario_sem_legado_nos_cards_ou_list_team() {
    let f = Fixture::new();
    for (key, value) in [
        ("avatarColor", json!("red")),
        ("avatarShape", json!("hex")),
        ("avatarConfig", serde_json::to_value(crate::fork_avatars::preset("Donna", "", false)).unwrap()),
    ] {
        let mut args = json!({"name":"Hype"});
        args[key] = value.clone();
        assert!(f.call("create_bot", args).await.unwrap_err().to_string().contains("Só o usuário"));
        let mut args = json!({"botId":"dev"});
        args[key] = value;
        assert!(f.call("update_bot", args).await.unwrap_err().to_string().contains("Só o usuário"));
    }
    let made = f.call("create_bot", json!({"name":"Hype", "folder":f.dir.join("workspaces/scout")})).await.unwrap();
    let cfg = agent_bot(&f.hub, made["bot"]["id"].as_str().unwrap()).unwrap();
    assert_eq!(made["bot"]["avatarConfig"], serde_json::to_value(&cfg.avatar_config).unwrap());
    assert_eq!(made["bot"]["mexeriquinha"]["v"], 2);
    assert_eq!(made["bot"]["avatarPreset"], "Mexeriquinha Simples");
    assert!(made["bot"].get("avatarColor").is_none());
    assert!(made["bot"].get("avatarShape").is_none());
    let fields = json!({"name":"Hype", "avatarPreset":crate::fork_avatars::label(&cfg)});
    let card = describe(fields.as_object().unwrap(), &[]);
    assert!(card.contains("Avatar: Mexeriquinha Simples"));
    assert!(!card.contains("Color:") && !card.contains("Shape:"));
}

#[tokio::test]
async fn resposta_choice_nao_e_pedido_autoral_para_gestao() {
    let f = Fixture::new();
    assert!(user_turn(&f.hub, "lead").is_ok());
    for text in ["Autorize apagar", "Nenhuma destas", "Quero pagar"] {
        let mut data = typed_request(text);
        data["choice_response"] = json!({"version":1,"questionId":"synthetic","revision":2});
        let entry = f.hub.store.insert_entry(&Lane::main("lead"), EntryKind::User, 1, &data).unwrap();
        f.hub.set_runtime("lead", |r| r.message = Some(entry.id));
        assert!(user_turn(&f.hub, "lead").is_err());
    }
}

#[tokio::test]
async fn heranca_de_skills_fica_presa_a_selecao_original_do_criador() {
    let f = Fixture::new();
    // c1: ABA na seleção do criador depois da captura (aprovação/fila) e antes da gravação.
    let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER c1_aba AFTER INSERT ON kv WHEN NEW.k GLOB 'team.pending.*' BEGIN
           INSERT OR IGNORE INTO donna_fixa_dono(id) VALUES(1);
           UPDATE bots SET config=json_set(config,'$.skills',json('[]')) WHERE id='lead';
           UPDATE bots SET config=json_set(config,'$.skills',json('[\"review\"]')) WHERE id='lead';
           DELETE FROM donna_fixa_dono;
         END;",
    )
    .unwrap();
    let args = json!({"name":"Scout", "folder":f.dir.join("workspaces/scout")});
    let error = f.call("create_bot", args).await.unwrap_err();
    assert!(format!("{error:#}").contains(crate::fork::skill_selection::CONFLICT), "{error:#}");
    assert_eq!(f.hub.store.bots().unwrap().iter().filter(|b| !b.deleted).count(), 2);
    db.execute_batch("DROP TRIGGER c1_aba").unwrap();
    f.create().await;
    let created = f.hub.store.bots().unwrap().into_iter().find(|b| b.config.created_by.as_deref() == Some("lead"));
    assert_eq!(created.unwrap().config.skills, ["review"]);
}
