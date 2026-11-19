//! D70 da fatia A (`sessionChoiceAnswer`, v1 casos 3–8 e 10, e a parte do consumidor dos casos 1/6): prova do
//! CONSUMIDOR, não do produtor C6. As perguntas nascem pelo produtor interno de storage (`storage::create` com
//! `Domain::PersonalSession`), sobre a origem canônica da extra gravada pelo `Store` (que a carimba com o
//! `spaceContext` da principal do bot, errata E1). Nenhuma pergunta, Authority ou vínculo é semeado por SQL/JSON; o
//! SQL só faz alterações NEGATIVAS delimitadas (carimbo estragado, linha principal divergente, gatilho TEMP de falha).
//! Conta indisponível: retirada do cadastro, como em `admissao_tests`.
use super::super::admissao_tests::{F, account, extra, fixture, fixture_with};
use super::super::{encerrar, store};
use crate::agent::bot::Cmd;
use crate::fork::engine_accounts;
use crate::fork::genui::contract::ValidatedChoice;
use crate::fork::genui::storage::{self, Context, Domain};
use crate::fork::spaces::PERSONAL;
use crate::store::{Device, DeviceSource, EntryKind, Lane, Scope};
use crate::{LockExt, api::devices::Caller, hub::Hub};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};

const METHOD: &str = "sessionChoiceAnswer";
const SCOPE: &str = "genui_question_scope_mismatch";

fn choice() -> ValidatedChoice {
    ValidatedChoice::parse(
        &json!({"version":1,"component":"choice","title":"Escolha","summary":"Dados informados pelo bot",
            "data":{"question":"Qual caminho?","options":[{"id":"a","label":"A"},{"id":"b","label":"B"}]}})
        .to_string(),
    )
    .unwrap()
}

/// A mensagem do dono gravada na própria extra pelo produtor canônico (`Store::insert_entry`, que carimba).
fn origin(hub: &Hub, lane: &Lane) -> String {
    let turn = hub.store.alloc_turn(&lane.chat).unwrap();
    hub.store.insert_entry(lane, EntryKind::User, turn, &json!({"text":"Pedido","status":"sent"})).unwrap().id
}

/// Pergunta da extra pelo produtor interno de storage, sobre a origem dada.
fn create_on(hub: &Hub, lane: &Lane, origin: &str) -> anyhow::Result<String> {
    let mut db = hub.store.db.locked();
    let tx = db.transaction()?;
    let ctx = Context { lane, space: PERSONAL, origin_entry: origin, domain: Domain::PersonalSession };
    let receipt = storage::create(&tx, &ctx, &uuid::Uuid::new_v4().to_string(), &choice(), None)?;
    tx.commit()?;
    Ok(receipt.question_id)
}

fn ask(hub: &Hub, lane: &Lane) -> String {
    create_on(hub, lane, &origin(hub, lane)).unwrap()
}

/// Alteração negativa delimitada do carimbo de UMA entrada (`json_set`/`json_remove` com o caminho dado).
fn spoil(hub: &Hub, entry: &str, sql: &str) {
    let changed =
        hub.store.db.locked().execute(&format!("UPDATE entries SET data={sql} WHERE id=?1"), [entry]).unwrap();
    assert_eq!(changed, 1);
}

fn projection(hub: &Hub, question: &str) -> String {
    let sql = "SELECT entry_id FROM donna_genui_questions WHERE id=?1";
    hub.store.db.locked().query_row(sql, [question], |r| r.get(0)).unwrap()
}

fn primary_revision(hub: &Hub, bot: &str) -> i64 {
    let sql = "SELECT revision FROM donna_space_conversations WHERE id=?1";
    hub.store.db.locked().query_row(sql, [bot], |r| r.get(0)).unwrap()
}

fn questions(hub: &Hub) -> i64 {
    hub.store.db.locked().query_row("SELECT COUNT(*) FROM donna_genui_questions", [], |r| r.get(0)).unwrap()
}

fn body(lane: &Lane, expected: i64, question: &str, revision: i64, answer: &Value) -> Value {
    json!({"botId":lane.chat,"sessionId":lane.session_id(),"expectedRevision":expected,
        "questionId":question,"revision":revision,"answer":answer})
}

async fn call(hub: &Arc<Hub>, caller: &Caller, body: Value) -> anyhow::Result<Value> {
    crate::api::dispatch(hub, caller, METHOD, body).await
}

