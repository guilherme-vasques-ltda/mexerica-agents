//! D541 complemento 3: preferência não é concessão; fixtures locais sem navegador ou dados reais.
#![allow(clippy::unwrap_used)]
use super::{BrowserPreference, METHOD};
use crate::{
    LockExt,
    api::{devices::Caller, dispatch},
    fork::{autoridade::Autoridade, spaces::tool_context::tests::Fixture},
    store::{BotConfig, Device, DeviceSource, Scope},
};
use serde_json::{Value, json};

fn request(f: &Fixture) -> Value {
    json!({"hubId":f.hub.host_id,"spaceId":f.cfg.space_id,"botId":f.cfg.id,"version":2,"target":"desktop"})
}

#[test]
fn configuracao_antiga_default_integrado_e_valores_invalidos_recusam() {
    let cfg: BotConfig = serde_json::from_value(json!({"id":"fixture","name":"Fixture"})).unwrap();
    assert_eq!(cfg.browser_preference, BrowserPreference::Integrated);
    assert_eq!(serde_json::to_value(&cfg).unwrap()["browserPreference"], "integrated");
    for value in [Value::Null, json!(false), json!("chrome"), json!({"mode":"computer"})] {
        let mut raw = serde_json::to_value(&cfg).unwrap();
        raw["browserPreference"] = value;
        assert!(serde_json::from_value::<BotConfig>(raw).is_err());
    }
}

#[tokio::test]
async fn dono_escolhe_em_todos_bots_patch_omisso_preserva_sem_conceder_tela() {
    for primary in [false, true] {
        let f = Fixture::with_memory_profile(|_, _| {});
        if primary {
            f.hub.store.set_team_settings(&json!({"primaryBotId":f.cfg.id})).unwrap();
        }
        let mut commands = f.hub.test_bot_commands(&f.cfg.id);
        let screen_before: i64 =
            f.hub.store.db.locked().query_row("SELECT COUNT(*) FROM donna_screen_access", [], |r| r.get(0)).unwrap();
        let unlocks = f.hub.store.vault_unlock_attempts.load(std::sync::atomic::Ordering::SeqCst);
        dispatch(&f.hub, &Caller::Local, "updateBot", json!({"id":f.cfg.id,"browserPreference":"computer"}))
            .await
            .unwrap();
        dispatch(&f.hub, &Caller::Local, "updateBot", json!({"id":f.cfg.id,"pinned":true})).await.unwrap();
        let stored = f.hub.store.bot(&f.cfg.id).unwrap().unwrap().config;
        assert_eq!(stored.browser_preference, BrowserPreference::Computer);
        assert_eq!(stored.computer, f.cfg.computer);
        assert_eq!(f.hub.bot_json(&f.hub.store.bot(&f.cfg.id).unwrap().unwrap())["browserPreference"], "computer");
        let target = crate::fork::tela_por_computador::host_target(&f.hub);
        {
            let db = f.hub.store.db.locked();
            assert!(crate::fork::tela_por_computador::allowed_on(&db, &f.cfg.id, &target, None).is_err());
            let count: i64 = db.query_row("SELECT COUNT(*) FROM donna_screen_access", [], |r| r.get(0)).unwrap();
            assert_eq!(count, screen_before);
        }
        // Consulta exclusivamente do integrado: a preferência PC não atesta disponibilidade de CUA.
        for runtime in ["desktop", "host"] {
            let mut body = request(&f);
            body["target"] = json!(runtime);
            let answer = dispatch(&f.hub, &Caller::Local, METHOD, body).await.unwrap();
            assert_eq!(answer["version"], 2);
            assert_eq!(answer["browserPreference"], "computer");
            assert_eq!(answer["reason"], "backend_absent");
            assert!(answer["capabilities"].as_object().unwrap().values().all(|v| v == false));
        }
        assert_eq!(f.hub.store.vault_unlock_attempts.load(std::sync::atomic::Ordering::SeqCst), unlocks);
        while let Ok(command) = commands.try_recv() {
            assert!(matches!(command, crate::agent::bot::Cmd::Reconfigure(_)));
        }
    }
}

