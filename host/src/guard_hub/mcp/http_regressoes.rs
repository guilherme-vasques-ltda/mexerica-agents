//! Regressões de contrato de transporte; fixtures próprias, sem implementação de SDK.
use super::*;
use futures::{StreamExt, stream};
use std::sync::Mutex;
use tokio::sync::{Notify, broadcast};

struct LegacyPeer {
    remote: Server,
    events: broadcast::Sender<String>,
    opens: Arc<AtomicUsize>,
    posts: Arc<AtomicUsize>,
    protocol: Arc<Mutex<Vec<Value>>>,
    owner: Arc<Mutex<Option<Arc<Fixture>>>>,
    posted_reply: Arc<Notify>,
    finish_ack: Arc<Notify>,
}

async fn legacy_peer(wait_ping: bool) -> LegacyPeer {
    let (events, _) = broadcast::channel::<String>(8);
    let outgoing = events.clone();
    let replies = events.clone();
    let opens = Arc::new(AtomicUsize::new(0));
    let opened = opens.clone();
    let posts = Arc::new(AtomicUsize::new(0));
    let posted = posts.clone();
    let protocol = Arc::new(Mutex::new(Vec::<Value>::new()));
    let recorded = protocol.clone();
    let owner = Arc::new(Mutex::new(None::<Arc<Fixture>>));
    let observed = owner.clone();
    let ack = Arc::new(Notify::new());
    let posted_reply = Arc::new(Notify::new());
    let sent_reply = posted_reply.clone();
    let finish_ack = Arc::new(Notify::new());
    let finish = finish_ack.clone();
    let remote = server(
        Router::new()
            .route(
                "/mcp",
                routing::post(|| async { StatusCode::METHOD_NOT_ALLOWED }).get(move || {
                    opened.fetch_add(1, Ordering::SeqCst);
                    let receiver = outgoing.subscribe();
                    async move {
                        let endpoint = stream::once(async {
                            Ok::<_, std::io::Error>("event: endpoint\ndata: /post\n\n".to_owned())
                        });
                        let events = stream::unfold(receiver, |mut receiver| async move {
                            receiver.recv().await.ok().map(|event| (Ok::<_, std::io::Error>(event), receiver))
                        });
                        AxumResponse::builder()
                            .header("content-type", "text/event-stream")
                            .body(Body::from_stream(endpoint.chain(events)))
                            .unwrap()
                    }
                }),
            )
            .route(
                "/post",
                routing::post(move |headers: HeaderMap, Json(frame): Json<Value>| {
                    let events = replies.clone();
                    let posted = posted.clone();
                    let protocol = recorded.clone();
                    let owner = observed.clone();
                    let ack = ack.clone();
                    let sent_reply = sent_reply.clone();
                    let finish = finish.clone();
                    async move {
                        // Servidor estrito do contrato MCP; não alega reproduzir 406 do Python atual.
                        let accept = headers.get("accept").unwrap().to_str().unwrap();
                        assert!(accept.contains("application/json") && accept.contains("text/event-stream"));
                        if frame.get("method").is_none() {
                            let fixture = owner.locked().clone().unwrap();
                            let (receipts, _) = fixture.hub.store.guard_test_snapshot().unwrap();
                            let previous = protocol.locked().len();
                            assert!(receipts.iter().filter(|e| e.contains("server-request")).count() > previous);
                            protocol.locked().push(frame.clone());
                            if frame["id"] == "post-ping" {
                                ack.notify_one();
                            }
                        } else {
                            posted.fetch_add(1, Ordering::SeqCst);
                            if frame["method"] != "notifications/initialized" {
                                if wait_ping && frame["method"] == "tools/call" {
                                    events.send(sse(&json!({"id":"post-ping","method":"ping"}))).unwrap();
                                    ack.notified().await;
                                }
                                events.send(": ping - fixture\n\n".to_owned() + &sse(&reply(&frame))).unwrap();
                                if frame["params"]["holdAck"] == true {
                                    sent_reply.notify_one();
                                    finish.notified().await;
                                }
                            }
                        }
                        (StatusCode::ACCEPTED, if frame["params"]["emptyAck"] == true { "" } else { "Accepted" })
                    }
                }),
            ),
    )
    .await;
    LegacyPeer { remote, events, opens, posts, protocol, owner, posted_reply, finish_ack }
}

