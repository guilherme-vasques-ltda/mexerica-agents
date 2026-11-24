//! Mensagem de grupo pendente no reinício: retomada uma única vez, numa rodada e sem entregar de novo a quem já
//! recebeu, ou interrompida para conferência (fora dos prompts da sala) quando não há prova; reinícios seguintes não
//! repetem nada.
use super::*;
use crate::agent::bot::Cmd;
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
    f.until("mensagem_pendente_sem_entrega_e_retomada_uma_vez", &hub, || {
        hub.store.entry(&id).unwrap().data["status"] == "sent"
    })
    .await;
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
    f.until("quem_ja_recebeu_nao_recebe_de_novo", &hub, || hub.store.entry(&id).unwrap().data["status"] == "sent")
        .await;
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
        f.until("entrega_incerta_ou_retomada_que_caiu_vira_conferencia_sem_repetir", &hub, || {
            hub.store.entry(&id).unwrap().data["status"] == "cancelled"
        })
        .await;
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
    f.until("envio_ao_grupo_nasce_pendente_e_so_confirma_depois_da_primeira_rodada", &f.hub, || f.prompts().len() == 1)
        .await;
    assert_eq!(f.hub.store.entry(&id).unwrap().data["status"], "sending", "o membro ainda não terminou");
    let saved: Value =
        serde_json::from_str(&f.hub.store.kv_get(&format!("donna.group.delivery.{id}")).unwrap()).unwrap();
    assert_eq!(saved["handed"], json!(["bot"]));
    std::fs::remove_file(f.dir.join("hold")).unwrap();
    f.until("envio_ao_grupo_nasce_pendente_e_so_confirma_depois_da_primeira_rodada", &f.hub, || {
        f.hub.store.entry(&id).unwrap().data["status"] == "sent"
    })
    .await;
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
    f.until("mensagem_de_aparelho_nao_e_retomada_sem_prova_atual_de_autoridade", &hub, || {
        hub.store.entry(&id).unwrap().data["status"] == "cancelled"
    })
    .await;
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
    f.until("dois_membros_so_quem_nao_recebeu_recebe_uma_vez", &hub, || {
        hub.store.entry(&id).unwrap().data["status"] == "sent"
    })
    .await;
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
    f.until("incerta_ao_lado_de_pendente_fica_fora_do_prompt", &hub, || {
        hub.store.entry(&next).unwrap().data["status"] == "sent"
    })
    .await;
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
    f.until("aparelho_revogado_com_o_hub_parado_nao_tem_a_mensagem_retomada", &f.hub, || f.prompts().len() == 1).await;
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
    f.until("aparelho_revogado_com_o_hub_parado_nao_tem_a_mensagem_retomada", &hub, || {
        hub.store.entry(&id).unwrap().data["status"] == "cancelled"
    })
    .await;
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
    f.until("duas_pendentes_so_a_identificada_e_retomada_sem_lote", &hub, || {
        hub.store.entry(&newest).unwrap().data["status"] == "sent"
    })
    .await;
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
    f.until("parada_nao_volta_no_prompt_da_proxima", &hub, || hub.store.entry(&next).unwrap().data["status"] == "sent")
        .await;
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
    crate::fork::grupo_entrega::Delivery::new(&id).confirm(&f.hub, ROOM);
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
    f.until("resposta_a_incerta_vai_para_conferencia", &hub, || {
        hub.store.entry(&reply).unwrap().data["status"] == "cancelled"
    })
    .await;
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
    f.until("retomada_mostra_so_a_mensagem_identificada", &hub, || {
        hub.store.entry(&id).unwrap().data["status"] == "sent"
    })
    .await;
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
    .await
    .expect_err("envio novo à sala bloqueada é recusado");
    assert_eq!(crate::api::error_code(&refused), Some("group_blocked"));
    let notices: i64 = hub
        .store
        .db
        .locked()
        .query_row(
            "SELECT COUNT(*) FROM entries WHERE bot_id = ?1 AND kind = 'notice' AND data LIKE '%\"code\":\"group_blocked\"%'",
            [ROOM],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(notices, 1, "o aviso da sala bloqueada leva o código para o app traduzir");
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
    f.until("falha_ao_marcar_uma_sala_nao_segura_as_outras", &hub, || {
        hub.store.entry(&fine).unwrap().data["status"] == "sent"
    })
    .await;
    assert_eq!(hub.store.entry(&stuck).unwrap().data["status"], "sending");
    assert!(f.prompts().iter().all(|p| !p.to_string().contains("Oi, sala")));
    hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Uma citação cuja linha ficou ilegível não conta como comprovada: nem na retomada do boot nem no prompt da sala.
#[test]
fn citacao_ilegivel_nao_conta_como_comprovada() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let quoted = store
        .insert_entry(&Lane::main(ROOM), EntryKind::Agent, 1, &json!({"text":"x","final":true,"author":"bot"}))
        .unwrap();
    assert!(!crate::fork::grupo_entrega::unproven(&store.entry(&quoted.id).unwrap()));
    store.db.locked().execute("UPDATE entries SET data = '{quebrado' WHERE id = ?1", [&quoted.id]).unwrap();
    assert!(crate::fork::grupo_entrega::unproven(&store.entry(&quoted.id).unwrap()));
}

