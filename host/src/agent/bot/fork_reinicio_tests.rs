//! Resposta cortada (GUI-203): desligar grava o último trecho, a varredura sem ator marca o corte e a ferramenta
//! aberta, erro do motor não promove o parcial a resposta final e gravação que falha é repetida.
#![allow(clippy::unwrap_used)]
use super::*;
use crate::LockExt;

fn fixture() -> (Arc<Hub>, Actor, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("reinicio-ator-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = crate::store::Store::open(std::path::Path::new(":memory:")).unwrap();
    let cfg: BotConfig =
        serde_json::from_value(json!({"id": "b", "name": "b", "backend": "claude", "cwd": "/tmp"})).unwrap();
    store.save_bot(&cfg).unwrap();
    let identity = crate::remote::identity::Identity::load_or_create(&dir).unwrap();
    let hub = Hub::new(store, "fixture".into(), identity, "fixture".into(), 0);
    let mut actor = new_actor(hub.clone(), hub.store.bot("b").unwrap().unwrap().config);
    actor.lane = Lane::main("b");
    actor.turn = Some(1);
    (hub, actor, dir)
}

fn reply(hub: &Hub, actor: &Actor) -> Value {
    hub.store.entry(actor.last_text.as_deref().unwrap()).unwrap().data
}

#[tokio::test]
async fn desligar_grava_o_ultimo_trecho_e_a_varredura_marca_o_corte() {
    let (hub, mut actor, dir) = fixture();
    actor.on_update(
        &json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Long", "status": "in_progress"}),
    );
    actor.append(SegKind::Text, "primeiro ");
    actor.append(SegKind::Text, "e o fim");
    let id = actor.last_text.clone().unwrap();
    assert_eq!(reply(&hub, &actor)["text"], "primeiro ", "o fim ainda só está na memória");
    let (done, _rx) = mpsc::unbounded_channel();
    assert!(!actor.on_cmd(Cmd::Shutdown, &done).await, "o ator sai do laço");
    let saved = hub.store.entry(&id).unwrap().data;
    assert_eq!(saved["text"], "primeiro e o fim", "nada se perde no desligamento normal");
    assert_eq!(saved["streaming"], true);

    assert!(hub.store.interrupt_open_work().unwrap() > 0);
    let cut = hub.store.entry(&id).unwrap().data;
    assert_eq!(
        (cut["text"].clone(), cut["final"].clone(), cut["interrupted"].clone()),
        (json!("primeiro e o fim"), json!(false), json!(true))
    );
    assert!(cut.get("streaming").is_none());
    let tool = hub.store.entry(&actor.tools["t1"]).unwrap().data;
    assert_eq!((tool["status"].clone(), tool["interrupted"].clone()), (json!("failed"), json!(true)));
    assert_eq!(hub.store.interrupt_open_work().unwrap(), 0, "a varredura não repete");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn texto_fechado_antes_da_ferramenta_que_nao_terminou_tambem_fica_interrompido() {
    let (hub, mut actor, dir) = fixture();
    actor.append(SegKind::Text, "Vou olhar o arquivo.");
    let id = actor.last_text.clone().unwrap();
    actor.on_update(&json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Ler", "status": "pending"}));
    assert!(hub.store.entry(&id).unwrap().data.get("streaming").is_none(), "o trecho fechou antes da ferramenta");
    hub.store.interrupt_open_work().unwrap();
    let data = hub.store.entry(&id).unwrap().data;
    assert_eq!((data["interrupted"].clone(), data["final"].clone()), (json!(true), json!(false)), "{data}");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn texto_antes_de_aprovacao_sem_resposta_fica_interrompido_e_o_cartao_segue_para_expirar() {
    let (hub, mut actor, dir) = fixture();
    actor.append(SegKind::Text, "Posso apagar a pasta?");
    let id = actor.last_text.clone().unwrap();
    actor.close_seg();
    let card = hub
        .store
        .insert_entry(&Lane::main("b"), EntryKind::Permission, 1, &json!({"title": "Apagar", "status": "pending"}))
        .unwrap();
    hub.store.interrupt_open_work().unwrap();
    assert_eq!(hub.store.entry(&id).unwrap().data["interrupted"], true);
    assert_eq!(hub.store.entry(&card.id).unwrap().data["status"], "pending", "o cartão fica para expire_pending");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn fim_que_nao_chegou_ao_disco_nao_promove_a_versao_antiga() {
    let (hub, mut actor, dir) = fixture();
    actor.append(SegKind::Text, "metade ");
    actor.append(SegKind::Text, "e o resto");
    let id = actor.last_text.clone().unwrap();
    hub.store
        .db
        .locked()
        .execute_batch(
            "CREATE TRIGGER sem_disco BEFORE UPDATE ON entries BEGIN SELECT RAISE(FAIL, 'disco cheio'); END;",
        )
        .unwrap();
    actor.finish_turn(&Ok(json!({"stopReason": "end_turn"})));
    hub.store.db.locked().execute_batch("DROP TRIGGER sem_disco;").unwrap();
    let data = hub.store.entry(&id).unwrap().data;
    assert_eq!(data["text"], "metade ");
    assert_ne!(data["final"], true, "texto velho não vira resposta final: {data}");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn resposta_fechada_e_texto_legado_ficam_fora_da_varredura() {
    let (hub, mut actor, dir) = fixture();
    actor.append(SegKind::Text, "antes da ferramenta");
    let closed = actor.last_text.clone().unwrap();
    actor.close_seg();
    assert!(hub.store.entry(&closed).unwrap().data.get("streaming").is_none(), "fechar tira o sinal");
    let legacy = hub
        .store
        .insert_entry(&Lane::main("b"), EntryKind::Agent, 1, &json!({"text": "velho", "final": false}))
        .unwrap();
    let done = hub.store.insert_entry(&Lane::main("b"), EntryKind::Tool, 1, &json!({"status": "completed"})).unwrap();
    assert_eq!(hub.store.interrupt_open_work().unwrap(), 0);
    for id in [&closed, &legacy.id, &done.id] {
        assert!(hub.store.entry(id).unwrap().data.get("interrupted").is_none());
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn erro_do_motor_nao_promove_o_parcial_a_resposta_final() {
    let (hub, mut actor, dir) = fixture();
    actor.append(SegKind::Text, "metade da respo");
    let id = actor.last_text.clone().unwrap();
    actor.finish_turn(&Err(anyhow!("conexão caiu")));
    let data = hub.store.entry(&id).unwrap().data;
    assert_ne!(data["final"], true, "{data}");
    assert_eq!(data["interrupted"], true, "{data}");
    assert_eq!(data["text"], "metade da respo");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn resposta_completa_continua_virando_final() {
    let (hub, mut actor, dir) = fixture();
    actor.append(SegKind::Text, "pronto");
    let id = actor.last_text.clone().unwrap();
    actor.finish_turn(&Ok(json!({"stopReason": "end_turn"})));
    let data = hub.store.entry(&id).unwrap().data;
    assert_eq!(data["final"], true, "{data}");
    assert!(data.get("interrupted").is_none() && data.get("streaming").is_none(), "{data}");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn gravacao_periodica_que_falha_e_repetida_sem_perder_o_fim() {
    let (hub, mut actor, dir) = fixture();
    actor.append(SegKind::Text, "a");
    actor.append(SegKind::Text, "b");
    let id = actor.last_text.clone().unwrap();
    hub.store
        .db
        .locked()
        .execute_batch(
            "CREATE TRIGGER sem_disco BEFORE UPDATE ON entries BEGIN SELECT RAISE(FAIL, 'disco cheio'); END;",
        )
        .unwrap();
    actor.flush(true);
    assert_eq!(hub.store.entry(&id).unwrap().data["text"], "a");
    hub.store.db.locked().execute_batch("DROP TRIGGER sem_disco;").unwrap();
    actor.flush(true);
    assert_eq!(hub.store.entry(&id).unwrap().data["text"], "ab", "o trecho que falhou é gravado na próxima vez");
    let _ = std::fs::remove_dir_all(dir);
}
