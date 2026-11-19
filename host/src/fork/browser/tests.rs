//! Fixtures isoladas; sem runtime de navegador, contas ou perfis reais.
#![allow(clippy::unwrap_used)]
use super::*;
use crate::{
    api::dispatch,
    fork::{
        sessoes::call_tests,
        spaces::{
            PERSONAL,
            tool_context::{self, tests::Fixture},
        },
    },
    store::{BotConfig, Device, DeviceSource, Scope},
};
use std::sync::atomic::Ordering;

fn args() -> Value {
    json!({"version":2,"target":"desktop"})
}
fn body(f: &Fixture) -> Value {
    let mut body = f.body.clone();
    body["name"] = json!(TOOL);
    body["arguments"] = args();
    body
}
fn app(f: &Fixture) -> Value {
    json!({"hubId":f.hub.host_id,"spaceId":f.cfg.space_id,"botId":f.cfg.id,
        "version":2,"target":"desktop"})
}
fn assert_unavailable(value: &Value, bot: &str, space: &str) {
    assert_eq!(value["version"], 2);
    assert_eq!(value["status"], "unavailable");
    assert_eq!(value["scope"]["botId"], bot);
    assert_eq!(value["scope"]["spaceId"], space);
    assert_eq!(value["profiles"], json!({"status":"unavailable"}));
    assert!(value["capabilities"].as_object().unwrap().values().all(|v| v == false));
    assert!(value.get("profileId").is_none());
}
fn add_space_b(f: &Fixture) {
    let mut state = f.hub.store.spaces().unwrap();
    let mut space = state.spaces[0].clone();
    space.id = "fixture-space-b".into();
    space.kind = "space".into();
    space.name = "Space B sintético".into();
    state.spaces.push(space);
    f.hub.store.kv_set("donna.spaces.v1", &serde_json::to_string(&state).unwrap()).unwrap();
    let cfg: BotConfig = serde_json::from_value(json!({
        "id":"fixture-bot-b","name":"Bot B sintético","spaceId":"fixture-space-b"
    }))
    .unwrap();
    f.hub.store.save_bot(&cfg).unwrap();
}

#[tokio::test]
async fn dispatch_app_e_bot_concordam_sem_perfis_ou_efeitos() {
    let f = Fixture::with_memory_profile(|_, _| {});
    // Store em memória inicia com chave sintética; esta fixture exige cofre trancado.
    *f.hub.store.secret_key.locked() = None;
    let files = std::fs::read_dir(&f.dir).unwrap().count();
    let unlocks = f.hub.store.vault_unlock_attempts.load(Ordering::SeqCst);
    let history = f.hub.store.history(f.cfg.session_owner(), i64::MAX, 100).unwrap();
    let mut events = f.hub.events.subscribe();
    assert!(f.hub.store.secret_key.locked().is_none());
    let bot = dispatch(&f.hub, &Caller::Local, "memoryCall", body(&f)).await.unwrap();
    let desktop = dispatch(&f.hub, &Caller::Local, METHOD, app(&f)).await.unwrap();
    assert_eq!(bot, desktop);
    assert_unavailable(&bot, &f.cfg.id, &f.cfg.space_id);
    assert_eq!(bot["reason"], "backend_absent");
    assert_eq!(bot["browserPreference"], "integrated");
    assert_eq!(definition()["inputSchema"]["properties"]["version"]["enum"], json!([2]));
    let mut request = app(&f);
    request["target"] = json!("host");
    let host = dispatch(&f.hub, &Caller::Local, METHOD, request).await.unwrap();
    assert_unavailable(&host, &f.cfg.id, &f.cfg.space_id);
    assert_eq!(host["target"], "host");
    assert_eq!(host["reason"], "backend_absent");
    assert_eq!(f.hub.store.vault_unlock_attempts.load(Ordering::SeqCst), unlocks);
    assert!(f.hub.store.secret_key.locked().is_none(), "não inicializa nem desbloqueia cofre");
    assert_eq!(f.hub.store.history(f.cfg.session_owner(), i64::MAX, 100).unwrap().len(), history.len());
    assert_eq!(std::fs::read_dir(&f.dir).unwrap().count(), files, "não cria diretórios de perfil");
    assert!(events.try_recv().is_err(), "não publica nem solicita aprovação humana");
}

