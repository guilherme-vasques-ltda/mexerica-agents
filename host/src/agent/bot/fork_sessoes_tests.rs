//! D70 do PR5 no ator: encerrar solta só a extra (fila em memória e restauração, `Cmd::Send` atrasado, pontes e
//! pedidos das irmãs vivos, principal rodando), o `newSession` legado recomeça só a principal com a autoridade
//! ORIGINAL (revogada antes do ator processar → nada muda; Local e aparelho com Control continuam), pedido expirado
//! não roda tarde, a retomada/boot de uma extra encerrada não executa, o cartão da continuação só é consumido no permit
//! e a varredura respeita o trabalho em memória. As fronteiras de Tx ficam em `fork/sessoes/pr5_tests.rs`.
#![allow(clippy::unwrap_used)]
use super::*;
use crate::LockExt;
use crate::fork::autoridade::Autoridade;
use crate::fork::sessoes::{bindings, encerrar, origin::SessionOrigin, store as sessions};
use crate::store::{Device, DeviceSource, Scope};

fn fixture() -> (Arc<Hub>, std::path::PathBuf) {
    let dir =
        std::fs::canonicalize(std::env::temp_dir()).unwrap().join(format!("sessoes-pr5-ator-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = crate::store::Store::open(std::path::Path::new(":memory:")).unwrap();
    for id in ["b", "c"] {
        let cfg: BotConfig =
            serde_json::from_value(json!({"id": id, "name": id, "backend": "claude", "cwd": "/tmp"})).unwrap();
        store.save_bot(&cfg).unwrap();
    }
    let identity = crate::remote::identity::Identity::load_or_create(&dir).unwrap();
    (Hub::new(store, "fixture".into(), identity, "fixture".into(), 0), dir)
}

fn actor_of(hub: &Arc<Hub>) -> Actor {
    new_actor(hub.clone(), hub.store.bot("b").unwrap().unwrap().config)
}

fn extra(hub: &Hub) -> String {
    let new = sessions::NewSession {
        chat: "b".into(),
        origin: SessionOrigin::Owner,
        source_session: None,
        parent_session: None,
        title: String::new(),
        runs_on: None,
    };
    sessions::create(&hub.store.db.locked(), &new).unwrap().id
}

fn lane(session: &str) -> Lane {
    Lane { chat: "b".into(), session: Some(session.into()) }
}

fn slot(session: &str) -> String {
    format!("session:{session}")
}

fn queued(actor: &mut Actor, lane: &Lane, text: &str) -> String {
    let entry =
        actor.hub.store.insert_entry(lane, EntryKind::User, 1, &json!({"text": text, "status": "queued"})).unwrap();
    actor.queue.push_back(Queued::User { lane: lane.clone(), entry_id: entry.id.clone(), text: text.into() });
    entry.id
}

/// A origem do próprio computador do Hub, explícita (as RPCs de cartão exigem a origem do pedido).
fn local() -> crate::fork::donna_fixa::pedido::Origem {
    crate::fork::donna_fixa::pedido::Origem::Local
}

fn status(hub: &Hub, id: &str) -> Value {
    hub.store.entry(id).unwrap().data["status"].clone()
}

fn state(hub: &Hub, id: &str) -> String {
    sessions::get(&hub.store.db.locked(), id).unwrap().unwrap().state
}

fn revision(hub: &Hub, id: &str) -> i64 {
    sessions::get(&hub.store.db.locked(), id).unwrap().unwrap().revision
}

/// Credencial de ponte viva no slot (`None` = principal), emitida pelo produtor canônico.
fn bridge(hub: &Hub, slot: Option<&str>, session: &str) {
    let binding = bindings::issue_in(&hub.store, "b", "b", session, slot, None).unwrap();
    crate::fork::ponte_credencial::provisionar(hub, "b", slot, &binding, &mut []).unwrap();
}

/// Aparelho pareado com Control (origem Local) e a autoridade ORIGINAL do transporte dele.
fn device(hub: &Arc<Hub>) -> (Device, Autoridade) {
    let device = Device {
        key: "aparelho-pr5".into(),
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

/// Revoga e reconcede o aparelho: a autoridade capturada antes deixa de valer (outra geração).
fn revoke(hub: &Hub, device: &Device) {
    hub.store.remove_device(&device.key).unwrap();
    hub.store.put_device(device).unwrap();
}

#[tokio::test]
async fn encerrar_solta_so_a_extra_com_principal_rodando_e_irma_viva() {
    let (hub, dir) = fixture();
    let (a, irma) = (extra(&hub), extra(&hub));
    let mut actor = actor_of(&hub);
    let da_a = queued(&mut actor, &lane(&a), "A");
    let restaurada = queued(&mut actor, &lane(&a), "A2");
    actor.queue.pop_back();
    actor.queue.push_back(Queued::RestoreUser { entry_id: restaurada.clone(), retry_at: None });
    let da_irma = queued(&mut actor, &lane(&irma), "B");
    let da_main = queued(&mut actor, &Lane::main("b"), "P");
    hub.store.kv_set(&crate::store::run_session_key("b", &slot(&a)), "acp-a").unwrap();
    bridge(&hub, Some(&slot(&a)), &a);
    bridge(&hub, Some(&slot(&irma)), &irma);
    bridge(&hub, None, "b");
    hub.team.start_turn("b", None);
    let pedido_irma = hub.team.begin_for_test("pedido-irma", "b", "c", &lane(&irma)).unwrap();
    // A principal está rodando.
    actor.lane = Lane::main("b");
    actor.turn = Some(2);
    let (_device, autoridade) = device(&hub);
    let reply = actor.session_close(&a, revision(&hub, &a), &autoridade).await.unwrap();
    assert_eq!(reply["session"]["state"], "closed");
    assert_eq!(reply["cancelled"]["entryIds"], json!([da_a, restaurada]));
    assert_eq!(reply["stopRequested"], false);
    assert!(!actor.stop_requested && actor.turn == Some(2), "o turno da principal continua");
    assert!(!actor.queue_has_user(&da_a) && !actor.queue_has_user(&restaurada));
    assert!(actor.queue_has_user(&da_irma) && actor.queue_has_user(&da_main));
    assert_eq!((status(&hub, &da_irma), status(&hub, &da_main)), (json!("queued"), json!("queued")));
    assert!(hub.store.kv_get(&crate::store::run_session_key("b", &slot(&a))).is_none());
    // Pontes e pedido das irmãs e da principal vivos; só os da extra encerrada caem.
    assert!(!hub.pontes.tem_slot("b", Some(&slot(&a))));
    assert!(hub.pontes.tem_slot("b", Some(&slot(&irma))) && hub.pontes.tem_slot("b", None));
    assert!(!*pedido_irma.borrow());
    // `Cmd::Send` atrasado da extra fechada: a entrada já é `cancelled`, então nunca vira lote.
    assert_eq!(status(&hub, &da_a), "cancelled");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn encerrar_a_extra_em_execucao_para_so_ela_e_preserva_pontes_da_principal() {
    let (hub, dir) = fixture();
    let a = extra(&hub);
    bridge(&hub, None, "b");
    let irma = extra(&hub);
    let mut actor = actor_of(&hub);
    actor.lane = lane(&a);
    actor.turn = Some(1);
    hub.team.start_turn("b", None);
    let da_irma = hub.team.begin_for_test("pedido-irma", "b", "c", &lane(&irma)).unwrap();
    let da_main = hub.team.begin_for_test("pedido-main", "b", "d", &Lane::main("b")).unwrap();
    let reply = actor.session_close(&a, revision(&hub, &a), &Autoridade::local()).await.unwrap();
    assert_eq!(reply["stopRequested"], true);
    assert!(actor.stop_requested);
    assert!(hub.pontes.tem_slot("b", None), "cancelar a extra não revoga as pontes do bot inteiro");
    // A conclusão da parada da extra não cancela pedidos da irmã nem da principal (R1-01).
    actor.finish_turn(&Ok(json!({"stopReason":"cancelled"})));
    assert!(!*da_irma.borrow() && !*da_main.borrow());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn autoridade_revogada_antes_do_ator_nao_grava_nada_e_pedido_expirado_nao_roda() {
    let (hub, dir) = fixture();
    let a = extra(&hub);
    let mut actor = actor_of(&hub);
    let da_a = queued(&mut actor, &lane(&a), "A");
    let da_main = queued(&mut actor, &Lane::main("b"), "P");
    actor.session_id = Some("acp-main".into());
    let (device, autoridade) = device(&hub);
    revoke(&hub, &device);
    // Encerrar: nada muda (sessão, fila, memória do ator).
    let error = actor.session_close(&a, revision(&hub, &a), &autoridade).await.unwrap_err();
    assert!(format!("{error:#}").contains("not_allowed"), "{error:#}");
    assert_eq!(state(&hub, &a), "open");
    assert_eq!(status(&hub, &da_a), "queued");
    assert!(actor.queue_has_user(&da_a));
    // Recomeçar a principal: nenhuma escrita, cancelamento ou cartão.
    let space = crate::fork::spaces::chat_space(&hub, "b").unwrap();
    let conversation = hub.store.space_conversation(&space, "b").unwrap().revision;
    let error = actor.primary_new_session("b", conversation, &autoridade).await.unwrap_err();
    assert!(format!("{error:#}").contains("not_allowed"), "{error:#}");
    assert_eq!(status(&hub, &da_main), "queued");
    assert!(actor.queue_has_user(&da_main));
    assert!(crate::chat::handoff::pending(&hub.store, "b").is_none(), "nenhum cartão");
    assert_eq!(actor.session_id.as_deref(), Some("acp-main"));
    // Pedido cuja resposta já expirou (`session_control_pending`): o ator não fecha tarde.
    let (reply, answer) = tokio::sync::oneshot::channel();
    drop(answer);
    let (done, _rx) = mpsc::unbounded_channel();
    let command = Cmd::SessionClose {
        session: a.clone(),
        expected: revision(&hub, &a),
        autoridade: Box::new(Autoridade::local()),
        reply,
    };
    actor.on_cmd(command, &done).await;
    assert_eq!(state(&hub, &a), "open");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn new_session_legado_recomeca_so_a_principal_com_a_autoridade_original() {
    let (hub, dir) = fixture();
    let (trabalhando, na_fila) = (extra(&hub), extra(&hub));
    let mut actor = actor_of(&hub);
    hub.store.insert_entry(&Lane::main("b"), EntryKind::User, 1, &json!({"text":"antes","status":"sent"})).unwrap();
    actor.session_id = Some("acp-main".into());
    let da_fila = queued(&mut actor, &lane(&na_fila), "fila");
    let da_main = queued(&mut actor, &Lane::main("b"), "principal");
    bridge(&hub, Some(&slot(&trabalhando)), &trabalhando);
    // A extra está rodando e esperando um `ask_bot` que ela fez.
    actor.lane = lane(&trabalhando);
    actor.turn = Some(3);
    hub.team.start_turn("b", None);
    let cancelled = hub.team.begin_for_test("pedido", "b", "c", &lane(&trabalhando)).unwrap();
    let space = crate::fork::spaces::chat_space(&hub, "b").unwrap();
    let conversation = hub.store.space_conversation(&space, "b").unwrap().revision;
    // Aparelho com Control: o positivo continua.
    let (_device, autoridade) = device(&hub);
    let reply = actor.primary_new_session("b", conversation, &autoridade).await.unwrap();
    assert_eq!(reply["stopRequested"], false);
    assert_eq!(reply["entryIds"], json!([da_main]));
    assert_eq!(status(&hub, &da_main), "cancelled");
    assert_eq!(status(&hub, &da_fila), "queued");
    assert!(actor.queue_has_user(&da_fila) && !actor.queue_has_user(&da_main));
    assert!(!actor.stop_requested && actor.turn == Some(3), "a extra continua trabalhando");
    assert!(!*cancelled.borrow(), "o pedido da extra não é cancelado");
    assert!(hub.pontes.tem_slot("b", Some(&slot(&trabalhando))));
    assert!(actor.session_id.is_none(), "só a ACP da principal é esquecida");
    // Cartão de passagem na chave legada, na mesma Tx, sem "Manter".
    let (card, _) = crate::chat::handoff::pending(&hub.store, "b").unwrap();
    assert_eq!(reply["handoff"]["id"], json!(card));
    assert_eq!(hub.store.entry(&card).unwrap().data["handoff"]["reason"], "restart");
    assert!(crate::chat::handoff::call_as(&hub, "keepConversation", &json!({"entryId": card}), &local()).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn principal_rodando_recomeca_sem_cancelar_pedido_de_extra() {
    let (hub, dir) = fixture();
    let a = extra(&hub);
    let mut actor = actor_of(&hub);
    actor.lane = Lane::main("b");
    actor.turn = Some(1);
    bridge(&hub, Some(&slot(&a)), &a);
    hub.team.start_turn("b", None);
    let da_extra = hub.team.begin_for_test("pedido-extra", "b", "c", &lane(&a)).unwrap();
    let da_main = hub.team.begin_for_test("pedido-main", "b", "d", &Lane::main("b")).unwrap();
    let space = crate::fork::spaces::chat_space(&hub, "b").unwrap();
    let conversation = hub.store.space_conversation(&space, "b").unwrap().revision;
    let reply = actor.primary_new_session("b", conversation, &Autoridade::local()).await.unwrap();
    assert_eq!(reply["stopRequested"], true);
    assert!(*da_main.borrow(), "o pedido da principal é cancelado");
    assert!(!*da_extra.borrow(), "o da extra continua");
    assert!(hub.pontes.tem_slot("b", Some(&slot(&a))), "pontes da extra intactas");
    // A conclusão da parada (o motor responde ao cancelamento) também só fecha a lane da principal (R1-01).
    actor.finish_turn(&Ok(json!({"stopReason":"cancelled"})));
    assert!(!*da_extra.borrow(), "a conclusão não cancela o pedido da extra");
    assert!(hub.team.pending_from_session("b", &lane(&a)));
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn retomada_e_boot_de_extra_encerrada_nao_executam() {
    let (hub, dir) = fixture();
    let (a, aberta) = (extra(&hub), extra(&hub));
    let waiting = |session: &str| {
        json!({"input":{"chat":"b","entries":[],"text":"x","hidden":false,"transient":false,"backend":"claude",
            "visibleSession":session},"phase":"retrying","retryAt":0,"backend":"claude","observedAt":0})
    };
    // Boot: o checkpoint da extra virou tombstone na Tx do fechamento; restaurar não revive.
    hub.store.kv_set("turn.waiting.b", &waiting(&a).to_string()).unwrap();
    {
        let mut db = hub.store.db.locked();
        let tx = db.transaction().unwrap();
        let rev = sessions::get(&tx, &a).unwrap().unwrap().revision;
        encerrar::close_on(&tx, "b", &a, rev).unwrap();
        tx.commit().unwrap();
    }
    let mut actor = actor_of(&hub);
    assert!(!actor.restore_waiting());
    assert!(actor.waiting.is_none());
    // Retomada em memória de uma extra encerrada (espera carregada antes do fechamento): termina sem turno.
    actor.waiting = Some(serde_json::from_value(waiting(&a)).unwrap());
    let (done, _rx) = mpsc::unbounded_channel();
    actor.retry_waiting(&done).await;
    assert!(actor.waiting.is_none() && actor.turn.is_none());
    let notice: i64 = hub
        .store
        .db
        .locked()
        .query_row(
            "SELECT COUNT(*) FROM entries WHERE session=?1 AND json_extract(data,'$.text')=?2",
            rusqlite::params![a, encerrar::CLOSED_BEFORE_REPLY],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(notice, 1);
    // Erro técnico ao ler o estado: adia sem destruir a espera.
    actor.waiting = Some(serde_json::from_value(waiting(&aberta)).unwrap());
    hub.store.db.locked().execute_batch("ALTER TABLE donna_bot_sessions RENAME TO sessoes_retidas").unwrap();
    assert!(actor.drop_closed_waiting());
    assert!(actor.waiting.is_some());
    hub.store.db.locked().execute_batch("ALTER TABLE sessoes_retidas RENAME TO donna_bot_sessions").unwrap();
    assert!(!actor.drop_closed_waiting(), "aberta segue para a retomada canônica");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn captura_do_cartao_e_por_lane_independe_do_buffer_legado_e_so_vai_ao_permit_com_o_texto_dela() {
    let (hub, dir) = fixture();
    let (n1, n2) = (extra(&hub), extra(&hub));
    for n in [&n1, &n2] {
        let c = hub.store.db.locked();
        let reason = crate::chat::handoff::Reason::Continuation;
        crate::chat::handoff::draft_on(&c, &lane(n), "b", reason, &format!("<handoff>RESUMO {n}</handoff>"), None)
            .unwrap();
    }
    let mut actor = actor_of(&hub);
    // Uma passagem legada de outra lane em memória (R2-03) não impede nem contamina a captura de n2.
    actor.handoff = Some("<handoff>DE OUTRA LANE</handoff>".into());
    actor.handoff_chat = Some(lane(&n1));
    actor.capture_session_card(&lane(&n2)).unwrap();
    let text2 = format!("<handoff>RESUMO {n2}</handoff>");
    let framed2 = crate::chat::handoff::as_context(&text2);
    assert_eq!(actor.turn_handoff(&lane(&n2)).as_deref(), Some(framed2.as_str()), "enquadrado pelo Hub como dado");
    assert_eq!(actor.handoff.as_deref(), Some("<handoff>DE OUTRA LANE</handoff>"), "o buffer legado não é tocado");
    // Capturar não consome: o rascunho e a chave continuam (falha antes do permit não perde nada).
    let c = hub.store.db.locked();
    let durable = crate::chat::handoff::capture_on(&c, &lane(&n2)).unwrap().unwrap();
    drop(c);
    assert_eq!(durable.text, text2);
    // Só vai ao permit quando o turno é da lane da captura e o insumo capturado leva exatamente o texto dela.
    actor.lane = lane(&n2);
    assert!(actor.session_card_for_permit().is_none(), "sem insumo capturado");
    actor.input = Some(
        serde_json::from_value(json!({"chat":"b","entries":[],"text":"oi","hidden":false,"transient":false,
            "backend":"claude","visibleSession":n2,"handoff":framed2}))
        .unwrap(),
    );
    assert_eq!(actor.session_card_for_permit(), Some((lane(&n2), durable.clone())));
    // Recomeço da principal também é capturado pelo mesmo leitor; um rollover legado não.
    let cfg = hub.store.bot("b").unwrap().unwrap().config;
    crate::chat::handoff::draft(&hub, &cfg, "acp-main", crate::chat::handoff::Reason::Full).unwrap();
    actor.capture_session_card(&Lane::main("b")).unwrap();
    assert!(actor.session_card.is_none(), "rollover segue o fluxo legado");
    // Reinício antes do permit: um ator novo acha o mesmo rascunho.
    let mut restarted = actor_of(&hub);
    restarted.capture_session_card(&lane(&n2)).unwrap();
    assert_eq!(restarted.turn_handoff(&lane(&n2)).as_deref(), Some(framed2.as_str()));
    // A outra continuação tem o próprio cartão, intocado.
    assert!(crate::chat::handoff::capture_on(&hub.store.db.locked(), &lane(&n1)).unwrap().is_some());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn varredura_respeita_o_trabalho_em_memoria_do_ator() {
    let (hub, dir) = fixture();
    let a = extra(&hub);
    let old = crate::store::now_ms() - 4 * 24 * 60 * 60 * 1000;
    let sql = "UPDATE donna_bot_sessions SET created_at=?2,last_used_at=?2 WHERE id=?1";
    hub.store.db.locked().execute(sql, rusqlite::params![a, old]).unwrap();
    let mut actor = actor_of(&hub);
    // Só em memória: um `ask_bot` pendente feito pela extra.
    hub.team.start_turn("b", None);
    let _cancel = hub.team.begin_for_test("pedido", "b", "c", &lane(&a)).unwrap();
    actor.session_idle_close(&a);
    assert_eq!(state(&hub, &a), "open");
    hub.team.clear_for_test();
    actor.session_idle_close(&a);
    let closed = sessions::get(&hub.store.db.locked(), &a).unwrap().unwrap();
    assert_eq!((closed.state.as_str(), closed.closed_reason.as_deref()), ("closed", Some("idle")));
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn recomecar_com_rascunho_antigo_vira_recomeco_na_mesma_tx_e_recusa_manter() {
    let (hub, dir) = fixture();
    let cfg = hub.store.bot("b").unwrap().unwrap().config;
    hub.store.insert_entry(&Lane::main("b"), EntryKind::User, 1, &json!({"text":"antes","status":"sent"})).unwrap();
    crate::chat::handoff::draft(&hub, &cfg, "acp-main", crate::chat::handoff::Reason::Full).unwrap();
    let (card, _) = crate::chat::handoff::pending(&hub.store, "b").unwrap();
    let edited = json!({"entryId": card, "text": "RESUMO EDITADO PELO DONO"});
    crate::chat::handoff::call_as(&hub, "editHandoff", &edited, &local()).unwrap();
    assert_eq!(hub.store.entry(&card).unwrap().data["handoff"]["reason"], "full", "antes: rollover, com Manter");
    let mut actor = actor_of(&hub);
    actor.session_id = Some("acp-main".into());
    let space = crate::fork::spaces::chat_space(&hub, "b").unwrap();
    let conversation = hub.store.space_conversation(&space, "b").unwrap().revision;
    let reply = actor.primary_new_session("b", conversation, &Autoridade::local()).await.unwrap();
    // O mesmo cartão (sem duplicar), agora do recomeço, com o texto editado.
    assert_eq!(reply["handoff"]["id"], json!(card));
    let data = hub.store.entry(&card).unwrap().data;
    assert_eq!(data["handoff"]["reason"], "restart");
    assert_eq!(data["handoff"]["status"], "draft");
    assert_eq!(data["handoff"]["text"], "RESUMO EDITADO PELO DONO");
    assert_eq!(crate::chat::handoff::pending(&hub.store, "b").map(|(id, _)| id), Some(card.clone()));
    let cards: i64 = hub
        .store
        .db
        .locked()
        .query_row("SELECT COUNT(*) FROM entries WHERE json_extract(data,'$.handoff.status')='draft'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(cards, 1);
    assert!(crate::chat::handoff::call_as(&hub, "keepConversation", &json!({"entryId": card}), &local()).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

/// Prende a próxima gravação do dono no gancho de teste (antes das travas de MB/emit) até `release`.
fn hold_owner_write(hub: &Hub) -> (tokio::sync::oneshot::Receiver<()>, std::sync::mpsc::Sender<()>) {
    let (entered, at_hook) = tokio::sync::oneshot::channel();
    let (release, resume) = std::sync::mpsc::channel::<()>();
    *hub.store.before_owner_write.locked() = Some(Box::new(move || {
        let _ = entered.send(());
        let _ = resume.recv();
    }));
    (at_hook, release)
}

#[tokio::test]
async fn manter_e_editar_decidem_pelo_cartao_atual_quando_o_recomeco_ganha_a_corrida() {
    for method in ["keepConversation", "editHandoff"] {
        let (hub, dir) = fixture();
        let cfg = hub.store.bot("b").unwrap().unwrap().config;
        hub.store.insert_entry(&Lane::main("b"), EntryKind::User, 1, &json!({"text":"antes","status":"sent"})).unwrap();
        crate::chat::handoff::draft(&hub, &cfg, "acp-main", crate::chat::handoff::Reason::Full).unwrap();
        let (card, _) = crate::chat::handoff::pending(&hub.store, "b").unwrap();
        let mut actor = actor_of(&hub);
        actor.session_id = Some("acp-main".into());
        // A ação do dono decidiu ver um rollover Full; fica presa antes das travas.
        let (at_hook, release) = hold_owner_write(&hub);
        let body = json!({"entryId": card, "text": "EDITADO DEPOIS"});
        let (h, m) = (hub.clone(), method);
        let action = tokio::task::spawn_blocking(move || crate::chat::handoff::call_as(&h, m, &body, &local()));
        at_hook.await.unwrap();
        // O recomeço ganha: retaga o mesmo cartão como Restart e esquece a ACP.
        let space = crate::fork::spaces::chat_space(&hub, "b").unwrap();
        let conversation = hub.store.space_conversation(&space, "b").unwrap().revision;
        actor.primary_new_session("b", conversation, &Autoridade::local()).await.unwrap();
        release.send(()).unwrap();
        let result = action.await.unwrap();
        let data = hub.store.entry(&card).unwrap().data;
        assert_eq!(data["handoff"]["reason"], "restart", "{method}: o motivo atual vale");
        assert_eq!(data["handoff"]["status"], "draft", "{method}: a passagem não some");
        if method == "keepConversation" {
            assert!(result.is_err(), "Manter tardio é recusado pelo cartão atual");
        } else {
            result.unwrap();
            assert_eq!(data["handoff"]["text"], "EDITADO DEPOIS", "a edição só troca o texto");
        }
        assert_eq!(crate::chat::handoff::pending(&hub.store, "b").map(|(id, _)| id), Some(card));
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[tokio::test]
async fn manter_concluido_antes_do_recomeco_continua_valido_e_o_recomeco_cria_o_proprio_cartao() {
    let (hub, dir) = fixture();
    let cfg = hub.store.bot("b").unwrap().unwrap().config;
    hub.store.insert_entry(&Lane::main("b"), EntryKind::User, 1, &json!({"text":"antes","status":"sent"})).unwrap();
    crate::chat::handoff::draft(&hub, &cfg, "acp-main", crate::chat::handoff::Reason::Full).unwrap();
    let (card, _) = crate::chat::handoff::pending(&hub.store, "b").unwrap();
    crate::chat::handoff::call_as(&hub, "keepConversation", &json!({"entryId": card}), &local()).unwrap();
    assert_eq!(hub.store.entry(&card).unwrap().data["handoff"]["status"], "kept");
    let mut actor = actor_of(&hub);
    actor.session_id = Some("acp-main".into());
    let space = crate::fork::spaces::chat_space(&hub, "b").unwrap();
    let conversation = hub.store.space_conversation(&space, "b").unwrap().revision;
    let reply = actor.primary_new_session("b", conversation, &Autoridade::local()).await.unwrap();
    let new = reply["handoff"]["id"].as_str().unwrap().to_owned();
    assert_ne!(new, card);
    assert_eq!(hub.store.entry(&new).unwrap().data["handoff"]["reason"], "restart");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn editar_ou_manter_com_a_autoridade_original_revogada_nao_muda_o_cartao() {
    for method in ["keepConversation", "editHandoff"] {
        let (hub, dir) = fixture();
        let cfg = hub.store.bot("b").unwrap().unwrap().config;
        crate::chat::handoff::draft(&hub, &cfg, "acp-main", crate::chat::handoff::Reason::Full).unwrap();
        let (card, _) = crate::chat::handoff::pending(&hub.store, "b").unwrap();
        let before = hub.store.entry(&card).unwrap();
        let (device, _) = device(&hub);
        let (_, caller) = crate::fork::autoridade::autenticar(&hub, &device.key).unwrap();
        // Revoga e reconcede entre a captura do pedido e a gravação (gancho do dono).
        let (h, d) = (hub.clone(), device.clone());
        *hub.store.before_owner_write.locked() = Some(Box::new(move || revoke(&h, &d)));
        let body = json!({"entryId": card, "text": "NAO GRAVAR"});
        assert!(crate::api::dispatch(&hub, &caller, method, body).await.is_err(), "{method}");
        let after = hub.store.entry(&card).unwrap();
        assert_eq!((after.rev, &after.data), (before.rev, &before.data), "{method}: nada muda");
        assert_eq!(crate::chat::handoff::pending(&hub.store, "b").map(|(id, _)| id), Some(card));
        let _ = std::fs::remove_dir_all(dir);
    }
    // O positivo: aparelho com Control vigente edita.
    let (hub, dir) = fixture();
    let cfg = hub.store.bot("b").unwrap().unwrap().config;
    crate::chat::handoff::draft(&hub, &cfg, "acp-main", crate::chat::handoff::Reason::Full).unwrap();
    let (card, _) = crate::chat::handoff::pending(&hub.store, "b").unwrap();
    let (device, _) = device(&hub);
    let (_, caller) = crate::fork::autoridade::autenticar(&hub, &device.key).unwrap();
    let body = json!({"entryId": card, "text": "EDITADO PELO APP"});
    crate::api::dispatch(&hub, &caller, "editHandoff", body).await.unwrap();
    assert_eq!(hub.store.entry(&card).unwrap().data["handoff"]["text"], "EDITADO PELO APP");
    let _ = std::fs::remove_dir_all(dir);
}

#[path = "fork_session_delete_tests.rs"]
mod excluir_tests;

#[tokio::test]
async fn d543_space_delete_ator_confere_turno_fila_pedido_e_reserva_ate_commit() {
    let (hub, dir) = fixture();
    let mut actor = actor_of(&hub);
    let foreign = Lane { chat: "foreign-space".into(), session: Some("extra".into()) };
    let chats = vec![foreign.chat.clone()];
    actor.lane = foreign.clone();
    actor.turn = Some(7);
    assert!(actor.space_delete_busy(&chats, false));
    assert!(!actor.space_delete_busy(&["outro".into()], false));
    assert!(actor.space_delete_busy(&[], true));
    actor.turn = None;
    actor.queue.push_back(Queued::User {
        lane: foreign.clone(),
        entry_id: "ram-only".into(),
        text: "não apagar".into(),
    });
    assert!(actor.space_delete_busy(&chats, false));
    actor.queue.clear();
    actor.queue.push_back(Queued::Routine("rotina-pessoal".into()));
    actor.queue.push_back(Queued::Compact { idle: true, manual: None });
    let (ask, _answer) = crate::chat::team::BotRequest::for_test("pedido-pessoal");
    actor.queue.push_back(Queued::BotRequest(ask));
    assert!(!actor.space_delete_busy(&chats, false), "trabalho do Pessoal não pertence à conversa foreign");
    assert!(actor.space_delete_busy(&chats, true), "bot nativo inclui toda a fila");
    actor.queue.clear();
    hub.team.start_turn("b", None);
    let request = hub.team.begin_for_test("colaboracao", "b", "c", &foreign).unwrap();
    assert!(actor.space_delete_busy(&chats, false));
    assert!(!*request.borrow());
    hub.team.cancel_from_session("b", &foreign);
    // Uma fixture nova sem pedidos pendentes, para provar a reserva reversível.
    let (idle, idle_dir) = fixture();
    let mut actor = actor_of(&idle);
    let (reply, result) = tokio::sync::oneshot::channel();
    let (release, held) = tokio::sync::oneshot::channel();
    let (done, _rx) = mpsc::unbounded_channel();
    let cmd = actor.on_cmd(Cmd::SpaceDeleteGuard { chats: vec!["b".into()], all: true, reply, release: held }, &done);
    tokio::pin!(cmd);
    tokio::select! {
        biased;
        _ = &mut cmd => panic!("não pode sair antes da decisão da Tx"),
        busy = result => assert!(!busy.unwrap()),
    }
    drop(release); // tanto commit quanto rollback soltam a reserva
    assert!(cmd.await);
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(idle_dir);
}
