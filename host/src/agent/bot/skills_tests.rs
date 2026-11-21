//! Regressão do ator: o perfil registrado após new deve ser o mesmo que foi enviado.
use crate::{
    LockExt,
    api::{self, devices::Caller},
    hub::{BotStatus, Hub},
    store::Store,
};
use axum::{Json, Router, http::StatusCode, routing};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot};

struct Request {
    body: Value,
    resume: oneshot::Sender<()>,
}
struct Fixture {
    hub: Arc<Hub>,
    requests: mpsc::UnboundedReceiver<Request>,
    server: tokio::task::JoinHandle<()>,
    dir: std::path::PathBuf,
    skill: String,
    prompt_action: Arc<std::sync::Mutex<Option<Value>>>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
impl Fixture {
    async fn new(inline: bool) -> Self {
        Self::with_instructions(inline, "CORPO_PRIVADO_SENTINELA").await
    }
    async fn with_instructions(inline: bool, instructions: &str) -> Self {
        std::fs::create_dir_all(crate::service::data_dir()).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let dir = std::env::temp_dir().join(format!("profile-skills-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("test.db")).unwrap();
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/profile_skills_agent.py");
        let command = format!(
            "{} {} {}",
            crate::shell::python(&script),
            crate::shell::quote(&format!("http://{address}/phase")),
            if inline { "--inline" } else { "" }
        );
        let cfg = serde_json::from_value(json!({"id":"bot", "name":"Teste", "backend":"fixture",
            "command":command, "cwd":dir, "notify":false}))
        .unwrap();
        store.save_bot(&cfg).unwrap();
        let skill = crate::fork::skill_catalog::custom(
            &store,
            &json!({"name":"teste-preparo", "description":"perfil", "instructions":instructions}),
            || Ok(()),
        )
        .unwrap();
        let skill = skill["skill"]["id"].as_str().unwrap().to_owned();
        crate::fork::skill_selection::set_for_test(&store, "bot", &[&skill]);
        let hub = Hub::new(
            store,
            "test".into(),
            crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
            "test".into(),
            address.port(),
        );
        let (tx, requests) = mpsc::unbounded_channel();
        let prompt_action = Arc::new(std::sync::Mutex::new(None::<Value>));
        let action = prompt_action.clone();
        let router = Router::new().route(
            "/phase",
            routing::post(move |Json(body): Json<Value>| {
                let tx = tx.clone();
                let action = action.clone();
                async move {
                    let action = if body["method"] == "session/prompt" { action.locked().take() } else { None };
                    let (resume, wait) = oneshot::channel();
                    tx.send(Request { body, resume }).unwrap();
                    wait.await.unwrap();
                    (StatusCode::OK, Json(action.unwrap_or(Value::Null)))
                }
            }),
        );
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        hub.start().unwrap();
        Self { hub, requests, server, dir, skill, prompt_action }
    }
    async fn send(&self) {
        api::dispatch(&self.hub, &Caller::Local, "send", json!({"botId":"bot", "text":"teste"})).await.unwrap();
    }
    async fn request(&mut self, method: &str) -> Request {
        let request = tokio::time::timeout(Duration::from_secs(20), self.requests.recv()).await.unwrap().unwrap();
        assert_eq!(request.body["method"], method);
        assert!(!request.body.to_string().contains("CORPO_PRIVADO_SENTINELA"));
        request
    }
    async fn idle(&self) {
        let mut events = self.hub.events.subscribe();
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                match self.hub.runtime("bot").status {
                    BotStatus::Idle => break,
                    BotStatus::Error => panic!("turno falhou"),
                    _ => {
                        events.recv().await.unwrap();
                    }
                }
            }
        })
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn system_prompt_adota_o_par_enviado_antes_do_await_e_anuncia_alteracao() {
    let mut f = Fixture::new(false).await;
    f.send().await;
    let new = f.request("session/new").await;
    let path = f.hub.store.skills_root.join(&f.skill).join("SKILL.md");
    let sent = new.body["params"]["systemPrompt"]["content"][0]["text"].as_str().unwrap();
    assert!(sent.contains(&path.display().to_string()), "new já expôs o caminho legado");
    std::fs::write(&path, "ADULTERADO").unwrap();
    new.resume.send(()).unwrap();
    let prompt = f.request("session/prompt").await;
    let snapshot: Value = serde_json::from_str(&f.hub.store.kv_get("context.bot.skills-session").unwrap()).unwrap();
    assert_eq!(snapshot["system"], sent);
    assert!(snapshot["systemProfile"].as_str().unwrap().contains(&path.display().to_string()));
    assert!(
        snapshot["announced"].as_str().unwrap().contains(&path.display().to_string()),
        "adopt não pode declarar outro perfil anunciado"
    );
    let text = prompt.body["params"]["prompt"][0]["text"].as_str().unwrap();
    assert!(text.contains("<agent_profile_update>"), "mudança durante await precisa ser anunciada");
    assert!(!text.contains(&path.display().to_string()));
    prompt.resume.send(()).unwrap();
    f.idle().await;
    f.hub.shutdown().await;
}

#[tokio::test]
async fn inline_consume_par_preparado_sem_inserir_corpo() {
    let mut f = Fixture::new(true).await;
    f.send().await;
    let new = f.request("session/new").await;
    assert!(new.body["params"].get("systemPrompt").is_none());
    new.resume.send(()).unwrap();
    let prompt = f.request("session/prompt").await;
    let text = prompt.body["params"]["prompt"][0]["text"].as_str().unwrap();
    let snapshot: Value = serde_json::from_str(&f.hub.store.kv_get("context.bot.skills-session").unwrap()).unwrap();
    assert!(text.contains(&format!("<bot-profile>\n{}\n</bot-profile>", snapshot["system"].as_str().unwrap())));
    assert!(snapshot["systemProfile"].as_str().unwrap().contains("read "));
    prompt.resume.send(()).unwrap();
    f.idle().await;
    f.hub.shutdown().await;
}

#[tokio::test]
async fn raw_load_monta_perfil_novo_com_d306_sem_alterar_comando_raw() {
    let mut f = Fixture::new(false).await;
    f.hub.shutdown().await;
    let cfg = f.hub.store.bot("bot").unwrap().unwrap().config;
    let mut actor = super::new_actor(f.hub.clone(), cfg);
    actor.restore_dispatch().unwrap();
    let (done, mut results) = mpsc::unbounded_channel();
    let lane = crate::store::Lane::main("bot");
    let (started, ()) = tokio::join!(actor.start_turn(lane.clone(), &[], "normal", false, &done), async {
        f.request("session/new").await.resume.send(()).unwrap();
        f.request("session/prompt").await.resume.send(()).unwrap();
    });
    started.unwrap();
    actor.finish_turn(&results.recv().await.unwrap().result);
    actor.conn.as_mut().unwrap().loaded.clear();
    crate::chat::context::refresh(&f.hub.store, "bot", "skills-session").unwrap();
    let path = f.hub.store.skills_root.join(&f.skill).join("SKILL.md");
    std::fs::write(&path, "ALTERADA_ANTES_LOAD").unwrap();
    actor.raw_prompt = true;
    let (started, ()) = tokio::join!(actor.start_turn(lane, &[], "/raw-exato", false, &done), async {
        let load = f.request("session/load").await;
        let system = load.body["params"]["systemPrompt"]["content"][0]["text"].as_str().unwrap();
        assert!(system.contains(crate::fork::skill_catalog::preparo::WARNING));
        assert!(!system.contains(&path.display().to_string()));
        load.resume.send(()).unwrap();
        let prompt = f.request("session/prompt").await;
        assert_eq!(prompt.body["params"]["prompt"][0]["text"], "/raw-exato");
        prompt.resume.send(()).unwrap();
    });
    started.unwrap();
    actor.finish_turn(&results.recv().await.unwrap().result);
    actor.conn.take().unwrap().acp.kill().await;
}

#[tokio::test]
async fn pending_conserva_ticket_preparo_e_params_sem_prometer_freshness_na_entrega() {
    let mut f = Fixture::new(false).await;
    f.hub.shutdown().await;
    let cfg = f.hub.store.bot("bot").unwrap().unwrap().config;
    let mut actor = super::new_actor(f.hub.clone(), cfg);
    actor.restore_dispatch().unwrap();
    let (done, mut results) = mpsc::unbounded_channel();
    let (started, ()) =
        tokio::join!(actor.start_turn(crate::store::Lane::main("bot"), &[], "normal", false, &done), async {
            let new = f.request("session/new").await;
            crate::fork::bot_admission::set_paused_for_test(&f.hub.store, "bot", true).unwrap();
            new.resume.send(()).unwrap();
        });
    started.unwrap();
    assert!(matches!(actor.pending_admission, Some(super::fork_admission::Pending::Prompt { .. })));
    let ticket = actor.dispatch.clone().unwrap();
    assert!(actor.profile_skills.as_ref().unwrap().owns(&ticket));
    crate::fork::skill_selection::set_for_test(&f.hub.store, "bot", &[]);
    crate::fork::skill_selection::set_for_test(&f.hub.store, "bot", &[&f.skill]);
    let path = f.hub.store.skills_root.join(&f.skill).join("SKILL.md");
    std::fs::write(&path, "DEPOIS_DA_MONTAGEM").unwrap();
    assert!(actor.project_profile_skills().unwrap().unwrap().contains(crate::fork::skill_catalog::preparo::WARNING));
    crate::fork::bot_admission::set_paused_for_test(&f.hub.store, "bot", false).unwrap();
    let ((), ()) = tokio::join!(actor.retry_admission(&done), async {
        let prompt = f.request("session/prompt").await;
        // Params preparados antes da mutação continuam legados: esta fatia não ordena envio/revogação.
        assert!(!prompt.body.to_string().contains("DEPOIS_DA_MONTAGEM"));
        prompt.resume.send(()).unwrap();
    });
    assert_eq!(actor.dispatch.as_ref(), Some(&ticket));
    actor.finish_turn(&results.recv().await.unwrap().result);
    assert!(actor.profile_skills.is_none());
    actor.conn.take().unwrap().acp.kill().await;
}

#[test]
fn contexto_estrangeiro_vazio_nao_reintroduz_selecao_pessoal() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let cfg: crate::store::BotConfig =
        serde_json::from_value(json!({"id":"bot", "name":"Teste", "backend":"fixture", "cwd":"/tmp"})).unwrap();
    store.save_bot(&cfg).unwrap();
    crate::fork::skill_selection::set_for_test(&store, "bot", &["pessoal-que-nao-pode-vazar"]);
    // Configuração estrangeira é produzida pela rotina canônica de Spaces, já testada na bateria existente.
    // Aqui conferimos que a fachada respeita skills vazias e não busca as do bot no DB.
    let mut foreign = cfg;
    foreign.context = Some(crate::fork::spaces::ExecutionContext {
        conversation_id: "estrangeira".into(),
        memory_id: "estrangeira".into(),
        foreign_identity: true,
    });
    assert!(super::fork_profile_skills::project(&store, &foreign, None).unwrap().is_none());
}

/// Pedido humano na fila da principal.
fn pedido(f: &Fixture, text: &str) -> String {
    let lane = crate::store::Lane::main("bot");
    let turn = f.hub.store.alloc_turn("bot").unwrap();
    let data = json!({"text":text,"status":"queued"});
    f.hub.store.insert_entry(&lane, crate::store::EntryKind::User, turn, &data).unwrap().id
}

/// A próxima seleção da principal (D372) em outro motor sem conta. Por SQL de fixture: `sessionEngineSet` só troca de
/// provedor com conta gerenciada, que exige o CLI real; aqui a ACP sintética roda pelo `command` do bot.
fn proxima(f: &Fixture, backend: &str) {
    let engine = json!({"schemaVersion":2,"source":"own","accountId":null,"backend":backend,"model":null,
        "modelOptions":{},"fastMode":false})
    .to_string();
    let sql = "UPDATE donna_bot_sessions SET engine_next=?1 WHERE id='bot'";
    f.hub.store.db.locked().execute(sql, [engine]).unwrap();
}

/// Avisos da principal: (linhas de troca, cartões de passagem).
fn avisos(f: &Fixture) -> (Vec<Value>, Vec<Value>) {
    let entries = f.hub.store.history_in_session("bot", Some("bot"), i64::MAX, 50).unwrap();
    let notices: Vec<Value> = entries.into_iter().filter(|e| e.kind == "notice").map(|e| e.data).collect();
    let lines = notices.iter().filter(|d| d["modelSwitch"].is_object()).cloned().collect();
    (lines, notices.into_iter().filter(|d| d["handoff"].is_object()).collect())
}

/// Ator manual sobre a ACP sintética (sem o ator do Hub).
async fn ator(f: &Fixture) -> super::Actor {
    f.hub.shutdown().await;
    let cfg = f.hub.store.bot("bot").unwrap().unwrap().config;
    let mut actor = super::new_actor(f.hub.clone(), cfg);
    actor.restore_dispatch().unwrap();
    actor
}

/// O texto do prompt que o motor recebe no próximo turno, até o fim do turno.
async fn envio(
    f: &mut Fixture,
    actor: &mut super::Actor,
    text: &str,
    new_session: bool,
    done: &mpsc::UnboundedSender<super::PromptDone>,
) -> String {
    let lane = crate::store::Lane::main("bot");
    let turn = f.hub.store.alloc_turn("bot").unwrap();
    let data = json!({"text":text,"status":"queued","contentOrigin":{"ingress":{"caller":"local"}}});
    let ids = [f.hub.store.insert_entry(&lane, crate::store::EntryKind::User, turn, &data).unwrap().id];
    let (started, prompt) = tokio::join!(actor.start_turn(lane, &ids, text, false, done), async {
        if new_session {
            f.request("session/new").await.resume.send(()).unwrap();
        }
        let prompt = f.request("session/prompt").await;
        let sent = prompt.body["params"]["prompt"].to_string();
        prompt.resume.send(()).unwrap();
        sent
    });
    started.unwrap();
    prompt
}

/// D372 × DON-907 C9a, na montagem REAL do envio (`start_turn` → `compose` → `session/prompt` da ACP sintética): a
/// troca de provedor promovida no início do turno humano novo posta a linha, envia o cartão `engineSwitch` e o
/// resumo da conversa antiga chega no prompt do motor novo.
#[tokio::test]
async fn troca_de_provedor_promovida_leva_o_resumo_ao_motor_novo_no_envio() {
    let mut f = Fixture::new(false).await;
    let mut actor = ator(&f).await;
    let (done, mut results) = mpsc::unbounded_channel();
    envio(&mut f, &mut actor, "SENTINELA pedido no motor antigo", true, &done).await;
    actor.finish_turn(&results.recv().await.unwrap().result);
    proxima(&f, "fixture-b");
    let prompt = envio(&mut f, &mut actor, "segundo pedido", true, &done).await;
    actor.finish_turn(&results.recv().await.unwrap().result);
    assert!(prompt.contains("SENTINELA pedido no motor antigo") && prompt.contains("segundo pedido"), "{prompt}");
    assert_eq!(actor.cfg.backend, "fixture-b");
    let (lines, cards) = avisos(&f);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["modelSwitch"]["kind"], "provider");
    assert_eq!(lines[0]["modelSwitch"]["carried"], "handoff");
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0]["handoff"]["reason"], "engineSwitch");
    assert_eq!(cards[0]["handoff"]["status"], "sent");
    actor.conn.take().unwrap().acp.kill().await;
}

