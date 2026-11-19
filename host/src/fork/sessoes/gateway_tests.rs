//! D70/R2 nos consumidores reais de credenciais e observe, sem motor ou sleeps.
#![allow(clippy::unwrap_used)]
use super::{bindings, bindings_tests::Fixture, call_tests};
use crate::{LockExt, api::devices::Caller, guard_hub::mcp::contexto::Context};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;

struct Gateway {
    f: Fixture,
    context: Context,
    bridge: String,
    transport: &'static str,
}

impl Gateway {
    fn new(transport: &'static str) -> Self {
        let f = Fixture::new();
        *f.hub.store.secret_key.locked() = Some(zeroize::Zeroizing::new(vec![42; 32]));
        let connector = if transport == "http" {
            json!({"id":"connector","name":"Fixture","url":"https://fixture.invalid/mcp",
                "headers":{"Authorization":"Bearer synthetic-secret"}})
        } else {
            json!({"id":"connector","name":"Fixture","command":"not-executed",
                "env":{"API_KEY":"synthetic-secret"}})
        };
        crate::market::save_connectors(&f.hub.store, &[serde_json::from_value(connector).unwrap()]).unwrap();
        let canonical = crate::market::connectors(&f.hub.store).unwrap().remove(0);
        let context = crate::fork::conectores::oauth_binding::issue_context(
            &f.hub.store,
            &canonical,
            "bot",
            "bot",
            None,
            f.a.id.as_str(),
        )
        .unwrap();
        Self { f, context, bridge: uuid::Uuid::new_v4().to_string(), transport }
    }

    async fn call(&self, action: &str, extra: Value) -> anyhow::Result<Value> {
        let mut body = json!({"context":self.context.id,"connector":"connector",
            "transport":self.transport,"bridgeId":self.bridge});
        body.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        crate::api::dispatch(&self.f.hub, &Caller::Local, action, body).await
    }
}

fn frame(method: &str) -> Value {
    let result = if method == "tools/call" {
        json!({"content":[{"type":"text","text":"Resultado sintético"}]})
    } else if method == "tools/list" {
        json!({"tools":[]})
    } else {
        json!({"protocolVersion":"2024-11-05","capabilities":{},"serverInfo":{"name":"fixture","version":"1"}})
    };
    json!({"method":method,"frame":{"jsonrpc":"2.0","id":1,"result":result}})
}

#[tokio::test]
async fn contexto_aberto_inativo_nao_recebe_credenciais_nem_descoberta_de_b() {
    for transport in ["http", "stdio"] {
        let g = Gateway::new(transport);
        g.call("donnaGuardRuntime", json!({"method":"initialize"})).await.unwrap();
        Fixture::start_on(&g.f.hub, &g.f.b);
        for method in ["initialize", "tools/list", "tools/call"] {
            assert!(g.call("donnaGuardRuntime", json!({"method":method})).await.is_err());
            assert!(g.call("donnaGuardObserve", frame(method)).await.is_err());
        }
        assert_eq!(g.f.hub.store.guard_test_snapshot().unwrap(), (vec![], 0));
        assert!(!g.f.hub.store.memory_session_marked("bot", "acp-a").unwrap());
        assert!(!g.f.hub.store.memory_session_marked("bot", "acp-b").unwrap());
    }
}

#[tokio::test]
async fn troca_durante_liberacao_de_credenciais_nao_adota_execucao_ou_preparacao_de_b() {
    for transport in ["http", "stdio"] {
        for preparing in [false, true] {
            for method in ["initialize", "tools/list"] {
                let g = Gateway::new(transport);
                if preparing {
                    g.f.hub.start_bot_turn("bot", |r| r.session = None).unwrap();
                    g.f.hub.prepare_session_binding(&g.f.a).unwrap();
                }
                let hub = g.f.hub.clone();
                let b = g.f.b.clone();
                let reached = call_tests::interleave(&g.f.hub.store, "gateway_credentials", move || {
                    Fixture::start_on(&hub, &b);
                });
                assert!(g.call("donnaGuardRuntime", json!({"method":method})).await.is_err());
                assert!(reached.load(Ordering::SeqCst));
                assert_eq!(g.f.hub.store.guard_test_snapshot().unwrap(), (vec![], 0));
            }
        }
    }
}

