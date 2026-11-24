//! MX-145 E3: teto de gasto — estado fail-closed, liberações, decisão na Tx do
//! permit, espera durável e fronteira dos envios de fundo.
#![allow(clippy::unwrap_used)]
use super::*;
use crate::{
    api::devices::Caller,
    fork::{
        bot_admission::{self, Origin},
        teto,
    },
    store::Scope,
};
use rusqlite::params;

const KEY: &str = "donna.spend-ceiling.v1";

fn hub_tmp() -> (Arc<Hub>, PathBuf) {
    let dir = std::env::temp_dir().join(format!("teto-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let hub = Hub::new(
        Store::open(&dir.join("test.db")).unwrap(),
        "hub".into(),
        crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
        "fixture".into(),
        0,
    );
    (hub, dir)
}

fn config(hub: &Hub, settings: &Value) {
    hub.store.kv_set(KEY, &settings.to_string()).unwrap();
}

fn ledger(store: &Store, at: i64, price: Option<(f64, &str)>, tokens: Option<(i64, i64)>) {
    store
        .db
        .locked()
        .execute(
            "INSERT INTO donna_turn_usage(id,bot_id,chat_id,session_id,at,input_tokens,output_tokens,amount,currency,source)
             VALUES(?1,'b','b','s',?2,?3,?4,?5,?6,'motor')",
            params![
                uuid::Uuid::new_v4().to_string(),
                at,
                tokens.map(|t| t.0),
                tokens.map(|t| t.1),
                price.map(|p| p.0),
                price.map(|p| p.1)
            ],
        )
        .unwrap();
}

fn now_in_month(_store: &Store) -> i64 {
    crate::fork::custo::month(None).unwrap().1 + 1
}

fn gate(store: &Store) -> teto::Gate {
    let db = store.db.locked();
    let settings = teto::settings_on(&db).unwrap();
    teto::gate_on(&db, &settings).unwrap()
}

/// A decisão real na fronteira: o mesmo `decide_on` pendurado na Tx do permit.
fn decide(store: &Store, origin: &Origin) -> teto::Verdict {
    let db = store.db.locked();
    let body = json!({"attempt":"a1","executionToken":"e1","origin":teto::origin_kind(origin),
        "originDetail":{"routineId":"r"},"lane":{"chat":"b","session":null},"setup":{}});
    teto::decide_on(&db, "bot", origin, &body).unwrap().verdict
}

fn hold(store: &Store) -> Option<Value> {
    holds(store).into_iter().next().map(|row| row.body)
}

fn holds(store: &Store) -> Vec<teto::HoldRow> {
    teto::turn_holds_of(&store.db.locked(), "bot").unwrap()
}

/// Alvo do `checkFailure`: a assinatura completa (`fingerprint#geração`) que o dono viu.
fn fp(hold: &Value) -> String {
    hold["signature"].as_str().unwrap().to_string()
}

async fn release(hub: &Arc<Hub>, kind: &str, alvo: &str) -> anyhow::Result<Value> {
    teto::call(hub, &Caller::Local, "donnaCeilingRelease", json!({"kind": kind, "alvo": alvo})).await
}

#[test]
fn teto_sem_preco_trava_dinheiro_mesmo_com_tokens_verdes() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let at = now_in_month(&store);
    ledger(&store, at, Some((0.5, "USD")), Some((10, 10)));
    ledger(&store, at, None, Some((10, 10)));
    config_for(&store, &json!({"enabled":true,"money":[{"currency":"USD","limit":10.0}],"tokens":1000}));
    let g = gate(&store);
    let blocked = g.blocked.unwrap();
    assert_eq!(blocked.reason, "unverifiable");
    assert_eq!(blocked.metrics, ["money:USD"]);
    let usd = &g.metrics[0];
    assert_eq!(usd.used, 0.5, "o conhecido é o piso");
    assert!(!usd.verified);
    assert!(g.metrics.iter().find(|m| m.id == "tokens").unwrap().verified);
}

#[test]
fn teto_token_nulo_trava_tokens() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    ledger(&store, now_in_month(&store), Some((0.5, "USD")), Some((10, 10)));
    ledger(&store, now_in_month(&store), Some((0.5, "USD")), None);
    config_for(&store, &json!({"enabled":true,"tokens":1000}));
    let g = gate(&store);
    assert_eq!(g.blocked.unwrap().reason, "unverifiable");
    assert_eq!(g.metrics[0].used, 20.0, "conhecido conta, NULL não zera");
}

#[test]
fn teto_dois_tetos_um_conferido_nao_substitui_o_outro() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let at = now_in_month(&store);
    ledger(&store, at, Some((0.5, "USD")), Some((10, 10)));
    ledger(&store, at, Some((0.5, "USD")), None);
    config_for(&store, &json!({"enabled":true,"money":[{"currency":"USD","limit":10.0}],"tokens":1000}));
    let g = gate(&store);
    let blocked = g.blocked.unwrap();
    assert_eq!(blocked.reason, "unverifiable");
    assert_eq!(blocked.metrics, ["tokens"], "dinheiro conferido não abre tokens");
}

#[test]
fn teto_valor_conhecido_atinge_limite_trava_por_valor() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    ledger(&store, now_in_month(&store), Some((12.0, "USD")), Some((10, 10)));
    config_for(&store, &json!({"enabled":true,"money":[{"currency":"USD","limit":10.0}],"tokens":1000}));
    let g = gate(&store);
    let blocked = g.blocked.unwrap();
    assert_eq!(blocked.reason, "valueReached");
    assert_eq!(blocked.metrics, ["money:USD"]);
}

/// MX-159: gasto estimado (`source` `estimado`) entra na conta de dinheiro do
/// teto como o informado; turno sem preço continua tornando a conferência
/// `Unverified` (fail-closed).
#[test]
fn teto_estimado_conta_como_dinheiro_e_bloqueia() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let at = now_in_month(&store);
    // Linha estimada gravada pelo caminho real do ledger de fundo.
    crate::fork::custo::record_background(
        &store,
        "bot",
        "titles",
        Some("claude-haiku-4-5"),
        None,
        Some(crate::fork::custo::UsoFundo {
            input: 60,
            output: 50,
            read: Some(7),
            write: Some(0),
            invalid: false,
            ..Default::default()
        }),
    )
    .unwrap();
    // Linha estimada escrita direto (mesmo efeito de um turno ACP estimado).
    store
        .db
        .locked()
        .execute(
            "INSERT INTO donna_turn_usage(id,bot_id,chat_id,session_id,at,input_tokens,output_tokens,amount,currency,source)
             VALUES('e2','b','b','s',?1,10,10,9.5,'USD','estimado')",
            params![at],
        )
        .unwrap();
    config_for(&store, &json!({"enabled":true,"money":[{"currency":"USD","limit":9.0}]}));
    let g = gate(&store);
    let usd = &g.metrics[0];
    assert!(usd.verified, "estimado é conferível, não é lacuna");
    assert!((usd.used - (9.5 + 0.000_310_7)).abs() < 1e-9, "informado+estimado somam: {}", usd.used);
    let blocked = g.blocked.unwrap();
    assert_eq!(blocked.reason, "valueReached");
    assert_eq!(blocked.metrics, ["money:USD"]);
}

#[test]
fn teto_mes_novo_zera_estado() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let (_, start, _) = crate::fork::custo::month(None).unwrap();
    ledger(&store, start - 40 * 86_400_000, None, None);
    config_for(&store, &json!({"enabled":true,"money":[{"currency":"USD","limit":10.0}],"tokens":1000}));
    assert!(gate(&store).blocked.is_none());
}

#[test]
fn teto_tokens_contam_em_inteiro_alem_de_2_53() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    // usado = 2^53, limite = 2^53+1: em f64 os dois são o MESMO número e a
    // comparação travaria; em inteiro falta exatamente um token.
    config_for(&store, &json!({"enabled":true,"tokens":9_007_199_254_740_993_u64}));
    ledger(&store, now_in_month(&store), Some((0.1, "USD")), Some((9_007_199_254_740_992, 0)));
    #[allow(clippy::cast_precision_loss, reason = "Prova de que f64 não separa os dois valores.")]
    let collapsed = (9_007_199_254_740_992_u64 as f64).to_bits() == (9_007_199_254_740_993_u64 as f64).to_bits();
    assert!(collapsed, "pré-condição: f64 confunde 2^53 e 2^53+1");
    assert!(gate(&store).blocked.is_none(), "2^53 < 2^53+1 em inteiro");
    ledger(&store, now_in_month(&store), Some((0.1, "USD")), Some((0, 1)));
    assert_eq!(gate(&store).blocked.unwrap().reason, "valueReached", "2^53+1 atinge o limite exato");
}