/// D372 × C9a: o cartão pendente que a fila tira para a mensagem (pelo ID exato guardado no `take_handoff`) é a
/// passagem da troca: vira `engineSwitch`, não pode ser mantido, não duplica e o texto editado chega ao motor novo.
#[tokio::test]
async fn cartao_pendente_levado_pela_fila_vira_o_da_troca_e_chega_ao_envio() {
    let mut f = Fixture::new(false).await;
    let mut actor = ator(&f).await;
    let (done, mut results) = mpsc::unbounded_channel();
    let lane = crate::store::Lane::main("bot");
    let old = json!({"text":"pedido anterior","status":"sent","contentOrigin":{"ingress":{"caller":"local"}}});
    crate::chat::memory::bind_profile_person(
        &f.hub.store,
        "bot",
        "skills-session",
        Some(crate::chat::memory::profile::OWNER),
    )
    .unwrap();
    f.hub.store.insert_entry(&lane, crate::store::EntryKind::User, 1, &old).unwrap();
    let cfg = f.hub.store.bot("bot").unwrap().unwrap().config;
    crate::chat::handoff::draft(&f.hub, &cfg, "skills-session", crate::chat::handoff::Reason::Full).unwrap();
    let (card, _) = crate::chat::handoff::pending(&f.hub.store, "bot").unwrap();
    let edit = json!({"entryId": card, "text": "SENTINELA resumo editado"});
    let local = crate::fork::donna_fixa::pedido::Origem::Local;
    crate::chat::handoff::call_as(&f.hub, "editHandoff", &edit, &local).unwrap();
    proxima(&f, "fixture-b");
    // Como `next_in_queue`: o cartão pendente vai com a próxima mensagem da principal.
    actor.handoff = actor.take_handoff();
    actor.handoff_chat = Some(lane);
    assert_eq!(actor.handoff_card.as_deref(), Some(card.as_str()));
    let prompt = envio(&mut f, &mut actor, "pedido seguinte", true, &done).await;
    actor.finish_turn(&results.recv().await.unwrap().result);
    assert!(prompt.contains("SENTINELA resumo editado") && prompt.contains("pedido seguinte"), "{prompt}");
    let (lines, cards) = avisos(&f);
    assert_eq!((lines.len(), cards.len()), (1, 1), "uma linha e o mesmo cartão, sem duplicar");
    assert_eq!(lines[0]["modelSwitch"]["carried"], "handoff");
    let saved = f.hub.store.entry(&card).unwrap();
    assert_eq!(saved.data["handoff"]["reason"], "engineSwitch");
    assert_eq!(saved.data["handoff"]["status"], "sent");
    assert!(crate::chat::handoff::call_as(&f.hub, "keepConversation", &json!({"entryId": card}), &local).is_err());
    actor.conn.take().unwrap().acp.kill().await;
}

