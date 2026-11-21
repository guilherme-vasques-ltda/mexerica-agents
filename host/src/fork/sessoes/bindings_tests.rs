//! D70: somente identidade de origem e a fronteira da gravação.
#![allow(clippy::unwrap_used)]
use super::{
    bindings::*,
    origin::SessionOrigin,
    store::{NewSession, create},
};
use crate::{
    LockExt,
    api::devices::Caller,
    hub::{BotStatus, Hub},
    store::{EntryKind, Lane, ManagementOrigin, Store},
};
use serde_json::json;
use std::sync::Arc;

pub(super) struct Fixture {
    pub(super) hub: Arc<Hub>,
    pub(super) a: BindingRef,
    pub(super) b: BindingRef,
    dir: std::path::PathBuf,
}
impl Fixture {
    pub(super) fn new() -> Self {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store
            .save_bot(&serde_json::from_value(json!({"id":"bot","name":"Bot","connectors":["connector"]})).unwrap())
            .unwrap();
        store.save_bot(&serde_json::from_value(json!({"id":"other","name":"Outro"})).unwrap()).unwrap();
        let extra = create(
            &store.db.locked(),
            &NewSession {
                chat: "bot".into(),
                origin: SessionOrigin::Owner,
                source_session: None,
                parent_session: None,
                title: "B".into(),
                runs_on: None,
            },
        )
        .unwrap();
        let a = issue(&store, "bot", "bot", "bot", Some("acp-a")).unwrap();
        let b = issue(&store, "bot", "bot", &extra.id, Some("acp-b")).unwrap();
        store.memory_session_start("bot", "acp-a", false).unwrap();
        store.memory_session_start("bot", "acp-b", false).unwrap();
        let dir = std::env::temp_dir().join(format!("sessoes-binding-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let hub = Hub::new(
            store,
            "fixture".into(),
            crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
            "fixture".into(),
            0,
        );
        let f = Self { hub, a, b, dir };
        f.start(&f.a);
        f
    }
    fn start(&self, binding: &BindingRef) {
        Self::start_on(&self.hub, binding);
    }
    pub(super) fn start_on(hub: &Hub, binding: &BindingRef) {
        let lane = Lane {
            chat: binding.session_ref.chat_id.clone(),
            session: (binding.session_ref.id != binding.session_ref.chat_id).then(|| binding.session_ref.id.clone()),
        };
        let e = hub.store.insert_entry(&lane, EntryKind::User, 1, &json!({"text":"Pedido do dono"})).unwrap();
        // O ator real ativa após session/new/load, antes de publicar a ACP do turno.
        activate(&hub.store, binding, binding.acp_session.as_deref().unwrap()).unwrap();
        hub.set_runtime("bot", |r| {
            r.status = BotStatus::Working;
            r.lane = Some(lane);
            r.message = Some(e.id);
            r.session = binding.acp_session.clone();
            r.memory_turn_token = Some(uuid::Uuid::new_v4().to_string());
        });
    }
    pub(super) fn execution(&self) -> ExecutionRef {
        capture(&self.hub, "bot", self.a.id.as_str()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn mcp_de_a_durante_b_e_binding_de_outro_bot_sao_recusados_em_cada_consumidor() {
    let f = Fixture::new();
    let original = f.execution();
    f.start(&f.b);
    for (method, name, args) in [
        ("chatCall", "send_message", json!({"text":"não publicar"})),
        ("chatCall", "react", json!({"emoji":"👍"})),
        ("chatCall", "send_file", json!({"path":"/arquivo-inexistente"})),
        ("manageCall", "create_bot", json!({"name":"não criar"})),
        ("computersCall", "list_computers", json!({})),
        ("memoryCall", "remember", json!({"text":"não guardar"})),
        ("memoryCall", "edit_memory", json!({"file":"a.md","content":"não guardar"})),
        ("memoryCall", "search_history", json!({"query":"Pedido"})),
        ("memoryCall", "open_history", json!({"sourceRef":"inválida"})),
        ("routineCall", "list_routines", json!({})),
    ] {
        let result = crate::api::dispatch(
            &f.hub,
            &Caller::Local,
            method,
            json!({"botId":"bot","sessionBinding":f.a.id.as_str(),"sessionSlot":"main","name":name,"arguments":args}),
        )
        .await;
        assert!(result.unwrap_err().to_string().contains("chamada vencida"), "{method}/{name}");
    }
    assert!(capture(&f.hub, "other", f.a.id.as_str()).is_err());
    // Também recusa quando a requisição foi validada antes de entrar na fila/await.
    assert!(
        crate::chat::outbox::call(&f.hub, "bot", "send_message", &json!({"text":"atrasada"}), Some(&original))
            .await
            .is_err()
    );
    assert!(
        crate::chat::manage::call_in_execution(&f.hub, "bot", "list_team", &json!({}), Some(&original)).await.is_err()
    );
    assert!(crate::fork::permission_card::request_session(&json!({"sessionId":"acp-a"}), Some("acp-b")).is_err());
}

#[test]
fn revogar_antes_da_transacao_impede_entrada_memoria_gestao_e_revogar_depois_preserva_commit() {
    let f = Fixture::new();
    let creator = f.hub.store.bot("bot").unwrap().unwrap().config;
    let creator_skills = crate::fork::skill_selection::creator_version(&f.hub, &creator).unwrap();
    let execution = f.execution();
    assert_execution_live(&f.hub.store.db.locked(), &execution).unwrap();
    let binding = resolve(&f.hub.store.db.locked(), f.a.id.as_str(), "bot").unwrap();
    let entry = f
        .hub
        .store
        .insert_visible_in_execution(
            &Lane::main("bot"),
            EntryKind::Agent,
            1,
            &json!({"text":"confirmada"}),
            Some(&execution),
        )
        .unwrap()
        .1;
    revoke(&f.hub.store, "bot", Some("acp-a")).unwrap();
    assert!(f.hub.store.entry(&entry.id).is_some(), "revogação após commit não desfaz a gravação");
    assert!(assert_binding_live(&f.hub.store.db.locked(), &binding).is_err());
    let before = f.hub.store.history("bot", i64::MAX, 100).unwrap().len();
    assert!(
        f.hub
            .store
            .insert_visible_in_execution(
                &Lane::main("bot"),
                EntryKind::Agent,
                1,
                &json!({"text":"proibida"}),
                Some(&execution)
            )
            .is_err()
    );
    assert_eq!(f.hub.store.history("bot", i64::MAX, 100).unwrap().len(), before);
    assert!(
        f.hub
            .store
            .memory_write_in_execution(Some(&execution), |c, _, _| {
                c.execute("INSERT INTO kv(k,v) VALUES('proibida','sim')", [])?;
                Ok(())
            })
            .is_err()
    );
    assert!(f.hub.store.kv_get("proibida").is_none());
    let cfg = serde_json::from_value(json!({"id":"nao-criar","name":"Não criar"})).unwrap();
    assert!(
        f.hub
            .store
            .save_team_creation_in_execution(
                &cfg,
                "{}",
                &("team.pending.teste".into(), "{}".into()),
                &creator_skills,
                Some(&execution),
                ManagementOrigin { manager: "manager", observed_external: || false },
            )
            .is_err()
    );
    assert!(f.hub.store.bot("nao-criar").unwrap().is_none());
}

#[test]
fn a1_b_a2_mantem_limitacao_anterior_sem_exigir_token_do_motor() {
    let f = Fixture::new();
    let a1 = f.execution();
    assert_eq!(
        a1.turn_token(),
        f.hub.runtime("bot").memory_turn_token.as_deref().unwrap(),
        "ExecutionRef referencia o token do Hub, sem gerar outro"
    );
    f.start(&f.b);
    assert!(recheck(&f.hub, &a1).is_err());
    f.start(&f.a);
    let a2 = f.execution();
    assert_ne!(a1, a2, "uma futura cerca de parar A₁ não deve identificar A₂");
    assert!(
        recheck(&f.hub, &a1).is_ok(),
        "D229 A: a mesma sessão continua como antes; não há correlação do prompt MCP"
    );
    assert!(
        f.hub
            .store
            .insert_visible_in_execution(&Lane::main("bot"), EntryKind::Agent, 2, &json!({"text":"A₂"}), Some(&a1))
            .is_ok()
    );
}

#[test]
fn gateway_atrasado_marca_a_nunca_b_e_abertura_absorve_so_a_propria_pendencia() {
    let f = Fixture::new();
    let collision = issue(&f.hub.store, "bot", "bot", &f.b.session_ref.id, None).unwrap();
    assert!(activate(&f.hub.store, &collision, "acp-a").is_err(), "ACP viva de A não pode ser remapeada para B");
    f.start(&f.b);
    crate::fork::memory::gateway::observe(&f.hub, "bot", f.a.id.as_str()).unwrap();
    assert!(f.hub.store.memory_session_marked("bot", "acp-a").unwrap());
    assert!(!f.hub.store.memory_session_marked("bot", "acp-b").unwrap());
    let pending = issue(&f.hub.store, "bot", "bot", "bot", None).unwrap();
    crate::fork::memory::gateway::observe(&f.hub, "bot", pending.id.as_str()).unwrap();
    activate(&f.hub.store, &f.b, "acp-b").unwrap();
    assert!(!f.hub.store.memory_session_marked("bot", "acp-b").unwrap());
    activate(&f.hub.store, &pending, "acp-new").unwrap();
    assert!(f.hub.store.memory_session_marked("bot", "acp-new").unwrap());
    revoke(&f.hub.store, "bot", Some("acp-new")).unwrap();
    assert!(crate::fork::memory::gateway::observe(&f.hub, "bot", pending.id.as_str()).is_err());
}

#[test]
fn encerrar_e_apagar_revogam_vinculos_no_mesmo_commit() {
    let f = Fixture::new();
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE donna_bot_sessions SET state='closed' WHERE id=?1", [&f.b.session_ref.id])
        .unwrap();
    assert!(resolve(&f.hub.store.db.locked(), f.b.id.as_str(), "bot").is_err());
    f.hub.store.delete_bot("bot").unwrap();
    assert!(resolve(&f.hub.store.db.locked(), f.a.id.as_str(), "bot").is_err());
}

#[test]
fn conversa_de_space_tem_vinculo_proprio_sem_herdar_a_principal() {
    let f = Fixture::new();
    let conversation = "conversa-fixture";
    super::ensure_principal(&f.hub.store.db.locked(), conversation).unwrap();
    assert_eq!(super::exposure::visible("bot", Some("conversation:conversa-fixture")), conversation);
    let binding = issue(&f.hub.store, "bot", conversation, conversation, Some("acp-space")).unwrap();
    activate(&f.hub.store, &binding, "acp-space").unwrap();
    f.hub.set_runtime("bot", |r| {
        r.lane = Some(Lane::main(conversation));
        r.session = Some("acp-space".into());
    });
    assert!(capture(&f.hub, "bot", binding.id.as_str()).is_ok());
    assert!(capture(&f.hub, "bot", f.a.id.as_str()).is_err());
    crate::fork::memory::gateway::observe(&f.hub, "bot", binding.id.as_str()).unwrap();
    assert!(!f.hub.store.memory_session_marked("bot", "acp-a").unwrap());
}

#[tokio::test]
async fn retorno_do_gateway_de_a_durante_b_e_recusado_sem_gravar_exposicao() {
    let f = Fixture::new();
    let context = crate::guard_hub::mcp::contexto::Context::issue_bound(
        &f.hub.store,
        "bot",
        "connector",
        "bot",
        None,
        Some(f.a.id.as_str()),
    )
    .unwrap();
    let namespace = f.hub.store.guard_runtime_namespace().unwrap();
    let bridge = uuid::Uuid::new_v4().to_string();
    crate::guard_hub::mcp::runtime::remember(&namespace, "connector", &context.id, &bridge, &[]);
    f.start(&f.b);
    let result = crate::api::dispatch(
        &f.hub,
        &Caller::Local,
        "donnaGuardObserve",
        json!({
            "context":context.id,"connector":"connector","bridgeId":bridge,"method":"tools/call",
            "frame":{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"conteúdo de A"}]}}
        }),
    )
    .await;
    assert!(result.unwrap_err().to_string().contains("chamada vencida"));
    assert!(!f.hub.store.memory_session_marked("bot", "acp-a").unwrap());
    assert_eq!(f.hub.store.guard_test_snapshot().unwrap(), (vec![], 0));
    assert!(!f.hub.store.memory_session_marked("bot", "acp-b").unwrap());
}

#[test]
fn abertura_atrasada_depois_da_troca_de_conta_ou_motor_nao_reativa_origem_antiga() {
    let f = Fixture::new();
    let original = f.hub.store.bot("bot").unwrap().unwrap().config;
    let mut changed = original.clone();
    changed.backend = "custom".into();
    // D372: o motor da principal é a seleção dela, não o padrão do bot; aqui ela passa a ser a do motor novo.
    let mut engine = super::engine_choice::SessionEngine::snapshot(&original);
    engine.backend = "custom".into();
    let raw = serde_json::to_string(&engine).unwrap();
    f.hub.store.db.locked().execute("UPDATE donna_bot_sessions SET engine_choice=?1 WHERE id='bot'", [raw]).unwrap();
    let pending = issue(&f.hub.store, "bot", "bot", "bot", None).unwrap();
    assert!(activate_for_config(&f.hub.store, &pending, "acp-new", &original).is_err());
    assert!(activate_for_config(&f.hub.store, &pending, "acp-new", &changed).is_ok());
}

/// D372: a promoção canônica da seleção EFETIVA para outro motor ou conta revoga, no mesmo comando, só os vínculos da
/// própria conversa (principal ou extra); rotina/canal da mesma sessão e a outra conversa ficam. Só modelo/opções e a
/// troca v1→v2 com os mesmos valores não revogam. Gravação ilegível não quebra o UPDATE e a leitura falha fechado.
#[test]
fn promocao_da_selecao_efetiva_revoga_so_a_conversa_quando_motor_ou_conta_mudam() {
    let f = Fixture::new();
    let store = &f.hub.store;
    let extra = f.b.session_ref.id.clone();
    let (first, second) = (format!("ac_{}", "a".repeat(64)), format!("ac_{}", "b".repeat(64)));
    let v2 = |backend: &str, account: &str, model: &str| -> String {
        let engine = json!({"schemaVersion":2,"source":"own","accountId":account,"backend":backend,"model":model,
            "modelOptions":{},"fastMode":false});
        engine.to_string()
    };
    let v1 = json!({"schemaVersion":1,"accountId":first,
        "selection":{"backend":"codex","model":"m1","modelOptions":{}}});
    // Estado de partida gravado antes dos vínculos do teste: principal em v1 (A, codex), extra em v2 (A, codex).
    let initial = "UPDATE donna_bot_sessions SET engine_choice=?2 WHERE id=?1";
    store.db.locked().execute(initial, ["bot", v1.to_string().as_str()]).unwrap();
    store.db.locked().execute(initial, [extra.as_str(), v2("codex", &first, "m1").as_str()]).unwrap();
    let promote = |session: &str, next: &str| {
        let mut c = store.db.locked();
        let tx = c.transaction().unwrap();
        tx.execute("UPDATE donna_bot_sessions SET engine_next=?2 WHERE id=?1", [session, next]).unwrap();
        assert!(super::engine_choice::promote(&tx, session).unwrap());
        tx.commit().unwrap();
    };
    let revoked = |binding: &BindingRef| -> bool {
        let sql = "SELECT revoked_at IS NOT NULL FROM donna_session_bindings WHERE id=?1";
        store.db.locked().query_row(sql, [binding.id.as_str()], |row| row.get(0)).unwrap()
    };
    // Produtor canônico com ACPs distintas: `issue_in` revoga os pendentes (sem ACP) do bot, então cada vínculo nasce
    // com a sua para coexistir. Slots reais: principal NULL, extra `session:<id>`, rotina, canal e conversa de Space.
    let emit = |chat: &str, session: &str, slot: Option<&str>, acp: &str| {
        issue_in(store, "bot", chat, session, slot, Some(acp)).unwrap()
    };
    let main = emit("bot", "bot", None, "acp-main-1");
    let routine = emit("bot", "bot", Some("routine:r1"), "acp-rotina");
    let other = emit("bot", &extra, Some(format!("session:{extra}").as_str()), "acp-extra");
    let channel = emit("bot", "bot", Some("group:g"), "acp-canal");
    super::ensure_principal(&store.db.locked(), "conversa-fixture").unwrap();
    let space = emit("conversa-fixture", "conversa-fixture", Some("conversation:conversa-fixture"), "acp-space");
    let untouched = || !revoked(&routine) && !revoked(&channel) && !revoked(&space);
    // Precondição: todos vivos antes da primeira promoção.
    for binding in [&main, &routine, &other, &channel, &space] {
        assert!(!revoked(binding), "vínculo {:?} já revogado antes da promoção", binding.slot);
    }
    // v1→v2 com os mesmos valores, depois só modelo: nada revoga.
    promote("bot", &v2("codex", &first, "m1"));
    promote("bot", &v2("codex", &first, "m2"));
    assert!(!revoked(&main) && !revoked(&routine) && !revoked(&other));
    // Só a PRÓXIMA (troca durante o turno) não revoga: a efetiva não mudou.
    let next = "UPDATE donna_bot_sessions SET engine_next=?2 WHERE id=?1";
    store.db.locked().execute(next, ["bot", v2("claude", &second, "m2").as_str()]).unwrap();
    assert!(!revoked(&main), "engine_next sozinha não revoga");
    // Outra conta: só a própria conversa.
    promote("bot", &v2("codex", &second, "m2"));
    assert!(revoked(&main), "conta nova revoga o vínculo da principal");
    assert!(!revoked(&routine), "rotina segue o padrão do bot");
    assert!(!revoked(&other), "a extra é outra conversa");
    // Outro motor.
    let main = emit("bot", "bot", None, "acp-main-2");
    promote("bot", &v2("claude", &second, "m2"));
    assert!(revoked(&main) && !revoked(&routine) && !revoked(&other));
    // Na extra: o vínculo dela, nunca o da principal.
    let main = emit("bot", "bot", None, "acp-main-3");
    promote(extra.as_str(), &v2("claude", &first, "m1"));
    assert!(revoked(&other) && !revoked(&main) && !revoked(&routine));
    assert!(untouched(), "rotina, canal e Space não seguem a seleção da conversa");
    // Ilegível: o UPDATE não quebra (revoga, falha fechado) e a leitura canônica continua recusando.
    let main = emit("bot", "bot", None, "acp-main-4");
    store.db.locked().execute("UPDATE donna_bot_sessions SET engine_choice='{' WHERE id='bot'", []).unwrap();
    assert!(revoked(&main) && untouched());
    assert!(super::engine_choice::read(&store.db.locked(), "bot").is_err());
}

#[test]
fn reinicio_retira_admissao_sem_perder_a_origem_do_vinculo() {
    let f = Fixture::new();
    let execution = f.execution();
    reset_runtime(&f.hub.store).unwrap();
    assert!(assert_execution_live(&f.hub.store.db.locked(), &execution).is_err());
    assert_eq!(for_acp(&f.hub.store, "bot", "acp-a").unwrap().id, f.a.id);
}

#[tokio::test]
async fn projecao_de_audiencia_nao_expoe_execucao_privada_nem_autoriza_captura_antiga() {
    let f = Fixture::new();
    let original = f.execution();
    let mut projected = f.hub.bot_json(&f.hub.store.bot("bot").unwrap().unwrap());
    assert_eq!(projected["status"], "working");
    for field in ["execution", "sessionBinding", "memoryTurnToken", "acpSession", "turnSession"] {
        assert!(projected.get(field).is_none(), "{field} não pertence ao fio do app");
    }
    let text = projected.to_string();
    assert!(!text.contains(f.a.id.as_str()));
    assert!(!text.contains(f.hub.runtime("bot").memory_turn_token.as_deref().unwrap()));
    f.start(&f.b);
    capture(&f.hub, "bot", f.b.id.as_str()).unwrap();
    assert!(recheck(&f.hub, &original).is_err(), "a projeção pública não adota o binding/turno de B");
    let before = f.hub.store.kv_read("routines.v1").unwrap();
    assert!(
        crate::api::dispatch(
            &f.hub,
            &Caller::Local,
            "routineCall",
            json!({"botId":"bot","sessionBinding":f.a.id.as_str(),"name":"list_routines","arguments":{}}),
        )
        .await
        .is_err()
    );
    assert_eq!(f.hub.store.kv_read("routines.v1").unwrap(), before);
    f.hub.set_memory_runtime("bot", |runtime| {
        runtime.status = BotStatus::Idle;
        runtime.lane = None;
        runtime.session = None;
    });
    crate::fork::runtime_audiencia::revalidate_event(&f.hub, crate::fork::spaces::PERSONAL, &mut projected);
    assert_eq!(projected["status"], "unavailable", "snapshot Working antigo não herda a observação nova");
    assert_eq!(projected["activity"], "");
    assert!(projected["activityCode"].is_null(), "o código sai junto do texto na redação");
    for field in ["startedAt", "resumeAt", "workingChat"] {
        assert!(projected[field].is_null(), "{field}");
    }
    assert_eq!(f.hub.runtime("bot").status, BotStatus::Idle, "indisponível só existe na projeção");
}

#[test]
fn o_motor_gravado_pelo_ator_e_o_que_o_porteiro_le() {
    let f = Fixture::new();
    let db = || f.hub.store.db.locked();
    assert_eq!(backend(&db(), f.a.id.as_str()).unwrap(), None, "vínculo sem motor gravado não prova motor");
    let mut cfg = f.hub.store.bot("bot").unwrap().unwrap().config;
    cfg.backend = "kilo".into();
    f.hub.store.save_bot(&cfg).unwrap();
    record_backend(&f.hub.store, &f.a, &cfg.backend).unwrap();
    assert_eq!(backend(&db(), f.a.id.as_str()).unwrap().as_deref(), Some("kilo"));
    cfg.backend = "claude".into();
    f.hub.store.save_bot(&cfg).unwrap();
    assert_eq!(
        backend(&db(), f.a.id.as_str()).unwrap().as_deref(),
        Some("kilo"),
        "trocar o padrão do bot depois não muda o motor registrado para a execução em andamento"
    );
    revoke(&f.hub.store, "bot", Some("acp-a")).unwrap();
    assert_eq!(backend(&db(), f.a.id.as_str()).unwrap(), None, "vínculo revogado não prova nada");
}

#[test]
fn pedido_de_turno_anterior_nao_passa_pelo_recheck_de_turno() {
    let f = Fixture::new();
    let original = f.execution();
    recheck_turn(&f.hub, &original).unwrap();
    f.hub.set_runtime("bot", |r| r.memory_turn_token = Some(uuid::Uuid::new_v4().to_string()));
    assert!(recheck(&f.hub, &original).is_ok(), "o recheck antigo só confere o vínculo");
    assert!(recheck_turn(&f.hub, &original).is_err(), "o pedido do turno anterior precisa ser rejeitado");
}