fn pending_in(dir: &std::path::Path, lane: &Lane, text: &str) -> String {
    let store = Store::open(&dir.join("test.db")).unwrap();
    let data = json!({"text":text,"status":"sending","contentOrigin":{"ingress":{"caller":"local"}}});
    store.insert_entry(lane, EntryKind::User, 1, &data).unwrap().id
}

fn session(id: &str) -> Lane {
    Lane { session: Some(id.to_owned()), chat: ROOM.to_owned() }
}

/// O envio a um membro falha (o ator dele não está de pé, como no desligamento): a mensagem não vira enviada, o
/// membro não conta como quem recebeu, e o boot entrega só a ele.
#[tokio::test]
async fn envio_que_falha_nao_confirma_e_o_boot_entrega_so_a_quem_faltou() {
    let f = Fixture::new();
    room_with(&f, &["bot", "bot2"]);
    let sent = crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId":ROOM,"text":"Oi, sala"}),
    )
    .await
    .unwrap();
    let id = sent["entry"]["id"].as_str().unwrap().to_owned();
    let saved = || -> Value {
        serde_json::from_str(&f.hub.store.kv_get(&format!("donna.group.delivery.{id}")).unwrap_or_default())
            .unwrap_or_default()
    };
    f.until("envio_que_falha_nao_confirma_e_o_boot_entrega_so_a_quem_faltou", &f.hub, || {
        saved()["done"] == json!(["bot"])
    })
    .await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(f.hub.store.entry(&id).unwrap().data["status"], "sending", "quem não recebeu não vira enviada");
    assert_eq!(saved()["handed"], json!(["bot"]), "o envio que falhou não conta como recebido");
    f.hub.shutdown().await;
    let before = f.prompts().len();
    let hub = reopen(&f.dir);
    f.until("envio_que_falha_nao_confirma_e_o_boot_entrega_so_a_quem_faltou", &hub, || {
        hub.store.entry(&id).unwrap().data["status"] == "sent"
    })
    .await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(f.prompts().len(), before + 1, "só quem faltou recebe, uma vez");
    assert!(f.prompts()[before].to_string().contains("Oi, sala"));
    assert_eq!(reviews(&hub), 0);
    hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Pendentes em duas sessões do mesmo grupo: cada retomada tem o próprio epoch, e nenhuma encerra a outra nem a