/// Negativo: sem promoção, a configuração que muda no início do turno continua descartando o resumo ao fechar a
/// conexão (só a troca promovida da própria conversa o conserva). Nem linha nem cartão.
#[tokio::test]
async fn sem_promocao_o_fechamento_da_conexao_descarta_o_resumo() {
    let mut f = Fixture::new(false).await;
    let mut actor = ator(&f).await;
    let (done, mut results) = mpsc::unbounded_channel();
    actor.handoff = Some("SENTINELA resumo de outra configuração".into());
    actor.handoff_chat = Some(crate::store::Lane::main("bot"));
    actor.cfg.model = Some("modelo-do-turno-anterior".into());
    let prompt = envio(&mut f, &mut actor, "pedido", true, &done).await;
    actor.finish_turn(&results.recv().await.unwrap().result);
    assert!(!prompt.contains("SENTINELA resumo de outra configuração"), "{prompt}");
    assert!(prompt.contains("pedido"));
    let (lines, cards) = avisos(&f);
    assert!(lines.is_empty() && cards.is_empty());
    actor.conn.take().unwrap().acp.kill().await;
}

/// R7 (Sol e8fd): depois de um turno de extra ou de rotina no motor novo, o ator guarda a configuração e a sessão
/// DAQUELA lane. A troca promovida da principal compara com a seleção efetiva da própria principal antes da promoção
/// e usa a sessão da principal: linha de `fixture` para `fixture-b`, cartão `engineSwitch` e o resumo da principal
/// no prompt do motor novo. O estado da lane anterior é o que `start_prepared_turn`/`ensure_session` deixam no ator
/// (`cfg`, `lane`, `turn_session`); o turno da principal é real até o `session/prompt`. Sem a correção, a troca
/// comparava com a lane anterior (mesmo motor) e não havia linha nem resumo.
#[tokio::test]
async fn troca_da_principal_depois_de_extra_ou_rotina_compara_com_a_propria_principal() {
    for anterior in ["extra", "rotina"] {
        let mut f = Fixture::new(false).await;
        let mut actor = ator(&f).await;
        let (done, mut results) = mpsc::unbounded_channel();
        envio(&mut f, &mut actor, "SENTINELA pedido da principal", true, &done).await;
        actor.finish_turn(&results.recv().await.unwrap().result);
        assert!(actor.session_id.is_some(), "a principal tem sessão própria");
        actor.cfg.backend = "fixture-b".into();
        actor.turn_session = Some(format!("sessao-da-{anterior}"));
        if anterior == "extra" {
            actor.lane = crate::store::Lane { chat: "bot".into(), session: Some("extra-no-motor-novo".into()) };
        }
        proxima(&f, "fixture-b");
        let prompt = envio(&mut f, &mut actor, "segundo pedido", true, &done).await;
        actor.finish_turn(&results.recv().await.unwrap().result);
        let resumo = prompt.contains("SENTINELA pedido da principal");
        assert!(resumo && prompt.contains("segundo pedido"), "{anterior}: {prompt}");
        let (lines, cards) = avisos(&f);
        assert_eq!(lines.len(), 1, "{anterior}");
        assert_eq!(lines[0]["modelSwitch"]["from"]["backend"], "fixture", "{anterior}: a origem é a principal");
        assert_eq!(lines[0]["modelSwitch"]["to"]["backend"], "fixture-b");
        assert_eq!(lines[0]["modelSwitch"]["kind"], "provider");
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0]["handoff"]["reason"], "engineSwitch");
        assert_eq!(cards[0]["handoff"]["status"], "sent");
        actor.conn.take().unwrap().acp.kill().await;
    }
}

