//! Agendamento da consolidação e retomada de leituras do fork.
use crate::chat::memory::dream::{DreamEvent, Ran};
use crate::chat::{memory::dream, naming};
use crate::hub::Hub;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;

const RETRY: Duration = Duration::from_secs(5);
const REVIEW_CHECK: Duration = Duration::from_secs(60);
// O executor ainda não preserva o horário de liberação informado pelo provedor.
const SUBSCRIPTION_RETRY: Duration = Duration::from_secs(15 * 60);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Recount {
    Evaluate,
    Continue,
}

pub(crate) fn spawn_dreamer(hub: Arc<Hub>, bot_id: String) -> mpsc::UnboundedSender<DreamEvent> {
    spawn(
        hub,
        bot_id,
        #[cfg(test)]
        None,
    )
}

fn spawn(
    hub: Arc<Hub>,
    bot_id: String,
    #[cfg(test)] hooks: Option<Arc<std::sync::Mutex<TestHooks>>>,
) -> mpsc::UnboundedSender<DreamEvent> {
    let (tx, mut rx) = mpsc::unbounded_channel::<DreamEvent>();
    let names = spawn_namer(hub.clone(), bot_id.clone());
    let tracker = hub.clone();
    let label = format!("sonhador:{bot_id}");
    let mut stop = hub.memory_stop.subscribe();
    let dreamer = tokio::spawn(async move {
        let mut waiting = 0usize;
        let mut review = false;
        let mut pending = Some(Recount::Evaluate);
        let idle = tokio::time::sleep(Duration::ZERO);
        tokio::pin!(idle);
        let mut failures = 0u32;
        let mut retry_at: Option<Instant> = None;
        let quiet_until = |retry_at: Option<Instant>| {
            let after = Instant::now() + RETRY;
            retry_at.map_or(after, |at| at.max(after))
        };
        loop {
            // MX-280: com o Hub parando nenhum trabalho novo começa; o que já
            // está em andamento termina antes do shutdown retornar. Pular a
            // manutenção não avança seus marcadores — nada se perde.
            if hub.routines.stopping() {
                break;
            }
            let due = pending.is_some() || waiting > 0 || review;
            let run = tokio::select! {
                biased;
                _ = stop.changed() => break,
                event = rx.recv() => match event {
                    None => break,
                    Some(DreamEvent::Turn(turn)) => {
                        // Nomeação tem consumidor próprio; seu await nunca bloqueia a fila de memória.
                        // Se o nomeador já saiu (desligamento), o turno é persistido aqui — nunca se perde.
                        if let Err(mpsc::error::SendError(turn)) = names.send(turn)
                            && let Err(error) = naming::record_pending(&hub.store, &bot_id, turn)
                        {
                            tracing::warn!(bot = %bot_id, %error, "turno de nomeação não persistido");
                        }
                        false
                    }
                    Some(DreamEvent::Wake | DreamEvent::Flush) => {
                        if pending!=Some(Recount::Continue){pending=Some(Recount::Evaluate);}
                        idle.as_mut().reset(retry_at.unwrap_or_else(Instant::now));
                        false
                    }
                },
                () = &mut idle, if due => true,
                () = tokio::time::sleep(REVIEW_CHECK), if !due => {
                    pending = Some(Recount::Evaluate);
                    idle.as_mut().reset(Instant::now());
                    false
                },
            };
            if hub.routines.stopping() {
                break;
            }
            if !run {
                continue;
            }
            let attempt = async {
                if let Some(mode) = pending {
                    (waiting, review) = read_status(
                        &hub,
                        &bot_id,
                        #[cfg(test)]
                        hooks.as_ref(),
                    )
                    .await?;
                    // A continuação só termina quando uma execução confirma Done.
                    if mode != Recount::Continue {
                        pending = None;
                    }
                    if mode == Recount::Evaluate && waiting == 0 && !review {
                        return Ok(None);
                    }
                }
                if hub.routines.stopping() {
                    return Ok(None);
                }
                execute(
                    &hub,
                    &bot_id,
                    #[cfg(test)]
                    hooks.as_ref(),
                )
                .await
                .map(Some)
            }
            .await;
            let next = match attempt {
                Ok(None) => {
                    failures = 0;
                    retry_at = None;
                    quiet_until(None)
                }
                Ok(Some(Ran::Done { more })) => {
                    failures = 0;
                    retry_at = None;
                    // Done já confirmou o cursor: só as leituras são repetidas se falharem.
                    pending = Some(if more { Recount::Continue } else { Recount::Evaluate });
                    Instant::now()
                }
                Ok(Some(Ran::Later)) => {
                    // MEX-1355: falha de seleção repetida espera mais (5 s, 10 s, 20 s...);
                    // a conferência da configuração segue a cada 60 s no máximo.
                    let after = crate::fork::background_selection::backoff::poll_after_ms(&hub.store);
                    retry_at = Some(Instant::now() + Duration::from_millis(u64::try_from(after).unwrap_or(5_000)));
                    if pending != Some(Recount::Continue) {
                        pending = Some(Recount::Evaluate);
                    }
                    quiet_until(retry_at)
                }
                Err(e) => {
                    if e.downcast_ref::<crate::fork::background_executor::Busy>().is_some() {
                        retry_at = Some(Instant::now() + RETRY);
                        if pending != Some(Recount::Continue) {
                            pending = Some(Recount::Evaluate);
                        }
                        idle.as_mut().reset(quiet_until(retry_at));
                        continue;
                    }
                    if let Ok(Some(row)) = hub.store.bot(&bot_id) {
                        crate::fork::background_notice::unavailable_with_error(&hub, &row.config, "memoria", Some(&e));
                    }
                    failures = failures.saturating_add(1);
                    if pending != Some(Recount::Continue) {
                        pending = Some(Recount::Evaluate);
                    }
                    let delay = if e.downcast_ref::<crate::fork::background_executor::SubscriptionLimit>().is_some() {
                        SUBSCRIPTION_RETRY * 2u32.pow(failures.saturating_sub(1).min(2))
                    } else {
                        RETRY * 2u32.pow(failures.min(5))
                    };
                    retry_at = Some(Instant::now() + delay);
                    tracing::warn!(bot = %bot_id, failures, error = format!("{e:#}"), "dreaming failed; it retries later");
                    quiet_until(retry_at)
                }
            };
            idle.as_mut().reset(next);
        }
        // MX-280: turnos de nomeação ainda na fila são persistidos sem chamar o
        // modelo; Wake/Flush não carregam dado — as filas duráveis reavaliam na
        // próxima abertura. Sem isto um turno concluído sumia da janela do nomeador.
        rx.close();
        while let Ok(event) = rx.try_recv() {
            if let DreamEvent::Turn(turn) = event
                && let Err(error) = naming::record_pending(&hub.store, &bot_id, turn)
            {
                tracing::warn!(bot = %bot_id, %error, "turno de nomeação não persistido no desligamento");
            }
        }
    });
    // MX-280: o Hub::shutdown aguarda estas tarefas; sem o registro uma manutenção
    // em spawn_blocking podia gravar depois do desligamento e travar o próximo Store::open.
    tracker.track_memory_task(&label, dreamer);
    tx
}

