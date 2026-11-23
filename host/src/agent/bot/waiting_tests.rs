//! Fronteiras D72: transcript único, reinício após uma hora e troca explícita com handoff.
use crate::LockExt;
use crate::hub::{BotStatus, Hub};
use crate::store::{BotConfig, EntryKind, Store};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn complete_request_p05_suprime_resposta_tardia_no_canal() {
    use crate::fork::retornos::{fim_do_turno, provas_privacidade_testes::pedido_para_callback_tardio};
    let f = Fixture::new();
    let fence = pedido_para_callback_tardio(&f.hub.store);
    let mut actor = f.actor();
    let (mut request, reply) = crate::chat::team::BotRequest::for_test_recorded("late");
    request.dispatch_recorded(fence);
    actor.active_request = Some(request);
    let text = "não publicar";
    let fim = fim_do_turno(false, false, false, "end_turn", Some(text), None, None);
    actor.complete_request(Ok(text.into()), false, fim);
    assert_eq!(reply.await.unwrap().unwrap_err().to_string(), "retorno_pedido_ja_decidido");
    assert!(actor.active_request.is_none());
    drop(actor);
    f.hub.shutdown().await;
    std::fs::remove_dir_all(&f.dir).unwrap();
}

#[tokio::test]
async fn rodada4_handoff_de_rollover_e_consumido_antes_da_primeira_espera() {
    let mut f = Fixture::new();
    let cfg = f.hub.store.bot("bot").unwrap().unwrap().config;
    f.sentinel_in_extra();
    crate::chat::memory::bind_profile_person(
        &f.hub.store,
        "bot",
        "sessao-anterior",
        Some(crate::chat::memory::profile::OWNER),
    )
    .unwrap();
    crate::chat::handoff::draft(&f.hub, &cfg, "sessao-anterior", crate::chat::handoff::Reason::Full).unwrap();
    f.send().await;
    assert!(f.prompts()[0]["prompt"][0]["text"].as_str().unwrap().contains("handoff"));
    assert!(!f.prompts()[0]["prompt"][0]["text"].as_str().unwrap().contains("SENTINELA"), "rollover lê só a principal");
    let saved: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    assert!(saved["input"]["handoff"].is_null());
    f.hub.shutdown().await;
    f.make_retry_due();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| f.prompts().len() == 2 && f.hub.runtime("bot").status == BotStatus::Idle).await;
    assert_eq!(f.prompts()[0]["sessionId"], f.prompts()[1]["sessionId"]);
    assert!(!f.prompts()[1]["prompt"][0]["text"].as_str().unwrap().contains("handoff"));
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn rodada4_mensagem_posterior_ao_stop_e_entregue_apos_conclusao() {
    let f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    let mut actor = f.actor();
    assert!(actor.restore_waiting());
    let mut saved: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    saved["phase"] = json!("retrying");
    actor.waiting = Some(serde_json::from_value(saved).unwrap());
    let lane = crate::store::Lane::main("bot");
    let before = f
        .hub
        .store
        .insert_entry(&lane, EntryKind::User, 2, &json!({"text":"Antes do Stop", "status":"queued"}))
        .unwrap();
    actor.stop().await;
    let after = f
        .hub
        .store
        .insert_entry(&lane, EntryKind::User, 3, &json!({"text":"Depois do Stop", "status":"queued"}))
        .unwrap();
    let (done, mut rx) = tokio::sync::mpsc::unbounded_channel();
    actor.on_cmd(super::Cmd::Send { lane, entry_id: after.id.clone(), text: "Depois do Stop".into() }, &done).await;
    actor.finish_turn(&Ok(json!({"stopReason":"cancelled"})));
    assert_eq!(f.hub.store.entry(&before.id).unwrap().data["status"], "cancelled");
    assert_eq!(f.hub.store.entry(&after.id).unwrap().data["status"], "queued");
    actor.next_in_queue(&done).await;
    actor.finish_turn(&rx.recv().await.unwrap().result);
    assert_eq!(f.hub.store.entry(&after.id).unwrap().data["status"], "sent");
    assert_eq!(f.prompts().len(), 2);
    assert!(f.prompts()[1]["prompt"][0]["text"].as_str().unwrap().contains("Depois do Stop"));
    if let Some(conn) = actor.conn.take() {
        conn.acp.kill().await;
    }
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn rodada4_crash_apos_permissao_preserva_intervalo_e_pausa_sem_replay() {
    use crate::fork::bot_admission as admission;
    let f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    std::fs::write(f.dir.join("hold"), "").unwrap();
    let (done, _rx) = tokio::sync::mpsc::unbounded_channel();
    f.make_retry_due();
    let mut actor = f.actor();
    assert!(actor.restore_waiting());
    let started = crate::store::now_ms();
    actor.retry_waiting(&done).await;
    f.until(|| f.prompts().len() == 2).await;
    let permitted = admission::current(&f.hub.store, "bot").unwrap().unwrap();
    let saved: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    assert_eq!(saved["phase"], "retrying");
    assert_eq!(saved["attemptCount"], 2);
    assert!(saved["retryAt"].as_i64().unwrap() >= started + 300_000);
    if let Some(conn) = actor.conn.take() {
        conn.acp.kill().await;
    }
    let mut restored = f.actor();
    restored.restore_dispatch().unwrap();
    assert!(restored.restore_waiting());
    restored.retry_waiting(&done).await;
    assert_eq!(f.prompts().len(), 2, "reiniciar não libera nova tentativa incerta");
    let recovered: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    assert_eq!(recovered["retryAt"], saved["retryAt"]);
    assert_eq!(recovered["attemptCount"], 2);
    assert_eq!(recovered["executionToken"], permitted.execution_token);
    assert_eq!(recovered["phase"], "paused");
    assert_eq!(f.hub.runtime("bot").status, BotStatus::Error);
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn rodada4_checkpoint_corrompido_nao_bloqueia_envio_nem_configuracao() {
    let f = Fixture::new();
    for raw in ["{quebrado", r#"{"phase":"waiting","input":null}"#] {
        f.hub.store.kv_set("turn.waiting.bot", raw).unwrap();
        f.hub.update_bot(&json!({"id":"bot", "model":"novo"})).unwrap();
        assert!(f.hub.store.kv_get("turn.waiting.bot").is_none());
    }
    f.hub.store.kv_set("turn.waiting.bot", "{quebrado").unwrap();
    let result = crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId":"bot", "text":"Pedido após checkpoint inválido"}),
    )
    .await
    .unwrap();
    assert_ne!(result["entry"]["data"]["subscriptionQueued"], true);
    f.until(|| f.waiting_checkpoint_published()).await;
    assert_eq!(f.prompts().len(), 1);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn rodada4_tombstone_e_inflight_legado_nao_fabricam_retomada_e_liberam_queued_seguinte() {
    let mut f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    let store = Store::open(&f.dir.join("test.db")).unwrap();
    let mut saved: Value = serde_json::from_str(&store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    saved["phase"] = json!("finished");
    store.kv_set("turn.waiting.bot", &saved.to_string()).unwrap();
    store.kv_set("turn.inflight.bot", &crate::store::now_ms().to_string()).unwrap();
    f.hub = Fixture::open(&f.dir, store);
    f.until(|| {
        f.hub.store.history("bot", i64::MAX, 100).unwrap().iter().any(|entry| {
            entry.kind == "notice"
                && entry.data["text"].as_str().is_some_and(|text| text.contains("sem checkpoint de retomada"))
        })
    })
    .await;
    assert_eq!(f.prompts().len(), 1);
    assert!(f.hub.store.kv_get("turn.waiting.bot").is_none());
    assert!(f.hub.store.kv_get("turn.inflight.bot").is_none());
    assert!(f.hub.runtime("bot").memory_turn_token.is_none());
    crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId":"bot","text":"Pedido novo após legado sem token"}),
    )
    .await
    .unwrap();
    f.until(|| f.prompts().len() == 2 && f.hub.runtime("bot").status == BotStatus::Idle).await;
    assert!(f.prompts()[1].to_string().contains("Pedido novo após legado sem token"));
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn rodada4_api_store_e_ator_compartilham_espera_ativa() {
    let f = Fixture::new();
    f.send().await;
    let base: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    let mut cases = Vec::new();
    for phase in ["waiting", "paused", "retrying", "completing", "finished"] {
        let mut checkpoint = base.clone();
        checkpoint["phase"] = json!(phase);
        f.hub.store.kv_set("turn.waiting.bot", &checkpoint.to_string()).unwrap();
        let result = crate::api::dispatch(
            &f.hub,
            &crate::api::devices::Caller::Local,
            "send",
            json!({"botId":"bot", "text":phase}),
        )
        .await
        .unwrap();
        let active = matches!(phase, "waiting" | "paused" | "retrying");
        assert_eq!(result["entry"]["data"]["subscriptionQueued"] == true, active);
        cases.push((phase, checkpoint, result["entry"]["id"].as_str().unwrap().to_owned(), active));
    }
    f.hub.shutdown().await;
    let (done, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut actor = f.actor();
    actor.turn = Some(1); // Só verifica enfileiramento, sem iniciar outro turno.
    for (phase, checkpoint, id, active) in cases {
        f.hub.store.kv_set("turn.waiting.bot", &checkpoint.to_string()).unwrap();
        let mut entry = f.hub.store.entry(&id).unwrap();
        entry.data["status"] = json!("queued");
        entry.data.as_object_mut().unwrap().remove("subscriptionQueued");
        f.hub.set_entry(&id, &entry.data).unwrap();
        actor.waiting = Some(serde_json::from_value(checkpoint).unwrap());
        actor
            .on_cmd(
                super::Cmd::Send { lane: crate::store::Lane::main("bot"), entry_id: id.clone(), text: phase.into() },
                &done,
            )
            .await;
        assert_eq!(f.hub.store.entry(&id).unwrap().data["subscriptionQueued"] == true, active);
        entry.data.as_object_mut().unwrap().remove("subscriptionQueued");
        f.hub.set_entry(&id, &entry.data).unwrap();
        f.hub.store.expire_pending().unwrap();
        assert_eq!(f.hub.store.entry(&id).unwrap().data["subscriptionQueued"] == true, active);
    }
    let _ = std::fs::remove_dir_all(&f.dir);
}

struct Fixture {
    hub: Arc<Hub>,
    dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        Self::with_backend("codex")
    }

    fn with_backend(backend: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("donnabot-limit-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("test.db")).unwrap();
        let agent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/subscription_agent.py");
        let cfg: BotConfig = serde_json::from_value(json!({
            "id": "bot", "name": "Bot", "backend": backend, "cwd": dir,
            "command": crate::shell::python(&agent), "notify": false, "permission":"ask",
        }))
        .unwrap();
        if backend == "claude" {
            std::fs::write(dir.join("claude"), "").unwrap();
        }
        store.save_bot(&cfg).unwrap();
        let hub = Self::open(&dir, store);
        Self { hub, dir }
    }

    fn open(dir: &std::path::Path, store: Store) -> Arc<Hub> {
        // O serviço cria a raiz antes de iniciar o Hub; este fixture inicia o Hub diretamente.
        std::fs::create_dir_all(crate::service::data_dir()).unwrap();
        let hub = Hub::new(
            store,
            "test".into(),
            crate::remote::identity::Identity::load_or_create(dir).unwrap(),
            "test".into(),
            19422,
        );
        hub.start().unwrap();
        hub
    }

    async fn until(&self, condition: impl Fn() -> bool) {
        tokio::time::timeout(Duration::from_secs(20), async {
            while !condition() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("estado esperado");
    }

    fn waiting_checkpoint_published(&self) -> bool {
        let runtime = self.hub.runtime("bot");
        runtime.status == BotStatus::Waiting
            && runtime.preparing_turn_token.is_none()
            && runtime.memory_turn_token.is_none()
            && self.hub.store.kv_read("turn.waiting.bot").unwrap().is_some_and(|raw| {
                let saved: Value = serde_json::from_str(&raw).unwrap();
                saved["phase"] == "waiting"
                    && saved["notice"]
                        .as_str()
                        .and_then(|id| self.hub.store.entry(id))
                        .is_some_and(|entry| entry.data["subscriptionWait"].is_object())
            })
    }

    fn human_admission_pending(&self) -> bool {
        let runtime = self.hub.runtime("bot");
        // O preparo ainda aponta a mensagem humana. A pendência só a limpa
        // depois de conservar Setup/Prompt, sem ativar ferramentas de memória.
        runtime.status == BotStatus::Waiting
            && runtime.message.is_none()
            && runtime.preparing_turn_token.is_some()
            && runtime.memory_turn_token.is_none()
    }

    async fn send(&self) {
        self.send_until(|| self.waiting_checkpoint_published()).await;
    }

    async fn send_until(&self, condition: impl Fn() -> bool) {
        crate::api::dispatch(
            &self.hub,
            &crate::api::devices::Caller::Local,
            "send",
            json!({"botId": "bot", "text": "Continue a tarefa pendente"}),
        )
        .await
        .unwrap();
        // Waiting também é preparo anterior à reserva/ACP. Cada cenário
        // fornece a prova que precisa; erro real continua falhando imediatamente.
        self.until(|| condition() || self.hub.runtime("bot").status == BotStatus::Error).await;
        assert_eq!(self.hub.bot_value_for_test("bot")["status"], "waiting");
        assert!(condition(), "a espera precisa da evidência pedida pelo cenário");
    }

    fn actor(&self) -> super::Actor {
        use super::*;
        let session_id = self.hub.store.bot("bot").unwrap().unwrap().session_id;
        let lane = Lane::main("bot");
        let fast_support = FastModeSupport::from_stored(None);
        let (dreamer, _) = mpsc::unbounded_channel();
        Actor {
            before_permit: None,
            account_revision: None,
            hub: self.hub.clone(),
            cfg: self.hub.store.bot("bot").unwrap().unwrap().config,
            conn: None,
            session_id,
            run_sessions: HashMap::new(),
            lane,
            turn_session: None,
            turn_binding: None,
            cost_turn: None,
            active_group: None,
            active_routine: None,
            routine_attempt: None,
            routine_deadline: None,
            routine_timed_out: false,
            routine_terminal: false,
            routine_completion: None,
            session_fresh: false,
            profile_resend: HashSet::new(),
            compacting: None,
            context_compaction: None,
            raw_prompt: false,
            handoff: None,
            handoff_chat: None,
            handoff_card: None,
            session_card: None,
            compaction_notice: None,
            dreamer,
            turn_text: None,
            turn_entries: Vec::new(),
            turn_memory_revision: String::new(),
            announce: None,
            turn: None,
            input: None,
            antigravity_grant: None,
            antigravity_active: None,
            waiting: None,
            turn_limit: None,
            fallback: fork_fallback::Turno::default(),
            finishing: None,
            queue: VecDeque::new(),
            queue_retry_at: None,
            queue_restore_retry_at: None,
            subscription_refusals: BTreeMap::new(),
            active_request: None,
            dispatch: None,
            profile_skills: None,
            dispatch_finishing: None,
            pending_admission: None,
            setup_reason: None,
            ceiling_holds: VecDeque::new(),
            ceiling_next_check: None,
            ceiling_probe: None,
            ceiling_unrestored: None,
            restart_admission: false,
            seg: Seg::None,
            tools: HashMap::new(),
            hub_tool_servers: HashMap::new(),
            plan_entry: None,
            last_text: None,
            sent: Vec::new(),
            reacted: Reacted::No,
            files: files::FileShares::default(),
            perms: HashMap::new(),
            stop_requested: false,
            cancel_queued: Vec::new(),
            send_now: Vec::new(),
            cancel_deadline: None,
            exit_tail: None,
            tools_changed: false,
            restart_after_turn: false,
            fast_support,
            idle_checked: Instant::now(),
            mudo: fork_motor_mudo::Vigia::new(),
        }
    }

    fn make_retry_due(&self) {
        let store = Store::open(&self.dir.join("test.db")).unwrap();
        let mut saved: Value = serde_json::from_str(&store.kv_get("turn.waiting.bot").unwrap()).unwrap();
        saved["retryAt"] = json!(0);
        store.kv_set("turn.waiting.bot", &saved.to_string()).unwrap();
    }

    fn make_routine_retry_due_after_start(&self) {
        let store = Store::open(&self.dir.join("test.db")).unwrap();
        let mut saved: Value = serde_json::from_str(&store.kv_get("turn.waiting.bot").unwrap()).unwrap();
        saved["retryAt"] = json!(crate::store::now_ms() + 2_000);
        store.kv_set("turn.waiting.bot", &saved.to_string()).unwrap();
    }

    /// `StoreBusy` é o `try_lock` recusando um banco segurado pelo ator naquele
    /// instante: os produtores reais repetem, então o teste repete também.
    async fn enqueue(&self, id: &str, source: &str, test: bool) -> Value {
        crate::fork::atividades::store::retry_store_busy(|| {
            self.hub.routines.enqueue(&self.hub.store, "bot", id, json!({"source": source}), None, test)
        })
        .await
        .unwrap()
    }

    /// Uma volta transitória do ciclo da espera (ex.: `StoreBusy` na preparação)
    /// devolve a espera à fase `waiting` com o `retryAt` rearmado no backoff do
    /// D72 (~5 min). Em espera devida o `retryAt` já passou; longe assim só sai
    /// por reagendamento.
    fn routine_wait_rearmed(&self) -> bool {
        self.hub.store.kv_get("turn.waiting.bot").and_then(|raw| serde_json::from_str::<Value>(&raw).ok()).is_some_and(
            |w| w["phase"] == "waiting" && w["retryAt"].as_i64().unwrap_or(0) > crate::store::now_ms() + 30_000,
        )
    }

    /// O Hub em execução só lê o `retryAt` na restauração: reabrir com o horário
    /// próximo repete a volta na hora, como o `make_routine_retry_due_after_start`.
    async fn reopen_retrying_routine_wait(&mut self) {
        self.hub.shutdown().await;
        self.make_routine_retry_due_after_start();
        self.hub = Self::open(&self.dir, Store::open(&self.dir.join("test.db")).unwrap());
    }

    fn prompts(&self) -> Vec<Value> {
        std::fs::read_to_string(self.dir.join("prompts.jsonl"))
            .unwrap_or_default()
            // A fixture anexa em paralelo; a linha só é publicada pelo delimitador.
            .split_inclusive('\n')
            .take_while(|line| line.ends_with('\n'))
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    /// Uma linha de uma sessão extra que nenhum resumo da principal pode levar.
    fn sentinel_in_extra(&self) {
        let extra = crate::store::Lane { chat: "bot".into(), session: Some("extra-1".into()) };
        let plan = json!({"entries":[{"content":"SENTINELA da extra","status":"pending"}]});
        self.hub.store.insert_entry(&extra, EntryKind::Plan, 1, &plan).unwrap();
    }

    fn assert_finished_once(&self) {
        let entries = self.hub.store.history("bot", i64::MAX, 100).unwrap();
        assert_eq!(entries.iter().filter(|e| e.kind == "user").count(), 1);
        assert_eq!(entries.iter().filter(|e| e.kind == "agent" && e.data["final"] == true).count(), 1);
        assert!(!entries.iter().any(|e| e.kind == "notice" && e.data["style"] == "error"));
    }
}

#[tokio::test]
async fn prompts_esperam_delimitador_e_nao_escondem_registro_corrompido() {
    use std::io::Write;

    let f = Fixture::new();
    let path = f.dir.join("prompts.jsonl");
    std::fs::write(&path, "{\"sessionId\":\"one\"}\n{\"sessionId\":\"two").unwrap();
    assert_eq!(f.prompts(), vec![json!({"sessionId": "one"})]);
    let mut log = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
    log.write_all(b"\"}\n").unwrap();
    drop(log);
    assert_eq!(f.prompts(), vec![json!({"sessionId": "one"}), json!({"sessionId": "two"})]);
    std::fs::write(&path, "{invalid}\n").unwrap();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f.prompts())).is_err());
    f.hub.shutdown().await;
    std::fs::remove_dir_all(&f.dir).unwrap();
}

#[tokio::test]
async fn espera_sobrevive_reinicio_e_retoma_sem_duplicar_transcript() {
    let mut f = Fixture::new();
    f.send().await;
    assert!(f.hub.bot_value_for_test("bot")["resumeAt"].as_i64().is_some());
    f.hub.shutdown().await;
    let store = Store::open(&f.dir.join("test.db")).unwrap();
    let raw = store.kv_get("turn.waiting.bot").expect("espera persistida");
    let mut pending: Value = serde_json::from_str(&raw).unwrap();
    pending["retryAt"] = json!(0);
    pending["resumeAt"] = json!(0);
    store.kv_set("turn.waiting.bot", &pending.to_string()).unwrap();
    // A retomada D72 independe da janela de uma hora do resume de crashes comum.
    store.kv_set("turn.inflight.bot", "1").unwrap();
    f.hub = Fixture::open(&f.dir, store);
    f.until(|| f.prompts().len() == 2 && f.hub.runtime("bot").status == BotStatus::Idle).await;
    assert_eq!(f.prompts()[1]["sessionId"], "session-1");
    assert_eq!(std::fs::read_to_string(f.dir.join("session-count")).unwrap(), "1");
    f.assert_finished_once();
    assert!(f.hub.store.kv_get("turn.waiting.bot").is_none());
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// D372 (a): editar o padrão do bot com um turno esperando o limite NÃO troca o modelo, a conta nem a sessão desse
/// turno; ele retoma na seleção admitida originalmente, também depois de reiniciar. O padrão novo vale para conversas
/// novas; a conversa só muda por `sessionEngineSet`, no próximo turno. Antes desta decisão, a edição trocava o turno
/// em espera (handoff numa sessão nova).
#[tokio::test]
async fn edicao_do_padrao_durante_espera_conserva_a_selecao_original_do_turno() {
    let mut f = Fixture::new();
    f.send().await;
    let before: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    assert!(f.hub.update_bot(&json!({"id": "bot", "backend": "claude", "model": "outro"})).is_err());
    assert_eq!(f.prompts().len(), 1, "a ferramenta de outro bot não libera o pedido");
    f.hub.update_bot_by_user(&json!({"id": "bot", "backend": "claude", "model": "outro"})).unwrap();
    tokio::time::sleep(Duration::from_millis(650)).await;
    assert_eq!(f.prompts().len(), 1, "sem retomada antecipada");
    let saved: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    for field in ["executionToken", "backend", "model"] {
        assert_eq!(saved[field], before[field], "{field} do turno original");
    }
    assert_eq!(saved["input"]["session"], before["input"]["session"]);
    assert!(saved["input"]["handoff"].is_null(), "nenhum handoff de troca");
    // Retomada após reiniciar: a seleção e a sessão originais, sem prompt duplicado.
    f.hub.shutdown().await;
    f.make_retry_due();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| f.prompts().len() == 2 && f.hub.runtime("bot").status == BotStatus::Idle).await;
    assert_eq!(f.prompts()[1]["sessionId"], "session-1");
    assert_eq!(std::fs::read_to_string(f.dir.join("session-count")).unwrap(), "1");
    f.assert_finished_once();
    // O padrão do bot mudou; a conversa principal continua na própria seleção (próximo turno inclusive).
    assert_eq!(f.hub.store.bot("bot").unwrap().unwrap().config.backend, "claude");
    let engine = crate::fork::sessoes::engine_choice::read(&f.hub.store.db.locked(), "bot").unwrap().unwrap();
    assert_eq!((engine.backend.as_str(), engine.model.as_deref()), ("codex", None));
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn parar_cancela_a_espera_persistida_do_claude_sem_reset() {
    let f = Fixture::with_backend("claude");
    f.send().await;
    assert!(f.hub.bot_value_for_test("bot")["resumeAt"].is_null());
    f.hub.send_cmd("bot", super::Cmd::Stop).unwrap();
    f.until(|| f.hub.runtime("bot").status == BotStatus::Idle).await;
    assert!(f.hub.store.kv_get("turn.waiting.bot").is_none());
    assert_eq!(f.prompts().len(), 1);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Retomada por reinício que cai durante a tentativa: o reinício seguinte não reenvia (permit incerto) e o Tentar de
/// novo continua na MESMA sessão. D372: o gatilho era a edição do modelo (troca com handoff), que não existe mais.
#[tokio::test]
async fn reinicio_durante_retry_preserva_a_sessao_da_tentativa() {
    let mut f = Fixture::new();
    f.send().await;
    std::fs::write(f.dir.join("hold"), "").unwrap();
    f.hub.shutdown().await;
    f.make_retry_due();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| f.prompts().len() == 2).await;
    assert_eq!(f.prompts()[1]["sessionId"], "session-1");
    let before: Value = serde_json::from_str(&f.hub.store.kv_read("turn.waiting.bot").unwrap().unwrap()).unwrap();
    assert_eq!(before["phase"], "retrying");
    f.hub.shutdown().await;
    std::fs::remove_file(f.dir.join("hold")).unwrap();
    f.make_retry_due();
    let store = Store::open(&f.dir.join("test.db")).unwrap();
    f.hub = Fixture::open(&f.dir, store);
    f.until(|| f.hub.runtime("bot").status == BotStatus::Error).await;
    assert_eq!(f.prompts().len(), 2, "permit incerta não autoriza novo prompt no reinício");
    let paused: Value = serde_json::from_str(&f.hub.store.kv_read("turn.waiting.bot").unwrap().unwrap()).unwrap();
    assert_eq!(paused["phase"], "paused");
    assert_eq!(paused["executionToken"], before["executionToken"]);
    assert_eq!(paused["input"]["session"], "session-1");
    f.hub
        .send_cmd("bot", super::Cmd::RetrySubscription { entry_id: paused["notice"].as_str().unwrap().into() })
        .unwrap();
    f.until(|| f.prompts().len() == 3 && f.hub.runtime("bot").status == BotStatus::Idle).await;
    assert_eq!(f.prompts()[2]["sessionId"], "session-1");
    assert_eq!(std::fs::read_to_string(f.dir.join("session-count")).unwrap(), "1");
    f.assert_finished_once();
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn cancelamento_com_falha_de_exclusao_nao_reexecuta_no_reinicio() {
    let mut f = Fixture::new();
    f.send().await;
    let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
    db.execute_batch("CREATE TRIGGER keep_wait BEFORE DELETE ON kv WHEN OLD.k = 'turn.waiting.bot' BEGIN SELECT RAISE(FAIL, 'temporariamente indisponível'); END;").unwrap();
    f.hub.send_cmd("bot", super::Cmd::Stop).unwrap();
    f.until(|| f.hub.runtime("bot").status == BotStatus::Idle).await;
    let saved: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    assert_eq!(saved["phase"], "finished");
    f.hub.shutdown().await;
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(f.prompts().len(), 1);
    assert_eq!(f.hub.runtime("bot").status, BotStatus::Idle);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn falha_inicial_de_persistencia_e_retentada_antes_do_reset() {
    let f = Fixture::new();
    let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_wait BEFORE INSERT ON kv WHEN NEW.k = 'turn.waiting.bot' BEGIN SELECT RAISE(FAIL, 'temporariamente indisponível'); END;").unwrap();
    f.send_until(|| {
        let runtime = f.hub.runtime("bot");
        runtime.preparing_turn_token.is_none()
            && runtime.memory_turn_token.is_none()
            && f.hub
                .store
                .history("bot", i64::MAX, 100)
                .unwrap()
                .iter()
                .any(|entry| entry.kind == "notice" && entry.data["subscriptionWait"].is_object())
    })
    .await;
    assert!(f.hub.store.kv_get("turn.waiting.bot").is_none());
    assert_ne!(f.hub.store.kv_get("turn.inflight.bot").unwrap_or_default(), "");
    db.execute_batch("DROP TRIGGER reject_wait").unwrap();
    f.until(|| f.hub.store.kv_get("turn.waiting.bot").is_some()).await;
    assert_eq!(f.hub.runtime("bot").status, BotStatus::Waiting);
    assert_eq!(f.prompts().len(), 1);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn cache_antigo_nao_antecipa_reset_informado_no_erro() {
    let f = Fixture::new();
    f.hub.set_usage(crate::usage::Usage {
        providers: vec![crate::usage::Provider {
            id: "codex",
            name: "Codex",
            source: "sessions",
            updated_at: 1,
            windows: vec![crate::usage::Window {
                id: "primary".into(),
                label: "5-hour".into(),
                percent: 100.0,
                resets_at: Some(crate::store::now_ms() + 1_800_000),
                resets_text: None,
                minutes: None,
            }],
        }],
    });
    f.send().await;
    let reset = f.hub.runtime("bot").resume_at;
    // O polling relê o mesmo arquivo e carimba updated_at com a hora atual.
    let mut polled = f.hub.usage.lock().unwrap().clone();
    polled.providers[0].updated_at = crate::store::now_ms() + 1;
    f.hub.set_usage(polled);
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(f.hub.runtime("bot").resume_at, reset);
    assert!(reset.unwrap() > crate::store::now_ms() + 3_500_000);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn turno_oculto_sem_sessao_recarregavel_encerra_a_espera() {
    let mut f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    let store = Store::open(&f.dir.join("test.db")).unwrap();
    let mut pending: Value = serde_json::from_str(&store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    pending["input"]["hidden"] = json!(true);
    pending["retryAt"] = json!(0);
    pending["resumeAt"] = json!(0);
    store.kv_set("turn.waiting.bot", &pending.to_string()).unwrap();
    std::fs::write(f.dir.join("fail-load"), "").unwrap();
    f.hub = Fixture::open(&f.dir, store);
    f.until(|| f.hub.runtime("bot").status == BotStatus::Error).await;
    assert!(f.hub.store.kv_get("turn.waiting.bot").is_none());
    assert_eq!(f.prompts().len(), 1, "não repete silenciosamente um turno oculto numa sessão vazia");
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn limite_ao_recarregar_sessao_nao_a_substitui_por_sessao_vazia() {
    let mut f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    let store = Store::open(&f.dir.join("test.db")).unwrap();
    let mut pending: Value = serde_json::from_str(&store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    pending["retryAt"] = json!(0);
    store.kv_set("turn.waiting.bot", &pending.to_string()).unwrap();
    std::fs::write(f.dir.join("limit-load"), "").unwrap();
    f.hub = Fixture::open(&f.dir, store);
    f.until(|| {
        f.hub.store.kv_get("turn.waiting.bot").is_some_and(|raw| {
            let saved: Value = serde_json::from_str(&raw).unwrap();
            saved["retryAt"].as_i64().unwrap_or(0) > crate::store::now_ms()
                && saved["phase"] == "waiting"
                && f.hub.runtime("bot").status == BotStatus::Waiting
        })
    })
    .await;
    assert_eq!(f.hub.runtime("bot").status, BotStatus::Waiting);
    assert_eq!(std::fs::read_to_string(f.dir.join("session-count")).unwrap(), "1");
    assert_eq!(f.prompts().len(), 1);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Falha ao gravar o checkpoint da tentativa: nenhum prompt antes de salvar, a espera continua e a próxima retomada
/// segue na mesma sessão. D372: o gatilho da tentativa é a retomada após reiniciar (antes, a edição do modelo).
#[tokio::test]
async fn falha_no_checkpoint_preserva_pedido_e_retenta_na_mesma_sessao() {
    let mut f = Fixture::new();
    f.send().await;
    let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
    db.execute_batch("CREATE TABLE checkpoint_attempts (attempt INTEGER); CREATE TRIGGER reject_checkpoint BEFORE INSERT ON kv WHEN NEW.k = 'turn.waiting.bot' AND json_extract(NEW.v, '$.phase') = 'retrying' BEGIN INSERT INTO checkpoint_attempts VALUES (1); SELECT RAISE(FAIL, 'checkpoint indisponível'); END;").unwrap();
    f.hub.shutdown().await;
    f.make_retry_due();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    // Evidência positiva: a tentativa chegou a gravar o checkpoint `retrying` e o gatilho o recusou (o contador fica,
    // porque RAISE(FAIL) não desfaz o que o comando já fez).
    let attempts = || -> i64 { db.query_row("SELECT COUNT(*) FROM checkpoint_attempts", [], |r| r.get(0)).unwrap() };
    f.until(|| attempts() >= 1 && f.hub.runtime("bot").status == BotStatus::Waiting).await;
    assert_eq!(f.prompts().len(), 1, "nenhum prompt antes de salvar o checkpoint");
    let saved: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    assert_eq!(saved["phase"], "waiting");
    f.hub.shutdown().await;
    db.execute_batch("DROP TRIGGER reject_checkpoint").unwrap();
    f.make_retry_due();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| f.prompts().len() == 2 && f.hub.runtime("bot").status == BotStatus::Idle).await;
    assert_eq!(f.prompts()[1]["sessionId"], "session-1");
    f.assert_finished_once();
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn rejeicao_de_uso_nao_reenvia_turno_concluido() {
    let f = Fixture::with_backend("claude");
    std::fs::write(f.dir.join("successful-rejection"), "").unwrap();
    crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId":"bot", "text":"Pedido concluído via fallback da assinatura"}),
    )
    .await
    .unwrap();
    f.until(|| f.prompts().len() == 1 && f.hub.runtime("bot").status != BotStatus::Working).await;
    assert_eq!(f.hub.runtime("bot").status, BotStatus::Idle);
    assert!(f.hub.store.kv_get("turn.waiting.bot").is_none());
    f.assert_finished_once();
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn stderr_de_ferramenta_nao_estaciona_o_turno() {
    let f = Fixture::new();
    std::fs::write(f.dir.join("tool-stderr"), "").unwrap();
    crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId":"bot", "text":"Execute a ferramenta"}),
    )
    .await
    .unwrap();
    f.until(|| f.hub.runtime("bot").status == BotStatus::Error || f.waiting_checkpoint_published()).await;
    assert_eq!(f.hub.runtime("bot").status, BotStatus::Error);
    assert!(f.hub.store.kv_get("turn.waiting.bot").is_none());
    assert_eq!(f.prompts().len(), 1);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn mensagens_na_fila_durante_espera_sobrevivem_reinicio() {
    let mut f = Fixture::new();
    f.send().await;
    let mut ids = Vec::new();
    for text in ["Mensagem com citação", "Outra mensagem pendente"] {
        let result = crate::api::dispatch(
            &f.hub,
            &crate::api::devices::Caller::Local,
            "send",
            json!({"botId":"bot", "text":text}),
        )
        .await
        .unwrap();
        ids.push(result["entry"]["id"].as_str().unwrap().to_owned());
    }
    f.hub.shutdown().await;
    let store = Store::open(&f.dir.join("test.db")).unwrap();
    let mut pending: Value = serde_json::from_str(&store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    pending["retryAt"] = json!(0);
    pending["resumeAt"] = json!(0);
    store.kv_set("turn.waiting.bot", &pending.to_string()).unwrap();
    f.hub = Fixture::open(&f.dir, store);
    f.until(|| f.prompts().len() >= 2 && f.hub.runtime("bot").status == BotStatus::Idle).await;
    assert_eq!(f.prompts().len(), 3, "turno guardado e um lote posterior, sem replay");
    let prompts = f.prompts();
    let text = prompts[2]["prompt"][0]["text"].as_str().unwrap();
    assert!(text.contains("Mensagem com citação") && text.contains("Outra mensagem pendente"));
    for id in ids {
        assert_eq!(f.hub.store.entry(&id).unwrap().data["status"], "sent");
    }
    assert_eq!(f.hub.store.history("bot", i64::MAX, 100).unwrap().iter().filter(|e| e.kind == "user").count(), 3);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn allowed_warning_limpa_rejeicao_antes_de_erro_comum() {
    let f = Fixture::with_backend("claude");
    std::fs::write(f.dir.join("allowed-warning"), "").unwrap();
    crate::api::dispatch(&f.hub, &crate::api::devices::Caller::Local, "send", json!({"botId":"bot", "text":"Execute"}))
        .await
        .unwrap();
    f.until(|| f.hub.runtime("bot").status == BotStatus::Error || f.waiting_checkpoint_published()).await;
    assert_eq!(f.hub.runtime("bot").status, BotStatus::Error);
    assert!(f.hub.store.kv_get("turn.waiting.bot").is_none());
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn bloqueios_sem_reset_esgotam_tentativas_e_exigem_acao_duravel() {
    let mut f = Fixture::with_backend("claude");
    f.send().await;
    f.hub.shutdown().await;
    let store = Store::open(&f.dir.join("test.db")).unwrap();
    let mut pending: Value = serde_json::from_str(&store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    pending["attemptCount"] = json!(5);
    pending["retryAt"] = json!(0);
    store.kv_set("turn.waiting.bot", &pending.to_string()).unwrap();
    std::fs::write(f.dir.join("always-limit"), "").unwrap();
    f.hub = Fixture::open(&f.dir, store);
    f.until(|| f.hub.runtime("bot").status == BotStatus::Error).await;
    let saved: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    assert_eq!(saved["phase"], "paused");
    assert_eq!(saved["attemptCount"], 6);
    let notice = saved["notice"].as_str().unwrap().to_owned();
    assert_eq!(f.hub.store.entry(&notice).unwrap().data["subscriptionWait"]["retry"], true);
    f.hub.shutdown().await;
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| f.hub.runtime("bot").status == BotStatus::Error).await;
    tokio::time::sleep(Duration::from_millis(650)).await;
    assert_eq!(f.prompts().len(), 2, "nem tick nem reinício liberam novas tentativas");
    std::fs::remove_file(f.dir.join("always-limit")).unwrap();
    crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "retrySubscription",
        json!({"botId":"bot", "entryId":notice}),
    )
    .await
    .unwrap();
    f.until(|| f.prompts().len() == 3 && f.hub.runtime("bot").status == BotStatus::Idle).await;
    f.assert_finished_once();
    assert_eq!(f.hub.store.entry(&notice).unwrap().data["subscriptionWait"]["retry"], false);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Troca de COMANDO (Comando ACP do bot, configuração de execução fora da seleção D372) durante a espera: a falha ao
/// gravar o checkpoint da troca é publicada uma vez, sem laço de tick, e a troca não fica salva pela metade. A
/// seleção da conversa (D372) não muda.
#[tokio::test]
async fn falha_na_troca_de_comando_e_publicada_uma_vez_sem_laco_de_tick() {
    let f = Fixture::new();
    f.send().await;
    let original = f.hub.store.bot("bot").unwrap().unwrap().config.command.unwrap();
    let command = format!("{original} --variante");
    let engine = crate::fork::sessoes::engine_choice::read(&f.hub.store.db.locked(), "bot").unwrap();
    let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
    let quoted = command.replace('\'', "''");
    db.execute_batch(&format!("CREATE TABLE handoff_attempts (attempt INTEGER); CREATE TRIGGER reject_handoff BEFORE INSERT ON kv WHEN NEW.k = 'turn.waiting.bot' AND json_extract(NEW.v, '$.command') = '{quoted}' BEGIN INSERT INTO handoff_attempts VALUES (1); SELECT RAISE(FAIL, 'handoff indisponível'); END;")).unwrap();
    f.hub.update_bot_by_user(&json!({"id":"bot", "command":command})).unwrap();
    f.until(|| f.hub.runtime("bot").status == BotStatus::Error).await;
    tokio::time::sleep(Duration::from_millis(950)).await;
    let attempts: i64 = db.query_row("SELECT COUNT(*) FROM handoff_attempts", [], |r| r.get(0)).unwrap();
    assert_eq!(attempts, 1);
    assert_eq!(f.prompts().len(), 1);
    let saved: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    assert_eq!(saved["command"], json!(original), "a troca não pode ficar salva pela metade");
    db.execute_batch("DROP TRIGGER reject_handoff").unwrap();
    f.hub
        .send_cmd("bot", super::Cmd::RetrySubscription { entry_id: saved["notice"].as_str().unwrap().into() })
        .unwrap();
    f.until(|| f.prompts().len() == 2 && f.hub.runtime("bot").status == BotStatus::Idle).await;
    f.assert_finished_once();
    assert_eq!(
        crate::fork::sessoes::engine_choice::read(&f.hub.store.db.locked(), "bot").unwrap(),
        engine,
        "a seleção da conversa não muda com o comando"
    );
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn estacionar_fecha_ferramentas_e_expira_permissoes() {
    let f = Fixture::new();
    std::fs::write(f.dir.join("pending-cards"), "").unwrap();
    f.send().await;
    let entries = f.hub.store.history("bot", i64::MAX, 100).unwrap();
    assert!(entries.iter().any(|e| e.kind == "tool" && e.data["status"] == "failed"));
    assert!(entries.iter().any(|e| e.kind == "permission" && e.data["status"] == "expired"));
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn tombstone_nao_bloqueia_troca_de_modelo_por_ferramenta() {
    let f = Fixture::new();
    f.hub.store.kv_set("turn.waiting.bot", r#"{"phase":"finished"}"#).unwrap();
    f.hub.store.kv_set("turn.inflight.bot", "turno posterior").unwrap();
    f.hub.set_runtime("bot", |r| r.status = BotStatus::Working);
    f.hub.update_bot(&json!({"id":"bot", "model":"novo"})).unwrap();
    assert!(f.hub.store.kv_get("turn.waiting.bot").is_none());
    assert_eq!(f.hub.store.kv_get("turn.inflight.bot").as_deref(), Some("turno posterior"));
    f.hub.set_runtime("bot", |r| r.status = BotStatus::Idle);
    f.hub.store.kv_set("turn.waiting.bot", r#"{"phase":"finished"}"#).unwrap();
    f.hub.update_bot(&json!({"id":"bot", "model":"outro"})).unwrap();
    assert!(f.hub.store.kv_get("turn.inflight.bot").is_none(), "um tombstone ocioso limpa também a marca antiga");
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn mensagem_recebida_durante_restore_nao_entra_duas_vezes_na_fila() {
    let mut f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    let store = Store::open(&f.dir.join("test.db")).unwrap();
    let mut pending: Value = serde_json::from_str(&store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    pending["retryAt"] = json!(0);
    store.kv_set("turn.waiting.bot", &pending.to_string()).unwrap();
    std::fs::write(f.dir.join("slow-load"), "").unwrap();
    f.hub = Fixture::open(&f.dir, store);
    f.until(|| f.dir.join("loading").exists()).await;
    let result = crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId":"bot", "text":"Mensagem exclusiva durante restore"}),
    )
    .await
    .unwrap();
    let id = result["entry"]["id"].as_str().unwrap();
    f.until(|| f.prompts().len() >= 3 && f.hub.runtime("bot").status == BotStatus::Idle).await;
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(f.prompts().len(), 3);
    assert_eq!(f.hub.store.entry(id).unwrap().data["status"], "sent");
    let prompts = f.prompts();
    let text = prompts[2]["prompt"][0]["text"].as_str().unwrap();
    assert_eq!(text.matches("Mensagem exclusiva durante restore").count(), 1);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// D372: a tentativa vem da retomada após reiniciar (antes, da edição do modelo, que não troca mais o turno).
#[tokio::test]
async fn conclusao_com_flush_final_indisponivel_e_publicada_sem_reexecutar_apos_crash() {
    let mut f = Fixture::new();
    f.send().await;
    let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_final BEFORE UPDATE ON entries WHEN NEW.kind = 'agent' AND json_extract(NEW.data, '$.final') = 1 BEGIN SELECT RAISE(FAIL, 'final indisponível'); END;").unwrap();
    f.hub.shutdown().await;
    f.make_retry_due();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| {
        f.hub
            .store
            .kv_get("turn.waiting.bot")
            .is_some_and(|raw| serde_json::from_str::<Value>(&raw).unwrap()["phase"] == "completing")
    })
    .await;
    f.hub.shutdown().await;
    db.execute_batch("DROP TRIGGER reject_final").unwrap();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| f.hub.store.kv_get("turn.waiting.bot").is_none()).await;
    assert_eq!(f.prompts().len(), 2, "publicar o resultado salvo nunca chama session/prompt");
    f.assert_finished_once();
    assert!(
        f.hub
            .store
            .history("bot", i64::MAX, 100)
            .unwrap()
            .iter()
            .any(|e| e.kind == "agent" && e.data["final"] == true && e.data["text"] == "Concluído")
    );
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// D372: a espera termina pela retomada após reiniciar (antes, pela edição do modelo, que não troca mais o turno).
/// R6 (D372): checkpoint anterior à D372 em `Completing` (resultado salvo, publicação final falhou com o modelo M0),
/// padrão do bot editado para M1 antes da atualização, banco ainda sem a seleção da conversa nem o marcador da
/// migração. No boot a migração fotografa M1; a conclusão durável só publica o resultado salvo, sem exigir que M1
/// execute o turno: publica uma vez, sem prompt novo, sem pausar e sem perder a fila nem o aviso.
#[tokio::test]
async fn conclusao_duravel_anterior_a_d372_publica_apos_migracao_sem_reexecutar() {
    let mut f = Fixture::new();
    f.send().await;
    let queued = crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId":"bot", "text":"Pedido seguinte na fila"}),
    )
    .await
    .unwrap();
    let queued = queued["entry"]["id"].as_str().unwrap().to_owned();
    let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_final BEFORE UPDATE ON entries WHEN NEW.kind = 'agent' AND json_extract(NEW.data, '$.final') = 1 BEGIN SELECT RAISE(FAIL, 'final indisponível'); END;").unwrap();
    f.hub.shutdown().await;
    f.make_retry_due();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| {
        f.hub
            .store
            .kv_get("turn.waiting.bot")
            .is_some_and(|raw| serde_json::from_str::<Value>(&raw).unwrap()["phase"] == "completing")
    })
    .await;
    f.hub.shutdown().await;
    let prompts = f.prompts().len();
    let notice: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    // Estado anterior à D372: padrão editado (M1) e nenhuma seleção da conversa nem marcador de migração.
    db.execute_batch(
        "DROP TRIGGER reject_final;
         UPDATE bots SET config=json_set(config,'$.model','modelo-m1') WHERE id='bot';
         UPDATE donna_bot_sessions SET engine_choice=NULL, engine_next=NULL;
         DELETE FROM kv WHERE k='sessoes.engine.v2';",
    )
    .unwrap();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| f.hub.store.kv_get("turn.waiting.bot").is_none()).await;
    let engine = crate::fork::sessoes::engine_choice::read(&f.hub.store.db.locked(), "bot").unwrap().unwrap();
    assert_eq!(engine.model.as_deref(), Some("modelo-m1"), "a migração fotografou o padrão vigente");
    let entries = f.hub.store.history("bot", i64::MAX, 100).unwrap();
    let done = |e: &&crate::store::Entry| e.kind == "agent" && e.data["final"] == true && e.data["text"] == "Concluído";
    assert_eq!(entries.iter().filter(done).count(), 1, "publica uma vez");
    assert!(!entries.iter().any(|e| e.kind == "notice" && e.data["style"] == "error"), "sem pausa");
    assert!(notice["notice"].as_str().is_some_and(|id| f.hub.store.entry(id).is_some()), "aviso conservado");
    assert!(f.hub.store.entry(&queued).is_some(), "a fila continua");
    f.until(|| f.prompts().len() > prompts || f.hub.runtime("bot").status == BotStatus::Idle).await;
    let queued_only = |p: &Value| p["prompt"][0]["text"].as_str().unwrap_or("").contains("Pedido seguinte");
    assert!(f.prompts()[prompts..].iter().all(queued_only), "nenhum prompt reexecuta o turno concluído");
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn comando_atrasado_nao_reenvia_entrada_ja_concluida() {
    let mut f = Fixture::new();
    f.send().await;
    let result = crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId":"bot", "text":"Segundo pedido exclusivo"}),
    )
    .await
    .unwrap();
    let id = result["entry"]["id"].as_str().unwrap().to_owned();
    f.hub.shutdown().await;
    f.make_retry_due();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| f.prompts().len() == 3 && f.hub.runtime("bot").status == BotStatus::Idle).await;
    f.hub
        .send_cmd(
            "bot",
            super::Cmd::Send {
                lane: crate::store::Lane::main("bot"),
                entry_id: id,
                text: "Segundo pedido exclusivo".into(),
            },
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(650)).await;
    assert_eq!(f.prompts().len(), 3, "estado sent rejeita até um comando entregue depois de terminar");
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn handoff_reconstruido_fica_no_checkpoint_da_nova_sessao() {
    let mut f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    let store = Store::open(&f.dir.join("test.db")).unwrap();
    let mut pending: Value = serde_json::from_str(&store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    pending["retryAt"] = json!(0);
    store.kv_set("turn.waiting.bot", &pending.to_string()).unwrap();
    std::fs::write(f.dir.join("fail-load"), "").unwrap();
    std::fs::write(f.dir.join("hold"), "").unwrap();
    // Antes de abrir: a retomada não pode correr à frente da linha da extra.
    let extra = crate::store::Lane { chat: "bot".into(), session: Some("extra-1".into()) };
    let plan = json!({"entries":[{"content":"SENTINELA da extra","status":"pending"}]});
    store.insert_entry(&extra, EntryKind::Plan, 1, &plan).unwrap();
    f.hub = Fixture::open(&f.dir, store);
    f.until(|| f.prompts().len() == 2).await;
    let saved: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    assert_eq!(saved["input"]["session"], "session-2");
    assert!(saved["input"]["handoff"].is_null(), "handoff consumido pelo prompt da nova sessão");
    assert!(f.prompts()[1]["prompt"][0]["text"].as_str().unwrap().contains("handoff"));
    assert!(!f.prompts()[1]["prompt"][0]["text"].as_str().unwrap().contains("SENTINELA"), "retomada sem load");
    f.hub.shutdown().await;
    std::fs::remove_file(f.dir.join("fail-load")).unwrap();
    std::fs::remove_file(f.dir.join("hold")).unwrap();
    f.make_retry_due();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| f.hub.runtime("bot").status == BotStatus::Error).await;
    assert_eq!(f.prompts().len(), 2, "crash após permit não autoriza replay automático");
    let paused: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    f.hub
        .send_cmd("bot", super::Cmd::RetrySubscription { entry_id: paused["notice"].as_str().unwrap().into() })
        .unwrap();
    f.until(|| f.prompts().len() == 3 && f.hub.runtime("bot").status == BotStatus::Idle).await;
    let prompts = f.prompts();
    assert_eq!(prompts[2]["sessionId"], "session-2");
    assert!(!prompts[2]["prompt"][0]["text"].as_str().unwrap().contains("handoff"));
    f.assert_finished_once();
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// D372: a espera termina pela retomada após reiniciar (antes, pela edição do modelo, que não troca mais o turno).
#[tokio::test]
async fn falha_ao_transferir_fila_nao_envia_prompt_nem_cria_espera_de_assinatura() {
    let mut f = Fixture::new();
    f.send().await;
    let result = crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId":"bot", "text":"Pedido durável da fila"}),
    )
    .await
    .unwrap();
    let id = result["entry"]["id"].as_str().unwrap().to_owned();
    let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
    db.execute_batch(&format!("CREATE TRIGGER reject_sent BEFORE UPDATE ON entries WHEN NEW.id = '{id}' AND json_extract(NEW.data, '$.status') = 'sent' BEGIN SELECT RAISE(FAIL, 'sent indisponível'); END;")).unwrap();
    f.hub.shutdown().await;
    f.make_retry_due();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| {
        f.human_admission_pending()
            && f.hub.store.kv_read("turn.waiting.bot").unwrap().is_none()
            && crate::fork::bot_admission::current(&f.hub.store, "bot")
                .unwrap()
                .is_some_and(|record| record.phase == crate::fork::bot_admission::Phase::Reserved)
    })
    .await;
    assert_eq!(f.prompts().len(), 2, "a entrada não pode executar antes de ser transferida");
    assert_eq!(f.hub.store.entry(&id).unwrap().data["status"], "queued");
    f.hub.shutdown().await;
    db.execute_batch("DROP TRIGGER reject_sent").unwrap();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| f.prompts().len() == 3 && f.hub.runtime("bot").status == BotStatus::Idle).await;
    assert_eq!(f.hub.store.entry(&id).unwrap().data["status"], "sent");
    assert!(f.prompts()[2]["prompt"][0]["text"].as_str().unwrap().contains("Pedido durável da fila"));
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn falha_no_aviso_final_mantem_completing_ate_publicar_apos_reinicio() {
    let mut f = Fixture::new();
    f.send().await;
    let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_notice BEFORE INSERT ON entries WHEN NEW.id LIKE '%-completion' BEGIN SELECT RAISE(FAIL, 'aviso indisponível'); END;").unwrap();
    f.hub.send_cmd("bot", super::Cmd::Stop).unwrap();
    f.until(|| {
        f.hub
            .store
            .kv_get("turn.waiting.bot")
            .is_some_and(|raw| serde_json::from_str::<Value>(&raw).unwrap()["phase"] == "completing")
    })
    .await;
    f.hub.shutdown().await;
    db.execute_batch("DROP TRIGGER reject_notice").unwrap();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| f.hub.store.kv_get("turn.waiting.bot").is_none()).await;
    let entries = f.hub.store.history("bot", i64::MAX, 100).unwrap();
    assert_eq!(entries.iter().filter(|e| e.kind == "notice" && e.data["text"] == "Stopped.").count(), 1);
    assert_eq!(f.prompts().len(), 1);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn parar_cancela_entrada_duravel_cujo_comando_ainda_nao_chegou() {
    let f = Fixture::new();
    f.send().await;
    let entry = f
        .hub
        .store
        .insert_entry(
            &crate::store::Lane::main("bot"),
            EntryKind::User,
            2,
            &json!({"text":"Comando atrasado", "status":"queued", "subscriptionQueued":true}),
        )
        .unwrap();
    f.hub.send_cmd("bot", super::Cmd::Stop).unwrap();
    f.until(|| f.hub.runtime("bot").status == BotStatus::Idle).await;
    assert_eq!(f.hub.store.entry(&entry.id).unwrap().data["status"], "cancelled");
    f.hub
        .send_cmd(
            "bot",
            super::Cmd::Send {
                lane: crate::store::Lane::main("bot"),
                entry_id: entry.id,
                text: "Comando atrasado".into(),
            },
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(f.prompts().len(), 1);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn cancelamento_da_fila_com_sqlite_indisponivel_nao_reexecuta_no_reinicio() {
    let mut f = Fixture::new();
    f.send().await;
    let entry = f
        .hub
        .store
        .insert_entry(
            &crate::store::Lane::main("bot"),
            EntryKind::User,
            2,
            &json!({"text":"Não executar", "status":"queued", "subscriptionQueued":true}),
        )
        .unwrap();
    let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_cancel BEFORE UPDATE ON entries WHEN json_extract(NEW.data, '$.status') = 'cancelled' BEGIN SELECT RAISE(FAIL, 'cancel indisponível'); END;").unwrap();
    f.hub.send_cmd("bot", super::Cmd::Stop).unwrap();
    f.until(|| {
        f.hub
            .store
            .kv_get("turn.waiting.bot")
            .is_some_and(|raw| serde_json::from_str::<Value>(&raw).unwrap()["phase"] == "completing")
    })
    .await;
    f.hub.shutdown().await;
    db.execute_batch("DROP TRIGGER reject_cancel").unwrap();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| f.hub.store.kv_get("turn.waiting.bot").is_none()).await;
    assert_eq!(f.hub.store.entry(&entry.id).unwrap().data["status"], "cancelled");
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(f.prompts().len(), 1);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Troca de COMANDO durante a espera com handoff falho, cancelada pelo Stop: não sobra a sessão ACP do comando
/// anterior nem prompt indevido. A seleção da conversa (D372) não muda.
#[tokio::test]
async fn cancelar_troca_de_comando_com_handoff_falho_nao_deixa_sessao_anterior() {
    let f = Fixture::new();
    f.send().await;
    let original = f.hub.store.bot("bot").unwrap().unwrap().config.command.unwrap();
    let command = format!("{original} --variante");
    let engine = crate::fork::sessoes::engine_choice::read(&f.hub.store.db.locked(), "bot").unwrap();
    let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
    let quoted = command.replace('\'', "''");
    db.execute_batch(&format!("CREATE TRIGGER reject_switch BEFORE INSERT ON kv WHEN NEW.k = 'turn.waiting.bot' AND json_extract(NEW.v, '$.command') = '{quoted}' BEGIN SELECT RAISE(FAIL, 'troca indisponível'); END;")).unwrap();
    f.hub.update_bot_by_user(&json!({"id":"bot", "command":command})).unwrap();
    f.until(|| f.hub.runtime("bot").status == BotStatus::Error).await;
    f.hub.send_cmd("bot", super::Cmd::Stop).unwrap();
    f.until(|| f.hub.runtime("bot").status == BotStatus::Idle).await;
    assert!(f.hub.store.bot("bot").unwrap().unwrap().session_id.is_none());
    assert_eq!(f.prompts().len(), 1);
    assert_eq!(crate::fork::sessoes::engine_choice::read(&f.hub.store.db.locked(), "bot").unwrap(), engine);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn rodada3_cancelamento_do_host_preserva_mensagens_na_fila() {
    let f = Fixture::new();
    f.send().await;
    let result = crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId":"bot", "text":"Preservar após cancelamento do host"}),
    )
    .await
    .unwrap();
    let id = result["entry"]["id"].as_str().unwrap();
    f.hub.shutdown().await;
    let mut actor = f.actor();
    assert!(actor.restore_waiting());
    actor.restore_waiting_queue().unwrap();
    actor.cancel_turn().await;
    assert_eq!(f.hub.store.entry(id).unwrap().data["status"], "queued");
    assert_eq!(actor.queue.len(), 1);
    let (done, mut rx) = tokio::sync::mpsc::unbounded_channel();
    actor.next_in_queue(&done).await;
    actor.finish_turn(&rx.recv().await.unwrap().result);
    assert_eq!(f.prompts().len(), 2);
    assert!(f.prompts()[1]["prompt"][0]["text"].as_str().unwrap().contains("Preservar após cancelamento"));
    if let Some(conn) = actor.conn.take() {
        conn.acp.kill().await;
    }
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn rodada3_rejected_nao_classifica_erro_posterior_sem_sinal_de_limite() {
    let f = Fixture::with_backend("claude");
    std::fs::write(f.dir.join("stale-rejection"), "").unwrap();
    crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId":"bot", "text":"Pedido com falha comum"}),
    )
    .await
    .unwrap();
    f.until(|| f.hub.runtime("bot").status == BotStatus::Error || f.waiting_checkpoint_published()).await;
    assert_eq!(f.hub.runtime("bot").status, BotStatus::Error);
    assert!(f.hub.store.kv_get("turn.waiting.bot").is_none());
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn rodada3_reset_informado_tambem_esgota_tentativas() {
    let mut f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    let store = Store::open(&f.dir.join("test.db")).unwrap();
    let mut saved: Value = serde_json::from_str(&store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    saved["attemptCount"] = json!(5);
    saved["retryAt"] = json!(0);
    store.kv_set("turn.waiting.bot", &saved.to_string()).unwrap();
    std::fs::write(f.dir.join("always-limit"), "").unwrap();
    f.hub = Fixture::open(&f.dir, store);
    f.until(|| f.prompts().len() == 2 && f.hub.runtime("bot").status != BotStatus::Working).await;
    assert_eq!(f.hub.runtime("bot").status, BotStatus::Error);
    let saved: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    assert_eq!(saved["phase"], "paused");
    assert_eq!(f.hub.store.entry(saved["notice"].as_str().unwrap()).unwrap().data["subscriptionWait"]["retry"], true);
    f.hub.shutdown().await;
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| f.hub.runtime("bot").status == BotStatus::Error).await;
    assert_eq!(f.prompts().len(), 2);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn rodada3_turno_posterior_a_espera_volta_ao_fluxo_normal() {
    let mut f = Fixture::new();
    f.send().await;
    std::fs::write(f.dir.join("hold-queued"), "").unwrap();
    crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId":"bot", "text":"Pedido seguinte"}),
    )
    .await
    .unwrap();
    // D372: a espera termina pela retomada após reiniciar (antes, pela edição do modelo).
    f.hub.shutdown().await;
    f.make_retry_due();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| f.prompts().len() == 3).await;
    assert!(f.hub.store.kv_get("turn.waiting.bot").is_none(), "o lote posterior não é uma retomada D72");
    std::fs::remove_file(f.dir.join("hold-queued")).unwrap();
    f.until(|| f.hub.runtime("bot").status == BotStatus::Idle).await;
    f.hub.store.kv_set("turn.waiting.bot", r#"{"phase":"finished"}"#).unwrap();
    std::fs::write(f.dir.join("hold"), "").unwrap();
    let result = crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId":"bot", "text":"Turno normal em voo"}),
    )
    .await
    .unwrap();
    f.until(|| f.prompts().len() == 4).await;
    assert!(f.hub.store.kv_get("turn.waiting.bot").is_none());
    assert_ne!(result["entry"]["data"]["subscriptionQueued"], true);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn rodada3_reset_curto_respeita_intervalo_minimo() {
    let f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    let mut actor = f.actor();
    assert!(actor.restore_waiting());
    let now = crate::store::now_ms();
    actor.finish_turn(&Err(crate::agent::acp::RpcError {
        code: -32603,
        message: "Internal error".into(),
        kind: None,
        data: json!({"codex_error_info":"UsageLimitExceeded", "resetsAt":now + 2000}),
    }
    .into()));
    let saved: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    assert!(saved["retryAt"].as_i64().unwrap() >= now + 300_000);
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// MX-263: rotina com a execução devolvida esperando no checkpoint D72.
async fn routine_waiting(restart: bool) -> (Fixture, String) {
    let mut f = Fixture::new();
    let routine = f
        .hub
        .routines
        .save(
            &f.hub,
            "bot",
            &json!({"name":"Rotina limitada", "instruction":"Conclua",
        "triggers":[{"type":"webhook"}], "timeoutSeconds":15}),
        )
        .unwrap();
    let id = routine["routine"]["id"].as_str().unwrap();
    let run = f.enqueue(id, "test", true).await;
    let run_id = run["run"]["id"].as_str().unwrap();
    f.hub.send_cmd("bot", super::Cmd::Routine(run_id.into())).unwrap();
    f.until(|| f.waiting_checkpoint_published()).await;
    let saved: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    assert_eq!(saved["input"]["routine"], run_id);
    // A espera devolve a run a `Recovering`: a retomada prepara uma continuação da
    // cadeia em vez de um segundo `started` sobre o recibo já despachado.
    assert_eq!(f.hub.routines.list("bot")["runs"][0]["status"], "recovering");
    if restart {
        f.hub.shutdown().await;
        f.make_routine_retry_due_after_start();
        f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    }
    (f, run_id.to_owned())
}

/// MX-263: a rotina atinge o limite, entra em Aguardando (sem falhar) e retoma no horário.
/// A retomada passa por reinício: o `retryAt` em memória não é editável no teste
/// e o checkpoint persistido é a fonte de verdade da volta.
async fn routine_limit_waits_and_finishes() {
    let (f, run_id) = routine_waiting(true).await;
    let saved: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    assert!(saved["resumeAt"].as_i64().is_some());
    let notice = saved["notice"].as_str().and_then(|id| f.hub.store.entry(id)).unwrap();
    assert!(notice.data["subscriptionWait"].is_object());
    assert!(notice.data["text"].as_str().unwrap().contains("Rotina «Rotina limitada»"));
    assert_eq!(notice.data["routineWait"]["runId"], run_id);
    // `routineId` no topo faria o desktop trocar o aviso pelo botão da rotina.
    assert!(notice.data["routineId"].is_null());
    f.until(|| f.prompts().len() == 2).await;
    f.until(|| {
        f.hub.routines.list("bot")["runs"][0]["status"] == "succeeded"
            && f.hub.store.kv_get("turn.waiting.bot").is_none()
    })
    .await;
    // Uma execução só: o gatilho não reenfileirou e a retomada não repetiu.
    assert_eq!(f.hub.routines.list("bot")["runs"].as_array().unwrap().len(), 1);
    assert_eq!(f.prompts().len(), 2);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn mx263_rotina_no_limite_espera_volta_e_conclui_uma_vez() {
    routine_limit_waits_and_finishes().await;
}

/// MX-263: força a retomada da espera pela mesma RPC de "retomar agora" — equivale
/// a antecipar o `retryAt` sem reiniciar o Hub.
async fn resume_wait(f: &Fixture, saved: &Value) {
    let (reply, receive) = tokio::sync::oneshot::channel();
    f.hub
        .send_cmd(
            "bot",
            super::Cmd::UsageWaitResume {
                target: crate::fork::usage_limits::Target {
                    bot_id: "bot".into(),
                    chat_id: "bot".into(),
                    session_id: None,
                    entry_id: saved["notice"].as_str().unwrap().into(),
                    wait_id: saved["usageControl"]["id"].as_str().unwrap().into(),
                },
                reply,
                origem: None,
            },
        )
        .unwrap();
    receive.await.unwrap().unwrap();
}

/// MX-263 P1: a retomada automática NO MESMO Hub (sem reiniciar) prepara uma
/// continuação da cadeia — a run não vira `interrupted` nem duplica `started`.
#[tokio::test]
async fn mx263_rotina_retomada_sem_reiniciar_conclui_uma_vez() {
    let (f, run_id) = routine_waiting(false).await;
    let saved: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    resume_wait(&f, &saved).await;
    f.until(|| f.prompts().len() == 2).await;
    f.until(|| {
        f.hub.routines.list("bot")["runs"][0]["status"] == "succeeded"
            && f.hub.store.kv_get("turn.waiting.bot").is_none()
    })
    .await;
    // Uma execução só: mesma run, um prompt a mais, checkpoint tombstone limpo.
    let list = f.hub.routines.list("bot");
    let runs = list["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["id"], run_id);
    assert_eq!(f.prompts().len(), 2);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// MX-263 P2: `StoreBusy` transitório na preparação da retomada devolve o turno à
/// espera com o número de turno restaurado; a próxima tentativa conclui uma vez.
#[tokio::test]
async fn mx263_retomada_com_storebusy_transitorio_conclui_uma_vez() {
    let (f, _run_id) = routine_waiting(false).await;
    f.hub.routines.fail_next.store(1, std::sync::atomic::Ordering::SeqCst);
    let saved: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    resume_wait(&f, &saved).await;
    // A tentativa abortada volta a `waiting` com o backoff do D72; a próxima
    // retomada (agenda ou "retomar agora") conclui uma vez só.
    f.until(|| {
        f.hub.store.kv_get("turn.waiting.bot").is_some_and(|raw| {
            let checkpoint: Value = serde_json::from_str(&raw).unwrap();
            checkpoint["phase"] == "waiting" && checkpoint["attemptCount"] == 1
        })
    })
    .await;
    resume_wait(&f, &saved).await;
    f.until(|| f.prompts().len() == 2).await;
    f.until(|| {
        f.hub.routines.list("bot")["runs"][0]["status"] == "succeeded"
            && f.hub.store.kv_get("turn.waiting.bot").is_none()
    })
    .await;
    assert_eq!(f.prompts().len(), 2);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// MX-263 P2: com a tentativa abortada (`StoreBusy`, turno zerado), excluir a rotina ainda
/// cancela a espera — e a fila volta a andar com um pedido novo.
#[tokio::test]
async fn mx263_storebusy_e_exclusao_cancela_a_espera_e_libera_a_fila() {
    let (f, routine_id) = {
        let (f, _run_id) = routine_waiting(false).await;
        let routine_id = f.hub.routines.list("bot")["runs"][0]["routineId"].as_str().unwrap().to_owned();
        (f, routine_id)
    };
    // Uma tentativa abortada basta: o rollback deixa a espera sem turno próprio
    // (turno realocado) até a exclusão chegar.
    f.hub.routines.fail_next.store(3, std::sync::atomic::Ordering::SeqCst);
    let saved: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    resume_wait(&f, &saved).await;
    f.until(|| {
        f.hub.store.kv_get("turn.waiting.bot").is_some_and(|raw| {
            let checkpoint: Value = serde_json::from_str(&raw).unwrap();
            checkpoint["phase"] == "waiting" && checkpoint["attemptCount"] == 1
        })
    })
    .await;
    f.hub.routines.remove(&f.hub.store, "bot", &routine_id).unwrap();
    f.until(|| {
        f.hub.routines.list("bot")["runs"][0]["status"] == "cancelled"
            && f.hub
                .store
                .kv_get("turn.waiting.bot")
                .is_none_or(|raw| serde_json::from_str::<Value>(&raw).unwrap()["phase"] == "finished")
    })
    .await;
    // A fila anda: um pedido direto do dono roda normalmente depois do cancelamento.
    crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId": "bot", "text": "Depois do cancelamento"}),
    )
    .await
    .unwrap();
    f.until(|| f.prompts().len() == 2).await;
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// MX-263 P2 (r3): a retomada atinge o limite de novo e a transição falha — a
/// espera retirada pelo `take()` NÃO pode virar checkpoint `retrying` órfão: ela
/// volta a ser persistida e o término da run a cancela, liberando a fila.
#[tokio::test]
async fn mx263_falha_na_transicao_na_retomada_nao_deixa_espera_orfa() {
    let (f, _run_id) = routine_waiting(false).await;
    // Limite em toda tentativa + falha injetada na transição da run: a retomada
    // envia, o motor devolve limite, `wait_on_limit` retira a espera e a
    // transição para `Recovering` falha.
    std::fs::write(f.dir.join("always-limit"), "").unwrap();
    f.hub.routines.fail_mark.store(1, std::sync::atomic::Ordering::SeqCst);
    let saved: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    resume_wait(&f, &saved).await;
    f.until(|| {
        f.hub.routines.list("bot")["runs"][0]["status"] == "failed"
            && f.hub
                .store
                .kv_get("turn.waiting.bot")
                .is_none_or(|raw| serde_json::from_str::<Value>(&raw).unwrap()["phase"] == "finished")
    })
    .await;
    // A fila anda: um pedido direto do dono roda normalmente depois do término.
    crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId": "bot", "text": "Depois da retomada abortada"}),
    )
    .await
    .unwrap();
    f.until(|| f.prompts().len() == 3).await;
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// MX-263 P2: `StoreBusy` PERSISTENTE na preparação não pode retomar para sempre.
/// No teto de tentativas o rollback pausa a espera e a política de rotina pausada
/// encerra a execução — a vaga do bot e a fila voltam a andar.
#[tokio::test]
async fn mx263_storebusy_persistente_esgota_tentativas_e_libera_a_fila() {
    let (f, _run_id) = routine_waiting(false).await;
    // Contador já no limite do D72: a próxima tentativa abortada é a última.
    let mut saved: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    saved["attemptCount"] = json!(5);
    saved["retryAt"] = json!(crate::store::now_ms() + 2_000);
    let mut f = f;
    f.hub.shutdown().await;
    f.hub.store.kv_set("turn.waiting.bot", &saved.to_string()).unwrap();
    // A run é catch-up: a pausa deve encerrá-la de forma terminal, sem que o
    // MX-110 a devolva a `pending` para novas tentativas automáticas.
    let mut state: Value = serde_json::from_str(&f.hub.store.kv_get("routines.v1").unwrap()).unwrap();
    state["runs"][0]["catchup"] = json!(true);
    f.hub.store.kv_set("routines.v1", &state.to_string()).unwrap();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.hub.routines.fail_next.store(3, std::sync::atomic::Ordering::SeqCst);
    f.until(|| {
        f.hub.routines.list("bot")["runs"][0]["status"] == "failed"
            && f.hub
                .store
                .kv_get("turn.waiting.bot")
                .is_none_or(|raw| serde_json::from_str::<Value>(&raw).unwrap()["phase"] == "finished")
    })
    .await;
    assert_eq!(f.prompts().len(), 1, "nenhum prompt além do primeiro — a espera nunca estaciona");
    // A fila anda: um pedido direto do dono roda normalmente depois do término.
    crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId": "bot", "text": "Depois do esgotamento"}),
    )
    .await
    .unwrap();
    f.until(|| f.prompts().len() == 2).await;
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// MX-263 P2: falha transitória na transição Running→Recovering falha fechado —
/// sem checkpoint de espera preso e com a fila liberada para a próxima execução.
#[tokio::test]
async fn mx263_falha_na_transicao_da_run_fecha_sem_espera() {
    let f = Fixture::new();
    let routine = f
        .hub
        .routines
        .save(
            &f.hub,
            "bot",
            &json!({"name":"Rotina frágil", "instruction":"Conclua",
        "triggers":[{"type":"webhook"}], "timeoutSeconds":15}),
        )
        .unwrap();
    let id = routine["routine"]["id"].as_str().unwrap().to_owned();
    f.hub.routines.fail_mark.store(1, std::sync::atomic::Ordering::SeqCst);
    let run = f.enqueue(&id, "test", true).await;
    f.hub.send_cmd("bot", super::Cmd::Routine(run["run"]["id"].as_str().unwrap().into())).unwrap();
    f.until(|| f.hub.routines.list("bot")["runs"][0]["status"] == "failed").await;
    assert!(f.hub.store.kv_get("turn.waiting.bot").is_none(), "sem transição não estaciona");
    // A vaga do bot ficou livre: a execução seguinte corre e conclui.
    let run2 = f.enqueue(&id, "test", true).await;
    f.hub.send_cmd("bot", super::Cmd::Routine(run2["run"]["id"].as_str().unwrap().into())).unwrap();
    f.until(|| {
        let list = f.hub.routines.list("bot");
        list["runs"].as_array().unwrap().iter().any(|r| r["status"] == "succeeded")
    })
    .await;
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn mx263_rotina_excluida_ou_desativada_cancela_a_espera() {
    for delete in [false, true] {
        let (f, routine_id) = if delete {
            let (f, _) = routine_waiting(false).await;
            let routine_id = f.hub.routines.list("bot")["runs"][0]["routineId"].as_str().unwrap().to_owned();
            (f, routine_id)
        } else {
            // Execução de agenda (não teste): desativar a rotina cancela a espera.
            let f = Fixture::new();
            let routine = f
                .hub
                .routines
                .save(
                    &f.hub,
                    "bot",
                    &json!({"name":"Rotina de agenda", "instruction":"Conclua",
                "triggers":[{"type":"interval","seconds":60}], "timeoutSeconds":15}),
                )
                .unwrap();
            let id = routine["routine"]["id"].as_str().unwrap();
            let run = f.enqueue(id, "schedule", false).await;
            f.hub.send_cmd("bot", super::Cmd::Routine(run["run"]["id"].as_str().unwrap().into())).unwrap();
            f.until(|| f.waiting_checkpoint_published()).await;
            (f, id.to_owned())
        };
        if delete {
            f.hub.routines.remove(&f.hub.store, "bot", &routine_id).unwrap();
        } else {
            f.hub.routines.set_enabled(&f.hub.store, "bot", &routine_id, false).unwrap();
        }
        f.until(|| {
            f.hub.routines.list("bot")["runs"][0]["status"] == "cancelled"
                && f.hub
                    .store
                    .kv_get("turn.waiting.bot")
                    .is_none_or(|raw| serde_json::from_str::<Value>(&raw).unwrap()["phase"] == "finished")
        })
        .await;
        assert_eq!(f.prompts().len(), 1, "a espera cancelada não retoma");
        f.hub.shutdown().await;
        let _ = std::fs::remove_dir_all(&f.dir);
    }
}

#[tokio::test]
async fn mx263_rotina_desativada_em_teste_continua_esperando() {
    let (f, _run_id) = routine_waiting(false).await;
    let routine_id = f.hub.routines.list("bot")["runs"][0]["routineId"].as_str().unwrap().to_owned();
    // O disparo de teste corre mesmo desativado (mesmo critério de `begin`): a espera fica.
    f.hub.routines.set_enabled(&f.hub.store, "bot", &routine_id, false).unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(f.waiting_checkpoint_published());
    let mut f = f;
    f.hub.shutdown().await;
    f.make_routine_retry_due_after_start();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| f.hub.routines.list("bot")["runs"][0]["status"] == "succeeded").await;
    assert_eq!(f.prompts().len(), 2);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn mx263_teto_bloqueando_na_retomada_segura_a_rotina() {
    let (f, _run_id) = routine_waiting(false).await;
    f.hub.store.kv_set("donna.spend-ceiling.v1", &json!({"enabled":true,"tokens":1}).to_string()).unwrap();
    // Toda linha do mês precisa ser conferível: a fixture registra tokens/valor
    // nulos, o que deixaria o teto em "não verificado" para sempre neste teste.
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE donna_turn_usage SET input_tokens=0, output_tokens=0, amount=0.1, currency='USD'", [])
        .unwrap();
    f.hub
        .store
        .db
        .locked()
        .execute(
            "INSERT INTO donna_turn_usage(id,bot_id,chat_id,session_id,at,input_tokens,output_tokens,amount,currency,source)
             VALUES(?1,'bot','bot','s',?2,10,10,NULL,NULL,'motor')",
            rusqlite::params![uuid::Uuid::new_v4().to_string(), crate::fork::custo::month(None).unwrap().1 + 1],
        )
        .unwrap();
    let mut f = f;
    f.hub.shutdown().await;
    f.make_routine_retry_due_after_start();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    // A preparação transitória pode precisar de algumas voltas do ciclo da
    // espera: cada uma devolve a espera à fase `waiting` com o `retryAt` no
    // backoff do D72 (~5 min). Enquanto o teto bloqueia, nenhum despacho está
    // em voo — reabrir o Hub com o horário próximo repete a volta na hora.
    tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            let held = crate::fork::teto::turn_holds_of(&f.hub.store.db.locked(), "bot")
                .unwrap()
                .iter()
                .any(|h| h.body["waiting"] == true);
            if held {
                break;
            }
            if f.routine_wait_rearmed() {
                f.reopen_retrying_routine_wait().await;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .expect("espera do teto para a retomada da rotina");
    assert_eq!(f.prompts().len(), 1, "o teto segurou a retomada antes do envio");
    // Reinício COM o hold persistido: ele restaura como espera D72 (não como
    // recuperação legada) e liberar o teto retoma uma única vez.
    let mut f = f;
    f.hub.shutdown().await;
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(f.prompts().len(), 1, "reaberto, o hold segue segurando o envio");
    crate::fork::teto::call(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "donnaCeilingRelease",
        json!({"kind":"setLimit","alvo":"tokens","newLimit":1000}),
    )
    .await
    .unwrap();
    // Liberado, o despacho pode estar em voo a qualquer momento — reabrir aqui
    // interromperia a run. Se a retomada falhar de forma transitória, a espera
    // fica devida só no backoff do D72 (~5 min): a margem cobre essa volta.
    tokio::time::timeout(Duration::from_secs(330), async {
        while !(f.prompts().len() == 2 && f.hub.routines.list("bot")["runs"][0]["status"] == "succeeded") {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("rotina retomada após a liberação do teto");
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn mx263_rotina_sem_horario_de_retorno_espera_com_backoff_d72() {
    let f = Fixture::new();
    std::fs::write(f.dir.join("no-reset"), "").unwrap();
    let routine = f
        .hub
        .routines
        .save(
            &f.hub,
            "bot",
            &json!({"name":"Rotina sem hora", "instruction":"Conclua",
        "triggers":[{"type":"webhook"}], "timeoutSeconds":15}),
        )
        .unwrap();
    let id = routine["routine"]["id"].as_str().unwrap();
    let run = f.enqueue(id, "test", true).await;
    f.hub.send_cmd("bot", super::Cmd::Routine(run["run"]["id"].as_str().unwrap().into())).unwrap();
    f.until(|| f.waiting_checkpoint_published()).await;
    let saved: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    assert!(saved["resumeAt"].is_null(), "sem retorno informado: vale o backoff D72");
    let now = crate::store::now_ms();
    assert!(saved["retryAt"].as_i64().unwrap() >= now + 290_000);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn assinatura_pessoal_recusa_nao_reenfileira_quando_sqlite_falha() {
    let f = Fixture::new();
    f.hub.shutdown().await;
    let mut actor = f.actor();
    let lane = crate::store::Lane::main("bot");
    let entry = f.hub.store.insert_entry(&lane, EntryKind::User, 1, &json!({"text":"oi","status":"queued"})).unwrap();
    f.hub.store.db.locked().execute_batch("CREATE TRIGGER reject_subscription_failure BEFORE UPDATE ON entries WHEN json_extract(NEW.data,'$.status')='failed' BEGIN SELECT RAISE(ABORT,'indisponível'); END;").unwrap();
    let ids = [entry.id.clone()];
    actor.stop_subscription_retry(&ids, &crate::fork::assinatura_pessoal::Refusal::Automation.into());
    actor.requeue_unreserved_humans(&ids);
    assert!(!actor.queue_has_user(&entry.id));
    assert!(actor.restore_waiting_queue().is_err());
    f.hub.store.db.locked().execute_batch("DROP TRIGGER reject_subscription_failure").unwrap();
    actor.restore_waiting_queue().unwrap();
    assert!(!actor.queue_has_user(&entry.id));
    assert_eq!(f.hub.store.entry(&entry.id).unwrap().data["status"], "failed");
    std::fs::remove_dir_all(&f.dir).unwrap();
}

#[tokio::test]
async fn assinatura_pessoal_rotina_para_antes_do_motor() {
    let f = Fixture::with_backend("claude");
    let routine = f
        .hub
        .routines
        .save(&f.hub, "bot", &json!({"name":"Rotina", "instruction":"Não enviar", "triggers":[{"type":"webhook"}]}))
        .unwrap();
    let id = routine["routine"]["id"].as_str().unwrap();
    let run = f.enqueue(id, "test", true).await;
    f.hub.send_cmd("bot", super::Cmd::Routine(run["run"]["id"].as_str().unwrap().into())).unwrap();
    f.until(|| f.hub.routines.list("bot")["runs"][0]["status"] == "failed").await;
    assert_eq!(f.prompts(), [] as [Value; 0]);
    assert!(!f.dir.join("session-count").exists());
    assert!(!f.waiting_checkpoint_published());
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn mx263_rotina_respeita_o_teto_de_tentativas_da_espera_d72() {
    let (f, _run_id) = routine_waiting(false).await;
    // Limite em toda tentativa + contador no limite: a espera pausaria no D72, mas
    // uma rotina pausada não tem dono para destravar — ela encerra a execução.
    std::fs::write(f.dir.join("always-limit"), "").unwrap();
    let mut saved: Value = serde_json::from_str(&f.hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap();
    saved["attemptCount"] = json!(5);
    f.hub.store.kv_set("turn.waiting.bot", &saved.to_string()).unwrap();
    let mut f = f;
    f.hub.shutdown().await;
    f.make_routine_retry_due_after_start();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| {
        let status = f.hub.routines.list("bot")["runs"][0]["status"].clone();
        status == "failed"
            && f.hub
                .store
                .kv_get("turn.waiting.bot")
                .is_none_or(|raw| serde_json::from_str::<Value>(&raw).unwrap()["phase"] == "finished")
    })
    .await;
    assert_eq!(f.prompts().len(), 2, "a retomada chegou a tentar uma vez antes de desistir");
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn mx263_rotina_com_limite_antes_da_permissao_falha_sem_espera() {
    let f = Fixture::new();
    std::fs::write(f.dir.join("limit-new"), "").unwrap();
    let routine = f
        .hub
        .routines
        .save(
            &f.hub,
            "bot",
            &json!({"name":"Rotina limitada", "instruction":"Conclua",
        "triggers":[{"type":"webhook"}], "timeoutSeconds":15}),
        )
        .unwrap();
    let id = routine["routine"]["id"].as_str().unwrap();
    let run = f.enqueue(id, "test", true).await;
    f.hub.send_cmd("bot", super::Cmd::Routine(run["run"]["id"].as_str().unwrap().into())).unwrap();
    f.until(|| f.hub.routines.list("bot")["runs"][0]["status"] == "failed").await;
    assert!(f.hub.store.kv_get("turn.waiting.bot").is_none(), "sem identidade de admissão não estaciona");
    assert!(f.hub.routines.list("bot")["runs"][0]["detail"].as_str().unwrap().contains("Limite da assinatura"));
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn rodada3_turnos_ocultos_e_compactacao_encerram_sem_espera_ou_reenvio() {
    for compact in [false, true] {
        let f = Fixture::new();
        f.send().await;
        f.hub.shutdown().await;
        f.hub
            .store
            .db
            .locked()
            .execute_batch("DELETE FROM kv WHERE k IN ('turn.waiting.bot', 'turn.inflight.bot')")
            .unwrap();
        std::fs::write(f.dir.join("always-limit"), "").unwrap();
        let mut actor = f.actor();
        actor.compacting = compact.then_some(false);
        actor.raw_prompt = compact;
        let text = if compact { "/compact" } else { "Turno oculto da fixture" };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        actor.start_turn(crate::store::Lane::main("bot"), &[], text, true, &tx).await.unwrap();
        if !compact {
            // Uma retomada oculta não herda a pessoa da sessão anterior: ela é encerrada sem reenviar o pedido.
            assert!(actor.turn.is_none() && actor.waiting.is_none());
            assert_eq!(f.prompts().len(), 1);
            if let Some(conn) = actor.conn.take() {
                conn.acp.kill().await;
            }
            let _ = std::fs::remove_dir_all(&f.dir);
            continue;
        }
        let done = tokio::time::timeout(Duration::from_secs(15), rx.recv())
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "compact={compact}, turn={}, pending={}, prompts={:?}",
                    actor.turn.is_some(),
                    actor.pending_admission.is_some(),
                    f.prompts()
                )
            })
            .unwrap();
        actor.finish_turn(&done.result);
        assert!(actor.turn.is_none() && actor.waiting.is_none());
        assert!(f.hub.store.kv_get("turn.waiting.bot").is_none());
        actor.retry_waiting(&tx).await;
        assert_eq!(f.prompts().len(), 2);
        if compact {
            assert_eq!(f.prompts()[1]["prompt"][0]["text"], "/compact");
        }
        assert!(
            f.hub
                .store
                .history("bot", i64::MAX, 100)
                .unwrap()
                .iter()
                .any(|e| e.kind == "notice" && e.data["text"].as_str().is_some_and(|t| t.contains("foi encerrad")))
        );
        if let Some(conn) = actor.conn.take() {
            conn.acp.kill().await;
        }
        let _ = std::fs::remove_dir_all(&f.dir);
    }
}

#[tokio::test]
async fn rodada3_hint_expirado_nao_completa_reset_do_resultado() {
    let f = Fixture::with_backend("claude");
    f.hub.shutdown().await;
    let mut actor = f.actor();
    let reset = crate::store::now_ms() + 3_600_000;
    actor.turn_limit =
        Some((crate::agent::subscription::Limit { resets_at: Some(reset) }, crate::store::now_ms() - 61_000));
    assert!(actor.result_limit(&Err(anyhow::anyhow!("falha comum"))).is_none());
    let result = Ok(json!({"stopReason":"rate_limit"}));
    assert!(actor.result_limit(&result).unwrap().resets_at.is_none());
    actor.turn_limit = Some((crate::agent::subscription::Limit { resets_at: Some(reset) }, crate::store::now_ms()));
    assert_eq!(actor.result_limit(&result).unwrap().resets_at, Some(reset));
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn rodada3_pedido_de_outro_bot_encerra_com_limite_sem_estacionar() {
    let f = Fixture::new();
    let mut source = f.hub.store.bot("bot").unwrap().unwrap().config;
    source.id = "source".into();
    source.name = "Remetente".into();
    let source = f.hub.create_bot(source).unwrap();
    f.hub.team.start_turn(source["id"].as_str().unwrap(), None);
    crate::chat::team::call(
        &f.hub,
        source["id"].as_str().unwrap(),
        "message_bot",
        &json!({"botId":"bot", "message":"Pedido do remetente"}),
    )
    .await
    .unwrap();
    f.until(|| f.hub.runtime("bot").status == BotStatus::Error || f.waiting_checkpoint_published()).await;
    assert_eq!(f.hub.runtime("bot").status, BotStatus::Error);
    assert!(f.hub.store.kv_get("turn.waiting.bot").is_none());
    assert_eq!(f.prompts().len(), 1);
    assert!(
        f.hub
            .store
            .history("bot", i64::MAX, 100)
            .unwrap()
            .iter()
            .any(|e| e.data["botMessage"]["detail"].as_str().is_some_and(|text| text.contains("Limite da assinatura")))
    );
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn rodada3_stop_do_usuario_prevalece_sobre_limite_entregue_depois() {
    let f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    let mut actor = f.actor();
    assert!(actor.restore_waiting());
    actor.stop_requested = true;
    actor.finish_turn(&Err(crate::agent::acp::RpcError {
        code: -32603,
        message: "Internal error".into(),
        kind: None,
        data: json!({"codex_error_info":"UsageLimitExceeded"}),
    }
    .into()));
    assert_eq!(f.hub.runtime("bot").status, BotStatus::Idle);
    assert!(actor.waiting.is_none() && actor.turn.is_none());
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn resumo_privado_nao_vai_para_o_turno_de_grupo_depois_de_falha_na_abertura() {
    use crate::fork::bot_admission as admission;
    use crate::store::{EntryKind, Lane};
    let f = Fixture::new();
    f.hub.shutdown().await;
    std::fs::write(f.dir.join("blocked"), "").unwrap();
    let lane = Lane::main("bot");
    f.hub
        .store
        .insert_entry(
            &lane,
            EntryKind::User,
            1,
            &json!({"text":"SENTINELA privada do dono","status":"sent","contentOrigin":{"ingress":{"caller":"local"}}}),
        )
        .unwrap();
    let cfg = f.hub.store.bot("bot").unwrap().unwrap().config;
    crate::chat::memory::bind_profile_person(
        &f.hub.store,
        "bot",
        "sessao-anterior",
        Some(crate::chat::memory::profile::OWNER),
    )
    .unwrap();
    crate::chat::handoff::draft(&f.hub, &cfg, "sessao-anterior", crate::chat::handoff::Reason::Full).unwrap();
    assert!(crate::chat::handoff::pending(&f.hub.store, "bot").unwrap().1.contains("SENTINELA"));
    let group: BotConfig =
        serde_json::from_value(json!({"id":"grupo","name":"Grupo","kind":"group","members":["bot"],"cwd":f.dir}))
            .unwrap();
    f.hub.store.save_bot(&group).unwrap();
    let mut actor = f.actor();
    let (done, mut rx) = tokio::sync::mpsc::unbounded_channel();

    // A conversa privada retira o resumo e falha ao abrir o motor.
    let mut broken = cfg.clone();
    broken.command = Some("/caminho/que/nao/existe".into());
    f.hub.store.save_bot(&broken).unwrap();
    let private = f
        .hub
        .store
        .insert_entry(
            &lane,
            EntryKind::User,
            2,
            &json!({"text":"Continue","status":"queued","contentOrigin":{"ingress":{"caller":"local"}}}),
        )
        .unwrap();
    actor.queue.push_back(super::Queued::User {
        lane: lane.clone(),
        entry_id: private.id.clone(),
        text: "Continue".into(),
    });
    actor.next_in_queue(&done).await;
    assert!(actor.pending_admission.is_some(), "a abertura falhou antes da permissão e conserva o lote");
    assert!(actor.handoff.as_deref().is_some_and(|h| h.contains("SENTINELA")));
    let handoff = actor.handoff.clone();
    let reserved = admission::current(&f.hub.store, "bot").unwrap().unwrap();
    assert_eq!(reserved.phase, admission::Phase::Reserved);
    assert_eq!(actor.dispatch.as_ref(), Some(&reserved.ticket()));
    assert_eq!(f.hub.store.entry(&private.id).unwrap().data["status"], "queued");
    assert_eq!(f.prompts(), [] as [Value; 0], "a abertura falha não enviou prompt");
    f.hub.store.save_bot(&cfg).unwrap();

    // A parada do turno reservado não pode apagar a mensagem que veio depois.
    let back = f
        .hub
        .store
        .insert_entry(
            &lane,
            EntryKind::User,
            3,
            &json!({"text":"De volta","status":"queued","contentOrigin":{"ingress":{"caller":"local"}}}),
        )
        .unwrap();
    actor.queue.push_back(super::Queued::User {
        lane: lane.clone(),
        entry_id: back.id.clone(),
        text: "De volta".into(),
    });
    // Só uma parada explícita pode liberar o grupo antes desse lote reservado.
    actor.cancel_turn().await;
    assert!(actor.turn.is_none() && actor.pending_admission.is_none());
    assert!(actor.dispatch.is_none() && admission::current(&f.hub.store, "bot").unwrap().is_none());
    assert_eq!(actor.handoff, handoff, "a parada conserva o resumo privado");
    assert_eq!(actor.handoff_chat, Some(Lane::main("bot")));
    assert_eq!(actor.queue.len(), 1, "cancel_turn conserva a fila posterior");
    assert!(matches!(actor.queue.front(), Some(super::Queued::User { lane: queued_lane, entry_id, .. })
        if queued_lane == &lane && entry_id == &back.id));
    assert_eq!(f.hub.store.entry(&back.id).unwrap().data["status"], "queued");
    assert_eq!(f.hub.store.entry(&back.id).unwrap().session_id, lane.chat);

    // Um turno de grupo vem antes de a conversa privada voltar: não leva o resumo dela.
    let group_lane = Lane::main("grupo");
    let prompt = "[Channel: \"Canal\"]\nSua vez";
    let (reply, _answer) = tokio::sync::oneshot::channel();
    actor.active_group = Some(crate::chat::group::GroupTurn {
        lane: group_lane.clone(),
        prompt: prompt.into(),
        shown: 0,
        epoch: f.hub.groups.reserve_epoch_for_test(&group_lane),
        reply,
    });
    actor.start_turn(group_lane, &[], prompt, false, &done).await.unwrap();
    assert!(actor.turn.is_some() && actor.pending_admission.is_none(), "o grupo precisa ter produtor de conclusão");
    let group_dispatch = admission::current(&f.hub.store, "bot").unwrap().expect("despacho do grupo");
    assert_eq!(group_dispatch.phase, admission::Phase::Permitted);
    let group_done = tokio::time::timeout(Duration::from_secs(20), rx.recv())
        .await
        .expect("VG-03a: grupo admitido não concluiu no ACP da fixture em 20 s")
        .expect("VG-03a: canal de conclusão do grupo fechado");
    assert_eq!(group_done.ticket, group_dispatch.ticket());
    assert_ne!(group_done.ticket, reserved.ticket());
    assert!(actor.accepts_completion(&group_done.ticket));
    assert!(group_done.result.is_ok(), "{:?}", group_done.result);
    actor.finish_turn(&group_done.result);
    assert_eq!(f.prompts().len(), 1);
    let in_group = f.prompts().last().unwrap().to_string();
    assert!(in_group.contains("Sua vez"), "{in_group}");
    assert!(!in_group.contains("SENTINELA"), "o grupo recebeu o resumo privado: {in_group}");
    assert_eq!(actor.handoff, handoff, "o resumo continua guardado para a conversa privada");

    // De volta à conversa privada, o resumo vai junto.
    assert!(actor.turn.is_none() && actor.pending_admission.is_none() && actor.dispatch.is_none());
    assert!(admission::current(&f.hub.store, "bot").unwrap().is_none(), "o grupo concluiu sua própria cerca");
    assert_eq!(f.hub.store.entry(&back.id).unwrap().data["status"], "queued");
    // Este Actor não tem tick: dirigir a elegibilidade após o backoff, sem esperar
    // pelo relógio nem alterar a fila, o handoff ou a permissão real do produtor.
    actor.queue_retry_at = None;
    actor.next_in_queue(&done).await;
    assert!(
        actor.turn.is_some() && actor.pending_admission.is_none(),
        "a fila privada precisa ter produtor de conclusão"
    );
    let private_dispatch = admission::current(&f.hub.store, "bot").unwrap().expect("despacho privado");
    assert_eq!(private_dispatch.phase, admission::Phase::Permitted);
    assert_eq!(f.hub.store.entry(&back.id).unwrap().data["status"], "sent", "claim privado real");
    let private_done = tokio::time::timeout(Duration::from_secs(20), rx.recv())
        .await
        .expect("VG-03a: privado admitido não concluiu no ACP da fixture em 20 s")
        .expect("VG-03a: canal de conclusão privado fechado");
    assert_eq!(private_done.ticket, private_dispatch.ticket());
    assert_ne!(private_done.ticket, group_done.ticket);
    assert!(actor.accepts_completion(&private_done.ticket));
    assert!(private_done.result.is_ok(), "{:?}", private_done.result);
    actor.finish_turn(&private_done.result);
    assert_eq!(f.prompts().len(), 2);
    let private = f.prompts().last().unwrap().to_string();
    assert!(private.contains("De volta") && private.contains("SENTINELA"), "{private}");
    if let Some(conn) = actor.conn.take() {
        conn.acp.kill().await;
    }
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn falha_no_vinculo_da_sessao_visivel_impede_o_turno() {
    let f = Fixture::new();
    std::fs::write(f.dir.join("blocked"), "").unwrap();
    // Só a ligação ACP → sessão visível falha; a exposição e a linhagem continuam gravando.
    let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER sem_vinculo BEFORE INSERT ON donna_session_acp BEGIN SELECT RAISE(FAIL, 'vínculo indisponível'); END;",
    )
    .unwrap();
    crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId": "bot", "text": "Continue a tarefa pendente"}),
    )
    .await
    .unwrap();
    f.until(|| f.human_admission_pending() && f.hub.runtime("bot").memory_exposure_error).await;
    assert_eq!(f.prompts(), [] as [Value; 0], "sem o vínculo, o turno não chega ao motor");
    assert!(f.hub.runtime("bot").memory_exposure_error, "a porta da memória automática continua fechada");
    assert_eq!(
        crate::fork::bot_admission::current(&f.hub.store, "bot").unwrap().unwrap().phase,
        crate::fork::bot_admission::Phase::Reserved
    );
    let bindings: i64 = db.query_row("SELECT COUNT(*) FROM donna_session_acp", [], |r| r.get(0)).unwrap();
    assert_eq!(bindings, 0, "o catálogo não confirmou um vínculo que falhou");
    db.execute_batch("DROP TRIGGER sem_vinculo").unwrap();
    f.until(|| f.prompts().len() == 1 && f.hub.runtime("bot").status == BotStatus::Idle).await;
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

// VG-03a: a mutação abaixo existe só em teste. Nenhuma RPC de pausa é publicada.
#[tokio::test]
async fn admissao_pausa_antes_preserva_fila_e_depois_de_permissao_deixa_atual_terminar() {
    use crate::fork::bot_admission as admission;
    let f = Fixture::new();
    std::fs::write(f.dir.join("blocked"), "").unwrap();
    admission::set_paused_for_test(&f.hub.store, "bot", true).unwrap();
    let sent = crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId":"bot","text":"Primeiro"}),
    )
    .await
    .unwrap();
    let first = sent["entry"]["id"].as_str().unwrap();
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(f.prompts(), [] as [Value; 0]);
    assert_eq!(f.hub.store.entry(first).unwrap().data["status"], "queued");
    assert!(f.hub.store.kv_read("turn.waiting.bot").unwrap().is_none());
    std::fs::write(f.dir.join("hold"), "").unwrap();
    admission::set_paused_for_test(&f.hub.store, "bot", false).unwrap();
    f.until(|| f.prompts().len() == 1).await;
    let dispatch = admission::current(&f.hub.store, "bot").unwrap().unwrap();
    assert_eq!(dispatch.phase, admission::Phase::Permitted);
    assert_eq!(Some(dispatch.execution_token), f.hub.runtime("bot").memory_turn_token);
    admission::set_paused_for_test(&f.hub.store, "bot", true).unwrap();
    let next = crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId":"bot","text":"Segundo"}),
    )
    .await
    .unwrap();
    std::fs::remove_file(f.dir.join("hold")).unwrap();
    f.until(|| f.hub.runtime("bot").status == BotStatus::Idle).await;
    assert_eq!(f.prompts().len(), 1, "o atual conclui, o próximo não foi admitido");
    assert_eq!(f.hub.store.entry(next["entry"]["id"].as_str().unwrap()).unwrap().data["status"], "queued");
    admission::set_paused_for_test(&f.hub.store, "bot", false).unwrap();
    f.until(|| f.prompts().len() == 2 && f.hub.runtime("bot").status == BotStatus::Idle).await;
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn admissao_pausa_durante_load_vence_ultima_fronteira_sem_sent_ou_falha_falsa() {
    use crate::fork::bot_admission as admission;
    let f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    f.hub
        .store
        .db
        .locked()
        .execute_batch("DELETE FROM kv WHERE k IN ('turn.waiting.bot', 'turn.inflight.bot')")
        .unwrap();
    std::fs::write(f.dir.join("slow-load"), "").unwrap();
    let entry = f
        .hub
        .store
        .insert_entry(
            &crate::store::Lane::main("bot"),
            EntryKind::User,
            2,
            &json!({"text":"Depois","status":"queued","contentOrigin":{"ingress":{"caller":"local"}}}),
        )
        .unwrap();
    let mut actor = f.actor();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let id = entry.id.clone();
    let task = tokio::spawn(async move {
        actor.start_turn(crate::store::Lane::main("bot"), &[id], "Depois", false, &tx).await.unwrap();
        (actor, tx)
    });
    f.until(|| f.dir.join("loading").exists()).await;
    admission::set_paused_for_test(&f.hub.store, "bot", true).unwrap();
    let (mut actor, tx) = task.await.unwrap();
    assert!(actor.pending_admission.is_some());
    assert_eq!(f.prompts().len(), 1);
    assert_eq!(f.hub.store.entry(&entry.id).unwrap().data["status"], "queued");
    assert_eq!(admission::current(&f.hub.store, "bot").unwrap().unwrap().phase, admission::Phase::Reserved);
    admission::set_paused_for_test(&f.hub.store, "bot", false).unwrap();
    actor.retry_admission(&tx).await;
    let done = rx.recv().await.unwrap();
    assert!(actor.accepts_completion(&done.ticket));
    actor.finish_turn(&done.result);
    assert_eq!(f.prompts().len(), 2);
    assert_eq!(f.hub.store.entry(&entry.id).unwrap().data["status"], "sent");
    if let Some(conn) = actor.conn.take() {
        conn.acp.kill().await;
    }
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn admissao_falha_de_persistencia_nao_envia_e_retenta_reserva_ou_permissao() {
    use crate::fork::bot_admission as admission;
    for phase in ["reserved", "permitted"] {
        let f = Fixture::new();
        std::fs::write(f.dir.join("blocked"), "").unwrap();
        f.hub.store.db.locked().execute_batch(&format!("CREATE TRIGGER reject_dispatch BEFORE INSERT ON kv WHEN NEW.k='turn.inflight.bot' AND json_extract(NEW.v,'$.phase')='{phase}' BEGIN SELECT RAISE(ABORT,'indisponível'); END;")).unwrap();
        let sent = crate::api::dispatch(
            &f.hub,
            &crate::api::devices::Caller::Local,
            "send",
            json!({"botId":"bot","text":"Conservar"}),
        )
        .await
        .unwrap();
        f.until(|| f.human_admission_pending()).await;
        assert_eq!(f.prompts(), [] as [Value; 0]);
        let record = admission::current(&f.hub.store, "bot").unwrap();
        if phase == "reserved" {
            assert!(record.is_none(), "reserva não confirmada nunca vira permissão");
        } else {
            assert_eq!(record.unwrap().phase, admission::Phase::Reserved);
        }
        let entry = f.hub.store.entry(sent["entry"]["id"].as_str().unwrap()).unwrap();
        assert_eq!(entry.data["status"], "queued");
        assert!(entry.data.get("subscriptionQueued").is_none());
        f.hub.store.db.locked().execute_batch("DROP TRIGGER reject_dispatch").unwrap();
        f.until(|| f.prompts().len() == 1 && f.hub.runtime("bot").status == BotStatus::Idle).await;
        assert!(admission::current(&f.hub.store, "bot").unwrap().is_none());
        f.hub.shutdown().await;
        let _ = std::fs::remove_dir_all(&f.dir);
    }
}

#[tokio::test]
async fn admissao_reinicio_apos_permissao_nao_repete_prompt_humano() {
    use crate::fork::bot_admission as admission;
    let f = Fixture::new();
    std::fs::write(f.dir.join("blocked"), "").unwrap();
    std::fs::write(f.dir.join("hold"), "").unwrap();
    crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId":"bot","text":"Efeito possível"}),
    )
    .await
    .unwrap();
    f.until(|| f.prompts().len() == 1).await;
    assert_eq!(admission::current(&f.hub.store, "bot").unwrap().unwrap().phase, admission::Phase::Permitted);
    f.hub.shutdown().await;
    std::fs::remove_file(f.dir.join("hold")).unwrap();
    let reopened = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| admission::current(&reopened.store, "bot").unwrap().is_none()).await;
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(f.prompts().len(), 1);
    assert!(
        reopened
            .store
            .history("bot", i64::MAX, 100)
            .unwrap()
            .iter()
            .any(|e| e.data["text"].as_str().is_some_and(|s| s.contains("Pode haver efeito já realizado")))
    );
    reopened.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn admissao_conclusao_antiga_nao_libera_tentativa_posterior() {
    use crate::fork::bot_admission as admission;
    let f = Fixture::new();
    f.hub.shutdown().await;
    let mut actor = f.actor();
    let old = admission::reserve(&f.hub.store, &actor.cfg, "bot", "hub-A", &admission::Origin::Human).unwrap();
    admission::finish(&f.hub.store, "bot", &old).unwrap();
    let next = admission::reserve(&f.hub.store, &actor.cfg, "bot", "hub-B", &admission::Origin::Group).unwrap();
    actor.dispatch = Some(next.clone());
    assert!(!actor.accepts_completion(&old));
    assert!(actor.accepts_completion(&next));
    assert_eq!(admission::current(&f.hub.store, "bot").unwrap().unwrap().execution_token, "hub-B");
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn admissao_checkpoint_falho_nao_apaga_prova_de_prompt_possivelmente_enviado() {
    use crate::fork::bot_admission as admission;
    let f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    let mut actor = f.actor();
    assert!(actor.restore_waiting());
    let mut checkpoint = serde_json::to_value(actor.waiting.as_ref().unwrap()).unwrap();
    checkpoint["phase"] = json!("retrying");
    let execution_token = checkpoint["executionToken"].as_str().unwrap().to_owned();
    let checkpoint = checkpoint.to_string();
    actor.waiting = Some(serde_json::from_str(&checkpoint).unwrap());
    let ticket =
        admission::reserve(&f.hub.store, &actor.cfg, "bot", &execution_token, &admission::Origin::Human).unwrap();
    let ids = actor.input.as_ref().unwrap().entries.clone();
    admission::permit(&f.hub.store, &actor.cfg, &ticket, Some(&checkpoint), &ids, None).unwrap().unwrap();
    actor.dispatch = Some(ticket);
    f.hub.store.db.locked().execute_batch("CREATE TRIGGER reject_wait BEFORE INSERT ON kv WHEN NEW.k='turn.waiting.bot' BEGIN SELECT RAISE(ABORT,'indisponível'); END;").unwrap();
    assert!(actor.wait_on_limit(crate::agent::subscription::Limit { resets_at: None }));
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    actor.retry_admission(&tx).await;
    assert_eq!(admission::current(&f.hub.store, "bot").unwrap().unwrap().phase, admission::Phase::Permitted);
    f.hub.store.db.locked().execute_batch("DROP TRIGGER reject_wait").unwrap();
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn admissao_boot_preserva_fila_extra_sem_prompt_na_principal() {
    use crate::fork::{
        bot_admission as admission,
        sessoes::{origin::SessionOrigin, store as sessions},
    };
    let f = Fixture::new();
    f.hub.shutdown().await;
    let store = Store::open(&f.dir.join("test.db")).unwrap();
    let extra = sessions::create(
        &store.db.locked(),
        &sessions::NewSession {
            chat: "bot".into(),
            origin: SessionOrigin::Owner,
            source_session: None,
            parent_session: None,
            title: "Conversa extra".into(),
            runs_on: None,
        },
    )
    .unwrap()
    .id;
    let lane = crate::store::Lane { chat: "bot".into(), session: Some(extra.clone()) };
    let entry =
        store.insert_entry(&lane, EntryKind::User, 1, &json!({"text":"Só na sessão extra","status":"queued"})).unwrap();
    let principal = store
        .insert_entry(
            &crate::store::Lane::main("bot"),
            EntryKind::User,
            2,
            &json!({"text":"Principal posterior","status":"queued"}),
        )
        .unwrap();
    drop(store);

    let reopened = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| reopened.runtime("bot").status == BotStatus::Waiting).await;
    tokio::time::sleep(Duration::from_millis(700)).await;
    let queued = reopened.store.queued_user_entries_for_identity("bot").unwrap();
    assert_eq!(queued.len(), 2);
    assert_eq!(queued[0].id, entry.id);
    assert_eq!(queued[0].session_id, extra);
    assert_eq!(queued[0].data["status"], "queued");
    assert_eq!(queued[1].id, principal.id);
    assert_eq!(queued[1].session_id, "bot");
    assert_eq!(reopened.runtime("bot").lane.as_ref(), Some(&lane));
    let raw_extra: Option<String> = reopened
        .store
        .db
        .locked()
        .query_row("SELECT session FROM entries WHERE id=?1", [&entry.id], |r| r.get(0))
        .unwrap();
    let raw_main: Option<String> = reopened
        .store
        .db
        .locked()
        .query_row("SELECT session FROM entries WHERE id=?1", [&principal.id], |r| r.get(0))
        .unwrap();
    assert_eq!(raw_extra.as_deref(), Some(extra.as_str()));
    assert_eq!(raw_main, None, "a principal mantém SQL NULL");
    assert_eq!(f.prompts(), [] as [Value; 0], "extra não pode virar prompt na principal durante o boot");
    assert!(admission::current(&reopened.store, "bot").unwrap().is_none());
    assert!(reopened.runtime("bot").memory_turn_token.is_none());
    reopened.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn admissao_setup_apos_reserva_retoma_no_mesmo_hub_sem_duplicar_ou_ultrapassar_lote() {
    use crate::fork::bot_admission as admission;
    for failure in ["session-new", "compose"] {
        let f = Fixture::new();
        std::fs::write(f.dir.join("blocked"), "").unwrap();
        if failure == "session-new" {
            std::fs::write(f.dir.join("fail-new"), "").unwrap();
        } else {
            // memory.opened em compose ocorre depois do setup e antes de permit.
            f.hub.store.db.locked().execute_batch("CREATE TRIGGER reject_compose BEFORE INSERT ON kv WHEN NEW.k LIKE 'memory.opening.bot.%' BEGIN SELECT RAISE(ABORT,'composição indisponível'); END;").unwrap();
            let cfg = f.hub.store.bot("bot").unwrap().unwrap().config;
            f.hub
                .store
                .insert_entry(
                    &crate::store::Lane::main("bot"),
                    EntryKind::User,
                    1,
                    &json!({"text":"SENTINELA do resumo","status":"sent","contentOrigin":{"ingress":{"caller":"local"}}}),
                )
                .unwrap();
            crate::chat::memory::bind_profile_person(
                &f.hub.store,
                "bot",
                "sessao-anterior",
                Some(crate::chat::memory::profile::OWNER),
            )
            .unwrap();
            crate::chat::handoff::draft(&f.hub, &cfg, "sessao-anterior", crate::chat::handoff::Reason::Full).unwrap();
        }
        let first = crate::api::dispatch(
            &f.hub,
            &crate::api::devices::Caller::Local,
            "send",
            json!({"botId":"bot","text":"Primeiro lote reservado"}),
        )
        .await
        .unwrap();
        let first_id = first["entry"]["id"].as_str().unwrap();
        f.until(|| f.human_admission_pending() && (failure != "session-new" || f.dir.join("new-failed").exists()))
            .await;
        assert_eq!(f.prompts(), [] as [Value; 0]);
        let reserved = admission::current(&f.hub.store, "bot").unwrap().unwrap();
        assert_eq!(reserved.phase, admission::Phase::Reserved);
        assert_eq!(f.hub.store.entry(first_id).unwrap().data["status"], "queued");
        assert!(f.hub.store.kv_read("turn.waiting.bot").unwrap().is_none());
        if failure == "session-new" {
            assert!(f.dir.join("new-failed").exists(), "ACP falhou depois de reservar e antes de prompt");
        } else {
            let runtime = f.hub.runtime("bot");
            let session = runtime.session.expect("setup e binding concluídos antes da composição falhar");
            assert!(!runtime.memory_exposure_error);
            assert!(f.hub.store.kv_read(&format!("memory.opening.bot.{session}")).unwrap().is_none());
        }
        let second = crate::api::dispatch(
            &f.hub,
            &crate::api::devices::Caller::Local,
            "send",
            json!({"botId":"bot","text":"Segundo lote posterior"}),
        )
        .await
        .unwrap();
        let second_id = second["entry"]["id"].as_str().unwrap();
        assert_eq!(f.hub.store.entry(second_id).unwrap().data["status"], "queued");
        if failure == "session-new" {
            std::fs::remove_file(f.dir.join("fail-new")).unwrap();
        } else {
            f.hub.store.db.locked().execute_batch("DROP TRIGGER reject_compose").unwrap();
        }
        f.until(|| f.prompts().len() == 2 && f.hub.runtime("bot").status == BotStatus::Idle).await;
        let prompts = f.prompts();
        let first_prompt = prompts[0]["prompt"][0]["text"].as_str().unwrap();
        let second_prompt = prompts[1]["prompt"][0]["text"].as_str().unwrap();
        assert!(first_prompt.contains("Primeiro lote reservado") && !first_prompt.contains("Segundo lote posterior"));
        assert!(second_prompt.contains("Segundo lote posterior") && !second_prompt.contains("Primeiro lote reservado"));
        if failure == "compose" {
            assert!(first_prompt.contains("SENTINELA do resumo"), "a composição falha não consome o handoff");
            assert!(!second_prompt.contains("SENTINELA do resumo"), "handoff é entregue uma vez");
        }
        for id in [first_id, second_id] {
            assert_eq!(f.hub.store.entry(id).unwrap().data["status"], "sent");
            assert_eq!(f.hub.store.history("bot", i64::MAX, 100).unwrap().iter().filter(|e| e.id == id).count(), 1);
        }
        assert!(admission::current(&f.hub.store, "bot").unwrap().is_none());
        tokio::time::sleep(Duration::from_millis(700)).await;
        assert_eq!(f.prompts().len(), 2, "nenhum retry tardio duplica os prompts");
        f.hub.shutdown().await;
        let _ = std::fs::remove_dir_all(&f.dir);
    }
}

#[tokio::test]
async fn admissao_boot_isola_space_revogado_sem_perder_principal_nem_ordem_elegivel() {
    use crate::fork::{
        bot_admission as admission,
        sessoes::{origin::SessionOrigin, store as sessions},
    };
    let f = Fixture::new();
    f.hub.shutdown().await;
    std::fs::write(f.dir.join("blocked"), "").unwrap();
    f.hub.store.kv_set("team.settings.v1", r#"{"primaryBotId":"bot","permission":"ask"}"#).unwrap();
    let caller = crate::api::devices::Caller::Local;
    let spaces = crate::api::dispatch(&f.hub, &caller, "donnaSpaces", json!({"hubId":f.hub.host_id})).await.unwrap();
    let spaces = crate::api::dispatch(
        &f.hub,
        &caller,
        "donnaSpaceCreate",
        json!({"hubId":f.hub.host_id,
        "expectedRevision":spaces["revision"],"name":"Trabalho",
        "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}}}),
    )
    .await
    .unwrap();
    let space = spaces["spaces"][1]["id"].as_str().unwrap();
    let created = crate::api::dispatch(
        &f.hub,
        &caller,
        "donnaConversationCreate",
        json!({"hubId":f.hub.host_id,"spaceId":space,"botId":"bot"}),
    )
    .await
    .unwrap();
    let chat = created["conversation"]["id"].as_str().unwrap();
    let revoked = f
        .hub
        .store
        .insert_entry(
            &crate::store::Lane::main(chat),
            EntryKind::User,
            1,
            &json!({"text":"SENTINELA revogada","status":"queued"}),
        )
        .unwrap();
    f.hub.store.sem_donna_fixa(); // D366: exercita a troca/exclusão legada, sem Donna fixa.
    f.hub.store.kv_set("team.settings.v1", r#"{"permission":"ask"}"#).unwrap();
    let cfg = f.hub.store.bot("bot").unwrap().unwrap().config;
    assert!(
        crate::fork::spaces::execution_config(&f.hub.store, &cfg, chat)
            .unwrap_err()
            .to_string()
            .contains("cross_space_reference")
    );
    let principal = f
        .hub
        .store
        .insert_entry(
            &crate::store::Lane::main("bot"),
            EntryKind::User,
            2,
            &json!({"text":"Principal elegível","status":"queued"}),
        )
        .unwrap();
    let extra = sessions::create(
        &f.hub.store.db.locked(),
        &sessions::NewSession {
            chat: "bot".into(),
            origin: SessionOrigin::Owner,
            source_session: None,
            parent_session: None,
            title: "Extra em hold".into(),
            runs_on: None,
        },
    )
    .unwrap()
    .id;
    let extra_lane = crate::store::Lane { chat: "bot".into(), session: Some(extra) };
    let held = f
        .hub
        .store
        .insert_entry(
            &extra_lane,
            EntryKind::User,
            3,
            &json!({"text":"Extra anterior à última principal","status":"queued"}),
        )
        .unwrap();
    let later = f
        .hub
        .store
        .insert_entry(
            &crate::store::Lane::main("bot"),
            EntryKind::User,
            4,
            &json!({"text":"Principal atrás da extra","status":"queued"}),
        )
        .unwrap();
    let reopened = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| {
        reopened.store.entry(&principal.id).unwrap().data["status"] == "sent"
            && reopened.runtime("bot").status == BotStatus::Waiting
    })
    .await;
    // Inclui ao menos um retry da origem revogada, sem reiniciar o Hub novamente.
    tokio::time::sleep(Duration::from_secs(6)).await;
    let prompts = f.prompts();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0]["prompt"][0]["text"].as_str().unwrap().contains("Principal elegível"));
    assert!(!prompts[0].to_string().contains("SENTINELA revogada"));
    let entry = reopened.store.entry(&revoked.id).unwrap();
    assert_eq!(entry.bot_id, chat);
    assert_eq!(entry.session_id, chat, "SQL NULL da conversa Space não vira principal do bot");
    assert_eq!(entry.data["status"], "queued");
    assert_eq!(reopened.store.entry(&held.id).unwrap().data["status"], "queued");
    assert_eq!(reopened.store.entry(&later.id).unwrap().data["status"], "queued");
    assert_eq!(reopened.runtime("bot").lane.as_ref(), Some(&extra_lane));
    assert!(admission::current(&reopened.store, "bot").unwrap().is_none());
    reopened.shutdown().await;
    let mut actor = f.actor();
    actor.restore_waiting_queue().unwrap();
    actor.restore_waiting_queue().unwrap();
    assert_eq!(
        actor
            .queue
            .iter()
            .filter(|q| matches!(q,
        super::Queued::RestoreUser { entry_id, .. } if entry_id == &revoked.id))
            .count(),
        1
    );
    if let Some(super::Queued::RestoreUser { retry_at, .. }) = actor.queue.front_mut() {
        *retry_at = None;
    } else {
        panic!("origem revogada conserva consumidor e posição");
    }
    actor.retry_queue_restore();
    assert!(
        matches!(actor.queue.front(), Some(super::Queued::RestoreUser { retry_at: Some(_), .. })),
        "retry conservador revalida e mantém a origem revogada"
    );
    // Restaurar a referência permite reconstruir, sem despachar ou trocar chat.
    f.hub.store.kv_set("team.settings.v1", r#"{"primaryBotId":"bot","permission":"ask"}"#).unwrap();
    if let Some(super::Queued::RestoreUser { retry_at, .. }) = actor.queue.front_mut() {
        *retry_at = None;
    }
    actor.retry_queue_restore();
    assert!(matches!(actor.queue.front(), Some(super::Queued::User { lane, entry_id, .. })
        if lane.chat == chat && lane.session.is_none() && entry_id == &revoked.id));
    assert_eq!(f.prompts().len(), 1, "reconstruir não concede permissão nem envia");
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn admissao_binding_invalidado_apos_reserva_libera_so_reserva_e_segue_fila_no_mesmo_hub() {
    use crate::fork::{
        bot_admission as admission,
        engine_accounts::{Account, AuthorizedScope, Backend, Billing, LaunchReference, Model, Registry, State},
    };
    let f = Fixture::new();
    f.hub.shutdown().await;
    std::fs::write(f.dir.join("blocked"), "").unwrap();
    let account = format!("ac_{}", "1".repeat(64));
    Registry {
        salt: uuid::Uuid::new_v4().to_string(),
        accounts: vec![Account {
            managed: None,
            id: account.clone(),
            backend: Backend::Codex,
            billing: Billing::Unverified,
            state: State::Connected,
            launch: LaunchReference::CliProfile {
                executable: f.dir.join("nao-executar"),
                directory: f.dir.join("perfil-fixture"),
            },
            scope: AuthorizedScope::Owner,
            quota_identity: None,
            verified_at: None,
            subscription_proof: None,
            models: vec![Model { id: "fixture".into(), options: vec![] }],
            hidden_model_ids: Vec::new(),
        }],
    }
    .persist(&f.hub.store)
    .unwrap();
    let caller = crate::api::devices::Caller::Local;
    let space = crate::fork::spaces::PERSONAL;
    let created = crate::api::dispatch(
        &f.hub,
        &caller,
        "donnaConversationCreate",
        json!({"hubId":f.hub.host_id,"spaceId":space,"botId":"bot"}),
    )
    .await
    .unwrap();
    let chat = created["conversation"]["id"].as_str().unwrap();
    let selected = crate::api::dispatch(
        &f.hub,
        &caller,
        "donnaConversationUpdate",
        json!({
            "hubId":f.hub.host_id,"spaceId":space,"conversationId":chat,
            "expectedRevision":created["conversation"]["revision"],"engineAccountId":account,
            "expectedAccountRevision":1,"engineSelection":{"backend":"codex","model":"fixture","modelOptions":{}}
        }),
    )
    .await
    .unwrap();
    let original = f.hub.store.bot("bot").unwrap().unwrap().config;
    let contextual = crate::fork::spaces::execution_config(&f.hub.store, &original, chat).unwrap();
    let binding = crate::fork::spaces::execution_stamp(&f.hub.store, &contextual).unwrap();
    let lane = crate::store::Lane::main(chat);
    let invalid = f
        .hub
        .store
        .insert_entry(
            &lane,
            EntryKind::User,
            1,
            &json!({"text":"SENTINELA que não deve executar","status":"queued","executionBinding":binding}),
        )
        .unwrap();
    let first = f
        .hub
        .store
        .insert_entry(
            &crate::store::Lane::main("bot"),
            EntryKind::User,
            2,
            &json!({"text":"Válida primeira","status":"queued"}),
        )
        .unwrap();
    let second = f
        .hub
        .store
        .insert_entry(
            &crate::store::Lane::main("bot"),
            EntryKind::User,
            3,
            &json!({"text":"Válida segunda","status":"queued"}),
        )
        .unwrap();
    let mut actor = f.actor();
    actor.restore_waiting_queue().unwrap();
    let Some(super::Queued::User { lane: restored, entry_id, text }) = actor.queue.pop_front() else {
        panic!("binding válido deve permitir reconstruir o primeiro item");
    };
    assert_eq!(restored, lane);
    assert_eq!(entry_id, invalid.id);
    crate::fork::spaces::validate_entries(&f.hub, &contextual, std::slice::from_ref(&invalid.id)).unwrap();
    actor.cfg = contextual;
    actor.lane = lane.clone();
    actor.turn = Some(invalid.turn);
    actor.input = Some(
        serde_json::from_value(json!({"chat":chat,"entries":[invalid.id],"text":text,
        "hidden":false,"transient":false,"backend":"codex","contextBinding":binding}))
        .unwrap(),
    );
    f.hub.prepare_bot_turn("bot", None, |r| r.lane = Some(lane.clone())).unwrap();
    actor.reserve_dispatch().unwrap();
    let reserved = actor.dispatch.clone().unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    // Pausa estaciona o prompt; o hook muda a revisão após a última validação e antes de permit.
    admission::set_paused_for_test(&f.hub.store, "bot", true).unwrap();
    actor
        .send_admitted(
            json!({"sessionId":"nao-executar","prompt":[{"type":"text","text":text}]}),
            vec![invalid.id.clone()],
            "nao-executar".into(),
            &tx,
        )
        .await
        .unwrap();
    assert!(matches!(actor.pending_admission, Some(super::fork_admission::Pending::Prompt { .. })));
    let mutation_chat = chat.to_owned();
    actor.before_permit = Some(Box::new(move |hub| {
        hub.store
            .db
            .locked()
            .execute("UPDATE donna_space_conversations SET revision=revision+1 WHERE id=?1", [&mutation_chat])
            .unwrap();
    }));
    admission::set_paused_for_test(&f.hub.store, "bot", false).unwrap();
    actor.retry_admission(&tx).await;
    assert_eq!(
        f.hub.store.entry(&invalid.id).unwrap().data["status"],
        "queued",
        "mutação depois da última validação não pode ganhar sent na permissão"
    );
    assert_eq!(admission::current(&f.hub.store, "bot").unwrap().unwrap().phase, admission::Phase::Reserved);
    assert_eq!(f.prompts(), [] as [Value; 0]);
    actor.queue_retry_at = None;
    actor.retry_admission(&tx).await;
    assert_eq!(f.hub.store.entry(&invalid.id).unwrap().data["status"], "failed");
    assert!(actor.pending_admission.is_none() && actor.dispatch.is_none() && actor.turn.is_none());
    assert!(admission::current(&f.hub.store, "bot").unwrap().is_none(), "só a reserva capturada foi liberada");
    assert_eq!(f.prompts(), [] as [Value; 0]);
    actor.next_in_queue(&tx).await;
    let done = tokio::time::timeout(Duration::from_secs(20), rx.recv()).await.unwrap().unwrap();
    assert_ne!(done.ticket, reserved);
    actor.finish_turn(&done.result);
    let prompts = f.prompts();
    assert_eq!(prompts.len(), 1);
    let prompt = prompts[0]["prompt"][0]["text"].as_str().unwrap();
    assert!(prompt.find("Válida primeira").unwrap() < prompt.find("Válida segunda").unwrap());
    assert!(!prompt.contains("SENTINELA"));
    for id in [&first.id, &second.id] {
        assert_eq!(f.hub.store.entry(id).unwrap().data["status"], "sent");
    }
    // Mesmo se o binding anterior voltar, failed não ganha novo consumidor/prompt.
    f.hub
        .store
        .db
        .locked()
        .execute(
            "UPDATE donna_space_conversations SET revision=?2 WHERE id=?1",
            rusqlite::params![chat, selected["conversation"]["revision"].as_i64().unwrap()],
        )
        .unwrap();
    actor.restore_waiting_queue().unwrap();
    actor.retry_admission(&tx).await;
    actor.next_in_queue(&tx).await;
    assert_eq!(f.prompts().len(), 1);
    assert_eq!(f.hub.store.entry(&invalid.id).unwrap().data["status"], "failed");
    if let Some(conn) = actor.conn.take() {
        conn.acp.kill().await;
    }
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn admissao_enumera_novamente_apos_erro_inicial_mesmo_com_fila_do_ator_vazia() {
    let f = Fixture::new();
    f.hub.shutdown().await;
    std::fs::write(f.dir.join("blocked"), "").unwrap();
    let lane = crate::store::Lane::main("bot");
    let first = f
        .hub
        .store
        .insert_entry(&lane, EntryKind::User, 1, &json!({"text":"Primeira entrada do boot","status":"queued"}))
        .unwrap();
    let second = f
        .hub
        .store
        .insert_entry(&lane, EntryKind::User, 2, &json!({"text":"Segunda entrada do boot","status":"queued"}))
        .unwrap();
    let mut actor = f.actor();
    // Falha SQL real na consulta inicial, depois do startup do Hub. Não há item
    // RestoreUser: o consumidor precisa estar armado antes dessa enumeração.
    f.hub.store.db.locked().execute_batch("ALTER TABLE donna_space_conversations RENAME TO conversas_retidas").unwrap();
    assert!(actor.restore_waiting_queue().is_err());
    assert!(actor.queue.is_empty());
    assert!(actor.queue_restore_retry_at.is_some());
    let (tx, commands) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(actor.run(commands, None));
    // O recibo de um comando sem efeito comprova que run passou pelo restore
    // ainda com a consulta indisponível. Não dependemos de sleep para essa ordem.
    let (reply, answered) = tokio::sync::oneshot::channel();
    let autoridade = Box::new(crate::fork::autoridade::Autoridade::local());
    tx.send(super::Cmd::PrimaryNewSession { chat: "sinal-sem-efeito".into(), revision: 0, autoridade, reply }).unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(20), answered).await.unwrap().unwrap().is_err());
    assert_eq!(f.prompts(), [] as [Value; 0]);
    assert_eq!(f.hub.store.entry(&first.id).unwrap().data["status"], "queued");
    f.hub.store.db.locked().execute_batch("ALTER TABLE conversas_retidas RENAME TO donna_space_conversations").unwrap();
    f.until(|| f.prompts().len() == 1 && f.hub.runtime("bot").status == BotStatus::Idle).await;
    for id in [&first.id, &second.id] {
        assert_eq!(f.hub.store.entry(id).unwrap().data["status"], "sent");
    }
    let prompts = f.prompts();
    let text = prompts[0]["prompt"][0]["text"].as_str().unwrap();
    assert!(text.find("Primeira entrada do boot").unwrap() < text.find("Segunda entrada do boot").unwrap());
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(f.prompts().len(), 1, "retry de enumeração não duplica prompt");
    tx.send(super::Cmd::Shutdown).unwrap();
    tokio::time::timeout(Duration::from_secs(20), task).await.unwrap().unwrap();
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn admissao_retry_de_setup_descarta_humano_tornado_terminal_durante_a_espera() {
    use crate::fork::bot_admission as admission;
    let f = Fixture::new();
    std::fs::write(f.dir.join("blocked"), "").unwrap();
    std::fs::write(f.dir.join("fail-new"), "").unwrap();
    let first = crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId":"bot","text":"SENTINELA terminal durante setup"}),
    )
    .await
    .unwrap();
    let first_id = first["entry"]["id"].as_str().unwrap();
    f.until(|| f.human_admission_pending() && f.dir.join("new-failed").exists()).await;
    let reserved = admission::current(&f.hub.store, "bot").unwrap().unwrap();
    assert_eq!(reserved.phase, admission::Phase::Reserved);
    assert!(f.dir.join("new-failed").exists());
    let mut entry = f.hub.store.entry(first_id).unwrap();
    entry.data["status"] = json!("failed");
    f.hub.set_entry(first_id, &entry.data).unwrap();
    let following = crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId":"bot","text":"Válida após terminal do setup"}),
    )
    .await
    .unwrap();
    std::fs::remove_file(f.dir.join("fail-new")).unwrap();
    f.until(|| f.prompts().len() == 1 && f.hub.runtime("bot").status == BotStatus::Idle).await;
    assert_eq!(f.hub.store.entry(first_id).unwrap().data["status"], "failed");
    assert_eq!(f.hub.store.entry(following["entry"]["id"].as_str().unwrap()).unwrap().data["status"], "sent");
    let prompts = f.prompts();
    let text = prompts[0]["prompt"][0]["text"].as_str().unwrap();
    assert!(text.contains("Válida após terminal do setup") && !text.contains("SENTINELA"));
    assert!(admission::current(&f.hub.store, "bot").unwrap().is_none());
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(f.prompts().len(), 1);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn admissao_conta_muda_entre_validacao_e_commit_sem_sent_nem_prompt_e_nova_captura_passa() {
    use crate::fork::{
        bot_admission as admission,
        engine_accounts::{Account, AuthorizedScope, Backend, Billing, LaunchReference, Model, Registry, State},
    };
    let f = Fixture::new();
    f.hub.shutdown().await;
    std::fs::write(f.dir.join("blocked"), "").unwrap();
    let account = format!("ac_{}", "1".repeat(64));
    Registry {
        salt: uuid::Uuid::new_v4().to_string(),
        accounts: vec![Account {
            managed: None,
            id: account.clone(),
            backend: Backend::Codex,
            billing: Billing::Unverified,
            state: State::Connected,
            launch: LaunchReference::CliProfile {
                executable: f.dir.join("nao-executar"),
                directory: f.dir.join("perfil-fixture"),
            },
            scope: AuthorizedScope::Owner,
            quota_identity: None,
            verified_at: None,
            subscription_proof: None,
            models: vec![Model { id: "fixture".into(), options: vec![] }],
            hidden_model_ids: Vec::new(),
        }],
    }
    .persist(&f.hub.store)
    .unwrap();
    let caller = crate::api::devices::Caller::Local;
    let space = crate::fork::spaces::PERSONAL;
    let created = crate::api::dispatch(
        &f.hub,
        &caller,
        "donnaConversationCreate",
        json!({"hubId":f.hub.host_id,"spaceId":space,"botId":"bot"}),
    )
    .await
    .unwrap();
    let chat = created["conversation"]["id"].as_str().unwrap();
    let selected = crate::api::dispatch(
        &f.hub,
        &caller,
        "donnaConversationUpdate",
        json!({
            "hubId":f.hub.host_id,"spaceId":space,"conversationId":chat,
            "expectedRevision":created["conversation"]["revision"],"engineAccountId":account,
            "expectedAccountRevision":1,"engineSelection":{"backend":"codex","model":"fixture","modelOptions":{}}
        }),
    )
    .await
    .unwrap();
    let original = f.hub.store.bot("bot").unwrap().unwrap().config;
    let contextual = crate::fork::spaces::execution_config(&f.hub.store, &original, chat).unwrap();
    let binding = crate::fork::spaces::execution_stamp(&f.hub.store, &contextual).unwrap();

    let before = f.hub.store.space_conversation(space, chat).unwrap();
    let mut registry = Registry::load(&f.hub.store).unwrap();
    let mut other = registry.accounts[0].clone();
    other.id = format!("ac_{}", "2".repeat(64));
    let other_id = other.id.clone();
    registry.accounts.push(other);
    registry.persist(&f.hub.store).unwrap();
    // Prepara a seleção B pelo produtor real enquanto Idle. O commit dessa
    // seleção é atrasado pela fixture até depois da última validação do ator.
    crate::api::dispatch(
        &f.hub,
        &caller,
        "donnaConversationUpdate",
        json!({
            "hubId":f.hub.host_id,"spaceId":space,"conversationId":chat,
            "expectedRevision":selected["conversation"]["revision"],"engineAccountId":other_id,
            "expectedAccountRevision":1,"engineSelection":{"backend":"codex","model":"fixture","modelOptions":{}}
        }),
    )
    .await
    .unwrap();
    let after = f.hub.store.space_conversation(space, chat).unwrap();
    f.hub
        .store
        .db
        .locked()
        .execute(
            "UPDATE donna_space_conversations SET revision=?2,engine_account_id=?3,account_stamp=?4 WHERE id=?1",
            rusqlite::params![chat, before.revision, before.engine_account_id, before.account_stamp],
        )
        .unwrap();
    let lane = crate::store::Lane::main(chat);
    let entry = f
        .hub
        .store
        .insert_entry(
            &lane,
            EntryKind::User,
            1,
            &json!({"text":"SENTINELA da conta antiga","status":"queued","executionBinding":binding}),
        )
        .unwrap();
    let mut actor = f.actor();
    // ACP local da fixture já vivo: a recusa precisa ocorrer antes de qualquer
    // prompt; não depende de inexistência de processo ou de credencial real.
    // Como no fluxo real (#238), a sessão nasce com identidade preparada, não ativa.
    f.hub.prepare_bot_turn("bot", None, |r| r.lane = Some(actor.lane.clone())).unwrap();
    actor.ensure_session(None).await.unwrap();
    let sid = actor.turn_session.clone().unwrap();
    actor.cfg = contextual;
    actor.lane = lane.clone();
    // A sessão acima foi aberta para a principal do bot; o setup desta conversa emitiria o vínculo dela (pendente,
    // antes da ACP) e capturaria a revisão da conta selecionada (sem metadados gerenciados: 1). O encaixe do PR4
    // confere esse vínculo contra a sessão do turno.
    actor.turn_binding = Some(crate::fork::sessoes::bindings::issue(&f.hub.store, "bot", chat, chat, None).unwrap());
    actor.account_revision = Some(1);
    actor.turn = Some(entry.turn);
    actor.input = Some(
        serde_json::from_value(json!({"chat":chat,"entries":[entry.id],"text":"SENTINELA da conta antiga",
        "hidden":false,"transient":false,"backend":"codex","contextBinding":binding}))
        .unwrap(),
    );
    f.hub.prepare_bot_turn("bot", None, |r| r.lane = Some(lane.clone())).unwrap();
    actor.reserve_dispatch().unwrap();
    let ticket = actor.dispatch.clone().unwrap();
    let mutation_chat = chat.to_owned();
    actor.before_permit = Some(Box::new(move |hub| {
        let changed = hub.store.db.locked().execute("UPDATE donna_space_conversations SET revision=?2,engine_account_id=?3,account_stamp=?4 WHERE id=?1 AND revision=?5",
            rusqlite::params![mutation_chat,after.revision,after.engine_account_id,after.account_stamp,before.revision]).unwrap();
        assert_eq!(changed, 1);
    }));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    actor
        .send_admitted(
            json!({"sessionId":sid,"prompt":[{"type":"text","text":"SENTINELA da conta antiga"}]}),
            vec![entry.id.clone()],
            sid.clone(),
            &tx,
        )
        .await
        .unwrap();
    assert!(actor.before_permit.is_none(), "mutação executou na janela exata");
    assert_eq!(f.hub.store.entry(&entry.id).unwrap().data["status"], "queued");
    assert_eq!(admission::current(&f.hub.store, "bot").unwrap().unwrap().phase, admission::Phase::Reserved);
    assert_eq!(f.prompts(), [] as [Value; 0]);
    assert!(actor.conn.is_some());
    admission::release_reserved(&f.hub.store, "bot", &ticket).unwrap();
    actor.dispatch = None;
    actor.pending_admission = None;
    actor.queue_retry_at = None;
    let current = crate::fork::spaces::execution_config(&f.hub.store, &original, chat).unwrap();
    let current_binding = crate::fork::spaces::execution_stamp(&f.hub.store, &current).unwrap();
    let next = f
        .hub
        .store
        .insert_entry(
            &lane,
            EntryKind::User,
            2,
            &json!({"text":"Captura nova válida","status":"queued","executionBinding":current_binding}),
        )
        .unwrap();
    actor.cfg = current;
    // A troca de conta revogou o vínculo da conversa (gatilho `donna_bindings_space_account`); o setup do próximo
    // turno emitiria um novo, com a revisão da conta nova (também sem metadados gerenciados: 1).
    actor.turn_binding = Some(crate::fork::sessoes::bindings::issue(&f.hub.store, "bot", chat, chat, None).unwrap());
    actor.account_revision = Some(1);
    actor.turn = Some(next.turn);
    actor.input = Some(
        serde_json::from_value(json!({"chat":chat,"entries":[next.id],"text":"Captura nova válida",
        "hidden":false,"transient":false,"backend":"codex","contextBinding":current_binding}))
        .unwrap(),
    );
    f.hub.prepare_bot_turn("bot", None, |r| r.lane = Some(lane)).unwrap();
    actor.reserve_dispatch().unwrap();
    actor
        .send_admitted(
            json!({"sessionId":sid,"prompt":[{"type":"text","text":"Captura nova válida"}]}),
            vec![next.id.clone()],
            sid,
            &tx,
        )
        .await
        .unwrap();
    let done = tokio::time::timeout(Duration::from_secs(20), rx.recv()).await.unwrap().unwrap();
    assert!(actor.accepts_completion(&done.ticket));
    assert!(done.result.is_ok());
    assert_eq!(f.hub.store.entry(&next.id).unwrap().data["status"], "sent");
    assert_eq!(f.hub.store.entry(&entry.id).unwrap().data["status"], "queued");
    assert_eq!(f.prompts().len(), 1);
    assert!(!f.prompts()[0].to_string().contains("SENTINELA"));
    actor.finish_turn(&done.result);
    if let Some(conn) = actor.conn.take() {
        conn.acp.kill().await;
    }
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn contrato_token_inicial_e_retomada_sent_apos_reinicio_usam_identidade_original_com_attempt_novo() {
    use crate::fork::bot_admission as admission;
    let f = Fixture::new();
    f.hub.shutdown().await;
    std::fs::write(f.dir.join("limit-hold"), "").unwrap();
    let lane = crate::store::Lane::main("bot");
    let entry = f
        .hub
        .store
        .insert_entry(&lane, EntryKind::User, 1, &json!({"text":"Contrato do token","status":"queued"}))
        .unwrap();
    let mut actor = f.actor();
    let checked = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let check = checked.clone();
    let id = entry.id.clone();
    actor.before_permit = Some(Box::new(move |hub| {
        let r = hub.runtime("bot");
        let record = admission::current(&hub.store, "bot").unwrap().unwrap();
        assert_eq!(record.phase, admission::Phase::Reserved);
        assert_eq!(r.preparing_turn_token.as_deref(), Some(record.execution_token.as_str()));
        assert!(r.memory_turn_token.is_none());
        assert_eq!(r.status, BotStatus::Waiting);
        assert_eq!(hub.store.entry(&id).unwrap().data["status"], "queued");
        assert!(hub.store.kv_read("turn.waiting.bot").unwrap().is_none(), "sem espera artificial");
        assert!(hub.with_inactive_bot("bot", || Ok(())).is_err(), "Undo permanece bloqueado no preparo");
        check.store(true, std::sync::atomic::Ordering::SeqCst);
    }));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    actor.start_turn(lane, std::slice::from_ref(&entry.id), "Contrato do token", false, &tx).await.unwrap();
    f.until(|| f.prompts().len() == 1).await;
    assert!(checked.load(std::sync::atomic::Ordering::SeqCst));
    let initial = admission::current(&f.hub.store, "bot").unwrap().unwrap();
    assert_eq!(initial.phase, admission::Phase::Permitted);
    assert_eq!(f.hub.store.entry(&entry.id).unwrap().data["status"], "sent");
    assert_eq!(f.hub.runtime("bot").memory_turn_token.as_deref(), Some(initial.execution_token.as_str()));
    assert!(f.hub.runtime("bot").preparing_turn_token.is_none());
    assert!(f.hub.store.kv_read("turn.waiting.bot").unwrap().is_none());
    std::fs::remove_file(f.dir.join("limit-hold")).unwrap();
    let done = tokio::time::timeout(Duration::from_secs(20), rx.recv()).await.unwrap().unwrap();
    actor.finish_turn(&done.result);
    let waiting: Value = serde_json::from_str(&f.hub.store.kv_read("turn.waiting.bot").unwrap().unwrap()).unwrap();
    assert_eq!(waiting["executionToken"], initial.execution_token);
    assert_eq!(waiting["phase"], "waiting");
    if let Some(conn) = actor.conn.take() {
        conn.acp.kill().await;
    }
    drop(actor);
    f.make_retry_due();
    std::fs::write(f.dir.join("hold"), "").unwrap();
    let reopened = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| f.prompts().len() == 2 && reopened.runtime("bot").status == BotStatus::Working).await;
    let resumed = admission::current(&reopened.store, "bot").unwrap().unwrap();
    let checkpoint: Value =
        serde_json::from_str(&reopened.store.kv_read("turn.waiting.bot").unwrap().unwrap()).unwrap();
    assert_eq!(checkpoint["phase"], "retrying");
    assert_eq!(checkpoint["executionToken"], initial.execution_token);
    assert_eq!(resumed.execution_token, initial.execution_token);
    assert_ne!(resumed.attempt, initial.attempt);
    assert_eq!(reopened.runtime("bot").memory_turn_token.as_deref(), Some(initial.execution_token.as_str()));
    assert_eq!(reopened.store.entry(&entry.id).unwrap().data["status"], "sent");
    std::fs::remove_file(f.dir.join("hold")).unwrap();
    f.until(|| reopened.runtime("bot").status == BotStatus::Idle).await;
    assert_eq!(f.prompts().len(), 2);
    reopened.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn legado_sent_sem_token_preserva_checkpoint_e_nao_fabrica_retomada() {
    let f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    let mut checkpoint: Value =
        serde_json::from_str(&f.hub.store.kv_read("turn.waiting.bot").unwrap().unwrap()).unwrap();
    checkpoint.as_object_mut().unwrap().remove("executionToken");
    checkpoint["retryAt"] = json!(0);
    f.hub.store.kv_set("turn.waiting.bot", &checkpoint.to_string()).unwrap();
    let reopened = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| reopened.runtime("bot").status == BotStatus::Error).await;
    tokio::time::sleep(Duration::from_millis(700)).await;
    let saved: Value = serde_json::from_str(&reopened.store.kv_read("turn.waiting.bot").unwrap().unwrap()).unwrap();
    assert_eq!(saved["phase"], "paused");
    assert!(saved["executionToken"].is_null());
    assert_eq!(f.prompts().len(), 1);
    assert!(reopened.runtime("bot").memory_turn_token.is_none());
    assert!(reopened.runtime("bot").preparing_turn_token.is_none());
    reopened.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn memoria_compartilhada_composta_revogada_antes_de_permit_nao_sai_nem_sobrevive_reconfigure() {
    use crate::fork::{
        bot_admission as admission,
        engine_accounts::{Account, AuthorizedScope, Backend, Billing, LaunchReference, Model, Registry, State},
    };
    let f = Fixture::new();
    f.hub.shutdown().await;
    std::fs::write(f.dir.join("blocked"), "").unwrap();
    let mut profile = f.hub.store.bot("bot").unwrap().unwrap().config;
    profile.shared_memory = Some(true);
    f.hub.store.save_bot(&profile).unwrap();
    f.hub
        .store
        .memory_write(|c, now, rev| {
            codync_mem::store::add(
                c,
                codync_mem::store::Writer::User,
                codync_mem::store::New {
                    bot: None,
                    text: "SENTINELA de memória compartilhada",
                    room: None,
                    evidence: &[],
                },
                now,
                rev,
            )
        })
        .unwrap();
    let account = format!("ac_{}", "1".repeat(64));
    Registry {
        salt: uuid::Uuid::new_v4().to_string(),
        accounts: vec![Account {
            managed: None,
            id: account.clone(),
            backend: Backend::Codex,
            billing: Billing::Unverified,
            state: State::Connected,
            launch: LaunchReference::CliProfile {
                executable: f.dir.join("nao-executar"),
                directory: f.dir.join("perfil-fixture"),
            },
            scope: AuthorizedScope::Owner,
            quota_identity: None,
            verified_at: None,
            subscription_proof: None,
            models: vec![Model { id: "fixture".into(), options: vec![] }],
            hidden_model_ids: Vec::new(),
        }],
    }
    .persist(&f.hub.store)
    .unwrap();
    let caller = crate::api::devices::Caller::Local;
    let space = crate::fork::spaces::PERSONAL;
    let created = crate::api::dispatch(
        &f.hub,
        &caller,
        "donnaConversationCreate",
        json!({"hubId":f.hub.host_id,"spaceId":space,"botId":"bot"}),
    )
    .await
    .unwrap();
    let chat = created["conversation"]["id"].as_str().unwrap();
    crate::api::dispatch(
        &f.hub,
        &caller,
        "donnaConversationUpdate",
        json!({
            "hubId":f.hub.host_id,"spaceId":space,"conversationId":chat,
            "expectedRevision":created["conversation"]["revision"],"engineAccountId":account,
            "expectedAccountRevision":1,"engineSelection":{"backend":"codex","model":"fixture","modelOptions":{}}
        }),
    )
    .await
    .unwrap();
    let original = f.hub.store.bot("bot").unwrap().unwrap().config;
    let contextual = crate::fork::spaces::execution_config(&f.hub.store, &original, chat).unwrap();
    let binding = crate::fork::spaces::execution_stamp(&f.hub.store, &contextual).unwrap();

    let lane = crate::store::Lane::main(chat);
    let entry = f
        .hub
        .store
        .insert_entry(
            &lane,
            EntryKind::User,
            1,
            &json!({"text":"Pedido com memória","status":"queued","executionBinding":binding}),
        )
        .unwrap();
    let mut actor = f.actor();
    // Como no fluxo real (#238), a sessão nasce com identidade preparada, não ativa.
    f.hub.prepare_bot_turn("bot", None, |r| r.lane = Some(actor.lane.clone())).unwrap();
    actor.ensure_session(None).await.unwrap();
    let sid = actor.turn_session.clone().unwrap();
    actor.cfg = contextual;
    actor.lane = lane.clone();
    actor.turn = Some(entry.turn);
    actor.session_fresh = true;
    actor.input = Some(
        serde_json::from_value(json!({"chat":chat,"entries":[entry.id],"text":"Pedido com memória",
        "hidden":false,"transient":false,"backend":"codex","contextBinding":binding}))
        .unwrap(),
    );
    f.hub.prepare_bot_turn("bot", None, |r| r.lane = Some(lane)).unwrap();
    actor.reserve_dispatch().unwrap();
    let prompt = actor.compose_reserved(&sid, "Pedido com memória").await.unwrap();
    assert!(
        prompt.contains("SENTINELA de memória compartilhada"),
        "abertura realmente composta com a camada compartilhada"
    );
    actor.before_permit = Some(Box::new(|hub| {
        let mut cfg = hub.store.bot("bot").unwrap().unwrap().config;
        cfg.shared_memory = Some(false);
        hub.store.save_bot(&cfg).unwrap();
    }));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    actor
        .send_admitted(
            json!({"sessionId":sid,"prompt":[{"type":"text","text":prompt}]}),
            vec![entry.id.clone()],
            sid.clone(),
            &tx,
        )
        .await
        .unwrap();
    assert_eq!(admission::current(&f.hub.store, "bot").unwrap().unwrap().phase, admission::Phase::Reserved);
    assert_eq!(f.hub.store.entry(&entry.id).unwrap().data["status"], "queued");
    assert_eq!(f.prompts(), [] as [Value; 0]);
    let current = f.hub.store.bot("bot").unwrap().unwrap().config;
    actor.on_cmd(super::Cmd::Reconfigure(Box::new(current.clone())), &tx).await;
    actor.queue_retry_at = None;
    actor.retry_admission(&tx).await;
    assert!(actor.pending_admission.is_none() && actor.dispatch.is_none() && actor.turn.is_none());
    assert_eq!(f.hub.store.entry(&entry.id).unwrap().data["status"], "failed");
    assert_eq!(f.prompts(), [] as [Value; 0]);
    let snapshot =
        crate::chat::context::resolve(&f.hub.store, &current, &sid, super::Instructions::FirstMessage).unwrap();
    assert!(!snapshot.system.contains("SENTINELA de memória compartilhada"), "cache não recompõe política antiga");
    let next = f
        .hub
        .store
        .insert_entry(
            &crate::store::Lane::main("bot"),
            EntryKind::User,
            2,
            &json!({"text":"Válida após revogação","status":"queued"}),
        )
        .unwrap();
    actor
        .on_cmd(
            super::Cmd::Send {
                lane: crate::store::Lane::main("bot"),
                entry_id: next.id.clone(),
                text: "Válida após revogação".into(),
            },
            &tx,
        )
        .await;
    let done = tokio::time::timeout(Duration::from_secs(20), rx.recv()).await.unwrap().unwrap();
    actor.finish_turn(&done.result);
    assert_eq!(f.hub.store.entry(&next.id).unwrap().data["status"], "sent");
    assert_eq!(f.prompts().len(), 1);
    assert!(!f.prompts()[0].to_string().contains("SENTINELA de memória compartilhada"));
    if let Some(conn) = actor.conn.take() {
        conn.acp.kill().await;
    }
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn memoria_nativa_efetiva_revogada_prende_preparo_ao_snapshot_e_reinicia_motor_antes_da_valida() {
    use crate::fork::bot_admission as admission;
    let f = Fixture::with_backend("copilot");
    f.hub.shutdown().await;
    std::fs::write(f.dir.join("blocked"), "").unwrap();
    let mut cfg = f.hub.store.bot("bot").unwrap().unwrap().config;
    cfg.harness_memory = true;
    cfg.shared_memory = Some(false);
    f.hub.store.save_bot(&cfg).unwrap();
    let entry = f
        .hub
        .store
        .insert_entry(
            &crate::store::Lane::main("bot"),
            EntryKind::User,
            1,
            &json!({"text":"SENTINELA do preparo nativo","status":"queued"}),
        )
        .unwrap();
    let mut actor = f.actor();
    actor.before_permit = Some(Box::new(|hub| {
        let mut cfg = hub.store.bot("bot").unwrap().unwrap().config;
        assert!(crate::agent::own_memory::kept(&cfg));
        cfg.harness_memory = false;
        hub.store.save_bot(&cfg).unwrap();
    }));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    actor
        .start_turn(
            crate::store::Lane::main("bot"),
            std::slice::from_ref(&entry.id),
            "SENTINELA do preparo nativo",
            false,
            &tx,
        )
        .await
        .unwrap();
    assert!(crate::agent::own_memory::kept(&actor.cfg));
    let policy: Value = serde_json::from_str(&f.hub.store.kv_read("turn.inflight.bot").unwrap().unwrap()).unwrap();
    assert_eq!(policy["memoryPolicy"]["harness"], true);
    assert_eq!(policy["phase"], "reserved");
    assert_eq!(f.prompts(), [] as [Value; 0]);
    assert_eq!(f.hub.store.entry(&entry.id).unwrap().data["status"], "queued");
    let old_acp = actor.conn.as_ref().unwrap().acp.clone();
    let current = f.hub.store.bot("bot").unwrap().unwrap().config;
    actor.on_cmd(super::Cmd::Reconfigure(Box::new(current)), &tx).await;
    actor.queue_retry_at = None;
    actor.retry_admission(&tx).await;
    assert!(actor.pending_admission.is_none() && actor.dispatch.is_none() && actor.turn.is_none());
    assert_eq!(f.hub.store.entry(&entry.id).unwrap().data["status"], "failed");
    assert_eq!(f.prompts(), [] as [Value; 0]);
    let next = f
        .hub
        .store
        .insert_entry(
            &crate::store::Lane::main("bot"),
            EntryKind::User,
            2,
            &json!({"text":"Válida com memória nativa desligada","status":"queued"}),
        )
        .unwrap();
    actor
        .on_cmd(
            super::Cmd::Send {
                lane: crate::store::Lane::main("bot"),
                entry_id: next.id.clone(),
                text: "Válida com memória nativa desligada".into(),
            },
            &tx,
        )
        .await;
    let done = tokio::time::timeout(Duration::from_secs(20), rx.recv()).await.unwrap().unwrap();
    assert!(!Arc::ptr_eq(&old_acp, &actor.conn.as_ref().unwrap().acp), "política de processo realmente reaplicada");
    assert!(!crate::agent::own_memory::kept(&actor.cfg));
    actor.finish_turn(&done.result);
    assert_eq!(f.hub.store.entry(&next.id).unwrap().data["status"], "sent");
    assert_eq!(f.prompts().len(), 1);
    assert!(!f.prompts()[0].to_string().contains("SENTINELA"));
    assert!(admission::current(&f.hub.store, "bot").unwrap().is_none());
    if let Some(conn) = actor.conn.take() {
        conn.acp.kill().await;
    }
    let _ = std::fs::remove_dir_all(&f.dir);
}

// D246: o turno normal nunca passou por wait_on_limit, mas sua conclusão também é durável.
#[tokio::test]
async fn conclusao_normal_com_job_admite_atomico_e_recupera_sem_prompt_nem_espera_humana() {
    use super::waiting::TurnInput;
    use crate::fork::memory_auto::queue;
    use crate::store::{EntryKind, Lane};
    for outcome in ["success", "failed", "cancelled"] {
        let f = Fixture::with_backend("claude");
        let mut actor = f.actor();
        let lane = Lane::main("bot");
        let user = f
            .hub
            .add_entry(&lane, EntryKind::User, 1, &json!({"text":"Prefiro café sem açúcar.","status":"sent"}))
            .unwrap();
        let reply = f
            .hub
            .add_entry(&lane, EntryKind::Agent, 1, &json!({"text":"Registrado no turno.","author":"bot","final":false}))
            .unwrap();
        f.hub.store.memory_session_start("bot", "normal-session", false).unwrap();
        f.hub.store.memory_session_turn("bot", "bot", "normal-session", 1).unwrap();
        f.hub.store.memory_turn_begin("bot", "bot", "normal-session", &[1], "owner").unwrap();
        f.hub.set_runtime("bot", |runtime| {
            runtime.status = BotStatus::Working;
            runtime.message = Some(user.id.clone());
            runtime.session = Some("normal-session".into());
            runtime.lane = Some(lane.clone());
            runtime.memory_turn_token = Some(uuid::Uuid::new_v4().to_string());
        });
        queue::capture_engine(&f.hub, &actor.cfg, &lane, "claude").unwrap();
        queue::begin(&f.hub, &actor.cfg, &lane, "normal-session", std::slice::from_ref(&user.id)).unwrap();
        let job = queue::active_job(&f.hub, "bot").unwrap().unwrap();
        actor.turn = Some(1);
        actor.turn_session = Some("normal-session".into());
        actor.last_text = Some(reply.id);
        actor.input = Some(TurnInput {
            chat: "bot".into(),
            entries: vec![user.id.clone()],
            text: "Prefiro café sem açúcar.".into(),
            hidden: false,
            handoff: None,
            routine: None,
            transient: false,
            backend: actor.cfg.backend.clone(),
            model: actor.cfg.model.clone(),
            command: actor.cfg.command.clone(),
            session: Some("normal-session".into()),
            context_binding: None,
            visible_session: None,
        });
        assert!(actor.waiting.is_none());
        let done = match outcome {
            "success" => Ok(json!({"stopReason":"end_turn"})),
            "cancelled" => Ok(json!({"stopReason":"cancelled"})),
            _ => Err(anyhow::anyhow!("falha normal controlada")),
        };
        let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
        db.execute_batch(
            "CREATE TRIGGER reject_normal_completion BEFORE INSERT ON kv WHEN NEW.k='turn.waiting.bot'
            BEGIN SELECT RAISE(FAIL,'checkpoint indisponível'); END;",
        )
        .unwrap();
        assert!(actor.checkpoint_completion(&done).is_err());
        assert!(f.hub.store.kv_get("turn.waiting.bot").is_none());
        let running: String =
            db.query_row("SELECT outcome FROM donna_memory_auto_jobs WHERE id=?1", [&job], |row| row.get(0)).unwrap();
        assert_eq!(running, "running", "falha de checkpoint não admite sucesso ou resultado parcial");
        db.execute_batch("DROP TRIGGER reject_normal_completion").unwrap();
        actor.checkpoint_completion(&done).unwrap();
        let raw = f.hub.store.kv_get("turn.waiting.bot").unwrap();
        let checkpoint: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(checkpoint["phase"], "completing");
        assert_eq!(checkpoint["completion"]["memory_job"], job);
        assert_eq!(checkpoint["input"]["entries"], json!([user.id]));
        assert!(checkpoint["notice"].is_null(), "Completing direto não cria cartão de espera ou permissão");
        let proof: String =
            db.query_row("SELECT outcome FROM donna_memory_auto_jobs WHERE id=?1", [&job], |row| row.get(0)).unwrap();
        assert_eq!(proof, outcome);
        // Retomar o Completing não precisa do token efêmero nem executa session/prompt outra vez.
        f.hub.set_runtime("bot", |runtime| runtime.memory_turn_token = None);
        let mut restored = f.actor();
        assert!(restored.restore_waiting());
        let persisted = restored.finishing.take().unwrap();
        restored.checkpoint_completion(&persisted).unwrap();
        restored.finish_turn(&persisted);
        assert!(f.hub.store.kv_get("turn.waiting.bot").is_none());
        assert_eq!(f.prompts().len(), 0);
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM donna_memory_auto_jobs WHERE id=?1", [&job], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT outcome FROM donna_memory_auto_jobs WHERE id=?1", [&job], |row| row
                .get::<_, String>(0))
                .unwrap(),
            outcome
        );
        assert!(
            !f.hub
                .store
                .history("bot", i64::MAX, 100)
                .unwrap()
                .iter()
                .any(|entry| entry.data["subscriptionWait"].is_object())
        );
        f.hub.shutdown().await;
        let _ = std::fs::remove_dir_all(&f.dir);
    }
}

#[tokio::test]
async fn falha_antes_do_prompt_fecha_coleta_mas_conserva_humano_com_retry_limitado_mesmo_com_rollback() {
    use crate::fork::memory_auto::queue;
    use crate::store::{EntryKind, Lane};
    for fail_checkpoint in [false, true] {
        let f = Fixture::with_backend("claude");
        std::fs::write(f.dir.join("blocked"), "").unwrap();
        let mut actor = f.actor();
        let lane = Lane::main("bot");
        let text = "Prefiro café sem açúcar.";
        let user = f.hub.add_entry(&lane, EntryKind::User, 1, &json!({"text":text,"status":"queued",
            "contentOrigin":{"status":"appReported","segments":[{"kind":"typed","start":0,"end":text.encode_utf16().count()}]}})).unwrap();
        let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
        db.execute_batch(
            "CREATE TRIGGER reject_opening BEFORE INSERT ON kv WHEN NEW.k LIKE 'memory.opening.%'
            BEGIN SELECT RAISE(FAIL,'opening indisponível'); END;",
        )
        .unwrap();
        let (done, mut replies) = tokio::sync::mpsc::unbounded_channel();
        let error = actor.start_turn(lane, std::slice::from_ref(&user.id), text, false, &done).await.unwrap_err();
        assert!(format!("{error:#}").contains("opening indisponível"), "{error:#}");
        assert!(actor.waiting.is_none(), "a falha ocorreu antes de qualquer espera ou conclusão");
        let job = queue::active_job(&f.hub, "bot").unwrap().unwrap();
        let token = f.hub.runtime("bot").preparing_turn_token.unwrap();
        assert_eq!(f.prompts().len(), 0, "compose falhou antes de session/prompt");
        let revision = f.hub.store.memory_rev();
        if fail_checkpoint {
            db.execute_batch(
                "CREATE TRIGGER reject_failure_audit BEFORE INSERT ON donna_memory_auto_non_saved
                BEGIN SELECT RAISE(FAIL,'auditoria indisponível'); END;",
            )
            .unwrap();
        }
        let mut events = f.hub.events.subscribe();
        actor.start_failed(&error);
        if fail_checkpoint {
            assert_eq!(
                db.query_row("SELECT outcome FROM donna_memory_auto_jobs WHERE id=?1", [&job], |r| r
                    .get::<_, String>(0))
                    .unwrap(),
                "running"
            );
            assert_eq!(f.hub.store.memory_rev(), revision, "rollback não publica auditoria parcial");
            assert!(f.hub.store.kv_get("turn.waiting.bot").is_none());
            assert_eq!(f.hub.runtime("bot").preparing_turn_token.as_deref(), Some(token.as_str()));
            assert!(f.hub.runtime("bot").memory_turn_token.is_none());
            assert!(actor.turn.is_some());
            assert!(actor.pending_admission.is_some(), "falha SQL conserva setup sem liberar outro preparo");
            assert!(actor.finishing.is_none());
            // P1 publica a espera do mesmo pedido (Waiting); invalidação de auditoria
            // seria evento com memoryRev novo, e o rollback não pode emiti-la.
            // A Torre (#264) também projeta essa mudança de estado, sem dado de memória.
            while let Ok(event) = events.try_recv() {
                if event["type"] == "donnaControlTowerChanged" {
                    continue;
                }
                assert_eq!(event["type"], "bot", "rollback não emite invalidação de auditoria");
                assert_eq!(event["bot"]["memoryRev"], revision, "rollback não emite invalidação de auditoria");
            }
            db.execute_batch("DROP TRIGGER reject_failure_audit").unwrap();
            actor.queue_retry_at = None;
            actor.retry_admission(&done).await;
        }
        assert!(actor.turn.is_some() && actor.pending_admission.is_some());
        assert_eq!(f.hub.store.entry(&user.id).unwrap().data["status"], "queued");
        assert_eq!(f.hub.runtime("bot").preparing_turn_token.as_deref(), Some(token.as_str()));
        assert!(f.hub.runtime("bot").memory_turn_token.is_none());
        assert!(f.hub.store.memory_rev() > revision);
        assert_eq!(
            db.query_row("SELECT outcome||'/'||status FROM donna_memory_auto_jobs WHERE id=?1", [&job], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "failed/ineligible"
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM donna_memory_auto_non_saved WHERE job=?1 AND reason='ineligible'",
                [&job],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM donna_memory_auto_proposals WHERE job=?1", [&job], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(f.hub.store.memory_read(|db| codync_mem::store::layer(db, Some("bot"))).unwrap().len(), 0);
        let mut invalidated = false;
        while let Ok(event) = events.try_recv() {
            invalidated |= event["type"] == "bot"
                && event["bot"]["id"] == "bot"
                && event["bot"]["memoryRev"].as_i64().is_some_and(|rev| rev > revision);
        }
        assert!(invalidated, "ocorrência confirmada invalida memoryRev no evento existente");
        assert!(f.hub.store.kv_get("turn.waiting.bot").is_none());
        assert!(
            f.hub.store.kv_get("memory.revision.bot").is_none(),
            "auditoria não anuncia memória aprendida ao motor"
        );
        assert!(
            !f.hub
                .store
                .history("bot", i64::MAX, 100)
                .unwrap()
                .iter()
                .any(|entry| entry.data["memory"].is_object() || entry.data["subscriptionWait"].is_object())
        );
        assert!(queue::reserve(&f.hub.store, Some("bot")).unwrap().is_none());
        let after = f.hub.store.memory_rev();
        actor.start_failed(&error);
        assert_eq!(f.hub.store.memory_rev(), after, "repetir falha não duplica auditoria");
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM donna_memory_auto_non_saved WHERE job=?1", [&job], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        db.execute_batch("DROP TRIGGER reject_opening").unwrap();
        actor.queue_retry_at = None;
        actor.retry_admission(&done).await;
        let reply = tokio::time::timeout(Duration::from_secs(20), replies.recv()).await.unwrap().unwrap();
        assert!(actor.accepts_completion(&reply.ticket));
        assert_eq!(reply.result.as_ref().unwrap()["stopReason"], "end_turn");
        actor.finish_turn(&reply.result);
        assert_eq!(f.prompts().len(), 1, "o mesmo humano pode prosseguir após preparo bem-sucedido");
        assert_eq!(f.hub.store.entry(&user.id).unwrap().data["status"], "sent");
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM donna_memory_auto_jobs WHERE bot='bot'", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT outcome||'/'||status FROM donna_memory_auto_jobs WHERE id=?1", [&job], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "failed/ineligible"
        );
        assert!(queue::reserve(&f.hub.store, Some("bot")).unwrap().is_none());
        assert_eq!(f.hub.store.memory_read(|db| codync_mem::store::layer(db, Some("bot"))).unwrap().len(), 0);
        assert!(replies.try_recv().is_err());
        if let Some(conn) = actor.conn.take() {
            conn.acp.kill().await;
        }
        f.hub.shutdown().await;
        let _ = std::fs::remove_dir_all(&f.dir);
    }
}

#[tokio::test]
async fn tres_turnos_contextuais_acordam_memoria_sem_entregar_texto_ao_nomeador_pessoal() {
    use super::waiting::TurnInput;
    use crate::api::{devices::Caller, dispatch};
    use crate::chat::memory::DreamEvent;
    use crate::fork::memory_auto::queue;
    use crate::store::{EntryKind, Lane};
    let f = Fixture::with_backend("claude");
    let mut original = f.hub.store.bot("bot").unwrap().unwrap().config;
    original.auto_name = true;
    f.hub.store.save_bot(&original).unwrap();
    f.hub.store.kv_set("team.settings.v1", r#"{"primaryBotId":"bot","permission":"ask"}"#).unwrap();
    let account = format!("ac_{}", "c".repeat(64));
    f.hub
        .store
        .kv_set(
            "fork.engine-accounts.v1",
            &json!({"salt":uuid::Uuid::new_v4().to_string(),"accounts":[{
                "id":account,"backend":"claude","billing":"subscription","state":"connected",
                "launch":{"kind":"cli_profile","executable":"/fixture/not-executed","directory":f.dir},
                "scope":"owner","quotaIdentity":account,"verifiedAt":1,
                "subscriptionProof":{"method":"claude_auth_status","plan":"max"},"models":[{"id":"haiku","options":[]}]
            }]})
            .to_string(),
        )
        .unwrap();
    let spaces = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaSpaceCreate",
        json!({
            "hubId":f.hub.host_id,"expectedRevision":f.hub.store.spaces().unwrap().revision,"name":"Space da fronteira",
            "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"SP"}}
        }),
    )
    .await
    .unwrap();
    let space = spaces["spaces"][1]["id"].as_str().unwrap();
    let created = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaConversationCreate",
        json!({
            "hubId":f.hub.host_id,"spaceId":space,"botId":"bot","title":"Conversa isolada"
        }),
    )
    .await
    .unwrap();
    let conversation = &created["conversation"];
    let chat = conversation["id"].as_str().unwrap();
    dispatch(
        &f.hub,
        &Caller::Local,
        "donnaConversationUpdate",
        json!({
            "hubId":f.hub.host_id,"spaceId":space,"conversationId":chat,"expectedRevision":conversation["revision"],
            "engineAccountId":account,"expectedAccountRevision":1,
            "engineSelection":{"backend":"claude","model":"haiku","modelOptions":{}}
        }),
    )
    .await
    .unwrap();
    let cfg = crate::fork::spaces::execution_config(&f.hub.store, &original, chat).unwrap();
    assert!(cfg.context.is_some() && cfg.auto_name);
    assert_ne!(cfg.space_id, crate::fork::spaces::PERSONAL);
    assert_eq!(f.hub.store.bot("bot").unwrap().unwrap().config.space_id, crate::fork::spaces::PERSONAL);
    let binding = crate::fork::spaces::execution_stamp(&f.hub.store, &cfg).unwrap();
    let mut actor = f.actor();
    actor.cfg = cfg;
    let (dreamer, mut events) = tokio::sync::mpsc::unbounded_channel();
    actor.dreamer = dreamer;
    let lane = Lane::main(chat);
    let sentinel = format!("SPACE_NAMING_SENTINEL_{}", uuid::Uuid::new_v4());
    let session = "context-naming-fixture";
    f.hub.store.memory_session_start("bot", session, false).unwrap();
    for turn in 1..=3 {
        let text = format!("{sentinel} turno {turn}");
        let user = f.hub.add_entry(&lane, EntryKind::User, turn, &json!({"text":text,"status":"sent",
            "contentOrigin":{"status":"appReported","segments":[{"kind":"typed","start":0,"end":text.encode_utf16().count()}]}})).unwrap();
        let reply = f
            .hub
            .add_entry(
                &lane,
                EntryKind::Agent,
                turn,
                &json!({"text":"Resposta contextual.","author":"bot","final":false}),
            )
            .unwrap();
        f.hub.store.memory_session_turn("bot", chat, session, turn).unwrap();
        f.hub.store.memory_turn_begin("bot", chat, session, &[turn], "owner").unwrap();
        f.hub.set_runtime("bot", |r| {
            r.status = BotStatus::Working;
            r.lane = Some(lane.clone());
            r.message = Some(user.id.clone());
            r.session = Some(session.into());
            r.memory_turn_token = Some(uuid::Uuid::new_v4().to_string());
        });
        queue::capture_engine(&f.hub, &actor.cfg, &lane, "claude").unwrap();
        queue::begin(&f.hub, &actor.cfg, &lane, session, std::slice::from_ref(&user.id)).unwrap();
        let job = queue::active_job(&f.hub, "bot").unwrap().unwrap();
        actor.lane = lane.clone();
        actor.turn = Some(turn);
        actor.turn_session = Some(session.into());
        actor.last_text = Some(reply.id);
        actor.turn_text = Some(text.clone());
        actor.turn_entries = vec![user.id.clone()];
        actor.input = Some(TurnInput {
            chat: chat.into(),
            entries: vec![user.id],
            text,
            hidden: false,
            handoff: None,
            routine: None,
            transient: false,
            backend: actor.cfg.backend.clone(),
            model: actor.cfg.model.clone(),
            command: actor.cfg.command.clone(),
            session: Some(session.into()),
            context_binding: Some(binding.clone()),
            visible_session: None,
        });
        actor.finish_turn(&Ok(json!({"stopReason":"end_turn"})));
        assert!(actor.turn.is_none());
        assert!(
            matches!(events.try_recv().unwrap(), DreamEvent::Wake),
            "memória acorda em cada sucesso, inclusive no primeiro"
        );
        assert!(
            events.try_recv().is_err(),
            "nenhum Turn chega ao consumidor Namer/global após {turn} sucessos contextuais"
        );
        let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
        assert_eq!(
            db.query_row("SELECT outcome FROM donna_memory_auto_jobs WHERE id=?1", [&job], |r| r.get::<_, String>(0))
                .unwrap(),
            "success"
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM kv WHERE k LIKE 'naming.turns.%' AND instr(v,?1)>0", [&sentinel], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    assert!(f.hub.store.kv_get("naming.turns.bot").is_none());
    // Controle do mesmo gancho: o Pessoal continua entregando os três turnos ao nomeador existente.
    actor.cfg = original;
    actor.lane = Lane::main("bot");
    actor.turn_session = None;
    actor.turn_entries.clear();
    for turn in 1..=3 {
        let text = format!("PERSONAL_NAMING_CONTROL_{turn}");
        let reply = f
            .hub
            .add_entry(
                &actor.lane,
                EntryKind::Agent,
                turn,
                &json!({"text":"Resposta pessoal.","author":"bot","final":false}),
            )
            .unwrap();
        actor.turn = Some(turn);
        actor.last_text = Some(reply.id);
        actor.turn_text = Some(text.clone());
        actor.finish_turn(&Ok(json!({"stopReason":"end_turn"})));
        let DreamEvent::Turn(observed) = events.try_recv().unwrap() else {
            panic!("controle pessoal deve chegar ao Namer")
        };
        assert_eq!(observed.user, text);
        assert_eq!(observed.agent, "Resposta pessoal.");
        assert!(!observed.user.contains(&sentinel));
        assert!(events.try_recv().is_err());
    }
    assert_eq!(f.prompts().len(), 0, "a fronteira é conferida antes de qualquer processo de nomeação");
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn preparo_humano_duas_falhas_conservam_ordem_e_restart_encerra_terceira_sem_quarta() {
    use crate::fork::bot_admission as admission;
    use crate::store::Lane;
    let f = Fixture::with_backend("claude");
    let mut actor = f.actor();
    let lane = Lane::main("bot");
    let first =
        f.hub.add_entry(&lane, EntryKind::User, 1, &json!({"text":"Primeiro pedido","status":"queued"})).unwrap();
    let later =
        f.hub.add_entry(&lane, EntryKind::User, 2, &json!({"text":"Pedido posterior","status":"queued"})).unwrap();
    f.hub.store.db.locked().execute_batch("CREATE TRIGGER reject_opening_limit BEFORE INSERT ON kv WHEN NEW.k LIKE 'memory.opening.%' BEGIN SELECT RAISE(ABORT,'preparo indisponível'); END;").unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let error =
        actor.start_turn(lane, std::slice::from_ref(&first.id), "Primeiro pedido", false, &tx).await.unwrap_err();
    assert!(format!("{error:#}").contains("preparo indisponível"));
    actor.start_failed(&error);
    let original = admission::current(&f.hub.store, "bot").unwrap().unwrap();
    assert_eq!(original.setup_attempts, 1);
    assert_eq!(original.entry_ids, std::slice::from_ref(&first.id));
    actor.queue_retry_at = None;
    actor.retry_admission(&tx).await;
    let twice = admission::current(&f.hub.store, "bot").unwrap().unwrap();
    assert_eq!(twice.setup_attempts, 2);
    assert_eq!(twice.ticket(), original.ticket());
    assert_eq!(twice.entry_ids, original.entry_ids);
    for entry in [&first, &later] {
        let saved = f.hub.store.entry(&entry.id).unwrap();
        assert_eq!(saved.seq, entry.seq);
        assert_eq!(saved.data["status"], "queued");
    }
    assert_eq!(f.prompts(), [] as [Value; 0]);
    if let Some(conn) = actor.conn.take() {
        conn.acp.kill().await;
    }
    drop(actor);
    let mut restored = f.actor();
    restored.restore_dispatch().unwrap();
    // A terceira falha marca o pedido, mas o aviso não pode ser perdido numa
    // publicação parcial. A cerca precisa sobreviver até o aviso ficar durável.
    f.hub.store.db.locked().execute_batch("CREATE TRIGGER reject_setup_notice BEFORE INSERT ON entries WHEN NEW.id LIKE 'setup-failed-%' BEGIN SELECT RAISE(ABORT,'aviso indisponível'); END;").unwrap();
    restored.queue_retry_at = None;
    // A sessão criada antes é carregada após o restart; a falha injetada (abertura
    // de memória da sessão) só se repete se o preparo ainda entregar a abertura.
    restored.session_fresh = true;
    restored.retry_admission(&tx).await;
    let third = admission::current(&f.hub.store, "bot").unwrap().unwrap();
    assert_eq!(third.setup_attempts, 3);
    assert_eq!(third.ticket(), original.ticket());
    assert_eq!(f.hub.store.entry(&first.id).unwrap().data["status"], "failed");
    assert_eq!(f.hub.store.entry(&later.id).unwrap().data["status"], "queued");
    assert!(restored.pending_admission.is_some());
    assert_eq!(f.prompts(), [] as [Value; 0]);
    if let Some(conn) = restored.conn.take() {
        conn.acp.kill().await;
    }
    drop(restored);
    f.hub.store.db.locked().execute_batch("DROP TRIGGER reject_setup_notice").unwrap();
    let mut terminal = f.actor();
    terminal.restore_dispatch().unwrap();
    terminal.retry_admission(&tx).await;
    assert!(admission::current(&f.hub.store, "bot").unwrap().is_none());
    assert!(terminal.pending_admission.is_none() && terminal.turn.is_none());
    assert_eq!(f.hub.runtime("bot").status, BotStatus::Error);
    let notice = f.hub.store.entry(&format!("setup-failed-{}", original.attempt)).unwrap();
    assert_eq!(notice.data["style"], "error");
    assert!(notice.data["text"].as_str().unwrap().contains("três tentativas"));
    assert!(f.hub.store.entry(&first.id).unwrap().data["error"].as_str().unwrap().contains("reenvie"));
    // Restart perdeu o erro em memória: motivo genérico, só código e motor (sem mensagem crua).
    let expected = json!({"code":"setup_failed","backend":"claude"});
    assert_eq!(notice.data["reason"], expected);
    assert_eq!(f.hub.store.entry(&first.id).unwrap().data["reason"], expected);
    assert_eq!(f.hub.store.entry(&later.id).unwrap().data["status"], "queued");
    assert!(f.prompts().is_empty(), "nem recuperação da publicação abre quarta tentativa");
    assert!(rx.try_recv().is_err());
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn motivo_do_setup_cita_a_lane_do_turno_nao_a_cfg_da_lane_anterior() {
    use crate::{
        api::{devices::Caller, dispatch},
        store::Lane,
    };
    // Revisão do PR: `self.cfg` só troca depois do preparo validar. Citá-la no
    // motivo apontaria a conta/motor da lane ANTERIOR; o certo é a candidata
    // desta lane (resolução do Space + seleção efetiva), aqui um Space sem
    // conta escolhida.
    let f = Fixture::with_backend("codex");
    let mut actor = f.actor();
    actor.cfg.fork_editor.engine_account_id = Some("ac_lane_anterior".into());
    actor.cfg.backend = "claude".into();
    f.hub.store.kv_set("team.settings.v1", r#"{"primaryBotId":"bot","permission":"ask"}"#).unwrap();
    let spaces = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaSpaceCreate",
        json!({"hubId":f.hub.host_id,"expectedRevision":f.hub.store.spaces().unwrap().revision,"name":"Trabalho",
        "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}}}),
    )
    .await
    .unwrap();
    let space = spaces["spaces"][1]["id"].as_str().unwrap();
    let created = dispatch(
        &f.hub,
        &Caller::Local,
        "donnaConversationCreate",
        json!({"hubId":f.hub.host_id,"spaceId":space,"botId":"bot"}),
    )
    .await
    .unwrap();
    let chat = created["conversation"]["id"].as_str().unwrap();
    let lane = Lane::main(chat);
    actor.lane = lane.clone();
    let entry = f.hub.add_entry(&lane, EntryKind::User, 1, &json!({"text":"Pedido","status":"queued"})).unwrap();
    actor.input = Some(
        serde_json::from_value(
            json!({"chat":chat,"entries":[entry.id],"text":"Pedido","hidden":false,"transient":false,"backend":"claude"}),
        )
        .unwrap(),
    );
    f.hub.prepare_bot_turn("bot", None, |r| r.lane = Some(lane.clone())).unwrap();
    actor.reserve_dispatch().unwrap();
    actor.defer_unpermitted_human(&anyhow::anyhow!("account_unavailable: x")).unwrap();
    let reason = actor.setup_reason.as_ref().unwrap();
    assert_eq!(reason.code, "account_unavailable");
    assert_eq!(reason.account_id, None, "Space sem conta escolhida não pode citar a conta da lane anterior");
    assert_eq!(reason.backend.as_deref(), Some("codex"), "o motor é o da candidata, não o da cfg antiga");
    drop(actor);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn retry_humano_permitido_nao_reabre_coleta_cancelada() {
    use crate::fork::{bot_admission as admission, memory_auto::queue};
    use crate::store::Lane;
    let f = Fixture::with_backend("claude");
    std::fs::write(f.dir.join("blocked"), "").unwrap();
    let mut actor = f.actor();
    let user = f
        .hub
        .add_entry(&Lane::main("bot"), EntryKind::User, 1, &json!({"text":"Pedido original","status":"queued"}))
        .unwrap();
    f.hub.store.db.locked().execute_batch("CREATE TRIGGER reject_cancel_opening BEFORE INSERT ON kv WHEN NEW.k LIKE 'memory.opening.%' BEGIN SELECT RAISE(ABORT,'preparo falhou'); END;").unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let error = actor
        .start_turn(Lane::main("bot"), std::slice::from_ref(&user.id), "Pedido original", false, &tx)
        .await
        .unwrap_err();
    let job = queue::active_job(&f.hub, "bot").unwrap().unwrap();
    let original = admission::current(&f.hub.store, "bot").unwrap().unwrap();
    // Produtor canônico de conclusão da coleta, sem concluir/consumir o humano.
    let key = admission::key("bot");
    let raw = f.hub.store.kv_read(&key).unwrap().unwrap();
    queue::checkpoint(&f.hub.store, &key, &raw, Some(&job), false, true).unwrap();
    actor.start_failed(&error);
    assert_eq!(f.hub.store.entry(&user.id).unwrap().data["status"], "queued");
    f.hub.store.db.locked().execute_batch("DROP TRIGGER reject_cancel_opening").unwrap();
    actor.queue_retry_at = None;
    actor.retry_admission(&tx).await;
    let permitted = admission::current(&f.hub.store, "bot").unwrap().unwrap();
    assert_eq!(permitted.ticket(), original.ticket());
    assert_eq!(permitted.phase, admission::Phase::Permitted);
    let reply = tokio::time::timeout(Duration::from_secs(20), rx.recv()).await.unwrap().unwrap();
    assert!(actor.accepts_completion(&reply.ticket));
    assert_eq!(reply.result.as_ref().unwrap()["stopReason"], "end_turn");
    actor.finish_turn(&reply.result);
    {
        let db = f.hub.store.db.locked();
        assert_eq!(
            db.query_row("SELECT outcome||'/'||status FROM donna_memory_auto_jobs WHERE id=?1", [&job], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "cancelled/ineligible"
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM donna_memory_auto_jobs WHERE bot='bot'", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM donna_memory_auto_proposals WHERE job=?1", [&job], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    assert_eq!(f.prompts().len(), 1);
    assert!(queue::reserve(&f.hub.store, Some("bot")).unwrap().is_none());
    if let Some(conn) = actor.conn.take() {
        conn.acp.kill().await;
    }
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// PR4 (DON-833): uma extra que o permit nunca admitiria é recusada no envio, sem entrada nem admissão pendente, e
/// o ator real continua atendendo a principal (antes, o pedido pendente travava a fila do bot inteiro).
#[tokio::test]
async fn extra_recusada_no_envio_nao_trava_a_principal() {
    let f = Fixture::new();
    let extra = {
        let c = f.hub.store.db.locked();
        let new = crate::fork::sessoes::store::NewSession {
            chat: "bot".into(),
            origin: crate::fork::sessoes::origin::SessionOrigin::Owner,
            source_session: None,
            parent_session: None,
            title: String::new(),
            runs_on: None,
        };
        crate::fork::sessoes::store::create(&c, &new).unwrap().id
    };
    let before = f.hub.store.last_seq("bot");
    let error = crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId":"bot","text":"Pedido para a extra","sessionId":extra}),
    )
    .await
    .unwrap_err();
    assert!(format!("{error:#}").contains("session_selection_unavailable"), "{error:#}");
    assert_eq!(f.hub.store.last_seq("bot"), before, "nenhuma entrada gravada");
    assert!(crate::fork::bot_admission::current(&f.hub.store, "bot").unwrap().is_none());
    // Controle positivo: a principal chega ao motor pelo ator real.
    f.send().await;
    let prompts = f.prompts();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].to_string().contains("Continue a tarefa pendente"));
    assert!(!prompts[0].to_string().contains("Pedido para a extra"));
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[path = "../../fork/teto/tests.rs"]
mod teto_tests;
#[path = "../../fork/usage_limits/tests.rs"]
mod usage_limits_tests;

// Sessões PR5 (R1-04/R2): a conclusão durável produzida por `checkpoint_completion` sobrevive a encerrar a extra ou
// recomeçar a principal e, depois de reiniciar, é publicada na mesma entrada (final=true), sem prompt novo.
// A (com job de memória): buffer e conclusão são gravados juntos por desenho (`memory_auto::checkpoint` aplica o
//   trecho na mesma Tx); prova a conclusão durável sem trecho pendente.
// B (sem job): a espera canônica do limite da assinatura (`wait_on_limit`) e o trecho só no buffer (`Seg::Open`);
//   a publicação falha depois do checkpoint (trigger de teste na entrada) e os bytes do buffer voltam depois do
//   reinício. Sem registro de admissão, essa espera nasce pausada (identidade não provada): o teste só recupera os
//   dados já produzidos da conclusão — nenhuma retomada, IA ou autorização nova.
#[tokio::test]
async fn sessoes_pr5_conclusao_duravel_sobrevive_a_encerrar_e_recomecar_e_publica_sem_prompt() {
    use super::waiting::TurnInput;
    use crate::fork::autoridade::Autoridade;
    use crate::fork::memory_auto::queue;
    use crate::fork::sessoes::{origin::SessionOrigin, store as sessions};
    use crate::store::{EntryKind, Lane};
    const BUFFER: &str = "RESPOSTA-PARCIAL-E-O-RESTO-SO-NO-BUFFER";
    for (extra, with_job) in [(true, true), (false, true), (true, false), (false, false)] {
        let f = Fixture::with_backend("claude");
        let session = extra.then(|| {
            let new = sessions::NewSession {
                chat: "bot".into(),
                origin: SessionOrigin::Owner,
                source_session: None,
                parent_session: None,
                title: String::new(),
                runs_on: None,
            };
            sessions::create(&f.hub.store.db.locked(), &new).unwrap().id
        });
        let lane = Lane { chat: "bot".into(), session: session.clone() };
        let mut actor = f.actor();
        let user = f
            .hub
            .add_entry(&lane, EntryKind::User, 1, &json!({"text":"Prefiro café sem açúcar.","status":"sent"}))
            .unwrap();
        let reply = f
            .hub
            .add_entry(&lane, EntryKind::Agent, 1, &json!({"text":"RESPOSTA-PARC","author":"bot","final":false}))
            .unwrap();
        f.hub.store.memory_session_start("bot", "pr5-session", false).unwrap();
        f.hub.store.memory_session_turn("bot", "bot", "pr5-session", 1).unwrap();
        f.hub.store.memory_turn_begin("bot", "bot", "pr5-session", &[1], "owner").unwrap();
        f.hub.set_runtime("bot", |runtime| {
            runtime.status = BotStatus::Working;
            runtime.message = Some(user.id.clone());
            runtime.session = Some("pr5-session".into());
            runtime.lane = Some(lane.clone());
            runtime.memory_turn_token = Some(uuid::Uuid::new_v4().to_string());
        });
        if with_job {
            queue::capture_engine(&f.hub, &actor.cfg, &lane, "claude").unwrap();
            queue::begin(&f.hub, &actor.cfg, &lane, "pr5-session", std::slice::from_ref(&user.id)).unwrap();
        }
        let job = queue::active_job(&f.hub, "bot").unwrap();
        assert_eq!(job.is_some(), with_job, "A tem job de memória; B não tem");
        actor.lane = lane.clone();
        actor.turn_session = Some("pr5-session".into());
        actor.input = Some(TurnInput {
            chat: "bot".into(),
            entries: vec![user.id.clone()],
            text: "Prefiro café sem açúcar.".into(),
            hidden: false,
            handoff: None,
            routine: None,
            transient: false,
            backend: actor.cfg.backend.clone(),
            model: actor.cfg.model.clone(),
            command: actor.cfg.command.clone(),
            session: Some("pr5-session".into()),
            context_binding: None,
            visible_session: session.clone(),
        });
        let db = rusqlite::Connection::open(f.dir.join("test.db")).unwrap();
        if !with_job {
            // B: a espera canônica do limite (produtor `wait_on_limit`); o turno seguinte termina com o trecho no buffer.
            assert!(actor.wait_on_limit(crate::agent::subscription::Limit { resets_at: None }));
            actor.seg = super::Seg::Open {
                kind: super::SegKind::Text,
                entry_id: reply.id.clone(),
                buf: BUFFER.into(),
                flushed: std::time::Instant::now(),
                dirty: true,
                solta: false,
            };
            // Falha de I/O controlada depois de gravar o checkpoint e antes de publicar: a entrada recusa a atualização.
            db.execute_batch(&format!(
                "CREATE TRIGGER reject_pr5_publish BEFORE UPDATE ON entries WHEN OLD.id='{}'
                BEGIN SELECT RAISE(FAIL,'publicação indisponível'); END;",
                reply.id
            ))
            .unwrap();
        }
        actor.turn = Some(1);
        actor.last_text = Some(reply.id.clone());
        // Produtor real da conclusão durável. A queda acontece logo depois.
        let done = Ok(json!({"stopReason":"end_turn"}));
        let produced = actor.checkpoint_completion(&done);
        assert_eq!(produced.is_err(), !with_job, "B: a publicação falhou depois do checkpoint");
        let raw = f.hub.store.kv_get("turn.waiting.bot").unwrap();
        let saved: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(saved["phase"], "completing");
        assert_eq!(saved["completion"]["memory_job"], json!(job));
        let expected = if with_job { "RESPOSTA-PARC" } else { BUFFER };
        if !with_job {
            assert_eq!(saved["completion"]["stream"][1]["text"], BUFFER, "o buffer está no checkpoint");
        }
        let pending = f.hub.store.entry(&reply.id).unwrap();
        assert_eq!(
            (pending.data["text"].as_str(), pending.data["final"].as_bool()),
            (Some("RESPOSTA-PARC"), Some(false))
        );
        drop(actor);
        // Depois da queda: o ator restaura a conclusão e o dono encerra a extra (ou recomeça a principal).
        let mut actor = f.actor();
        assert!(actor.restore_waiting());
        if let Some(id) = &session {
            let revision = sessions::get(&f.hub.store.db.locked(), id).unwrap().unwrap().revision;
            actor.session_close(id, revision, &Autoridade::local()).await.unwrap();
        } else {
            let space = crate::fork::spaces::chat_space(&f.hub, "bot").unwrap();
            let revision = f.hub.store.space_conversation(&space, "bot").unwrap().revision;
            actor.primary_new_session("bot", revision, &Autoridade::local()).await.unwrap();
        }
        let raw = f.hub.store.kv_get("turn.waiting.bot").unwrap();
        assert_eq!(serde_json::from_str::<Value>(&raw).unwrap()["phase"], "completing", "não vira tombstone");
        assert!(actor.waiting_completing());
        drop(actor);
        if !with_job {
            db.execute_batch("DROP TRIGGER reject_pr5_publish").unwrap();
        }
        // Reiniciar de novo depois do commit: a conclusão é publicada, sem IA.
        let mut restored = f.actor();
        assert!(restored.restore_waiting());
        let persisted = restored.finishing.take().unwrap();
        restored.checkpoint_completion(&persisted).unwrap();
        restored.finish_turn(&persisted);
        let published = f.hub.store.entry(&reply.id).unwrap();
        assert_eq!(published.data["text"], expected, "os bytes da conclusão, na mesma entrada");
        assert_eq!(published.data["final"], true, "a mesma entrada vira a resposta final");
        assert!(f.hub.store.kv_get("turn.waiting.bot").is_none());
        assert_eq!(f.prompts().len(), 0, "nenhum prompt novo");
        f.hub.shutdown().await;
        let _ = std::fs::remove_dir_all(&f.dir);
    }
}

// DON-907 R1-01/R1-02 (D70): "Mandar agora" consumido pelo lote real (`next_in_queue`) com o motor de teste.
mod mandar_agora {
    use super::*;
    use crate::agent::bot::fork_fila::Acao;
    use crate::fork::autoridade::Autoridade;

    fn fila(f: &Fixture, actor: &mut super::super::Actor, textos: &[&str]) -> Vec<String> {
        std::fs::write(f.dir.join("blocked"), "").unwrap();
        let lane = crate::store::Lane::main("bot");
        textos
            .iter()
            .map(|text| {
                let entry = f
                    .hub
                    .store
                    .insert_entry(&lane, EntryKind::User, 1, &json!({"text": text, "status": "queued"}))
                    .unwrap();
                actor.queue.push_back(super::super::Queued::User {
                    lane: lane.clone(),
                    entry_id: entry.id.clone(),
                    text: (*text).into(),
                });
                entry.id
            })
            .collect()
    }

    async fn acao(actor: &mut super::super::Actor, id: &str, acao: Acao) {
        let (done, _rx) = tokio::sync::mpsc::unbounded_channel();
        actor.queued_control(id.to_owned(), acao, &Autoridade::local(), &done).await.unwrap();
    }

    /// Cada lote que o motor recebeu, só com os textos da fila.
    async fn lotes(f: &Fixture, actor: &mut super::super::Actor, turnos: usize) -> Vec<Vec<String>> {
        let (done, mut rx) = tokio::sync::mpsc::unbounded_channel();
        for _ in 0..turnos {
            actor.next_in_queue(&done).await;
            let completed = tokio::time::timeout(Duration::from_secs(20), rx.recv())
                .await
                .expect("o turno deve produzir uma conclusão")
                .expect("o canal de conclusão deve permanecer aberto");
            actor.finish_turn(&completed.result);
        }
        if let Some(conn) = actor.conn.take() {
            conn.acp.kill().await;
        }
        f.prompts()
            .iter()
            .map(|p| {
                let text = p["prompt"].to_string();
                ["Alfa", "Bravo", "Charlie"].iter().filter(|t| text.contains(*t)).map(|t| (*t).to_owned()).collect()
            })
            .collect()
    }

    /// O fim do turno interrompido (o motor confirma o cancelamento).
    fn interrompida(actor: &mut super::super::Actor) {
        actor.finish_turn(&Ok(json!({"stopReason": "cancelled"})));
        actor.turn = None;
    }

    #[tokio::test]
    async fn reordenar_depois_de_mandar_agora_nao_junta_a_marcada_ao_lote() {
        let f = Fixture::new();
        f.hub.shutdown().await;
        let mut actor = f.actor();
        let ids = fila(&f, &mut actor, &["Alfa", "Charlie", "Bravo"]);
        let (a, c) = (ids[0].clone(), ids[1].clone());
        actor.lane = crate::store::Lane::main("bot");
        actor.turn = Some(1);
        acao(&mut actor, &c, Acao::SendNow).await;
        let rev = f.hub.store.entry(&a).unwrap().rev;
        acao(&mut actor, &a, Acao::Move { position: 0, expected: rev }).await;
        interrompida(&mut actor);
        assert_eq!(lotes(&f, &mut actor, 3).await, [vec!["Alfa"], vec!["Charlie"], vec!["Bravo"]]);
        let _ = std::fs::remove_dir_all(&f.dir);
    }

    #[tokio::test]
    async fn mandar_agora_em_duas_mensagens_mantem_as_duas_separadas() {
        let f = Fixture::new();
        f.hub.shutdown().await;
        let mut actor = f.actor();
        let ids = fila(&f, &mut actor, &["Alfa", "Bravo", "Charlie"]);
        actor.lane = crate::store::Lane::main("bot");
        actor.turn = Some(1);
        acao(&mut actor, &ids[2], Acao::SendNow).await;
        acao(&mut actor, &ids[1], Acao::SendNow).await;
        interrompida(&mut actor);
        assert_eq!(lotes(&f, &mut actor, 3).await, [vec!["Bravo"], vec!["Charlie"], vec!["Alfa"]]);
        let _ = std::fs::remove_dir_all(&f.dir);
    }

    #[tokio::test]
    async fn reiniciar_depois_de_mandar_agora_mantem_a_frente_e_o_envio_separado() {
        let f = Fixture::new();
        f.hub.shutdown().await;
        let mut actor = f.actor();
        let ids = fila(&f, &mut actor, &["Alfa", "Bravo", "Charlie"]);
        // Outra conversa responde: a escolhida só passa para a frente e o Hub reinicia antes do despacho.
        actor.lane = crate::store::Lane { chat: "bot".into(), session: Some("outra".into()) };
        actor.turn = Some(1);
        acao(&mut actor, &ids[2], Acao::SendNow).await;
        drop(actor);
        let mut boot = f.actor();
        boot.restore_waiting_queue().unwrap();
        assert_eq!(lotes(&f, &mut boot, 2).await, [vec!["Charlie"], vec!["Alfa", "Bravo"]]);
        let _ = std::fs::remove_dir_all(&f.dir);
    }
}

#[path = "fork_pausa_conversa_tests.rs"]
mod pausa_conversa;

/// Espera D72 numa conversa pausada por efeito incerto: a retomada passa por `start_turn` sem conferir a pausa e o
/// permit recusa. A reserva sai sem despacho, a continuação não é enviada e a outra conversa do mesmo bot (uma sala)
/// é admitida; a pausa continua.
#[tokio::test]
async fn retomada_da_espera_em_conversa_pausada_nao_segura_outra_conversa() {
    use crate::fork::bot_admission as admission;
    let mut f = Fixture::new();
    f.send().await;
    let room: BotConfig =
        serde_json::from_value(json!({"id":"sala","name":"Sala","kind":"group","members":["bot"]})).unwrap();
    f.hub.store.save_bot(&room).unwrap();
    f.hub.shutdown().await;
    f.make_retry_due();
    admission::pause_conversation(&Store::open(&f.dir.join("test.db")).unwrap(), "bot", "bot", "t-velho").unwrap();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    let waiting_phase = || {
        f.hub
            .store
            .kv_get("turn.waiting.bot")
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .map(|w| w["phase"].clone())
    };
    f.until(|| waiting_phase().is_none_or(|phase| phase == "finished")).await;
    assert_eq!(f.hub.runtime("bot").status, BotStatus::Idle, "a retenção não deixa o bot em erro");
    let history = f.hub.store.history("bot", i64::MAX, 200).unwrap();
    assert!(
        !history.iter().any(|e| e.kind == "notice" && e.data["style"] == "error"),
        "a retenção não publica aviso de erro"
    );
    assert_eq!(f.prompts().len(), 1, "a continuação da conversa pausada não sai");
    crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId":"sala", "text":"Oi, sala"}),
    )
    .await
    .unwrap();
    f.until(|| f.prompts().len() == 2).await;
    let prompt = f.prompts()[1].to_string();
    assert!(prompt.contains("Oi, sala"), "a sala é admitida");
    assert!(!prompt.contains("Continue a tarefa pendente"), "a continuação da conversa pausada não sai");
    assert!(admission::conversation_paused(&f.hub.store, "bot", "bot").unwrap(), "a pausa continua");
    f.until(|| f.hub.runtime("bot").status != BotStatus::Waiting).await;
    assert!(
        f.hub
            .store
            .kv_get("turn.waiting.bot")
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .is_none_or(|w| w["phase"] != "waiting"),
        "a espera da conversa pausada não fica armada"
    );
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[path = "fork_pergunta_restaurada_tests.rs"]
mod pergunta_restaurada;
