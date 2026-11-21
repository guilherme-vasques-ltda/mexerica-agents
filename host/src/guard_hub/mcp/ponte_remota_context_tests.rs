//! D70: Bridge HTTP real → router do Hub → handlers Guard → servidor sintético.
use super::*;
use crate::{
    LockExt,
    fork::spaces::{connector_context, tool_context::tests::Fixture},
    market::oauth::upstream02_tests::{RuntimeWindow, pause_runtime},
};
use axum::{Json, Router, body::Body, http::StatusCode, response::Response, routing};
use futures::{StreamExt, stream};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Notify;

struct Server {
    port: u16,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn server(router: Router) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener fixture");
    let port = listener.local_addr().expect("porta fixture").port();
    let task = tokio::spawn(async move {
        axum::serve(listener, router.into_make_service_with_connect_info::<std::net::SocketAddr>())
            .await
            .expect("servidor fixture");
    });
    Server { port, task }
}
/// Initialize canônico (M1): sem `protocolVersion` a ponte recusa localmente, sem rede.
fn initialize() -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":crate::mcp::PROTOCOL_VERSION}})
}
fn reply(frame: &Value) -> Value {
    json!({"jsonrpc":"2.0","id":frame["id"],"result":if frame["method"] == "initialize" {
        json!({"protocolVersion":crate::mcp::PROTOCOL_VERSION,"capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}})
    } else { json!({"content":[{"type":"text","text":"fixture_http"}]}) }})
}
async fn bridge(url: &str) -> (Fixture, Server, Session<Bridge>) {
    let f = Fixture::with_memory_profile(|store, cfg| {
        cfg.connectors = vec!["fixture".into()];
        let connector =
            serde_json::from_value(json!({"id":"fixture","name":"Fixture","url":url})).expect("conector fixture");
        crate::market::save_connectors(store, &[connector]).expect("salvar conector fixture");
    });
    let connector = crate::market::connectors(&f.hub.store).expect("conectores fixture").remove(0);
    let definition = connector_context::definition(
        &f.hub,
        &f.cfg,
        Some("vinculo-A"),
        &connector,
        std::path::Path::new("/fixture/host"),
        0,
        &f.cfg.id,
        f.cfg.session_owner(),
        None,
        f.body["sessionBinding"].as_str().expect("vínculo de sessão da fixture"),
    )
    .expect("definição real")
    .expect("MCP configurado");
    let context = definition["args"]
        .as_array()
        .expect("argumentos")
        .windows(2)
        .find(|p| p[0] == "--context")
        .expect("contexto emitido")[1]
        .as_str()
        .expect("ID")
        .to_owned();
    let api = server(crate::api::router(f.hub.clone())).await;
    let session = Session::new(Bridge {
        port: api.port,
        token: Zeroizing::new("test".into()),
        connector: "fixture".into(),
        context,
        id: uuid::Uuid::new_v4().to_string(),
        acquired: false,
        request: String::new(),
        begin: true,
        sequence: 0,
        params: Value::Null,
    })
    .expect("sessão fixture");
    (f, api, session)
}
fn change(f: &Fixture, what: &str) {
    match what {
        "context" => f.hub.set_runtime(&f.cfg.id, |r| r.memory_turn_token = Some("turno-B".into())),
        "permission" => {
            let mut commands = f.hub.test_bot_commands(&f.cfg.id);
            assert_eq!(
                f.hub.store.bot(&f.cfg.id).expect("perfil fixture").expect("bot fixture").config.permission,
                crate::store::Permission::Auto,
                "pré-condição permissiva"
            );
            f.hub
                .update_bot_by_user(&json!({"id":f.cfg.id,"permission":"ask","permissionChosenByUser":true}))
                .expect("revogar permissão");
            assert_eq!(
                f.hub.store.bot(&f.cfg.id).expect("perfil fixture").expect("bot fixture").config.permission,
                crate::store::Permission::Ask,
                "revogação humana persistida antes de liberar a espera"
            );
            assert!(
                matches!(commands.try_recv(), Ok(crate::agent::bot::Cmd::Reconfigure(cfg)) if cfg.id == f.cfg.id && cfg.permission == crate::store::Permission::Ask),
                "Reconfigure Ask do mesmo bot recebido antes de liberar a espera"
            );
        }
        _ => {}
    }
}
fn main_empty(f: &Fixture) {
    assert_eq!(
        f.hub.store.history(&f.cfg.id, i64::MAX, 30).expect("histórico").into_iter().map(|e| e.id).collect::<Vec<_>>(),
        Vec::<String>::new(),
        "nenhuma entrada na principal"
    );
}

#[tokio::test]
async fn http_real_recusa_contexto_ou_permissao_revogados_durante_unlock_antes_do_post() {
    for what in ["none", "context", "permission"] {
        let posts = Arc::new(AtomicUsize::new(0));
        let count = posts.clone();
        let remote = server(Router::new().route(
            "/mcp",
            routing::post(move |Json(frame): Json<Value>| {
                let count = count.clone();
                async move {
                    if frame["method"] == "tools/call" {
                        count.fetch_add(1, Ordering::SeqCst);
                    }
                    Json(reply(&frame))
                }
            }),
        ))
        .await;
        let (f, _api, mut session) = bridge(&format!("http://127.0.0.1:{}/mcp", remote.port)).await;
        let initialized = session.exchange(initialize()).await.expect("handshake real").expect("resposta initialize");
        assert_eq!(initialized["id"], 1);
        assert!(initialized.get("result").is_some(), "handshake com versão aceita: {initialized}");
        let gate = pause_runtime(&f.hub, RuntimeWindow::BeforeUnlock);
        let call = tokio::spawn(async move {
            session.exchange(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"fixture"}})).await
        });
        gate.wait().await;
        change(&f, what);
        gate.open();
        let result = call.await.expect("chamada fixture");
        if what == "none" {
            assert!(result.expect("positivo").expect("retorno").to_string().contains("fixture_http"));
            assert_eq!(posts.load(Ordering::SeqCst), 1, "positivo encaminhou uma vez");
        } else {
            assert!(result.is_err(), "captura original revogada deve falhar");
            assert_eq!(posts.load(Ordering::SeqCst), 0, "nenhuma chamada encaminhada após espera");
        }
        main_empty(&f);
    }
}