#[tokio::test]
async fn revogacao_entre_captura_e_commit_nao_grava_preferencia() {
    let f = Fixture::with_memory_profile(|_, _| {});
    let device = Device {
        key: "browser-preference-fixture".into(),
        name: "Fixture".into(),
        platform: "fixture".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: 1,
        last_seen_at: None,
    };
    f.hub.store.put_device(&device).unwrap();
    let caller = crate::fork::autoridade::autenticar(&f.hub, &device.key).unwrap().1;
    let hub = f.hub.clone();
    *f.hub.store.before_owner_write.locked() = Some(Box::new(move || {
        hub.store.db.locked().execute("DELETE FROM devices WHERE key=?1", [&device.key]).unwrap();
    }));
    let error = dispatch(&f.hub, &caller, "updateBot", json!({"id":f.cfg.id,"browserPreference":"computer"}))
        .await
        .unwrap_err();
    assert!(error.is::<crate::fork::autoridade::AutorizacaoEncerrada>(), "{error:#}");
    assert_eq!(f.hub.store.bot(&f.cfg.id).unwrap().unwrap().config.browser_preference, BrowserPreference::Integrated);
    assert!(f.hub.store.before_owner_write.locked().is_none());
}

#[test]
fn escritores_internos_e_criacao_por_bot_nao_escolhem_computer_undo_preserva() {
    let f = Fixture::with_memory_profile(|_, _| {});
    let patch = json!({"id":f.cfg.id,"browserPreference":"computer"});
    assert!(f.hub.update_bot(&patch).is_err());
    assert!(f.hub.update_bot_by_user_authorized(&patch, None, None).is_err());
    let before = f.hub.store.bot(&f.cfg.id).unwrap().unwrap().config;
    let mut pc = before.clone();
    pc.browser_preference = BrowserPreference::Computer;
    assert!(f.hub.store.save_bot(&pc).is_err(), "a guarda vale também fora do patch do Hub");
    Autoridade::local()
        .with_db(&f.hub, None, |db| crate::fork::donna_fixa::como_dono(db, |db| crate::store::bots::save_bot(db, &pc)))
        .unwrap();
    let mut child: BotConfig = serde_json::from_value(json!({
        "id":"browser-child-fixture","name":"Filho","createdBy":f.cfg.id
    }))
    .unwrap();
    assert_eq!(child.browser_preference, BrowserPreference::Integrated);
    f.hub.store.save_bot(&child).unwrap();
    child.id = "browser-child-forged".into();
    child.browser_preference = pc.browser_preference;
    assert!(f.hub.store.save_bot_as_owner(&child).is_err(), "criação por bot não herda nem escolhe PC");
    let restored = crate::store::restore_team_profile(&pc, &before);
    assert_eq!(restored.browser_preference, BrowserPreference::Computer, "Undo de gestão preserva a escolha atual");
    assert!(f.hub.store.save_bot(&restored).is_ok(), "escritor interno pode preservar a preferência");
}

