//! Fronteiras de origem: chamadas reais, mudança durante espera e cartões duráveis.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::*;
use crate::{
    LockExt,
    fork::memory_auto::{apply, queue, transport},
    remote::identity::Identity,
    store::{EntryKind, Store},
};
use rusqlite::params;
use serde_json::json;

pub(crate) struct Fixture {
    pub(crate) hub: Arc<Hub>,
    pub(crate) cfg: BotConfig,
    pub(crate) body: Value,
    pub(crate) dir: std::path::PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
impl Fixture {
    pub(crate) fn new() -> Self {
        Self::with_shared(None)
    }
    fn with_shared(shared: Option<bool>) -> Self {
        Self::with_backend(shared, "codex")
    }
    pub(crate) fn with_inherited_permission() -> Self {
        Self::with_profile(None, "codex", |store, cfg| {
            let creator = uuid::Uuid::new_v4().to_string();
            let creator_cfg: BotConfig =
                serde_json::from_value(json!({"id":creator,"name":"Criador","cwd":cfg.cwd,"permission":"auto"}))
                    .unwrap();
            store.save_bot(&creator_cfg).unwrap();
            store.set_team_settings(&json!({"permission":"auto"})).unwrap();
            cfg.created_by = Some(creator);
            cfg.permission = crate::store::Permission::Auto;
            cfg.permission_chosen_by_user = false;
        })
    }
    fn with_backend(shared: Option<bool>, backend: &str) -> Self {
        Self::with_profile(shared, backend, |_, _| {})
    }
    pub(crate) fn with_profile(
        shared: Option<bool>,
        backend: &str,
        prepare: impl FnOnce(&Store, &mut BotConfig),
    ) -> Self {
        Self::with_store(shared, backend, prepare, |dir| Store::open(&dir.join("store.db")).unwrap())
    }
    pub(crate) fn with_memory_profile(prepare: impl FnOnce(&Store, &mut BotConfig)) -> Self {
        Self::with_store(None, "codex", prepare, |_| Store::open(std::path::Path::new(":memory:")).unwrap())
    }
    fn with_store(
        shared: Option<bool>,
        backend: &str,
        prepare: impl FnOnce(&Store, &mut BotConfig),
        open: impl FnOnce(&std::path::Path) -> Store,
    ) -> Self {
        let dir =
            std::fs::canonicalize(std::env::temp_dir()).unwrap().join(format!("tool-context-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = open(&dir);
        let bot = uuid::Uuid::new_v4().to_string();
        let mut original: BotConfig =
            serde_json::from_value(json!({"id":bot,"name":"Donna","backend":backend,"cwd":dir,"sharedMemory":shared}))
                .unwrap();
        prepare(&store, &mut original);
        store.save_bot(&original).unwrap();
        let account = format!("ac_{}", "1".repeat(64));
        store
            .kv_set(
                "fork.engine-accounts.v1",
                &json!({"salt":uuid::Uuid::new_v4().to_string(),"accounts":[{
                    "id":account,"backend":backend,"billing":"paid_api","state":"connected","scope":"owner",
                    "launch":{"kind":"cli_profile","executable":"/fixture/cli","directory":"/fixture/config"},
                    "quotaIdentity":null,"verifiedAt":null,"subscriptionProof":null,"models":[]
                }]})
                .to_string(),
            )
            .unwrap();
        let chat = uuid::Uuid::new_v4().to_string();
        let (account_revision, account_stamp) = crate::fork::spaces::binding::account(&store, &account).unwrap();
        {
            let db = store.db.locked();
            let rev = crate::store::next_rev(&db).unwrap();
            db.execute("INSERT INTO donna_space_conversations(id,space_id,bot_id,title,revision,is_primary,engine_account_id,account_revision,account_stamp) VALUES(?1,?2,?3,'Adicional',?4,0,?5,?6,?7)",
                params![chat, crate::fork::spaces::PERSONAL, bot, rev, account, i64::try_from(account_revision).unwrap(), account_stamp]).unwrap();
        }
        let cfg = crate::fork::spaces::execution_config(&store, &original, &chat).unwrap();
        let hub = Hub::new(store, "test".into(), Identity::load_or_create(&dir).unwrap(), "test".into(), 1);
        let stamp = crate::fork::spaces::execution_stamp(&hub.store, &cfg).unwrap();
        let entry = hub.add_entry(&Lane::main(&chat), EntryKind::User, 1,
            &json!({"text":"Lembre que gosto de café.","executionBinding":stamp,"attachments":[],"contentOrigin":{"status":"appReported","segments":[{"kind":"typed","start":0,"end":24}],"attachments":[]}})).unwrap();
        hub.store.memory_session_start(&bot, "sessao-A", false).unwrap();
        hub.start_bot_turn(&bot, |r| {
            r.lane = Some(Lane::main(&chat));
            r.message = Some(entry.id);
            r.session = Some("sessao-A".into());
            r.started_at = Some(1);
            r.status = BotStatus::Working;
        })
        .unwrap();
        let definitions = json!([{"env":[{"name":BINDING_ENV,"value":"vinculo-A"}]}]);
        super::prepare(&hub, &cfg, &definitions).unwrap();
        activate(&hub, &cfg, &definitions, "sessao-A").unwrap();
        crate::fork::sessoes::ensure_principal(&hub.store.db.locked(), &chat).unwrap();
        let session_binding = crate::fork::sessoes::bindings::issue(&hub.store, &bot, &chat, &chat, None).unwrap();
        crate::fork::sessoes::bindings::activate_for_config(&hub.store, &session_binding, "sessao-A", &cfg).unwrap();
        let body = json!({"botId":bot,"spaceId":cfg.space_id,"sessionSlot":format!("conversation:{chat}"),"toolBinding":"vinculo-A","sessionBinding":session_binding.id.as_str(),"name":"remember","arguments":{"text":"A pessoa gosta de café."}});
        Self { hub, cfg, body, dir }
    }
    fn memory(shared: Option<bool>) -> Self {
        Self::memory_for_backend(shared, "claude")
    }
    fn memory_for_backend(shared: Option<bool>, backend: &str) -> Self {
        let mut f = Self::with_backend(shared, backend);
        let lane = Lane::main(f.cfg.session_owner());
        let message = f.hub.runtime(&f.cfg.id).message.unwrap();
        f.hub.store.memory_session_turn(&f.cfg.id, &lane.chat, "sessao-A", 1).unwrap();
        f.hub.store.memory_turn_begin(&f.cfg.id, &lane.chat, "sessao-A", &[1], "owner").unwrap();
        transport::new_generation(&f.hub, &f.cfg, &lane.chat, backend).unwrap();
        queue::begin(&f.hub, &f.cfg, &lane, "sessao-A", &[message]).unwrap();
        let slot = format!("conversation:{}", lane.chat);
        let capability = transport::issue(&f.hub, &f.cfg, &lane.chat, Some(&slot), Some("sessao-A")).unwrap();
        let servers = json!([{"name":"memory","env":[{"name":transport::ENV,"value":capability}]}]);
        transport::bind(&f.hub, &f.cfg, &lane.chat, Some(&slot), &servers, "sessao-A").unwrap();
        f.body["arguments"][transport::ARG] = capability.into();
        f
    }
    fn complete_proposals(&self) -> Vec<String> {
        let job = queue::active_job(&self.hub, &self.cfg.id).unwrap().unwrap();
        queue::checkpoint(
            &self.hub.store,
            "fixture.completion",
            r#"{"completion":{"turn":1}}"#,
            Some(&job),
            true,
            false,
        )
        .unwrap();
        let reservation = queue::reserve_job(&self.hub.store, &job).unwrap().unwrap();
        match apply::apply_prepared(&self.hub, &reservation).unwrap() {
            apply::PreparedOutcome::Applied(ids) => ids,
            outcome => panic!("proposta não aplicada após sucesso: {outcome:?}"),
        }
    }
    pub(crate) fn context(&self) -> ToolContext {
        ToolContext::capture(&self.hub, &self.body, self.cfg.session_owner()).unwrap()
    }
}

#[tokio::test]
async fn memoria_da_adicional_so_grava_apos_sucesso_com_recibo_e_undo_na_origem() {
    let f = Fixture::memory(None);
    f.hub
        .add_entry(
            &Lane::main(f.cfg.session_owner()),
            EntryKind::User,
            2,
            &json!({"text":"Mensagem posterior na fila","status":"queued"}),
        )
        .unwrap();
    let saved = route(&f.hub, "memoryCall", &f.body).await.unwrap().unwrap();
    assert_eq!(saved["status"], "prepared");
    assert_eq!(
        f.hub.store.memory_read(|db| codync_mem::store::layer(db, Some(&f.cfg.id))).unwrap(),
        [] as [codync_mem::store::Entry; 0],
    );
    let ids = f.complete_proposals();
    assert_eq!(ids.len(), 1);
    let memory = f.hub.store.memory_read(|db| codync_mem::store::layer(db, Some(&f.cfg.id))).unwrap().remove(0);
    assert_eq!(memory.bot.as_deref(), Some(f.cfg.id.as_str()));
    let notices = f.hub.store.history(f.cfg.session_owner(), i64::MAX, 30).unwrap();
    let card = notices.iter().find(|e| e.data["memory"].is_object()).unwrap();
    assert_eq!(card.turn, 1, "aviso conserva o turno emissor, não a mensagem posterior na fila");
    assert!(f.hub.store.history(&f.cfg.id, i64::MAX, 30).unwrap().is_empty());
    crate::fork::memory::notices::undo(&f.hub, &card.id).unwrap();
    assert_eq!(f.hub.store.entry(&card.id).unwrap().bot_id, f.cfg.session_owner());
}

#[tokio::test]
async fn recibo_de_memoria_sobrevive_a_reinicio_sem_confirmacao_nem_troca_de_destino() {
    let f = Fixture::memory(None);
    f.hub.store.memory_session_mark(&f.cfg.id, "sessao-A", "teste externo").unwrap();
    let prepared = route(&f.hub, "memoryCall", &f.body).await.unwrap().unwrap();
    assert_eq!(prepared["status"], "prepared");
    assert!(
        f.hub.store.history(f.cfg.session_owner(), i64::MAX, 30).unwrap().iter().all(|e| !e.data["memory"].is_object())
    );
    let ids = f.complete_proposals();
    assert_eq!(ids.len(), 1);
    let card = f.hub.store.entry(&ids[0]).unwrap();
    assert_eq!(card.data["memory"]["confirmation"], false);
    assert_eq!(card.data["memory"]["origin"]["kind"], "external");
    assert!(
        crate::fork::memory::notices::answer_confirmed(&f.hub, &card.id, f.cfg.session_owner(), card.rev, true)
            .is_err()
    );
    let restarted = Hub::new(
        Store::open(&f.dir.join("store.db")).unwrap(),
        "test".into(),
        Identity::load_or_create(&f.dir).unwrap(),
        "test".into(),
        0,
    );
    let undone = crate::fork::memory::notices::undo(&restarted, &card.id).unwrap();
    assert_eq!(undone["entry"]["botId"], f.cfg.session_owner());
    assert_eq!(undone["entry"]["data"]["memory"]["status"], "undone");
    assert!(restarted.store.history(&f.cfg.id, i64::MAX, 30).unwrap().is_empty());
}

#[tokio::test]
async fn guarda_recusa_contexto_fechado_outro_bot_outro_space_e_binding_aba() {
    let f = Fixture::new();
    for (field, value) in [
        ("botId", json!(uuid::Uuid::new_v4().to_string())),
        ("spaceId", json!(uuid::Uuid::new_v4().to_string())),
        ("toolBinding", json!("forjado")),
        ("sessionSlot", json!(format!("conversation:{}", uuid::Uuid::new_v4()))),
    ] {
        let mut body = f.body.clone();
        body[field] = value;
        assert!(route(&f.hub, "memoryCall", &body).await.is_err());
    }
    for field in ["sessionSlot", "toolBinding"] {
        let mut body = f.body.clone();
        body.as_object_mut().unwrap().remove(field);
        assert!(route(&f.hub, "memoryCall", &body).await.is_err());
    }
    for method in ["routineCall", "manageCall", "computerCall", "computersCall"] {
        assert!(route(&f.hub, method, &f.body).await.is_err());
    }
    let context = f.context();
    let other: BotConfig =
        serde_json::from_value(json!({"id":uuid::Uuid::new_v4().to_string(),"name":"Outro bot","cwd":f.dir})).unwrap();
    f.hub.store.save_bot(&other).unwrap();
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE donna_space_conversations SET bot_id=?2 WHERE id=?1", params![f.cfg.session_owner(), other.id])
        .unwrap();
    assert!(context.check(&f.hub).is_err());
    f.hub
        .store
        .db
        .locked()
        .execute(
            "UPDATE donna_space_conversations SET bot_id=?2,space_id=?3 WHERE id=?1",
            params![f.cfg.session_owner(), f.cfg.id, uuid::Uuid::new_v4().to_string()],
        )
        .unwrap();
    assert!(context.check(&f.hub).is_err());
    assert!(
        f.hub.store.kv_prefix("donna.space.memory.").is_empty(),
        "recusa não pode criar namespace foreign por acidente"
    );
    f.hub
        .store
        .db
        .locked()
        .execute(
            "UPDATE donna_space_conversations SET space_id=?2 WHERE id=?1",
            params![f.cfg.session_owner(), f.cfg.space_id],
        )
        .unwrap();
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE donna_space_conversations SET closed=1 WHERE id=?1", [f.cfg.session_owner()])
        .unwrap();
    assert!(context.check(&f.hub).is_err());
    f.hub
        .store
        .db
        .locked()
        .execute(
            "UPDATE donna_space_conversations SET closed=0,revision=revision+2 WHERE id=?1",
            [f.cfg.session_owner()],
        )
        .unwrap();
    assert!(context.check(&f.hub).is_err());
    assert!(f.hub.store.history(&f.cfg.id, i64::MAX, 30).unwrap().is_empty());
}

#[tokio::test]
async fn espera_nao_adota_turno_novo_nem_outra_conversa_apos_await() {
    let f = Fixture::new();
    let context = f.context();
    let (ready, waiting) = tokio::sync::oneshot::channel();
    let hub = f.hub.clone();
    let body = f.body.clone();
    let task = tokio::spawn(async move {
        waiting.await.unwrap();
        context.check(&hub)?;
        crate::chat::memory::tool_call_in(&hub, &context.cfg.id, "remember", &body["arguments"], Some(&context)).await
    });
    f.hub.set_runtime(&f.cfg.id, |r| {
        r.memory_turn_token = Some("turno-B".into());
        r.lane = Some(Lane::main(&f.cfg.id));
    });
    ready.send(()).unwrap();
    assert!(task.await.unwrap().is_err());
    assert_eq!(f.hub.store.history(f.cfg.session_owner(), i64::MAX, 30).unwrap().len(), 1);
    assert!(f.hub.store.history(&f.cfg.id, i64::MAX, 30).unwrap().is_empty());
}

#[tokio::test]
async fn publicacao_apos_espera_recusa_configuracao_revogada_mesmo_com_ator_antigo() {
    let f = Fixture::new();
    let context = f.context();
    assert!(!needs_rebind(&f.hub, &f.cfg).unwrap());
    let (ready, waiting) = tokio::sync::oneshot::channel();
    let hub = f.hub.clone();
    let old_cfg = f.cfg.clone();
    let lane = context.lane();
    let task = tokio::spawn(async move {
        waiting.await.unwrap();
        context.check_actor(&hub, &old_cfg, &lane, Some("sessao-A"))
    });
    let mut changed = f.hub.store.bot(&f.cfg.id).unwrap().unwrap().config;
    changed.computer = true;
    f.hub.store.save_bot(&changed).unwrap();
    let fresh_cfg = crate::fork::spaces::execution_config(&f.hub.store, &changed, f.cfg.session_owner()).unwrap();
    assert!(needs_rebind(&f.hub, &fresh_cfg).unwrap(), "próximo turno deve receber novas definições MCP");
    ready.send(()).unwrap();
    assert!(task.await.unwrap().is_err(), "cópia terminada não pode publicar com configuração antiga do ator");
    assert!(f.hub.store.history(&f.cfg.id, i64::MAX, 30).unwrap().is_empty());
    assert_eq!(f.hub.store.history(f.cfg.session_owner(), i64::MAX, 30).unwrap().len(), 1);
}

#[tokio::test]
async fn adicional_nao_pode_buscar_historico_privado_da_main() {
    let f = Fixture::new();
    for name in ["search_history", "open_history"] {
        let mut body = f.body.clone();
        body["name"] = name.into();
        let answer = route(&f.hub, "memoryCall", &body).await.unwrap().unwrap();
        assert!(answer.to_string().contains("denied"));
    }
}

#[tokio::test]
async fn adicional_herda_compartilhamento_efetivo_e_revogacao_invalida_captura() {
    let project = Fixture::memory(None);
    assert!(
        !crate::chat::memory::shared(&project.cfg),
        "pasta adicional gerenciada não amplia o compartilhamento do projeto"
    );
    let mut body = project.body.clone();
    body["arguments"]["shared"] = true.into();
    // A proposta não pode promover uma pasta de projeto à memória compartilhada.
    assert!(route(&project.hub, "memoryCall", &body).await.is_err());
    assert_eq!(
        project.hub.store.memory_read(|db| codync_mem::store::layer(db, None)).unwrap(),
        [] as [codync_mem::store::Entry; 0],
    );
    let shared = Fixture::with_shared(Some(true));
    let context = shared.context();
    let mut cfg = shared.hub.store.bot(&shared.cfg.id).unwrap().unwrap().config;
    cfg.shared_memory = Some(false);
    shared.hub.store.save_bot(&cfg).unwrap();
    assert!(context.check(&shared.hub).is_err(), "não pode usar a escolha antiga depois da revogação");
}

#[tokio::test]
async fn adicional_codex_omite_e_recusa_mutantes_mesmo_com_transporte_valido() {
    let f = Fixture::memory_for_backend(None, "codex");
    let mut body = f.body.clone();
    body["name"] = "__tools".into();
    let offered = route(&f.hub, "memoryCall", &body).await.unwrap().unwrap();
    for name in ["remember", "revise_memory", "archive_memory", "edit_memory"] {
        assert!(offered["tools"].as_array().unwrap().iter().all(|tool| tool["name"] != name));
        body["name"] = name.into();
        let error = route(&f.hub, "memoryCall", &body).await.unwrap_err();
        assert!(error.to_string().contains("codex_memory_mutation_refused"));
    }
    let count: i64 = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT COUNT(*) FROM donna_memory_auto_proposals", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(
        f.hub.store.memory_read(|db| codync_mem::store::layer(db, Some(&f.cfg.id))).unwrap(),
        [] as [codync_mem::store::Entry; 0],
    );
}

#[tokio::test]
async fn composicao_pr3_revoga_a_captura_original_sem_gravar_proposta_ou_adotar_binding_novo() {
    for mode in ["session", "tool"] {
        let f = Fixture::memory(None);
        let context = f.context();
        let hub = f.hub.clone();
        let cfg = f.cfg.clone();
        let original = context.turn.execution.as_ref().unwrap().clone();
        let reached = crate::fork::sessoes::call_tests::interleave(&f.hub.store, "memory_proposal_commit", move || {
            if mode == "session" {
                crate::fork::sessoes::bindings::revoke(&hub.store, &cfg.id, Some("sessao-A")).unwrap();
                let next = crate::fork::sessoes::bindings::issue(
                    &hub.store,
                    &cfg.id,
                    cfg.session_owner(),
                    cfg.session_owner(),
                    None,
                )
                .unwrap();
                crate::fork::sessoes::bindings::activate_for_config(&hub.store, &next, "sessao-A", &cfg).unwrap();
                assert_ne!(original.acp_binding(), Some(next.id.as_str()));
            } else {
                let raw = hub.store.kv_read(&key(cfg.session_owner())).unwrap().unwrap();
                let mut binding: Binding = serde_json::from_str(&raw).unwrap();
                binding.token = "outra-definicao".into();
                hub.store.kv_set(&key(cfg.session_owner()), &serde_json::to_string(&binding).unwrap()).unwrap();
            }
        });
        let result =
            crate::chat::memory::tool_call_in(&f.hub, &f.cfg.id, "remember", &f.body["arguments"], Some(&context))
                .await;
        assert!(reached.load(std::sync::atomic::Ordering::SeqCst));
        assert!(result.is_err(), "{mode}");
        let count: i64 = f
            .hub
            .store
            .db
            .locked()
            .query_row("SELECT COUNT(*) FROM donna_memory_auto_proposals", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "{mode}: sem proposta na origem substituta");
        assert!(context.check(&f.hub).is_err());
    }
}