// API candidata: os testes abaixo passam pelo setup/permit/ativação reais do ator.
async fn reading_actor(f: &mut Fixture) -> (super::Actor, mpsc::UnboundedReceiver<super::PromptDone>) {
    f.hub.shutdown().await;
    let cfg = f.hub.store.bot("bot").unwrap().unwrap().config;
    let mut actor = super::new_actor(f.hub.clone(), cfg);
    actor.restore_dispatch().unwrap();
    let (done, results) = mpsc::unbounded_channel();
    let (started, ()) = tokio::join!(
        actor.start_turn(crate::store::Lane::main("bot"), &[], "leitura candidata", false, &done),
        async {
            f.request("session/new").await.resume.send(()).unwrap();
            let prompt = f.request("session/prompt").await;
            assert_eq!(prompt.body["params"]["_meta"]["mexericaSkillsReadCandidate"]["status"], "candidate");
            prompt.resume.send(()).unwrap();
        }
    );
    started.unwrap();
    (actor, results)
}
fn read_request(actor: &super::Actor, skill: &str) -> crate::fork::bot_skills::read::Request {
    serde_json::from_value(json!({"sessionId":"skills-session", "skillId":skill,
        "preparationId":actor.dispatch.as_ref().unwrap().attempt}))
    .unwrap()
}

#[tokio::test]
async fn api_candidata_consumidor_acp_entrega_bytes_preparados_e_recusa_paths() {
    let mut f = Fixture::new(false).await;
    let (mut actor, mut results) = reading_actor(&mut f).await;
    let path = f.hub.store.skills_root.join(&f.skill).join("SKILL.md");
    let original = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, "ALTERADO_APOS_PREPARO").unwrap();
    let prepared = actor.profile_skills.clone().unwrap();
    let request = read_request(&actor, &f.skill);
    assert_eq!(prepared.read(&f.hub, &request).unwrap(), original);
    // O caminho atual pode até virar symlink: o pedido não o abre.
    std::fs::remove_file(&path).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("/arquivo-privado-inexistente", &path).unwrap();
    assert_eq!(prepared.read(&f.hub, &request).unwrap(), original);
    for id in ["../test.db", "/etc/passwd", "nao-selecionada", "observada/SKILL.md"] {
        assert!(prepared.read(&f.hub, &read_request(&actor, id)).is_err());
    }
    assert!(
        serde_json::from_value::<crate::fork::bot_skills::read::Request>(json!({
        "sessionId":"skills-session", "skillId":f.skill, "preparationId":request.preparation,
        "path":"/etc/passwd"}))
        .is_err()
    );
    assert!(
        !f.hub.store.guard_external_exposed("bot").unwrap(),
        "IDs inválidos e cópias internas não entregam conteúdo"
    );
    let params = json!({"sessionId":request.session,"skillId":request.skill,"preparationId":request.preparation});
    // Consumer real de Incoming → handler → respond_scoped → stdin da ACP sintética.
    let ((), received) = tokio::join!(
        actor.on_incoming(Some(crate::agent::acp::Incoming::Request {
            id: json!("candidate-read"),
            method: crate::fork::bot_skills::read::METHOD.into(),
            params
        })),
        f.requests.recv()
    );
    let received = received.unwrap();
    assert_eq!(received.body["method"], "candidate-read-response");
    assert_eq!(received.body["result"]["content"], original);
    assert!(f.hub.store.guard_external_exposed("bot").unwrap());
    assert!(f.hub.store.memory_session_marked("bot", "skills-session").unwrap());
    received.resume.send(()).unwrap();
    actor.finish_turn(&results.recv().await.unwrap().result);
    assert!(actor.profile_skills.is_none());
    assert!(prepared.read(&f.hub, &request).is_err());
    actor.conn.take().unwrap().acp.kill().await;
}