// Nomeação conserva retry_pending/Busy do executor contido, em consumidor independente da memória.
fn spawn_namer(hub: Arc<Hub>, bot: String) -> mpsc::UnboundedSender<naming::Turn> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let tracker = hub.clone();
    let label = format!("nomeador:{bot}");
    let mut stop = hub.memory_stop.subscribe();
    let task = tokio::spawn(async move {
        let mut retry = true;
        loop {
            if hub.routines.stopping() {
                break;
            }
            let result = tokio::select! {
                biased;
                _ = stop.changed() => break,
                turn = rx.recv() => match turn {
                    Some(turn) => naming::observe(&hub, &bot, turn).await,
                    None => break,
                },
                () = tokio::time::sleep(RETRY), if retry => naming::retry_pending(&hub, &bot).await,
            };
            match result {
                Ok(()) => retry = false,
                Err(error) if error.downcast_ref::<crate::fork::background_executor::Busy>().is_some() => retry = true,
                Err(error) => {
                    retry = false;
                    if let Ok(Some(row)) = hub.store.bot(&bot) {
                        crate::fork::background_notice::unavailable_with_error(&hub, &row.config, "nome", Some(&error));
                    }
                    tracing::warn!(bot=%bot,%error,"nomeação indisponível");
                }
            }
        }
        // MX-280: turnos já encaminhados e ainda na fila são persistidos sem
        // chamar o modelo — o break por parada não pode descartá-los.
        rx.close();
        while let Ok(turn) = rx.try_recv() {
            if let Err(error) = naming::record_pending(&hub.store, &bot, turn) {
                tracing::warn!(bot=%bot,%error,"turno de nomeação não persistido no desligamento");
            }
        }
    });
    // MX-280: o Hub::shutdown aguarda estas tarefas; sem o registro uma manutenção
    // em spawn_blocking podia gravar depois do desligamento e travar o próximo Store::open.
    tracker.track_memory_task(&label, task);
    tx
}

