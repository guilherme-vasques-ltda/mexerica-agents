//! M1: versão MCP honesta e header canônico por Session; fixtures locais estritas.
use super::*;
use futures::{StreamExt, stream};
use std::sync::Mutex;

type Log = Arc<Mutex<Vec<(String, Vec<String>, Vec<String>)>>>;

fn record(log: &Log, kind: String, headers: &HeaderMap) {
    let all =
        |name: &str| -> Vec<String> { headers.get_all(name).iter().map(|v| v.to_str().unwrap().to_owned()).collect() };
    log.locked().push((kind, all("mcp-protocol-version"), all("mcp-session-id")));
}

fn expected(rows: &[(&str, &[&str], &[&str])]) -> Vec<(String, Vec<String>, Vec<String>)> {
    let owned = |values: &[&str]| values.iter().map(|v| (*v).to_owned()).collect();
    rows.iter().map(|(kind, version, session)| ((*kind).to_owned(), owned(version), owned(session))).collect()
}

/// Configuração conflitante: valores MCP reservados duplicados e em outra caixa.
struct Conflito(Guard);

impl Mediation for Conflito {
    async fn target(&mut self, method: &str, stale: bool) -> Result<Target> {
        let mut target = self.0.target(method, stale).await?;
        for (name, value) in [
            ("MCP-Protocol-Version", "1999-01-01"),
            ("mcp-protocol-version", "2024-11-05"),
            ("Mcp-Session-Id", "sessao-alheia"),
        ] {
            target.headers.append(
                reqwest::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                reqwest::header::HeaderValue::from_static(value),
            );
        }
        Ok(target)
    }

    async fn observe(&mut self, frame: Value, method: &str) -> Result<Option<Value>> {
        self.0.observe(frame, method).await
    }
}

fn initialize(name: &str, version: Option<Value>) -> Value {
    let mut frame = request(1, "initialize");
    frame["params"]["clientInfo"] = json!({"name":name,"version":"1"});
    if let Some(version) = version {
        frame["params"]["fixtureVersion"] = version;
    }
    frame
}

fn initialized() -> Value {
    json!({"jsonrpc":"2.0","method":"notifications/initialized"})
}

fn sse_response(body: String) -> AxumResponse {
    AxumResponse::builder().header("content-type", "text/event-stream").body(Body::from(body)).unwrap()
}

#[tokio::test]
async fn duas_sessions_na_mesma_url_mantem_versao_e_um_header_canonico_cada() {
    let log: Log = Arc::default();
    let seen = log.clone();
    let remote = server(Router::new().route(
        "/mcp",
        routing::post(move |headers: HeaderMap, Json(frame): Json<Value>| {
            let seen = seen.clone();
            async move {
                let Some(method) = frame["method"].as_str() else {
                    record(&seen, format!("reply:{}", frame["id"].as_str().unwrap()), &headers);
                    return AxumResponse::builder().status(202).body(Body::empty()).unwrap();
                };
                record(&seen, method.to_owned(), &headers);
                match method {
                    "notifications/initialized" => {
                        AxumResponse::builder().status(202).body(Body::from("Accepted")).unwrap()
                    }
                    "initialize" => {
                        let name = frame["params"]["clientInfo"]["name"].as_str().unwrap().to_owned();
                        // Ping reverso antes do resultado: nenhuma versão aceita ainda.
                        let body = sse(&json!({"id":format!("pre-{name}"),"method":"ping"})) + &sse(&reply(&frame));
                        let mut response = sse_response(body);
                        response.headers_mut().insert("mcp-session-id", format!("sessao-{name}").parse().unwrap());
                        response
                    }
                    // Ping depois do resultado aceito, na resposta SSE da descoberta.
                    _ => sse_response(sse(&json!({"id":"pos","method":"ping"})) + &sse(&reply(&frame))),
                }
            }
        }),
    ))
    .await;
    // Dois canais para a mesma URL; estado só na Session, nunca global/URL/config.
    let (first, second) = (Fixture::new(&remote.url), Fixture::new(&remote.url));
    let mut a = Session::new(Conflito(first.mediation())).unwrap();
    let mut b = Session::new(Conflito(second.mediation())).unwrap();
    let ra = a.exchange(initialize("a", None)).await.unwrap().unwrap();
    let rb = b.exchange(initialize("b", Some(json!("2025-03-26")))).await.unwrap().unwrap();
    assert_eq!(ra["result"]["protocolVersion"], "2025-06-18");
    // Resposta alternativa válida do fornecedor chega intacta ao motor.
    assert_eq!(rb["result"]["protocolVersion"], "2025-03-26");
    assert!(a.exchange(initialized()).await.unwrap().is_none());
    assert!(b.exchange(initialized()).await.unwrap().is_none());
    assert!(a.exchange(request(2, "tools/list")).await.unwrap().unwrap()["result"]["tools"].is_array());
    assert!(b.exchange(request(2, "tools/list")).await.unwrap().unwrap()["result"]["tools"].is_array());
    let (v1, v2) = (["2025-06-18"].as_slice(), ["2025-03-26"].as_slice());
    let (sa, sb) = (["sessao-a"].as_slice(), ["sessao-b"].as_slice());
    assert_eq!(
        *log.locked(),
        expected(&[
            ("initialize", &[], &[]),
            ("reply:pre-a", &[], sa),
            ("initialize", &[], &[]),
            ("reply:pre-b", &[], sb),
            ("notifications/initialized", v1, sa),
            ("notifications/initialized", v2, sb),
            ("tools/list", v1, sa),
            ("reply:pos", v1, sa),
            ("tools/list", v2, sb),
            ("reply:pos", v2, sb),
        ])
    );
}

