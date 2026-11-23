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
    pending_from(dir, "Oi, sala", &json!({"caller":"local"}), delivery)
}

fn pending_from(dir: &std::path::Path, text: &str, ingress: &Value, delivery: Option<Value>) -> String {
    pending_with(dir, &json!({"text":text,"status":"sending","contentOrigin":{"ingress":ingress}}), delivery)
}

fn pending_with(dir: &std::path::Path, data: &Value, delivery: Option<Value>) -> String {
    let store = Store::open(&dir.join("test.db")).unwrap();
    let entry = store.insert_entry(&Lane::main(ROOM), EntryKind::User, 1, data).unwrap();
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
    let id = pending_from(&f.dir, "Do celular", &json!({"caller":"device","device":"celular-revogado"}), None);
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
    let uncertain = pending_from(&f.dir, "Primeira incerta", &local, Some(json!({"handed":["bot"],"done":[]})));
    let next = pending_from(&f.dir, "Segunda pendente", &local, None);
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

/// O Stop interrompe a mensagem na hora, fora dos prompts: mesmo que a sala grave o recibo depois e o Hub caia, ela
/// nunca roda no boot.
#[tokio::test]
async fn queda_no_stop_nao_retoma_a_mensagem_parada() {
    let f = Fixture::new();
    room(&f);
    f.hub.shutdown().await;
    let id = pending(&f.dir, None);
    crate::fork::grupo_entrega::stop(&f.hub, ROOM).unwrap();
    let stopped = f.hub.store.entry(&id).unwrap();
    assert_eq!(stopped.data["status"], "cancelled", "o Stop grava a interrupção junto");
    assert_eq!(stopped.data["groupReview"], true);
    let mut late = crate::fork::grupo_entrega::Delivery::new(&id);
    late.hand(&f.hub, ROOM, "outro").unwrap();
    let hub = reopen(&f.dir);
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(hub.store.entry(&id).unwrap().data["status"], "cancelled");
    assert_eq!(f.prompts().len(), 0, "a mensagem parada não é entregue");
    assert_eq!(reviews(&hub), 0);
    hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Envio real de um aparelho, revogado com o Hub parado: a origem gravada pela API é o aparelho, e o boot não o
/// retoma.
#[tokio::test]
async fn aparelho_revogado_com_o_hub_parado_nao_tem_a_mensagem_retomada() {
    use crate::store::{Device, DeviceSource, Scope};
    let f = Fixture::new();
    room(&f);
    std::fs::write(f.dir.join("hold"), "").unwrap();
    f.hub
        .store
        .put_device(&Device {
            key: "celular-grupo".into(),
            name: "Celular de teste".into(),
            platform: "fixture".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![Scope::Control],
            lease_until: None,
            created_at: crate::store::now_ms(),
            last_seen_at: None,
        })
        .unwrap();
    let (_, caller) = crate::fork::autoridade::autenticar(&f.hub, "celular-grupo").unwrap();
    let sent = crate::api::dispatch(&f.hub, &caller, "send", json!({"botId":ROOM,"text":"Do celular"})).await.unwrap();
    let id = sent["entry"]["id"].as_str().unwrap().to_owned();
    assert_eq!(sent["entry"]["data"]["contentOrigin"]["ingress"]["caller"], "device");
    f.until(|| f.prompts().len() == 1).await;
    f.hub.shutdown().await;
    std::fs::remove_file(f.dir.join("hold")).unwrap();
    {
        // A queda chega antes de a sala confirmar; a revogação, com o Hub parado.
        let store = Store::open(&f.dir.join("test.db")).unwrap();
        let mut entry = store.entry(&id).unwrap();
        entry.data["status"] = json!("sending");
        store.update_entry(&id, &entry.data).unwrap();
        store.kv_delete(&format!("donna.group.delivery.{id}")).unwrap();
    }
    assert!(f.hub.revoke_device("celular-grupo").unwrap());
    let hub = reopen(&f.dir);
    f.until(|| hub.store.entry(&id).unwrap().data["status"] == "cancelled").await;
    assert_eq!(reviews(&hub), 1);
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(f.prompts().len(), 1, "nada é entregue de novo");
    hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Dois membros e duas pendentes: só a mais nova é retomada, só a quem ainda não a recebeu; a anterior vai para
/// conferência e fica fora do prompt.
#[tokio::test]
async fn duas_pendentes_so_a_identificada_e_retomada_sem_lote() {
    let f = Fixture::new();
    room_with(&f, &["bot", "bot2"]);
    f.hub.shutdown().await;
    let local = json!({"caller":"local"});
    let older = pending_from(&f.dir, "Pendente anterior", &local, None);
    let newest =
        pending_from(&f.dir, "Pendente nova", &local, Some(json!({"handed":["bot"],"done":["bot"],"resumed":false})));
    let hub = reopen(&f.dir);
    f.until(|| hub.store.entry(&newest).unwrap().data["status"] == "sent").await;
    assert_eq!(hub.store.entry(&older).unwrap().data["status"], "cancelled");
    assert_eq!(reviews(&hub), 1);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(f.prompts().len(), 1, "só o segundo membro recebe, uma vez");
    let prompt = f.prompts()[0].to_string();
    assert!(prompt.contains("Pendente nova"));
    assert!(!prompt.contains("Pendente anterior"), "sem lote: a anterior não vai junto");
    hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Parada antes da queda e outra pendente depois: a parada fica interrompida, fora do prompt e sem aviso.
#[tokio::test]
async fn parada_nao_volta_no_prompt_da_proxima() {
    let f = Fixture::new();
    room(&f);
    f.hub.shutdown().await;
    let local = json!({"caller":"local"});
    let stopped = pending_from(&f.dir, "Mensagem parada", &local, None);
    crate::fork::grupo_entrega::stop(&f.hub, ROOM).unwrap();
    let next = pending_from(&f.dir, "Mensagem seguinte", &local, None);
    let hub = reopen(&f.dir);
    f.until(|| hub.store.entry(&next).unwrap().data["status"] == "sent").await;
    let saved = hub.store.entry(&stopped).unwrap();
    assert_eq!(saved.data["status"], "cancelled");
    assert_eq!(saved.data["groupReview"], true);
    assert_eq!(reviews(&hub), 0);
    let prompt = f.prompts()[0].to_string();
    assert!(prompt.contains("Mensagem seguinte"));
    assert!(!prompt.contains("Mensagem parada"));
    hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// A sala termina depois do Stop: a confirmação não sobrescreve a parada.
#[tokio::test]
async fn confirmacao_depois_do_stop_nao_vira_enviada() {
    let f = Fixture::new();
    room(&f);
    f.hub.shutdown().await;
    let id = pending(&f.dir, None);
    crate::fork::grupo_entrega::stop(&f.hub, ROOM).unwrap();
    crate::fork::grupo_entrega::Delivery::new(&id).confirm(&f.hub);
    let saved = f.hub.store.entry(&id).unwrap();
    assert_eq!(saved.data["status"], "cancelled");
    assert_eq!(saved.data["groupReview"], true);
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Uma resposta que cita a mensagem incerta dependeria de uma citação que o prompt omite: as duas vão para
/// conferência e nada é entregue.
#[tokio::test]
async fn resposta_a_incerta_vai_para_conferencia() {
    let f = Fixture::new();
    room(&f);
    f.hub.shutdown().await;
    let local = json!({"caller":"local"});
    let uncertain = pending_from(&f.dir, "Texto incerto", &local, Some(json!({"handed":["bot"],"done":[]})));
    let reply = pending_with(
        &f.dir,
        &json!({"text":"Respondendo","status":"sending","contentOrigin":{"ingress":local},
            "replyTo":{"id":uncertain,"author":"user","text":"Texto incerto"}}),
        None,
    );
    let hub = reopen(&f.dir);
    f.until(|| hub.store.entry(&reply).unwrap().data["status"] == "cancelled").await;
    assert_eq!(hub.store.entry(&uncertain).unwrap().data["status"], "cancelled");
    assert_eq!(reviews(&hub), 2);
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(f.prompts().len(), 0);
    hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// A retomada mostra só a mensagem identificada: uma anterior já enviada, que o membro passou sem responder, não volta.
#[tokio::test]
async fn retomada_mostra_so_a_mensagem_identificada() {
    let f = Fixture::new();
    room(&f);
    f.hub.shutdown().await;
    {
        let store = Store::open(&f.dir.join("test.db")).unwrap();
        store
            .insert_entry(&Lane::main(ROOM), EntryKind::User, 1, &json!({"text":"Antiga entregue","status":"sent"}))
            .unwrap();
    }
    let id = pending(&f.dir, None);
    let hub = reopen(&f.dir);
    f.until(|| hub.store.entry(&id).unwrap().data["status"] == "sent").await;
    let prompt = f.prompts()[0].to_string();
    assert!(prompt.contains("Oi, sala"));
    assert!(!prompt.contains("Antiga entregue"), "só a identificada é conteúdo da retomada");
    hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Linha ilegível numa sala: o Hub abre mesmo assim, e só essa sala fica bloqueada até o próximo reinício, sem
/// decidir nem entregar a pendente.
#[tokio::test]
async fn entrada_corrompida_bloqueia_so_a_sala_e_o_hub_abre() {
    let f = Fixture::new();
    room(&f);
    f.hub.shutdown().await;
    let stuck = pending(&f.dir, None);
    {
        let store = Store::open(&f.dir.join("test.db")).unwrap();
        let broken = store.insert_entry(&Lane::main(ROOM), EntryKind::User, 1, &json!({"text":"x"})).unwrap();
        store.db.locked().execute("UPDATE entries SET data = '{quebrado' WHERE id = ?1", [&broken.id]).unwrap();
    }
    let hub = reopen(&f.dir);
    assert!(hub.groups.blocked(ROOM));
    assert_eq!(hub.store.entry(&stuck).unwrap().data["status"], "sending", "a sala bloqueada não decide nem entrega");
    let refused = crate::api::dispatch(
        &hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId":ROOM,"text":"Mais uma"}),
    )
    .await;
    assert!(refused.is_err(), "envio novo à sala bloqueada é recusado");
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(f.prompts().len(), 0);
    hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Falha persistente ao marcar uma sala: ela fica bloqueada e as outras salas seguem com a própria retomada.
#[tokio::test]
async fn falha_ao_marcar_uma_sala_nao_segura_as_outras() {
    let f = Fixture::new();
    room(&f);
    let other: BotConfig =
        serde_json::from_value(json!({"id":"sala2","name":"Sala 2","kind":"group","members":["bot"]})).unwrap();
    f.hub.store.save_bot(&other).unwrap();
    f.hub.shutdown().await;
    let stuck = pending(&f.dir, Some(json!({"handed":["bot"],"done":[]})));
    let fine = {
        let store = Store::open(&f.dir.join("test.db")).unwrap();
        store
            .db
            .locked()
            .execute_batch(&format!(
                "CREATE TRIGGER reject_room BEFORE UPDATE ON entries WHEN OLD.bot_id = '{ROOM}'
                 BEGIN SELECT RAISE(ABORT, 'falha simulada'); END"
            ))
            .unwrap();
        let data = json!({"text":"Na outra sala","status":"sending","contentOrigin":{"ingress":{"caller":"local"}}});
        store.insert_entry(&Lane::main("sala2"), EntryKind::User, 1, &data).unwrap().id
    };
    let hub = reopen(&f.dir);
    assert!(hub.groups.blocked(ROOM));
    assert!(!hub.groups.blocked("sala2"));
    f.until(|| hub.store.entry(&fine).unwrap().data["status"] == "sent").await;
    assert_eq!(hub.store.entry(&stuck).unwrap().data["status"], "sending");
    assert!(f.prompts().iter().all(|p| !p.to_string().contains("Oi, sala")));
    hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}