async fn read_status(
    hub: &Arc<Hub>,
    bot: &str,
    #[cfg(test)] hooks: Option<&Arc<std::sync::Mutex<TestHooks>>>,
) -> anyhow::Result<(usize, bool)> {
    #[cfg(test)]
    if let Some(hooks) = hooks {
        let mut hooks = hooks.lock().unwrap();
        hooks.reads += 1;
        if hooks.fail_read {
            anyhow::bail!("falha de contagem simulada");
        }
        hooks.review_reads += 1;
        if hooks.fail_review {
            anyhow::bail!("falha de revisão simulada");
        }
        return Ok((hooks.waiting, hooks.review));
    }
    let (waiting, review) = (dream::waiting(&hub.store, bot)?, dream::review_due(&hub.store, bot)?);
    // Sono 2b1: sem extração nem revisão do bot, confere se a rodada do dia está vencida (sem timer novo).
    if waiting == 0 && !review {
        // Manutenção não atrasa extrações. A barreira pode estar reservada por exclusão/edição;
        // espere no pool bloqueante, liberando o runtime para concluir a operação administrativa.
        let (maintenance_hub, maintenance_bot) = (hub.clone(), bot.to_owned());
        let maintenance = tokio::task::spawn_blocking(move || {
            super::profile::maintain(&maintenance_hub, &maintenance_bot, crate::store::now_ms())
        })
        .await
        .unwrap_or_else(|error| Err(error.into()));
        if let Err(error) = maintenance {
            tracing::warn!(bot, %error, "envelhecimento da memória adiado");
        }
        crate::fork::memory_auto::sleep::tick(hub, bot).await;
    }
    Ok((waiting, review))
}

async fn execute(
    hub: &Arc<Hub>,
    bot: &str,
    #[cfg(test)] hooks: Option<&Arc<std::sync::Mutex<TestHooks>>>,
) -> anyhow::Result<Ran> {
    #[cfg(test)]
    if let Some(hooks) = hooks {
        let gate = hooks.lock().unwrap().execute_gate.clone();
        hooks.lock().unwrap().runs += 1;
        if let Some(entered) = hooks.lock().unwrap().execute_entered.take() {
            let _ = entered.send(());
        }
        if let Some(gate) = gate {
            let _held = gate.lock().await;
        }
        let mut hooks = hooks.lock().unwrap();
        let cursor = hub.store.kv_read(&format!("memory.dream.{bot}"))?;
        hooks.cursors.push(cursor);
        if hooks.runs == 2 {
            match hooks.second_outcome {
                Some(TestOutcome::Later) => return Ok(Ran::Later),
                Some(TestOutcome::Error) => anyhow::bail!("falha simulada da continuação"),
                Some(TestOutcome::Limit) => return Err(crate::fork::background_executor::SubscriptionLimit.into()),
                Some(TestOutcome::Busy) => return Err(crate::fork::background_executor::Busy.into()),
                None => {}
            }
        }
        hub.store.kv_set(&format!("memory.dream.{bot}"), "42")?;
        hooks.waiting = 0;
        hooks.review = false;
        if let Some(sender) = hooks.turn_on_done.take() {
            hooks.waiting = 1;
            sender.send(DreamEvent::Wake)?;
        }
        let more = hooks.runs == 1 && hooks.first_more.is_some();
        hooks.fail_read = hooks.runs == 1 && hooks.fail_after_first.is_some();
        return Ok(Ran::Done { more });
    }
    dream::run(hub, bot).await
}

#[cfg(test)]
#[derive(Clone, Copy)]
enum TestOutcome {
    Later,
    Error,
    Limit,
    Busy,
}

#[cfg(test)]
#[derive(Default)]
struct TestHooks {
    reads: usize,
    review_reads: usize,
    runs: usize,
    waiting: usize,
    review: bool,
    fail_read: bool,
    fail_review: bool,
    first_more: Option<()>,
    fail_after_first: Option<()>,
    turn_on_done: Option<mpsc::UnboundedSender<DreamEvent>>,
    second_outcome: Option<TestOutcome>,
    cursors: Vec<Option<String>>,
    execute_gate: Option<Arc<tokio::sync::Mutex<()>>>,
    execute_entered: Option<tokio::sync::oneshot::Sender<()>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LockExt;
    use crate::remote::identity::Identity;
    use crate::store::{BotConfig, Store};
    use std::path::Path;
    use std::sync::Mutex;