#[tokio::test]
async fn legado_aceita_versao_do_get_antes_do_ack_sem_deadlock_e_sem_liberar_descoberta() {
    let log: Log = Arc::default();
    let (events, _) = tokio::sync::broadcast::channel::<String>(8);
    let ack = Arc::new(tokio::sync::Notify::new());
    let (seen_post, seen_get, seen_legacy) = (log.clone(), log.clone(), log.clone());
    let (subscribe, publish, acked, waiting) = (events.clone(), events.clone(), ack.clone(), ack.clone());
    let remote = server(
        Router::new()
            .route(
                "/mcp",
                routing::post(move |headers: HeaderMap| {
                    let seen = seen_post.clone();
                    async move {
                        record(&seen, "post-inicial".into(), &headers);
                        StatusCode::METHOD_NOT_ALLOWED
                    }
                })
                .get(move |headers: HeaderMap| {
                    record(&seen_get, "get".into(), &headers);
                    let receiver = subscribe.subscribe();
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
                    let (seen, events, acked, waiting) =
                        (seen_legacy.clone(), publish.clone(), acked.clone(), waiting.clone());
                    async move {
                        let Some(method) = frame["method"].as_str() else {
                            let id = frame["id"].as_str().unwrap();
                            record(&seen, format!("reply:{id}"), &headers);
                            if id == "pos" {
                                acked.notify_one();
                            }
                            return (StatusCode::ACCEPTED, "Accepted");
                        };
                        record(&seen, method.to_owned(), &headers);
                        match method {
                            "initialize" => {
                                events.send(sse(&json!({"id":"pre","method":"ping"}))).unwrap();
                                events.send(sse(&reply(&frame))).unwrap();
                                events.send(sse(&json!({"id":"pos","method":"ping"}))).unwrap();
                                // O ACK do POST só sai depois do ping que já usa a versão aceita.
                                waiting.notified().await;
                            }
                            "tools/list" => {
                                events.send(sse(&reply(&frame))).unwrap();
                            }
                            _ => {}
                        }
                        (StatusCode::ACCEPTED, "Accepted")
                    }
                }),
            ),
    )
    .await;
    let fixture = Fixture::new(&remote.url);
    let mut session = Session::new(Conflito(fixture.mediation())).unwrap();
    let init =
        tokio::time::timeout(Duration::from_secs(5), session.exchange(initialize("legado", Some(json!("2025-03-26")))))
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
    assert!(session.exchange(initialized()).await.unwrap().is_none());
    assert!(session.exchange(request(2, "tools/list")).await.unwrap().unwrap()["result"]["tools"].is_array());
    let v = ["2025-03-26"].as_slice();
    assert_eq!(
        *log.locked(),
        expected(&[
            ("post-inicial", &[], &[]),
            ("get", &[], &[]),
            ("initialize", &[], &[]),
            ("reply:pre", &[], &[]),
            ("reply:pos", v, &[]),
            ("notifications/initialized", v, &[]),
            ("tools/list", v, &[]),
        ])
    );
    session.close();
}