#[tokio::test]
async fn api_candidata_revogacao_aba_fim_sucessor_e_outro_bot_nao_recapturam() {
    let mut f = Fixture::new(false).await;
    let (mut actor, mut results) = reading_actor(&mut f).await;
    let prepared = actor.profile_skills.clone().unwrap();
    let request = read_request(&actor, &f.skill);
    assert!(prepared.read(&f.hub, &request).is_ok());
    let mut wrong = read_request(&actor, &f.skill);
    wrong.session = "outro-bot".into();
    assert!(prepared.read(&f.hub, &wrong).is_err());
    wrong = read_request(&actor, &f.skill);
    wrong.preparation = "tentativa-anterior".into();
    assert!(prepared.read(&f.hub, &wrong).is_err());
    // Sem esperar Reconfigure ou o ator: commit canônico invalida a geração retida.
    crate::fork::skill_selection::set_for_test(&f.hub.store, "bot", &[]);
    assert!(prepared.read(&f.hub, &request).is_err());
    crate::fork::skill_selection::set_for_test(&f.hub.store, "bot", &[&f.skill]);
    assert!(prepared.read(&f.hub, &request).is_err());
    actor.finish_turn(&results.recv().await.unwrap().result);
    assert!(prepared.read(&f.hub, &request).is_err());
    let ticket = crate::fork::bot_admission::reserve(
        &f.hub.store,
        &actor.cfg,
        "bot",
        "sucessor",
        &crate::fork::bot_admission::Origin::Human,
    )
    .unwrap();
    f.hub.prepare_bot_turn("bot", Some("sucessor"), |_| {}).unwrap();
    f.hub.activate_bot_turn("bot", "sucessor").unwrap();
    assert!(prepared.read(&f.hub, &request).is_err());
    crate::fork::bot_admission::finish(&f.hub.store, "bot", &ticket).unwrap();
    actor.conn.take().unwrap().acp.kill().await;
}

#[tokio::test]
async fn api_candidata_pipe_pendente_libera_locks_e_revogacao_impede_novos_bytes() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut f = Fixture::new(false).await;
    let (mut actor, mut results) = reading_actor(&mut f).await;
    let prepared = actor.profile_skills.clone().unwrap();
    let request = read_request(&actor, &f.skill);
    let content = prepared.read(&f.hub, &request).unwrap();
    let mut frame = serde_json::to_vec(&json!({"result":{"content":content}})).unwrap();
    frame.push(b'\n');
    let (mut writer, mut reader) = tokio::io::duplex(8);
    writer.write_all(b"anterior").await.unwrap();
    let mut delivery = Box::pin(crate::fork::spaces::tool_effects::write_scoped(&mut writer, &frame, |effect| {
        prepared.delivery_scope(&f.hub, &request, effect)
    }));
    assert!(futures::poll!(delivery.as_mut()).is_pending());
    assert!(f.hub.store.memory_boundary.try_lock().is_ok());
    assert!(f.hub.store.db.try_lock().is_ok());
    assert!(f.hub.store.guard_external_exposed("bot").unwrap(), "marca conservadora antes do prefixo pendente");
    assert!(f.hub.store.memory_session_marked("bot", "skills-session").unwrap());
    crate::fork::skill_selection::set_for_test(&f.hub.store, "bot", &[]);
    assert!(delivery.await.is_err());
    writer.shutdown().await.unwrap();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await.unwrap();
    assert_eq!(bytes, b"anterior");
    actor.finish_turn(&results.recv().await.unwrap().result);
    actor.conn.take().unwrap().acp.kill().await;
}

#[tokio::test]
async fn api_candidata_revogada_durante_setup_nao_adota_selecao_nova() {
    let mut f = Fixture::new(false).await;
    f.hub.shutdown().await;
    let cfg = f.hub.store.bot("bot").unwrap().unwrap().config;
    let mut actor = super::new_actor(f.hub.clone(), cfg);
    actor.restore_dispatch().unwrap();
    let (done, mut results) = mpsc::unbounded_channel();
    let (started, ()) =
        tokio::join!(actor.start_turn(crate::store::Lane::main("bot"), &[], "teste", false, &done), async {
            let new = f.request("session/new").await;
            crate::fork::skill_selection::set_for_test(&f.hub.store, "bot", &[]);
            crate::fork::skill_selection::set_for_test(&f.hub.store, "bot", &[&f.skill]);
            new.resume.send(()).unwrap();
            f.request("session/prompt").await.resume.send(()).unwrap();
        });
    started.unwrap();
    let request = read_request(&actor, &f.skill);
    assert!(actor.profile_skills.as_ref().unwrap().read(&f.hub, &request).is_err());
    actor.finish_turn(&results.recv().await.unwrap().result);
    actor.conn.take().unwrap().acp.kill().await;
}