#[tokio::test]
async fn space_a_b_nao_compartilham_escopo_mesmo_no_mesmo_hub_e_dono() {
    let f = Fixture::with_memory_profile(|_, _| {});
    add_space_b(&f);
    for (bot, space) in [(f.cfg.id.as_str(), PERSONAL), ("fixture-bot-b", "fixture-space-b")] {
        let mut request = app(&f);
        request["botId"] = json!(bot);
        request["spaceId"] = json!(space);
        let response = dispatch(&f.hub, &Caller::Local, METHOD, request).await.unwrap();
        assert_unavailable(&response, bot, space);
    }
    for (bot, space) in [(f.cfg.id.as_str(), "fixture-space-b"), ("fixture-bot-b", PERSONAL)] {
        let mut request = app(&f);
        request["botId"] = json!(bot);
        request["spaceId"] = json!(space);
        assert!(dispatch(&f.hub, &Caller::Local, METHOD, request).await.is_err());
    }
    let context = f.context();
    assert!(call(&f.hub, "fixture-bot-b", TOOL, &args(), Some(&context), context.turn.execution.as_ref()).is_err());
}

#[tokio::test]
async fn aba_e_revogacao_antes_da_resposta_descartam_contexto_original() {
    for revoke in [false, true] {
        let f = Fixture::with_memory_profile(|_, _| {});
        let hub = f.hub.clone();
        let bot = f.cfg.id.clone();
        let reached = call_tests::interleave(&f.hub.store, "browser_response", move || {
            if revoke {
                bindings::revoke(&hub.store, &bot, None).unwrap();
            } else {
                // Escritor sintético sob a cerca existente: A→B→A conserva a geração incrementada.
                let db = hub.store.db.locked();
                let original: String =
                    db.query_row("SELECT config FROM bots WHERE id=?1", [&bot], |r| r.get(0)).unwrap();
                db.execute("UPDATE bots SET config=json_set(config,'$.spaceId','fixture-space-b') WHERE id=?1", [&bot])
                    .unwrap();
                db.execute("UPDATE bots SET config=?2 WHERE id=?1", rusqlite::params![bot, original]).unwrap();
            }
        });
        assert!(dispatch(&f.hub, &Caller::Local, "memoryCall", body(&f)).await.is_err());
        assert!(reached.load(Ordering::SeqCst));
    }
}

#[tokio::test]
async fn resposta_tardia_nao_adota_turno_sucessor() {
    let f = Fixture::with_memory_profile(|_, _| {});
    let hub = f.hub.clone();
    let bot = f.cfg.id.clone();
    let reached = call_tests::interleave(&f.hub.store, "browser_response", move || {
        hub.set_runtime(&bot, |runtime| runtime.memory_turn_token = Some("turno-sucessor-fixture".into()));
    });
    assert!(dispatch(&f.hub, &Caller::Local, "memoryCall", body(&f)).await.is_err());
    assert!(reached.load(Ordering::SeqCst));
}

#[tokio::test]
async fn contexto_ausente_cliente_antigo_e_recurso_inventado_falham_fechado() {
    let f = Fixture::with_memory_profile(|_, _| {});
    let mut request = body(&f);
    request.as_object_mut().unwrap().remove("sessionBinding");
    assert!(dispatch(&f.hub, &Caller::Local, "memoryCall", request).await.is_err());
    assert!(call(&f.hub, &f.cfg.id, TOOL, &args(), None, None).is_err());
    for field in ["hubId", "spaceId", "botId"] {
        let mut request = app(&f);
        request.as_object_mut().unwrap().remove(field);
        assert!(dispatch(&f.hub, &Caller::Local, METHOD, request).await.is_err(), "{field}");
        let mut request = app(&f);
        request[field] = json!("inventado");
        assert!(dispatch(&f.hub, &Caller::Local, METHOD, request).await.is_err(), "{field}");
    }
    for negotiation in
        [json!({}), json!({"version":1,"target":"host"}), json!({"version":3,"target":"host"}), json!({"version":2})]
    {
        let mut request = body(&f);
        request["arguments"] = negotiation;
        let response = dispatch(&f.hub, &Caller::Local, "memoryCall", request).await.unwrap();
        assert_eq!(response["reason"], "client_unsupported");
        assert_unavailable(&response, &f.cfg.id, PERSONAL);
    }
    let context = f.context();
    for name in ["browser_open", "browser_profiles", "browser_login", "browser_evaluate"] {
        assert!(call(&f.hub, &f.cfg.id, name, &args(), Some(&context), context.turn.execution.as_ref()).is_err());
    }
}