#[tokio::test]
async fn http_legacy_real_revalida_apos_esperar_endpoint_sem_postar_na_origem_errada() {
    for what in ["none", "context", "permission"] {
        let posts = Arc::new(AtomicUsize::new(0));
        let (entered, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
        let (ready, gate, count) = (entered.clone(), release.clone(), posts.clone());
        let remote = server(
            Router::new()
                .route(
                    "/mcp",
                    routing::post(|| async { StatusCode::METHOD_NOT_ALLOWED }).get(move || {
                        let (ready, gate) = (ready.clone(), gate.clone());
                        async move {
                            ready.notify_one();
                            gate.notified().await;
                            let events =
                                stream::once(async { Ok::<_, std::io::Error>("event: endpoint\ndata: /post\n\n") })
                                    .chain(stream::pending());
                            Response::builder()
                                .header("content-type", "text/event-stream")
                                .body(Body::from_stream(events))
                                .expect("stream fixture")
                        }
                    }),
                )
                .route(
                    "/post",
                    routing::post(move |Json(frame): Json<Value>| {
                        let count = count.clone();
                        async move {
                            count.fetch_add(1, Ordering::SeqCst);
                            Json(reply(&frame))
                        }
                    }),
                ),
        )
        .await;
        let (f, _api, mut session) = bridge(&format!("http://127.0.0.1:{}/mcp", remote.port)).await;
        let call = tokio::spawn(async move { session.exchange(initialize()).await });
        tokio::time::timeout(Duration::from_secs(5), entered.notified()).await.expect("GET legado alcançou espera");
        change(&f, what);
        release.notify_one();
        let result = call.await.expect("chamada fixture");
        if what == "none" {
            assert!(result.is_ok(), "handshake legado positivo: {result:?}");
            assert_eq!(posts.load(Ordering::SeqCst), 1);
        } else {
            assert!(result.is_err(), "retomada legada revogada deve falhar");
            assert_eq!(posts.load(Ordering::SeqCst), 0, "nenhum POST legado depois da espera");
        }
        main_empty(&f);
    }
}

#[tokio::test]
async fn regra_recusa_http_antes_do_post() {
    use crate::fork::rules::rule::{Effect, Rule};
    let posts = Arc::new(AtomicUsize::new(0));
    let count = posts.clone();
    let remote = server(Router::new().route(
        "/mcp",
        routing::post(move |Json(frame): Json<Value>| {
            let count = count.clone();
            async move {
                if frame["method"] == "tools/call" {
                    count.fetch_add(1, Ordering::SeqCst);
                }
                Json(reply(&frame))
            }
        }),
    ))
    .await;
    let (f, _api, mut session) = bridge(&format!("http://127.0.0.1:{}/mcp", remote.port)).await;
    session.exchange(initialize()).await.unwrap();
    let revision = f.hub.store.donna_rules().unwrap().revision;
    f.hub
        .store
        .donna_rule_save(
            Rule {
                id: String::new(),
                effect: Effect::Deny,
                field: "ferramenta".into(),
                op: "igual".into(),
                value: "mcp__fixture__fixture".into(),
                bot_id: None,
                tool: None,
                scope: None,
                enabled: true,
                revision: 0,
                created_at: 0,
                updated_at: 0,
            },
            revision,
            true,
        )
        .unwrap();
    let result = session
        .exchange(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"fixture","arguments":{}}}))
        .await;
    assert_eq!(result.unwrap_err(), Failure::Denied);
    assert_eq!(posts.load(Ordering::SeqCst), 0);
    f.hub.store.db.locked().execute("DROP TABLE donna_rules", []).unwrap();
    let unavailable = session
        .exchange(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"fixture","arguments":{}}}))
        .await;
    assert!(unavailable.is_err());
    assert_eq!(posts.load(Ordering::SeqCst), 0);
}