#[tokio::test]
async fn api_candidata_reinstalacao_identica_e_cancelamento_antes_da_entrega() {
    let mut f = Fixture::new(false).await;
    let (mut actor, mut results) = reading_actor(&mut f).await;
    let prepared = actor.profile_skills.clone().unwrap();
    let request = read_request(&actor, &f.skill);
    let detail = crate::fork::skill_catalog::details(&f.hub.store, &json!({"id":f.skill}), || Ok(())).unwrap();
    let old_id = detail["installation"]["installationId"].clone();
    let bytes = prepared.read(&f.hub, &request).unwrap();
    crate::fork::skill_catalog::remove(
        &f.hub.store,
        &json!({"id":f.skill,
        "expectedInstallationId":old_id, "expectedReceipt":{"kind":"none"}}),
        || Ok(()),
    )
    .unwrap();
    let new = crate::fork::skill_catalog::custom(
        &f.hub.store,
        &json!({"name":"teste-preparo", "description":"perfil",
        "instructions":"CORPO_PRIVADO_SENTINELA"}),
        || Ok(()),
    )
    .unwrap();
    assert_eq!(new["skill"]["id"], f.skill);
    assert_ne!(new["skill"]["installationId"], old_id);
    assert_eq!(std::fs::read_to_string(f.hub.store.skills_root.join(&f.skill).join("SKILL.md")).unwrap(), bytes);
    assert!(prepared.read(&f.hub, &request).is_err());
    // Fonte antiga jamais vira válida ao completar um await no transporte.
    let acp = actor.conn.as_ref().unwrap().acp.clone();
    let held = acp.hold_input().await;
    let mut delivering = Box::pin(prepared.deliver(&f.hub, &acp, json!("cancelled-read"), &request));
    // Já revogada: nem começa a aguardar stdin.
    assert!(futures::poll!(delivering.as_mut()).is_ready());
    drop(delivering);
    drop(held);
    f.hub.set_memory_runtime("bot", |_| {});
    assert!(prepared.read(&f.hub, &request).is_err());
    actor.finish_turn(&results.recv().await.unwrap().result);
    actor.conn.take().unwrap().acp.kill().await;
}

#[tokio::test]
async fn api_candidata_revogacao_enquanto_aguarda_stdin_revalida_antes_do_pipe() {
    let mut f = Fixture::new(false).await;
    let (mut actor, mut results) = reading_actor(&mut f).await;
    let prepared = actor.profile_skills.clone().unwrap();
    let request = read_request(&actor, &f.skill);
    let acp = actor.conn.as_ref().unwrap().acp.clone();
    let held = acp.hold_input().await;
    let mut delivering = Box::pin(prepared.deliver(&f.hub, &acp, json!("late-read"), &request));
    assert!(futures::poll!(delivering.as_mut()).is_pending());
    f.hub.set_memory_runtime("bot", |_| {});
    drop(held);
    assert!(delivering.await.is_err());
    // Nenhum byte tinha sido escrito: o mesmo stdin aceita o erro contratual.
    let (sent, received) = tokio::join!(
        acp.respond_error(json!("candidate-refusal"), -32602, "skills_read_unavailable"),
        f.requests.recv()
    );
    sent.unwrap();
    let received = received.unwrap();
    assert_eq!(received.body["method"], "candidate-refusal-response");
    assert_eq!(received.body["error"]["code"], -32602);
    assert_eq!(received.body["error"]["message"], "skills_read_unavailable");
    assert!(received.body["result"].is_null());
    received.resume.send(()).unwrap();
    actor.finish_turn(&results.recv().await.unwrap().result);
    actor.conn.take().unwrap().acp.kill().await;
}

// R1: motor retido no pedido real; ainda não confirmou cancelamento nem concluiu o prompt.
async fn reading_actor_pending(f: &mut Fixture) -> (super::Actor, Request) {
    f.hub.shutdown().await;
    let cfg = f.hub.store.bot("bot").unwrap().unwrap().config;
    let mut actor = super::new_actor(f.hub.clone(), cfg);
    actor.restore_dispatch().unwrap();
    let (done, _results) = mpsc::unbounded_channel();
    let (started, prompt) = tokio::join!(
        actor.start_turn(crate::store::Lane::main("bot"), &[], "leitura candidata", false, &done),
        async {
            f.request("session/new").await.resume.send(()).unwrap();
            f.request("session/prompt").await
        }
    );
    started.unwrap();
    (actor, prompt)
}

#[tokio::test]
async fn api_candidata_parada_pessoal_real_recusa_antes_do_ack() {
    use std::io::Write;
    let mut f = Fixture::new(false).await;
    *f.prompt_action.locked() = Some(json!({"holdCompletion":true,"responsesBeforeCompletion":2}));
    let (mut actor, prompt) = reading_actor_pending(&mut f).await;
    let prepared = actor.profile_skills.clone().unwrap();
    let request = read_request(&actor, &f.skill);
    assert!(prepared.read(&f.hub, &request).is_ok());
    let params = json!({"sessionId":request.session,"preparationId":request.preparation,"skillId":request.skill});
    let acp = actor.conn.as_ref().unwrap().acp.clone();
    let held = acp.hold_input().await;
    actor.on_skill_read(json!("candidate-refusal"), params.clone());
    tokio::time::timeout(Duration::from_secs(20), acp.scoped_entered()).await.unwrap();
    actor.cancel_turn_kept_bridges().await;
    assert!(actor.stop_requested && actor.turn.is_some());
    assert_eq!(
        f.hub.runtime("bot").memory_turn_token.as_deref(),
        Some(actor.dispatch.as_ref().unwrap().execution_token.as_str())
    );
    assert!(
        prepared.read(&f.hub, &request).is_err(),
        "R1-01: parada pessoal processada deve recusar leitura original antes do ACK"
    );
    let canonical = crate::fork::bot_admission::current(&f.hub.store, "bot").unwrap().unwrap();
    assert_eq!(canonical.phase, crate::fork::bot_admission::Phase::Permitted);
    assert_eq!(Some(canonical.ticket()), actor.dispatch);
    // Nova leitura depois da parada; o pedido anterior já aguardava I/O.
    actor.on_skill_read(json!("candidate-read"), params);
    drop(held);
    prompt.resume.send(()).unwrap();
    let mut responses = Vec::new();
    for _ in 0..2 {
        let response = tokio::time::timeout(Duration::from_secs(20), f.requests.recv()).await.unwrap().unwrap();
        responses.push(response.body["method"].as_str().unwrap().to_owned());
        assert_eq!(response.body["error"]["code"], -32602);
        assert_eq!(response.body["error"]["message"], "skills_read_unavailable");
        assert!(response.body["result"].is_null());
        assert!(actor.turn.is_some());
        // A ACP só conclui o prompt DEPOIS de liberar a segunda recusa observada.
        response.resume.send(()).unwrap();
    }
    responses.sort();
    assert_eq!(responses, ["candidate-read-response", "candidate-refusal-response"]);
    writeln!(std::io::stderr(), "R1-01 corte=antes-ACK: parada pessoal real; token original preservado; leitura em voo e nova leitura recusadas no mesmo canal, sem corpo").unwrap();
    actor.conn.take().unwrap().kill().await;
}