fn rev(hub: &Hub, id: &str) -> i64 {
    store::get(&hub.store.db.locked(), id).unwrap().unwrap().revision
}

fn count(hub: &Hub) -> i64 {
    hub.store.db.locked().query_row("SELECT COUNT(*) FROM entries", [], |r| r.get(0)).unwrap()
}

fn question(hub: &Hub, id: &str) -> (String, i64) {
    let sql = "SELECT state,revision FROM donna_genui_questions WHERE id=?1";
    hub.store.db.locked().query_row(sql, [id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap()
}

fn code(error: &anyhow::Error) -> String {
    format!("{error:#}")
}

/// Nenhum evento nem `Cmd` pendente.
fn quiet(events: &mut broadcast::Receiver<Value>, commands: &mut mpsc::UnboundedReceiver<Cmd>) {
    assert!(events.try_recv().is_err(), "nenhum evento");
    assert!(commands.try_recv().is_err(), "nenhum Cmd::Send");
}

/// Aparelho pareado com Control e a autoridade ORIGINAL do transporte (o caller real do canal).
fn device(hub: &Arc<Hub>) -> (Device, Caller) {
    let device = Device {
        key: "aparelho-g1b".into(),
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
    (device, caller)
}

/// Revoga e reconcede no gancho entre a captura e a gravação: a autoridade capturada deixa de valer.
fn revoke_before_write(hub: &Arc<Hub>, device: &Device) {
    let (h, d) = (hub.clone(), device.clone());
    *hub.store.before_owner_write.locked() = Some(Box::new(move || {
        h.store.remove_device(&d.key).unwrap();
        h.store.put_device(&d).unwrap();
    }));
}

fn retire_account_b(f: &F) {
    let registry = json!({"salt":uuid::Uuid::new_v4().to_string(), "accounts":[
        {"id":account('a'),"backend":"codex","billing":"paid_api","state":"connected","launch":{"kind":"vault_profile","slot":"PRIVATE_A"},"scope":"owner","quotaIdentity":null,"verifiedAt":1,"subscriptionProof":null,"models":[{"id":"model-a","options":[]}]}
    ]});
    f.hub.store.kv_set(engine_accounts::SLOT, &registry.to_string()).unwrap();
}

/// Casos 1 (consumidor), 6 e D4: Device Control real sem `engine.accounts`; resposta nova publica só entradas após
/// o commit e entrega um `Cmd::Send` na lane da extra; replay (também após reinício e com a conta retirada) devolve o
/// mesmo recibo sem gravar, publicar nem reenviar; no mesmo cenário de conta, a resposta NOVA falha fechado.
#[tokio::test]
async fn resposta_nova_entra_na_fila_da_extra_e_replay_nao_regrava_nem_reenvia() {
    let f = fixture_with(true, true);
    let mut commands = f.hub.test_bot_commands("dev");
    let lane = extra(&f);
    let id = lane.session_id().to_owned();
    let q = ask(&f.hub, &lane);
    let (_device, caller) = device(&f.hub);
    let before_rev = rev(&f.hub, &id);
    let mut events = f.hub.events.subscribe();
    let answer = json!({"kind":"option","id":"a"});

    let first = call(&f.hub, &caller, body(&lane, before_rev, &q, 1, &answer)).await.unwrap();
    assert_eq!(first["replayed"], false);
    assert_eq!((&first["question"]["state"], &first["question"]["revision"]), (&json!("answered"), &json!(2)));
    assert_eq!(first["question"]["answer"], answer);
    assert_eq!((&first["session"]["id"], &first["session"]["revision"]), (&json!(id), &json!(before_rev)));
    assert_eq!((&first["entry"]["sessionId"], &first["entry"]["botId"]), (&json!(id), &json!("dev")));
    let response_id = first["entry"]["id"].as_str().unwrap().to_owned();
    let response = f.hub.store.entry(&response_id).unwrap();
    assert_eq!((response.kind.as_str(), &response.data["status"]), ("user", &json!("queued")));
    assert_eq!(response.data["choice_response"], json!({"version":1,"questionId":q,"revision":2}));
    assert!(!response.is_user_authored(), "escolha não é autoria nem aprovação");
    assert!(response.data.get("executionBinding").is_none() && response.data.get("contentOrigin").is_none());
    let mut published = Vec::new();
    while let Ok(event) = events.try_recv() {
        assert_eq!(event["type"], "entry", "sem evento session: a linha não muda");
        published.push(event);
    }
    let revs: Vec<i64> = published.iter().map(|e| e["rev"].as_i64().unwrap()).collect();
    assert!(revs.windows(2).all(|w| w[0] < w[1]), "ordem por rev");
    assert!(published.iter().any(|e| e["entry"]["id"] == response_id.as_str()));
    assert!(published.iter().any(|e| e["entry"]["data"]["genuiChoice"]["state"] == "answered"));
    let sent = commands.try_recv().unwrap();
    assert!(
        matches!(&sent, Cmd::Send { lane: to, entry_id, .. } if *to == lane && *entry_id == response_id),
        "esperava Cmd::Send da extra"
    );
    quiet(&mut events, &mut commands);

    let entries = count(&f.hub);
    let replay = call(&f.hub, &Caller::Local, body(&lane, before_rev, &q, 1, &answer)).await.unwrap();
    assert_eq!((&replay["replayed"], &replay["entry"]["id"]), (&json!(true), &json!(response_id)));
    assert_eq!(replay["question"], first["question"]);
    assert_eq!(count(&f.hub), entries);
    quiet(&mut events, &mut commands);

    // Reinício: outro Hub sobre o mesmo banco; o recibo é o mesmo e a entrada durável continua uma só na fila.
    let reopened = f.reopen();
    let again = call(&reopened, &Caller::Local, body(&lane, before_rev, &q, 1, &answer)).await.unwrap();
    assert_eq!((&again["replayed"], &again["entry"]["id"]), (&json!(true), &json!(response_id)));
    assert_eq!(count(&reopened), entries);
    let queued = reopened.store.queued_user_entries_for_identity("dev").unwrap();
    assert_eq!(queued.iter().filter(|e| e.id == response_id).count(), 1);

    // D4: conta da extra retirada, sessão e revisões mantidas.
    retire_account_b(&f);
    let replay = call(&f.hub, &Caller::Local, body(&lane, before_rev, &q, 1, &answer)).await.unwrap();
    assert_eq!(replay["replayed"], true, "recibo confirmado continua consultável");
    let q2 = ask(&f.hub, &lane);
    let entries = count(&f.hub);
    let error = call(&f.hub, &Caller::Local, body(&lane, rev(&f.hub, &id), &q2, 1, &answer)).await.unwrap_err();
    assert!(code(&error).contains("session_selection_unavailable"), "{error:#}");
    assert_eq!((count(&f.hub), question(&f.hub, &q2)), (entries, ("pending".into(), 1)));
    quiet(&mut events, &mut commands);
}

/// Caso 3 (C3): sessão de outro bot, principal, pergunta de outra sessão e inexistente, sem eco e sem efeito.
#[tokio::test]
async fn alvo_errado_recusa_com_codigo_unico_sem_eco() {
    let f = fixture(true);
    let mut commands = f.hub.test_bot_commands("dev");
    let mut cfg = f.hub.store.bot("dev").unwrap().unwrap().config;
    cfg.id = "outro".into();
    f.hub.store.save_bot(&cfg).unwrap();
    let (lane, irma) = (extra(&f), extra(&f));
    let alheia = {
        let new = store::NewSession {
            chat: "outro".into(),
            origin: super::super::origin::SessionOrigin::Owner,
            source_session: None,
            parent_session: None,
            title: String::new(),
            runs_on: None,
        };
        store::create(&f.hub.store.db.locked(), &new).unwrap().id
    };
    let (q, q_irma) = (ask(&f.hub, &lane), ask(&f.hub, &irma));
    let id = lane.session_id().to_owned();
    let answer = json!({"kind":"none"});
    let entries = count(&f.hub);
    let mut events = f.hub.events.subscribe();

    let mut foreign = body(&lane, rev(&f.hub, &alheia), &q, 1, &answer);
    foreign["sessionId"] = alheia.clone().into();
    let error = call(&f.hub, &Caller::Local, foreign).await.unwrap_err();
    assert_eq!(code(&error), "unknown_session");
    let mut main = body(&lane, rev(&f.hub, "dev"), &q, 1, &answer);
    main["sessionId"] = "dev".into();
    assert_eq!(code(&call(&f.hub, &Caller::Local, main).await.unwrap_err()), "genui_conversation_required");
    for other in [q_irma.as_str(), "inexistente"] {
        let error = call(&f.hub, &Caller::Local, body(&lane, rev(&f.hub, &id), other, 1, &answer)).await.unwrap_err();
        assert_eq!(code(&error), SCOPE, "mesmo código, sem IDs");
    }
    assert_eq!(count(&f.hub), entries);
    assert_eq!((question(&f.hub, &q).0, question(&f.hub, &q_irma).0), ("pending".into(), "pending".into()));
    quiet(&mut events, &mut commands);
}

/// Casos 4 e 5: audiência, envelope comum, decoder fechado e autoridade original (revogada antes da Tx, na
/// resposta nova e no replay); origem pessoal do bootstrap recusada pela guarda da main, sem exceção.
#[tokio::test]
async fn audiencia_envelope_e_revogacao_recusam_sem_efeito_inclusive_no_replay() {
    let f = fixture(true);
    let mut commands = f.hub.test_bot_commands("dev");
    let lane = extra(&f);
    let id = lane.session_id().to_owned();
    let q = ask(&f.hub, &lane);
    let answer = json!({"kind":"option","id":"b"});
    let ok = body(&lane, rev(&f.hub, &id), &q, 1, &answer);
    let entries = count(&f.hub);
    let mut events = f.hub.events.subscribe();

    let viewer = Caller::Device { key: "sem-controle".into(), scopes: vec![Scope::Screen], autoridade: None };
    assert!(call(&f.hub, &viewer, ok.clone()).await.unwrap_err().is::<crate::api::devices::Forbidden>());
    let bare = Caller::Device { key: "sem-original".into(), scopes: vec![Scope::Control], autoridade: None };
    let error = call(&f.hub, &bare, ok.clone()).await.unwrap_err();
    assert_eq!(code(&error), "not_allowed: autoridade original de controle ausente");
    let mut wrong_hub = ok.clone();
    wrong_hub["hubId"] = "outro-hub".into();
    assert!(code(&call(&f.hub, &Caller::Local, wrong_hub).await.unwrap_err()).contains("hub_context_mismatch"));
    let mut extra_key = ok.clone();
    extra_key["sobra"] = 1.into();
    let mut text_revision = ok.clone();
    text_revision["revision"] = "1".into();
    let mut no_answer = ok.clone();
    no_answer.as_object_mut().unwrap().remove("answer");
    let mut unknown_kind = ok.clone();
    unknown_kind["answer"] = json!({"kind":"desconhecido"});
    let mut answer_extra = ok.clone();
    answer_extra["answer"] = json!({"kind":"none","extra":1});
    for invalid in [extra_key, text_revision, no_answer, unknown_kind, answer_extra] {
        let error = call(&f.hub, &Caller::Local, invalid).await.unwrap_err();
        assert_eq!(code(&error), "invalid_field: pedido de resposta inválido.");
    }
    let mut empty = ok.clone();
    empty["questionId"] = "".into();
    assert_eq!(code(&call(&f.hub, &Caller::Local, empty).await.unwrap_err()), "genui_question_required");

    // Revogada entre a captura e a Tx: resposta nova recusa.
    let (device, caller) = device(&f.hub);
    revoke_before_write(&f.hub, &device);
    let error = call(&f.hub, &caller, ok.clone()).await.unwrap_err();
    assert!(code(&error).starts_with("not_allowed"), "{error:#}");
    assert_eq!((count(&f.hub), question(&f.hub, &q)), (entries, ("pending".into(), 1)));
    quiet(&mut events, &mut commands);

    // Com `hubId` igual ao do Hub (removido pelo envelope) e autoridade nova, responde; o replay revogado recusa.
    let (_, caller) = crate::fork::autoridade::autenticar(&f.hub, &device.key).unwrap();
    let mut with_hub = ok.clone();
    with_hub["hubId"] = f.hub.host_id.clone().into();
    call(&f.hub, &caller, with_hub).await.unwrap();
    while events.try_recv().is_ok() {}
    assert!(matches!(commands.try_recv().unwrap(), Cmd::Send { .. }));
    let entries = count(&f.hub);
    revoke_before_write(&f.hub, &device);
    let error = call(&f.hub, &caller, ok.clone()).await.unwrap_err();
    assert!(code(&error).starts_with("not_allowed"), "{error:#}");
    assert_eq!(count(&f.hub), entries);
    quiet(&mut events, &mut commands);

    // Ponte real do bot (vínculo canônico da principal, credencial provisionada e autenticada só em memória, sem
    // HTTP nem log do token): não é o dono e não chega à RPC.
    assert!(!crate::fork::ponte_credencial::PERMITIDOS.contains(&METHOD), "Ponte (bot/MCP) não chega");
    let binding = super::super::bindings::issue(&f.hub.store, "dev", "dev", "dev", Some("acp-ponte")).unwrap();
    super::super::bindings::activate(&f.hub.store, &binding, "acp-ponte").unwrap();
    let mut servers = vec![json!({"env":[],"args":["mcp","remote","--context","original"]})];
    crate::fork::ponte_credencial::provisionar(&f.hub, "dev", None, &binding, &mut servers).unwrap();
    let token = zeroize::Zeroizing::new(servers[0]["env"][0]["value"].as_str().unwrap().to_owned());
    let ponte = f.hub.pontes.autenticar(&f.hub, &token).unwrap();
    assert!(matches!(&ponte, Caller::Ponte { bot, .. } if bot == "dev"));
    let q2 = ask(&f.hub, &lane);
    let entries = count(&f.hub);
    let ponte_body = body(&lane, rev(&f.hub, &id), &q2, 1, &answer);
    // A recusa é da audiência: `ponte_credencial::conferir` → `devices::permit` devolve o `Forbidden` tipado (403),
    // preservado pelo `?`; nenhuma outra causa passa por este assert.
    let error = call(&f.hub, &ponte, ponte_body).await.unwrap_err();
    assert!(error.is::<crate::api::devices::Forbidden>(), "{error:#}");
    assert_eq!((count(&f.hub), question(&f.hub, &q2)), (entries, ("pending".into(), 1)));
    quiet(&mut events, &mut commands);

    // Origem pessoal do bootstrap (main331), num Hub novo e separado, como o vigia da main
    // (`autoridade::provas::origem_pessoal_recusa_autenticar_reconferir_e_conferir_do_canal_generico`): a geração
    // pessoal é instalada ANTES de qualquer Device, o mesmo aparelho é cadastrado pelo escritor canônico (o gatilho
    // marca a origem pessoal) e chega com o caller real da mesma chave, autenticado no Hub legado. Ali não se
    // autentica nem recaptura: `guard_generic` recusa com o `Forbidden` tipado antes de `com_pedido`, sem efeito.
    let (_, caller) = crate::fork::autoridade::autenticar(&f.hub, &device.key).unwrap();
    let p = fixture(true);
    let mut p_commands = p.hub.test_bot_commands("dev");
    let p_lane = extra(&p);
    let p_q = ask(&p.hub, &p_lane);
    let p_body = body(&p_lane, rev(&p.hub, p_lane.session_id()), &p_q, 1, &answer);
    let (cfg, _) = crate::fork::personal_bootstrap::provisioning::fixture().unwrap();
    p.hub.store.personal_install_generation(&cfg).unwrap();
    p.hub.store.put_device(&device).unwrap();
    assert!(p.hub.store.personal_origin(&device.key).unwrap().is_some(), "origem pessoal marcada pelo gatilho");
    let p_entries = count(&p.hub);
    let mut p_events = p.hub.events.subscribe();
    let error = call(&p.hub, &caller, p_body).await.unwrap_err();
    assert!(error.is::<crate::api::devices::Forbidden>(), "{error:#}");
    assert_eq!((count(&p.hub), question(&p.hub, &p_q)), (p_entries, ("pending".into(), 1)));
    quiet(&mut p_events, &mut p_commands);
    quiet(&mut events, &mut commands);
}

/// Caso 7: CAS da sessão antes de qualquer código da pergunta; revisão da pergunta antes do estado.
#[tokio::test]
async fn cas_da_sessao_vem_antes_e_revisao_da_pergunta_antes_do_estado() {
    let f = fixture(true);
    let _commands = f.hub.test_bot_commands("dev");
    let lane = extra(&f);
    let id = lane.session_id().to_owned();
    let q = ask(&f.hub, &lane);
    let current = rev(&f.hub, &id);
    let entries = count(&f.hub);
    let stale = call(&f.hub, &Caller::Local, body(&lane, current - 1, &q, 99, &json!({"kind":"none"}))).await;
    assert!(code(&stale.unwrap_err()).contains("session_revision_conflict"));
    assert_eq!(count(&f.hub), entries);
    let answer = json!({"kind":"option","id":"a"});
    call(&f.hub, &Caller::Local, body(&lane, current, &q, 1, &answer)).await.unwrap();
    let entries = count(&f.hub);
    for (revision, code_) in [(1, "genui_revision_conflict"), (2, "genui_final_state")] {
        let error = call(&f.hub, &Caller::Local, body(&lane, current, &q, revision, &json!({"kind":"none"})))
            .await
            .unwrap_err();
        assert_eq!(code(&error), code_);
    }
    assert_eq!(count(&f.hub), entries);
    // Sessão renomeada depois da resposta: revisão velha conflita; a nova devolve o replay.
    let renamed = crate::api::dispatch(
        &f.hub,
        &Caller::Local,
        "sessionRename",
        json!({"botId":"dev","sessionId":id,"title":"Nova","expectedRevision":current}),
    )
    .await
    .unwrap();
    let error = call(&f.hub, &Caller::Local, body(&lane, current, &q, 1, &answer)).await.unwrap_err();
    assert!(code(&error).contains("session_revision_conflict"));
    let newer = renamed["session"]["revision"].as_i64().unwrap();
    let replay = call(&f.hub, &Caller::Local, body(&lane, newer, &q, 1, &answer)).await.unwrap();
    assert_eq!(replay["replayed"], true);
    assert_eq!(count(&f.hub), entries);
}

fn close(hub: &Hub, id: &str) -> encerrar::Closed {
    let mut db = hub.store.db.locked();
    let tx = db.transaction().unwrap();
    let revision = store::get(&tx, id).unwrap().unwrap().revision;
    let closed = encerrar::close_on(&tx, "dev", id, revision).unwrap();
    tx.commit().unwrap();
    closed
}

/// Caso 8: fechamento comitado antes recusa sem efeito; resposta comitada e ainda `queued` no fechamento vira
/// `cancelled`, a pergunta continua `answered` e a cerca do claim recusa a extra fechada.
#[tokio::test]
async fn fechamento_antes_recusa_e_depois_cancela_a_resposta_na_fila() {
    let f = fixture(true);
    let _commands = f.hub.test_bot_commands("dev");
    let (antes, depois) = (extra(&f), extra(&f));
    let (q1, q2) = (ask(&f.hub, &antes), ask(&f.hub, &depois));
    close(&f.hub, antes.session_id());
    assert_eq!(question(&f.hub, &q1).0, "cancelled", "o fechamento cancela a pendente da extra");
    let entries = count(&f.hub);
    let late = body(&antes, rev(&f.hub, antes.session_id()), &q1, 2, &json!({"kind":"none"}));
    assert_eq!(code(&call(&f.hub, &Caller::Local, late).await.unwrap_err()), "session_closed");
    assert_eq!(count(&f.hub), entries);

    let answered =
        call(&f.hub, &Caller::Local, body(&depois, rev(&f.hub, depois.session_id()), &q2, 1, &json!({"kind":"none"})))
            .await
            .unwrap();
    let response = answered["entry"]["id"].as_str().unwrap().to_owned();
    let closed = close(&f.hub, depois.session_id());
    assert!(closed.cancelled.contains(&response));
    assert_eq!(f.hub.store.entry(&response).unwrap().data["status"], "cancelled");
    assert_eq!(question(&f.hub, &q2).0, "answered");
    let fence = encerrar::assert_open_on(&f.hub.store.db.locked(), "dev", depois.session.as_deref());
    assert_eq!(code(&fence.unwrap_err()), "session_closed");
}

/// Caso 10: falha SQL no CAS da pergunta (gatilho TEMP) descarta tudo, inclusive entrada e marcador provisórios.
#[tokio::test]
async fn falha_no_meio_da_tx_nao_deixa_resposta_orfa_nem_publica() {
    let f = fixture(true);
    let mut commands = f.hub.test_bot_commands("dev");
    let lane = extra(&f);
    let q = ask(&f.hub, &lane);
    // Troca canônica de lane: a última entrada do chat passa a ser da principal, então a resposta na extra exige
    // marcador de sessão (decidido na inserção, na mesma Tx).
    let turn = f.hub.store.alloc_turn("dev").unwrap();
    f.hub.store.insert_entry(&Lane::main("dev"), EntryKind::User, turn, &json!({"text":"Na principal"})).unwrap();
    assert!(super::super::marker::needed(&f.hub.store.db.locked(), &lane).unwrap().is_some(), "marcador seria gerado");
    let markers = || -> i64 {
        let sql = "SELECT COUNT(*) FROM entries WHERE json_extract(data,'$.sessionMarker.sessionId')=?1";
        f.hub.store.db.locked().query_row(sql, [lane.session_id()], |r| r.get(0)).unwrap()
    };
    let before_markers = markers();
    let entries = count(&f.hub);
    let ok = body(&lane, rev(&f.hub, lane.session_id()), &q, 1, &json!({"kind":"option","id":"a"}));
    f.hub
        .store
        .db
        .locked()
        .execute_batch(
            "CREATE TEMP TRIGGER g1b_falha BEFORE UPDATE OF state ON donna_genui_questions
             BEGIN SELECT RAISE(ABORT, 'falha_injetada'); END;",
        )
        .unwrap();
    let mut events = f.hub.events.subscribe();
    let error = call(&f.hub, &Caller::Local, ok.clone()).await.unwrap_err();
    assert!(code(&error).contains("falha_injetada"), "{error:#}");
    assert_eq!((count(&f.hub), question(&f.hub, &q)), (entries, ("pending".into(), 1)));
    assert_eq!(markers(), before_markers, "marcador provisório descartado");
    quiet(&mut events, &mut commands);
    f.hub.store.db.locked().execute_batch("DROP TRIGGER g1b_falha;").unwrap();
    assert_eq!(call(&f.hub, &Caller::Local, ok).await.unwrap()["replayed"], false);
    // Controle: sem a falha, o mesmo pedido grava o marcador e a resposta.
    assert_eq!((markers(), count(&f.hub)), (before_markers + 1, entries + 2));
}

/// Errata E1, origem: a pergunta só nasce sobre o carimbo canônico da principal do mesmo bot no Pessoal. Ausência,
/// JSON `null` (inválido, não "ausente"), tipo errado, outro bot/chat/Space, conversa adicional (o `conversationId` da
/// extra) e revisão fora de 1..=atual recusam com `genui_scope_mismatch`, sem criar nada. Controle: a origem intacta
/// do mesmo produtor cria.
#[tokio::test]
async fn e1_origem_exige_carimbo_canonico_da_principal_do_mesmo_bot() {
    let f = fixture(true);
    let lane = extra(&f);
    let current = primary_revision(&f.hub, "dev");
    let extra_id = lane.session_id().to_owned();
    let spoils = [
        "json_remove(data,'$.spaceContext')".to_owned(),
        "json_set(data,'$.spaceContext',json('null'))".to_owned(),
        "json_set(data,'$.spaceContext.botId','outro')".to_owned(),
        "json_remove(data,'$.spaceContext.botId')".to_owned(),
        format!("json_set(data,'$.spaceContext.conversationId','{extra_id}')"),
        "json_set(data,'$.spaceContext.spaceId','outro-space')".to_owned(),
        "json_set(data,'$.spaceContext.version',2)".to_owned(),
        "json_set(data,'$.spaceContext.version','1')".to_owned(),
        "json_set(data,'$.spaceContext.conversationRevision',0)".to_owned(),
        format!("json_set(data,'$.spaceContext.conversationRevision',{})", current + 1),
        "json_set(data,'$.spaceContext.conversationRevision','1')".to_owned(),
    ];
    let before = questions(&f.hub);
    for sql in &spoils {
        let entry = origin(&f.hub, &lane);
        spoil(&f.hub, &entry, sql);
        let error = create_on(&f.hub, &lane, &entry).unwrap_err();
        assert_eq!(code(&error), "genui_scope_mismatch", "{sql}");
    }
    assert_eq!(questions(&f.hub), before, "nenhuma pergunta criada");
    let intact = origin(&f.hub, &lane);
    let stamp = f.hub.store.entry(&intact).unwrap().data["spaceContext"].clone();
    assert_eq!(
        (&stamp["conversationId"], &stamp["botId"], &stamp["spaceId"]),
        (&json!("dev"), &json!("dev"), &json!(PERSONAL))
    );
    create_on(&f.hub, &lane, &intact).unwrap();
}

/// Errata E1, projeção e principal: carimbo da projeção estragado recusa a resposta sem efeito; revisão histórica do
/// carimbo (o bot salvo depois sobe a revisão da principal) continua válida; linha principal fora do Pessoal (o
/// conflito da pendência lateral de troca de Space) falha fechado.
#[tokio::test]
async fn e1_projecao_estrita_revisao_historica_valida_e_principal_divergente_recusa() {
    let f = fixture(true);
    let mut commands = f.hub.test_bot_commands("dev");
    let lane = extra(&f);
    let id = lane.session_id().to_owned();
    let answer = json!({"kind":"none"});
    for sql in [
        "json_remove(data,'$.spaceContext')",
        "json_set(data,'$.spaceContext',json('null'))",
        "json_set(data,'$.spaceContext.botId','outro')",
    ] {
        let q = ask(&f.hub, &lane);
        spoil(&f.hub, &projection(&f.hub, &q), sql);
        let entries = count(&f.hub);
        let error = call(&f.hub, &Caller::Local, body(&lane, rev(&f.hub, &id), &q, 1, &answer)).await.unwrap_err();
        assert_eq!(code(&error), "genui_scope_mismatch", "{sql}");
        assert_eq!((count(&f.hub), question(&f.hub, &q)), (entries, ("pending".into(), 1)));
    }
    assert!(commands.try_recv().is_err());

    let q = ask(&f.hub, &lane);
    let stamped = f.hub.store.entry(&projection(&f.hub, &q)).unwrap().data["spaceContext"]["conversationRevision"]
        .as_i64()
        .unwrap();
    let cfg = f.hub.store.bot("dev").unwrap().unwrap().config;
    f.hub.store.save_bot(&cfg).unwrap();
    assert!(primary_revision(&f.hub, "dev") > stamped, "salvar o bot sobe a revisão da principal");
    let answered = call(&f.hub, &Caller::Local, body(&lane, rev(&f.hub, &id), &q, 1, &answer)).await.unwrap();
    assert_eq!(answered["replayed"], false, "revisão histórica não invalida a pergunta");

    let q = ask(&f.hub, &lane);
    let entries = count(&f.hub);
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE donna_space_conversations SET space_id='outro' WHERE id='dev'", [])
        .unwrap();
    let error = call(&f.hub, &Caller::Local, body(&lane, rev(&f.hub, &id), &q, 1, &answer)).await.unwrap_err();
    assert_eq!(code(&error), "genui_scope_mismatch");
    assert_eq!((count(&f.hub), question(&f.hub, &q)), (entries, ("pending".into(), 1)));
}

/// Errata E1, domínios: a RPC de conversa de Space na PRINCIPAL não responde pergunta de extra; `sessionChoiceAnswer`
/// não responde pergunta de conversa de Space. Cada caminho mantém o próprio ramo, sem decidir pelo carimbo.
#[tokio::test]
async fn e1_rpc_de_space_e_rpc_de_sessao_nao_respondem_pergunta_do_outro_dominio() {
    let f = fixture(true);
    let _commands = f.hub.test_bot_commands("dev");
    let lane = extra(&f);
    let q = ask(&f.hub, &lane);
    let entries = count(&f.hub);
    let principal = f.hub.store.space_conversation(PERSONAL, "dev").unwrap();
    let space_body = json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"conversationId":"dev",
        "expectedRevision":principal.revision,"questionId":q,"revision":1,"answer":{"kind":"none"}});
    let error =
        crate::api::dispatch(&f.hub, &Caller::Local, "donnaConversationChoiceAnswer", space_body).await.unwrap_err();
    assert!(["genui_conversation_required", SCOPE].contains(&code(&error).as_str()), "{error:#}");
    assert_eq!((count(&f.hub), question(&f.hub, &q)), (entries, ("pending".into(), 1)));

    // Inverso: pergunta real de conversa de Space (produtor `flow::show` da conversa capturada).
    let space = crate::fork::spaces::tool_context::tests::Fixture::new();
    let shown = crate::fork::genui::flow::show(&space.hub, &space.context(), "ponte:1", &choice()).unwrap();
    let space_q = shown["questionId"].as_str().unwrap().to_owned();
    let bot = space.cfg.id.clone();
    let new = store::NewSession {
        chat: bot.clone(),
        origin: super::super::origin::SessionOrigin::Owner,
        source_session: None,
        parent_session: None,
        title: String::new(),
        runs_on: None,
    };
    let other = store::create(&space.hub.store.db.locked(), &new).unwrap().id;
    let other_lane = Lane { chat: bot, session: Some(other.clone()) };
    let entries = count(&space.hub);
    let session_body = body(&other_lane, rev(&space.hub, &other), &space_q, 1, &json!({"kind":"none"}));
    let error = call(&space.hub, &Caller::Local, session_body).await.unwrap_err();
    assert_eq!(code(&error), SCOPE);
    assert_eq!((count(&space.hub), question(&space.hub, &space_q)), (entries, ("pending".into(), 1)));
}
