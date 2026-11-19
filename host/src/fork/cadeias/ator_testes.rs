#![allow(clippy::unwrap_used)]
//! D70 pelo ator real: falha seletiva de Interrupted não pode virar Failed.
//! Compilado como filho de `agent::bot`; sem processo de motor ou conta real.
use super::*;
use crate::{LockExt, remote::identity::Identity, store::Store};
use std::path::{Path, PathBuf};

struct Fixture {
    hub: Arc<Hub>,
    dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("donna-cadeia-ator-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(Path::new(":memory:")).unwrap();
        let cfg: BotConfig = serde_json::from_value(json!({
            "id":"bot","name":"Bot sintético","backend":"custom","command":"false","cwd":dir,"notify":false
        }))
        .unwrap();
        store.save_bot(&cfg).unwrap();
        store
            .kv_set(
                "routines.v1",
                &json!({"routines":[],"runs":[{
                    "id":"run-legada","routineId":"rotina-legada","botId":"bot","status":"running",
                    "createdAt":1,"finishedAt":null,"detail":null,"event":{"source":"schedule"},
                    "deliveryId":null,"startedAt":1
                }]})
                .to_string(),
            )
            .unwrap();
        let hub = Hub::new(store, "teste".into(), Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
        hub.routines.restore(&hub.store).unwrap();
        Self { hub, dir }
    }

    fn actor(&self) -> Actor {
        let (dreamer, _) = mpsc::unbounded_channel();
        Actor {
            hub: self.hub.clone(),
            cfg: self.hub.store.bot("bot").unwrap().unwrap().config,
            conn: None,
            account_revision: None,
            session_id: None,
            run_sessions: HashMap::new(),
            lane: Lane::main("bot"),
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
            waiting: None,
            turn_limit: None,
            fallback: fork_fallback::Turno::default(),
            finishing: None,
            queue: VecDeque::new(),
            queue_retry_at: None,
            queue_restore_retry_at: None,
            active_request: None,
            before_permit: None,
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
            fast_support: FastModeSupport::from_stored(None),
            idle_checked: Instant::now(),
        }
    }

    fn counts(&self) -> (i64, i64) {
        self.hub
            .store
            .db
            .locked()
            .query_row(
                "SELECT (SELECT COUNT(*) FROM donna_chain_roots),(SELECT COUNT(*) FROM donna_chain_admissions)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).unwrap();
    }
}

#[tokio::test]
async fn ator_preserva_preparacao_legada_quando_so_interrupted_e_recusado() {
    let f = Fixture::new();
    let run = f.hub.routines.recovery("bot").unwrap();
    assert_eq!(run.id, "run-legada");
    assert_eq!(f.hub.routines.list("bot")["runs"][0]["status"], "recovering");
    assert!(f.hub.routines.chain_reserved_for_test("bot"));
    let before = f.hub.routines.list("bot");
    let raw = f.hub.store.kv_read("routines.v1").unwrap();
    let mut actor = f.actor();
    actor.active_routine = Some(run.id.clone());
    let slot = crate::routines::session_slot(&run.id);
    actor.set_session(Some(&slot), Some("sessao-sintetica"));
    f.hub
        .store
        .db
        .locked()
        .execute_batch(
            "CREATE TEMP TRIGGER recusar_so_interrupted BEFORE INSERT ON kv
         WHEN NEW.k='routines.v1' AND json_extract(NEW.v,'$.runs[0].status')='interrupted'
         BEGIN SELECT RAISE(ABORT,'recusa seletiva de Interrupted; Failed permitido'); END;",
        )
        .unwrap();
    let (done_tx, mut done_rx) = mpsc::unbounded_channel::<PromptDone>();

    // Caminho real: recover_routine -> núcleo canônico negativo -> start_failed.
    actor.recover_routine().await;
    assert_eq!(f.hub.routines.list("bot"), before);
    assert_eq!(f.hub.store.kv_read("routines.v1").unwrap(), raw);
    assert!(f.hub.routines.chain_reserved_for_test("bot"));
    assert!(f.hub.routines.guard_run_live("bot", &run.id));
    assert_eq!(actor.queue.len(), 1);
    assert!(matches!(actor.queue.front(), Some(Queued::Routine(id)) if id == &run.id));
    assert!(actor.queue_retry_at.is_some());
    assert!(actor.routine_completion.is_none());
    assert!(actor.conn.is_none() && actor.turn.is_none() && actor.input.is_none());
    assert_eq!(actor.session_for(Some(&slot)).as_deref(), Some("sessao-sintetica"));
    assert!(done_rx.try_recv().is_err());
    assert_eq!(f.counts(), (0, 0));
    assert_eq!(f.hub.runtime("bot").status, BotStatus::Error);
    assert_eq!(
        f.hub.runtime("bot").activity,
        "Não foi possível salvar a preparação da rotina. Aguardando nova tentativa."
    );

    f.hub.store.db.locked().execute_batch("DROP TRIGGER recusar_so_interrupted").unwrap();
    // Faz vencer o mesmo retry, sem espera de relógio nem chamada direta ao adapter.
    actor.queue_retry_at = Some(Instant::now());
    actor.next_in_queue(&done_tx).await;
    let after = f.hub.routines.list("bot");
    assert_eq!(after["runs"][0]["id"], run.id);
    assert_eq!(after["runs"][0]["status"], "interrupted");
    assert!(after["runs"][0]["detail"].as_str().unwrap().contains("tentativas anteriores"));
    let stored: Value = serde_json::from_str(&f.hub.store.kv_read("routines.v1").unwrap().unwrap()).unwrap();
    assert_eq!(stored["runs"][0]["status"], "interrupted");
    assert!(stored["runs"][0]["_chain"].is_null());
    assert!(!f.hub.routines.chain_reserved_for_test("bot"));
    assert!(!f.hub.routines.guard_run_live("bot", &run.id));
    assert!(actor.queue.is_empty() && actor.queue_retry_at.is_none());
    assert!(actor.active_routine.is_none() && actor.routine_completion.is_none());
    assert!(actor.conn.is_none() && actor.turn.is_none());
    assert!(done_rx.try_recv().is_err());
    assert_eq!(f.counts(), (0, 0));
    let restored = crate::routines::Routines::default();
    restored.restore(&f.hub.store).unwrap();
    assert_eq!(restored.list("bot"), after);
}
