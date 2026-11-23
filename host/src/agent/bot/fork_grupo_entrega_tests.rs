//! Mensagem de grupo pendente no reinício: retomada uma única vez, sem entregar de novo a quem já recebeu, ou marcada
//! para conferência quando a entrega ficou incerta; reinícios seguintes não repetem nada.
use super::*;
use crate::store::Lane;

const ROOM: &str = "sala";

fn room(f: &Fixture) {
    std::fs::write(f.dir.join("blocked"), "").unwrap();
    let cfg: BotConfig =
        serde_json::from_value(json!({"id":ROOM,"name":"Sala","kind":"group","members":["bot"]})).unwrap();
    f.hub.store.save_bot(&cfg).unwrap();
}

/// Simula a queda: a linha do dono gravada como pendente e, opcionalmente, o que a sala já tinha registrado.
fn pending(dir: &std::path::Path, delivery: Option<Value>) -> String {
    let store = Store::open(&dir.join("test.db")).unwrap();
    let entry = store
        .insert_entry(&Lane::main(ROOM), EntryKind::User, 1, &json!({"text":"Oi, sala","status":"sending"}))
        .unwrap();
    if let Some(delivery) = delivery {
        store.kv_set(&format!("donna.group.delivery.{}", entry.id), &delivery.to_string()).unwrap();
    }
    entry.id
}

fn reopen(dir: &std::path::Path) -> Arc<Hub> {
    Fixture::open(dir, Store::open(&dir.join("test.db")).unwrap())
}

fn reviews(hub: &Hub) -> usize {
    hub.store
        .history(ROOM, i64::MAX, 100)
        .unwrap()
        .iter()
        .filter(|e| {
            e.kind == "notice" && e.data["text"].as_str().is_some_and(|t| t.contains("confira antes de reenviar"))
        })
        .count()
}

#[tokio::test]
async fn mensagem_pendente_sem_entrega_e_retomada_uma_vez() {
    let f = Fixture::new();
    room(&f);
    f.hub.shutdown().await;
    let id = pending(&f.dir, None);
    let hub = reopen(&f.dir);
    f.until(|| hub.store.entry(&id).unwrap().data["status"] == "sent").await;
    assert!(f.prompts()[0].to_string().contains("Oi, sala"), "o membro recebe a mensagem");
    assert!(hub.store.kv_get(&format!("donna.group.delivery.{id}")).is_none());
    assert_eq!(reviews(&hub), 0);
    // As rodadas seguintes da sala são conversa nova entre membros; espera a sala assentar.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    hub.shutdown().await;
    let settled = f.prompts().len();
    let again = reopen(&f.dir);
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(f.prompts().len(), settled, "o segundo reinício não entrega de novo");
    again.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn quem_ja_recebeu_nao_recebe_de_novo() {
    let f = Fixture::new();
    room(&f);
    f.hub.shutdown().await;
    let id = pending(&f.dir, Some(json!({"handed":["bot"],"done":["bot"],"resumed":false})));
    let hub = reopen(&f.dir);
    f.until(|| hub.store.entry(&id).unwrap().data["status"] == "sent").await;
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(f.prompts().len(), 0, "o membro que já recebeu fica de fora da retomada");
    assert_eq!(reviews(&hub), 0);
    hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn entrega_incerta_ou_retomada_que_caiu_vira_conferencia_sem_repetir() {
    for delivery in [
        json!({"handed":["bot"],"done":[],"resumed":false}),
        json!({"handed":[],"done":[],"resumed":true}),
        json!("ilegível"),
    ] {
        let f = Fixture::new();
        room(&f);
        f.hub.shutdown().await;
        let id = pending(&f.dir, Some(delivery.clone()));
        let hub = reopen(&f.dir);
        f.until(|| hub.store.entry(&id).unwrap().data["status"] == "sent").await;
        assert_eq!(reviews(&hub), 1, "{delivery}");
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(f.prompts().len(), 0, "nada é entregue de novo: {delivery}");
        hub.shutdown().await;
        let again = reopen(&f.dir);
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(reviews(&again), 1, "o aviso não se repete: {delivery}");
        assert_eq!(f.prompts().len(), 0);
        again.shutdown().await;
        let _ = std::fs::remove_dir_all(&f.dir);
    }
}

#[tokio::test]
async fn envio_ao_grupo_nasce_pendente_e_so_confirma_depois_da_primeira_rodada() {
    let f = Fixture::new();
    room(&f);
    std::fs::write(f.dir.join("hold"), "").unwrap();
    let sent = crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId":ROOM,"text":"Oi, sala"}),
    )
    .await
    .unwrap();
    let id = sent["entry"]["id"].as_str().unwrap().to_owned();
    assert_eq!(sent["entry"]["data"]["status"], "sending");
    f.until(|| f.prompts().len() == 1).await;
    assert_eq!(f.hub.store.entry(&id).unwrap().data["status"], "sending", "o membro ainda não terminou");
    let saved: Value =
        serde_json::from_str(&f.hub.store.kv_get(&format!("donna.group.delivery.{id}")).unwrap()).unwrap();
    assert_eq!(saved["handed"], json!(["bot"]));
    std::fs::remove_file(f.dir.join("hold")).unwrap();
    f.until(|| f.hub.store.entry(&id).unwrap().data["status"] == "sent").await;
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}