#[tokio::test]
async fn argumentos_de_pagina_nao_concedem_acesso_ou_alteram_contexto() {
    let f = Fixture::with_memory_profile(|_, _| {});
    // Store em memória inicia com chave sintética; esta fixture exige cofre trancado.
    *f.hub.store.secret_key.locked() = None;
    let unlocks = f.hub.store.vault_unlock_attempts.load(Ordering::SeqCst);
    for extra in [
        json!({"spaceId":"outro"}),
        json!({"profileId":"inventado"}),
        json!({"browserPreference":"computer"}),
        json!({"password":"canario-sintetico"}),
        json!({"approved":true}),
        json!({"authority":"dono"}),
        json!({"url":"https://fixture.invalid/"}),
        json!({"target":"compartilhado"}),
        json!({"version":"1"}),
    ] {
        let mut request = body(&f);
        request["arguments"].as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        assert!(dispatch(&f.hub, &Caller::Local, "memoryCall", request).await.is_err());
        let mut request = app(&f);
        request.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        assert!(dispatch(&f.hub, &Caller::Local, METHOD, request).await.is_err());
    }
    assert_eq!(f.hub.store.vault_unlock_attempts.load(Ordering::SeqCst), unlocks);
    assert!(f.hub.store.secret_key.locked().is_none());
}