#[tokio::test]
async fn accepted_texto_mesmo_get_75_pedidos_keepalive_e_ping_antes_do_ack() {
    let peer = legacy_peer(true).await;
    let fixture = Fixture::new(&peer.remote.url);
    *peer.owner.locked() = Some(fixture.clone());
    let mut session = Session::new(fixture.mediation()).unwrap();
    session.exchange(request(1, "initialize")).await.unwrap();
    session.exchange(json!({"jsonrpc":"2.0","method":"notifications/initialized"})).await.unwrap();
    for id in 2..77 {
        // Relógio da janela controlado: não depende de sleeps nem força um flood legítimo.
        session.rate.locked().started = tokio::time::Instant::now() - Duration::from_secs(1);
        let method = if id == 2 { "tools/call" } else { "tools/list" };
        let response = tokio::time::timeout(Duration::from_secs(3), session.exchange(request(id, method)))
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(response["id"], id);
        assert!(response["result"].is_object());
    }
    assert_eq!(peer.opens.load(Ordering::SeqCst), 1);
    assert_eq!(peer.posts.load(Ordering::SeqCst), 77);
    assert_eq!(peer.protocol.locked()[0]["id"], "post-ping");
    assert_eq!(peer.protocol.locked()[0]["result"], json!({}));
    session.close();
}

#[tokio::test]
async fn idle_legado_observa_ping_e_recusa_sem_proximo_stdin() {
    let peer = legacy_peer(false).await;
    let fixture = Fixture::new(&peer.remote.url);
    *peer.owner.locked() = Some(fixture.clone());
    let mut session = Session::new(fixture.mediation()).unwrap();
    let (engine, proxy) = tokio::io::duplex(8192);
    let (input, output) = tokio::io::split(proxy);
    let (engine_in, mut engine_out) = tokio::io::split(engine);
    let mut lines = BufReader::new(engine_in).lines();
    let task = tokio::spawn(async move { mcp::ponte_remota::relay(input, output, &mut session).await });
    engine_out.write_all(format!("{}\n", request(1, "initialize")).as_bytes()).await.unwrap();
    assert!(lines.next_line().await.unwrap().unwrap().contains("result"));
    peer.events
        .send(
            sse(&json!({"method":"notifications/message","params":{"data":"alphabeticaltoken"}}))
                + &sse(&json!({"id":"idle-ping","method":"ping"}))
                + &sse(&json!({"id":"idle-sampling","method":"sampling/createMessage"})),
        )
        .unwrap();
    // Uma barreira no protocolo evita espera temporal e prova atendimento sem outra entrada.
    let wait = async {
        loop {
            if peer.protocol.locked().len() == 2 {
                break;
            }
            tokio::task::yield_now().await;
        }
    };
    tokio::time::timeout(Duration::from_secs(3), wait).await.unwrap();
    assert_eq!(peer.protocol.locked()[0]["result"], json!({}));
    assert_eq!(peer.protocol.locked()[1]["error"]["code"], -32601);
    assert_eq!(peer.posts.load(Ordering::SeqCst), 1);
    engine_out.shutdown().await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap().unwrap();
    assert!(lines.next_line().await.unwrap().is_none());
}