    fn fixture(
        hooks: TestHooks,
    ) -> (Arc<Hub>, Arc<Mutex<TestHooks>>, mpsc::UnboundedSender<DreamEvent>, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dream-worker-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(Path::new(":memory:")).unwrap();
        let cfg: BotConfig = serde_json::from_value(serde_json::json!({"id":"bot", "name":"Bot", "cwd":dir})).unwrap();
        store.save_bot(&cfg).unwrap();
        let hub = Hub::new(store, "test".into(), Identity::load_or_create(&dir).unwrap(), "test".into(), 0);
        let hooks = Arc::new(Mutex::new(hooks));
        let tx = spawn(hub.clone(), "bot".into(), Some(hooks.clone()));
        (hub, hooks, tx, dir)
    }

    async fn tick(duration: Duration) {
        tokio::time::advance(duration).await;
        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn falha_de_contagem_retoma_sem_evento_e_sem_executar_material_desconhecido() {
        let (_, hooks, tx, dir) = fixture(TestHooks { waiting: 1, fail_read: true, ..Default::default() });
        tick(Duration::ZERO).await;
        assert_eq!(hooks.lock().unwrap().runs, 0);
        hooks.lock().unwrap().fail_read = false;
        tick(RETRY * 2).await;
        assert_eq!(hooks.lock().unwrap().runs, 1);
        drop(tx);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn um_turno_acorda_imediatamente_sem_limiar_ou_esperar_nomeacao() {
        let (_, hooks, tx, dir) = fixture(TestHooks::default());
        tick(Duration::ZERO).await;
        assert_eq!(hooks.lock().unwrap().runs, 0);
        hooks.lock().unwrap().waiting = 1;
        tx.send(DreamEvent::Turn(naming::Turn { user: "oi".into(), agent: "ok".into(), at: 0 })).unwrap();
        tx.send(DreamEvent::Wake).unwrap();
        tick(Duration::ZERO).await;
        assert_eq!(hooks.lock().unwrap().runs, 1, "um turno, sem avançar relógio ou esperar oito mensagens");
        drop(tx);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn continuacao_confirmada_sobrevive_a_falha_de_recontagem() {
        for fail in [false, true] {
            let (hub, hooks, tx, dir) = fixture(TestHooks {
                waiting: 1,
                first_more: Some(()),
                fail_after_first: fail.then_some(()),
                ..Default::default()
            });
            tick(Duration::ZERO).await;
            assert_eq!(hub.store.kv_read("memory.dream.bot").unwrap().as_deref(), Some("42"));
            if fail {
                assert_eq!(hooks.lock().unwrap().runs, 1);
                hooks.lock().unwrap().fail_read = false;
                tick(RETRY * 2).await;
            }
            assert_eq!(hooks.lock().unwrap().runs, 2);
            drop(tx);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[tokio::test(start_paused = true)]
    async fn limite_da_assinatura_nao_tenta_a_cada_ciclo() {
        let (_, hooks, tx, dir) = fixture(TestHooks {
            waiting: 1,
            first_more: Some(()),
            second_outcome: Some(TestOutcome::Limit),
            ..Default::default()
        });
        tick(Duration::ZERO).await;
        assert_eq!(hooks.lock().unwrap().runs, 2);
        tx.send(DreamEvent::Wake).unwrap();
        tick(Duration::from_secs(60)).await;
        assert_eq!(hooks.lock().unwrap().runs, 2, "Wake e ciclos não antecipam a espera do limite");
        tick(Duration::from_secs(14 * 60)).await;
        assert_eq!(hooks.lock().unwrap().runs, 3, "a manutenção deve voltar a tentar");
        drop(tx);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn continuacao_sobrevive_a_later_ou_erro_sem_evento() {
        for outcome in [TestOutcome::Error, TestOutcome::Later, TestOutcome::Limit, TestOutcome::Busy] {
            let (hub, hooks, tx, dir) = fixture(TestHooks {
                waiting: 1,
                first_more: Some(()),
                second_outcome: Some(outcome),
                ..Default::default()
            });
            tick(Duration::ZERO).await;
            assert_eq!(hooks.lock().unwrap().runs, 2);
            tick(Duration::ZERO).await;
            assert_eq!(hooks.lock().unwrap().runs, 2, "sem spin");
            if matches!(outcome, TestOutcome::Busy) {
                tx.send(DreamEvent::Wake).unwrap();
                tick(Duration::ZERO).await;
                assert_eq!(hooks.lock().unwrap().runs, 2, "Wake respeita o prazo de Busy");
            }
            tick(match outcome {
                TestOutcome::Later | TestOutcome::Busy => RETRY,
                TestOutcome::Error => RETRY * 2,
                TestOutcome::Limit => SUBSCRIPTION_RETRY,
            })
            .await;
            assert_eq!(hooks.lock().unwrap().runs, 3, "continuação parou após falha ou espera");
            if matches!(outcome, TestOutcome::Busy) {
                assert_eq!(hub.store.entries_since(0, 100).unwrap().len(), 0, "fila cheia não deve gerar aviso");
            }
            assert_eq!(hooks.lock().unwrap().cursors, vec![None, Some("42".into()), Some("42".into())]);
            drop(tx);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[tokio::test(start_paused = true)]
    async fn falha_de_leitura_apos_commit_nao_repete_aplicacao() {
        let (_, hooks, tx, dir) = fixture(TestHooks { waiting: 1, fail_after_first: Some(()), ..Default::default() });
        tick(Duration::ZERO).await;
        assert_eq!(hooks.lock().unwrap().runs, 1);
        hooks.lock().unwrap().fail_read = false;
        tick(RETRY * 2).await;
        assert_eq!(hooks.lock().unwrap().runs, 1);
        drop(tx);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn desligamento_espera_trabalho_de_memoria_em_andamento_sem_recomecar() {
        let gate = Arc::new(tokio::sync::Mutex::new(()));
        let hold = gate.lock().await;
        let (entered, ready) = tokio::sync::oneshot::channel();
        let (hub, hooks, tx, dir) = fixture(TestHooks {
            waiting: 1,
            execute_gate: Some(gate.clone()),
            execute_entered: Some(entered),
            ..Default::default()
        });
        ready.await.unwrap();
        assert_eq!(hooks.lock().unwrap().runs, 1, "execute entrou e ficou preso no gate");
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        hub.track_memory_task(
            "concluída",
            tokio::spawn(async move {
                let _ = done_tx.send(());
            }),
        );
        done_rx.await.unwrap();
        let (wait_tx, wait_rx) = tokio::sync::oneshot::channel();
        hub.memory_wait_started.locked().replace(wait_tx);
        let mut stopped = hub.memory_stop.subscribe();
        let shutdown = {
            let hub = hub.clone();
            tokio::spawn(async move { hub.shutdown().await })
        };
        stopped.changed().await.unwrap();
        wait_rx.await.unwrap();
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(!shutdown.is_finished(), "shutdown voltou com trabalho de memória em andamento");
        drop(hold);
        shutdown.await.unwrap();
        assert_eq!(hooks.lock().unwrap().runs, 1, "nenhuma execução nova depois do desligamento");
        assert!(tx.send(DreamEvent::Wake).is_err(), "sonhador encerrado não aceita eventos");
        std::fs::remove_dir_all(dir).unwrap();
    }

    // Sonda dentro do spawn_blocking da manutenção: prova a entrada do trabalho
    // bloqueante e o segura até o shutdown estar esperando — sem sleep fixo.
    #[tokio::test]
    async fn desligamento_espera_manutencao_bloqueante_sem_sleeps() {
        let dir = std::env::temp_dir().join(format!("dream-shutdown-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("test.db");
        let store = Store::open(&db).unwrap();
        let cfg: BotConfig = serde_json::from_value(serde_json::json!({"id":"bot", "name":"Bot", "cwd":dir})).unwrap();
        store.save_bot(&cfg).unwrap();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        store.before_memory_maintain.locked().replace(Box::new(move || {
            let _ = entered_tx.send(());
            // Teto de segurança: se o teste falhar antes de liberar, a thread
            // bloqueante não fica presa para sempre no runtime.
            let _ = release_rx.recv_timeout(Duration::from_secs(30));
        }));
        let hub = Hub::new(store, "test".into(), Identity::load_or_create(&dir).unwrap(), "test".into(), 0);
        let tx = spawn(hub.clone(), "bot".into(), None);
        // O recv bloqueia só a thread de teste; o spawn_blocking segue na pool.
        tokio::task::spawn_blocking(move || entered_rx.recv_timeout(Duration::from_secs(30)))
            .await
            .unwrap()
            .expect("a manutenção entrou no spawn_blocking");
        let mut stopped = hub.memory_stop.subscribe();
        let shutdown = {
            let hub = hub.clone();
            tokio::spawn(async move { hub.shutdown().await })
        };
        stopped.changed().await.unwrap();
        assert!(hub.routines.stopping(), "sinal de parada não chegou");
        assert!(!shutdown.is_finished(), "shutdown voltou com manutenção de memória bloqueada");
        release_tx.send(()).unwrap();
        shutdown.await.unwrap();
        assert!(tx.send(DreamEvent::Wake).is_err(), "sonhador encerrado não aceita eventos");
        let checks: i64 = hub
            .store
            .db
            .locked()
            .query_row("SELECT COUNT(*) FROM donna_memory_age_checks WHERE bot=?1", [cfg.memory_id()], |r| r.get(0))
            .unwrap();
        assert_eq!(checks, 0, "parada antes do commit: o marcador não avança e a manutenção se refaz");
        drop(hub);
        drop(tx);
        let store = Store::open(&db).unwrap();
        assert!(store.bot("bot").unwrap().is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }

    // Turnos de nomeação ainda na fila do sonhador são persistidos na chave
    // durável no desligamento, sem chamar o modelo — e sobrevivem a um reinício.
    #[tokio::test(start_paused = true)]
    async fn desligamento_persiste_turnos_de_nomeacao_na_fila() {
        let dir = std::env::temp_dir().join(format!("dream-naming-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("test.db");
        let store = Store::open(&db).unwrap();
        let cfg: BotConfig =
            serde_json::from_value(serde_json::json!({"id":"bot", "name":"Bot", "cwd":dir, "autoName":true})).unwrap();
        store.save_bot(&cfg).unwrap();
        let hub = Hub::new(store, "test".into(), Identity::load_or_create(&dir).unwrap(), "test".into(), 0);
        // O execute fica preso no gate: os Turnos enviados enquanto isso ficam
        // enfileirados no canal do sonhador até o desligamento drenar a fila.
        let gate = Arc::new(tokio::sync::Mutex::new(()));
        let hold = gate.lock().await;
        let (entered, ready) = tokio::sync::oneshot::channel();
        let hooks = Arc::new(Mutex::new(TestHooks {
            waiting: 1,
            execute_gate: Some(gate.clone()),
            execute_entered: Some(entered),
            ..Default::default()
        }));
        let tx = spawn(hub.clone(), "bot".into(), Some(hooks.clone()));
        ready.await.unwrap();
        assert_eq!(hooks.lock().unwrap().runs, 1, "execute entrou e ficou preso no gate");
        for i in 0..2 {
            tx.send(DreamEvent::Turn(naming::Turn { user: format!("u{i}"), agent: "ok".into(), at: i64::from(i) }))
                .unwrap();
        }
        let (wait_tx, wait_rx) = tokio::sync::oneshot::channel();
        hub.memory_wait_started.locked().replace(wait_tx);
        let mut stopped = hub.memory_stop.subscribe();
        let shutdown = {
            let hub = hub.clone();
            tokio::spawn(async move { hub.shutdown().await })
        };
        stopped.changed().await.unwrap();
        assert!(hub.routines.stopping(), "sinal de parada não chegou");
        wait_rx.await.unwrap();
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(!shutdown.is_finished(), "shutdown voltou com o sonhador preso no execute");
        drop(hold);
        shutdown.await.unwrap();
        let raw = hub.store.kv_get("naming.turns.bot").expect("turnos persistidos");
        let turns: Vec<naming::Turn> = serde_json::from_str(&raw).unwrap();
        assert_eq!(turns.len(), 2, "os dois turnos enfileirados foram persistidos sem o modelo");
        assert_eq!(turns[0].user, "u0");
        // Reinício: o banco reaberto ainda tem a fila para o retry_pending.
        drop(hub);
        drop(tx);
        let store = Store::open(&db).unwrap();
        let raw = store.kv_get("naming.turns.bot").expect("fila sobrevive ao reinício");
        let turns: Vec<naming::Turn> = serde_json::from_str(&raw).unwrap();
        assert_eq!(turns.len(), 2);
        let hub = Hub::new(store, "test".into(), Identity::load_or_create(&dir).unwrap(), "test".into(), 0);
        naming::retry_pending(&hub, "bot").await.unwrap();
        hub.shutdown().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
}