/// Marketplace, fatia 2: Bridge real → router do Hub → admissão final. A conta é do catálogo (ficha do Linear), então
/// o alvo não é o servidor sintético: o que se prova aqui é a decisão do Hub imediatamente antes do envio.
async fn bridge_do_catalogo() -> (Fixture, Server, Bridge, crate::market::Connector) {
    let f = Fixture::with_memory_profile(|store, cfg| {
        cfg.connectors = vec!["fixture".into()];
        let connector = serde_json::from_value(json!({
            "id":"fixture","name":"Fixture","url":"https://mcp.linear.app/mcp/readonly",
            "servico":"linear","tipo":"linear.oficial_oauth","liberacoes":["ler"]}))
        .expect("conector fixture");
        crate::market::save_connectors(store, &[connector]).expect("salvar conector fixture");
    });
    let connector = crate::market::connectors(&f.hub.store).expect("conectores fixture").remove(0);
    let definition = connector_context::definition(
        &f.hub,
        &f.cfg,
        Some("vinculo-A"),
        &connector,
        std::path::Path::new("/fixture/host"),
        0,
        &f.cfg.id,
        f.cfg.session_owner(),
        None,
        f.body["sessionBinding"].as_str().expect("vínculo de sessão da fixture"),
    )
    .expect("definição real")
    .expect("MCP configurado");
    let context = definition["args"]
        .as_array()
        .expect("argumentos")
        .windows(2)
        .find(|p| p[0] == "--context")
        .expect("contexto emitido")[1]
        .as_str()
        .expect("ID")
        .to_owned();
    let api = server(crate::api::router(f.hub.clone())).await;
    let bridge = Bridge {
        port: api.port,
        token: Zeroizing::new("test".into()),
        connector: "fixture".into(),
        context,
        id: uuid::Uuid::new_v4().to_string(),
        acquired: false,
        request: uuid::Uuid::new_v4().to_string(),
        begin: false,
        sequence: 1,
        params: Value::Null,
    };
    (f, api, bridge, connector)
}