#[tokio::test]
async fn streamable_ack_textual_notificacao_e_falha_recuperavel_nao_matam_ponte() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let remote = server(Router::new().route(
        "/mcp",
        routing::post(move |Json(frame): Json<Value>| {
            let counted = counted.clone();
            async move {
                if frame["method"] == "notifications/initialized" {
                    return AxumResponse::builder().status(202).body(Body::from("Accepted")).unwrap();
                }
                if frame["method"] == "tools/call" && counted.fetch_add(1, Ordering::SeqCst) == 0 {
                    return AxumResponse::builder().status(503).body(Body::from("alphabeticaltoken")).unwrap();
                }
                AxumResponse::new(Body::from(reply(&frame).to_string()))
            }
        }),
    ))
    .await;
    let fixture = Fixture::new(&remote.url);
    let mut session = Session::new(fixture.mediation()).unwrap();
    let (engine, proxy) = tokio::io::duplex(8192);
    let (input, output) = tokio::io::split(proxy);
    let (engine_in, mut engine_out) = tokio::io::split(engine);
    let mut lines = BufReader::new(engine_in).lines();
    let task = tokio::spawn(async move { mcp::ponte_remota::relay(input, output, &mut session).await });
    engine_out.write_all(format!("{}\n", request(1, "initialize")).as_bytes()).await.unwrap();
    lines.next_line().await.unwrap().unwrap();
    engine_out.write_all(format!("{{\"jsonrpc\":\"2.0\",\"method\":\"notifications/cancelled\",\"params\":{{\"requestId\":999}}}}\n{{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}}\n{}\n",request(2,"tools/call")).as_bytes()).await.unwrap();
    let error = lines.next_line().await.unwrap().unwrap();
    assert_eq!(serde_json::from_str::<Value>(&error).unwrap()["id"], 2);
    assert!(error.contains("error") && !error.contains("alphabeticaltoken"));
    assert_eq!(calls.load(Ordering::SeqCst), 1); // Falha não reenvia automaticamente.
    engine_out.write_all(format!("{}\n", request(3, "tools/list")).as_bytes()).await.unwrap();
    let discovery = lines.next_line().await.unwrap().unwrap();
    assert_eq!(serde_json::from_str::<Value>(&discovery).unwrap()["id"], 3);
    assert!(discovery.contains("tools"));
    fixture.hub.store.guard_context_remove(&fixture.context.id).unwrap();
    engine_out.write_all(format!("{}\n", request(4, "tools/list")).as_bytes()).await.unwrap();
    assert!(lines.next_line().await.unwrap().unwrap().contains("error"));
    tokio::time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap().unwrap();
    assert!(lines.next_line().await.unwrap().is_none());
}

#[tokio::test]
async fn cursor_quatro_listagens_duas_paginas_erro_explicito_e_exposicao() {
    let remote = server(Router::new().route(
        "/mcp",
        routing::post(|Json(frame): Json<Value>| async move {
            if frame["method"] == "initialize" {
                return Json(reply(&frame));
            }
            let key = match frame["method"].as_str().unwrap() {
                "tools/list" => "tools",
                "resources/list" => "resources",
                "resources/templates/list" => "resourceTemplates",
                "prompts/list" => "prompts",
                _ => unreachable!(),
            };
            let name = if frame["params"].get("cursor").is_some() { "page_b" } else { "page_a" };
            let item = match key {
                "tools" => json!({"name":name,"inputSchema":{"type":"object"}}),
                "resources" => json!({"name":name,"uri":"fixture:page"}),
                "resourceTemplates" => json!({"name":name,"uriTemplate":"fixture:{id}"}),
                _ => json!({"name":name}),
            };
            let mut result = json!({key:[item]});
            if let Some(cursor) = frame["params"].get("fixtureCursor") {
                result["nextCursor"] = cursor.clone();
            } else if let Some(cursor) = frame["params"].get("cursor") {
                assert_eq!(cursor, "page+/=:%-two");
            } else {
                result["nextCursor"] = json!("page+/=:%-two");
            }
            Json(json!({"jsonrpc":"2.0","id":frame["id"],"result":result}))
        }),
    ))
    .await;
    let fixture = Fixture::new(&remote.url);
    let mut session = Session::new(fixture.mediation()).unwrap();
    session.exchange(request(1, "initialize")).await.unwrap();
    for method in ["tools/list", "resources/list", "resources/templates/list", "prompts/list"] {
        let first = session.exchange(request(2, method)).await.unwrap().unwrap();
        assert_eq!(first["result"]["_meta"]["donnabot/guard"]["origin"], "external");
        assert_eq!(first["result"]["nextCursor"], "page+/=:%-two");
        assert!(first["result"].to_string().contains("page_a"));
        if method == "tools/list" {
            memory_without_owner_transport_is_refused(&fixture).await;
        }
        let mut next = request(3, method);
        next["params"]["cursor"] = first["result"]["nextCursor"].clone();
        let last = session.exchange(next).await.unwrap().unwrap();
        assert!(last["result"].get("nextCursor").is_none());
        assert!(last["result"].to_string().contains("page_b"));
        for cursor in [
            json!("alphabeticaltoken"),
            json!("oldalphabeticaltoken"),
            json!("<donnabot-external>"),
            json!(7),
            json!("x".repeat(4097)),
        ] {
            let mut next = request(4, method);
            next["params"]["fixtureCursor"] = cursor;
            let error = session.exchange(next).await.unwrap().unwrap();
            assert!(error.get("result").is_none());
            assert!(error["error"]["message"].as_str().unwrap().contains("descoberta incompleta"));
            assert!(!error.to_string().contains("alphabeticaltoken"));
            assert_eq!(error["error"]["data"]["_meta"]["donnabot/guard"]["coverage"], "unsupported");
        }
    }
}

