//! Reinício com efeito incerto: a conversa fica pausada (inclusive depois de outro reinício), nada é repetido e só a
//! liberação explícita do dono devolve a fila à admissão.
use super::*;
use crate::fork::bot_admission as admission;

fn notices(store: &Store) -> usize {
    containing(store, "pode haver efeito já realizado")
}

/// Avisos da conversa com o trecho, sem diferenciar maiúscula no começo.
fn containing(store: &Store, part: &str) -> usize {
    store
        .history("bot", i64::MAX, 200)
        .unwrap()
        .iter()
        .filter(|e| e.kind == "notice" && e.data["text"].as_str().is_some_and(|s| s.to_lowercase().contains(part)))
        .count()
}

/// Rotina interrompida: um único aviso de efeito incerto (o da rotina) e um único aviso de pausa.
fn assert_routine_notices(store: &Store, effect: usize) {
    assert_eq!(containing(store, "efeito já realizado"), effect, "o aviso de efeito não se duplica");
    assert_eq!(containing(store, "ficou pausada"), 1, "um aviso de pausa");
}

async fn send(hub: &Arc<Hub>, text: &str) -> String {
    let sent =
        crate::api::dispatch(hub, &crate::api::devices::Caller::Local, "send", json!({"botId":"bot","text":text}))
            .await
            .unwrap();
    sent["entry"]["id"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn efeito_incerto_pausa_a_conversa_ate_liberar_e_sobrevive_a_reinicio_duplo() {
    let f = Fixture::new();
    std::fs::write(f.dir.join("blocked"), "").unwrap();
    std::fs::write(f.dir.join("hold"), "").unwrap();
    send(&f.hub, "Efeito possível").await;
    f.until(|| f.prompts().len() == 1).await;
    assert_eq!(admission::current(&f.hub.store, "bot").unwrap().unwrap().phase, admission::Phase::Permitted);
    f.hub.shutdown().await;
    std::fs::remove_file(f.dir.join("hold")).unwrap();

    let first = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| admission::current(&first.store, "bot").unwrap().is_none()).await;
    assert!(admission::conversation_paused(&first.store, "bot", "bot").unwrap());
    let later = send(&first, "Depois do reinício").await;
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert_eq!(f.prompts().len(), 1, "nada sai da conversa pausada");
    assert_eq!(first.store.entry(&later).unwrap().data["status"], "queued");
    assert_eq!(notices(&first.store), 1);
    first.shutdown().await;

    let second = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert!(admission::conversation_paused(&second.store, "bot", "bot").unwrap(), "a pausa sobrevive ao reinício");
    assert_eq!(f.prompts().len(), 1, "o segundo reinício também não envia nem repete");
    assert_eq!(second.store.entry(&later).unwrap().data["status"], "queued");
    assert_eq!(notices(&second.store), 1, "o aviso não se repete");

    let released = crate::api::dispatch(
        &second,
        &crate::api::devices::Caller::Local,
        "releaseConversation",
        json!({"botId":"bot"}),
    )
    .await
    .unwrap();
    assert_eq!(released["released"], json!(["bot"]));
    f.until(|| f.prompts().len() == 2 && second.store.entry(&later).unwrap().data["status"] == "sent").await;
    let prompt = f.prompts()[1].to_string();
    assert!(prompt.contains("Depois do reinício"));
    assert!(!prompt.contains("Efeito possível"), "o turno incerto não é reenviado");
    second.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn pausa_de_uma_conversa_nao_segura_outra_e_permit_recusa_a_pausada() {
    let f = Fixture::new();
    f.hub.shutdown().await;
    let mut actor = f.actor();
    admission::pause_conversation(&f.hub.store, "bot", "outra-conversa", "t-velho").unwrap();
    assert!(actor.conversation_held("outra-conversa"));
    assert!(!actor.conversation_held("bot"), "a principal segue");
    let ticket =
        admission::reserve(&f.hub.store, &actor.cfg, "outra-conversa", "hub-A", &admission::Origin::Group).unwrap();
    let refused = admission::permit(&f.hub.store, &actor.cfg, &ticket, None, &[], None).unwrap_err();
    assert!(refused.is::<admission::ConversationPaused>(), "a última fronteira também recusa a conversa pausada");
    assert!(admission::current(&f.hub.store, "bot").unwrap().is_none(), "a reserva sai sem despacho");
    let other = admission::reserve(&f.hub.store, &actor.cfg, "bot", "hub-B", &admission::Origin::Group).unwrap();
    admission::finish(&f.hub.store, "bot", &other).unwrap();
    actor.dispatch = None;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Liberar a pausa é só deste computador: aparelho com Control (delegado ou já revogado), ponte de ferramentas e
/// canal de pareamento são recusados e a pausa continua.
#[tokio::test]
async fn so_este_computador_libera_a_conversa_pausada() {
    use crate::api::devices::Caller;
    use crate::store::{Device, DeviceSource, Scope};
    let f = Fixture::new();
    admission::pause_conversation(&f.hub.store, "bot", "bot", "t-velho").unwrap();
    let device = |key: &str| {
        f.hub
            .store
            .put_device(&Device {
                key: key.into(),
                name: "Aparelho de teste".into(),
                platform: "fixture".into(),
                source: DeviceSource::Local,
                grant_id: None,
                scopes: vec![Scope::Control],
                lease_until: None,
                created_at: crate::store::now_ms(),
                last_seen_at: None,
            })
            .unwrap();
        crate::fork::autoridade::autenticar(&f.hub, key).map(|(_, caller)| caller).unwrap()
    };
    let delegado = device("pausa-control");
    let revogado = device("pausa-revogado");
    assert!(f.hub.revoke_device("pausa-revogado").unwrap());
    crate::fork::sessoes::bindings::fixture_execution(&f.hub, "bot");
    let binding = crate::fork::sessoes::bindings::for_acp(&f.hub.store, "bot", "fixture-acp").unwrap();
    let mut servers = vec![json!({"env": []})];
    crate::fork::ponte_credencial::provisionar(&f.hub, "bot", None, &binding, &mut servers).unwrap();
    let token = zeroize::Zeroizing::new(servers[0]["env"][0]["value"].as_str().unwrap().to_owned());
    let ponte = f.hub.pontes.autenticar(&f.hub, &token).unwrap();
    assert!(matches!(&ponte, Caller::Ponte { .. }));
    for (label, caller) in [
        ("delegado", delegado),
        ("revogado", revogado),
        ("ponte", ponte),
        ("pareamento", Caller::Pairing { key: "x".into() }),
    ] {
        let result = crate::api::dispatch(&f.hub, &caller, "releaseConversation", json!({"botId":"bot"})).await;
        assert!(result.is_err(), "{label} não libera a conversa pausada");
        assert!(admission::conversation_paused(&f.hub.store, "bot", "bot").unwrap());
    }
    let released =
        crate::api::dispatch(&f.hub, &Caller::Local, "releaseConversation", json!({"botId":"bot"})).await.unwrap();
    assert_eq!(released["released"], json!(["bot"]));
    assert!(!admission::conversation_paused(&f.hub.store, "bot", "bot").unwrap());
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// O boot marca como interrompida a rotina com despacho permitido; isso não conta como conclusão. A conversa pausa,
/// a mensagem nova espera (também depois de outro reinício) e só a liberação a solta, sem repetir a rotina.
#[tokio::test]
async fn rotina_com_efeito_incerto_tambem_pausa_a_conversa_em_dois_boots() {
    let f = Fixture::new();
    std::fs::write(f.dir.join("blocked"), "").unwrap();
    std::fs::write(f.dir.join("hold"), "").unwrap();
    let routine = f
        .hub
        .routines
        .save(
            &f.hub,
            "bot",
            &json!({"name":"Rotina com efeito", "instruction":"Faça", "triggers":[{"type":"webhook"}]}),
        )
        .unwrap();
    let id = routine["routine"]["id"].as_str().unwrap().to_owned();
    let run = f.enqueue(&id, "test", true).await;
    let run_id = run["run"]["id"].as_str().unwrap().to_owned();
    f.hub.send_cmd("bot", crate::agent::bot::Cmd::Routine(run_id.clone())).unwrap();
    f.until(|| f.prompts().len() == 1).await;
    let record = admission::current(&f.hub.store, "bot").unwrap().unwrap();
    assert_eq!(record.phase, admission::Phase::Permitted);
    assert!(matches!(&record.origin, admission::Origin::Routine { id } if *id == run_id));
    f.hub.shutdown().await;
    std::fs::remove_file(f.dir.join("hold")).unwrap();

    let first = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| admission::current(&first.store, "bot").unwrap().is_none()).await;
    assert_eq!(first.routines.list("bot")["runs"][0]["status"], "interrupted");
    assert!(admission::conversation_paused(&first.store, "bot", &record.conversation_id).unwrap());
    let later = send(&first, "Depois da rotina").await;
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert_eq!(f.prompts().len(), 1, "nada sai da conversa pausada");
    assert_eq!(first.store.entry(&later).unwrap().data["status"], "queued");
    assert_routine_notices(&first.store, 1);
    first.shutdown().await;

    let second = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert!(admission::conversation_paused(&second.store, "bot", &record.conversation_id).unwrap());
    assert_eq!(f.prompts().len(), 1, "o segundo boot não envia nem repete a rotina");
    assert_eq!(second.store.entry(&later).unwrap().data["status"], "queued");
    assert_routine_notices(&second.store, 1);
    assert_eq!(second.routines.list("bot")["runs"][0]["status"], "interrupted");

    crate::api::dispatch(&second, &crate::api::devices::Caller::Local, "releaseConversation", json!({"botId":"bot"}))
        .await
        .unwrap();
    f.until(|| f.prompts().len() == 2 && second.store.entry(&later).unwrap().data["status"] == "sent").await;
    assert!(f.prompts()[1].to_string().contains("Depois da rotina"));
    assert_eq!(second.routines.list("bot")["runs"].as_array().unwrap().len(), 1, "a rotina não é reenfileirada");
    second.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Queda entre a pausa e o aviso: o próximo boot repete só a pausa (idempotente) e grava o aviso uma vez.
#[tokio::test]
async fn queda_depois_da_pausa_e_antes_do_aviso_nao_duplica_nada() {
    let f = Fixture::new();
    std::fs::write(f.dir.join("blocked"), "").unwrap();
    std::fs::write(f.dir.join("hold"), "").unwrap();
    send(&f.hub, "Efeito possível").await;
    f.until(|| f.prompts().len() == 1).await;
    let record = admission::current(&f.hub.store, "bot").unwrap().unwrap();
    f.hub.shutdown().await;
    std::fs::remove_file(f.dir.join("hold")).unwrap();
    {
        let store = Store::open(&f.dir.join("test.db")).unwrap();
        admission::pause_conversation(&store, "bot", &record.conversation_id, &record.attempt).unwrap();
        assert_eq!(admission::current(&store, "bot").unwrap().unwrap().phase, admission::Phase::Permitted);
        assert_eq!(notices(&store), 0);
    }
    let first = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until(|| admission::current(&first.store, "bot").unwrap().is_none()).await;
    assert!(admission::conversation_paused(&first.store, "bot", &record.conversation_id).unwrap());
    assert_eq!(notices(&first.store), 1);
    assert_eq!(f.prompts().len(), 1);
    first.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Estado terminal gravado pela rotina não prova a conclusão enquanto a cerca segue permitida: falha (timeout),
/// parada e sucesso sem confirmação da cerca também pausam a conversa no boot.
#[tokio::test]
async fn rotina_terminal_sem_prova_da_cerca_tambem_pausa() {
    for status in ["failed", "cancelled", "succeeded"] {
        let f = Fixture::new();
        std::fs::write(f.dir.join("blocked"), "").unwrap();
        std::fs::write(f.dir.join("hold"), "").unwrap();
        let routine = f
            .hub
            .routines
            .save(&f.hub, "bot", &json!({"name":"Rotina", "instruction":"Faça", "triggers":[{"type":"webhook"}]}))
            .unwrap();
        let id = routine["routine"]["id"].as_str().unwrap().to_owned();
        let run = f.enqueue(&id, "test", true).await;
        let run_id = run["run"]["id"].as_str().unwrap().to_owned();
        f.hub.send_cmd("bot", crate::agent::bot::Cmd::Routine(run_id)).unwrap();
        f.until(|| f.prompts().len() == 1).await;
        let record = admission::current(&f.hub.store, "bot").unwrap().unwrap();
        assert_eq!(record.phase, admission::Phase::Permitted);
        f.hub.shutdown().await;
        std::fs::remove_file(f.dir.join("hold")).unwrap();
        {
            // Queda entre gravar o estado da rotina e soltar a cerca.
            let store = Store::open(&f.dir.join("test.db")).unwrap();
            let mut state: Value = serde_json::from_str(&store.kv_get("routines.v1").unwrap()).unwrap();
            state["runs"][0]["status"] = json!(status);
            state["runs"][0]["finishedAt"] = json!(crate::store::now_ms());
            store.kv_set("routines.v1", &state.to_string()).unwrap();
        }
        let hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
        f.until(|| admission::current(&hub.store, "bot").unwrap().is_none()).await;
        assert!(admission::conversation_paused(&hub.store, "bot", &record.conversation_id).unwrap(), "{status}");
        assert_routine_notices(&hub.store, 0);
        assert_eq!(hub.routines.list("bot")["runs"][0]["status"], status);
        assert_eq!(f.prompts().len(), 1, "{status}: nada é repetido");
        hub.shutdown().await;
        let _ = std::fs::remove_dir_all(&f.dir);
    }
}
