//! D70 C4: somente a fronteira do envio e os controles duráveis da espera.
use super::*;
use crate::fork::usage_limits::{self as limits, Target};

fn checkpoint(f: &Fixture) -> Value {
    serde_json::from_str(&f.hub.store.kv_read("turn.waiting.bot").unwrap().unwrap()).unwrap()
}
fn target(w: &Value) -> Target {
    Target {
        bot_id: "bot".into(),
        chat_id: w["input"]["chat"].as_str().unwrap().into(),
        session_id: w["input"]["visibleSession"].as_str().map(str::to_owned),
        entry_id: w["notice"].as_str().unwrap().into(),
        wait_id: w["usageControl"]["id"].as_str().unwrap().into(),
    }
}
fn policy(f: &Fixture, auto_resume: bool, snooze: bool) {
    f.hub
        .store
        .kv_set("donna.usage-limits.v1", &json!({"autoResume":auto_resume,"snooze":snooze}).to_string())
        .unwrap();
}

#[tokio::test]
async fn c4_desligado_sobrevive_reinicio_e_manual_preserva_pedido_e_sessao() {
    let mut f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    policy(&f, false, false);
    f.make_retry_due();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until("c4_desligado_sobrevive_reinicio_e_manual_preserva_pedido_e_sessao", &f.hub, || {
        f.hub.runtime("bot").status == BotStatus::Error
    })
    .await;
    assert_eq!(f.prompts().len(), 1);
    let saved = checkpoint(&f);
    assert_eq!(saved["phase"], "paused");
    assert_eq!(saved["usageControl"]["policyPaused"], true);
    assert!(!limits::settings(&f.hub.store).unwrap().auto_resume);
    f.hub.shutdown().await;
    let mut actor = f.actor();
    assert!(actor.restore_waiting());
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    actor.resume_usage_wait(&target(&saved), None).unwrap();
    actor.retry_waiting(&tx).await;
    actor.finish_turn(&rx.recv().await.unwrap().result);
    assert_eq!(f.prompts().len(), 2);
    assert_eq!(f.prompts()[0]["sessionId"], f.prompts()[1]["sessionId"]);
    assert!(f.prompts()[1]["prompt"][0]["text"].as_str().unwrap().contains("Continue a tarefa pendente"));
    if let Some(c) = actor.conn.take() {
        c.acp.kill().await;
    }
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn c4_cancelar_na_fronteira_do_permit_nao_envia_nem_cancela_a_fila() {
    let f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    f.make_retry_due();
    let saved = checkpoint(&f);
    let alvo = target(&saved);
    let old = alvo.clone();
    let queued = f
        .hub
        .store
        .insert_entry(
            &crate::store::Lane { chat: "bot".into(), session: Some("outra-conversa".into()) },
            EntryKind::User,
            2,
            &json!({"text":"Próximo pedido de outra conversa", "status":"queued"}),
        )
        .unwrap();
    let mut actor = f.actor();
    assert!(actor.restore_waiting());
    actor.before_permit = Some(Box::new(move |hub| {
        assert_eq!(limits::cancel(&hub.store, &alvo).unwrap()["alreadyStarted"], false);
    }));
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    actor.retry_waiting(&tx).await;
    assert!(actor.before_permit.is_none());
    assert_eq!(f.prompts().len(), 1, "cancelamento dentro do preparo recusa o envio");
    assert!(actor.waiting.is_none());
    assert!(actor.turn.is_none());
    assert_eq!(f.hub.store.entry(&queued.id).unwrap().data["status"], "queued");
    assert_eq!(f.hub.store.entry(&queued.id).unwrap().session_id, "outra-conversa");
    let notice = f.hub.store.entry(&old.entry_id).unwrap();
    assert_eq!(notice.data["subscriptionWait"]["cancelled"], true);
    assert_eq!(notice.data["subscriptionWait"]["retry"], true);
    // Guarda durável não pode ser sobrescrita pelo checkpoint capturado antes do clique.
    let mut late = saved.clone();
    late["phase"] = json!("retrying");
    assert!(!limits::allow_on(&f.hub.store.db.locked(), Some(&late.to_string())).unwrap());
    policy(&f, false, true);
    policy(&f, true, false);
    assert!(!limits::allow_on(&f.hub.store.db.locked(), Some(&late.to_string())).unwrap());
    // Só uma ação manual explícita cria geração nova, com a identidade original.
    actor.resume_usage_wait(&old, None).unwrap();
    let restored = checkpoint(&f);
    assert_ne!(restored["usageControl"]["id"], saved["usageControl"]["id"]);
    assert_eq!(restored["executionToken"], saved["executionToken"]);
    assert_eq!(restored["input"], saved["input"]);
    assert_eq!(restored["memoryPolicy"], saved["memoryPolicy"]);
    assert_eq!(restored["routing"], saved["routing"]);
    assert!(limits::cancel(&f.hub.store, &old).is_err(), "clique antigo não controla a nova geração");
    if let Some(c) = actor.conn.take() {
        c.acp.kill().await;
    }
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn c4_cancelamento_sobrevive_reinicio_e_nao_atinge_outro_destino() {
    let mut f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    let saved = checkpoint(&f);
    let alvo = target(&saved);
    for wrong in [
        Target { chat_id: "outra".into(), ..alvo.clone() },
        Target { session_id: Some("extra".into()), ..alvo.clone() },
        Target { wait_id: "antiga".into(), ..alvo.clone() },
        Target { bot_id: "outro".into(), ..alvo.clone() },
    ] {
        assert!(limits::cancel(&f.hub.store, &wrong).is_err());
    }
    limits::cancel(&f.hub.store, &alvo).unwrap();
    f.make_retry_due();
    policy(&f, true, true);
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until("c4_cancelamento_sobrevive_reinicio_e_nao_atinge_outro_destino", &f.hub, || {
        f.hub.store.entry(&alvo.entry_id).unwrap().data["subscriptionWait"]["cancelled"] == true
    })
    .await;
    assert_eq!(f.prompts().len(), 1);
    assert!(limits::cancellation(&f.hub.store, &alvo.wait_id).unwrap().is_some());
    assert!(f.hub.store.kv_read("turn.waiting.bot").unwrap().is_none());
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn c4_soneca_respeita_reset_mesmo_com_retry_vencido_e_avisa_sem_enviar() {
    let f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    policy(&f, true, true);
    let mut saved = checkpoint(&f);
    saved["retryAt"] = json!(0);
    saved["resumeAt"] = json!(crate::store::now_ms() + 3_600_000);
    f.hub.store.kv_set("turn.waiting.bot", &saved.to_string()).unwrap();
    let mut actor = f.actor();
    actor.restore_waiting();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    actor.retry_waiting(&tx).await;
    assert_eq!(f.prompts().len(), 1);
    assert_eq!(checkpoint(&f)["attemptCount"], saved["attemptCount"]);
    policy(&f, false, true);
    actor.retry_waiting(&tx).await;
    let notice = f.hub.store.entry(saved["notice"].as_str().unwrap()).unwrap();
    assert_eq!(notice.data["subscriptionWait"]["snoozed"], true);
    assert_eq!(notice.data["subscriptionWait"]["retry"], false);
    let mut woke = checkpoint(&f);
    woke["resumeAt"] = json!(crate::store::now_ms() - 1);
    f.hub.store.kv_set("turn.waiting.bot", &woke.to_string()).unwrap();
    let mut actor = f.actor();
    actor.restore_waiting();
    actor.retry_waiting(&tx).await;
    assert_eq!(f.prompts().len(), 1);
    let notice = f.hub.store.entry(saved["notice"].as_str().unwrap()).unwrap();
    assert_eq!(notice.data["subscriptionWait"]["ready"], true);
    assert_eq!(notice.data["subscriptionWait"]["retry"], true);
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn c4_falha_de_gravacao_nao_confirma_cancelamento_nem_preferencia() {
    let f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    let saved = checkpoint(&f);
    f.hub.store.db.locked().execute_batch("CREATE TRIGGER fail_c4 BEFORE INSERT ON kv WHEN NEW.k LIKE 'donna.usage%' BEGIN SELECT RAISE(ABORT,'gravação recusada'); END;").unwrap();
    let caller = crate::api::devices::Caller::Local;
    assert!(
        limits::call(&f.hub, &caller, "donnaUsageLimitsSave", json!({"autoResume":false,"snooze":true})).await.is_err()
    );
    assert!(limits::call(&f.hub, &caller, "donnaUsageWaitCancel", serde_json::to_value(json!({"botId":"bot","chatId":"bot","sessionId":null,"entryId":saved["notice"],"waitId":saved["usageControl"]["id"]})).unwrap()).await.is_err());
    assert!(limits::settings(&f.hub.store).unwrap().auto_resume);
    assert!(limits::cancellation(&f.hub.store, &target(&saved).wait_id).unwrap().is_none());
    assert_eq!(checkpoint(&f), saved);
    assert_ne!(
        f.hub.store.entry(saved["notice"].as_str().unwrap()).unwrap().data["subscriptionWait"]["cancelled"],
        true
    );
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn c4_upgrade_persiste_geracao_antes_de_publicar_botao() {
    let f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    let mut old = checkpoint(&f);
    old.as_object_mut().unwrap().remove("usageControl");
    f.hub.store.kv_set("turn.waiting.bot", &old.to_string()).unwrap();
    let mut actor = f.actor();
    assert!(actor.restore_waiting());
    let upgraded = checkpoint(&f);
    let id = upgraded["usageControl"]["id"].clone();
    assert!(id.is_string());
    assert_eq!(upgraded["retryAt"], old["retryAt"]);
    assert_eq!(upgraded["attemptCount"], old["attemptCount"]);
    assert_eq!(f.hub.store.entry(old["notice"].as_str().unwrap()).unwrap().data["subscriptionWait"]["waitId"], id);
    let mut restarted = f.actor();
    restarted.restore_waiting();
    assert_eq!(checkpoint(&f)["usageControl"]["id"], id);
    assert_eq!(limits::cancel(&f.hub.store, &target(&upgraded)).unwrap()["cancelled"], true);
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn c4_soneca_ligada_no_preparo_conserva_prompt_e_nao_prende_reserva() {
    let f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    let mut saved = checkpoint(&f);
    saved["resumeAt"] = json!(crate::store::now_ms() + 3_600_000);
    saved["phase"] = json!("paused");
    f.hub.store.kv_set("turn.waiting.bot", &saved.to_string()).unwrap();
    let mut actor = f.actor();
    actor.restore_waiting();
    actor.resume_usage_wait(&target(&saved), None).unwrap();
    actor.before_permit = Some(Box::new(|hub| {
        hub.store.kv_set("donna.usage-limits.v1", &json!({"autoResume":true,"snooze":true}).to_string()).unwrap();
    }));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    actor.retry_waiting(&tx).await;
    assert!(actor.before_permit.is_none());
    assert_eq!(f.prompts().len(), 1);
    assert!(actor.pending_admission.is_some());
    assert!(actor.dispatch.is_some());
    let attempts = checkpoint(&f)["attemptCount"].clone();
    // Avança só o reset do checkpoint em memória, sem desmontar a reserva/prompt capturados.
    let mut woke = serde_json::to_value(actor.waiting.as_ref().unwrap()).unwrap();
    woke["resumeAt"] = json!(crate::store::now_ms() - 1);
    actor.waiting = Some(serde_json::from_value(woke.clone()).unwrap());
    f.hub.store.kv_set("turn.waiting.bot", &woke.to_string()).unwrap();
    actor.queue_retry_at = None;
    actor.retry_admission(&tx).await;
    let done = tokio::time::timeout(Duration::from_secs(10), rx.recv()).await.unwrap().unwrap();
    assert_eq!(checkpoint(&f)["attemptCount"], attempts);
    actor.finish_turn(&done.result);
    assert_eq!(f.prompts().len(), 2);
    assert!(actor.pending_admission.is_none());
    if let Some(c) = actor.conn.take() {
        c.acp.kill().await;
    }
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn c4_desligar_no_preparo_libera_reserva_sem_perder_pedido() {
    let f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    f.make_retry_due();
    let mut actor = f.actor();
    actor.restore_waiting();
    actor.before_permit = Some(Box::new(|hub| {
        hub.store.kv_set("donna.usage-limits.v1", &json!({"autoResume":false,"snooze":false}).to_string()).unwrap();
    }));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    actor.retry_waiting(&tx).await;
    assert!(actor.before_permit.is_none());
    assert_eq!(f.prompts().len(), 1);
    assert!(actor.dispatch.is_none());
    assert!(actor.pending_admission.is_none());
    assert_eq!(checkpoint(&f)["phase"], "paused");
    actor.resume_usage_wait(&target(&checkpoint(&f)), None).unwrap();
    actor.retry_waiting(&tx).await;
    actor.finish_turn(&rx.recv().await.unwrap().result);
    assert_eq!(f.prompts().len(), 2);
    if let Some(c) = actor.conn.take() {
        c.acp.kill().await;
    }
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn c4_recupera_aviso_cancelado_apos_falha_de_publicacao_e_reinicio() {
    let mut f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    let alvo = target(&checkpoint(&f));
    limits::cancel(&f.hub.store, &alvo).unwrap();
    f.hub.store.db.locked().execute_batch("CREATE TRIGGER fail_cancel_notice BEFORE UPDATE ON entries WHEN json_extract(NEW.data,'$.subscriptionWait.cancelled')=1 BEGIN SELECT RAISE(ABORT,'aviso indisponível'); END;").unwrap();
    let mut actor = f.actor();
    actor.restore_waiting();
    assert!(actor.sync_usage_control().await);
    assert!(actor.waiting.is_none());
    assert!(f.hub.store.kv_read("turn.waiting.bot").unwrap().is_none());
    assert_eq!(limits::pending_notices(&f.hub.store, "bot").unwrap().len(), 1);
    f.hub.store.db.locked().execute_batch("DROP TRIGGER fail_cancel_notice").unwrap();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until("c4_recupera_aviso_cancelado_apos_falha_de_publicacao_e_reinicio", &f.hub, || {
        f.hub.store.entry(&alvo.entry_id).unwrap().data["subscriptionWait"]["cancelled"] == true
    })
    .await;
    assert_eq!(f.hub.store.entry(&alvo.entry_id).unwrap().data["subscriptionWait"]["retry"], true);
    assert_eq!(limits::pending_notices(&f.hub.store, "bot").unwrap(), [] as [Value; 0]);
    assert_eq!(f.prompts().len(), 1);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn c4_cancelar_apos_permissao_nao_promete_ausencia_de_efeitos_nem_libera_replay() {
    let f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    f.make_retry_due();
    std::fs::write(f.dir.join("hold"), "").unwrap();
    let mut actor = f.actor();
    actor.restore_waiting();
    let alvo = target(&checkpoint(&f));
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    actor.retry_waiting(&tx).await;
    f.until("c4_cancelar_apos_permissao_nao_promete_ausencia_de_efeitos_nem_libera_replay", &f.hub, || {
        f.prompts().len() == 2
    })
    .await;
    assert_eq!(limits::cancel(&f.hub.store, &alvo).unwrap()["alreadyStarted"], true);
    assert!(actor.sync_usage_control().await);
    assert!(actor.stop_requested);
    assert!(actor.resume_usage_wait(&alvo, None).is_err());
    let notice = f.hub.store.entry(&alvo.entry_id).unwrap();
    assert_eq!(notice.data["subscriptionWait"]["alreadyStarted"], true);
    assert_eq!(notice.data["subscriptionWait"]["retry"], false);
    assert!(notice.data["text"].as_str().unwrap().contains("confira os efeitos"));
    if let Some(c) = actor.conn.take() {
        c.acp.kill().await;
    }
    let mut restarted = f.actor();
    restarted.restore_dispatch().unwrap();
    restarted.restore_waiting();
    restarted.retry_waiting(&tx).await;
    assert_eq!(f.prompts().len(), 2, "reinício não repete despacho incerto cancelado");
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn c4_rpc_de_preferencia_exige_dono_e_confirma_estado_gravado() {
    let f = Fixture::new();
    let owner = crate::api::devices::Caller::Local;
    let untrusted = crate::api::devices::Caller::Pairing { key: "sem-autoridade".into() };
    let wanted = json!({"autoResume":false,"snooze":true});
    assert!(limits::call(&f.hub, &untrusted, "donnaUsageLimits", json!({})).await.is_err());
    let denied = limits::call(&f.hub, &untrusted, "donnaUsageLimitsSave", wanted.clone()).await;
    assert!(denied.is_err());
    let defaults = limits::call(&f.hub, &owner, "donnaUsageLimits", json!({})).await.unwrap();
    assert_eq!(defaults, json!({"autoResume":true,"snooze":false}));
    let saved = limits::call(&f.hub, &owner, "donnaUsageLimitsSave", wanted.clone()).await.unwrap();
    assert_eq!(saved, wanted);
    let read = limits::call(&f.hub, &owner, "donnaUsageLimits", json!({})).await.unwrap();
    assert_eq!(read, wanted);
    assert_eq!(f.prompts(), [] as [Value; 0]);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn c4_cancelar_apos_reinicio_conserva_incerteza_mesmo_sem_cerca_ativa() {
    let f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    f.make_retry_due();
    std::fs::write(f.dir.join("hold"), "").unwrap();
    let mut actor = f.actor();
    actor.restore_waiting();
    let alvo = target(&checkpoint(&f));
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    actor.retry_waiting(&tx).await;
    f.until("c4_cancelar_apos_reinicio_conserva_incerteza_mesmo_sem_cerca_ativa", &f.hub, || f.prompts().len() == 2)
        .await;
    if let Some(c) = actor.conn.take() {
        c.acp.kill().await;
    }
    let mut restarted = f.actor();
    restarted.restore_dispatch().unwrap();
    restarted.restore_waiting();
    assert!(checkpoint(&f)["usageControl"]["uncertainAt"].as_i64().is_some());
    assert!(crate::fork::bot_admission::current(&f.hub.store, "bot").unwrap().is_none());
    assert_eq!(limits::cancel(&f.hub.store, &alvo).unwrap()["alreadyStarted"], true);
    restarted.sync_usage_control().await;
    assert!(restarted.resume_usage_wait(&alvo, None).is_err());
    assert_eq!(f.hub.store.entry(&alvo.entry_id).unwrap().data["subscriptionWait"]["retry"], false);
    assert_eq!(f.prompts().len(), 2);
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// D368 (revisão Sol, rodada 5): o pedido "Retomar" do app pareado espera na fila do ator; o aparelho
/// perde o acesso nessa espera e o ator não retoma. Vale para a espera pausada e para a desistência.
#[tokio::test]
async fn c4_retomar_revogado_na_fila_do_ator_nao_retoma() {
    use crate::fork::donna_fixa::pedido::Origem;
    use crate::store::{Device, DeviceSource, Scope};
    let mut f = Fixture::new();
    f.send().await;
    f.hub.shutdown().await;
    policy(&f, false, false);
    f.make_retry_due();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until("c4_retomar_revogado_na_fila_do_ator_nao_retoma", &f.hub, || {
        f.hub.runtime("bot").status == BotStatus::Error
    })
    .await;
    f.hub.shutdown().await;
    let device = Device {
        key: "mac-c4".into(),
        name: "Mac".into(),
        platform: "macos".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: crate::store::now_ms(),
        last_seen_at: None,
    };
    let origem = |f: &Fixture| {
        // Reconcessão de aparelho Local = o passo do pareamento por código (`readmitir_pareado`), que libera o bloqueio e grava o aparelho numa Tx.
        crate::fork::workspace::readmitir_pareado(&f.hub.store, &device).unwrap();
        let (_, caller) = crate::fork::autoridade::autenticar(&f.hub, &device.key).unwrap();
        Origem::Dono(f.hub.clone(), crate::fork::autoridade::Autoridade::capturar(&f.hub, &caller).unwrap())
    };
    // Espera pausada: revogado depois do pedido, antes de o ator agir.
    let mut actor = f.actor();
    assert!(actor.restore_waiting());
    let saved = checkpoint(&f);
    let pedido = origem(&f);
    assert!(f.hub.revoke_device(&device.key).unwrap());
    assert!(actor.resume_usage_wait(&target(&saved), Some(&pedido)).is_err());
    assert_eq!(checkpoint(&f), saved, "a espera pausada não muda");
    // Revogado DEPOIS da conferência do ator e ANTES da gravação (gancho before_owner_write): a
    // mesma transação recusa; a espera não ganha manual=true nem volta a tentar.
    let no_intervalo = |f: &Fixture| {
        let fired = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let revoker = f.hub.clone();
        let marca = fired.clone();
        let key = device.key.clone();
        *f.hub.store.before_owner_write.lock().unwrap() = Some(Box::new(move || {
            marca.store(true, std::sync::atomic::Ordering::SeqCst);
            revoker.revoke_device(&key).unwrap();
        }));
        fired
    };
    let pedido = origem(&f);
    let fired = no_intervalo(&f);
    assert!(actor.resume_usage_wait(&target(&saved), Some(&pedido)).is_err());
    assert!(fired.load(std::sync::atomic::Ordering::SeqCst), "pausada: a fronteira da gravação foi atingida");
    assert_eq!(checkpoint(&f), saved, "pausada: revogado no intervalo, nada gravado");
    let w = serde_json::to_value(actor.waiting.as_ref().expect("a espera continua no ator")).unwrap();
    assert_eq!(w, saved, "pausada: em memória, a espera anterior foi reposta (sem manual=true)");
    assert_eq!(w["phase"], "paused", "pausada: sem nova tentativa");
    // Desistência gravada: a retomada revogada não consome a desistência nem instala a espera.
    limits::cancel(&f.hub.store, &target(&saved)).unwrap();
    let mut actor = f.actor();
    let pedido = origem(&f);
    assert!(f.hub.revoke_device(&device.key).unwrap());
    assert!(actor.resume_usage_wait(&target(&saved), Some(&pedido)).is_err());
    let cancelada = limits::cancellation(&f.hub.store, &target(&saved).wait_id).unwrap().unwrap();
    assert_eq!(cancelada["resumed"], false);
    // Desistência gravada, revogado no intervalo: resume_archive grava sob a mesma autoridade e recusa.
    let antes = f.hub.store.kv_read("turn.waiting.bot").unwrap();
    let pedido = origem(&f);
    let fired = no_intervalo(&f);
    assert!(actor.resume_usage_wait(&target(&saved), Some(&pedido)).is_err());
    assert!(fired.load(std::sync::atomic::Ordering::SeqCst), "arquivada: a fronteira da gravação foi atingida");
    let cancelada = limits::cancellation(&f.hub.store, &target(&saved).wait_id).unwrap().unwrap();
    assert_eq!(cancelada["resumed"], false, "arquivada: a desistência não é consumida");
    assert_eq!(f.hub.store.kv_read("turn.waiting.bot").unwrap(), antes, "arquivada: a espera não é instalada");
    assert!(actor.waiting.is_none(), "arquivada: sem nova tentativa");
    // Com a autoridade vigente, a mesma retomada passa.
    let pedido = origem(&f);
    actor.resume_usage_wait(&target(&saved), Some(&pedido)).unwrap();
    if let Some(c) = actor.conn.take() {
        c.acp.kill().await;
    }
    let _ = std::fs::remove_dir_all(&f.dir);
}