#[tokio::test]
async fn dois_frames_40k_no_mesmo_chunk_preservam_limite_por_frame() {
    let first = sse(&json!({"method":"notifications/message","params":{"data":"x".repeat(40*1024)}}));
    let second = sse(&json!({"id":1,"result":{"content":[{"type":"text","text":"y".repeat(40*1024)}]}}));
    let wire = first.clone() + &second;
    let response = Response::from(axum::http::Response::new(reqwest::Body::from(wire)));
    let mut events = Events::new(response, Arc::new(Mutex::new(Rate::default())));
    assert_eq!(events.next().await.unwrap().unwrap(), first);
    assert_eq!(events.next().await.unwrap().unwrap(), second);
    assert!(events.next().await.unwrap().is_none());
    let large =
        Response::from(axum::http::Response::new(reqwest::Body::from(sse(&json!({"text":"x".repeat(MAX_FRAME)})))));
    assert!(Events::new(large, Arc::new(Mutex::new(Rate::default()))).next().await.is_err());
    let remote = server(Router::new().route(
        "/mcp",
        routing::post(|Json(frame): Json<Value>| async move {
            if frame["method"] == "initialize" {
                return AxumResponse::new(Body::from(reply(&frame).to_string()));
            }
            let notification = sse(&json!({"method":"notifications/message","params":{"data":"x".repeat(40*1024)}}));
            let response =
                sse(&json!({"jsonrpc":"2.0","id":frame["id"],"result":{"content":[{"type":"text","text":"y".repeat(40*1024)}]}}));
            AxumResponse::builder()
                .header("content-type", "text/event-stream")
                .body(Body::from(notification + &response))
                .unwrap()
        }),
    ))
    .await;
    let fixture = Fixture::new(&remote.url);
    let mut session = Session::new(fixture.mediation()).unwrap();
    session.exchange(request(1, "initialize")).await.unwrap();
    let result = session.exchange(request(2, "tools/call")).await.unwrap().unwrap();
    assert!(result["result"]["content"][0]["text"].as_str().unwrap().contains(&"y".repeat(40 * 1024)));
}

#[test]
fn janela_monotonica_limita_flood_e_renova_sem_orcamento_vitalicio() {
    let start = tokio::time::Instant::now();
    let mut rate = Rate::default();
    rate.started = start;
    rate.charge_at(start, 256 * 1024, 64, 8).unwrap();
    assert!(rate.charge_at(start + Duration::from_millis(999), 1, 1, 1).is_err());
    rate.charge_at(start + Duration::from_secs(1), 1, 1, 1).unwrap();
    assert!(rate.charge_at(start + Duration::from_secs(1), 0, 0, 8).is_err());
    rate.charge_at(start + Duration::from_secs(2), 0, 64, 0).unwrap();
    assert!(rate.charge_at(start + Duration::from_secs(2), 0, 1, 0).is_err());
    rate.charge_at(start + Duration::from_secs(3), 256 * 1024, 0, 0).unwrap();
    assert!(rate.charge_at(start + Duration::from_secs(3), 1, 0, 0).is_err());
}