#[tokio::test]
async fn admissao_final_barra_o_envio_de_acao_nao_ligada_desligada_depois_ou_de_lista_invalidada() {
    use crate::market::ferramentas::{self, PedidoLigar};
    let (f, _api, mut bridge, conta) = bridge_do_catalogo().await;
    let ferramenta = |nome: &str, perigosa: bool| {
        let mut tool = json!({"name": nome, "description": "d", "inputSchema": {"type": "object"}});
        if perigosa {
            tool["annotations"] = json!({"destructiveHint": true});
        }
        tool
    };
    let tools = [ferramenta("get_issue", false), ferramenta("list_issues", true), ferramenta("create_issue", false)];
    let nomes: std::collections::BTreeSet<String> =
        tools.iter().map(|t| t["name"].as_str().unwrap().to_owned()).collect();
    let geracao = Some(ferramentas::geracao(&f.hub.store, "fixture").unwrap());
    ferramentas::registrar(&f.hub.store, &conta, &tools, Some(&nomes), true, geracao).expect("inventário completo");
    let tentar = async |bridge: &mut Bridge, nome: &str| {
        bridge.params = json!({"name": nome, "arguments": {}});
        bridge.admit(&Value::Null, "tools/call").await
    };
    // O pedido real nasce no alvo (`beginRequest`); a admissão final reusa o mesmo pedido.
    bridge
        .call(
            "donnaGuardRuntime",
            json!({"method":"tools/call","validateOnly":true,"beginRequest":true,"params":{"name":"inicio"}}),
        )
        .await
        .expect("pedido registrado no Hub");
    // A ficha do Linear não marca leitura alguma: nada passa até a pessoa ligar, nem o que parece leitura.
    for nome in ["get_issue", "list_issues", "create_issue", "fantasma"] {
        let erro = tentar(&mut bridge, nome).await.expect_err(nome);
        assert!(erro.downcast_ref::<super::super::http::PolicyDenied>().is_some(), "{nome}: {erro}");
    }
    // A pessoa liga list_issues neste bot: passa. Desliga depois da decisão do alvo: a admissão final recusa.
    let digest = ferramentas::ler_ferramenta(&tools[1]).unwrap().digest;
    let pedido = PedidoLigar { bot: &f.cfg.id, conta: "fixture", ferramenta: "list_issues", digest: &digest };
    ferramentas::gravar_liberacao(&f.hub.store.db.locked(), &pedido).expect("a pessoa liga");
    assert!(tentar(&mut bridge, "list_issues").await.is_ok());
    ferramentas::gravar_desligamento(&f.hub.store.db.locked(), &f.cfg.id, "fixture", "list_issues").expect("desligar");
    assert!(tentar(&mut bridge, "list_issues").await.is_err(), "desligada antes do envio: não sai");
    // Aviso de lista mudada: nada passa, nem o que a pessoa ligou, até uma lista completa nova.
    ferramentas::gravar_liberacao(&f.hub.store.db.locked(), &pedido).expect("a pessoa liga de novo");
    assert!(tentar(&mut bridge, "list_issues").await.is_ok());
    ferramentas::invalidar(&f.hub.store, "fixture").unwrap();
    assert!(tentar(&mut bridge, "list_issues").await.is_err());
    let geracao = Some(ferramentas::geracao(&f.hub.store, "fixture").unwrap());
    ferramentas::registrar(&f.hub.store, &conta, &tools, Some(&nomes), true, geracao).unwrap();
    assert!(tentar(&mut bridge, "list_issues").await.is_ok(), "uma lista completa nova reabre o que a pessoa ligou");
    // Outros métodos MCP não passam por esta admissão.
    bridge.params = Value::Null;
    assert!(bridge.admit(&Value::Null, "tools/list").await.is_ok());
}

