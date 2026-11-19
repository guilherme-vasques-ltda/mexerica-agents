//! D70 da fila (DON-907, D431): as quatro ações só valem com a mensagem ainda na fila e com a autoridade ORIGINAL
//! viva na Tx; pedido atrasado (já despachada, revisão velha) é recusado sem efeito; a reordenação fica guardada e
//! volta igual depois de reiniciar; "Mandar agora" interrompe só a resposta da própria sessão e mantém o resto da fila.
//! MX-77: as mesmas ações no pedido de outro bot ainda `queued` — remover recusa avisando o remetente, editar
//! reescreve o par com a marca "editado pelo dono" e mandar agora interrompe o turno da principal; sem reordenar.
#![allow(clippy::unwrap_used)]
use super::fork_fila::Acao;
use super::*;
use crate::LockExt;
use crate::chat::team::BotRequest;
use crate::fork::autoridade::Autoridade;
use crate::fork::sessoes::{origin::SessionOrigin, store as sessions};
use crate::store::{Device, DeviceSource, Scope};

fn fixture() -> (Arc<Hub>, Actor, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("fila-ator-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = crate::store::Store::open(std::path::Path::new(":memory:")).unwrap();
    let cfg: BotConfig =
        serde_json::from_value(json!({"id": "b", "name": "b", "backend": "claude", "cwd": "/tmp"})).unwrap();
    store.save_bot(&cfg).unwrap();
    let identity = crate::remote::identity::Identity::load_or_create(&dir).unwrap();
    let hub = Hub::new(store, "fixture".into(), identity, "fixture".into(), 0);
    let actor = new_actor(hub.clone(), hub.store.bot("b").unwrap().unwrap().config);
    (hub, actor, dir)
}

fn extra(hub: &Hub) -> Lane {
    let new = sessions::NewSession {
        chat: "b".into(),
        origin: SessionOrigin::Owner,
        source_session: None,
        parent_session: None,
        title: String::new(),
        runs_on: None,
    };
    Lane { chat: "b".into(), session: Some(sessions::create(&hub.store.db.locked(), &new).unwrap().id) }
}

fn queued(actor: &mut Actor, lane: &Lane, text: &str) -> String {
    let entry =
        actor.hub.store.insert_entry(lane, EntryKind::User, 1, &json!({"text": text, "status": "queued"})).unwrap();
    actor.queue.push_back(Queued::User { lane: lane.clone(), entry_id: entry.id.clone(), text: text.into() });
    entry.id
}

fn order(actor: &Actor) -> Vec<String> {
    actor
        .queue
        .iter()
        .filter_map(|q| match q {
            Queued::User { entry_id, .. } | Queued::RestoreUser { entry_id, .. } => Some(entry_id.clone()),
            _ => None,
        })
        .collect()
}

fn durable(hub: &Hub) -> Vec<String> {
    hub.store.queued_user_entries_for_identity("b").unwrap().into_iter().map(|e| e.id).collect()
}

fn rev(hub: &Hub, id: &str) -> i64 {
    hub.store.entry(id).unwrap().rev
}

async fn act(actor: &mut Actor, id: &str, acao: Acao, autoridade: &Autoridade) -> Result<Value> {
    let (done, _rx) = mpsc::unbounded_channel();
    actor.queued_control(id.to_owned(), acao, autoridade, &done).await
}

fn device(hub: &Arc<Hub>) -> (Device, Autoridade) {
    let device = Device {
        key: "aparelho-fila".into(),
        name: "Teste".into(),
        platform: "teste".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: crate::store::now_ms(),
        last_seen_at: None,
    };
    hub.store.put_device(&device).unwrap();
    let (_, caller) = crate::fork::autoridade::autenticar(hub, &device.key).unwrap();
    (device, Autoridade::capturar(hub, &caller).unwrap())
}

#[tokio::test]
async fn autoridade_revogada_antes_do_ator_nao_muda_nada_nas_quatro_acoes() {
    let (hub, mut actor, dir) = fixture();
    let main = Lane::main("b");
    let (a, b) = (queued(&mut actor, &main, "A"), queued(&mut actor, &main, "B"));
    actor.lane = main.clone();
    actor.turn = Some(1);
    let (device, autoridade) = device(&hub);
    hub.store.remove_device(&device.key).unwrap();
    hub.store.put_device(&device).unwrap();
    let antes = (rev(&hub, &a), rev(&hub, &b));
    for acao in [
        Acao::Remove,
        Acao::Edit { text: "novo".into(), expected: antes.1 },
        Acao::Move { position: 0, expected: antes.1 },
        Acao::SendNow,
    ] {
        let error = act(&mut actor, &b, acao, &autoridade).await.unwrap_err();
        assert!(format!("{error:#}").contains("not_allowed"), "{error:#}");
    }
    assert_eq!((rev(&hub, &a), rev(&hub, &b)), antes, "nenhuma gravação");
    assert_eq!(hub.store.entry(&b).unwrap().data["text"], "B");
    assert_eq!(order(&actor), [a.clone(), b.clone()]);
    assert_eq!(durable(&hub), [a, b]);
    assert!(!actor.stop_requested && actor.send_now.is_empty(), "nada interrompido");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn pedido_atrasado_de_mensagem_despachada_ou_revisao_velha_e_recusado_sem_efeito() {
    let (hub, mut actor, dir) = fixture();
    let main = Lane::main("b");
    let (a, b) = (queued(&mut actor, &main, "A"), queued(&mut actor, &main, "B"));
    let local = Autoridade::local();
    // A já foi entregue ao turno (saiu da fila do ator), mesmo com o status durável ainda `queued`.
    actor.queue.pop_front();
    actor.input = Some(waiting::TurnInput {
        chat: "b".into(),
        entries: vec![a.clone()],
        text: "A".into(),
        hidden: false,
        handoff: None,
        routine: None,
        transient: false,
        backend: "claude".into(),
        model: None,
        command: None,
        session: None,
        context_binding: None,
        visible_session: None,
    });
    for acao in [Acao::Remove, Acao::Edit { text: "x".into(), expected: rev(&hub, &a) }, Acao::SendNow] {
        let error = act(&mut actor, &a, acao, &local).await.unwrap_err();
        assert!(format!("{error:#}").contains("queued_entry_dispatched"), "{error:#}");
    }
    assert_eq!(hub.store.entry(&a).unwrap().data["text"], "A");
    // Revisão velha: editar e mover recusam pelo compare-and-swap.
    let velha = rev(&hub, &b);
    hub.store.update_entry(&b, &json!({"text": "B", "status": "queued", "outro": true})).unwrap();
    let error = act(&mut actor, &b, Acao::Edit { text: "x".into(), expected: velha }, &local).await.unwrap_err();
    assert!(format!("{error:#}").contains("queued_revision_conflict"), "{error:#}");
    let error = act(&mut actor, &b, Acao::Move { position: 0, expected: velha }, &local).await.unwrap_err();
    assert!(format!("{error:#}").contains("queued_revision_conflict"), "{error:#}");
    assert_eq!(hub.store.entry(&b).unwrap().data["text"], "B");
    // Remover a que ainda está na fila: `cancelled` e fora do ator; repetir o pedido é recusado.
    let reply = act(&mut actor, &b, Acao::Remove, &local).await.unwrap();
    assert_eq!(reply["entries"][0]["data"]["status"], "cancelled");
    assert!(!actor.queue_has_user(&b));
    let error = act(&mut actor, &b, Acao::Remove, &local).await.unwrap_err();
    assert!(format!("{error:#}").contains("queued_entry_dispatched"), "{error:#}");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn editar_troca_o_texto_da_entrada_e_do_item_do_ator_juntos() {
    let (hub, mut actor, dir) = fixture();
    let main = Lane::main("b");
    let a = queued(&mut actor, &main, "A");
    let reply =
        act(&mut actor, &a, Acao::Edit { text: "A editada".into(), expected: rev(&hub, &a) }, &Autoridade::local())
            .await
            .unwrap();
    assert_eq!(reply["entries"][0]["data"]["text"], "A editada");
    assert_eq!(hub.store.entry(&a).unwrap().data["text"], "A editada");
    let Some(Queued::User { text, .. }) = actor.queue.front() else { panic!("o item continua na fila") };
    assert!(text.contains("A editada"), "o prompt é reconstruído da entrada: {text}");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn mover_reordena_so_a_sessao_e_a_ordem_guardada_volta_igual_no_boot() {
    let (hub, mut actor, dir) = fixture();
    let (main, irma) = (Lane::main("b"), extra(&hub));
    let a = queued(&mut actor, &main, "A");
    let x = queued(&mut actor, &irma, "X");
    let b = queued(&mut actor, &main, "B");
    let c = queued(&mut actor, &main, "C");
    actor.lane = main.clone();
    actor.turn = Some(1);
    act(&mut actor, &c, Acao::Move { position: 0, expected: rev(&hub, &c) }, &Autoridade::local()).await.unwrap();
    // C vai para o lugar de A; X (outra sessão) fica onde estava.
    assert_eq!(order(&actor), [c.clone(), x.clone(), a.clone(), b.clone()]);
    assert_eq!(durable(&hub), [c.clone(), x.clone(), a.clone(), b.clone()]);
    assert!(hub.store.entry(&x).unwrap().data.get("queueOrder").is_none(), "a irmã não é tocada");
    // Reinício: um ator novo restaura a fila na ordem guardada.
    let mut boot = new_actor(hub.clone(), hub.store.bot("b").unwrap().unwrap().config);
    boot.restore_waiting_queue().unwrap();
    assert_eq!(order(&boot), [c, x, a, b]);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn mandar_agora_interrompe_so_a_resposta_da_sessao_e_mantem_o_resto_da_fila() {
    let (hub, mut actor, dir) = fixture();
    let (main, irma) = (Lane::main("b"), extra(&hub));
    let (a, b) = (queued(&mut actor, &main, "A"), queued(&mut actor, &main, "B"));
    let x = queued(&mut actor, &irma, "X");
    // A irmã está respondendo: a da principal passa para a frente, sem interromper a outra sessão.
    actor.lane = irma.clone();
    actor.turn = Some(1);
    let reply = act(&mut actor, &b, Acao::SendNow, &Autoridade::local()).await.unwrap();
    assert_eq!(reply["stopRequested"], false);
    assert!(!actor.stop_requested);
    assert_eq!(order(&actor), [b.clone(), a.clone(), x.clone()]);
    // A principal está respondendo: só essa resposta para, com o aviso; o resto da fila continua `queued`.
    actor.lane = main.clone();
    let reply = act(&mut actor, &a, Acao::SendNow, &Autoridade::local()).await.unwrap();
    assert_eq!(reply["stopRequested"], true);
    assert!(actor.stop_requested && actor.turn == Some(1), "o turno encerra pelo cancelamento do motor");
    assert_eq!(order(&actor), [a.clone(), b.clone(), x.clone()]);
    for id in [&a, &b, &x] {
        assert_eq!(hub.store.entry(id).unwrap().data["status"], "queued");
    }
    let history = hub.store.history("b", i64::MAX, 100).unwrap();
    assert!(history.iter().any(|e| e.kind == "notice" && e.data["text"] == fork_fila::INTERROMPIDA), "aviso honesto");
    // O próximo lote leva só a mensagem escolhida.
    assert!(actor.take_send_now(0));
    let _ = std::fs::remove_dir_all(dir);
}

/// MX-77: o par de avisos `queued` de um pedido de bot — o do remetente na conversa de "a" e o do
/// destinatário na principal de "b", na mesma `delegationId`.
fn request_notices(hub: &Hub, delegation: &str) -> (Entry, Entry) {
    let notice = |bot: &str| {
        hub.store
            .insert_entry(
                &Lane::main(bot),
                EntryKind::Notice,
                1,
                &json!({
                    "text": "pedido", "heading": "pedido", "style": "info", "status": "queued",
                    "delegationId": delegation, "sourceBotId": "a", "targetBotId": "b",
                    "sourceChat": "a", "sourceSession": null,
                    "botMessage": {"sourceBotId": "a", "targetBotId": "b", "text": "pedido original"},
                }),
            )
            .unwrap()
    };
    (notice("a"), notice("b"))
}

/// Um `ask_bot` enfileirado no ator: devolve o id do aviso do destinatário e o canal da resposta.
fn ask_queued(hub: &Arc<Hub>, actor: &mut Actor) -> (String, tokio::sync::oneshot::Receiver<Result<String>>) {
    let delegation = uuid::Uuid::new_v4().to_string();
    let (_sender, recipient) = request_notices(hub, &delegation);
    let (request, answer) = BotRequest::for_test_named(&delegation, &recipient.id, "Byte");
    actor.queue.push_back(Queued::BotRequest(request));
    (recipient.id, answer)
}

/// Um `message_bot` enfileirado no ator: devolve o id do aviso do destinatário.
fn message_queued(hub: &Arc<Hub>, actor: &mut Actor) -> String {
    // A cota do message_bot real exige o remetente em turno; a fixture não roda agentes.
    hub.team.start_turn("a", None);
    let delegation = uuid::Uuid::new_v4().to_string();
    let (sender, recipient) = request_notices(hub, &delegation);
    actor.queue.push_back(Queued::BotRequest(BotRequest::for_test_message(
        &delegation,
        &recipient.id,
        hub.clone(),
        vec![sender.id.clone(), recipient.id.clone()],
    )));
    recipient.id
}

fn notices_of(hub: &Hub, delegation: &str) -> Vec<Entry> {
    ["a", "b"]
        .into_iter()
        .flat_map(|bot| hub.store.history(bot, i64::MAX, 20).unwrap())
        .filter(|e| e.data["delegationId"] == delegation)
        .collect()
}

#[tokio::test]
async fn mx77_remover_pedido_de_ask_rejeita_e_devolve_o_motivo_ao_remetente() {
    let (hub, mut actor, dir) = fixture();
    let (id, answer) = ask_queued(&hub, &mut actor);
    let delegation = hub.store.entry(&id).unwrap().data["delegationId"].as_str().unwrap().to_owned();
    let reply = act(&mut actor, &id, Acao::Remove, &Autoridade::local()).await.unwrap();
    assert!(reply["entries"].as_array().unwrap().is_empty(), "a recusa não grava na Tx");
    assert!(actor.queue.is_empty(), "o pedido sai da fila do ator");
    let error = answer.await.unwrap().unwrap_err();
    assert!(format!("{error:#}").contains("Cancelled by the owner"), "{error:#}");
    // O fechamento dos avisos é do Waiting do remetente; aqui provamos o erro tipado que o dispara.
    let _ = notices_of(&hub, &delegation);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn mx77_remover_pedido_de_message_bot_cancela_o_par_com_o_motivo() {
    let (hub, mut actor, dir) = fixture();
    let id = message_queued(&hub, &mut actor);
    let delegation = hub.store.entry(&id).unwrap().data["delegationId"].as_str().unwrap().to_owned();
    act(&mut actor, &id, Acao::Remove, &Autoridade::local()).await.unwrap();
    assert!(actor.queue.is_empty());
    let notices = notices_of(&hub, &delegation);
    assert_eq!(notices.len(), 2);
    for notice in notices {
        assert_eq!(notice.data["status"], "cancelled", "{}", notice.bot_id);
        assert_eq!(notice.data["botMessage"]["detail"], "Cancelled by the owner before execution.");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn mx77_editar_pedido_reescreve_o_par_marca_editado_e_remonta_o_prompt() {
    let (hub, mut actor, dir) = fixture();
    let (id, _answer) = ask_queued(&hub, &mut actor);
    let expected = rev(&hub, &id);
    let reply = act(&mut actor, &id, Acao::Edit { text: "texto do dono".into(), expected }, &Autoridade::local())
        .await
        .unwrap();
    assert_eq!(reply["entries"].as_array().unwrap().len(), 2, "o par inteiro é reescrito");
    let delegation = hub.store.entry(&id).unwrap().data["delegationId"].as_str().unwrap().to_owned();
    for notice in notices_of(&hub, &delegation) {
        assert_eq!(notice.data["botMessage"]["text"], "texto do dono", "{}", notice.bot_id);
        assert_eq!(notice.data["botMessage"]["edited"], true, "a marca 'editado pelo dono'");
        assert!(notice.data["text"].as_str().unwrap().contains("texto do dono"));
    }
    let Some(Queued::BotRequest(request)) = actor.queue.front() else { panic!("o pedido continua na fila") };
    assert!(request.prompt.contains("texto do dono"), "{}", request.prompt);
    assert!(request.prompt.contains("edited the request text"), "{}", request.prompt);
    assert!(request.prompt.contains("Byte"), "o nome do remetente vem do pedido: {}", request.prompt);
    // CAS: revisão velha recusa sem efeito; o pedido não se reordena.
    let error =
        act(&mut actor, &id, Acao::Edit { text: "outro".into(), expected }, &Autoridade::local()).await.unwrap_err();
    assert!(format!("{error:#}").contains("queued_revision_conflict"), "{error:#}");
    let error = act(&mut actor, &id, Acao::Move { position: 0, expected: rev(&hub, &id) }, &Autoridade::local())
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("queued_move_unsupported"), "{error:#}");
    assert_eq!(hub.store.entry(&id).unwrap().data["botMessage"]["text"], "texto do dono");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn mx77_mandar_agora_poe_o_pedido_na_frente_e_interrompe_o_turno_da_principal() {
    let (hub, mut actor, dir) = fixture();
    let main = Lane::main("b");
    let user = queued(&mut actor, &main, "mensagem do dono");
    let (id, _answer) = ask_queued(&hub, &mut actor);
    actor.lane = main.clone();
    actor.turn = Some(1);
    let reply = act(&mut actor, &id, Acao::SendNow, &Autoridade::local()).await.unwrap();
    assert_eq!(reply["stopRequested"], true);
    assert!(actor.stop_requested, "o turno da principal para como no 'Mandar agora' de mensagem");
    assert!(matches!(actor.queue.front(), Some(Queued::BotRequest(a)) if a.entry_id == id), "o pedido passa à frente");
    assert_eq!(order(&actor), vec![user], "a mensagem do dono continua na fila, logo atrás");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn mx77_controles_de_pedido_recusam_aviso_errado_outro_destino_despachado_e_autoridade_morta() {
    let (hub, mut actor, dir) = fixture();
    let (id, mut answer) = ask_queued(&hub, &mut actor);
    let local = Autoridade::local();
    // O aviso do REMETENTE não controla o pedido: o bot_id dele não é o do ator.
    let real = hub.store.entry(&id).unwrap().data["delegationId"].as_str().unwrap().to_owned();
    let sender = notices_of(&hub, &real).into_iter().find(|e| e.bot_id == "a").unwrap().id;
    // Um pedido enfileirado cujo aviso aponta para ESTE id mas pertence à conversa de "a".
    actor.queue.push_back(Queued::BotRequest(BotRequest::for_test_named(&real, &sender, "Byte").0));
    let error = act(&mut actor, &sender, Acao::Remove, &local).await.unwrap_err();
    assert!(format!("{error:#}").contains("cross_conversation_reference"), "{error:#}");
    actor.queue.pop_back();
    // Aviso de pedido endereçado a OUTRO bot não passa neste ator.
    let other = hub
        .store
        .insert_entry(
            &Lane::main("b"),
            EntryKind::Notice,
            1,
            &json!({
                "text": "x", "status": "queued", "delegationId": "del-outro",
                "sourceBotId": "a", "targetBotId": "c",
                "botMessage": {"sourceBotId": "a", "targetBotId": "c", "text": "x"},
            }),
        )
        .unwrap();
    actor.queue.push_back(Queued::BotRequest(BotRequest::for_test_named("del-outro", &other.id, "Byte").0));
    let error = act(&mut actor, &other.id, Acao::Remove, &local).await.unwrap_err();
    assert!(format!("{error:#}").contains("queued_entry_dispatched"), "{error:#}");
    actor.queue.pop_back();
    // Pedido já despachado (fora da fila do ator) é recusado sem efeito.
    actor.queue.pop_front();
    let error = act(&mut actor, &id, Acao::Remove, &local).await.unwrap_err();
    assert!(format!("{error:#}").contains("queued_entry_dispatched"), "{error:#}");
    assert!(answer.try_recv().is_err(), "o pedido não foi rejeitado");
    // Autoridade revogada antes do ator: nada muda em nenhuma das quatro ações.
    actor.queue.push_back(Queued::BotRequest(BotRequest::for_test_named("del-vivo", &id, "Byte").0));
    let (device, autoridade) = device(&hub);
    hub.store.remove_device(&device.key).unwrap();
    hub.store.put_device(&device).unwrap();
    for acao in [
        Acao::Remove,
        Acao::Edit { text: "x".into(), expected: rev(&hub, &id) },
        Acao::Move { position: 0, expected: rev(&hub, &id) },
        Acao::SendNow,
    ] {
        let error = act(&mut actor, &id, acao, &autoridade).await.unwrap_err();
        assert!(format!("{error:#}").contains("not_allowed"), "{error:#}");
    }
    assert!(actor.queue.len() == 1, "o pedido continua na fila");
    assert!(answer.try_recv().is_err(), "nada foi rejeitado");
    let _ = std::fs::remove_dir_all(dir);
}