#[tokio::test]
async fn troca_entre_check_e_observe_tx_nao_grava_recibo_exposicao_ou_marca_parcial() {
    for transport in ["http", "stdio"] {
        for method in ["initialize", "tools/list", "tools/call"] {
            let g = Gateway::new(transport);
            g.call("donnaGuardRuntime", json!({"method":"initialize"})).await.unwrap();
            let hub = g.f.hub.clone();
            let b = g.f.b.clone();
            let reached = call_tests::interleave(&g.f.hub.store, "observe_commit", move || Fixture::start_on(&hub, &b));
            assert!(g.call("donnaGuardObserve", frame(method)).await.is_err());
            assert!(reached.load(Ordering::SeqCst));
            assert_eq!(g.f.hub.store.guard_test_snapshot().unwrap(), (vec![], 0));
            assert!(!g.f.hub.store.memory_session_marked("bot", "acp-a").unwrap());
            assert!(!g.f.hub.store.memory_session_marked("bot", "acp-b").unwrap());
        }
    }
}

#[tokio::test]
async fn notificacao_tardia_so_marca_a_sem_estado_global_credenciais_ou_retorno() {
    for transport in ["http", "stdio"] {
        let g = Gateway::new(transport);
        g.call("donnaGuardRuntime", json!({"method":"initialize"})).await.unwrap();
        Fixture::start_on(&g.f.hub, &g.f.b);
        let notification = json!({"method":"notification",
            "frame":{"jsonrpc":"2.0","method":"notifications/message","params":{"level":"info","data":"Externo de A"}}});
        let answer = g.call("donnaGuardObserve", notification).await.unwrap();
        assert!(answer["message"].is_null());
        let (receipts, global) = g.f.hub.store.guard_test_snapshot().unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(global, 0, "a exceção tardia não pode marcar lane global '*'");
        assert!(g.f.hub.store.memory_session_marked("bot", "acp-a").unwrap());
        assert!(!g.f.hub.store.memory_session_marked("bot", "acp-b").unwrap());
        assert!(g.call("donnaGuardRuntime", json!({"method":"initialize"})).await.is_err());
        let forged = json!({"method":"notification",
            "frame":{"id":2,"result":{"content":[{"type":"text","text":"não é notificação"}]}}});
        assert!(g.call("donnaGuardObserve", forged).await.is_err());
        assert_eq!(g.f.hub.store.guard_test_snapshot().unwrap(), (receipts, 0));
    }
}

#[tokio::test]
async fn frames_invalidos_nao_ganham_excecao_passiva_durante_b() {
    for transport in ["http", "stdio"] {
        let g = Gateway::new(transport);
        g.call("donnaGuardRuntime", json!({"method":"initialize"})).await.unwrap();
        Fixture::start_on(&g.f.hub, &g.f.b);
        for frame in [
            json!({"method":"tools/call"}),
            json!({"jsonrpc":"2.0","method":"tools/call"}),
            json!({"method":"notifications/message","params":{"level":"info","data":"sem versão"}}),
            json!({"jsonrpc":"1.0","method":"notifications/message","params":{"level":"info","data":"versão errada"}}),
            json!({"jsonrpc":"2.0","method":"notifications/message","id":null,"params":{"level":"info","data":"ID null"}}),
            json!({"jsonrpc":"2.0","method":"notifications/message","result":null,"params":{"level":"info","data":"resultado"}}),
            json!({"jsonrpc":"2.0","method":"notifications/message","error":null,"params":{"level":"info","data":"erro"}}),
            json!({"jsonrpc":"2.0","method":"notifications/message","params":{"data":"sem nível"}}),
            json!({"jsonrpc":"2.0","method":"notifications/progress","params":{"progressToken":"fixture","progress":0,"total":null}}),
        ] {
            // Mesmo rótulo forjado da ponte não constitui autoridade passiva.
            assert!(g.call("donnaGuardObserve", json!({"method":"notification","frame":frame})).await.is_err());
            assert_eq!(g.f.hub.store.guard_test_snapshot().unwrap(), (vec![], 0));
            assert!(!g.f.hub.store.memory_session_marked("bot", "acp-a").unwrap());
            assert!(!g.f.hub.store.memory_session_marked("bot", "acp-b").unwrap());
        }
    }
}