#[tokio::test]
async fn desligado_e_so_navegador_computador_livre_e_patch_preserva() {
    let f = Fixture::with_memory_profile(|_, _| {});
    let _commands = f.hub.test_bot_commands(&f.cfg.id);
    dispatch(&f.hub, &Caller::Local, "updateBot", json!({"id":f.cfg.id,"computer":true})).await.unwrap();
    assert!(f.hub.store.bot(&f.cfg.id).unwrap().unwrap().config.computer);
    dispatch(&f.hub, &Caller::Local, "updateBot", json!({"id":f.cfg.id,"browserPreference":"off"})).await.unwrap();
    let cfg = f.hub.store.bot(&f.cfg.id).unwrap().unwrap().config;
    assert_eq!(cfg.browser_preference, BrowserPreference::Off);
    // Desligado = só navegador (dono, 10/11): "Usar o computador" não muda.
    assert!(cfg.computer, "off não desliga Usar o computador");
    // O status v2 devolve a preferência canônica "off" e segue indisponível.
    let answer = dispatch(&f.hub, &Caller::Local, METHOD, request(&f)).await.unwrap();
    assert_eq!(answer["browserPreference"], "off");
    assert_eq!(answer["status"], "unavailable");
    assert!(answer["capabilities"].as_object().unwrap().values().all(|v| v == false));
    // Patch sem o campo preserva; o computador continua ligável e desligável com off.
    dispatch(&f.hub, &Caller::Local, "updateBot", json!({"id":f.cfg.id,"pinned":true})).await.unwrap();
    assert_eq!(f.hub.store.bot(&f.cfg.id).unwrap().unwrap().config.browser_preference, BrowserPreference::Off);
    dispatch(&f.hub, &Caller::Local, "updateBot", json!({"id":f.cfg.id,"computer":false})).await.unwrap();
    assert!(!f.hub.store.bot(&f.cfg.id).unwrap().unwrap().config.computer);
    dispatch(&f.hub, &Caller::Local, "updateBot", json!({"id":f.cfg.id,"computer":true})).await.unwrap();
    assert!(f.hub.store.bot(&f.cfg.id).unwrap().unwrap().config.computer, "computer=true com off é permitido");
    // Sair de Desligado não mexe no computador.
    dispatch(&f.hub, &Caller::Local, "updateBot", json!({"id":f.cfg.id,"browserPreference":"computer"})).await.unwrap();
    let cfg = f.hub.store.bot(&f.cfg.id).unwrap().unwrap().config;
    assert_eq!(cfg.browser_preference, BrowserPreference::Computer);
    assert!(cfg.computer);
}

#[test]
fn criacao_por_bot_nao_herda_nem_escolhe_desligado() {
    let f = Fixture::with_memory_profile(|_, _| {});
    let child: BotConfig = serde_json::from_value(json!({
        "id":"browser-off-child","name":"Filho","createdBy":f.cfg.id,"browserPreference":"off"
    }))
    .unwrap();
    assert_eq!(child.browser_preference, BrowserPreference::Off);
    assert!(f.hub.store.save_bot(&child).is_err(), "criação por bot não herda nem escolhe Desligado");
    // Desligado convive com "Usar o computador": off+computer=true é estado válido
    // (a preferência só muda pelo caminho do dono).
    let mut cfg = f.cfg.clone();
    cfg.browser_preference = BrowserPreference::Off;
    cfg.computer = true;
    assert!(f.hub.store.save_bot_as_owner(&cfg).is_ok());
}

#[tokio::test]
async fn patch_omisso_preserva_preferencia_escrita_antes_do_commit() {
    let f = Fixture::with_memory_profile(|_, _| {});
    let _commands = f.hub.test_bot_commands(&f.cfg.id);
    let hub = f.hub.clone();
    let bot = f.cfg.id.clone();
    *f.hub.store.before_owner_write.locked() = Some(Box::new(move || {
        let db = hub.store.db.locked();
        let mut cfg = crate::store::Store::bot_on(&db, &bot).unwrap().unwrap().config;
        cfg.browser_preference = BrowserPreference::Computer;
        crate::fork::donna_fixa::como_dono(&db, |db| crate::store::bots::save_bot(db, &cfg)).unwrap();
    }));
    dispatch(&f.hub, &Caller::Local, "updateBot", json!({"id":f.cfg.id,"pinned":true})).await.unwrap();
    let cfg = f.hub.store.bot(&f.cfg.id).unwrap().unwrap().config;
    assert_eq!(cfg.browser_preference, BrowserPreference::Computer);
    assert!(cfg.pinned);
    assert!(!cfg.computer);
}