/// Duas pontes do mesmo bot e da mesma conta: o aviso de lista mudada chega com um `tools/list` em voo em cada uma.
/// Resposta atrasada (anotada antes do aviso) e resposta mediada retida não reabrem o inventário nem apagam o que a
/// pessoa ligou; só uma descoberta pedida depois do aviso reabre. Conta de envios = admissões finais que passaram.
#[tokio::test]
async fn duas_pontes_resposta_atrasada_e_descoberta_retida_nao_reabrem_o_inventario_e_nenhum_envio_sai() {
    use crate::market::ferramentas::{self, PedidoLigar};
    let (f, _api, mut a, conta) = bridge_do_catalogo().await;
    let mut b = Bridge {
        port: a.port,
        token: Zeroizing::new("test".into()),
        connector: "fixture".into(),
        context: a.context.clone(),
        id: uuid::Uuid::new_v4().to_string(),
        acquired: false,
        request: uuid::Uuid::new_v4().to_string(),
        begin: false,
        sequence: 1,
        params: Value::Null,
    };
    let tools = [json!({"name": "list_issues", "description": "d", "inputSchema": {"type": "object"}})];
    let nomes: std::collections::BTreeSet<String> = ["list_issues".to_owned()].into();
    let store = &f.hub.store;
    let geracao = |store: &crate::store::Store| Some(ferramentas::geracao(store, "fixture").unwrap());
    ferramentas::registrar(store, &conta, &tools, Some(&nomes), true, geracao(store)).unwrap();
    let digest = ferramentas::ler_ferramenta(&tools[0]).unwrap().digest;
    let pedido = PedidoLigar { bot: &f.cfg.id, conta: "fixture", ferramenta: "list_issues", digest: &digest };
    ferramentas::gravar_liberacao(&store.db.locked(), &pedido).expect("a pessoa liga");
    for ponte in [&mut a, &mut b] {
        ponte
            .call(
                "donnaGuardRuntime",
                json!({"method":"tools/call","validateOnly":true,"beginRequest":true,"params":{"name":"inicio"}}),
            )
            .await
            .expect("pedido registrado no Hub");
    }
    let mut envios = 0;
    let mut tentar = async |ponte: &mut Bridge| {
        ponte.params = json!({"name": "list_issues", "arguments": {}});
        let passou = ponte.admit(&Value::Null, "tools/call").await.is_ok();
        envios += usize::from(passou);
        passou
    };
    assert!(tentar(&mut a).await && tentar(&mut b).await, "ligada e com inventário: as duas pontes enviam");
    // Cada ponte anota a geração antes de o tools/list sair; depois o fornecedor avisa a mudança.
    ferramentas::iniciar_descoberta(store, &a.id, "fixture").unwrap();
    ferramentas::iniciar_descoberta(store, &b.id, "fixture").unwrap();
    ferramentas::invalidar(store, "fixture").unwrap();
    assert!(!tentar(&mut a).await && !tentar(&mut b).await, "aviso de lista mudada: nada sai");
    // A resposta da ponte B, pedida antes do aviso, chega atrasada: não reabre.
    let atrasada = ferramentas::encerrar_descoberta(&b.id, "fixture");
    ferramentas::registrar(store, &conta, &tools, Some(&nomes), true, atrasada).unwrap();
    assert!(!tentar(&mut a).await && !tentar(&mut b).await, "resposta anterior ao aviso não revalida o inventário");
    // A ponte A pede de novo depois do aviso, mas a resposta mediada vem retida (erro): só invalida.
    ferramentas::iniciar_descoberta(store, &a.id, "fixture").unwrap();
    let retida = ferramentas::encerrar_descoberta(&a.id, "fixture");
    ferramentas::registrar(store, &conta, &tools, None, true, retida).unwrap();
    assert!(!tentar(&mut a).await && !tentar(&mut b).await, "descoberta retida não vale como lista");
    // A resposta da ponte A, sem ter sido anotada (ou já consumida), também não instala nada.
    let sem_nota = ferramentas::encerrar_descoberta(&a.id, "fixture");
    ferramentas::registrar(store, &conta, &tools, Some(&nomes), true, sem_nota).unwrap();
    assert!(!tentar(&mut a).await && !tentar(&mut b).await, "sem geração anotada nada instala");
    let ligadas: i64 =
        store.db.locked().query_row("SELECT COUNT(*) FROM mexerica_ferramenta_liberacoes", [], |r| r.get(0)).unwrap();
    assert_eq!(ligadas, 1, "a liberação da pessoa continua guardada");
    // Uma descoberta pedida depois do aviso, completa e mediada, reabre: as duas pontes voltam a enviar.
    ferramentas::iniciar_descoberta(store, &a.id, "fixture").unwrap();
    let nova = ferramentas::encerrar_descoberta(&a.id, "fixture");
    ferramentas::registrar(store, &conta, &tools, Some(&nomes), true, nova).unwrap();
    assert!(tentar(&mut a).await && tentar(&mut b).await);
    // O próprio fornecedor responde com erro à descoberta seguinte: a lista anterior deixa de valer nas duas pontes.
    ferramentas::iniciar_descoberta(store, &a.id, "fixture").unwrap();
    let erro = json!({"jsonrpc": "2.0", "id": 9, "error": {"code": -32000, "message": "indisponível"}});
    assert!(ferramentas::lista_do_frame(store, "fixture", &erro).is_err());
    assert!(!tentar(&mut a).await && !tentar(&mut b).await, "descoberta com erro do fornecedor fecha a admissão");
    let nova = ferramentas::encerrar_descoberta(&a.id, "fixture");
    ferramentas::registrar(store, &conta, &tools, Some(&nomes), true, nova).unwrap();
    assert!(!tentar(&mut a).await && !tentar(&mut b).await, "a resposta anotada antes da falha não reabre");
    ferramentas::iniciar_descoberta(store, &a.id, "fixture").unwrap();
    let nova = ferramentas::encerrar_descoberta(&a.id, "fixture");
    ferramentas::registrar(store, &conta, &tools, Some(&nomes), true, nova).unwrap();
    assert!(tentar(&mut a).await && tentar(&mut b).await);
    assert_eq!(envios, 6, "só as admissões com inventário vigente enviam");
}