struct IdleHeld {
    guard: Guard,
    arrived: Arc<Notify>,
    release: Arc<Notify>,
}

impl Mediation for IdleHeld {
    async fn target(&mut self, method: &str, stale: bool) -> Result<Target> {
        self.guard.target(method, stale).await
    }

    async fn observe(&mut self, frame: Value, method: &str) -> Result<Option<Value>> {
        let result = self.guard.observe(frame, method).await?;
        if method == "server-request" {
            self.arrived.notify_one();
            self.release.notified().await;
        }
        Ok(result)
    }
}

#[tokio::test]
async fn eof_cancel_e_revogacao_durante_idle_nao_enviam_ferramenta_ou_ping() {
    for mode in ["eof", "cancel", "revoked"] {
        let peer = legacy_peer(false).await;
        let fixture = Fixture::new(&peer.remote.url);
        *peer.owner.locked() = Some(fixture.clone());
        let arrived = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let guard = IdleHeld { guard: fixture.mediation(), arrived: arrived.clone(), release: release.clone() };
        let mut session = Session::new(guard).unwrap();
        let (engine, proxy) = tokio::io::duplex(8192);
        let (input, output) = tokio::io::split(proxy);
        let (engine_in, mut engine_out) = tokio::io::split(engine);
        let mut lines = BufReader::new(engine_in).lines();
        let task = tokio::spawn(async move { mcp::ponte_remota::relay(input, output, &mut session).await });
        engine_out.write_all(format!("{}\n", request(1, "initialize")).as_bytes()).await.unwrap();
        lines.next_line().await.unwrap().unwrap();
        peer.events.send(sse(&json!({"id":"idle-held","method":"ping"}))).unwrap();
        tokio::time::timeout(Duration::from_secs(2), arrived.notified()).await.unwrap();
        match mode {
            "cancel" => {
                engine_out.write_all(format!("{}\n{{\"jsonrpc\":\"2.0\",\"method\":\"notifications/cancelled\",\"params\":{{\"requestId\":2}}}}\n", request(2,"tools/call")).as_bytes()).await.unwrap();
            }
            "revoked" => {
                fixture.hub.store.guard_context_remove(&fixture.context.id).unwrap();
                release.notify_one();
            }
            _ => engine_out.shutdown().await.unwrap(),
        }
        tokio::time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap().unwrap();
        assert_eq!(peer.posts.load(Ordering::SeqCst), 1);
        assert_eq!(peer.protocol.locked().len(), 0);
        assert!(lines.next_line().await.unwrap().is_none());
    }
}

#[tokio::test]
async fn comentarios_nao_consumem_frames_de_resposta_mas_flood_tem_limite() {
    let fixture = Fixture::new("http://127.0.0.1:1/mcp");
    let mut guard = fixture.mediation();
    guard.target("initialize", false).await.unwrap();
    let mut session = Session::new(guard).unwrap();
    session.budget = Some(Budget::default());
    for _ in 0..70 {
        session.event(": ping\n\n", None, "tools/list").await.unwrap();
    }
    let id = json!("pending");
    let data = sse(&json!({"jsonrpc":"2.0","id":id,"result":{"tools":[]}}));
    assert!(session.event(&data, Some(&id), "tools/list").await.unwrap().is_some());
    assert_eq!(session.budget.as_ref().unwrap().frames, 1);
    session.budget = Some(Budget::default());
    let noise = sse(&json!({"method":"notifications/message","params":{"data":"dado"}}));
    for _ in 0..64 {
        session.event(&noise, None, "tools/list").await.unwrap();
    }
    assert!(session.event(&noise, None, "tools/list").await.is_err());
    assert_eq!(session.failure, Failure::Fatal);
    let response = Response::from(axum::http::Response::new(reqwest::Body::from(": ping\n\n".repeat(65))));
    let mut events = Events::new(response, Arc::new(Mutex::new(Rate::default())));
    for _ in 0..64 {
        events.next().await.unwrap().unwrap();
    }
    assert!(events.next().await.is_err());
}

