//! Mensagem de grupo pendente no reinício: retomada uma única vez, numa rodada e sem entregar de novo a quem já
//! recebeu, ou interrompida para conferência (fora dos prompts da sala) quando não há prova; reinícios seguintes não
//! repetem nada.
use super::*;
use crate::store::Lane;

const ROOM: &str = "sala";

fn room(f: &Fixture) {
    room_with(f, &["bot"]);
}

fn room_with(f: &Fixture, members: &[&str]) {
    std::fs::write(f.dir.join("blocked"), "").unwrap();
    let bot = f.hub.store.bot("bot").unwrap().unwrap().config;
    for id in members.iter().filter(|id| **id != "bot") {
        let mut other = bot.clone();
        (*id).clone_into(&mut other.id);
        f.hub.store.save_bot(&other).unwrap();
    }
    let cfg: BotConfig =
        serde_json::from_value(json!({"id":ROOM,"name":"Sala","kind":"group","members":members})).unwrap();
    f.hub.store.save_bot(&cfg).unwrap();
}

/// Simula a queda: a linha do dono, enviada deste computador, gravada como pendente e, opcionalmente, o que a sala
/// já tinha registrado.
fn pending(dir: &std::path::Path, delivery: Option<Value>) -> String {
    pending_from(dir, "Oi, sala", json!({"caller":"local"}), delivery)
}

fn pending_from(dir: &std::path::Path, text: &str, ingress: Value, delivery: Option<Value>) -> String {
    let store = Store::open(&dir.join("test.db")).unwrap();
    let data = json!({"text":text,"status":"sending","contentOrigin":{"ingress":ingress}});
    let entry = store.insert_entry(&Lane::main(ROOM), EntryKind::User, 1, &data).unwrap();
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
        f.until(|| hub.store.entry(&id).unwrap().data["status"] == "cancelled").await;
        assert_eq!(hub.store.entry(&id).unwrap().data["groupReview"], true, "incerta nunca vira enviada: {delivery}");
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

/// Aparelho revogado com o Hub parado: o boot não tem prova atual da autoridade de quem enviou, então a mensagem não
/// é retomada; fica interrompida para conferência.
#[tokio::test]
async fn mensagem_de_aparelho_nao_e_retomada_sem_prova_atual_de_autoridade() {
    let f = Fixture::new();
    room(&f);
    f.hub.shutdown().await;
    let id = pending_from(&f.dir, "Do celular", json!({"caller":"device","device":"celular-revogado"}), None);
    let hub = reopen(&f.dir);
    f.until(|| hub.store.entry(&id).unwrap().data["status"] == "cancelled").await;
    assert_eq!(reviews(&hub), 1);
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(f.prompts().len(), 0, "nada é entregue");
    hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Dois membros, um já recebeu e respondeu: só o outro recebe, numa rodada só, e outro boot não repete nada.
#[tokio::test]
async fn dois_membros_so_quem_nao_recebeu_recebe_uma_vez() {
    let f = Fixture::new();
    room_with(&f, &["bot", "bot2"]);
    f.hub.shutdown().await;
    let id = pending(&f.dir, Some(json!({"handed":["bot"],"done":["bot"],"resumed":false})));
    let hub = reopen(&f.dir);
    f.until(|| hub.store.entry(&id).unwrap().data["status"] == "sent").await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(f.prompts().len(), 1, "só o segundo membro recebe, e uma vez");
    assert!(f.prompts()[0].to_string().contains("Oi, sala"));
    hub.shutdown().await;
    let again = reopen(&f.dir);
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(f.prompts().len(), 1, "o segundo boot não entrega de novo");
    again.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Uma incerta ao lado de uma pendente: a incerta fica para conferência e fora do prompt; só a pendente é entregue.
#[tokio::test]
async fn incerta_ao_lado_de_pendente_fica_fora_do_prompt() {
    let f = Fixture::new();
    room(&f);
    f.hub.shutdown().await;
    let local = json!({"caller":"local"});
    let uncertain = pending_from(&f.dir, "Primeira incerta", local.clone(), Some(json!({"handed":["bot"],"done":[]})));
    let next = pending_from(&f.dir, "Segunda pendente", local, None);
    let hub = reopen(&f.dir);
    f.until(|| hub.store.entry(&next).unwrap().data["status"] == "sent").await;
    assert_eq!(hub.store.entry(&uncertain).unwrap().data["status"], "cancelled");
    assert_eq!(reviews(&hub), 1);
    let prompt = f.prompts()[0].to_string();
    assert!(prompt.contains("Segunda pendente"));
    assert!(!prompt.contains("Primeira incerta"), "a incerta não volta a nenhum membro");
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(f.prompts().len(), 1);
    hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Queda logo depois de o Stop ser gravado: mesmo que a sala grave o recibo depois, a mensagem parada nunca roda no
/// boot.
#[tokio::test]
async fn queda_no_stop_nao_retoma_a_mensagem_parada() {
    let f = Fixture::new();
    room(&f);
    f.hub.shutdown().await;
    let id = pending(&f.dir, None);
    crate::fork::grupo_entrega::stop(&f.hub, ROOM).unwrap();
    let mut late = crate::fork::grupo_entrega::Delivery::new(&id);
    late.hand(&f.hub, ROOM, "outro").unwrap();
    let saved: Value =
        serde_json::from_str(&f.hub.store.kv_get(&format!("donna.group.delivery.{id}")).unwrap()).unwrap();
    assert_eq!(saved["stopped"], true, "a gravação da sala não desfaz o Stop");
    let hub = reopen(&f.dir);
    f.until(|| hub.store.entry(&id).unwrap().data["status"] == "cancelled").await;
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(f.prompts().len(), 0, "a mensagem parada não é entregue");
    assert_eq!(reviews(&hub), 0);
    hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}