#[tokio::test]
async fn api_candidata_ator_real_atende_parada_com_transporte_pendente() {
    #[cfg(target_os = "linux")]
    {
        Box::pin(ator_com_pipe_parcial("deadline")).await;
        Box::pin(ator_com_pipe_parcial("completion")).await;
        Box::pin(ator_com_pipe_parcial("shutdown")).await;
        Box::pin(ator_com_pipe_parcial("positive")).await;
    }
}

async fn drive<F: Future>(running: &mut (impl Future<Output = ()> + Unpin), action: F) -> F::Output {
    tokio::select! {
        result = action => result,
        () = running => panic!("ator terminou antes do corte"),
    }
}

#[cfg(target_os = "linux")]
async fn ator_com_pipe_parcial(action: &str) {
    use std::{io::Write, os::fd::AsRawFd};
    let mut f = Fixture::with_instructions(false, &"conteudo conferido\n".repeat(1200)).await;
    let (mut actor, mut results) = reading_actor(&mut f).await;
    actor.finish_turn(&results.recv().await.unwrap().result);
    let acp = actor.conn.as_ref().unwrap().acp.clone();
    let held = acp.hold_input().await;
    let fd = held.as_ref().unwrap().as_raw_fd();
    // Pipe real reduzido antes do próximo envio; frame da skill é maior que ele.
    // SAFETY: fd vem de ChildStdin vivo, retido por held; fcntl recebe apenas o tamanho inteiro.
    assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETPIPE_SZ, 4096) }, 4096);
    drop(held);
    *f.prompt_action.locked() = Some(json!({"readSkill":f.skill,"complete":action == "completion"}));
    let (cmd, commands) = mpsc::unbounded_channel();
    let mut running = Box::pin(actor.run(commands, None));
    assert!(futures::poll!(running.as_mut()).is_pending());
    let entry_id = pedido(&f, "pedido com leitura bloqueada");
    cmd.send(super::Cmd::Send {
        lane: crate::store::Lane::main("bot"),
        entry_id,
        text: "pedido com leitura bloqueada".into(),
    })
    .unwrap();
    drive(&mut running, f.request("session/prompt")).await.resume.send(()).unwrap();
    let blocked = drive(&mut running, f.request("candidate-blocked")).await;
    drive(&mut running, tokio::time::timeout(Duration::from_secs(20), acp.scoped_pending())).await.unwrap();
    let mut unread: libc::c_int = 0;
    // SAFETY: o corte mantém o writer vivo; unread é um c_int inicializado e válido para escrita.
    assert_eq!(unsafe { libc::ioctl(fd, libc::FIONREAD, &mut unread) }, 0);
    assert!(unread > 0, "o prefixo REAL deve existir antes da interrupção");
    assert!(f.hub.store.memory_boundary.try_lock().is_ok());
    assert!(f.hub.store.db.try_lock().is_ok());
    let record = crate::fork::bot_admission::current(&f.hub.store, "bot").unwrap().unwrap();
    assert_eq!(record.phase, crate::fork::bot_admission::Phase::Permitted);
    assert_eq!(f.hub.runtime("bot").memory_turn_token.as_deref(), Some(record.execution_token.as_str()));
    writeln!(
        std::io::stderr(),
        "R1-02 corte={action}: Actor::run, Ticket Permitted original, pipe Pending com prefixo real, locks livres"
    )
    .unwrap();
    let cards: Vec<_> = f
        .hub
        .store
        .history_in_session("bot", Some("bot"), i64::MAX, 50)
        .unwrap()
        .into_iter()
        .filter(|entry| entry.kind == "permission" && entry.data["status"] == "pending")
        .collect();
    assert_eq!(cards.len(), 1, "cartão ACP real publicado antes da leitura");
    let card = &cards[0];
    assert_eq!(card.data["options"][0]["optionId"], "allow");
    assert_eq!(f.hub.runtime("bot").status, BotStatus::NeedsInput);
    let (reply, mut permission_answered) = oneshot::channel();
    let option_id = Some("allow".into());
    cmd.send(super::Cmd::Permission { entry_id: card.id.clone(), option_id, reply, origem: None }).unwrap();
    assert!(futures::poll!(running.as_mut()).is_pending());
    let confirmed = f.hub.store.entry(&card.id).unwrap();
    assert_eq!(confirmed.data["status"], "answered", "resposta real validada antes do controle");
    assert_eq!(confirmed.data["selected"], "allow");
    writeln!(std::io::stderr(), "R1-02 corte={action}: cartão ACP real da mesma sessão; opção allow validada e confirmada antes do controle; leitor ainda bloqueado").unwrap();
    assert!(
        permission_answered
            .try_recv()
            .expect("R1-02: resposta ao cartão ACP real não pode prender Actor::run no stdin Pending")
            .is_ok(),
        "resposta legítima ao cartão deve ser aceita antes do controle"
    );
    match action {
        "completion" => {
            // Libera só a conclusão pelo stdout; o leitor de stdin continua parado.
            blocked.resume.send(()).unwrap();
            let draining = drive(&mut running, f.request("candidate-completed")).await;
            drive(&mut running, f.idle()).await;
            let held = acp.hold_input().await;
            assert!(held.is_none(), "fim processado pelo done_rx fecha prefixo parcial");
            drop(held);
            writeln!(
                std::io::stderr(),
                "R1-02 corte=completion: done_rx concluiu o turno e fechou prefixo parcial sem drenar stdin"
            )
            .unwrap();
            draining.resume.send(()).unwrap();
        }
        "deadline" => {
            cmd.send(super::Cmd::Stop).unwrap();
            let (reply, mut answered) = oneshot::channel();
            let option_id = Some("allow".into());
            cmd.send(super::Cmd::Permission { entry_id: card.id.clone(), option_id, reply, origem: None }).unwrap();
            assert!(futures::poll!(running.as_mut()).is_pending());
            assert!(
                answered.try_recv().is_ok(),
                "R1-02: Actor::run deve atender parada e comando seguinte sem liberar o pipe parcial"
            );
            let held = drive(&mut running, acp.hold_input()).await;
            assert!(held.is_none(), "cancelamento fecha o prefixo sem drenar o pipe");
            drop(held);
            // O leitor segue parado. O tick real precisa vencer CANCEL_GRACE e
            // retirar o processo sem depender de ACK, de stdin ou de done_rx.
            drive(&mut running, tokio::time::timeout(Duration::from_secs(20), acp.stopped())).await.unwrap();
            writeln!(std::io::stderr(), "R1-02 corte=deadline: tick venceu CANCEL_GRACE sem ACK e sem drenar stdin")
                .unwrap();
            blocked.resume.send(()).unwrap();
            drive(&mut running, f.request("candidate-completed")).await.resume.send(()).unwrap();
        }
        "shutdown" => {
            cmd.send(super::Cmd::Shutdown).unwrap();
            tokio::time::timeout(Duration::from_secs(20), &mut running).await.unwrap();
            assert!(acp.hold_input().await.is_none(), "Drop da entrega parcial fecha stdin");
            writeln!(
                std::io::stderr(),
                "R1-02 corte=shutdown: Actor::run terminou; Drop do prefixo parcial fechou stdin"
            )
            .unwrap();
            blocked.resume.send(()).unwrap();
            f.request("candidate-completed").await.resume.send(()).unwrap();
            return;
        }
        "positive" => {
            // Nenhuma revogação: o mesmo leitor volta legitimamente e recebe o frame inteiro.
            blocked.resume.send(()).unwrap();
            drive(&mut running, f.request("candidate-completed")).await.resume.send(()).unwrap();
            let received = drive(&mut running, f.requests.recv()).await.unwrap();
            assert_eq!(received.body["method"], "candidate-read-response");
            assert!(received.body["result"]["content"].as_str().unwrap().contains("conteudo conferido"));
            received.resume.send(()).unwrap();
            drive(&mut running, f.idle()).await;
            writeln!(
                std::io::stderr(),
                "R1-02 corte=positive: o mesmo leitor liberado recebeu o frame inteiro autorizado"
            )
            .unwrap();
        }
        _ => unreachable!(),
    }
    cmd.send(super::Cmd::Shutdown).unwrap();
    tokio::time::timeout(Duration::from_secs(20), &mut running).await.unwrap();
}