#[tokio::test]
async fn get_ja_observado_nao_e_entregue_se_contexto_revoga_antes_do_ack_vazio() {
    let peer = legacy_peer(false).await;
    let fixture = Fixture::new(&peer.remote.url);
    *peer.owner.locked() = Some(fixture.clone());
    let mut session = Session::new(fixture.mediation()).unwrap();
    session.exchange(request(1, "initialize")).await.unwrap();
    let mut call = request(2, "tools/list");
    call["params"]["holdAck"] = json!(true);
    call["params"]["emptyAck"] = json!(true);
    let task = tokio::spawn(async move { session.exchange(call).await });
    tokio::time::timeout(Duration::from_secs(2), peer.posted_reply.notified()).await.unwrap();
    let observed = async {
        loop {
            if fixture.hub.store.guard_test_snapshot().unwrap().0.iter().any(|e| e.contains("tools/list")) {
                break;
            }
            tokio::task::yield_now().await;
        }
    };
    tokio::time::timeout(Duration::from_secs(2), observed).await.unwrap();
    fixture.hub.store.guard_context_remove(&fixture.context.id).unwrap();
    peer.finish_ack.notify_one();
    let result = tokio::time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap();
    assert_eq!(result.unwrap_err(), Failure::Fatal);
}

struct FailureObservationHeld {
    runtime_started: Arc<Notify>,
    retention_started: Arc<Notify>,
}

impl Mediation for FailureObservationHeld {
    async fn target(&mut self, _method: &str, _stale: bool) -> Result<Target> {
        self.runtime_started.notify_one();
        tokio::time::sleep(Duration::from_secs(88)).await;
        // Validação local de origem falha após 88 s; nenhum HTTP é enviado.
        Ok(Target { url: Url::parse("fixture://deadline-private/mcp")?, headers: HeaderMap::new() })
    }

    async fn observe(&mut self, _frame: Value, method: &str) -> Result<Option<Value>> {
        assert_eq!(method, "invalid");
        self.retention_started.notify_one();
        std::future::pending().await
    }
}

#[tokio::test(start_paused = true)]
async fn prazo_total_visto_pelo_motor_inclui_retencao_bloqueada_da_falha() {
    use tokio::time::{Instant, advance};
    let runtime_started = Arc::new(Notify::new());
    let retention_started = Arc::new(Notify::new());
    let guard = FailureObservationHeld {
        runtime_started: runtime_started.clone(),
        retention_started: retention_started.clone(),
    };
    let mut session = Session::new(guard).unwrap();
    let (engine, proxy) = tokio::io::duplex(8192);
    let (input, output) = tokio::io::split(proxy);
    let (engine_in, mut engine_out) = tokio::io::split(engine);
    let mut lines = BufReader::new(engine_in).lines();
    let started = Instant::now();
    let relay = tokio::spawn(async move { mcp::ponte_remota::relay(input, output, &mut session).await });
    let reader = tokio::spawn(async move {
        let line = lines.next_line().await.unwrap().unwrap();
        (Instant::now(), serde_json::from_str::<Value>(&line).unwrap())
    });
    engine_out.write_all(format!("{}\n", request(1, "initialize")).as_bytes()).await.unwrap();
    runtime_started.notified().await;
    advance(Duration::from_secs(88)).await;
    retention_started.notified().await;
    assert_eq!(Instant::now() - started, Duration::from_secs(88));
    advance(Duration::from_secs(1)).await;
    assert!(!reader.is_finished());
    advance(Duration::from_secs(1)).await;
    let (delivered, response) = tokio::time::timeout(Duration::from_secs(1), reader).await.unwrap().unwrap();
    assert_eq!(delivered - started, Duration::from_secs(90));
    assert_eq!(
        response,
        json!({"jsonrpc":"2.0","id":1,"error":{"code":-32603,
        "message":"Retorno MCP retido; confira o efeito antes de nova tentativa. A ponte não repete o pedido."}})
    );
    tokio::time::timeout(Duration::from_secs(1), relay).await.unwrap().unwrap().unwrap();
}
