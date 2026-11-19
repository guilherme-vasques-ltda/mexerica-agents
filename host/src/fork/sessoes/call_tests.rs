//! D70 dos P1: intercalações síncronas na fronteira real, sem sleeps ou motor externo.
#![allow(clippy::unwrap_used)]
use super::{bindings, bindings_tests::Fixture};
use crate::{LockExt, api::devices::Caller, hub::Hub, store::Store};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

type Window = (usize, &'static str);
type Interleaving = Box<dyn FnOnce() + Send>;
static WINDOWS: LazyLock<Mutex<HashMap<Window, Interleaving>>> = LazyLock::new(Mutex::default);

pub(crate) fn before_effect(store: &Store, phase: &'static str) {
    let action = WINDOWS.locked().remove(&(std::ptr::from_ref(store) as usize, phase));
    if let Some(action) = action {
        action();
    }
}

pub(crate) fn interleave(
    store: &Store,
    phase: &'static str,
    action: impl FnOnce() + Send + 'static,
) -> Arc<AtomicBool> {
    let reached = Arc::new(AtomicBool::new(false));
    let flag = reached.clone();
    let previous = WINDOWS.locked().insert(
        (std::ptr::from_ref(store) as usize, phase),
        Box::new(move || {
            action();
            flag.store(true, Ordering::SeqCst);
        }),
    );
    assert!(previous.is_none());
    reached
}

fn interrupt(f: &Fixture, phase: &'static str, revoke: bool) -> Arc<AtomicBool> {
    let hub = f.hub.clone();
    let a = f.a.clone();
    let b = f.b.clone();
    let reached = Arc::new(AtomicBool::new(false));
    let flag = reached.clone();
    assert!(
        WINDOWS
            .locked()
            .insert(
                (std::ptr::from_ref(&f.hub.store) as usize, phase),
                Box::new(move || {
                    bindings::capture(&hub, "bot", a.id.as_str()).unwrap();
                    if revoke {
                        bindings::revoke(&hub.store, "bot", a.acp_session.as_deref()).unwrap();
                    } else {
                        Fixture::start_on(&hub, &b);
                    }
                    flag.store(true, Ordering::SeqCst);
                }),
            )
            .is_none()
    );
    reached
}

fn connector(f: &Fixture) {
    *f.hub.store.secret_key.locked() = Some(zeroize::Zeroizing::new(vec![42; 32]));
    let c = serde_json::from_value(json!({"id":"connector","name":"Fixture","command":"not-executed"})).unwrap();
    crate::market::save_connectors(&f.hub.store, &[c]).unwrap();
}

fn requests() -> Vec<(&'static str, Value)> {
    vec![
        ("request_login", json!({"site":"fixture.invalid","reason":"Pedido original"})),
        ("request_connection", json!({"connectorId":"connector","reason":"Pedido original"})),
        (
            "request_secret",
            json!({"connectorId":"connector","field":"KEY","location":"env","reason":"Pedido original"}),
        ),
    ]
}

fn no_tool_entry(hub: &Hub, events: &mut tokio::sync::broadcast::Receiver<Value>) {
    while let Ok(event) = events.try_recv() {
        assert_ne!(event["type"], "entry", "ferramenta não pode publicar entrada após a intercalação");
    }
    assert!(hub.store.history("bot", i64::MAX, 100).unwrap().iter().all(|e| e.data["connectionRequest"].is_null()));
}

#[tokio::test]
async fn connector_call_conserva_origem_da_api_apos_desbloqueio_do_cofre() {
    for (name, args) in requests() {
        let f = Fixture::new();
        connector(&f);
        let reached = interrupt(&f, "api_origin", false);
        let mut events = f.hub.events.subscribe();
        let result = crate::api::dispatch(
            &f.hub,
            &Caller::Local,
            "connectorCall",
            json!({
                "botId":"bot","sessionBinding":f.a.id.as_str(),"name":name,"arguments":args
            }),
        )
        .await;
        assert!(reached.load(Ordering::SeqCst));
        assert!(result.is_err(), "{name}");
        no_tool_entry(&f.hub, &mut events);
    }
}

#[tokio::test]
async fn pedidos_nativos_revalidam_na_transacao_e_controle_valido_fica_na_principal() {
    for (name, args) in requests() {
        for revoke in [false, true] {
            let f = Fixture::new();
            connector(&f);
            let execution = f.execution();
            let reached = interrupt(&f, "connector_commit", revoke);
            let mut events = f.hub.events.subscribe();
            assert!(
                crate::api::dispatch(
                    &f.hub,
                    &Caller::Local,
                    "connectorCall",
                    json!({
                        "botId":"bot","sessionBinding":execution.acp_binding().unwrap(),"name":name,"arguments":args
                    })
                )
                .await
                .is_err()
            );
            assert!(reached.load(Ordering::SeqCst));
            assert!(!f.hub.computers.external("bot"));
            no_tool_entry(&f.hub, &mut events);
        }
        let f = Fixture::new();
        connector(&f);
        let result = crate::api::dispatch(
            &f.hub,
            &Caller::Local,
            "connectorCall",
            json!({
                "botId":"bot","sessionBinding":f.a.id.as_str(),"name":name,"arguments":args
            }),
        )
        .await
        .unwrap();
        let entry = f.hub.store.entry(result["requestId"].as_str().unwrap()).unwrap();
        assert_eq!(entry.session_id, "bot");
        assert_eq!(entry.data["connectionRequest"]["status"], "pending");
        assert!(f.hub.computers.external("bot"));
    }
}

#[tokio::test]
async fn recusa_de_cartao_nao_apaga_marca_externa_anterior() {
    let f = Fixture::new();
    connector(&f);
    f.hub.computers.mark_external("bot");
    let reached = interrupt(&f, "connector_commit", true);
    assert!(
        crate::api::dispatch(
            &f.hub,
            &Caller::Local,
            "connectorCall",
            json!({
                "botId":"bot","sessionBinding":f.a.id.as_str(),"name":"request_login",
                "arguments":{"site":"fixture.invalid","reason":"Pedido original"}
            })
        )
        .await
        .is_err()
    );
    assert!(reached.load(Ordering::SeqCst));
    assert!(f.hub.computers.external("bot"));
}

fn routine() -> Value {
    json!({"name":"Rotina original","instruction":"Fixture sintética","triggers":[{"type":"webhook"}]})
}

fn chain_counts(hub: &Hub) -> (i64, i64, i64, i64) {
    let db = hub.store.db.locked();
    db.query_row(
        "SELECT COUNT(*),(SELECT COUNT(*) FROM donna_chain_admissions),COALESCE(SUM(used),0),
        COALESCE(SUM(CASE WHEN linked=0 THEN 1 ELSE 0 END),0) FROM donna_chain_roots",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
    .unwrap()
}

#[test]
fn ferramentas_de_rotina_revalidam_commit_sem_trocar_estado_fila_hooks_ou_publicar() {
    for (name, update) in [
        ("save_routine", false),
        ("save_routine", true),
        ("set_routine_enabled", false),
        ("delete_routine", false),
        ("run_routine", false),
        ("routine_webhook", false),
    ] {
        for revoke in [false, true] {
            let f = Fixture::new();
            let saved = f.hub.routines.save(&f.hub, "bot", &routine()).unwrap();
            let id = saved["routine"]["id"].as_str().unwrap();
            let args = match name {
                "save_routine" => {
                    let mut args = routine();
                    args["instruction"] = json!("Alteração recusada");
                    if update {
                        args["id"] = json!(id);
                    }
                    args
                }
                "set_routine_enabled" => json!({"id":id,"enabled":false}),
                "routine_webhook" => json!({"id":id,"rotate":true}),
                _ => json!({"id":id}),
            };
            let execution = f.execution();
            let before = f.hub.store.kv_read("routines.v1").unwrap();
            let state = f.hub.routines.list("bot");
            let hooks = serde_json::to_value(f.hub.routines.hooks()).unwrap();
            let hook_updates = f.hub.routines.subscribe_hooks();
            let reached = interrupt(&f, "routine_commit", revoke);
            let mut events = f.hub.events.subscribe();
            assert!(crate::routines::call_in_execution(&f.hub, "bot", name, &args, &execution).is_err(), "{name}");
            assert!(reached.load(Ordering::SeqCst));
            assert_eq!(f.hub.store.kv_read("routines.v1").unwrap(), before, "{name}");
            assert_eq!(f.hub.routines.list("bot"), state, "{name}: estado/fila não publicados");
            assert_eq!(serde_json::to_value(f.hub.routines.hooks()).unwrap(), hooks);
            assert!(!hook_updates.has_changed().unwrap());
            assert_eq!(chain_counts(&f.hub), (0, 0, 0, 0), "{name}: sem recibo/raiz órfã antes da guarda");
            no_tool_entry(&f.hub, &mut events);
        }
    }
}

#[tokio::test]
async fn run_routine_reverte_admissao_e_fila_se_json_falha_e_retry_nao_duplica_raiz() {
    let f = Fixture::new();
    let saved = f.hub.routines.save(&f.hub, "bot", &routine()).unwrap();
    let id = saved["routine"]["id"].as_str().unwrap();
    let body = json!({"botId":"bot","sessionBinding":f.a.id.as_str(),
        "name":"run_routine","arguments":{"id":id}});
    let before = f.hub.store.kv_read("routines.v1").unwrap();
    let state = f.hub.routines.list("bot");
    let hooks = serde_json::to_value(f.hub.routines.hooks()).unwrap();
    let hook_updates = f.hub.routines.subscribe_hooks();
    let mut events = f.hub.events.subscribe();
    f.hub
        .store
        .db
        .locked()
        .execute_batch(
            "CREATE TRIGGER falhar_apos_admitir BEFORE INSERT ON kv
        WHEN NEW.k='routines.v1' AND EXISTS(SELECT 1 FROM donna_chain_admissions)
        BEGIN SELECT RAISE(ABORT,'falha após admitir'); END;",
        )
        .unwrap();
    let error = crate::api::dispatch(&f.hub, &Caller::Local, "routineCall", body.clone()).await.unwrap_err();
    assert!(error.to_string().contains("falha após admitir"), "{error:#}");
    assert_eq!(chain_counts(&f.hub), (0, 0, 0, 0));
    assert_eq!(crate::fork::cadeias::admissao::orfas(&f.hub.store).unwrap(), [] as [(String, String, String); 0]);
    assert_eq!(f.hub.store.kv_read("routines.v1").unwrap(), before);
    assert_eq!(f.hub.routines.list("bot"), state);
    assert_eq!(serde_json::to_value(f.hub.routines.hooks()).unwrap(), hooks);
    assert!(!hook_updates.has_changed().unwrap());
    no_tool_entry(&f.hub, &mut events);
    f.hub.store.db.locked().execute_batch("DROP TRIGGER falhar_apos_admitir").unwrap();
    let valid = crate::api::dispatch(&f.hub, &Caller::Local, "routineCall", body.clone()).await.unwrap();
    assert_eq!(valid["run"]["status"], "pending");
    assert!(valid["run"].get("_chain").is_none());
    assert_eq!(chain_counts(&f.hub), (1, 1, 1, 0));
    assert!(crate::api::dispatch(&f.hub, &Caller::Local, "routineCall", body).await.is_err());
    assert_eq!(chain_counts(&f.hub), (1, 1, 1, 0), "retry sem nonce não cria outro pedido ativo");
    let restored = crate::routines::Routines::default();
    restored.restore(&f.hub.store).unwrap();
    assert_eq!(restored.list("bot")["runs"][0]["id"], valid["run"]["id"]);
    assert_eq!(chain_counts(&f.hub), (1, 1, 1, 0));
}

#[test]
fn ferramentas_de_rotina_validas_preservam_produtores_app_e_disparo_na_principal() {
    let f = Fixture::new();
    let execution = f.execution();
    let saved = crate::routines::call_in_execution(&f.hub, "bot", "save_routine", &routine(), &execution).unwrap();
    let id = saved["routine"]["id"].as_str().unwrap();
    for enabled in [false, true] {
        crate::routines::call_in_execution(
            &f.hub,
            "bot",
            "set_routine_enabled",
            &json!({"id":id,"enabled":enabled}),
            &execution,
        )
        .unwrap();
    }
    crate::routines::call_in_execution(&f.hub, "bot", "routine_webhook", &json!({"id":id,"rotate":true}), &execution)
        .unwrap();
    let run = crate::routines::call_in_execution(&f.hub, "bot", "run_routine", &json!({"id":id}), &execution).unwrap();
    assert_eq!(run["run"]["status"], "pending");
    crate::routines::call_in_execution(&f.hub, "bot", "delete_routine", &json!({"id":id}), &execution).unwrap();
    assert_eq!(f.hub.routines.list("bot")["runs"][0]["status"], "cancelled");
    assert!(f.hub.store.history("bot", i64::MAX, 100).unwrap().iter().all(|e| e.session_id == "bot"));
    // O caminho do app conserva seu próprio produtor, sem ExecutionRef de MCP fabricada.
    f.hub.routines.save(&f.hub, "bot", &routine()).unwrap();
}

#[tokio::test]
async fn leituras_de_conectores_e_rotinas_revalidam_antes_de_responder() {
    for name in ["list_connectors", "list_logins"] {
        let f = Fixture::new();
        connector(&f);
        let reached = interrupt(&f, "connector_read", false);
        assert!(crate::market::requests::call_in_execution(&f.hub, "bot", name, &json!({}), &f.execution()).is_err());
        assert!(reached.load(Ordering::SeqCst));
    }
    for name in ["list_routines", "routine_webhook"] {
        let f = Fixture::new();
        let saved = f.hub.routines.save(&f.hub, "bot", &routine()).unwrap();
        let reached = interrupt(&f, "routine_read", false);
        assert!(
            crate::routines::call_in_execution(
                &f.hub,
                "bot",
                name,
                &json!({"id":saved["routine"]["id"]}),
                &f.execution(),
            )
            .is_err()
        );
        assert!(reached.load(Ordering::SeqCst));
    }
}

#[tokio::test]
async fn team_pessoal_conserva_execucao_ate_admissao_atomica_e_reverte_segundo_insert() {
    use crate::chat::team::admission_tests::{fail_recipient, no_notices, snapshot};

    for name in ["ask_bot", "message_bot"] {
        for phase in ["api_origin", "team_notices_commit"] {
            for revoke in [false, true] {
                let f = Fixture::new();
                let mut recipient = f.hub.test_bot_commands("other");
                f.hub.team.start_turn("bot", None);
                let before = snapshot(&f.hub, "other").unwrap();
                let reached = interrupt(&f, phase, revoke);
                let mut events = f.hub.events.subscribe();
                let result = crate::api::dispatch(
                    &f.hub,
                    &Caller::Local,
                    "teamCall",
                    json!({"botId":"bot","sessionBinding":f.a.id.as_str(),"name":name,
                        "arguments":{"botId":"other","message":"Pedido negado"}}),
                )
                .await;
                assert!(reached.load(Ordering::SeqCst));
                assert!(result.is_err(), "{name}/{phase}/{revoke}");
                assert_eq!(snapshot(&f.hub, "other").unwrap(), before);
                assert!(recipient.try_recv().is_err(), "nenhum pedido nem CancelAsk na admissão negada");
                no_notices(&mut events);
            }
        }
        let f = Fixture::new();
        let mut recipient = f.hub.test_bot_commands("other");
        f.hub.team.start_turn("bot", None);
        fail_recipient(&f.hub, "other").unwrap();
        let before = snapshot(&f.hub, "other").unwrap();
        let mut events = f.hub.events.subscribe();
        let body = json!({"botId":"bot","sessionBinding":f.a.id.as_str(),"name":name,
            "arguments":{"botId":"other","message":"Par atômico"}});
        let error = crate::api::dispatch(&f.hub, &Caller::Local, "teamCall", body.clone()).await.unwrap_err();
        assert!(error.to_string().contains("team second insert"), "{error:#}");
        assert_eq!(snapshot(&f.hub, "other").unwrap(), before, "inclui rollback do contador de turno");
        assert!(recipient.try_recv().is_err());
        no_notices(&mut events);
        f.hub.store.db.locked().execute_batch("DROP TRIGGER team_second_insert").unwrap();
        let mut valid = Box::pin(crate::api::dispatch(&f.hub, &Caller::Local, "teamCall", body));
        if name == "ask_bot" {
            assert!(futures::poll!(valid.as_mut()).is_pending());
        } else {
            assert!(matches!(futures::poll!(valid.as_mut()), std::task::Poll::Ready(Ok(_))));
        }
        let crate::agent::bot::Cmd::BotRequest(request) = recipient.try_recv().unwrap() else {
            panic!("pedido admitido esperado")
        };
        let after = snapshot(&f.hub, "other").unwrap();
        assert_eq!(after.0, before.0 + 2);
        assert_eq!(after.1.as_deref(), Some("1"));
        assert_eq!(after.2, if name == "ask_bot" { (1, 0) } else { (0, 1) });
        request.complete(Ok("Resposta".into()), false);
        if name == "ask_bot" {
            assert!(valid.await.is_ok());
        }
        assert_eq!(f.hub.team.test_pending_counts(), (0, 0));
        while let Ok(command) = recipient.try_recv() {
            assert!(!matches!(command, crate::agent::bot::Cmd::BotRequest(_)));
        }
    }
}

#[tokio::test]
async fn pedido_da_extra_usa_pessoa_da_acp_e_nao_do_destinatario() {
    for person in [None, Some(crate::chat::memory::profile::OWNER)] {
        let f = Fixture::new();
        Fixture::start_on(&f.hub, &f.b);
        if let Some(person) = person {
            crate::chat::memory::bind_profile_person(&f.hub.store, "bot", "acp-b", Some(person)).unwrap();
        }
        crate::chat::memory::bind_profile_person(
            &f.hub.store,
            "other",
            "acp-destino",
            Some(crate::chat::memory::profile::OWNER),
        )
        .unwrap();
        let mut recipient = f.hub.test_bot_commands("other");
        f.hub.team.start_turn("bot", None);
        crate::api::dispatch(&f.hub, &Caller::Local, "teamCall", json!({"botId":"bot","sessionBinding":f.b.id.as_str(),"name":"message_bot", "arguments":{"botId":"other","message":"Pedido"}})).await.unwrap();
        let crate::agent::bot::Cmd::BotRequest(request) = recipient.try_recv().unwrap() else {
            panic!("pedido esperado")
        };
        assert_eq!(request.person.as_deref(), person);
        request.complete(Ok("Resposta".into()), false);
    }
}