#[path = "seg_cf14_tests.rs"]
mod seg_cf14_tests;

/// Instruções do dono (todos os bots e Space): chegam no prompt de cada turno, em qualquer jeito de o motor receber o
/// texto de sistema, e uma mudança vale na mensagem seguinte sem reiniciar bot nem sessão.
async fn instrucao_geral_chega_e_muda_na_mensagem_seguinte(inline: bool) {
    let mut f = Fixture::new(inline).await;
    let dono = f.hub.clone();
    let salvar = |escopo: &'static str, texto: &'static str| {
        let hub = dono.clone();
        async move {
            let space = hub.store.bot("bot").unwrap().unwrap().config.space_id;
            let corpo = |revisao: &Value| match escopo {
                "geral" => json!({"escopo":"geral","texto":texto,"expectedRevision":revisao}),
                _ => json!({"escopo":"space","spaceId":space,"texto":texto,"expectedRevision":revisao}),
            };
            let estado = api::dispatch(&hub, &Caller::Local, "instrucaoGeral", json!({"spaceId":space})).await.unwrap();
            let revisao = if escopo == "geral" {
                estado["geral"]["revision"].clone()
            } else {
                estado["space"]["revision"].clone()
            };
            api::dispatch(&hub, &Caller::Local, "instrucaoGeralSalvar", corpo(&revisao)).await.unwrap();
        }
    };
    salvar("geral", "MARCA_GERAL_UM").await;
    salvar("space", "MARCA_SPACE_UM").await;
    f.send().await;
    f.request("session/new").await.resume.send(()).unwrap();
    let primeiro = f.request("session/prompt").await;
    let texto = primeiro.body["params"]["prompt"][0]["text"].as_str().unwrap().to_owned();
    assert_eq!(texto.matches("MARCA_GERAL_UM").count(), 1, "uma única cópia de cada campo");
    assert_eq!(texto.matches("MARCA_SPACE_UM").count(), 1);
    assert!(texto.find("MARCA_SPACE_UM").unwrap() < texto.find("MARCA_GERAL_UM").unwrap());
    primeiro.resume.send(()).unwrap();
    f.idle().await;
    salvar("geral", "MARCA_GERAL_DOIS").await;
    f.send().await;
    let segundo = f.request("session/prompt").await;
    let texto = segundo.body["params"]["prompt"][0]["text"].as_str().unwrap();
    assert!(
        texto.contains("MARCA_GERAL_DOIS") && !texto.contains("MARCA_GERAL_UM"),
        "o valor novo vale na mensagem seguinte"
    );
    assert!(texto.contains("MARCA_SPACE_UM"));
    segundo.resume.send(()).unwrap();
    f.idle().await;
    f.hub.shutdown().await;
}

#[tokio::test]
async fn instrucao_geral_no_prompt_com_texto_de_sistema_separado() {
    instrucao_geral_chega_e_muda_na_mensagem_seguinte(false).await;
}

#[tokio::test]
async fn instrucao_geral_no_prompt_com_perfil_na_primeira_mensagem() {
    instrucao_geral_chega_e_muda_na_mensagem_seguinte(true).await;
}
