//! G1b D70: limpar a conversa (reset) cancela o cartão de escolha na MESMA transação do corte de contexto.
#![allow(clippy::unwrap_used)]
use super::*;
use crate::LockExt;
use crate::fork::spaces::{PERSONAL, tool_context::tests::Fixture};

#[tokio::test]
async fn limpeza_corta_o_contexto_e_cancela_o_cartao_na_mesma_transacao() {
    let f = Fixture::new();
    let chat = f.cfg.session_owner().to_owned();
    let payload = crate::fork::genui::contract::ValidatedChoice::parse(
        &json!({"version":1,"component":"choice","title":"Escolha","summary":"Resumo",
            "data":{"question":"Qual?","options":[{"id":"a","label":"A"}]}})
        .to_string(),
    )
    .unwrap();
    let shown = crate::fork::genui::flow::show(&f.hub, &f.context(), "ponte:1", &payload).unwrap();
    let id = shown["questionId"].as_str().unwrap().to_owned();
    f.hub.set_runtime(&f.cfg.id, |r| r.status = BotStatus::Idle);
    let original = f.hub.store.bot(&f.cfg.id).unwrap().unwrap().config;
    let mut actor = new_actor(f.hub.clone(), original);
    let revision = f.hub.store.space_conversation(PERSONAL, &chat).unwrap().revision;
    actor.context_control(&chat, revision, None, true).await.unwrap();
    // A retirada comum de `ask_owner` não toca nos cartões de escolha: só a transação do corte cancela.
    let (state, cut, seq): (String, i64, i64) = f
        .hub
        .store
        .db
        .locked()
        .query_row(
            "SELECT q.state,s.context_start_seq,e.seq FROM donna_genui_questions q
                JOIN donna_space_conversations s ON s.id=q.chat JOIN entries e ON e.id=q.entry_id WHERE q.id=?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(state, "cancelled");
    assert!(cut > seq, "a pergunta ficou antes do corte de contexto");
}

/// G1b C6, casos 1/2/9 do R3 com o ator real (sem processo ACP: o motor gerenciado não aceita `command` de fixture,
/// então a ACP é um id sintético; fronteira declarada no plano). Definições pelo `mcp_servers` real, Ponte e selo
/// emitidos pelo Hub, `chatCall` pelo `dispatch` → outbox → `Cmd::ContextTool` → `on_cmd` → `flow::show`, resposta
/// pela `sessionChoiceAnswer` existente. Nada de pergunta, Authority, binding ou grant semeado por SQL.
mod c6 {
    use super::*;
    use crate::api::{self, devices::Caller};
    use crate::fork::sessoes::{bindings, contexto_ferramenta as seal, encerrar, store as sessions};
    use crate::store::{Device, DeviceSource, Scope};
    use tokio::sync::mpsc::UnboundedReceiver;

    const BOT: &str = "dev";

    struct Defined {
        lane: Lane,
        slot: String,
        binding: bindings::BindingRef,
        servers: Value,
        caller: Caller,
    }

    impl Defined {
        fn chat(&self) -> &Value {
            self.servers.as_array().unwrap().iter().find(|s| s["name"] == "chat").unwrap()
        }
        fn env(&self, name: &str) -> Option<String> {
            self.chat()["env"]
                .as_array()
                .unwrap()
                .iter()
                .find(|e| e["name"] == name)
                .and_then(|e| e["value"].as_str().map(str::to_owned))
        }
        /// O corpo como `mcp::send` o monta a partir do env da definição real.
        fn body(&self, name: &str, invocation: Option<&str>) -> Value {
            let mut body = json!({"botId":BOT,"name":name,"arguments":if name == seal::CATALOG { json!({}) } else { choice() },
                "spaceId":self.env("DONNABOT_SPACE_ID").unwrap(),
                "sessionSlot":self.env(crate::guard_hub::tool_origin::SESSION_SLOT_ENV).unwrap(),
                "sessionBinding":self.env(bindings::ENV).unwrap()});
            if let Some(token) = self.env(seal::SEAL_ENV) {
                body[seal::BODY] = token.into();
            }
            if let Some(invocation) = invocation {
                body[crate::fork::genui::flow::INVOCATION] = invocation.into();
            }
            body
        }
    }

    fn choice() -> Value {
        json!({"version":1,"component":"choice","title":"Escolha","summary":"Dados informados pelo bot",
            "data":{"question":"Qual caminho?","options":[{"id":"a","label":"A"},{"id":"b","label":"B"}]}})
    }

    fn questions(hub: &Hub) -> i64 {
        hub.store.db.locked().query_row("SELECT COUNT(*) FROM donna_genui_questions", [], |r| r.get(0)).unwrap()
    }

    fn state(hub: &Hub, id: &str) -> (String, i64) {
        let sql = "SELECT state,revision FROM donna_genui_questions WHERE id=?1";
        hub.store.db.locked().query_row(sql, [id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap()
    }

    fn slot(lane: &Lane) -> String {
        format!("session:{}", lane.session.as_deref().unwrap())
    }

    /// Abertura do turno até as definições: binding canônico (`issue_in`), runtime preparado e Preparation
    /// (`prepare_session_binding`), como `ensure_session_inner` antes de session/new/load.
    fn define(actor: &Actor, lane: &Lane, binding: Option<bindings::BindingRef>) -> Defined {
        define_in(actor, lane, &slot(lane), binding)
    }

    /// Como [`define`], para um slot dado (a sessão visível vem de `exposure::visible`, como no ator).
    fn define_in(actor: &Actor, lane: &Lane, slot: &str, binding: Option<bindings::BindingRef>) -> Defined {
        let hub = &actor.hub;
        let slot = slot.to_owned();
        let session = crate::fork::sessoes::exposure::visible(BOT, Some(&slot));
        let binding =
            binding.unwrap_or_else(|| bindings::issue_in(&hub.store, BOT, BOT, &session, Some(&slot), None).unwrap());
        hub.set_runtime(BOT, |r| {
            r.status = BotStatus::Waiting;
            r.lane = Some(lane.clone());
            r.session = None;
            r.memory_turn_token = None;
            r.preparing_turn_token = Some(uuid::Uuid::new_v4().to_string());
        });
        hub.prepare_session_binding(&binding).unwrap();
        // Motor e transporte de memória do turno preparado, pelos produtores canônicos (como `start_turn`).
        crate::fork::memory_auto::transport::new_generation(hub, &actor.cfg, BOT, &actor.cfg.backend).unwrap();
        crate::fork::memory_auto::queue::capture_engine(hub, &actor.cfg, lane, &actor.cfg.backend).unwrap();
        let (servers, _) = actor.mcp_servers(Some(&slot), &binding).unwrap();
        let ponte = servers[0]["env"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["name"] == crate::fork::ponte_credencial::ENV)
            .unwrap()["value"]
            .as_str()
            .unwrap()
            .to_owned();
        let caller = hub.pontes.autenticar(hub, &ponte).unwrap();
        Defined { lane: lane.clone(), slot, binding, servers, caller }
    }

    /// Mensagem do dono na extra pelo produtor canônico (`Store::insert_entry`, que carimba a origem, E1).
    fn human(hub: &Hub, lane: &Lane) -> String {
        let turn = hub.store.alloc_turn(&lane.chat).unwrap();
        hub.store.insert_entry(lane, EntryKind::User, turn, &json!({"text":"Pedido","status":"sent"})).unwrap().id
    }

    /// Depois de session/new/load: ativação canônica do binding e do selo na ACP, e o turno ativo da extra.
    fn activate(actor: &mut Actor, defined: &Defined, acp: &str, message: &str) {
        let hub = actor.hub.clone();
        bindings::activate_for_config(&hub.store, &defined.binding, acp, &actor.cfg).unwrap();
        seal::activate(&hub, &defined.servers, Some(&defined.slot), &defined.binding, acp).unwrap();
        start(actor, &defined.lane, acp, message);
    }

    /// Turno novo na mesma ACP carregada (reuso), sem reemitir definições.
    fn start(actor: &mut Actor, lane: &Lane, acp: &str, message: &str) {
        actor.hub.set_runtime(BOT, |r| {
            r.status = BotStatus::Working;
            r.lane = Some(lane.clone());
            r.session = Some(acp.into());
            r.message = Some(message.into());
            r.started_at = Some(crate::store::now_ms());
            r.preparing_turn_token = None;
            r.memory_turn_token = Some(uuid::Uuid::new_v4().to_string());
        });
        actor.lane = lane.clone();
        actor.turn = Some(actor.hub.store.entry(message).unwrap().turn);
        actor.turn_session = Some(acp.into());
    }

    /// `dispatch` real; o `Cmd` que chegar ao ator passa por `between` e depois por `on_cmd`.
    async fn call(
        actor: &mut Actor,
        commands: &mut UnboundedReceiver<Cmd>,
        caller: &Caller,
        method: &str,
        body: Value,
        between: impl FnOnce(&Hub),
    ) -> Result<Value> {
        let (hub, caller, method) = (actor.hub.clone(), caller.clone(), method.to_owned());
        let mut task = tokio::spawn(async move { api::dispatch(&hub, &caller, &method, body).await });
        tokio::select! {
            biased;
            result = &mut task => result.unwrap(),
            Some(cmd) = commands.recv() => {
                between(&actor.hub);
                let (done, _rx) = mpsc::unbounded_channel();
                actor.on_cmd(cmd, &done).await;
                task.await.unwrap()
            }
        }
    }

    /// `dispatch` real até o `Cmd` entrar na fila do ator; o MESMO comando fica retido para [`release`].
    async fn hold(
        actor: &Actor,
        commands: &mut UnboundedReceiver<Cmd>,
        caller: &Caller,
        body: Value,
    ) -> (tokio::task::JoinHandle<Result<Value>>, Cmd) {
        let (hub, caller) = (actor.hub.clone(), caller.clone());
        let mut task = tokio::spawn(async move { api::dispatch(&hub, &caller, "chatCall", body).await });
        tokio::select! {
            biased;
            result = &mut task => {
                let result = result.unwrap().map_err(|e| format!("{e:#}"));
                panic!("a chamada devia ficar na fila: {result:?}")
            }
            Some(cmd) = commands.recv() => (task, cmd),
        }
    }

    /// Entrega ao ator o comando retido por [`hold`] e devolve o resultado da mesma chamada.
    async fn release(actor: &mut Actor, task: tokio::task::JoinHandle<Result<Value>>, cmd: Cmd) -> Result<Value> {
        let (done, _rx) = mpsc::unbounded_channel();
        actor.on_cmd(cmd, &done).await;
        task.await.unwrap()
    }

    /// Nenhum evento de pergunta ou marcador publicado desde a inscrição.
    fn no_question_events(events: &mut tokio::sync::broadcast::Receiver<Value>) {
        while let Ok(event) = events.try_recv() {
            let data = &event["entry"]["data"];
            assert!(data["genuiChoice"].is_null() && data["sessionMarker"].is_null(), "publicado: {event}");
        }
    }

    fn code(result: Result<Value>) -> String {
        format!("{:#}", result.unwrap_err())
    }

    fn device(hub: &Arc<Hub>) -> Caller {
        let device = Device {
            key: "aparelho-c6".into(),
            name: "Teste".into(),
            platform: "teste".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![Scope::Control],
            lease_until: None,
            created_at: crate::store::now_ms(),
            last_seen_at: None,
        };
        hub.store.put_device(&device).unwrap();
        crate::fork::autoridade::autenticar(hub, &device.key).unwrap().1
    }

    fn close(hub: &Hub, session: &str) {
        let mut db = hub.store.db.locked();
        let tx = db.transaction().unwrap();
        let revision = sessions::get(&tx, session).unwrap().unwrap().revision;
        encerrar::close_on(&tx, BOT, session, revision).unwrap();
        tx.commit().unwrap();
    }

    /// Ator real com um processo ACP sintético só para o `Conn` (curados); nenhum session/new/prompt é pedido a ele.
    /// A conta gerenciada da fixture não aceita `command`, então o ciclo session/new/load real fica fora (lacuna).
    async fn actor(f: &crate::fork::sessoes::contexto_ferramenta::tests::Fixture) -> Actor {
        let mut actor = new_actor(f.hub.clone(), f.hub.store.bot(BOT).unwrap().unwrap().config);
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/oauth_context_agent.py");
        let command = format!("{} http://127.0.0.1:9/sem-uso", crate::shell::python(&script));
        let cwd = actor.cfg.cwd.clone();
        actor.conn = Some(start_agent_protected(&command, &cwd, &[], Duration::from_secs(5)).await.unwrap());
        actor
    }

    /// Caso 1 e parte do 2: primeiro turno descobre pela Preparation sem conceder execução; depois da ativação, a
    /// chamada NOVA grava a pergunta na extra exata (E1 na origem e na projeção), publica só após o commit, o replay
    /// mantém uma pergunta e a resposta existente entra na fila da extra; `choice_response` não abre outra escolha;
    /// retomada (reuso da ACP e load com definição nova) só vale para a chamada nova.
    #[tokio::test]
    async fn extra_selada_descobre_no_preparo_grava_apos_ativar_e_resposta_nao_abre_outra() {
        let f = crate::fork::sessoes::contexto_ferramenta::tests::Fixture::new(false);
        let mut commands = f.hub.test_bot_commands(BOT);
        let mut actor = actor(&f).await;
        let lane = f.extra();
        let a = define(&actor, &lane, None);
        let token = a.env(seal::SEAL_ENV).expect("selo só no chat da extra");
        for server in a.servers.as_array().unwrap().iter().filter(|s| s["name"] != "chat") {
            assert!(!server.to_string().contains(&token), "selo fora dos outros servidores");
        }

        // Discovery: catálogo autenticado antes da ACP, e tools/call na MESMA fase recusa sem efeito.
        let catalog =
            call(&mut actor, &mut commands, &a.caller, "chatCall", a.body(seal::CATALOG, None), |_| {}).await.unwrap();
        assert_eq!(catalog, json!({"available": true}));
        assert!(!catalog.to_string().contains(&token), "selo fora do catálogo");
        let early =
            call(&mut actor, &mut commands, &a.caller, "chatCall", a.body("mostrar_componente", Some("n:0")), |_| {})
                .await;
        assert!(code(early).contains("chamada vencida"));
        assert_eq!(questions(&f.hub), 0);

        // Ativa: chamada nova grava; eventos depois do commit, na extra exata.
        let message = human(&f.hub, &lane);
        activate(&mut actor, &a, "acp-a", &message);
        let mut events = f.hub.events.subscribe();
        let shown =
            call(&mut actor, &mut commands, &a.caller, "chatCall", a.body("mostrar_componente", Some("n:1")), |_| {})
                .await
                .unwrap();
        let text = shown;
        let id = text["questionId"].as_str().unwrap().to_owned();
        assert_eq!((text["state"].as_str(), questions(&f.hub)), (Some("pending"), 1));
        let mut published = Vec::new();
        while let Ok(event) = events.try_recv() {
            published.push(event);
        }
        let projection = published
            .iter()
            .find(|e| e["entry"]["data"]["genuiChoice"]["questionId"] == id.as_str())
            .expect("projeção publicada");
        assert_eq!(projection["entry"]["sessionId"], json!(lane.session_id()));
        assert_eq!(projection["entry"]["data"]["spaceContext"]["conversationId"], BOT, "E1: carimbo da principal");
        assert!(!projection.to_string().contains(&token), "selo fora da pergunta");
        let replay =
            call(&mut actor, &mut commands, &a.caller, "chatCall", a.body("mostrar_componente", Some("n:1")), |_| {})
                .await
                .unwrap();
        assert!(replay.to_string().contains(&id) && questions(&f.hub) == 1, "replay: pergunta única");

        // Resposta pela RPC existente: fila da extra; a resposta não é fala humana para outra escolha.
        let session_rev = sessions::get(&f.hub.store.db.locked(), lane.session_id()).unwrap().unwrap().revision;
        let answered = api::dispatch(
            &f.hub,
            &device(&f.hub),
            "sessionChoiceAnswer",
            json!({"botId":BOT,"sessionId":lane.session_id(),"expectedRevision":session_rev,
                "questionId":id,"revision":1,"answer":{"kind":"option","id":"a"}}),
        )
        .await
        .unwrap();
        let response = answered["entry"]["id"].as_str().unwrap().to_owned();
        let sent = commands.try_recv().unwrap();
        assert!(matches!(&sent, Cmd::Send { lane: to, entry_id, .. } if *to == lane && *entry_id == response));
        start(&mut actor, &lane, "acp-a", &response);
        let again =
            call(&mut actor, &mut commands, &a.caller, "chatCall", a.body("mostrar_componente", Some("n:2")), |_| {})
                .await;
        assert!(code(again).contains("human_turn_required"));
        assert_eq!(questions(&f.hub), 1);

        // Reuso da ACP carregada: turno novo, mesma definição/selo; só a chamada nova grava.
        let message = human(&f.hub, &lane);
        start(&mut actor, &lane, "acp-a", &message);
        call(&mut actor, &mut commands, &a.caller, "chatCall", a.body("mostrar_componente", Some("n:3")), |_| {})
            .await
            .unwrap();
        assert_eq!(questions(&f.hub), 2);

        // Load com definição nova: selo novo; a chamada com o selo antigo não adota o sucessor.
        bindings::revoke(&f.hub.store, BOT, Some("acp-a")).unwrap();
        let b = define(&actor, &lane, None);
        assert_ne!(b.env(seal::SEAL_ENV), a.env(seal::SEAL_ENV));
        let message = human(&f.hub, &lane);
        activate(&mut actor, &b, "acp-a2", &message);
        let mut stale = b.body("mostrar_componente", Some("n:4"));
        stale[seal::BODY] = a.env(seal::SEAL_ENV).unwrap().into();
        let stale = call(&mut actor, &mut commands, &b.caller, "chatCall", stale, |_| {}).await;
        assert!(code(stale).contains("tool_binding_mismatch"));
        call(&mut actor, &mut commands, &b.caller, "chatCall", b.body("mostrar_componente", Some("n:5")), |_| {})
            .await
            .unwrap();
        assert_eq!(questions(&f.hub), 3);
    }

    fn charts(hub: &Hub) -> i64 {
        hub.store.db.locked().query_row("SELECT COUNT(*) FROM donna_genui_charts", [], |r| r.get(0)).unwrap()
    }

    /// Gráfico de leitura pela mesma ponte real da extra selada: grava com E1 na origem e na projeção, publica só
    /// depois do commit, o replay mantém um gráfico, entrada hostil recusa sem efeito, e o gráfico não cria pergunta.
    #[tokio::test]
    async fn grafico_na_extra_selada_grava_pela_ponte_real_sem_abrir_pergunta() {
        let f = crate::fork::sessoes::contexto_ferramenta::tests::Fixture::new(false);
        let mut commands = f.hub.test_bot_commands(BOT);
        let mut actor = actor(&f).await;
        let lane = f.extra();
        let a = define(&actor, &lane, None);
        let message = human(&f.hub, &lane);
        activate(&mut actor, &a, "acp-a", &message);
        let token = a.env(seal::SEAL_ENV).expect("selo só no chat da extra");
        let chart = json!({"version":1,"component":"pie_chart","title":"Partes","summary":"Valores fictícios",
            "data":{"labels":["A","B"],"series":[{"id":"s","label":"Partes","values":[40, 60]}],
                "unit":"%","period":"Novembro de 2026","sourceLabel":"Dados fictícios"}});
        let call_with = |invocation: &str, arguments: &Value| {
            let mut body = a.body("mostrar_componente", Some(invocation));
            body["arguments"] = arguments.clone();
            body
        };

        let mut hostile = chart.clone();
        hostile["data"]["html"] = json!("<b>SENTINELA_PRIVADA</b>");
        let refused = call(&mut actor, &mut commands, &a.caller, "chatCall", call_with("g:0", &hostile), |_| {}).await;
        let refused = code(refused);
        assert!(refused.contains("genui_chart_invalid_payload") && !refused.contains("SENTINELA_PRIVADA"));
        assert_eq!(charts(&f.hub), 0);

        let mut events = f.hub.events.subscribe();
        let shown =
            call(&mut actor, &mut commands, &a.caller, "chatCall", call_with("g:1", &chart), |_| {}).await.unwrap();
        assert_eq!((shown["status"].as_str(), charts(&f.hub), questions(&f.hub)), (Some("shown"), 1, 0));
        let id = shown["chartId"].as_str().unwrap().to_owned();
        let mut published = Vec::new();
        while let Ok(event) = events.try_recv() {
            published.push(event);
        }
        let projection = published
            .iter()
            .find(|e| e["entry"]["data"]["genuiChart"]["chartId"] == id.as_str())
            .expect("projeção publicada");
        assert_eq!(projection["entry"]["sessionId"], json!(lane.session_id()));
        assert_eq!(projection["entry"]["data"]["spaceContext"]["conversationId"], BOT, "E1: carimbo da principal");
        assert!(!projection.to_string().contains(&token), "selo fora do gráfico");
        let replay =
            call(&mut actor, &mut commands, &a.caller, "chatCall", call_with("g:1", &chart), |_| {}).await.unwrap();
        assert!(replay["chartId"] == id.as_str() && charts(&f.hub) == 1, "replay: gráfico único");
        close(&f.hub, lane.session_id());
        let after_close =
            call(&mut actor, &mut commands, &a.caller, "chatCall", call_with("g:2", &chart), |_| {}).await;
        assert!(after_close.is_err() && charts(&f.hub) == 1, "extra fechada não grava");
    }

    /// Caso 2: selo ausente/falso, só prefixo, principal e A↔B sem descritor e sem efeito; barreira entre captura e
    /// entrega (fechar a extra) e entre a entrega e o ator (revogar) recusam; binding novo não salva a captura antiga.
    #[tokio::test]
    async fn selo_falso_cruzado_principal_e_revogacao_apos_captura_nao_gravam() {
        let f = crate::fork::sessoes::contexto_ferramenta::tests::Fixture::new(false);
        let mut commands = f.hub.test_bot_commands(BOT);
        let mut actor = actor(&f).await;
        let (lane_a, lane_b) = (f.extra(), f.extra());
        let b = define(&actor, &lane_b, None);
        let a = define(&actor, &lane_a, None);
        for body in [
            {
                let mut x = a.body(seal::CATALOG, None);
                x.as_object_mut().unwrap().remove(seal::BODY);
                x
            },
            {
                let mut x = a.body(seal::CATALOG, None);
                x[seal::BODY] = "selo_falso".into();
                x
            },
            {
                let mut x = a.body(seal::CATALOG, None);
                x[seal::BODY] = b.env(seal::SEAL_ENV).unwrap().into();
                x
            },
        ] {
            let refused = call(&mut actor, &mut commands, &a.caller, "chatCall", body, |_| {}).await;
            assert!(code(refused).contains("tool_binding"));
        }
        // Só prefixo: a Ponte da principal não fala pelo slot da extra.
        actor.lane = Lane::main(BOT);
        let principal = bindings::issue_in(&f.hub.store, BOT, BOT, BOT, None, None).unwrap();
        f.hub.set_runtime(BOT, |r| r.lane = Some(Lane::main(BOT)));
        f.hub.prepare_session_binding(&principal).unwrap();
        let (main_servers, _) = actor.mcp_servers(None, &principal).unwrap();
        assert!(!main_servers.to_string().contains(seal::SEAL_ENV), "principal sem selo");
        let main_ponte = main_servers[0]["env"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["name"] == crate::fork::ponte_credencial::ENV)
            .unwrap()["value"]
            .as_str()
            .unwrap()
            .to_owned();
        let main_caller = f.hub.pontes.autenticar(&f.hub, &main_ponte).unwrap();
        let mut prefix = a.body(seal::CATALOG, None);
        prefix["sessionBinding"] = principal.id.as_str().into();
        assert!(call(&mut actor, &mut commands, &main_caller, "chatCall", prefix, |_| {}).await.is_err());

        // A ativa; o selo de B com o binding de A não grava; nada de pergunta.
        let a = define(&actor, &lane_a, None);
        let message = human(&f.hub, &lane_a);
        activate(&mut actor, &a, "acp-a", &message);
        let mut crossed = a.body("mostrar_componente", Some("x:1"));
        crossed[seal::BODY] = b.env(seal::SEAL_ENV).unwrap().into();
        assert!(
            code(call(&mut actor, &mut commands, &a.caller, "chatCall", crossed, |_| {}).await)
                .contains("tool_binding")
        );
        assert_eq!(questions(&f.hub), 0);

        // Barreira após a captura: revogação entre a entrega e o ator.
        let mut events = f.hub.events.subscribe();
        let revoked =
            call(&mut actor, &mut commands, &a.caller, "chatCall", a.body("mostrar_componente", Some("x:2")), |hub| {
                bindings::revoke(&hub.store, BOT, Some("acp-a")).unwrap();
            })
            .await;
        assert!(revoked.is_err());
        assert_eq!(questions(&f.hub), 0);
        assert!(events.try_recv().is_err(), "nenhum evento de pergunta");
        // Binding novo não salva a chamada antiga; a chamada NOVA grava.
        let old = a.body("mostrar_componente", Some("x:3"));
        let fresh = define(&actor, &lane_a, None);
        let message = human(&f.hub, &lane_a);
        activate(&mut actor, &fresh, "acp-a3", &message);
        assert!(call(&mut actor, &mut commands, &fresh.caller, "chatCall", old, |_| {}).await.is_err());
        assert_eq!(questions(&f.hub), 0);
        // Caller não-Ponte com execução e selo válidos: recusa sem efeito; a mesma chamada pela Ponte real grava.
        let mut events = f.hub.events.subscribe();
        let local = fresh.body("mostrar_componente", Some("x:4"));
        let local = call(&mut actor, &mut commands, &Caller::Local, "chatCall", local, |_| {}).await;
        assert!(local.unwrap_err().is::<api::devices::Forbidden>(), "só a Ponte original");
        assert!(commands.try_recv().is_err() && events.try_recv().is_err(), "nenhum comando ou evento");
        assert_eq!(questions(&f.hub), 0);
        call(
            &mut actor,
            &mut commands,
            &fresh.caller,
            "chatCall",
            fresh.body("mostrar_componente", Some("x:4")),
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(questions(&f.hub), 1);

        // Fechar a extra entre a captura e a entrega ao ator: sem pergunta nem evento.
        let message = human(&f.hub, &lane_a);
        start(&mut actor, &lane_a, "acp-a3", &message);
        let hub = f.hub.clone();
        let closing = lane_a.session.clone().unwrap();
        f.after_capture(move || close(&hub, &closing));
        let mut events = f.hub.events.subscribe();
        let closed = call(
            &mut actor,
            &mut commands,
            &fresh.caller,
            "chatCall",
            fresh.body("mostrar_componente", Some("x:5")),
            |_| {},
        )
        .await;
        assert!(closed.is_err());
        assert_eq!(questions(&f.hub), 1);
        while let Ok(event) = events.try_recv() {
            assert!(event["entry"]["data"]["genuiChoice"]["state"] != "pending", "nenhuma pergunta nova publicada");
        }
    }

    /// Caso 9: pergunta criada pelo produtor C6; estado interrompido INJETADO (`mark_closed_on` sem a limpeza, não uma
    /// queda dentro da Tx atômica do fechamento); reinício e varredura real cancelam só a da extra fechada, de forma
    /// idempotente, e a irmã fica pendente. Ausência da `SessionRow`: remoção só numa CÓPIA descartável do banco, sem
    /// desligar FK/gatilhos/checks.
    #[tokio::test]
    async fn varredura_cancela_extra_fechada_ou_ausente_e_preserva_irma() {
        let f = crate::fork::sessoes::contexto_ferramenta::tests::Fixture::new(true);
        let mut commands = f.hub.test_bot_commands(BOT);
        let mut actor = actor(&f).await;
        let mut ids = Vec::new();
        for (n, lane) in [f.extra(), f.extra()].into_iter().enumerate() {
            let defined = define(&actor, &lane, None);
            let message = human(&f.hub, &lane);
            activate(&mut actor, &defined, &format!("acp-{n}"), &message);
            let shown = call(
                &mut actor,
                &mut commands,
                &defined.caller,
                "chatCall",
                defined.body("mostrar_componente", Some(&format!("r:{n}"))),
                |_| {},
            )
            .await
            .unwrap();
            let text = shown;
            ids.push((lane, text["questionId"].as_str().unwrap().to_owned()));
        }
        let [(closed, a), (sister, b)] = <[_; 2]>::try_from(ids).ok().unwrap();
        {
            let mut db = f.hub.store.db.locked();
            let tx = db.transaction().unwrap();
            sessions::mark_closed_on(&tx, closed.session_id(), "injetado").unwrap();
            tx.commit().unwrap();
        }
        assert_eq!(state(&f.hub, &a).0, "pending", "estado interrompido injetado");
        let reopened = f.reopen();
        crate::fork::owner_question::sweep(&reopened);
        let after = state(&reopened, &a);
        assert_eq!((after.0.as_str(), state(&reopened, &b).0.as_str()), ("cancelled", "pending"));
        let projection: String = reopened
            .store
            .db
            .locked()
            .query_row(
                "SELECT json_extract(e.data,'$.genuiChoice.state') FROM donna_genui_questions q JOIN entries e ON e.id=q.entry_id WHERE q.id=?1",
                [&a],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(projection, "cancelled");
        crate::fork::owner_question::sweep(&reopened);
        assert_eq!(state(&reopened, &a), after, "idempotente");

        // Ausência: cópia descartável; só a SessionRow da irmã some; FK/gatilhos/checks continuam ligados.
        let copy_dir = crate::store::private_fixture_dir("c6-copia");
        let copy = copy_dir.join("copy.db");
        reopened.store.db.locked().execute_batch(&format!("VACUUM INTO '{}'", copy.display())).unwrap();
        let copied = crate::store::Store::open(&copy).unwrap();
        let mut c = copied.db.locked();
        // FK ligadas de propósito (o padrão do Store as deixa desligadas); gatilhos e checks intactos.
        c.execute_batch("PRAGMA foreign_keys=ON").unwrap();
        assert_eq!(c.execute("DELETE FROM donna_bot_sessions WHERE id=?1", [sister.session_id()]).unwrap(), 1);
        let tx = c.transaction().unwrap();
        assert!(!crate::fork::genui::flow::sweep_on(&tx).unwrap().is_empty());
        let state_b: String =
            tx.query_row("SELECT state FROM donna_genui_questions WHERE id=?1", [&b], |r| r.get(0)).unwrap();
        assert_eq!(state_b, "cancelled");
        drop(tx);
        drop(c);
        drop(copied);
        std::fs::remove_dir_all(copy_dir).unwrap();
        assert_eq!(state(&reopened, &b).0, "pending", "o banco real não muda");
    }

    /// C6-MAC-01(a): rollback do PRODUTOR pessoal. Troca canônica de lane antes (a última entrada do chat fica na
    /// principal), para que o marcador da extra seja de fato produzido. Falha técnica de teste delimitada dentro de
    /// `create → snapshot → commit`: (1) a inserção da pergunta aborta só quando a projeção e o marcador provisórios já
    /// estão na Tx; (2) a projeção recém-criada perde o carimbo E1 e o `snapshot` pessoal recusa. Nada fica: pergunta
    /// (recibo), projeção, marcador ou evento. Sem a falha, a MESMA invocação grava e publica marcador → projeção.
    #[tokio::test]
    async fn falha_tecnica_no_show_pessoal_desfaz_pergunta_projecao_e_marcador() {
        const FAULTS: [(&str, &str); 2] = [
            (
                "CREATE TRIGGER c6_falha BEFORE INSERT ON donna_genui_questions
                    WHEN NEW.marker_entry IS NOT NULL AND EXISTS(SELECT 1 FROM entries WHERE id=NEW.marker_entry)
                        AND EXISTS(SELECT 1 FROM entries WHERE id=NEW.entry_id)
                    BEGIN SELECT RAISE(ABORT,'c6 falha técnica na pergunta'); END;",
                "c6 falha técnica na pergunta",
            ),
            (
                "CREATE TRIGGER c6_falha AFTER INSERT ON donna_genui_questions WHEN NEW.marker_entry IS NOT NULL
                    BEGIN UPDATE entries SET data=json_remove(data,'$.spaceContext')
                        WHERE id=NEW.entry_id AND json_extract(data,'$.spaceContext') IS NOT NULL; END;",
                "genui_scope_mismatch",
            ),
        ];
        let f = crate::fork::sessoes::contexto_ferramenta::tests::Fixture::new(false);
        let mut commands = f.hub.test_bot_commands(BOT);
        let mut actor = actor(&f).await;
        let lane = f.extra();
        let a = define(&actor, &lane, None);
        let message = human(&f.hub, &lane);
        activate(&mut actor, &a, "acp-a", &message);
        let turn = f.hub.store.alloc_turn(BOT).unwrap();
        f.hub
            .store
            .insert_entry(&Lane::main(BOT), EntryKind::Notice, turn, &json!({"text":"Aviso da principal"}))
            .unwrap();
        assert!(crate::fork::sessoes::marker::needed(&f.hub.store.db.locked(), &lane).unwrap().is_some());
        let tally = |hub: &Hub| -> (i64, i64, i64) {
            hub.store
                .db
                .locked()
                .query_row(
                    "SELECT (SELECT COUNT(*) FROM donna_genui_questions),(SELECT COUNT(*) FROM entries),
                        (SELECT COUNT(*) FROM entries WHERE json_extract(data,'$.sessionMarker.sessionId')=?1)",
                    [lane.session_id()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap()
        };
        let before = tally(&f.hub);
        for (fault, expected) in FAULTS {
            f.hub.store.db.locked().execute_batch(fault).unwrap();
            let mut events = f.hub.events.subscribe();
            let failed = call(
                &mut actor,
                &mut commands,
                &a.caller,
                "chatCall",
                a.body("mostrar_componente", Some("f:1")),
                |_| {},
            )
            .await;
            let error = code(failed);
            assert!(error.contains(expected), "{error}");
            assert_eq!(tally(&f.hub), before, "rollback completo: pergunta, projeção e marcador");
            no_question_events(&mut events);
            f.hub.store.db.locked().execute_batch("DROP TRIGGER c6_falha").unwrap();
        }
        // Sem a falha, a MESMA invocação grava pergunta, projeção e marcador da extra.
        let mut events = f.hub.events.subscribe();
        let shown =
            call(&mut actor, &mut commands, &a.caller, "chatCall", a.body("mostrar_componente", Some("f:1")), |_| {})
                .await
                .unwrap();
        let id = shown["questionId"].as_str().unwrap().to_owned();
        assert_eq!(tally(&f.hub), (before.0 + 1, before.1 + 2, before.2 + 1));
        let (entry, marker): (String, Option<String>) = f
            .hub
            .store
            .db
            .locked()
            .query_row("SELECT entry_id,marker_entry FROM donna_genui_questions WHERE id=?1", [&id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert!(marker.is_some(), "marcador produzido");
        let marker = marker.unwrap();
        let mut published = Vec::new();
        while let Ok(event) = events.try_recv() {
            if let Some(id) = event["entry"]["id"].as_str().filter(|id| *id == entry || *id == marker) {
                published.push(id.to_owned());
            }
        }
        assert_eq!(published, vec![marker, entry], "marcador antes da projeção, só após o commit");
    }

    /// C6-MAC-01(b) e SEG-C6-01: o MESMO comando já na fila do ator, liberado depois de cada corrida injetada. (1) Só o
    /// token do turno muda (mesma ACP, `BindingId`, lane, mensagem e `started_at`): `bindings::recheck` aceitaria
    /// a captura antiga; a comparação explícita recusa. (2) Revogação e ativação do binding sucessor ANTES de liberar.
    /// (3) Só a Ponte do slot é revogada, com `BindingId`, selo e runtime vivos: a escrita recusa pela Origem original.
    /// Em todos, zero pergunta e evento; só a chamada NOVA grava.
    #[tokio::test]
    async fn captura_antiga_na_fila_recusa_token_trocado_sucessor_e_ponte_revogada() {
        let f = crate::fork::sessoes::contexto_ferramenta::tests::Fixture::new(false);
        let mut commands = f.hub.test_bot_commands(BOT);
        let mut actor = actor(&f).await;
        let lane = f.extra();
        let a = define(&actor, &lane, None);
        let message = human(&f.hub, &lane);
        activate(&mut actor, &a, "acp-a", &message);

        // (1) Troca isolada do token do turno, injeção negativa de corrida em memória.
        let (task, held) = hold(&actor, &mut commands, &a.caller, a.body("mostrar_componente", Some("h:1"))).await;
        let captured = {
            let Cmd::ContextTool { context, .. } = &held else { panic!("comando contextual esperado") };
            context.turn.execution.clone().unwrap()
        };
        let original = f.hub.runtime(BOT);
        f.hub.set_runtime(BOT, |r| r.memory_turn_token = Some(uuid::Uuid::new_v4().to_string()));
        let swapped = f.hub.runtime(BOT);
        assert!(
            swapped.lane == original.lane
                && swapped.session == original.session
                && swapped.message == original.message
                && swapped.started_at == original.started_at
                && swapped.memory_turn_token != original.memory_turn_token
        );
        assert!(bindings::recheck(&f.hub, &captured).is_ok(), "recheck sozinho aceitaria a captura antiga");
        let mut events = f.hub.events.subscribe();
        let old = release(&mut actor, task, held).await;
        assert!(code(old).contains("tool_turn_changed"), "comparação explícita do token original");
        assert_eq!(questions(&f.hub), 0);
        no_question_events(&mut events);
        call(&mut actor, &mut commands, &a.caller, "chatCall", a.body("mostrar_componente", Some("h:2")), |_| {})
            .await
            .unwrap();
        assert_eq!(questions(&f.hub), 1, "só a chamada nova grava");

        // (2) Sucessor revogado e ativado antes de liberar o MESMO comando antigo.
        let (task, held) = hold(&actor, &mut commands, &a.caller, a.body("mostrar_componente", Some("h:3"))).await;
        bindings::revoke(&f.hub.store, BOT, Some("acp-a")).unwrap();
        let successor = define(&actor, &lane, None);
        let message = human(&f.hub, &lane);
        activate(&mut actor, &successor, "acp-b", &message);
        let mut events = f.hub.events.subscribe();
        assert!(release(&mut actor, task, held).await.is_err());
        assert_eq!(questions(&f.hub), 1);
        no_question_events(&mut events);
        call(
            &mut actor,
            &mut commands,
            &successor.caller,
            "chatCall",
            successor.body("mostrar_componente", Some("h:4")),
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(questions(&f.hub), 2, "só a chamada nova do sucessor grava");

        // (3) SEG-C6-01: só a Ponte do slot sai; BindingId, selo e runtime continuam vivos.
        let slot = successor.slot.clone();
        let runtime = f.hub.runtime(BOT);
        let mut events = f.hub.events.subscribe();
        let revoked = call(
            &mut actor,
            &mut commands,
            &successor.caller,
            "chatCall",
            successor.body("mostrar_componente", Some("h:5")),
            |hub| {
                hub.pontes.revogar_slot(BOT, Some(&slot));
                assert!(!hub.pontes.tem_slot(BOT, Some(&slot)));
            },
        )
        .await;
        assert!(revoked.unwrap_err().is::<api::devices::Forbidden>(), "a Ponte original não acompanha mais a escrita");
        assert_eq!(questions(&f.hub), 2);
        no_question_events(&mut events);
        let live = bindings::resolve(&f.hub.store.db.locked(), successor.binding.id.as_str(), BOT).unwrap();
        assert_eq!(live.acp_session.as_deref(), Some("acp-b"), "BindingId vivo");
        let now = f.hub.runtime(BOT);
        assert!(now.memory_turn_token == runtime.memory_turn_token && now.session == runtime.session, "runtime vivo");
        // Catálogo real na fase ativa, com a MESMA Caller cuja credencial saiu: a conferência final recusa.
        let catalog = seal::catalog(&f.hub, &successor.caller, &successor.body(seal::CATALOG, None));
        assert!(catalog.unwrap_err().is::<api::devices::Forbidden>(), "o catálogo também exige a Ponte original");
    }

    /// C6-MAC-01(c): alvo inelegível real, uma rotina. Binding pelo `issue_in` com o slot canônico de execução de rotina,
    /// preparação e `mcp_servers` reais: sem selo na definição e nenhum registro de selo novo. O catálogo REAL, pela Ponte
    /// real da rotina, recusa mesmo forçado com o selo VÁLIDO de uma extra; com o slot da extra recusa já na Ponte. A
    /// apresentação existente (`mcp::session_tools`) fica sem descritor.
    #[tokio::test]
    async fn rotina_sem_selo_nem_disponibilidade_no_catalogo_real() {
        let f = crate::fork::sessoes::contexto_ferramenta::tests::Fixture::new(false);
        let mut commands = f.hub.test_bot_commands(BOT);
        let mut actor = actor(&f).await;
        let extra = f.extra();
        let a = define(&actor, &extra, None);
        let token = a.env(seal::SEAL_ENV).unwrap();
        let seals = |hub: &Hub| -> i64 {
            hub.store
                .db
                .locked()
                .query_row("SELECT COUNT(*) FROM kv WHERE k LIKE 'donna.sessions.tool-seal.%'", [], |r| r.get(0))
                .unwrap()
        };
        let before = seals(&f.hub);
        actor.lane = Lane::main(BOT);
        let routine = define_in(&actor, &Lane::main(BOT), &crate::routines::session_slot("c6-rotina"), None);
        assert!(!routine.servers.to_string().contains(seal::SEAL_ENV), "rotina sem selo na definição");
        assert_eq!(seals(&f.hub), before, "nenhum registro de selo para a rotina");
        let forced = |slot: &str| {
            let mut body = a.body(seal::CATALOG, None);
            body["sessionSlot"] = slot.into();
            body["sessionBinding"] = routine.binding.id.as_str().into();
            body[seal::BODY] = token.as_str().into();
            body
        };
        let descriptor = crate::fork::genui::contract::descriptor();
        for body in [forced(&routine.slot), forced(&a.slot)] {
            let result = call(&mut actor, &mut commands, &routine.caller, "chatCall", body, |_| {}).await;
            let available = result.as_ref().is_ok_and(|v| v["available"] == true);
            assert!(result.is_err() && !available, "catálogo real sem disponibilidade");
            let tools = crate::mcp::session_tools(json!([]), available);
            assert!(
                !tools["tools"].as_array().unwrap().iter().any(|t| t["name"] == descriptor["name"]),
                "sem descritor"
            );
        }
        assert!(commands.try_recv().is_err(), "o catálogo não abre comando no ator");
    }
}