/// confirma sem entrega. Com o membro preso no primeiro turno, as duas já foram passadas a ele e seguem pendentes.
#[tokio::test]
async fn retomadas_em_duas_sessoes_do_grupo_passam_as_duas_ao_membro() {
    let f = Fixture::new();
    room(&f);
    f.hub.shutdown().await;
    let ids = [
        pending_in(&f.dir, &Lane::main(ROOM), "Na conversa principal"),
        pending_in(&f.dir, &session("s0"), "Na outra sessão"),
    ];
    std::fs::write(f.dir.join("hold"), "").unwrap();
    let hub = reopen(&f.dir);
    let handed = |id: &str| -> Value {
        hub.store
            .kv_get(&format!("donna.group.delivery.{id}"))
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .map_or(Value::Null, |saved| saved["handed"].clone())
    };
    f.until("retomadas_em_duas_sessoes_do_grupo_passam_as_duas_ao_membro", &hub, || {
        ids.iter().all(|id| handed(id) == json!(["bot"]))
    })
    .await;
    for id in &ids {
        assert_eq!(hub.store.entry(id).unwrap().data["status"], "sending", "nenhuma retomada confirma a outra");
    }
    std::fs::remove_file(f.dir.join("hold")).unwrap();
    f.until("retomadas_em_duas_sessoes_do_grupo_passam_as_duas_ao_membro", &hub, || {
        ids.iter().all(|id| hub.store.entry(id).unwrap().data["status"] == "sent")
    })
    .await;
    assert!(f.prompts().iter().any(|p| p.to_string().contains("Na conversa principal")));
    hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// O Stop do grupo encerra a rodada em todas as sessões dele, e só nele.
#[tokio::test]
async fn stop_do_grupo_encerra_todas_as_sessoes_dele() {
    let f = Fixture::new();
    room(&f);
    let lanes = [Lane::main(ROOM), session("s0")];
    let epochs: Vec<u64> = lanes.iter().map(|lane| f.hub.groups.reserve_epoch_for_test(lane)).collect();
    let elsewhere = Lane::main("sala2");
    let kept = f.hub.groups.reserve_epoch_for_test(&elsewhere);
    for (lane, epoch) in lanes.iter().zip(&epochs) {
        assert!(f.hub.groups.current(lane, *epoch), "uma sessão não encerra a outra: {lane:?}");
    }
    f.hub.groups.stop(&f.hub, ROOM);
    for (lane, epoch) in lanes.iter().zip(epochs) {
        assert!(!f.hub.groups.current(lane, epoch), "{lane:?}");
    }
    assert!(f.hub.groups.current(&elsewhere, kept), "outro grupo segue");
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Stop com um membro no meio do turno: a mensagem fica parada, quem não recebeu não recebe, e a rodada encerrada
/// não a confirma como enviada.
#[tokio::test]
async fn stop_no_meio_da_rodada_nao_confirma_sem_entrega() {
    let f = Fixture::new();
    room_with(&f, &["bot", "bot2"]);
    f.hub.shutdown().await;
    let hub = reopen(&f.dir);
    std::fs::write(f.dir.join("hold"), "").unwrap();
    let local = crate::api::devices::Caller::Local;
    let sent = crate::api::dispatch(&hub, &local, "send", json!({"botId":ROOM,"text":"Oi, sala"})).await.unwrap();
    let id = sent["entry"]["id"].as_str().unwrap().to_owned();
    f.until("stop_no_meio_da_rodada_nao_confirma_sem_entrega", &f.hub, || f.prompts().len() == 1).await;
    crate::api::dispatch(&hub, &local, "stop", json!({"botId":ROOM})).await.unwrap();
    assert_eq!(hub.store.entry(&id).unwrap().data["status"], "cancelled");
    std::fs::remove_file(f.dir.join("hold")).unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let saved = hub.store.entry(&id).unwrap();
    assert_eq!(saved.data["status"], "cancelled", "a rodada encerrada não confirma");
    assert_eq!(saved.data["groupReview"], true);
    assert_eq!(f.prompts().len(), 1, "o segundo membro não recebe depois do Stop");
    hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Uma sessão retomável e outra que falha ao ser classificada depois dela: a sala fica bloqueada sem retomar nada.
#[tokio::test]
async fn sala_que_falha_no_meio_da_classificacao_nao_retoma_nada() {
    let f = Fixture::new();
    room(&f);
    f.hub.shutdown().await;
    let resumable = pending(&f.dir, None);
    let uncertain = pending_in(&f.dir, &session("sb"), "Incerta");
    {
        let store = Store::open(&f.dir.join("test.db")).unwrap();
        let delivery = json!({"handed":["bot"],"done":[]});
        store.kv_set(&format!("donna.group.delivery.{uncertain}"), &delivery.to_string()).unwrap();
        store
            .db
            .locked()
            .execute_batch(
                "CREATE TRIGGER reject_session BEFORE UPDATE ON entries WHEN OLD.session_id = 'sb'
                 BEGIN SELECT RAISE(ABORT, 'falha simulada'); END",
            )
            .unwrap();
    }
    let hub = reopen(&f.dir);
    assert!(hub.groups.blocked(ROOM));
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(f.prompts().len(), 0, "nada é entregue numa sala bloqueada");
    assert_eq!(hub.store.entry(&resumable).unwrap().data["status"], "sending");
    hub.shutdown().await;
    // Corrigida a falha, o boot seguinte ainda retoma a mensagem: a sala bloqueada não gastou a retomada.
    Store::open(&f.dir.join("test.db")).unwrap().db.locked().execute_batch("DROP TRIGGER reject_session").unwrap();
    let again = reopen(&f.dir);
    f.until("sala_que_falha_no_meio_da_classificacao_nao_retoma_nada", &again, || {
        again.store.entry(&resumable).unwrap().data["status"] == "sent"
    })
    .await;
    assert!(f.prompts()[0].to_string().contains("Oi, sala"));
    again.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn stop_gravado_cancela_antes_da_proxima_rodada_sem_bloquear_a_sala() {
    let f = Fixture::new();
    room(&f);
    let mut bot = f.hub.test_bot_commands("bot");
    let local = crate::api::devices::Caller::Local;
    let send = |text| crate::api::dispatch(&f.hub, &local, "send", json!({"botId": ROOM, "text": text}));
    let first = send("Primeira").await.unwrap();
    let first_id = first["entry"]["id"].as_str().unwrap();
    let Some(Cmd::Group(turn)) = tokio::time::timeout(Duration::from_secs(5), bot.recv()).await.unwrap() else {
        panic!("a primeira rodada chegou ao membro");
    };
    crate::api::dispatch(&f.hub, &local, "stop", json!({"botId": ROOM})).await.unwrap();
    assert!(!f.hub.groups.blocked(ROOM));
    assert_eq!(f.hub.store.entry(first_id).unwrap().data["status"], "cancelled");

    let next = send("Segunda").await.unwrap();
    let next_id = next["entry"]["id"].as_str().unwrap();
    assert!(matches!(bot.try_recv(), Ok(Cmd::CancelGroup { chat }) if chat == ROOM));
    drop(turn.reply);
    let Some(Cmd::Group(next_turn)) = tokio::time::timeout(Duration::from_secs(5), bot.recv()).await.unwrap() else {
        panic!("a rodada depois do Stop chegou ao membro");
    };
    assert_eq!(next_turn.epoch, turn.epoch + 2, "o Stop e a nova rodada avançam o epoch");
    assert!(f.hub.groups.current(&next_turn.lane, next_turn.epoch));
    next_turn.reply.send(Ok(None)).unwrap();
    f.until("rodada depois do Stop", &f.hub, || f.hub.store.entry(next_id).unwrap().data["status"] == "sent").await;
    assert_eq!(f.hub.store.entry(first_id).unwrap().data["status"], "cancelled");
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn stop_cancela_membro_ja_enfileirado_mesmo_sem_ler_o_grupo() {
    let f = Fixture::new();
    room(&f);
    let mut bot = f.hub.test_bot_commands("bot");
    crate::api::dispatch(
        &f.hub,
        &crate::api::devices::Caller::Local,
        "send",
        json!({"botId": ROOM, "text": "Oi, sala"}),
    )
    .await
    .unwrap();
    let Some(Cmd::Group(turn)) = tokio::time::timeout(Duration::from_secs(5), bot.recv()).await.unwrap() else {
        panic!("a rodada chegou ao membro");
    };
    f.hub.store.db.locked().execute("UPDATE bots SET config = x'00' WHERE id = ?1", [ROOM]).unwrap();
    assert!(f.hub.store.bot(ROOM).is_err());
    crate::fork::grupo_entrega::stop(&f.hub, ROOM).unwrap();
    assert!(matches!(bot.try_recv(), Ok(Cmd::CancelGroup { chat }) if chat == ROOM));
    drop(turn.reply);
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn stop_que_falha_bloqueia_a_sala_e_deixa_a_classificacao_para_o_boot() {
    let f = Fixture::new();
    room_with(&f, &["bot", "bot2"]);
    let mut bot = f.hub.test_bot_commands("bot");
    let mut bot2 = f.hub.test_bot_commands("bot2");
    let local = crate::api::devices::Caller::Local;
    let body = json!({"botId": ROOM, "text": "Oi, sala"});
    let sent = crate::api::dispatch(&f.hub, &local, "send", body.clone()).await.unwrap();
    let id = sent["entry"]["id"].as_str().unwrap();
    let Some(Cmd::Group(turn)) = tokio::time::timeout(Duration::from_secs(5), bot.recv()).await.unwrap() else {
        panic!("o primeiro membro recebeu a mensagem");
    };
    let lane = Lane::main(ROOM);
    assert!(f.hub.groups.current(&lane, turn.epoch));
    f.hub
        .store
        .db
        .locked()
        .execute_batch(&format!(
            "CREATE TRIGGER reject_stop BEFORE UPDATE ON entries
             WHEN OLD.bot_id = '{ROOM}' AND json_extract(NEW.data, '$.status') = 'cancelled'
             BEGIN SELECT RAISE(ABORT, 'falha simulada'); END"
        ))
        .unwrap();
    crate::api::dispatch(&f.hub, &local, "stop", json!({"botId": ROOM}))
        .await
        .expect_err("a falha ao gravar chega a quem pediu o Stop");
    assert!(!f.hub.groups.current(&lane, turn.epoch));
    assert_eq!(f.hub.groups.reserve_epoch_for_test(&lane), turn.epoch + 1, "o Stop falho não avança o epoch");
    assert!(matches!(bot.try_recv(), Ok(Cmd::CancelGroup { chat }) if chat == ROOM));
    assert!(matches!(bot2.try_recv(), Ok(Cmd::CancelGroup { chat }) if chat == ROOM));

    turn.reply.send(Ok(None)).unwrap();
    let delivery = crate::fork::grupo_entrega::Delivery::new(id);
    delivery.confirm(&f.hub, ROOM);
    assert!(delivery.interrupt(&f.hub, ROOM).is_err());
    assert_eq!(f.hub.store.entry(id).unwrap().data["status"], "sending");

    f.hub.store.db.locked().execute_batch("DROP TRIGGER reject_stop").unwrap();
    let refused = crate::api::dispatch(&f.hub, &local, "send", body).await.expect_err("a sala continua bloqueada");
    assert_eq!(crate::api::error_code(&refused), Some("group_blocked"));
    assert!(bot2.try_recv().is_err(), "o próximo membro não recebe a mensagem");
    f.hub.shutdown().await;
    assert_eq!(f.hub.store.entry(id).unwrap().data["status"], "sending");

    let hub = reopen(&f.dir);
    assert!(!hub.groups.blocked(ROOM));
    let saved = hub.store.entry(id).unwrap();
    assert_eq!(saved.data["status"], "cancelled", "a entrega incerta é classificada no boot");
    assert_eq!(saved.data["groupReview"], true);
    assert_eq!(reviews(&hub), 1);
    hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}