#[tokio::test]
async fn preparacao_canonica_new_load_e_reuso_da_principal_preserva_bootstrap() {
    for transport in ["http", "stdio"] {
        for loading in [false, true] {
            let g = Gateway::new(transport);
            g.f.hub.start_bot_turn("bot", |r| r.session = None).unwrap();
            let binding = if loading {
                g.f.a.clone()
            } else {
                bindings::issue(&g.f.hub.store, "bot", "bot", "bot", None).unwrap()
            };
            let canonical = crate::market::connectors(&g.f.hub.store).unwrap().remove(0);
            let context = crate::fork::conectores::oauth_binding::issue_context(
                &g.f.hub.store,
                &canonical,
                "bot",
                "bot",
                None,
                binding.id.as_str(),
            )
            .unwrap();
            let unprepared = g.call("donnaGuardRuntime", json!({"context":context.id,"method":"initialize"})).await;
            assert!(unprepared.is_err(), "linha aberta sem preparação canônica não é autoridade");
            g.f.hub.prepare_session_binding(&binding).unwrap();
            let preparing = crate::fork::conectores::oauth_runtime::RuntimeIntent::capture(
                &g.f.hub,
                &context.id,
                "connector",
                "initialize",
            )
            .unwrap();
            let check_preparing = || {
                preparing.validate_live(&g.f.hub)?;
                preparing.validate_db(&g.f.hub.store.db.locked()).map(|_| ())
            };
            check_preparing().unwrap();
            if transport == "http" {
                preparing.snapshot(&g.f.hub).unwrap();
            }
            for method in ["initialize", "tools/list"] {
                let answer = g.call("donnaGuardRuntime", json!({"context":context.id,"method":method})).await.unwrap();
                assert!(if transport == "http" { answer["url"].is_string() } else { answer["command"].is_string() });
                let mut observed = frame(method);
                observed["context"] = json!(context.id);
                assert!(g.call("donnaGuardObserve", observed).await.unwrap()["message"].is_object());
            }
            assert!(g.call("donnaGuardRuntime", json!({"context":context.id,"method":"tools/call"})).await.is_err());
            let acp = if loading { "acp-a" } else { "acp-new" };
            bindings::activate(&g.f.hub.store, &binding, acp).unwrap();
            assert!(preparing.validate_db(&g.f.hub.store.db.locked()).is_err());
            assert!(check_preparing().is_err(), "ativação consome a capacidade no próprio commit");
            g.f.hub.set_runtime("bot", |r| r.session = Some(acp.into()));
            assert!(check_preparing().is_err(), "captura antiga não migra para execução ativa");
            g.call("donnaGuardRuntime", json!({"context":context.id,"method":"initialize"})).await.unwrap();
            let mut body = frame("tools/call");
            body["context"] = json!(context.id);
            let answer = g.call("donnaGuardObserve", body).await.unwrap();
            assert!(answer["message"].is_object());
            // Reuso legítimo da mesma principal em outro turno passa pelo mesmo produtor.
            g.f.hub.start_bot_turn("bot", |r| r.session = None).unwrap();
            let live = bindings::resolve(&g.f.hub.store.db.locked(), binding.id.as_str(), "bot").unwrap();
            g.f.hub.prepare_session_binding(&live).unwrap();
            g.call("donnaGuardRuntime", json!({"context":context.id,"method":"initialize"})).await.unwrap();
        }
    }
}

#[tokio::test]
async fn intencao_de_preparacao_nao_adota_token_novo_e_setup_continua_restrito() {
    let g = Gateway::new("http");
    g.f.hub.start_bot_turn("bot", |r| r.session = None).unwrap();
    g.f.hub.prepare_session_binding(&g.f.a).unwrap();
    let original = crate::fork::conectores::oauth_runtime::RuntimeIntent::capture(
        &g.f.hub,
        &g.context.id,
        "connector",
        "initialize",
    )
    .unwrap();
    g.f.hub.start_bot_turn("bot", |r| r.session = None).unwrap();
    g.f.hub.prepare_session_binding(&g.f.a).unwrap();
    assert!(original.snapshot(&g.f.hub).is_err(), "mesmo binding não adota preparação/token do turno seguinte");
    let canonical = crate::market::connectors(&g.f.hub.store).unwrap().remove(0);
    let setup = crate::fork::conectores::oauth_binding::issue_setup(&g.f.hub.store, &canonical).unwrap();
    for method in ["initialize", "tools/list", "ping"] {
        let answer = g.call("donnaGuardRuntime", json!({"context":setup.id,"method":method})).await.unwrap();
        assert_eq!(answer["setup"], true);
    }
    assert!(g.call("donnaGuardRuntime", json!({"context":setup.id,"method":"tools/call"})).await.is_err());
    let id = setup.id.clone();
    drop(setup);
    assert!(g.call("donnaGuardRuntime", json!({"context":id,"method":"initialize"})).await.is_err());
}