#[tokio::test]
async fn teto_decisao_na_tx_grava_espera_e_apaga_reserva() {
    let f = Fixture::new();
    let store = &f.hub.store;
    config(&f.hub, &json!({"enabled":true,"tokens":100}));
    ledger(store, now_in_month(store), Some((0.1, "USD")), None);
    let cfg = f.hub.store.bot("bot").unwrap().unwrap().config;
    let ticket = bot_admission::reserve(store, &cfg, "bot", "exec", &Origin::Routine { id: "r1".into() }).unwrap();
    let mut gate = move |c: &rusqlite::Connection, record: &bot_admission::Dispatch| -> anyhow::Result<teto::Verdict> {
        let body = json!({"attempt":record.attempt,"origin":"routine","originDetail":{"routineId":"r1"}});
        teto::decide_on(c, "bot", &record.origin, &body).map(|d| d.verdict)
    };
    let permitted = bot_admission::permit_with(
        store,
        &cfg,
        &ticket,
        None,
        &[],
        None,
        bot_admission::ClaimHooks { gate: Some(&mut gate), ..Default::default() },
    );
    assert!(permitted.unwrap().is_none(), "teto trava dentro da Tx do permit");
    assert!(hold(store).is_some(), "espera durável gravada na mesma Tx");
    assert!(bot_admission::current(store, "bot").unwrap().is_none(), "reserva apagada na mesma Tx");
    // Mesma Tx, origem isenta: o dono sem a opção passa livre com rotina parada.
    let human = bot_admission::reserve(store, &cfg, "bot", "exec2", &Origin::Human).unwrap();
    let mut gate2 =
        move |c: &rusqlite::Connection, record: &bot_admission::Dispatch| -> anyhow::Result<teto::Verdict> {
            teto::decide_on(c, "bot", &record.origin, &json!({"attempt":record.attempt,"origin":"human"}))
                .map(|d| d.verdict)
        };
    let blocked_binding = crate::fork::sessoes::bindings::fixture_binding(store, &cfg.id);
    let mut dispatch = |_: &rusqlite::Connection, _: &crate::fork::sessoes::bindings::BindingRef| {
        Ok(bot_admission::RoutingClass::NotRouted { reason: bot_admission::NotRoutedReason::UnprovenSelection })
    };
    let permitted = bot_admission::permit_with(
        store,
        &cfg,
        &human,
        None,
        &[],
        None,
        bot_admission::ClaimHooks {
            binding: Some(&blocked_binding),
            dispatch: Some(&mut dispatch),
            gate: Some(&mut gate2),
            ..Default::default()
        },
    );
    assert!(permitted.unwrap().is_some(), "humano isento recebe permitted na mesma Tx");
    assert!(hold(store).is_some(), "espera da rotina intocada pelo despacho isento");
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn teto_check_failure_velho_nao_reabre_estado_novo() {
    let (hub, dir) = hub_tmp();
    let store = &hub.store;
    config(&hub, &json!({"enabled":true,"tokens":100}));
    ledger(store, now_in_month(store), Some((0.1, "USD")), None);
    assert_eq!(decide(store, &Origin::Routine { id: "r".into() }), teto::Verdict::Hold);
    let fp = fp(&hold(store).unwrap());
    release(&hub, "checkFailure", &fp).await.unwrap();
    assert_eq!(decide(store, &Origin::Routine { id: "r".into() }), teto::Verdict::Pass);
    // +110 tokens conhecidos mudam o estado observado: a geração anda e a
    // checkFailure velha morre com a assinatura que a gerou.
    ledger(store, now_in_month(store), Some((0.1, "USD")), Some((100, 10)));
    assert_eq!(decide(store, &Origin::Routine { id: "r".into() }), teto::Verdict::Hold);
    assert_eq!(hold(store).unwrap()["reason"], "valueReached");
    release(&hub, "plus25", "tokens").await.unwrap();
    // De volta à assinatura de não conferido: geração nova, liberação velha morta.
    assert_eq!(decide(store, &Origin::Routine { id: "r".into() }), teto::Verdict::Hold);
    let mark = hold(store).unwrap();
    assert_eq!(mark["reason"], "unverifiable");
    assert!(mark["signature"].as_str().unwrap().ends_with("#3"), "geração nova: {}", mark["signature"]);
    drop(hub);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn teto_check_failure_libera_falha_de_leitura() {
    let (hub, dir) = hub_tmp();
    let store = &hub.store;
    config(&hub, &json!({"enabled":true,"money":[{"currency":"USD","limit":10.0}]}));
    store.db.locked().execute_batch("ALTER TABLE donna_turn_usage RENAME TO broken_turn_usage").unwrap();
    assert_eq!(decide(store, &Origin::Routine { id: "r".into() }), teto::Verdict::Hold);
    let fp = fp(&hold(store).unwrap());
    assert!(fp.starts_with("readFailure:"), "falha de leitura tem identidade: {fp}");
    // Liberação explícita do dono para ESTA falha — alvo = a impressão readFailure.
    release(&hub, "checkFailure", &fp).await.unwrap();
    assert_eq!(decide(store, &Origin::Routine { id: "r".into() }), teto::Verdict::Pass);
    // A ledger sara e o gate volta a conferir normalmente.
    store.db.locked().execute_batch("ALTER TABLE broken_turn_usage RENAME TO donna_turn_usage").unwrap();
    assert_eq!(decide(store, &Origin::Routine { id: "r".into() }), teto::Verdict::Pass);
    // Falha de novo = geração nova: a checkFailure velha não vale mais.
    store.db.locked().execute_batch("ALTER TABLE donna_turn_usage RENAME TO broken_turn_usage").unwrap();
    assert_eq!(decide(store, &Origin::Routine { id: "r".into() }), teto::Verdict::Hold);
    store.db.locked().execute_batch("ALTER TABLE broken_turn_usage RENAME TO donna_turn_usage").unwrap();
    drop(hub);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn teto_espera_duravel_estavel_sem_regravar() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    config_for(&store, &json!({"enabled":true,"tokens":100}));
    ledger(&store, now_in_month(&store), Some((0.1, "USD")), None);
    assert_eq!(decide(&store, &Origin::Routine { id: "r".into() }), teto::Verdict::Hold);
    let first = hold(&store).unwrap();
    let at = first["at"].as_i64().unwrap();
    std::thread::sleep(Duration::from_millis(6));
    // Retentativa do mesmo estado: `at` preservado e nenhuma gravação nova.
    assert_eq!(decide(&store, &Origin::Routine { id: "r".into() }), teto::Verdict::Hold);
    let again = hold(&store).unwrap();
    assert_eq!(again["at"].as_i64().unwrap(), at);
    assert_eq!(again, first, "marca byte a byte idêntica no mesmo estado");
}

/// P3-10/P3-11: a fronteira de fundo grava UMA espera por job, emite só quando
/// algo mudou, e a espera some na mudança de estado (liberação) — sem depender de
/// um novo job de fundo passar.
#[tokio::test]
async fn teto_background_gate_trava_emite_so_na_mudanca_e_limpa_na_liberacao() {
    let (hub, dir) = hub_tmp();
    let store = &hub.store;
    config(&hub, &json!({"enabled":true,"tokens":100}));
    ledger(store, now_in_month(store), Some((0.1, "USD")), None);
    let mut events = hub.events.subscribe();
    let mut ceiling_events = move || {
        let mut n = 0;
        while let Ok(event) = events.try_recv() {
            n += usize::from(event["type"] == "ceiling");
        }
        n
    };
    for job in ["titles", "memory", "dailySummary"] {
        assert!(teto::background_gate(&hub, "bot", job).is_err(), "{job} trava na fronteira");
    }
    assert_eq!(ceiling_events(), 3, "uma espera nova por job");
    for job in ["titles", "memory", "dailySummary"] {
        assert!(teto::background_gate(&hub, "bot", job).is_err());
    }
    assert_eq!(ceiling_events(), 0, "mesma parada: nenhum evento repetido");
    let mark = teto::read_hold(&store.db.locked(), "bot", "bg:titles").unwrap().unwrap();
    assert_eq!(mark["reason"], "unverifiable");
    let fp = fp(&mark);
    release(&hub, "checkFailure", &fp).await.unwrap();
    // A própria liberação reavalia: o estado abriu, as esperas de fundo saem.
    assert!(
        teto::holds_on(&store.db.locked()).unwrap().iter().all(|h| h["kind"] != "background"),
        "nenhuma espera de fundo órfã depois da liberação"
    );
    assert!(teto::background_gate(&hub, "bot", "titles").is_ok(), "liberado abre a fronteira");
    drop(hub);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn teto_plus_recusa_limite_que_vira_infinito() {
    let (hub, dir) = hub_tmp();
    let store = &hub.store;
    config(&hub, &json!({"enabled":true,"money":[{"currency":"USD","limit":1.6e308}]}));
    ledger(store, now_in_month(store), Some((1.6e308, "USD")), Some((10, 10)));
    assert_eq!(gate(store).blocked.unwrap().reason, "valueReached");
    let error = release(&hub, "plus25", "money:USD").await.unwrap_err();
    assert!(format!("{error:#}").contains("ceiling_release_overflow"), "{error:#}");
    drop(hub);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn teto_send_admitted_estaciona_humano_e_libera_para_setup() {
    let f = Fixture::new();
    let hub = &f.hub;
    config(hub, &json!({"enabled":true,"tokens":10,"applyToConversations":true}));
    ledger(&hub.store, now_in_month(&hub.store), Some((0.1, "USD")), None);
    let lane = crate::store::Lane::main("bot");
    let entry = hub.store.insert_entry(&lane, EntryKind::User, 1, &json!({"text":"oi","status":"queued"})).unwrap();
    let mut actor = f.actor();
    actor.input = Some(crate::agent::bot::waiting::TurnInput {
        chat: "bot".into(),
        entries: vec![entry.id.clone()],
        text: "oi".into(),
        hidden: false,
        handoff: None,
        routine: None,
        transient: false,
        backend: "codex".into(),
        model: None,
        command: None,
        session: None,
        context_binding: None,
        visible_session: None,
    });
    let ticket = bot_admission::reserve(&hub.store, &actor.cfg, "bot", "exec", &Origin::Human).unwrap();
    actor.dispatch = Some(ticket);
    // R3-P3-2: um preparo SUCESSOR (outro token) não é do ticket descartado e fica.
    hub.set_memory_runtime("bot", |r| r.preparing_turn_token = Some("sucessor".into()));
    let (done_tx, _done_rx) = tokio::sync::mpsc::unbounded_channel();
    actor
        .send_admitted(json!({"prompt":[{"type":"text","text":"oi"}]}), vec![entry.id.clone()], "s".into(), &done_tx)
        .await
        .unwrap();
    assert_eq!(hub.runtime("bot").preparing_turn_token.as_deref(), Some("sucessor"), "sucessor preservado");
    assert!(actor.pending_admission.is_none(), "espera do teto não ocupa a admissão");
    assert!(actor.turn.is_none() && actor.dispatch.is_none(), "fila e turno livres para origens isentas");
    assert_eq!(actor.ceiling_holds.len(), 1);
    assert!(hold(&hub.store).is_some(), "espera durável gravada");
    assert!(bot_admission::current(&hub.store, "bot").unwrap().is_none(), "reserva apagada na Tx");
    // O orçamento de preparo não foi consumido: a reserva nova cabe de novo.
    let ticket2 = bot_admission::reserve(&hub.store, &actor.cfg, "bot", "exec2", &Origin::Human).unwrap();
    bot_admission::drop_inflight(&hub.store.db.locked(), "bot").unwrap();
    drop(ticket2);
    // Liberação do dono devolve a captura como Pending::Setup — orçamento intacto.
    let fp = fp(&hold(&hub.store).unwrap());
    release(hub, "checkFailure", &fp).await.unwrap();
    actor.ceiling_next_check = None;
    actor.retry_ceiling_holds(&done_tx).await;
    assert!(
        matches!(actor.pending_admission, Some(crate::agent::bot::fork_admission::Pending::Setup { .. })),
        "captura volta como Setup para re-preparo"
    );
    assert!(hold(&hub.store).is_none(), "espera durável limpa na retomada");
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn teto_espera_duravel_sobrevive_restart_e_retoma_rotina() {
    let f = Fixture::new();
    let hub = &f.hub;
    config(hub, &json!({"enabled":true,"tokens":100}));
    ledger(&hub.store, now_in_month(&hub.store), Some((0.1, "USD")), None);
    // O ator original estaciona uma rotina pelo caminho real da Tx.
    decide_with_body(hub, "routine", &json!({"routineId":"r9"}));
    // Reinício: ator novo, sem memória — a marca durável reconstrói a espera.
    let mut actor = f.actor();
    actor.restore_ceiling_hold().unwrap();
    assert_eq!(actor.ceiling_holds.len(), 1);
    let stops = teto::call(hub, &Caller::Local, "donnaCeiling", json!({})).await.unwrap()["stopped"].clone();
    assert_eq!(stops.as_array().unwrap().len(), 1, "stops_on lista após restart");
    // Ainda travado: o hold fica; liberado: a rotina volta à fila pelo caminho original.
    let (done_tx, _done_rx) = tokio::sync::mpsc::unbounded_channel();
    actor.retry_ceiling_holds(&done_tx).await;
    assert_eq!(actor.ceiling_holds.len(), 1, "continua parado enquanto trava");
    let fp = fp(&hold(&hub.store).unwrap());
    release(hub, "checkFailure", &fp).await.unwrap();
    actor.ceiling_next_check = None;
    // Fila em espera curta: `next_in_queue` conserva o item — prova que a rotina
    // volta à fila pelo caminho normal, nunca despachada por fora.
    actor.queue_retry_at = Some(std::time::Instant::now() + Duration::from_secs(60));
    actor.retry_ceiling_holds(&done_tx).await;
    assert!(actor.ceiling_holds.is_empty());
    assert!(
        matches!(actor.queue.front(), Some(crate::agent::bot::Queued::Routine(id)) if id == "r9"),
        "rotina volta à fila para re-drive pelo caminho normal"
    );
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// P2-2 + três boots COMPLETOS (Hub novo, atores, agendador, restauração da
/// fila): uma rotina e uma fala do dono travadas no mesmo bot têm uma linha
/// durável cada, na ordem; nenhum boot duplica nem perde uma delas; a liberação
/// envia cada uma exatamente uma vez.
#[tokio::test]
async fn teto_duas_esperas_do_bot_sobrevivem_tres_boots_e_saem_uma_vez() {
    let mut f = Fixture::new();
    config(&f.hub, &json!({"enabled":true,"tokens":10,"applyToConversations":true}));
    ledger(&f.hub.store, now_in_month(&f.hub.store), Some((0.1, "USD")), None);
    let routine = f
        .hub
        .routines
        .save(&f.hub, "bot", &json!({"name":"Rotina do teto","instruction":"Conclua","triggers":[{"type":"webhook"}]}))
        .unwrap();
    let id = routine["routine"]["id"].as_str().unwrap();
    let run = f.hub.routines.enqueue(&f.hub.store, "bot", id, json!({"source":"test"}), None, true).unwrap();
    let run_id = run["run"]["id"].as_str().unwrap().to_owned();
    f.hub.send_cmd("bot", crate::agent::bot::Cmd::Routine(run_id.clone())).unwrap();
    f.until("teto_duas_esperas_do_bot_sobrevivem_tres_boots_e_saem_uma_vez", || holds(&f.hub.store).len() == 1).await;
    crate::api::dispatch(&f.hub, &Caller::Local, "send", json!({"botId":"bot","text":"oi do dono"})).await.unwrap();
    f.until("teto_duas_esperas_do_bot_sobrevivem_tres_boots_e_saem_uma_vez", || holds(&f.hub.store).len() == 2).await;
    let works = |f: &Fixture| holds(&f.hub.store).into_iter().map(|h| h.work).collect::<Vec<_>>();
    let before = works(&f);
    assert_eq!(before[0], format!("routine:{run_id}"), "ordem de chegada: rotina primeiro");
    assert!(before[1].starts_with("human:"));
    for boot in 0..3 {
        f.hub.shutdown().await;
        f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
        f.until("teto_duas_esperas_do_bot_sobrevivem_tres_boots_e_saem_uma_vez", || {
            f.hub.runtime("bot").activity == "Parado pelo teto de gasto."
        })
        .await;
        // O agendador re-envia a rotina (Starting -> Pending no restore); a fila
        // restaurada vê a entrada queued: nenhum dos dois cria espera nova.
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(works(&f), before, "boot {boot}: as mesmas duas esperas, na mesma ordem");
        assert!(f.prompts().is_empty(), "boot {boot}: nada enviado enquanto trava");
    }
    // A fixture responde normalmente (sem simular limite da assinatura).
    std::fs::write(f.dir.join("blocked"), "").unwrap();
    let fp = fp(&hold(&f.hub.store).unwrap());
    release(&f.hub, "checkFailure", &fp).await.unwrap();
    f.until("teto_duas_esperas_do_bot_sobrevivem_tres_boots_e_saem_uma_vez", || f.prompts().len() >= 2).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let prompts: Vec<String> = f.prompts().iter().map(Value::to_string).collect();
    assert_eq!(prompts.len(), 2, "cada trabalho sai uma vez: {prompts:?}");
    // R3-P3-1: a ordem dos PROMPTS é a ordem em que travaram (rotina, depois a fala).
    assert!(prompts[0].contains("Scheduled routine"), "primeiro a rotina: {prompts:?}");
    assert!(prompts[1].contains("oi do dono"), "depois a fala do dono: {prompts:?}");
    assert!(holds(&f.hub.store).is_empty(), "esperas apagadas uma a uma na retomada");
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Rotina segurada pelo teto com `applyToConversations=false`: a marca durável
/// existe, mas não ocupa admissão nem fila — a fala do dono passa pelo caminho real.
#[tokio::test]
async fn teto_rotina_travada_nao_segura_fala_humana_isenta() {
    let f = Fixture::new();
    let hub = &f.hub;
    config(hub, &json!({"enabled":true,"tokens":10,"applyToConversations":false}));
    ledger(&hub.store, now_in_month(&hub.store), Some((0.1, "USD")), None);
    decide_with_body(hub, "routine", &json!({"routineId":"r5"}));
    assert!(hold(&hub.store).is_some(), "rotina segurada deixou a marca durável");
    crate::api::dispatch(hub, &Caller::Local, "send", json!({"botId":"bot","text":"oi"})).await.unwrap();
    f.until("teto_rotina_travada_nao_segura_fala_humana_isenta", || !f.prompts().is_empty()).await;
    assert_eq!(f.prompts().len(), 1, "fala do dono isenta chegou ao agente com rotina parada");
    assert!(hold(&hub.store).is_some(), "espera da rotina continua listada");
    assert!(
        teto::call(hub, &Caller::Local, "donnaCeiling", json!({})).await.unwrap()["stopped"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["botId"] == "bot"),
        "stops_on segue listando o bot"
    );
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Os três chamadores de fundo (`one_shot`, `one_shot_own`, `one_shot_observed`)
/// recusam com `ceiling_held` antes de vaga, conta ou processo — teto travado.
#[tokio::test]
async fn teto_callers_de_fundo_travam_sem_agente() {
    use crate::fork::background_executor::{self, MemoryBoundary};
    let f = Fixture::new();
    let hub = &f.hub;
    config(hub, &json!({"enabled":true,"tokens":10}));
    ledger(&hub.store, now_in_month(&hub.store), Some((0.1, "USD")), None);
    let cfg = hub.store.bot("bot").unwrap().unwrap().config;
    let boundary = MemoryBoundary {
        prepare: &|_: &crate::fork::background_selection::Resolved| -> anyhow::Result<String> { Ok(String::new()) },
        verify: &|_: &crate::fork::background_selection::Resolved| -> anyhow::Result<()> { Ok(()) },
    };
    let errs = [
        ("one_shot", background_executor::one_shot(hub, &cfg, "s", "u").await),
        ("one_shot_own", background_executor::one_shot_own(hub, &cfg, "s", "u").await),
        ("one_shot_observed", background_executor::one_shot_observed(hub, &cfg, "s", &mut false, boundary).await),
    ];
    for (name, result) in errs {
        let Err(err) = result else { panic!("{name} passou com o teto travado") };
        assert!(format!("{err:#}").contains("ceiling_held"), "{name}: {err:#}");
    }
    assert!(f.prompts().is_empty(), "nenhum prompt de fundo saiu");
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// E2E pelo `api::dispatch`/`send_admitted` de verdade: o prompt nunca chega ao
/// agente travado; a liberação do dono acorda o ator e o prompt sai.
#[tokio::test]
async fn teto_envio_real_trava_libera_e_envia() {
    let f = Fixture::new();
    let hub = &f.hub;
    config(hub, &json!({"enabled":true,"tokens":10,"applyToConversations":true}));
    ledger(&hub.store, now_in_month(&hub.store), Some((0.1, "USD")), None);
    crate::api::dispatch(hub, &Caller::Local, "send", json!({"botId": "bot", "text": "Continue a tarefa pendente"}))
        .await
        .unwrap();
    f.until("teto_envio_real_trava_libera_e_envia", || hold(&hub.store).is_some()).await;
    assert!(f.prompts().is_empty(), "nenhum prompt saiu enquanto o teto trava");
    // R3-P3-2: o preparo do ticket descartado não fica como fantasma — Desfazer
    // (`with_inactive_bot`) e a instalação do motor não veem o bot ocupado.
    f.until("teto_envio_real_trava_libera_e_envia", || hub.runtime("bot").activity == "Parado pelo teto de gasto.")
        .await;
    assert!(hub.runtime("bot").preparing_turn_token.is_none(), "preparo do ticket descartado limpo");
    hub.with_inactive_bot("bot", || Ok(())).expect("Desfazer não recusa o bot parado pelo teto");
    assert!(
        teto::call(hub, &Caller::Local, "donnaCeiling", json!({})).await.unwrap()["stopped"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["botId"] == "bot"),
        "stops_on lista o bot parado"
    );
    let fp = fp(&hold(&hub.store).unwrap());
    release(hub, "checkFailure", &fp).await.unwrap();
    f.until("teto_envio_real_trava_libera_e_envia", || !f.prompts().is_empty()).await;
    assert!(hold(&hub.store).is_none(), "espera liberada depois do envio");
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

fn decide_with_body(hub: &Hub, origin: &str, detail: &Value) {
    let db = hub.store.db.locked();
    let body = json!({"attempt":"a1","executionToken":"e1","origin":origin,"originDetail":detail,
        "lane":{"chat":"bot","session":null},"setup":{"entries":[],"text":"","hidden":false}});
    let o = if origin == "routine" {
        Origin::Routine { id: detail["routineId"].as_str().unwrap().into() }
    } else {
        Origin::Human
    };
    assert_eq!(teto::decide_on(&db, "bot", &o, &body).unwrap().verdict, teto::Verdict::Hold);
}

async fn release_with(hub: &Arc<Hub>, body: Value) -> anyhow::Result<Value> {
    teto::call(hub, &Caller::Local, "donnaCeilingRelease", body).await
}

/// (`kind`, `actor`, `limit_before`, `limit_after`, `overage_known`, `state`) de cada liberação gravada.
type ReleaseRow = (String, String, Option<f64>, Option<f64>, Option<f64>, Option<String>);

fn release_rows(store: &Store) -> Vec<ReleaseRow> {
    store
        .db
        .locked()
        .prepare(
            "SELECT kind,actor,limit_before,limit_after,overage_known,state FROM donna_ceiling_releases ORDER BY rowid",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

#[tokio::test]
async fn teto_plus25_libera_e_trava_de_novo_no_novo_limite() {
    let (hub, dir) = hub_tmp();
    let store = &hub.store;
    config(&hub, &json!({"enabled":true,"money":[{"currency":"USD","limit":10.0}]}));
    ledger(store, now_in_month(store), Some((10.5, "USD")), Some((10, 10)));
    assert_eq!(gate(store).blocked.unwrap().reason, "valueReached");
    let released = release(&hub, "plus25", "money:USD").await.unwrap();
    assert_eq!(released["release"]["metric"], "money:USD");
    assert_eq!(released["release"]["limitBefore"], 10.0);
    assert_eq!(released["release"]["limitAfter"], 12.5);
    assert_eq!(released["release"]["overageKnown"], 0.5);
    assert!(gate(store).blocked.is_none(), "10,5 < 12,5 efetivos");
    ledger(store, now_in_month(store), Some((2.0, "USD")), Some((10, 10)));
    assert_eq!(gate(store).blocked.unwrap().reason, "valueReached", "12,5 trava de novo no novo limite");
    drop(hub);
    let _ = std::fs::remove_dir_all(&dir);
}

/// MX-160 (decisão do dono): "Continuar mesmo assim" exige escolher +25%, +50% ou
/// um limite livre; atalho cujo novo limite não passa do gasto conhecido é
/// recusado; limite livre precisa ser finito, > gasto e > limite vigente; os
/// tipos antigos (plus20, untilMonthEnd) não existem mais; tudo fica registrado.
#[tokio::test]
async fn teto_continuar_exige_novo_limite_acima_do_gasto() {
    let (hub, dir) = hub_tmp();
    let store = &hub.store;
    config(&hub, &json!({"enabled":true,"money":[{"currency":"USD","limit":100.0}]}));
    ledger(store, now_in_month(store), Some((130.0, "USD")), Some((10, 10)));
    let state = teto::call(&hub, &Caller::Local, "donnaCeiling", json!({})).await.unwrap()["state"].clone();
    let shortcuts = &state["metrics"][0]["shortcuts"];
    assert_eq!(shortcuts["plus25"]["allowed"], false, "US$ 125 <= US$ 130 gastos: o app desliga");
    assert_eq!(shortcuts["plus50"], json!({"limitAfter":150.0,"allowed":true}));
    let refused = |error: anyhow::Error, code: &str| assert!(format!("{error:#}").contains(code), "{code}: {error:#}");
    refused(release(&hub, "plus25", "money:USD").await.unwrap_err(), "ceiling_release_below_spend");
    for kind in ["plus20", "untilMonthEnd"] {
        refused(release(&hub, kind, "money:USD").await.unwrap_err(), "invalid_ceiling_release");
    }
    let body = |kind: &str, limit: Value| json!({"kind":kind,"alvo":"money:USD","newLimit":limit});
    refused(
        release_with(&hub, json!({"kind":"setLimit","alvo":"money:USD"})).await.unwrap_err(),
        "invalid_ceiling_release",
    );
    refused(release_with(&hub, body("plus50", json!(200))).await.unwrap_err(), "invalid_ceiling_release");
    refused(release_with(&hub, body("setLimit", json!(130.0))).await.unwrap_err(), "ceiling_release_below_spend");
    refused(release_with(&hub, body("setLimit", json!(120))).await.unwrap_err(), "ceiling_release_below_spend");
    refused(release_with(&hub, body("setLimit", json!("200"))).await.unwrap_err(), "invalid_ceiling_release");
    assert!(release_rows(store).is_empty(), "nenhuma recusa grava liberação");
    let released = release(&hub, "plus50", "money:USD").await.unwrap();
    assert_eq!(released["release"]["limitAfter"], 150.0);
    assert!(gate(store).blocked.is_none(), "130 < 150");
    ledger(store, now_in_month(store), Some((25.0, "USD")), Some((10, 10)));
    assert_eq!(gate(store).blocked.unwrap().reason, "valueReached", "155 >= 150 pergunta de novo");
    refused(release_with(&hub, body("setLimit", json!(155.0))).await.unwrap_err(), "ceiling_release_below_spend");
    let released = release_with(&hub, body("setLimit", json!(180.5))).await.unwrap();
    assert_eq!(released["release"]["limitBefore"], 150.0);
    assert_eq!(released["release"]["limitAfter"], 180.5);
    assert_eq!(released["release"]["overageKnown"], 5.0);
    assert_eq!(gate(store).metrics[0].effective_limit, 180.5);
    assert!(gate(store).blocked.is_none());
    let rows = release_rows(store);
    assert_eq!(
        rows,
        [
            ("plus50".into(), "local".into(), Some(100.0), Some(150.0), Some(30.0), Some("150".into())),
            ("setLimit".into(), "local".into(), Some(150.0), Some(180.5), Some(5.0), Some("180.5".into())),
        ],
        "quem, de quanto para quanto e quanto passou"
    );
    // Não conferido: liberação de valor não dispensa (só checkFailure).
    ledger(store, now_in_month(store), None, Some((10, 10)));
    assert_eq!(gate(store).blocked.unwrap().reason, "unverifiable");
    refused(release_with(&hub, body("setLimit", json!(1000))).await.unwrap_err(), "ceiling_not_blocked");
    drop(hub);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Tokens: o limite livre é inteiro exato (além de 2^53) e o atalho arredonda para cima.
#[tokio::test]
async fn teto_set_limit_de_tokens_e_exato() {
    let (hub, dir) = hub_tmp();
    let store = &hub.store;
    config(&hub, &json!({"enabled":true,"tokens":9_007_199_254_740_992_u64}));
    ledger(store, now_in_month(store), Some((0.1, "USD")), Some((9_007_199_254_740_992, 0)));
    assert_eq!(gate(store).blocked.unwrap().reason, "valueReached");
    let body = |limit: Value| json!({"kind":"setLimit","alvo":"tokens","newLimit":limit});
    let error = release_with(&hub, body(json!(9.1e15))).await.unwrap_err();
    assert!(format!("{error:#}").contains("invalid_ceiling_release_limit"), "tokens só inteiro: {error:#}");
    release_with(&hub, body(json!(9_007_199_254_740_993_u64))).await.unwrap();
    assert_eq!(release_rows(store)[0].5.as_deref(), Some("9007199254740993"), "valor exato gravado");
    assert!(gate(store).blocked.is_none(), "2^53 < 2^53+1");
    ledger(store, now_in_month(store), Some((0.1, "USD")), Some((0, 1)));
    assert_eq!(gate(store).blocked.unwrap().reason, "valueReached", "2^53+1 atinge o limite livre exato");
    drop(hub);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn teto_liberacao_recusada_sem_bloqueio_e_para_nao_dono() {
    let (hub, dir) = hub_tmp();
    config(&hub, &json!({"enabled":true,"money":[{"currency":"USD","limit":10.0}]}));
    let owner = Caller::Local;
    let denied = Caller::Device { key: "k".into(), scopes: vec![], autoridade: None };
    let control = Caller::Device { key: "mac-1".into(), scopes: vec![Scope::Control], autoridade: None };
    for body in [
        json!({"kind":"plus25","alvo":"money:USD"}),
        json!({"kind":"plus50","alvo":"money:USD"}),
        json!({"kind":"setLimit","alvo":"money:USD","newLimit":50.0}),
    ] {
        assert!(teto::call(&hub, &owner, "donnaCeilingRelease", body.clone()).await.is_err(), "{body} sem bloqueio");
    }
    assert!(teto::call(&hub, &denied, "donnaCeiling", json!({})).await.is_err());
    assert!(teto::call(&hub, &denied, "donnaCeilingSave", json!({"enabled":true})).await.is_err());
    assert!(
        teto::call(&hub, &denied, "donnaCeilingRelease", json!({"kind":"plus25","alvo":"money:USD"})).await.is_err()
    );
    assert!(teto::call(&hub, &control, "donnaCeiling", json!({})).await.is_ok());
    drop(hub);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn teto_tabela_de_liberacoes_recusa_update_e_delete() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let db = store.db.locked();
    db.execute(
        "INSERT INTO donna_ceiling_releases(id,at,actor,kind,month,metric,scope) VALUES('r1',1,'local','plus25','2026-11','money:USD','autonomous')",
        [],
    )
    .unwrap();
    assert!(db.execute("UPDATE donna_ceiling_releases SET actor='x' WHERE id='r1'", []).is_err());
    assert!(db.execute("DELETE FROM donna_ceiling_releases WHERE id='r1'", []).is_err());
    let count: i64 = db.query_row("SELECT COUNT(*) FROM donna_ceiling_releases", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn teto_save_valida_e_estado_responde() {
    let (hub, dir) = hub_tmp();
    for bad in [
        json!({"enabled":true,"money":[{"currency":"usd","limit":1.0}]}),
        json!({"enabled":true,"money":[{"currency":"USD","limit":0}]}),
        json!({"enabled":true,"tokens":0}),
        json!({"enabled":true,"warnPercent":75}),
        json!({"enabled":true,"money":[{"currency":"USD","limit":1},{"currency":"USD","limit":2}]}),
        json!({"enabled":true,"surpresa":1}),
    ] {
        assert!(teto::call(&hub, &Caller::Local, "donnaCeilingSave", bad).await.is_err());
    }
    let saved = teto::call(
        &hub,
        &Caller::Local,
        "donnaCeilingSave",
        json!({"enabled":true,"money":[{"currency":"USD","limit":10.0}],"tokens":1000,"warnPercent":90,"applyToConversations":true}),
    )
    .await
    .unwrap();
    assert_eq!(saved["warnPercent"], 90);
    let state = teto::call(&hub, &Caller::Local, "donnaCeiling", json!({})).await.unwrap();
    assert_eq!(state["config"]["enabled"], true);
    assert_eq!(state["state"]["blocked"], Value::Null);
    assert_eq!(state["stopped"], json!([]));
    drop(hub);
    let _ = std::fs::remove_dir_all(&dir);
}

fn config_for(store: &Store, settings: &Value) {
    store.kv_set(KEY, &settings.to_string()).unwrap();
}

fn ledger_row(store: &Store, id: &str, tokens: Option<(i64, i64)>) {
    store
        .db
        .locked()
        .execute(
            "INSERT INTO donna_turn_usage(id,bot_id,chat_id,session_id,at,input_tokens,output_tokens,amount,currency,source)
             VALUES(?1,'b','b','s',?2,?3,?4,0.1,'USD','motor')",
            params![id, now_in_month(store), tokens.map(|t| t.0), tokens.map(|t| t.1)],
        )
        .unwrap();
}

fn set_row_tokens(store: &Store, id: &str, tokens: Option<(i64, i64)>) {
    store
        .db
        .locked()
        .execute(
            "UPDATE donna_turn_usage SET input_tokens=?2, output_tokens=?3 WHERE id=?1",
            params![id, tokens.map(|t| t.0), tokens.map(|t| t.1)],
        )
        .unwrap();
}

async fn save(hub: &Arc<Hub>, settings: Value) -> anyhow::Result<Value> {
    teto::call(hub, &Caller::Local, "donnaCeilingSave", settings).await
}

/// P2-1: falha F -> checkFailure(F) -> leitura saudável vista SÓ pela fronteira de
/// fundo -> F volta: a geração andou e a liberação velha não reabre.
#[tokio::test]
async fn teto_check_failure_morre_com_saida_saudavel_vista_so_pelo_fundo() {
    let (hub, dir) = hub_tmp();
    let store = &hub.store;
    config(&hub, &json!({"enabled":true,"tokens":100}));
    ledger_row(store, "linha", None);
    assert_eq!(decide(store, &Origin::Routine { id: "r".into() }), teto::Verdict::Hold);
    let fp = fp(&hold(store).unwrap());
    release(&hub, "checkFailure", &fp).await.unwrap();
    assert_eq!(decide(store, &Origin::Routine { id: "r".into() }), teto::Verdict::Pass);
    set_row_tokens(store, "linha", Some((1, 1)));
    assert!(teto::background_gate(&hub, "bot", "titles").is_ok(), "só o fundo viu o estado saudável");
    set_row_tokens(store, "linha", None);
    assert_eq!(
        decide(store, &Origin::Routine { id: "r".into() }),
        teto::Verdict::Hold,
        "a mesma impressão de volta é geração nova: checkFailure velha morta"
    );
    drop(hub);
    let _ = std::fs::remove_dir_all(&dir);
}

/// P2-1: desligar e religar é transição — a checkFailure anterior não volta a valer.
#[tokio::test]
async fn teto_desligar_e_religar_invalida_check_failure() {
    let (hub, dir) = hub_tmp();
    let store = &hub.store;
    save(&hub, json!({"enabled":true,"tokens":100})).await.unwrap();
    ledger_row(store, "linha", None);
    assert_eq!(decide(store, &Origin::Routine { id: "r".into() }), teto::Verdict::Hold);
    let fp = fp(&hold(store).unwrap());
    release(&hub, "checkFailure", &fp).await.unwrap();
    assert_eq!(decide(store, &Origin::Routine { id: "r".into() }), teto::Verdict::Pass);
    save(&hub, json!({"enabled":false,"tokens":100})).await.unwrap();
    save(&hub, json!({"enabled":true,"tokens":100})).await.unwrap();
    assert_eq!(decide(store, &Origin::Routine { id: "r".into() }), teto::Verdict::Hold, "religar exige decisão nova");
    drop(hub);
    let _ = std::fs::remove_dir_all(&dir);
}

/// P2-3: "Parar" na principal cancela a rotina estacionada (que está em Starting,
/// fora da fila): a execução vira cancelada, a vaga do bot volta e a liberação
/// posterior não traz nada de volta.
#[tokio::test]
async fn teto_parar_cancela_rotina_estacionada() {
    let f = Fixture::new();
    let hub = &f.hub;
    config(hub, &json!({"enabled":true,"tokens":10}));
    ledger(&hub.store, now_in_month(&hub.store), Some((0.1, "USD")), None);
    let routine = hub
        .routines
        .save(hub, "bot", &json!({"name":"Rotina parada","instruction":"Conclua","triggers":[{"type":"webhook"}]}))
        .unwrap();
    let id = routine["routine"]["id"].as_str().unwrap();
    let run = hub.routines.enqueue(&hub.store, "bot", id, json!({"source":"test"}), None, true).unwrap();
    let run_id = run["run"]["id"].as_str().unwrap().to_owned();
    hub.send_cmd("bot", crate::agent::bot::Cmd::Routine(run_id.clone())).unwrap();
    f.until("teto_parar_cancela_rotina_estacionada", || !holds(&hub.store).is_empty()).await;
    assert_eq!(hub.routines.list("bot")["runs"][0]["status"], "starting", "estacionada fora da fila");
    crate::api::dispatch(hub, &Caller::Local, "stop", json!({"botId":"bot"})).await.unwrap();
    f.until("teto_parar_cancela_rotina_estacionada", || holds(&hub.store).is_empty()).await;
    assert_eq!(hub.routines.list("bot")["runs"][0]["status"], "cancelled");
    assert!(!hub.routines.chain_reserved_for_test("bot"), "a vaga de rotina do bot voltou");
    let fp = teto::call(hub, &Caller::Local, "donnaCeiling", json!({})).await.unwrap()["state"]["blocked"]["signature"]
        .as_str()
        .unwrap()
        .to_owned();
    release(hub, "checkFailure", &fp).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(f.prompts().is_empty(), "rotina cancelada não volta depois da liberação");
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// P2-3: recomeçar a principal tira as esperas dela (rotina e captura da principal).
#[tokio::test]
async fn teto_recomeco_da_principal_tira_esperas_da_principal() {
    let f = Fixture::new();
    let hub = &f.hub;
    config(hub, &json!({"enabled":true,"tokens":10,"applyToConversations":true}));
    ledger(&hub.store, now_in_month(&hub.store), Some((0.1, "USD")), None);
    decide_with_body(hub, "routine", &json!({"routineId":"r7"}));
    let body = json!({"attempt":"a2","origin":"human","originDetail":{},"lane":{"chat":"bot","session":null},
        "setup":{"entries":["e-principal"],"text":"oi","hidden":false}});
    assert_eq!(
        teto::decide_on(&hub.store.db.locked(), "bot", &Origin::Human, &body).unwrap().verdict,
        teto::Verdict::Hold
    );
    let mut actor = f.actor();
    actor.restore_ceiling_hold().unwrap();
    assert_eq!(actor.ceiling_holds.len(), 2);
    let space = crate::fork::spaces::chat_space(hub, "bot").unwrap();
    let revision = hub.store.space_conversation(&space, "bot").unwrap().revision;
    actor.primary_new_session("bot", revision, &crate::fork::autoridade::Autoridade::local()).await.unwrap();
    assert!(actor.ceiling_holds.is_empty(), "nada da principal fica parado");
    assert!(holds(&hub.store).is_empty(), "linhas duráveis apagadas junto");
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// P2-4: desmarcar `applyToConversations` solta a fala do dono JÁ parada — pelo
/// caminho real — e a rotina, autônoma, continua parada.
#[tokio::test]
async fn teto_desmarcar_conversas_solta_fala_parada_e_mantem_rotina() {
    let f = Fixture::new();
    let hub = &f.hub;
    config(hub, &json!({"enabled":true,"tokens":10,"applyToConversations":true}));
    ledger(&hub.store, now_in_month(&hub.store), Some((0.1, "USD")), None);
    let routine = hub
        .routines
        .save(hub, "bot", &json!({"name":"Rotina","instruction":"Conclua","triggers":[{"type":"webhook"}]}))
        .unwrap();
    let id = routine["routine"]["id"].as_str().unwrap();
    let run = hub.routines.enqueue(&hub.store, "bot", id, json!({"source":"test"}), None, true).unwrap();
    hub.send_cmd("bot", crate::agent::bot::Cmd::Routine(run["run"]["id"].as_str().unwrap().into())).unwrap();
    f.until("teto_desmarcar_conversas_solta_fala_parada_e_mantem_rotina", || holds(&hub.store).len() == 1).await;
    crate::api::dispatch(hub, &Caller::Local, "send", json!({"botId":"bot","text":"fala parada"})).await.unwrap();
    f.until("teto_desmarcar_conversas_solta_fala_parada_e_mantem_rotina", || holds(&hub.store).len() == 2).await;
    save(hub, json!({"enabled":true,"tokens":10,"applyToConversations":false})).await.unwrap();
    f.until("teto_desmarcar_conversas_solta_fala_parada_e_mantem_rotina", || !f.prompts().is_empty()).await;
    assert!(f.prompts()[0].to_string().contains("fala parada"), "a fala do dono saiu");
    let left: Vec<String> = holds(&hub.store).into_iter().map(|h| h.work).collect();
    assert_eq!(left.len(), 1);
    assert!(left[0].starts_with("routine:"), "a rotina continua parada: {left:?}");
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// P2-5: o consumo de fundo entra no ledger (uma linha por envio), conta no teto
/// e aparece no relatório com o mesmo formato; sem preço/tokens é Unverified.
#[tokio::test]
async fn teto_trabalho_de_fundo_entra_no_ledger_e_no_relatorio() {
    let (hub, dir) = hub_tmp();
    let store = &hub.store;
    config(&hub, &json!({"enabled":true,"tokens":100}));
    crate::fork::custo::record_background(
        store,
        "bot",
        "titles",
        Some("m"),
        None,
        Some(crate::fork::custo::UsoFundo {
            input: 60,
            output: 50,
            read: Some(7),
            write: Some(0),
            invalid: false,
            ..Default::default()
        }),
    )
    .unwrap();
    let g = gate(store);
    assert_eq!(g.blocked.unwrap().reason, "valueReached", "110 tokens de fundo atingem o teto de 100");
    let report = crate::fork::custo::call(&hub, &Caller::Local, json!({})).await.unwrap();
    assert_eq!(report["total"]["turns"], 1);
    assert_eq!(report["total"]["unpricedTurns"], 1);
    let conversation = &report["conversations"][0];
    assert_eq!(conversation["botId"], "bot");
    assert_eq!(conversation["chatId"], "bot");
    assert_eq!(conversation["sessionId"], "background:titles");
    assert_eq!(conversation["title"], "Trabalho de fundo (titles)");
    config(&hub, &json!({"enabled":true,"tokens":1000,"money":[{"currency":"USD","limit":10.0}]}));
    let g = gate(store);
    let blocked = g.blocked.unwrap();
    assert_eq!(blocked.metrics, ["money:USD"], "fundo sem preço: dinheiro não conferido");
    crate::fork::custo::record_background(store, "bot", "memory", None, None, None).unwrap();
    let g = gate(store);
    assert_eq!(g.blocked.unwrap().metrics, ["money:USD", "tokens"], "fundo sem uso: tokens não conferidos");
    drop(hub);
    let _ = std::fs::remove_dir_all(&dir);
}

/// P3-6: o aviso também é marcado na passagem (sem trava) e só uma vez.
#[test]
fn teto_aviso_na_passagem_marca_uma_vez() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    config_for(&store, &json!({"enabled":true,"tokens":100,"warnPercent":80}));
    ledger(&store, now_in_month(&store), Some((0.1, "USD")), Some((50, 35)));
    let body = json!({"attempt":"a","origin":"routine","originDetail":{"routineId":"r"}});
    let origin = Origin::Routine { id: "r".into() };
    let first = teto::decide_on(&store.db.locked(), "bot", &origin, &body).unwrap();
    assert_eq!(first, teto::Decision { verdict: teto::Verdict::Pass, changed: true }, "aviso novo na passagem");
    let again = teto::decide_on(&store.db.locked(), "bot", &origin, &body).unwrap();
    assert_eq!(again, teto::Decision { verdict: teto::Verdict::Pass, changed: false }, "o mesmo aviso não repete");
}

/// P3-7: painel, gate e liberação usam o MESMO estado efetivo da falha de leitura;
/// liberar de novo um estado já liberado é recusado.
#[tokio::test]
async fn teto_painel_e_liberacao_usam_o_mesmo_estado_efetivo() {
    let (hub, dir) = hub_tmp();
    let store = &hub.store;
    config(&hub, &json!({"enabled":true,"money":[{"currency":"USD","limit":10.0}]}));
    store.db.locked().execute_batch("ALTER TABLE donna_turn_usage RENAME TO broken_turn_usage").unwrap();
    let state = teto::call(&hub, &Caller::Local, "donnaCeiling", json!({})).await.unwrap()["state"].clone();
    let fp = state["blocked"]["signature"].as_str().unwrap().to_owned();
    assert!(fp.starts_with("readFailure:"));
    let fingerprint = state["blocked"]["fingerprint"].as_str().unwrap();
    assert!(fp.starts_with(&format!("{fingerprint}#")), "a falha tem geração, não é fabricada: {fp}");
    release(&hub, "checkFailure", &fp).await.unwrap();
    let state = teto::call(&hub, &Caller::Local, "donnaCeiling", json!({})).await.unwrap()["state"].clone();
    assert!(state["blocked"].is_null(), "o painel mostra o que o gate faz");
    assert_eq!(decide(store, &Origin::Routine { id: "r".into() }), teto::Verdict::Pass);
    let error = release(&hub, "checkFailure", &fp).await.unwrap_err();
    assert!(format!("{error:#}").contains("ceiling_not_blocked"), "{error:#}");
    store.db.locked().execute_batch("ALTER TABLE broken_turn_usage RENAME TO donna_turn_usage").unwrap();
    drop(hub);
    let _ = std::fs::remove_dir_all(&dir);
}

/// O limite liberado vale no mês: baixar o teto configurado não desfaz a decisão
/// do dono, e um teto configurado maior prevalece (o efetivo é o maior dos dois).
#[tokio::test]
async fn teto_limite_liberado_vale_no_mes_e_teto_maior_prevalece() {
    let (hub, dir) = hub_tmp();
    let store = &hub.store;
    save(&hub, json!({"enabled":true,"money":[{"currency":"USD","limit":10.0}]})).await.unwrap();
    ledger(store, now_in_month(store), Some((10.0, "USD")), Some((1, 1)));
    release(&hub, "plus50", "money:USD").await.unwrap();
    save(&hub, json!({"enabled":true,"money":[{"currency":"USD","limit":5.0}]})).await.unwrap();
    assert_eq!(gate(store).metrics[0].effective_limit, 15.0, "a liberação do mês continua valendo");
    save(&hub, json!({"enabled":true,"money":[{"currency":"USD","limit":40.0}]})).await.unwrap();
    assert_eq!(gate(store).metrics[0].effective_limit, 40.0, "teto maior prevalece");
    drop(hub);
    let _ = std::fs::remove_dir_all(&dir);
}

/// P3-9: a sonda é O(1) (contador por gatilho) e acompanha QUALQUER escrita do
/// ledger, inclusive UPDATE em linha antiga; a agregação usa o índice por mês.
#[test]
fn teto_sonda_o1_acompanha_toda_escrita_do_ledger() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    config_for(&store, &json!({"enabled":true,"tokens":100}));
    let probe = || teto::probe_on(&store.db.locked()).unwrap();
    let p0 = probe();
    assert_eq!(probe(), p0, "sem escrita, sonda idêntica");
    let (_, start, _) = crate::fork::custo::month(None).unwrap();
    ledger(&store, start - 40 * 86_400_000, None, None);
    let p1 = probe();
    assert_ne!(p1, p0, "INSERT (mesmo fora do mês) anda o contador");
    store.db.locked().execute("UPDATE donna_turn_usage SET amount=1.0", []).unwrap();
    assert_ne!(probe(), p1, "UPDATE em linha antiga também anda");
    let plan: String = store
        .db
        .locked()
        .query_row(
            "EXPLAIN QUERY PLAN SELECT COUNT(*) FROM donna_turn_usage WHERE at>=1 AND at<2 AND input_tokens IS NULL",
            [],
            |r| r.get(3),
        )
        .unwrap();
    assert!(plan.contains("donna_turn_usage_month"), "agregação mensal usa o índice: {plan}");
}

/// P3-12: pedido parado que perdeu o canal no reinício vira aviso durável no chat
/// ANTES de a espera sair; a repetição não duplica o aviso.
#[tokio::test]
async fn teto_pedido_sem_canal_apos_restart_vira_aviso_duravel() {
    let f = Fixture::new();
    let hub = &f.hub;
    config(hub, &json!({"enabled":true,"tokens":10}));
    ledger(&hub.store, now_in_month(&hub.store), Some((0.1, "USD")), None);
    let body = json!({"attempt":"a","origin":"ask","originDetail":{"requestId":"q1"},
        "lane":{"chat":"bot","session":null},"setup":{"entries":[],"text":"","hidden":false}});
    assert_eq!(
        teto::decide_on(&hub.store.db.locked(), "bot", &Origin::Ask, &body).unwrap().verdict,
        teto::Verdict::Hold
    );
    let mut actor = f.actor();
    actor.restore_ceiling_hold().unwrap();
    let fp = fp(&hold(&hub.store).unwrap());
    release(hub, "checkFailure", &fp).await.unwrap();
    let (done_tx, _done_rx) = tokio::sync::mpsc::unbounded_channel();
    actor.ceiling_next_check = None;
    actor.retry_ceiling_holds(&done_tx).await;
    assert!(holds(&hub.store).is_empty() && actor.ceiling_holds.is_empty());
    let notices = |hub: &Hub| {
        hub.store
            .history("bot", i64::MAX, 100)
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == "notice" && e.data["text"].as_str().is_some_and(|t| t.contains("não foi retomado")))
            .count()
    };
    assert_eq!(notices(hub), 1, "aviso durável no chat");
    actor.restore_ceiling_hold().unwrap();
    assert_eq!(notices(hub), 1, "idempotente");
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// P3-13: o padrão devolvido pelo primeiro `donnaCeiling` passa no próprio save.
#[tokio::test]
async fn teto_padrao_do_painel_passa_no_proprio_save() {
    let (hub, dir) = hub_tmp();
    let state = teto::call(&hub, &Caller::Local, "donnaCeiling", json!({})).await.unwrap();
    assert_eq!(state["config"]["warnPercent"], 80);
    save(&hub, state["config"].clone()).await.unwrap();
    drop(hub);
    let _ = std::fs::remove_dir_all(&dir);
}

/// P3-14: pedido entre bots parado pelo teto continua `accepted` (nunca
/// "possivelmente enviado"); o reinício o fecha como `restart_before_start`. Quando
/// o teto passa, `running` é gravado na MESMA Tx do permit.
#[tokio::test]
async fn teto_pedido_entre_bots_parado_nao_vira_running() {
    let f = Fixture::new();
    let hub = &f.hub;
    config(hub, &json!({"enabled":true,"tokens":10}));
    ledger(&hub.store, now_in_month(&hub.store), Some((0.1, "USD")), None);
    let insert = |id: &str| {
        hub.store
            .db
            .locked()
            .execute(
                "INSERT INTO donna_session_requests(id,schema_version,kind,mode,requester_bot_id,requester_chat_id,
                 requester_session_id,requester_execution_ref,target_bot_id,target_chat_id,target_session_id,chain_id,
                 admission_id,origin_entry_id,target_entry_id,attempt,fence,state,created_at,updated_at)
                 VALUES(?1,1,'inter_bot','wait','outro','outro','outro','ref','bot','bot','bot','c','a','o','t',1,1,'accepted',1,1)",
                [id],
            )
            .unwrap();
    };
    let state = |id: &str| -> (String, Option<String>) {
        hub.store
            .db
            .locked()
            .query_row("SELECT state,end_reason FROM donna_session_requests WHERE id=?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap()
    };
    insert("q-parado");
    let (done_tx, _done_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut actor = f.actor();
    let (ask, _answer) = crate::chat::team::BotRequest::for_test_recorded("q-parado");
    actor.active_request = Some(ask);
    let ticket = bot_admission::reserve(&hub.store, &actor.cfg, "bot", "exec", &Origin::Ask).unwrap();
    actor.dispatch = Some(ticket);
    actor.send_admitted(json!({"prompt":[]}), Vec::new(), "s".into(), &done_tx).await.unwrap();
    assert_eq!(holds(&hub.store)[0].work, "request:q-parado");
    assert_eq!(state("q-parado").0, "accepted", "parado pelo teto não é running");
    crate::fork::retornos::recuperar(&hub.store).unwrap();
    assert_eq!(state("q-parado"), ("interrupted".into(), Some("restart_before_start".into())));
    // Teto aberto: a transição acompanha a autorização, na mesma Tx do permit.
    config(hub, &json!({"enabled":false}));
    insert("q-passa");
    let (ask, _answer) = crate::chat::team::BotRequest::for_test_recorded("q-passa");
    actor.active_request = Some(ask);
    let ticket = bot_admission::reserve(&hub.store, &actor.cfg, "bot", "exec2", &Origin::Ask).unwrap();
    actor.dispatch = Some(ticket);
    // Sem processo ACP o envio falha DEPOIS do permit: o registro já é `running`.
    let _ = actor.send_admitted(json!({"prompt":[]}), Vec::new(), "s".into(), &done_tx).await;
    assert_eq!(state("q-passa").0, "running");
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// R3-P2-1: banco com esperas do formato anterior (kv `donna.ceiling.hold.*`).
/// A migração converte cada uma em linha com `work` (identidade da origem), na
/// ordem em que travaram, com o corpo inteiro como evidência, e só então tira a
/// origem do kv; o que não converte (ilegível, sem origem, `stop.*`) fica no kv.
#[test]
fn teto_migracao_converte_esperas_antigas_e_conserva_o_resto() {
    let dir = std::env::temp_dir().join(format!("teto-mig-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.db");
    let routine = json!({"kind":"turn","attempt":"a-r","executionToken":"t-r","origin":"routine",
        "originDetail":{"routineId":"run-1"},"lane":{"chat":"bot","session":null},
        "setup":{"entries":[],"text":"","hidden":false},"reason":"unverifiable","metrics":["tokens"],
        "signature":"fp#3","at":200});
    let human = json!({"kind":"turn","attempt":"a-h","executionToken":"t-h","origin":"human","originDetail":{},
        "lane":{"chat":"outro","session":null},"setup":{"entries":["e1","e2"],"text":"um\n\ndois","hidden":false},
        "reason":"valueReached","metrics":["tokens"],"signature":"fp#2","at":100});
    let background = json!({"kind":"background","job":"titles","reason":"unverifiable","metrics":["tokens"],
        "signature":"fp#3","at":300});
    {
        let store = Store::open(&path).unwrap();
        store.kv_set("donna.ceiling.hold.bot", &routine.to_string()).unwrap();
        store.kv_set("donna.ceiling.hold.outro", &human.to_string()).unwrap();
        store.kv_set("donna.ceiling.hold.bg.bot", &background.to_string()).unwrap();
        store.kv_set("donna.ceiling.hold.ruim", "{nao-json").unwrap();
        store.kv_set("donna.ceiling.hold.semorigem", r#"{"attempt":"x","at":50}"#).unwrap();
        store.kv_set("donna.ceiling.stop.bot", r#"{"attempt":"a0","origin":"routine"}"#).unwrap();
    }
    // Boot real: `Store::open` roda a migração sobre o banco antigo.
    let store = Store::open(&path).unwrap();
    let all = teto::holds_on(&store.db.locked()).unwrap();
    let works: Vec<(&str, &str, &str)> = all
        .iter()
        .map(|h| (h["botId"].as_str().unwrap(), h["work"].as_str().unwrap(), h["kind"].as_str().unwrap()))
        .collect();
    assert_eq!(
        works,
        [("outro", "human:e1,e2", "turn"), ("bot", "routine:run-1", "turn"), ("bot", "bg:titles", "background")],
        "identidade por origem e ordem de chegada (at)"
    );
    // Evidência: o corpo original segue inteiro, com a origem do kv anotada.
    let migrated = &teto::turn_holds_of(&store.db.locked(), "bot").unwrap()[0].body;
    for field in ["attempt", "executionToken", "originDetail", "reason", "metrics", "signature", "at"] {
        assert_eq!(migrated[field], routine[field], "{field} preservado");
    }
    assert_eq!(migrated["migratedFrom"], "donna.ceiling.hold.bot");
    for gone in ["donna.ceiling.hold.bot", "donna.ceiling.hold.outro", "donna.ceiling.hold.bg.bot"] {
        assert!(store.kv_read(gone).unwrap().is_none(), "{gone}: origem sai depois da linha");
    }
    for kept in ["donna.ceiling.hold.ruim", "donna.ceiling.hold.semorigem", "donna.ceiling.stop.bot"] {
        assert!(store.kv_read(kept).unwrap().is_some(), "{kept}: não convertida, conservada");
    }
    drop(store);
    // Idempotente: outro boot não duplica nem perde nada.
    let store = Store::open(&path).unwrap();
    assert_eq!(teto::holds_on(&store.db.locked()).unwrap().len(), 3);
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

fn user_entry(hub: &Hub, text: &str) -> String {
    hub.store
        .history("bot", i64::MAX, 200)
        .unwrap()
        .into_iter()
        .find(|e| e.kind == "user" && e.data["text"] == text)
        .unwrap()
        .id
}

async fn stop_entry(hub: &Arc<Hub>, entry: &str) {
    let space = crate::fork::spaces::chat_space(hub, "bot").unwrap();
    let revision = hub.store.space_conversation(&space, "bot").unwrap().revision;
    let (reply, answer) = tokio::sync::oneshot::channel();
    hub.send_cmd(
        "bot",
        crate::agent::bot::Cmd::ContextControl {
            chat: "bot".into(),
            revision,
            entry_id: Some(entry.into()),
            reset: false,
            reply,
        },
    )
    .unwrap();
    answer.await.unwrap().unwrap();
}

/// R3-P2-2: lote [e1,e2] parado pelo teto pelo caminho real (fila → setup →
/// permit). Parar só e1 reescreve a captura com e2 e o texto dela, no mesmo
/// lugar; liberar envia e2 exatamente uma vez e nunca e1.
#[tokio::test]
async fn teto_parar_uma_mensagem_do_lote_conserva_as_demais() {
    let f = Fixture::new();
    let hub = &f.hub;
    std::fs::write(f.dir.join("blocked"), "").unwrap();
    std::fs::write(f.dir.join("hold"), "").unwrap();
    let send =
        |text: &'static str| crate::api::dispatch(hub, &Caller::Local, "send", json!({"botId":"bot","text":text}));
    send("turno em andamento").await.unwrap();
    f.until("teto_parar_uma_mensagem_do_lote_conserva_as_demais", || f.prompts().len() == 1).await;
    send("LOTE-UM").await.unwrap();
    send("LOTE-DOIS").await.unwrap();
    let (e1, e2) = (user_entry(hub, "LOTE-UM"), user_entry(hub, "LOTE-DOIS"));
    config(hub, &json!({"enabled":true,"tokens":10,"applyToConversations":true}));
    ledger(&hub.store, now_in_month(&hub.store), Some((0.1, "USD")), None);
    std::fs::remove_file(f.dir.join("hold")).unwrap();
    f.until("teto_parar_uma_mensagem_do_lote_conserva_as_demais", || holds(&hub.store).len() == 1).await;
    assert_eq!(holds(&hub.store)[0].work, format!("human:{e1},{e2}"), "o lote travou junto");
    stop_entry(hub, &e1).await;
    let rows = holds(&hub.store);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].work, format!("human:{e2}"), "a captura segue só com a sobrevivente");
    let text = rows[0].body["setup"]["text"].as_str().unwrap();
    assert!(text.contains("LOTE-DOIS") && !text.contains("LOTE-UM"), "texto da sobrevivente: {text}");
    assert_eq!(hub.store.entry(&e1).unwrap().data["status"], "cancelled");
    assert_eq!(hub.store.entry(&e2).unwrap().data["status"], "queued");
    let fp = fp(&hold(&hub.store).unwrap());
    release(hub, "checkFailure", &fp).await.unwrap();
    f.until("teto_parar_uma_mensagem_do_lote_conserva_as_demais", || f.prompts().len() >= 2).await;
    tokio::time::sleep(Duration::from_millis(600)).await;
    let prompts: Vec<String> = f.prompts().iter().map(Value::to_string).collect();
    assert_eq!(prompts.len(), 2, "{prompts:?}");
    assert_eq!(prompts.iter().filter(|p| p.contains("LOTE-DOIS")).count(), 1, "e2 sai exatamente uma vez");
    assert!(!prompts.iter().any(|p| p.contains("LOTE-UM")), "e1 parada nunca sai");
    assert!(holds(&hub.store).is_empty());
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// R3-P2-3: retomada D72 (assinatura) barrada pelo teto → reinício do Hub →
/// liberação ANTES do retryAt: a espera D72 retoma o mesmo pedido (um único
/// consumidor), sem ficar presa em `Pending::Prompt`.
#[tokio::test]
async fn teto_retomada_d72_barrada_retoma_apos_restart() {
    let mut f = Fixture::new();
    f.send().await;
    assert_eq!(f.prompts().len(), 1, "primeiro envio esbarrou no limite da assinatura");
    f.hub.shutdown().await;
    {
        let store = Store::open(&f.dir.join("test.db")).unwrap();
        store.kv_set(KEY, &json!({"enabled":true,"tokens":10,"applyToConversations":true}).to_string()).unwrap();
        ledger(&store, now_in_month(&store), Some((0.1, "USD")), None);
    }
    f.make_retry_due();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until("teto_retomada_d72_barrada_retoma_apos_restart", || holds(&f.hub.store).len() == 1).await;
    assert_eq!(hold(&f.hub.store).unwrap()["waiting"], true, "a espera sabe que é a retomada D72");
    let checkpoint =
        |hub: &Hub| -> Value { serde_json::from_str(&hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap() };
    f.until("teto_retomada_d72_barrada_retoma_apos_restart", || checkpoint(&f.hub)["phase"] == "waiting").await;
    assert_eq!(f.prompts().len(), 1, "a retomada não saiu com o teto travado");
    f.hub.shutdown().await;
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(holds(&f.hub.store).len(), 1, "a espera sobrevive ao reinício");
    assert_eq!(f.prompts().len(), 1);
    assert!(checkpoint(&f.hub)["retryAt"].as_i64().unwrap() > crate::store::now_ms(), "liberação antes do retryAt");
    let fp = fp(&hold(&f.hub.store).unwrap());
    release(&f.hub, "checkFailure", &fp).await.unwrap();
    f.until("teto_retomada_d72_barrada_retoma_apos_restart", || f.prompts().len() == 2).await;
    f.until("teto_retomada_d72_barrada_retoma_apos_restart", || f.hub.runtime("bot").status == BotStatus::Idle).await;
    assert!(holds(&f.hub.store).is_empty());
    assert!(f.hub.store.kv_read("turn.waiting.bot").unwrap().is_none_or(|raw| raw.contains("\"finished\"")));
    f.assert_finished_once();
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

fn sql(hub: &Hub, batch: &str) {
    hub.store.db.locked().execute_batch(batch).unwrap();
}

const INJ_ROTINA: &str = "CREATE TRIGGER teto_inj_rotina BEFORE UPDATE ON kv WHEN NEW.k='routines.v1'
     AND instr(NEW.v,'\"status\":\"cancelled\"')>0 BEGIN SELECT RAISE(ABORT,'injected'); END;";
const INJ_LINHA: &str =
    "CREATE TRIGGER teto_inj_linha BEFORE DELETE ON donna_ceiling_holds BEGIN SELECT RAISE(ABORT,'injected'); END;";

/// Rotina real parada pelo teto (Starting, fora da fila); devolve o id da execução.
async fn held_routine(f: &Fixture, name: &str) -> String {
    let hub = &f.hub;
    let routine = hub
        .routines
        .save(hub, "bot", &json!({"name":name,"instruction":"Conclua","triggers":[{"type":"webhook"}]}))
        .unwrap();
    let id = routine["routine"]["id"].as_str().unwrap();
    let run = hub.routines.enqueue(&hub.store, "bot", id, json!({"source":"test"}), None, true).unwrap();
    let run_id = run["run"]["id"].as_str().unwrap().to_owned();
    let before = holds(&hub.store).len();
    hub.send_cmd("bot", crate::agent::bot::Cmd::Routine(run_id.clone())).unwrap();
    f.until("held_routine", || holds(&hub.store).len() == before + 1).await;
    run_id
}

fn run_status(hub: &Hub) -> Value {
    hub.routines.list("bot")["runs"][0]["status"].clone()
}

/// A intenção de cancelar do dono está gravada (tabela própria, só INSERT).
fn cancel_recorded(hub: &Hub) -> bool {
    hub.store
        .db
        .locked()
        .query_row("SELECT EXISTS(SELECT 1 FROM donna_ceiling_cancels WHERE bot_id='bot')", [], |r| r.get(0))
        .unwrap()
}

async fn blocked_fp(hub: &Arc<Hub>) -> String {
    teto::call(hub, &Caller::Local, "donnaCeiling", json!({})).await.unwrap()["state"]["blocked"]["signature"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Revisão final P2 #1 + P3 #3: Parar com a rotina parada pelo teto. O cancelamento
/// falha: a INTENÇÃO fica gravada na linha (`cancelling`) e a rotina não volta. Sem
/// repetir Parar, o efeito é retentado assim que deixa de falhar; se a linha não
/// sai depois do efeito, a liquidação fica gravada (`settled`) e a linha sai no
/// ciclo seguinte com o teto AINDA travado. A liberação não executa nada.
#[tokio::test]
async fn teto_parar_conserva_espera_se_o_efeito_falha() {
    let f = Fixture::new();
    let hub = &f.hub;
    config(hub, &json!({"enabled":true,"tokens":10}));
    ledger(&hub.store, now_in_month(&hub.store), Some((0.1, "USD")), None);
    held_routine(&f, "Rotina parada").await;
    // 1) O cancelamento da rotina falha: a intenção é durável e nada sai.
    sql(hub, INJ_ROTINA);
    crate::api::dispatch(hub, &Caller::Local, "stop", json!({"botId":"bot"})).await.unwrap();
    f.until("teto_parar_conserva_espera_se_o_efeito_falha", || cancel_recorded(hub)).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(holds(&hub.store).len(), 1, "efeito falhou: a espera fica, marcada");
    assert_eq!(run_status(hub), "starting");
    // 2) Sem repetir Parar: o efeito volta a valer, mas a linha não sai — liquidada.
    sql(hub, INJ_LINHA);
    sql(hub, "DROP TRIGGER teto_inj_rotina;");
    f.until("teto_parar_conserva_espera_se_o_efeito_falha", || run_status(hub) == "cancelled").await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(holds(&hub.store).len(), 1, "linha conservada até poder sair");
    assert!(cancel_recorded(hub), "a intenção fica gravada até a linha sair");
    // 3) A linha liquidada sai no ciclo seguinte, com o teto ainda travado.
    sql(hub, "DROP TRIGGER teto_inj_linha;");
    f.until("teto_parar_conserva_espera_se_o_efeito_falha", || holds(&hub.store).is_empty()).await;
    assert!(!blocked_fp(hub).await.is_empty(), "o teto continua travado");
    assert!(!hub.routines.chain_reserved_for_test("bot"), "a vaga de rotina do bot voltou");
    release(hub, "checkFailure", &blocked_fp(hub).await).await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(f.prompts().is_empty(), "rotina cancelada não volta");
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Revisão final P2 #1 + P3 #3 através de reinícios: a intenção de cancelar
/// gravada antes do restart é cumprida pelo Hub novo sem repetir Parar, e a
/// liquidação gravada antes de outro restart apaga a linha com o teto travado.
/// Liberar o teto depois disso não executa a rotina.
#[tokio::test]
async fn teto_parar_com_falha_sobrevive_ao_restart_e_nunca_executa() {
    let mut f = Fixture::new();
    config(&f.hub, &json!({"enabled":true,"tokens":10}));
    ledger(&f.hub.store, now_in_month(&f.hub.store), Some((0.1, "USD")), None);
    held_routine(&f, "Rotina parada").await;
    sql(&f.hub, INJ_ROTINA);
    crate::api::dispatch(&f.hub, &Caller::Local, "stop", json!({"botId":"bot"})).await.unwrap();
    f.until("teto_parar_com_falha_sobrevive_ao_restart_e_nunca_executa", || cancel_recorded(&f.hub)).await;
    assert_eq!(run_status(&f.hub), "starting");
    f.hub.shutdown().await;
    // O efeito volta a funcionar; apagar a linha ainda falha.
    Store::open(&f.dir.join("test.db"))
        .unwrap()
        .db
        .locked()
        .execute_batch(&format!("DROP TRIGGER teto_inj_rotina; {INJ_LINHA}"))
        .unwrap();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until("teto_parar_com_falha_sobrevive_ao_restart_e_nunca_executa", || run_status(&f.hub) == "cancelled").await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(holds(&f.hub.store).len() == 1 && cancel_recorded(&f.hub), "liquidada: só a linha e a intenção ficam");
    assert!(f.prompts().is_empty(), "a intenção restaurada nunca executa a rotina");
    f.hub.shutdown().await;
    Store::open(&f.dir.join("test.db")).unwrap().db.locked().execute_batch("DROP TRIGGER teto_inj_linha;").unwrap();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until("teto_parar_com_falha_sobrevive_ao_restart_e_nunca_executa", || holds(&f.hub.store).is_empty()).await;
    assert!(!blocked_fp(&f.hub).await.is_empty(), "liquidação saiu com o teto travado");
    release(&f.hub, "checkFailure", &blocked_fp(&f.hub).await).await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(f.prompts().is_empty(), "a rotina cancelada não executa depois da liberação");
    assert_eq!(run_status(&f.hub), "cancelled");
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Revisão final P3 #4: lote [e1,e2] parado (seq 1) e uma rotina parada depois
/// (seq 2). Parar e1 com a reescrita do lote falhando: a linha fica no lugar e a
/// captura não sai com e1; a reescrita é retentada no lugar. Liberado, e2 sai
/// ANTES da rotina — a ordem de chegada se conserva; e1 nunca sai.
#[tokio::test]
async fn teto_falha_na_reescrita_do_lote_conserva_a_ordem() {
    let f = Fixture::new();
    let hub = &f.hub;
    std::fs::write(f.dir.join("blocked"), "").unwrap();
    std::fs::write(f.dir.join("hold"), "").unwrap();
    let send =
        |text: &'static str| crate::api::dispatch(hub, &Caller::Local, "send", json!({"botId":"bot","text":text}));
    send("turno em andamento").await.unwrap();
    f.until("teto_falha_na_reescrita_do_lote_conserva_a_ordem", || f.prompts().len() == 1).await;
    send("LOTE-UM").await.unwrap();
    send("LOTE-DOIS").await.unwrap();
    let (e1, e2) = (user_entry(hub, "LOTE-UM"), user_entry(hub, "LOTE-DOIS"));
    config(hub, &json!({"enabled":true,"tokens":10,"applyToConversations":true}));
    ledger(&hub.store, now_in_month(&hub.store), Some((0.1, "USD")), None);
    std::fs::remove_file(f.dir.join("hold")).unwrap();
    f.until("teto_falha_na_reescrita_do_lote_conserva_a_ordem", || holds(&hub.store).len() == 1).await;
    let run_id = held_routine(&f, "Rotina depois do lote").await;
    let works = || holds(&hub.store).into_iter().map(|h| h.work).collect::<Vec<_>>();
    assert_eq!(works(), [format!("human:{e1},{e2}"), format!("routine:{run_id}")]);
    sql(
        hub,
        "CREATE TRIGGER teto_inj_lote BEFORE UPDATE ON donna_ceiling_holds BEGIN SELECT RAISE(ABORT,'injected'); END;",
    );
    stop_entry(hub, &e1).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(works(), [format!("human:{e1},{e2}"), format!("routine:{run_id}")], "linha e ordem no lugar");
    assert_eq!(hub.store.entry(&e1).unwrap().data["status"], "cancelled");
    sql(hub, "DROP TRIGGER teto_inj_lote;");
    f.until("teto_falha_na_reescrita_do_lote_conserva_a_ordem", || {
        works() == [format!("human:{e2}"), format!("routine:{run_id}")]
    })
    .await;
    release(hub, "checkFailure", &blocked_fp(hub).await).await.unwrap();
    f.until("teto_falha_na_reescrita_do_lote_conserva_a_ordem", || f.prompts().len() >= 3).await;
    tokio::time::sleep(Duration::from_millis(600)).await;
    let prompts: Vec<String> = f.prompts().iter().map(Value::to_string).collect();
    assert_eq!(prompts.len(), 3, "{prompts:?}");
    assert!(prompts[1].contains("LOTE-DOIS"), "e2 sai na vez dele, antes da rotina: {prompts:?}");
    assert!(prompts[2].contains("Scheduled routine"), "a rotina sai depois: {prompts:?}");
    assert!(!prompts.iter().any(|p| p.contains("LOTE-UM")), "e1 parada nunca sai");
    assert!(holds(&hub.store).is_empty());
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Revisão final P2 #2 (migração): linha da versão anterior (sem `waiting`) cujas
/// entradas e conversa são as do checkpoint D72 vivo vira espera D72; a captura
/// humana comum do mesmo bot não é tocada.
#[test]
fn teto_migracao_marca_espera_d72_da_versao_anterior() {
    let dir = std::env::temp_dir().join(format!("teto-d72-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.db");
    let row = |entry: &str| {
        json!({"attempt":"a","executionToken":"t","origin":"human","originDetail":{},
            "lane":{"chat":"bot","session":null},"setup":{"entries":[entry],"text":"oi","hidden":false},
            "reason":"unverifiable","metrics":["tokens"],"signature":"fp#1","at":1,"work":format!("human:{entry}")})
    };
    {
        let store = Store::open(&path).unwrap();
        for entry in ["e-d72", "e-comum"] {
            store
                .db
                .locked()
                .execute(
                    "INSERT INTO donna_ceiling_holds(bot_id,work,kind,body) VALUES('bot',?1,'turn',?2)",
                    params![format!("human:{entry}"), row(entry).to_string()],
                )
                .unwrap();
        }
        store
            .kv_set(
                "turn.waiting.bot",
                &json!({"phase":"waiting","input":{"chat":"bot","entries":["e-d72"]}}).to_string(),
            )
            .unwrap();
    }
    let store = Store::open(&path).unwrap();
    let rows = teto::turn_holds_of(&store.db.locked(), "bot").unwrap();
    assert_eq!(rows[0].body["waiting"], true, "a captura da retomada D72 vira espera D72");
    assert_eq!(rows[0].body["migratedWaiting"], true);
    assert!(rows[1].body.get("waiting").is_none(), "a captura comum fica como estava");
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Revisão final P2 #2 (restore): Hub atualizado sobre a linha da versão anterior
/// (sem `waiting`) + checkpoint D72; liberar antes do retryAt dá UMA retomada, sem
/// captura residual.
#[tokio::test]
async fn teto_retomada_d72_da_versao_anterior_retoma_uma_vez() {
    let mut f = Fixture::new();
    f.send().await;
    assert_eq!(f.prompts().len(), 1, "primeiro envio esbarrou no limite da assinatura");
    f.hub.shutdown().await;
    {
        let store = Store::open(&f.dir.join("test.db")).unwrap();
        store.kv_set(KEY, &json!({"enabled":true,"tokens":10,"applyToConversations":true}).to_string()).unwrap();
        ledger(&store, now_in_month(&store), Some((0.1, "USD")), None);
    }
    f.make_retry_due();
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until("teto_retomada_d72_da_versao_anterior_retoma_uma_vez", || holds(&f.hub.store).len() == 1).await;
    let checkpoint =
        |hub: &Hub| -> Value { serde_json::from_str(&hub.store.kv_get("turn.waiting.bot").unwrap()).unwrap() };
    f.until("teto_retomada_d72_da_versao_anterior_retoma_uma_vez", || checkpoint(&f.hub)["phase"] == "waiting").await;
    f.hub.shutdown().await;
    // A linha como a versão anterior (732ea047b) gravava: captura humana sem `waiting`.
    // Retirada DEPOIS da migração do boot, para provar também o reconhecimento no restore.
    let store = Store::open(&f.dir.join("test.db")).unwrap();
    {
        let db = store.db.locked();
        let raw: String = db.query_row("SELECT body FROM donna_ceiling_holds", [], |r| r.get(0)).unwrap();
        let mut body: Value = serde_json::from_str(&raw).unwrap();
        body.as_object_mut().unwrap().remove("waiting");
        db.execute("UPDATE donna_ceiling_holds SET body=?1", [body.to_string()]).unwrap();
    }
    f.hub = Fixture::open(&f.dir, store);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(holds(&f.hub.store).len(), 1);
    assert_eq!(f.prompts().len(), 1);
    assert!(checkpoint(&f.hub)["retryAt"].as_i64().unwrap() > crate::store::now_ms(), "liberação antes do retryAt");
    release(&f.hub, "checkFailure", &blocked_fp(&f.hub).await).await.unwrap();
    f.until("teto_retomada_d72_da_versao_anterior_retoma_uma_vez", || f.prompts().len() == 2).await;
    f.until("teto_retomada_d72_da_versao_anterior_retoma_uma_vez", || f.hub.runtime("bot").status == BotStatus::Idle)
        .await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(f.prompts().len(), 2, "uma única retomada, sem captura residual");
    assert!(holds(&f.hub.store).is_empty());
    f.assert_finished_once();
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// FINAL 2 P2-3: `checkFailure` exige a assinatura COMPLETA vista pelo dono
/// (`blocked.signature` = `fingerprint#geração`). F#1 -> saudável -> F#3: a
/// confirmação atrasada de F#1 é recusada (`ceiling_state_changed`, o app relê e
/// pergunta de novo) e a de F#3 é aceita. Contrato de `donnaCeiling.blocked`.
#[tokio::test]
async fn teto_check_failure_atrasado_nao_libera_geracao_nova() {
    let (hub, dir) = hub_tmp();
    let store = &hub.store;
    config(&hub, &json!({"enabled":true,"tokens":100}));
    ledger_row(store, "linha", None);
    let blocked = || async {
        teto::call(&hub, &Caller::Local, "donnaCeiling", json!({})).await.unwrap()["state"]["blocked"].clone()
    };
    let first = blocked().await;
    let mut keys: Vec<&String> = first.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["fingerprint", "kind", "metrics", "signature"], "contrato de donnaCeiling.blocked");
    assert_eq!(first["kind"], "unverifiable");
    let f1 = first["signature"].as_str().unwrap().to_owned();
    let fingerprint = first["fingerprint"].as_str().unwrap().to_owned();
    assert_eq!(f1, format!("{fingerprint}#1"));
    set_row_tokens(store, "linha", Some((1, 1)));
    assert!(blocked().await.is_null(), "saudável observado: a geração anda");
    set_row_tokens(store, "linha", None);
    let f3 = blocked().await["signature"].as_str().unwrap().to_owned();
    assert_eq!(f3, format!("{fingerprint}#3"), "a mesma impressão de volta é geração nova");
    for stale in [f1.as_str(), fingerprint.as_str()] {
        let error = release(&hub, "checkFailure", stale).await.unwrap_err();
        assert!(format!("{error:#}").contains("ceiling_state_changed"), "{stale}: {error:#}");
    }
    assert!(!blocked().await.is_null(), "pedido atrasado não liberou F#3");
    let released = release(&hub, "checkFailure", &f3).await.unwrap();
    assert_eq!(released["release"]["kind"], "checkFailure");
    assert!(blocked().await.is_null(), "o pedido sobre F#3 libera F#3");
    drop(hub);
    let _ = std::fs::remove_dir_all(&dir);
}

/// FINAL 2 P2-1 (cenário do revisor): rotina Starting parada; falham a reescrita
/// da linha da espera E o cancelamento em routines.v1. A intenção de cancelar vai
/// por INSERT próprio (não depende de reescrever a linha), logo é durável e Parar
/// não mente. Reinício sob as mesmas falhas, falhas removidas, teto liberado =>
/// a rotina NÃO executa.
#[tokio::test]
async fn teto_parar_sem_reescrever_a_linha_sobrevive_ao_restart() {
    let mut f = Fixture::new();
    config(&f.hub, &json!({"enabled":true,"tokens":10}));
    ledger(&f.hub.store, now_in_month(&f.hub.store), Some((0.1, "USD")), None);
    held_routine(&f, "Rotina parada").await;
    sql(&f.hub, INJ_ROTINA);
    sql(
        &f.hub,
        "CREATE TRIGGER teto_inj_marca BEFORE UPDATE ON donna_ceiling_holds BEGIN SELECT RAISE(ABORT,'injected'); END;",
    );
    crate::api::dispatch(&f.hub, &Caller::Local, "stop", json!({"botId":"bot"})).await.unwrap();
    assert!(cancel_recorded(&f.hub), "intenção durável apesar da linha não poder ser reescrita");
    assert_eq!(run_status(&f.hub), "starting");
    f.hub.shutdown().await;
    f.hub = Fixture::open(&f.dir, Store::open(&f.dir.join("test.db")).unwrap());
    f.until("teto_parar_sem_reescrever_a_linha_sobrevive_ao_restart", || {
        f.hub.runtime("bot").activity == "Parado pelo teto de gasto."
    })
    .await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(run_status(&f.hub), "pending", "mesmas falhas: o efeito ainda não vale (Starting -> Pending no boot)");
    sql(&f.hub, "DROP TRIGGER teto_inj_rotina; DROP TRIGGER teto_inj_marca;");
    f.until("teto_parar_sem_reescrever_a_linha_sobrevive_ao_restart", || run_status(&f.hub) == "cancelled").await;
    f.until("teto_parar_sem_reescrever_a_linha_sobrevive_ao_restart", || holds(&f.hub.store).is_empty()).await;
    release(&f.hub, "checkFailure", &blocked_fp(&f.hub).await).await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(f.prompts().is_empty(), "a rotina parada pelo dono nunca executa");
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// FINAL 2 P2-1: nem a intenção nem o efeito puderam ser gravados — Parar devolve
/// erro explícito (não anuncia cancelamento durável) e a espera continua
/// bloqueada para retomada: liberar o teto não executa nada. Quando o banco volta,
/// o ciclo grava a intenção e cancela sem o dono repetir Parar.
#[tokio::test]
async fn teto_parar_sem_nada_duravel_devolve_erro_e_segura_a_espera() {
    let f = Fixture::new();
    let hub = &f.hub;
    config(hub, &json!({"enabled":true,"tokens":10}));
    ledger(&hub.store, now_in_month(&hub.store), Some((0.1, "USD")), None);
    held_routine(&f, "Rotina parada").await;
    sql(hub, INJ_ROTINA);
    sql(
        hub,
        "CREATE TRIGGER teto_inj_intencao BEFORE INSERT ON donna_ceiling_cancels BEGIN SELECT RAISE(ABORT,'injected'); END;",
    );
    let error = crate::api::dispatch(hub, &Caller::Local, "stop", json!({"botId":"bot"})).await.unwrap_err();
    assert!(format!("{error:#}").contains("ceiling_cancel_not_durable"), "{error:#}");
    release(hub, "checkFailure", &blocked_fp(hub).await).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(f.prompts().is_empty(), "a espera segue bloqueada para retomada");
    assert_eq!(holds(&hub.store).len(), 1);
    sql(hub, "DROP TRIGGER teto_inj_rotina; DROP TRIGGER teto_inj_intencao;");
    f.until("teto_parar_sem_nada_duravel_devolve_erro_e_segura_a_espera", || run_status(hub) == "cancelled").await;
    f.until("teto_parar_sem_nada_duravel_devolve_erro_e_segura_a_espera", || holds(&hub.store).is_empty()).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(f.prompts().is_empty(), "nada enviado depois do cancelamento");
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// FINAL 2 P2-2: rotina parada com o cancelamento pedido e o efeito pendente. No
/// boot a leitura das esperas falha (uma vez): o ator fica fechado (o re-drive do
/// agendador não despacha) e relê até conseguir. O banco volta: a espera com a
/// intenção é restaurada, a admissão nunca apaga nem sobrescreve a linha, e
/// liberar o teto não envia nenhum prompt.
#[tokio::test]
async fn teto_restore_que_falha_fecha_o_ator_e_preserva_o_cancelamento() {
    let mut f = Fixture::new();
    config(&f.hub, &json!({"enabled":true,"tokens":10}));
    ledger(&f.hub.store, now_in_month(&f.hub.store), Some((0.1, "USD")), None);
    held_routine(&f, "Rotina parada").await;
    sql(&f.hub, INJ_ROTINA);
    let _ = crate::api::dispatch(&f.hub, &Caller::Local, "stop", json!({"botId":"bot"})).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(run_status(&f.hub), "starting", "efeito pendente");
    f.hub.shutdown().await;
    let store = Store::open(&f.dir.join("test.db")).unwrap();
    store.db.locked().execute_batch("ALTER TABLE donna_ceiling_holds RENAME TO teto_holds_fora;").unwrap();
    f.hub = Fixture::open(&f.dir, store);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(f.prompts().is_empty(), "sem esperas lidas, nada é despachado (nem o re-drive)");
    sql(&f.hub, "ALTER TABLE teto_holds_fora RENAME TO donna_ceiling_holds;");
    f.until("teto_restore_que_falha_fecha_o_ator_e_preserva_o_cancelamento", || {
        f.hub.runtime("bot").activity == "Parado pelo teto de gasto."
    })
    .await;
    tokio::time::sleep(Duration::from_millis(1000)).await;
    assert_eq!(holds(&f.hub.store).len(), 1, "a espera com o cancelamento continua");
    release(&f.hub, "checkFailure", &blocked_fp(&f.hub).await).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(f.prompts().is_empty(), "liberar o teto não envia a rotina cancelada");
    sql(&f.hub, "DROP TRIGGER teto_inj_rotina;");
    f.until("teto_restore_que_falha_fecha_o_ator_e_preserva_o_cancelamento", || run_status(&f.hub) == "cancelled")
        .await;
    f.until("teto_restore_que_falha_fecha_o_ator_e_preserva_o_cancelamento", || holds(&f.hub.store).is_empty()).await;
    assert!(f.prompts().is_empty(), "nada enviado depois do cancelamento");
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// FINAL 2 P2-2(b): na fronteira durável de admissão, a linha de um trabalho
/// cancelado nunca é apagada (teto aberto) nem sobrescrita (teto fechado).
#[test]
fn teto_admissao_nao_apaga_nem_sobrescreve_trabalho_cancelado() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    config_for(&store, &json!({"enabled":true,"tokens":100}));
    ledger(&store, now_in_month(&store), Some((0.1, "USD")), None);
    assert_eq!(decide(&store, &Origin::Routine { id: "r".into() }), teto::Verdict::Hold);
    let before = hold(&store).unwrap();
    teto::record_cancel(&store.db.locked(), "bot", "routine:r").unwrap();
    ledger(&store, now_in_month(&store), Some((0.1, "USD")), Some((1, 1)));
    assert_eq!(decide(&store, &Origin::Routine { id: "r".into() }), teto::Verdict::Hold, "fechado: segura");
    assert_eq!(hold(&store).unwrap(), before, "corpo intocado");
    config_for(&store, &json!({"enabled":false}));
    assert_eq!(decide(&store, &Origin::Routine { id: "r".into() }), teto::Verdict::Hold, "aberto: não passa");
    assert_eq!(hold(&store).unwrap(), before, "linha não apagada");
}

/// Revisão FINAL 2 P2-2 (a), complemento: só a LEITURA do restore falha (a
/// tabela segue usável por `decide_on`), com a marca `cancelling` do formato
/// anterior na linha, efeito pendente e re-drive explícito. No boot, a LEITURA do restore falha (linha com corpo ilegível
/// para o driver): o ator fica travado — o re-drive da rotina não passa nem
/// regrava a linha — e retenta o restore. O banco volta, o restore vale, a marca
/// é respeitada: liberar o teto não envia nenhum prompt.
#[tokio::test]
async fn teto_restore_falho_trava_o_ator_e_preserva_a_marca() {
    let mut f = Fixture::new();
    config(&f.hub, &json!({"enabled":true,"tokens":10}));
    ledger(&f.hub.store, now_in_month(&f.hub.store), Some((0.1, "USD")), None);
    let run_id = held_routine(&f, "Rotina cancelando").await;
    f.hub.shutdown().await;
    let store = Store::open(&f.dir.join("test.db")).unwrap();
    {
        let db = store.db.locked();
        db.execute_batch(INJ_ROTINA).unwrap();
        db.execute("UPDATE donna_ceiling_holds SET body=json_set(body,'$.cancelling',json('true'))", []).unwrap();
        // Depois da migração do open, antes do restore do ator: a leitura do restore falha.
        db.execute("INSERT INTO donna_ceiling_holds(bot_id,work,kind,body) VALUES('bot','ilegivel','turn',x'00')", [])
            .unwrap();
    }
    f.hub = Fixture::open(&f.dir, store);
    // Re-drive explícito (além do agendador): não pode passar nem regravar a linha.
    f.hub.send_cmd("bot", crate::agent::bot::Cmd::Routine(run_id.clone())).unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(f.prompts().is_empty(), "ator travado enquanto o restore falha");
    let row = |hub: &Hub| {
        holds(&hub.store)
            .into_iter()
            .find(|h| h.work == format!("routine:{run_id}"))
            .map(|h| h.body)
            .unwrap_or_default()
    };
    // `holds` lê com o driver: a linha ilegível precisa sair antes.
    sql(&f.hub, "DELETE FROM donna_ceiling_holds WHERE work='ilegivel';");
    assert_eq!(row(&f.hub)["cancelling"], true, "a marca do dono continua na linha");
    f.until("teto_restore_falho_trava_o_ator_e_preserva_a_marca", || {
        f.hub.runtime("bot").activity == "Parado pelo teto de gasto."
    })
    .await;
    release(&f.hub, "checkFailure", &blocked_fp(&f.hub).await).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(f.prompts().is_empty(), "rotina cancelada pelo dono não sai com o teto aberto");
    assert_eq!(row(&f.hub)["cancelling"], true);
    sql(&f.hub, "DROP TRIGGER teto_inj_rotina;");
    f.until("teto_restore_falho_trava_o_ator_e_preserva_a_marca", || run_status(&f.hub) == "cancelled").await;
    f.until("teto_restore_falho_trava_o_ator_e_preserva_a_marca", || holds(&f.hub.store).is_empty()).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(f.prompts().is_empty(), "nada enviado depois do cancelamento");
    f.hub.shutdown().await;
    let _ = std::fs::remove_dir_all(&f.dir);
}

/// Revisão FINAL 2 P2-2 (b), complemento: marcas `cancelling`/`settled` do
/// formato anterior na linha — `decide_on` não apaga (teto aberto) nem
/// sobrescreve (teto fechado), e `write_hold` não as perde.
#[tokio::test]
async fn teto_decide_on_preserva_marca_do_dono() {
    for flag in ["cancelling", "settled"] {
        let (hub, dir) = hub_tmp();
        let store = &hub.store;
        config(&hub, &json!({"enabled":true,"tokens":100}));
        ledger(store, now_in_month(store), Some((0.1, "USD")), None);
        let routine = Origin::Routine { id: "r".into() };
        assert_eq!(decide(store, &routine), teto::Verdict::Hold);
        store
            .db
            .locked()
            .execute(
                "UPDATE donna_ceiling_holds SET body=json_set(body,'$.'||?1,json('true')) WHERE work='routine:r'",
                [flag],
            )
            .unwrap();
        // Teto fechado: captura nova não regrava a linha nem perde a marca.
        assert_eq!(decide(store, &routine), teto::Verdict::Hold);
        assert_eq!(hold(store).unwrap()[flag], true, "{flag}: marca preservada com o teto fechado");
        let mut body = hold(store).unwrap();
        body["cancelling"] = Value::Null;
        body["settled"] = Value::Null;
        teto::write_hold(&store.db.locked(), "bot", "routine:r", "turn", &body).unwrap();
        assert_eq!(hold(store).unwrap()[flag], true, "{flag}: write_hold não apaga a marca");
        // Teto aberto: o trabalho não passa e a linha não é apagada.
        release(&hub, "checkFailure", &blocked_fp(&hub).await).await.unwrap();
        assert_eq!(decide(store, &routine), teto::Verdict::Hold, "{flag}: decisão do dono vale com o teto aberto");
        assert_eq!(hold(store).unwrap()[flag], true, "{flag}: linha conservada com o teto aberto");
        drop(hub);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

// MARK: MX-159 período diário

fn day_config(store: &Store, zone: &str) {
    config_for(
        store,
        &json!({"enabled":true,"period":"day","timeZone":zone,"money":[{"currency":"USD","limit":10.0}],"tokens":1000}),
    );
}

/// Início da janela vigente no período/fuso da config gravada.
fn day_window(store: &Store) -> teto::Window {
    let db = store.db.locked();
    let settings = teto::settings_on(&db).unwrap();
    teto::window_on(&db, &settings, crate::store::now_ms()).unwrap()
}

fn ms(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis()
}

fn window_at(store: &Store, zone: &str, at: i64) -> teto::Window {
    let db = store.db.locked();
    let settings = teto::Settings { period: "day".into(), time_zone: Some(zone.into()), ..Default::default() };
    teto::window_on(&db, &settings, at).unwrap()
}

#[test]
fn teto_dia_conta_so_o_dia_local() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    day_config(&store, "America/Sao_Paulo");
    let win = day_window(&store);
    // Gasto de ontem (fora da janela) acima do limite não trava nem conta.
    ledger(&store, win.start - 1_000, Some((99.0, "USD")), Some((999, 999)));
    let g = gate(&store);
    assert!(g.blocked.is_none(), "ontem ficou fora da janela do dia");
    assert_eq!(g.metrics[0].used, 0.0);
    // O mesmo gasto dentro do dia trava por valor.
    ledger(&store, win.start + 1, Some((11.0, "USD")), Some((10, 10)));
    let g = gate(&store);
    assert_eq!(g.blocked.unwrap().reason, "valueReached");
    assert_eq!(g.period, "day");
    assert_eq!(g.time_zone, "America/Sao_Paulo");
    assert_eq!((g.start, g.end), (win.start, win.end));
}

#[test]
fn teto_janela_do_dia_no_fuso_do_dono() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    // 00:30 UTC = 21:30 do dia anterior em São Paulo (UTC-3, sem horário de verão).
    let at = ms("2026-06-15T00:30:00Z");
    let win = window_at(&store, "America/Sao_Paulo", at);
    assert_eq!(win.label, "2026-06-14@America/Sao_Paulo", "21h local ainda é o dia anterior");
    let start = ms("2026-06-14T03:00:00Z");
    assert_eq!((win.start, win.end), (start, start + 86_400_000));
    // Em UTC puro o mesmo instante já é 15: a virada é no fuso, não em UTC.
    let utc = ms("2026-06-15T02:59:59Z");
    assert_eq!(window_at(&store, "America/Sao_Paulo", utc).label, "2026-06-14@America/Sao_Paulo");
    assert_eq!(window_at(&store, "UTC", utc).label, "2026-06-15@UTC");
    // O rótulo do dia é a chave de estado/avisos/liberações — diferente do do mês.
    assert_eq!(window_at(&store, "America/Sao_Paulo", win.end).label, "2026-06-15@America/Sao_Paulo");
}

#[test]
fn teto_dia_dst_23_e_25_horas() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    // America/New_York: 08/03/2026 entra no horário de verão (dia de 23 h),
    // 01/11/2026 sai (dia de 25 h). A janela segue as meias-noites locais.
    let at = ms("2026-03-08T17:00:00Z");
    let win = window_at(&store, "America/New_York", at);
    assert_eq!(win.label, "2026-03-08@America/New_York");
    assert_eq!(win.end - win.start, 82_800_000, "dia de 23 h");
    let at = ms("2026-11-01T17:00:00Z");
    let win = window_at(&store, "America/New_York", at);
    assert_eq!(win.label, "2026-11-01@America/New_York");
    assert_eq!(win.end - win.start, 90_000_000, "dia de 25 h");
}

#[test]
fn teto_liberacao_do_dia_nao_vale_no_dia_seguinte() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    day_config(&store, "UTC");
    let win = day_window(&store);
    // Liberação alta gravada com o rótulo de ONTEM: append-only, mas a janela dela passou.
    let ontem = window_at(&store, "UTC", win.start - 1_000).label;
    store
        .db
        .locked()
        .execute(
            "INSERT INTO donna_ceiling_releases(id,at,actor,kind,month,metric,scope,state)
             VALUES('r-ontem',1,'local','setLimit',?1,'money:USD','autonomous','9999')",
            params![ontem],
        )
        .unwrap();
    ledger(&store, win.start + 1, Some((11.0, "USD")), Some((10, 10)));
    let g = gate(&store);
    assert_eq!(g.blocked.unwrap().reason, "valueReached", "a liberação era de outra janela");
    assert_eq!(g.metrics[0].effective_limit, 10.0, "limite volta ao configurado");
}

#[test]
fn teto_aviso_uma_vez_por_dia() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    day_config(&store, "UTC");
    let win = day_window(&store);
    // Aviso já marcado ontem não abafa o de hoje: a chave é por período.
    let ontem = window_at(&store, "UTC", win.start - 1_000).label;
    store.kv_set(&format!("donna.ceiling.warned.{ontem}"), &json!(["money:USD:9999"]).to_string()).unwrap();
    ledger(&store, win.start + 1, Some((9.0, "USD")), Some((10, 10))); // 90% de 10
    let db = store.db.locked();
    let settings = teto::settings_on(&db).unwrap();
    let eval = teto::evaluate_on(&db, settings.clone()).unwrap();
    assert_eq!(eval.gate.warn_pending, vec!["money:USD:10".to_string()], "aviso novo do dia de hoje");
    assert!(eval.changed);
    let eval = teto::evaluate_on(&db, settings).unwrap();
    assert!(eval.gate.warn_pending.is_empty(), "marcado: não repete no mesmo dia");
    let hoje = format!("donna.ceiling.warned.{}", win.label);
    drop(db);
    let marcados: Vec<String> = serde_json::from_str(&store.kv_read(&hoje).unwrap().unwrap()).unwrap();
    assert_eq!(marcados, ["money:USD:10"]);
}

#[test]
fn teto_mes_inalterado_com_fuso_configurado() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    // Regressão: `month` segue o mês civil UTC mesmo com um fuso exótico gravado.
    config_for(
        &store,
        &json!({"enabled":true,"period":"month","timeZone":"Pacific/Kiritimati","money":[{"currency":"USD","limit":10.0}]}),
    );
    let (mes, start, end) = crate::fork::custo::month(None).unwrap();
    let g = gate(&store);
    assert_eq!((g.period, g.time_zone), ("month".into(), "UTC".into()));
    assert_eq!((g.month.clone(), g.start, g.end), (mes.clone(), start, end));
    // Sem `period` no kv (Hub antigo): idem, e o fuso não aparece na saída.
    config_for(&store, &json!({"enabled":true,"money":[{"currency":"USD","limit":10.0}]}));
    let g = gate(&store);
    assert_eq!(g.period, "month");
    assert_eq!(g.month, mes);
}

#[tokio::test]
async fn teto_save_periodo_e_fuso_e_estado_responde() {
    let (hub, dir) = hub_tmp();
    for bad in [
        json!({"enabled":true,"period":"semana"}),
        json!({"enabled":true,"period":"day","timeZone":"Marte/Olimpo"}),
        json!({"enabled":true,"period":"day","timeZone":3}),
        json!({"enabled":true,"period":"day","surpresa":1}),
    ] {
        assert!(save(&hub, bad.clone()).await.is_err(), "{bad}");
    }
    // Sem `timeZone` no pedido: o padrão é o MESMO do resumo diário — o fuso local do Hub.
    let saved = save(&hub, json!({"enabled":true,"period":"day","tokens":1000})).await.unwrap();
    assert_eq!(saved["period"], "day");
    assert_eq!(saved["timeZone"], crate::fork::resumo_diario::fuso_local());
    // Se o resumo diário já tem fuso, ele é o padrão do teto diário.
    hub.store
        .kv_set(
            "daily_summary.v1",
            &json!({"enabled":true,"time":"19:00","timeZone":"Europe/Lisbon","revision":2}).to_string(),
        )
        .unwrap();
    let saved = save(&hub, json!({"enabled":true,"period":"day","tokens":1000})).await.unwrap();
    assert_eq!(saved["timeZone"], "Europe/Lisbon", "reaproveita o fuso do resumo diário");
    let state = teto::call(&hub, &Caller::Local, "donnaCeiling", json!({})).await.unwrap();
    assert_eq!(state["state"]["period"], "day");
    assert_eq!(state["state"]["window"]["timeZone"], "Europe/Lisbon");
    assert!(state["state"]["window"]["start"].as_i64().unwrap() < state["state"]["window"]["end"].as_i64().unwrap());
    drop(hub);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn teto_hub_com_config_antiga() {
    // kv gravado por Hub sem `period`/`timeZone`: tudo se comporta como mês.
    let (hub, dir) = hub_tmp();
    config(&hub, &json!({"enabled":true,"tokens":10}));
    ledger(&hub.store, now_in_month(&hub.store), Some((0.1, "USD")), Some((10, 10)));
    let state = teto::call(&hub, &Caller::Local, "donnaCeiling", json!({})).await.unwrap();
    assert_eq!(state["state"]["period"], "month");
    assert_eq!(state["state"]["window"]["timeZone"], "UTC");
    assert_eq!(state["state"]["blocked"]["kind"], "valueReached");
    drop(hub);
    let _ = std::fs::remove_dir_all(&dir);
}

/// P2-1: a liberação é da janela EFETIVA (data + fuso). O mesmo dia civil em
/// fusos diferentes são janelas diferentes: a liberação de A não vale em B e
/// volta a valer ao retornar a A na mesma janela.
#[test]
fn teto_liberacao_presa_ao_fuso_da_janela() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    day_config(&store, "Pacific/Kiritimati");
    let win_a = day_window(&store);
    store
        .db
        .locked()
        .execute(
            "INSERT INTO donna_ceiling_releases(id,at,actor,kind,month,metric,scope,state)
             VALUES('ra',1,'local','setLimit',?1,'money:USD','autonomous','9999')",
            params![win_a.label],
        )
        .unwrap();
    ledger(&store, crate::store::now_ms(), Some((11.0, "USD")), Some((10, 10)));
    assert!(gate(&store).blocked.is_none(), "a liberação da janela vale no fuso A");
    let troca = |zone: &str| {
        config_for(
            &store,
            &json!({"enabled":true,"period":"day","timeZone":zone,"money":[{"currency":"USD","limit":10.0}],"tokens":1000}),
        );
    };
    troca("Pacific/Honolulu");
    let g = gate(&store);
    assert_eq!(g.blocked.unwrap().reason, "valueReached", "janela efetiva mudou: liberação de A não vale");
    assert_eq!(g.metrics[0].effective_limit, 10.0, "limite volta ao configurado");
    troca("Pacific/Kiritimati");
    assert_eq!(day_window(&store).label, win_a.label, "mesma data + mesmo fuso = mesma janela");
    assert!(gate(&store).blocked.is_none(), "de volta ao fuso A, a liberação vale de novo");
}

/// P2-2: a fronteira é a troca REAL da data local, nunca a meia-noite suposta —
/// dia civil inteiro pulado no fuso e meia-noite inexistente.
#[test]
fn teto_fronteira_do_dia_sem_meia_noite() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    // Pacific/Apia pulou 2011-12-30 inteiro (UTC-10 -> UTC+14): a janela do dia
    // 29 fecha exatamente no instante do pulo, e a do dia 31 abre nele.
    let w29 = window_at(&store, "Pacific/Apia", ms("2011-12-29T20:00:00Z"));
    assert_eq!(w29.label, "2011-12-29@Pacific/Apia");
    assert_eq!(w29.start, ms("2011-12-29T10:00:00Z"), "meia-noite local do dia 29 (-10)");
    assert_eq!(w29.end, ms("2011-12-30T10:00:00Z"), "o fim é o pulo, não uma meia-noite de 30/12");
    let w31 = window_at(&store, "Pacific/Apia", ms("2011-12-31T05:00:00Z"));
    assert_eq!(w31.label, "2011-12-31@Pacific/Apia");
    assert_eq!(w31.start, w29.end, "o dia 31 começa no instante exato do pulo");
    // Meia-noite inexistente (sem dia pulado): Santiago entra no horário de
    // verão às 24:00 de 07/09/2019 pulando direto para 01:00 de 08/09; o domingo
    // não tem 00:00 e o dia tem 23 h.
    let w = window_at(&store, "America/Santiago", ms("2019-09-08T12:00:00Z"));
    assert_eq!(w.label, "2019-09-08@America/Santiago");
    assert_eq!(w.start, ms("2019-09-08T04:00:00Z"), "o dia começa na primeira hora válida");
    assert_eq!(w.end - w.start, 82_800_000, "dia de 23 h");
    // Havana: mesma situação em 08/03/2020 (00:00 -> 01:00).
    let w = window_at(&store, "America/Havana", ms("2020-03-08T12:00:00Z"));
    assert_eq!(w.start, ms("2020-03-08T05:00:00Z"));
    assert_eq!(w.end - w.start, 82_800_000);
}

/// P2-3: sem registro do resumo diário o padrão é o MESMO do resumo (o fuso
/// local do Hub); registro presente mas ilegível/inválido falha fechado.
#[test]
fn teto_fuso_padrao_igual_ao_do_resumo_e_falha_fecha() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let db = store.db.locked();
    // Sem registro: o padrão é o fuso local do Hub — injetado, nunca `TZ` global.
    let lisboa = || "Europe/Lisbon".to_owned();
    assert_eq!(
        teto::default_time_zone_with(&db, lisboa).unwrap(),
        "Europe/Lisbon",
        "sem registro: o fuso local do Hub"
    );
    // E é exatamente o mesmo cálculo que o resumo diário usa.
    assert_eq!(teto::default_time_zone(&db).unwrap(), crate::fork::resumo_diario::fuso_local());
    drop(db);
    store.kv_set("daily_summary.v1", "lixo").unwrap();
    assert!(teto::default_time_zone(&store.db.locked()).is_err(), "registro ilegível: erro, não fallback");
    // O mesmo carregador do resumo: registro sem os campos dele é erro também.
    store.kv_set("daily_summary.v1", &json!({"timeZone":"Europe/Lisbon"}).to_string()).unwrap();
    assert!(teto::default_time_zone(&store.db.locked()).is_err(), "registro parcial que o resumo não carrega: erro");
    store
        .kv_set(
            "daily_summary.v1",
            &json!({"enabled":true,"time":"19:00","timeZone":"Europe/Lisbon","revision":2}).to_string(),
        )
        .unwrap();
    assert_eq!(teto::default_time_zone(&store.db.locked()).unwrap(), "Europe/Lisbon");
}

/// A espera não sai por uma liberação com o rótulo de OUTRA janela (a virada do
/// dia cria janela nova; o que foi liberado ontem não reabre nada).
#[test]
fn teto_virada_de_janela_nao_libera_espera() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    day_config(&store, "UTC");
    let win = day_window(&store);
    // Tokens NULL na janela -> não conferido -> a decisão real estaciona.
    ledger(&store, crate::store::now_ms(), Some((0.1, "USD")), None);
    let routine = Origin::Routine { id: "r".into() };
    assert_eq!(decide(&store, &routine), teto::Verdict::Hold);
    let signature = fp(&hold(&store).unwrap());
    // checkFailure com a assinatura certa, mas gravada na janela de ONTEM.
    let ontem = window_at(&store, "UTC", win.start - 1_000).label;
    store
        .db
        .locked()
        .execute(
            "INSERT INTO donna_ceiling_releases(id,at,actor,kind,month,metric,scope,state)
             VALUES('r-velha',1,'local','checkFailure',?1,'tokens','autonomous',?2)",
            params![ontem, signature],
        )
        .unwrap();
    assert_eq!(decide(&store, &routine), teto::Verdict::Hold, "a liberação era de outra janela");
    assert!(hold(&store).is_some(), "a espera continua");
}

/// P2-1: a data local pode RECUAR (`Goose_Bay` voltou de 00:00:59 do dia 30 para
/// 22:01 do dia 29 em 30/10/1988). Janela não determinável falha fechado —
/// nunca um início que deixe gasto de fora.
#[test]
fn teto_dia_com_data_que_recua_falha_fechado() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    // Antes, durante e depois do recuo: a data 30 não é contígua nem monotônica.
    for at in ["1988-10-30T02:30:00Z", "1988-10-30T06:00:00Z", "1988-10-30T18:00:00Z"] {
        let db = store.db.locked();
        let settings =
            teto::Settings { period: "day".into(), time_zone: Some("America/Goose_Bay".into()), ..Default::default() };
        assert!(teto::window_on(&db, &settings, ms(at)).is_err(), "{at}: janela não determinável");
    }
    // O dia anterior (sem recuo) segue determinável.
    let w = window_at(&store, "America/Goose_Bay", ms("1988-10-29T12:00:00Z"));
    assert_eq!(w.label, "1988-10-29@America/Goose_Bay");
    assert!(w.start < w.end);
}

/// P2-2: sem janela determinável o estado de falha tem rótulo verificável que
/// expira na virada do dia UTC — a liberação do dia anterior não reabre nada.
#[test]
fn teto_falha_de_janela_expira_na_virada_utc() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    // Config diária SEM timeZone + registro do resumo ilegível = falha persistente.
    config_for(&store, &json!({"enabled":true,"period":"day","tokens":1000}));
    store.kv_set("daily_summary.v1", "lixo").unwrap();
    let db = store.db.locked();
    let settings = teto::settings_on(&db).unwrap();
    let antes = ms("2026-06-15T23:59:59Z");
    let depois = ms("2026-06-16T00:00:00Z");
    let error = teto::window_on(&db, &settings, antes).unwrap_err();
    let gate = teto::failure_gate_at(&db, &settings, &error, antes).unwrap();
    let blocked = gate.blocked.unwrap();
    assert_eq!(blocked.reason, "unverifiable", "falha de janela bloqueia");
    let hoje = gate.month.clone();
    assert!(hoje.starts_with("falha@"), "rótulo verificável de falha: {hoje}");
    // checkFailure aceito hoje abre (vinculado à assinatura+geração de hoje).
    db.execute(
        "INSERT INTO donna_ceiling_releases(id,at,actor,kind,month,metric,scope,state)
         VALUES('rf',1,'local','checkFailure',?1,'ledger','autonomous',?2)",
        params![hoje, gate.signature],
    )
    .unwrap();
    assert!(teto::failure_gate_at(&db, &settings, &error, antes).unwrap().blocked.is_none(), "liberado hoje");
    let amanha = teto::failure_gate_at(&db, &settings, &error, depois).unwrap();
    assert_ne!(amanha.month, hoje, "a virada do dia troca o rótulo de falha");
    assert_eq!(amanha.blocked.unwrap().reason, "unverifiable", "a falha persistente volta a bloquear");
    drop(db);
}

/// Mesmo dia civil em fusos diferentes são janelas diferentes (rótulos e
/// fronteiras distintas) — complementa o caso Kiritimati×Honolulu (24 h).
#[test]
fn teto_mesmo_dia_civil_em_fusos_diferentes() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let at = ms("2026-06-15T12:00:00Z"); // 09:00 em São Paulo: dia 15 nos dois fusos.
    let utc = window_at(&store, "UTC", at);
    let sp = window_at(&store, "America/Sao_Paulo", at);
    assert_eq!(utc.label, "2026-06-15@UTC");
    assert_eq!(sp.label, "2026-06-15@America/Sao_Paulo");
    assert_ne!(utc.label, sp.label, "mesma data civil, janelas diferentes");
    assert_eq!((utc.start, sp.start), (ms("2026-06-15T00:00:00Z"), ms("2026-06-15T03:00:00Z")));
    // Com liberação existente numa delas, trocar o fuso muda a janela efetiva:
    // a liberação gravada sob `2026-06-15@UTC` não aparece na janela de SP.
    {
        let db = store.db.locked();
        db.execute(
            "INSERT INTO donna_ceiling_releases(id,at,actor,kind,month,metric,scope,state)
             VALUES('ru',1,'local','setLimit',?1,'money:USD','autonomous','9999')",
            params![utc.label],
        )
        .unwrap();
        let releases_sp: i64 = db
            .query_row("SELECT COUNT(*) FROM donna_ceiling_releases WHERE month = ?1", params![sp.label], |r| r.get(0))
            .unwrap();
        assert_eq!(releases_sp, 0, "liberação da janela UTC não se aplica à janela SP");
    }
    day_config(&store, "UTC");
    config_for(
        &store,
        &json!({"enabled":true,"period":"day","timeZone":"America/Sao_Paulo","money":[{"currency":"USD","limit":10.0}],"tokens":1000}),
    );
    assert!(day_window(&store).label.ends_with("@America/Sao_Paulo"), "fuso trocado = janela efetiva nova");
}