#[tokio::test]
async fn catalogo_real_anuncia_somente_consulta_negativa() {
    let f = Fixture::with_memory_profile(|_, _| {});
    let mut request = f.body.clone();
    request["name"] = json!("__tools");
    request["arguments"] = json!({});
    let response = tool_context::route(&f.hub, "memoryCall", &request).await.unwrap().unwrap();
    let browser = response["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["name"].as_str().unwrap().starts_with("browser_"))
        .map(|t| t["name"].clone())
        .collect::<Vec<_>>();
    assert_eq!(browser, vec![json!(TOOL)]);
}

#[tokio::test]
async fn porta_de_rpc_exige_autoridade_original_de_controle_e_classificacao() {
    use crate::fork::donna_fixa::metodos::{Classe, METODOS};
    let f = Fixture::with_memory_profile(|_, _| {});
    assert!(METODOS.contains(&(METHOD, Classe::Leitura)));
    let device = Device {
        key: "browser-fixture-device".into(),
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
    assert!(dispatch(&f.hub, &caller, METHOD, app(&f)).await.is_ok());
    let bare = Caller::Device { key: device.key.clone(), scopes: device.scopes.clone(), autoridade: None };
    assert!(dispatch(&f.hub, &bare, METHOD, app(&f)).await.is_err());
    f.hub.store.db.locked().execute("DELETE FROM devices WHERE key=?1", [&device.key]).unwrap();
    assert!(dispatch(&f.hub, &caller, METHOD, app(&f)).await.is_err());
    let pairing = Caller::Pairing { key: "fixture-pairing".into() };
    assert!(dispatch(&f.hub, &pairing, METHOD, app(&f)).await.is_err());
}

#[tokio::test]
async fn somente_browser_status_dispensa_validacao_d62_no_consumidor_real() {
    let f = Fixture::with_memory_profile(|_, _| {});
    let runtime = f.hub.runtime(&f.cfg.id);
    // Falha de leitura sintética e localizada: o tipo BLOB não pode virar backend String.
    // Assim o teste prova a passagem pelo leitor D62, além de observar um erro final.
    f.hub
        .store
        .db
        .locked()
        .execute(
            "INSERT INTO donna_memory_auto_engines(bot,token,chat,backend) VALUES(?1,?2,?3,?4)
         ON CONFLICT(bot) DO UPDATE SET token=excluded.token,chat=excluded.chat,backend=excluded.backend",
            rusqlite::params![
                f.cfg.id,
                runtime.memory_turn_token.as_deref().unwrap(),
                f.cfg.session_owner(),
                rusqlite::types::Value::Blob(vec![0])
            ],
        )
        .unwrap();
    let response = dispatch(&f.hub, &Caller::Local, "memoryCall", body(&f)).await.unwrap();
    assert_unavailable(&response, &f.cfg.id, &f.cfg.space_id);
    for name in ["browser_open", "browser_status_extra"] {
        let mut request = body(&f);
        request["name"] = json!(name);
        request["arguments"][crate::fork::memory_auto::transport::ARG] = json!(uuid::Uuid::new_v4().to_string());
        let error = dispatch(&f.hub, &Caller::Local, "memoryCall", request).await.unwrap_err();
        assert!(
            matches!(error.downcast_ref::<rusqlite::Error>(),
            Some(rusqlite::Error::InvalidColumnType(_, column, rusqlite::types::Type::Blob)) if column == "backend"),
            "a recusa deve vir da leitura D62: {error:#}"
        );
    }
    // A exceção exata não limpa argumentos forjados para torná-los válidos.
    let mut request = body(&f);
    request["arguments"][crate::fork::memory_auto::transport::ARG] = json!("forjado");
    let error = dispatch(&f.hub, &Caller::Local, "memoryCall", request).await.unwrap_err();
    assert!(error.to_string().starts_with("invalid_field:"), "{error:#}");
}

#[tokio::test]
async fn rpc_do_app_recusa_ponte_de_bot_e_aparelhos_screen_ou_executor() {
    let f = Fixture::with_memory_profile(|_, _| {});
    let unlocks = f.hub.store.vault_unlock_attempts.load(Ordering::SeqCst);
    let binding =
        bindings::resolve(&f.hub.store.db.locked(), f.body["sessionBinding"].as_str().unwrap(), &f.cfg.id).unwrap();
    let mut servers = vec![json!({"env":[]})];
    crate::fork::ponte_credencial::provisionar(
        &f.hub,
        &f.cfg.id,
        f.body["sessionSlot"].as_str(),
        &binding,
        &mut servers,
    )
    .unwrap();
    let token = zeroize::Zeroizing::new(servers[0]["env"][0]["value"].as_str().unwrap().to_owned());
    let ponte = f.hub.pontes.autenticar(&f.hub, &token).unwrap();
    assert!(matches!(&ponte, Caller::Ponte { bot, .. } if bot == &f.cfg.id));
    let error = dispatch(&f.hub, &ponte, METHOD, app(&f)).await.unwrap_err();
    assert!(error.is::<crate::api::devices::Forbidden>(), "{error:#}");
    for scope in [Scope::Screen, Scope::Executor] {
        let device = Device {
            key: "browser-fixture-sem-controle".into(),
            name: "Fixture".into(),
            platform: "fixture".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![scope],
            lease_until: None,
            created_at: 1,
            last_seen_at: None,
        };
        f.hub.store.put_device(&device).unwrap();
        let caller = crate::fork::autoridade::autenticar(&f.hub, &device.key).unwrap().1;
        let error = dispatch(&f.hub, &caller, METHOD, app(&f)).await.unwrap_err();
        assert!(error.is::<crate::api::devices::Forbidden>(), "{error:#}");
    }
    assert_eq!(f.hub.store.vault_unlock_attempts.load(Ordering::SeqCst), unlocks);
}

#[tokio::test]
async fn rpc_do_app_recusa_bot_apagado_e_bot_de_grupo() {
    let f = Fixture::with_memory_profile(|_, _| {});
    let group: BotConfig = serde_json::from_value(json!({
        "id":"browser-fixture-grupo","name":"Grupo sintético","kind":"group","members":[]
    }))
    .unwrap();
    assert!(group.is_group());
    f.hub.store.save_bot(&group).unwrap();
    let mut request = app(&f);
    request["botId"] = json!(group.id);
    let error = dispatch(&f.hub, &Caller::Local, METHOD, request).await.unwrap_err();
    assert_eq!(error.to_string(), "browser_scope_unavailable");
    f.hub.store.db.locked().execute("UPDATE bots SET deleted=1 WHERE id=?1", [&f.cfg.id]).unwrap();
    assert!(dispatch(&f.hub, &Caller::Local, METHOD, app(&f)).await.is_err());
    // Exercita também a guarda própria, sem depender da recusa anterior de spaces::guard.
    let mut request = app(&f);
    request.as_object_mut().unwrap().remove("hubId");
    assert_eq!(status(&f.hub, &Caller::Local, request).unwrap_err().to_string(), "browser_scope_unavailable");
}