#[tokio::test]
async fn versao_invalida_e_fatal_depois_do_recibo_e_nada_mais_vai_a_rede() {
    let posts = Arc::new(AtomicUsize::new(0));
    let counted = posts.clone();
    let remote = server(Router::new().route(
        "/mcp",
        routing::post(move |Json(frame): Json<Value>| {
            counted.fetch_add(1, Ordering::SeqCst);
            async move { Json(reply(&frame)) }
        }),
    ))
    .await;
    for (version, error) in [
        (json!("1999-01-01"), Erro::NaoSuportada),
        (json!("omit"), Erro::RespostaInvalida),
        (json!(20_250_618), Erro::RespostaInvalida),
    ] {
        posts.store(0, Ordering::SeqCst);
        let fixture = Fixture::new(&remote.url);
        let mut session = Session::new(fixture.mediation()).unwrap();
        // Pedido local sem versão: -32602 sem rede e sem fabricar 2025-06-18.
        let mut missing = request(1, "initialize");
        missing["params"].as_object_mut().unwrap().remove("protocolVersion");
        let refused = session.exchange(missing).await.unwrap().unwrap();
        assert_eq!(refused, json!({"jsonrpc":"2.0","id":1,"error":Erro::PedidoInvalido.json()}));
        assert_eq!(posts.load(Ordering::SeqCst), 0);
        assert_eq!(session.exchange(initialize("x", Some(version))).await.unwrap_err(), Failure::Incompatible(error));
        // Recibo do initialize gravado antes do erro técnico; sem valor remoto no recibo.
        let (receipts, _) = fixture.hub.store.guard_test_snapshot().unwrap();
        assert!(receipts.iter().any(|e| e.contains("\"initialize\"")));
        assert!(receipts.iter().all(|e| !e.contains("1999-01-01")));
        assert!(session.exchange(initialized()).await.is_err());
        assert!(session.exchange(request(2, "tools/list")).await.is_err());
        assert_eq!(posts.load(Ordering::SeqCst), 1);
    }
    // Na ponte: só o -32603 fixo e encerramento, sem "Retorno retido" nem eco remoto.
    let fixture = Fixture::new(&remote.url);
    let mut session = Session::new(fixture.mediation()).unwrap();
    let (engine, proxy) = tokio::io::duplex(8192);
    let (input, output) = tokio::io::split(proxy);
    let (engine_in, mut engine_out) = tokio::io::split(engine);
    let mut lines = BufReader::new(engine_in).lines();
    let task = tokio::spawn(async move { mcp::ponte_remota::relay(input, output, &mut session).await });
    engine_out.write_all(format!("{}\n", initialize("x", Some(json!("1999-01-01")))).as_bytes()).await.unwrap();
    let line = lines.next_line().await.unwrap().unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&line).unwrap(),
        json!({"jsonrpc":"2.0","id":1,"error":Erro::NaoSuportada.json()})
    );
    tokio::time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap().unwrap();
    assert!(lines.next_line().await.unwrap().is_none());
}

#[tokio::test]
async fn fallback_legado_so_com_404_405_no_initialize_e_nunca_com_400_ou_erro_200() {
    for (status, gets) in [(400, 0), (404, 1), (405, 1), (200, 0)] {
        let opened = Arc::new(AtomicUsize::new(0));
        let counted = opened.clone();
        let remote = server(Router::new().route(
            "/mcp",
            routing::post(move |Json(frame): Json<Value>| async move {
                let error = json!({"jsonrpc":"2.0","id":frame["id"],"error":{"code":-32602,"message":"Unsupported protocol version"}});
                (StatusCode::from_u16(status).unwrap(), Json(error))
            })
            .get(move || {
                counted.fetch_add(1, Ordering::SeqCst);
                async { StatusCode::INTERNAL_SERVER_ERROR }
            }),
        ))
        .await;
        let fixture = Fixture::new(&remote.url);
        let mut session = Session::new(fixture.mediation()).unwrap();
        let result = session.exchange(request(1, "initialize")).await;
        if status == 200 {
            // Erro JSON-RPC mediado: não negocia versão nem abre SSE.
            assert_eq!(result.unwrap().unwrap()["error"]["code"], -32602);
            assert!(session.exchange(request(2, "tools/list")).await.is_err());
        } else {
            assert!(result.is_err());
        }
        assert_eq!(opened.load(Ordering::SeqCst), gets, "HTTP {status}");
    }
}
