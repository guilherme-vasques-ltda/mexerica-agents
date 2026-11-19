//! D70 da D372 (`UI01b`) nas fronteiras de Sessões: retrato do padrão na criação, troca por conversa isolada
//! (`sessionEngineSet`), padrão do bot só para conversas novas, turno em andamento × próximo, retomada pela seleção
//! admitida, CAS/concorrência e falha fechada (conta retirada, motor incompatível, modelo fora do catálogo,
//! caminho sem conta só na principal, sem conta gerenciada nova nem troca de motor). Admissão real: `admissao_tests`.
use super::admissao_tests::{
    F, account, admit, checkpoints, done, exec_cfg, extra, fixture, key, queued, revision, set_engine,
};
use super::{
    admissao::{self, Turn},
    bindings::{self, BindingRef},
    engine_choice::{self, EngineSource},
    store,
};
use crate::{
    LockExt,
    api::devices::Caller,
    fork::{
        bot_admission::{self as admission, ClaimHooks, Origin, RoutingClass},
        roteamento::receipt,
    },
    store::{BotConfig, Lane, Scope, Store},
};
use rusqlite::Connection;
use serde_json::{Value, json};

async fn set(f: &F, caller: &Caller, session: Option<&str>, revision: i64, engine: Value) -> anyhow::Result<Value> {
    crate::api::dispatch(
        &f.hub,
        caller,
        "sessionEngineSet",
        json!({"botId":"dev","sessionId":session,"expectedRevision":revision,"engine":engine}),
    )
    .await
}

fn managed(letter: char, model: &str) -> Value {
    req(Some(account(letter).as_str()), "codex", &json!(model), false)
}

/// O corpo `engine` do pedido, com opções vazias.
fn req(account: Option<&str>, backend: &str, model: &Value, fast: bool) -> Value {
    json!({"accountId":account,"backend":backend,"model":model,"modelOptions":{},"fastMode":fast})
}

fn ghost() -> String {
    format!("ac_{}", "c".repeat(64))
}

fn rev(f: &F, id: &str) -> i64 {
    store::get(&f.hub.store.db.locked(), id).unwrap().unwrap().revision
}

/// A seleção configurada (a que o app vê: próxima ?? efetiva).
fn engine_of(f: &F, id: &str) -> engine_choice::SessionEngine {
    engine_choice::configured(&f.hub.store.db.locked(), id).unwrap().unwrap()
}

/// A seleção efetiva (a que os turnos executam).
fn effective_of(f: &F, id: &str) -> engine_choice::SessionEngine {
    engine_choice::read(&f.hub.store.db.locked(), id).unwrap().unwrap()
}

fn code(error: &anyhow::Error) -> String {
    format!("{error:#}")
}

/// D422: com `dev` como Donna (principal canônica do Hub), o dono troca a sessão extra dela pelo caminho gerenciado;
/// a extra continua recusando o caminho sem conta, sem gravar.
#[tokio::test]
async fn d422_extra_da_donna_troca_pelo_dono_e_recusa_sem_conta() {
    let f = fixture(true);
    f.hub.store.set_team_settings(&json!({"primaryBotId":"dev"})).unwrap();
    let a = extra(&f).session_id().to_owned();
    let before = (rev(&f, &a), engine_of(&f, &a));
    let no_account = req(None, "codex", &json!("model-b"), true);
    let error = set(&f, &Caller::Local, Some(&a), before.0, no_account).await.unwrap_err();
    assert!(code(&error).contains("engine_not_allowed"));
    assert_eq!((rev(&f, &a), engine_of(&f, &a)), before, "sem conta não grava");
    set(&f, &Caller::Local, Some(&a), before.0, managed('a', "model-a")).await.unwrap();
    let after = engine_of(&f, &a);
    assert_eq!((after.account_id.as_deref(), after.model.as_deref()), (Some(account('a').as_str()), Some("model-a")));
}

/// Padrão B na criação; troca só A (extra) e depois a principal; mudar o padrão para A só vale para a conversa nova C.
/// Inclui extra criada e ainda sem envio. A projeção e o evento trazem `engine` com a revisão da sessão.
#[tokio::test]
async fn retrato_na_criacao_troca_isolada_e_padrao_novo_so_em_conversa_nova() {
    let f = fixture(true);
    let mut events = f.hub.events.subscribe();
    let (a, b) = (extra(&f), extra(&f));
    let (a, b) = (a.session_id().to_owned(), b.session_id().to_owned());
    for id in ["dev", a.as_str(), b.as_str()] {
        let engine = engine_of(&f, id);
        assert_eq!(
            (engine.source, engine.account_id.as_deref(), engine.model.as_deref()),
            (EngineSource::BotDefault, Some(account('b').as_str()), Some("model-b"))
        );
    }
    let listed = crate::api::dispatch(&f.hub, &Caller::Local, "sessions", json!({"botId":"dev"})).await.unwrap();
    assert!(listed["sessionActions"].as_array().unwrap().contains(&json!("engine")));
    assert_eq!(listed["sessions"][0]["engine"]["revision"], listed["sessions"][0]["revision"]);
    // Troca só A (conta e modelo A).
    let before_bot = f.hub.store.bot("dev").unwrap().unwrap().config;
    let response = set(&f, &Caller::Local, Some(&a), rev(&f, &a), managed('a', "model-a")).await.unwrap();
    assert_eq!(response["switch"]["kind"], "provider");
    assert_eq!(response["switch"]["appliesAt"], "now");
    assert_eq!(response["switch"]["from"]["accountId"], account('b'));
    assert_eq!(response["session"]["engine"]["source"], "own");
    assert_eq!(response["session"]["engine"]["revision"], response["session"]["revision"]);
    let event = loop {
        let event = events.recv().await.unwrap();
        let own = event["session"]["engine"]["source"] == "own";
        if event["type"] == "session" && event["session"]["id"] == a.as_str() && own {
            break event;
        }
    };
    assert_eq!(event["session"]["engine"]["model"], "model-a");
    assert_eq!(engine_of(&f, &a).account_id.as_deref(), Some(account('a').as_str()));
    assert_eq!(effective_of(&f, &a).account_id.as_deref(), Some(account('b').as_str()), "efetiva até o próximo turno");
    for id in ["dev", b.as_str()] {
        assert_eq!(engine_of(&f, id).account_id.as_deref(), Some(account('b').as_str()), "{id} não muda");
    }
    let after_bot = f.hub.store.bot("dev").unwrap().unwrap().config;
    assert_eq!(
        (after_bot.fork_editor.engine_account_id, after_bot.model),
        (before_bot.fork_editor.engine_account_id, before_bot.model),
        "a troca da conversa nunca edita o padrão do bot"
    );
    // A principal (`sessionId: null`) troca só a si mesma; modelo da mesma conta = `model`.
    set(&f, &Caller::Local, None, rev(&f, "dev"), managed('b', "model-b")).await.unwrap();
    assert_eq!(engine_of(&f, "dev").source, EngineSource::Own);
    assert_eq!(engine_of(&f, &b).source, EngineSource::BotDefault);
    // Padrão do bot vai para A: abertas preservadas, a conversa nova nasce com A.
    let mut bot = f.hub.store.bot("dev").unwrap().unwrap().config;
    bot.model = Some("model-a".into());
    bot.fork_editor.engine_account_id = Some(account('a'));
    f.hub.store.save_bot(&bot).unwrap();
    assert_eq!(engine_of(&f, &b).model.as_deref(), Some("model-b"), "extra sem envio preserva o retrato");
    assert_eq!(engine_of(&f, "dev").model.as_deref(), Some("model-b"));
    assert_eq!(engine_of(&f, &a).model.as_deref(), Some("model-a"));
    let c = extra(&f);
    let engine = engine_of(&f, c.session_id());
    assert_eq!((engine.source, engine.model.as_deref()), (EngineSource::BotDefault, Some("model-a")));
    // A execução de cada conversa segue a própria seleção.
    assert_eq!(exec_cfg(&f, &Lane::main("dev")).unwrap().model.as_deref(), Some("model-b"));
    let lane_b = Lane { chat: "dev".into(), session: Some(b.clone()) };
    assert_eq!(exec_cfg(&f, &lane_b).unwrap().model.as_deref(), Some("model-b"));
}

/// CAS, encerrada, conta retirada/incompatível, modelo fora do catálogo, pedido malformado, conversa de outro bot e
/// caller sem a posse `engine.accounts`: recusam com o código estável e não gravam nada.
#[tokio::test]
async fn troca_falha_fechado_sem_gravar() {
    let f = fixture(true);
    let a = extra(&f).session_id().to_owned();
    let before = (rev(&f, &a), engine_of(&f, &a));
    let stale = rev(&f, &a) - 1;
    let mut option = managed('a', "model-a");
    option["modelOptions"] = json!({"x":"y"});
    let mut source = managed('a', "model-a");
    source["source"] = json!("own");
    let mut no_fast = managed('a', "model-a");
    no_fast.as_object_mut().unwrap().remove("fastMode");
    let account_a = account('a');
    let cases: Vec<(Option<&str>, i64, Value, &str)> = vec![
        (Some(&a), stale, managed('a', "model-a"), "session_revision_conflict"),
        (Some(&a), before.0, req(Some(ghost().as_str()), "codex", &json!("model-a"), false), "engine_unavailable"),
        (Some(&a), before.0, managed('a', "fora-do-catalogo"), "engine_unavailable"),
        (Some(&a), before.0, req(Some(account_a.as_str()), "claude", &json!("model-a"), false), "engine_not_allowed"),
        (Some(&a), before.0, option, "engine_unavailable"),
        (Some(&a), before.0, source, "invalid_field"),
        (Some(&a), before.0, no_fast, "invalid_field"),
        // Extra nunca toma o caminho sem conta.
        (Some(&a), before.0, req(None, "codex", &json!("model-b"), true), "engine_not_allowed"),
        (Some("inexistente"), before.0, managed('a', "model-a"), "unknown_session"),
    ];
    for (session, revision, engine, expected) in cases {
        let error = set(&f, &Caller::Local, session, revision, engine.clone()).await.unwrap_err();
        assert!(code(&error).contains(expected), "{engine} → {expected}");
        assert_eq!((rev(&f, &a), engine_of(&f, &a)), before, "nada gravado: {engine}");
        assert_eq!(effective_of(&f, &a), before.1);
    }
    // `sessionId` ausente não vira principal por omissão.
    let missing = crate::api::dispatch(
        &f.hub,
        &Caller::Local,
        "sessionEngineSet",
        json!({"botId":"dev","expectedRevision":before.0,"engine":managed('a', "model-a")}),
    )
    .await
    .unwrap_err();
    assert!(code(&missing).contains("invalid_field"));
    // Aparelho sem a posse de contas: recusado antes de qualquer leitura.
    let device = Caller::Device { key: "k".into(), scopes: vec![Scope::Control], autoridade: None };
    let error = set(&f, &device, Some(&a), before.0, managed('a', "model-a")).await.unwrap_err();
    assert!(code(&error).contains("engine_owner_only"));
    // Encerrada.
    f.hub.store.db.locked().execute("UPDATE donna_bot_sessions SET state='closed' WHERE id=?1", [&a]).unwrap();
    let error = set(&f, &Caller::Local, Some(&a), before.0, managed('a', "model-a")).await.unwrap_err();
    assert!(code(&error).contains("session_closed"));
    assert_eq!(engine_of(&f, &a), before.1);
}

/// Caminho legado estreito: a principal sem conta gerenciada (CLI/comando próprio) troca só modelo/opções/velocidade,
/// no mesmo motor e sem conta, e só ela. Retirar a conta de quem tem, trocar o motor, adotar conta inexistente ou
/// modelo inválido recusam.
#[tokio::test]
async fn principal_sem_conta_troca_so_modelo_e_velocidade_no_mesmo_motor() {
    let legacy = fixture(false);
    let extra_legacy = extra(&legacy).session_id().to_owned();
    let main = rev(&legacy, "dev");
    let mut fast = req(None, "codex", &json!("model-x"), true);
    fast["modelOptions"] = json!({"effort":"high"});
    set(&legacy, &Caller::Local, None, main, fast).await.unwrap();
    let engine = engine_of(&legacy, "dev");
    assert_eq!(
        (engine.source, engine.account_id.as_deref(), engine.model.as_deref(), engine.fast_mode),
        (EngineSource::Own, None, Some("model-x"), true)
    );
    let cfg = exec_cfg(&legacy, &Lane::main("dev")).unwrap();
    assert!(cfg.fast_mode && cfg.fork_editor.engine_account_id.is_none(), "não concede conta");
    let other = engine_of(&legacy, &extra_legacy);
    assert_eq!((other.model.as_deref(), other.fast_mode), (Some("model-b"), false), "a extra não muda");
    let main = rev(&legacy, "dev");
    for (engine, expected) in [
        (req(None, "claude", &json!("m"), false), "engine_not_allowed"),
        (req(None, "codex", &json!(""), false), "engine_not_allowed"),
        (req(None, "codex", &json!("x".repeat(101)), false), "engine_not_allowed"),
        (req(Some(ghost().as_str()), "codex", &json!("m"), false), "engine_unavailable"),
    ] {
        let error = set(&legacy, &Caller::Local, None, main, engine.clone()).await.unwrap_err();
        assert!(code(&error).contains(expected), "{engine} → {expected}");
    }
    // Quem tem conta gerenciada não a perde pelo caminho sem conta.
    let f = fixture(true);
    let drop_account = req(None, "codex", &json!("model-b"), true);
    let error = set(&f, &Caller::Local, None, rev(&f, "dev"), drop_account).await.unwrap_err();
    assert!(code(&error).contains("engine_not_allowed"));
    assert_eq!(engine_of(&f, "dev").account_id.as_deref(), Some(account('b').as_str()));
}

fn turn<'a>(cfg: &'a BotConfig, lane: &'a Lane, ids: &'a [String], token: &'a str) -> Turn<'a> {
    Turn { cfg, lane, entries: ids, token, account_revision: revision(cfg) }
}

fn dispatch(f: &F, t: &Turn<'_>, binding: &BindingRef) -> anyhow::Result<RoutingClass> {
    let mut db = f.hub.store.db.locked();
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).unwrap();
    admissao::dispatch(&tx, t, binding)
}

/// Troca com o setup/permit suspenso e retry com o MESMO token: a efetiva não muda (a troca fica na próxima), o
/// retry (que não promove) captura a mesma seleção e o despacho estrito admite o turno com ela. Rename (revisão nova
/// sem troca) não autoriza uma configuração fora da efetiva. O turno humano novo seguinte promove uma vez, sem mexer
/// na revisão nem na projeção; troca concorrente com a promoção não se perde.
#[test]
fn troca_durante_o_turno_fica_na_proxima_e_o_turno_novo_promove_uma_vez() {
    let f = fixture(true);
    let lane = extra(&f);
    let session = lane.session_id().to_owned();
    let captured = exec_cfg(&f, &lane).unwrap();
    set_engine(&f, &session, &account('a'), Some("model-a"));
    // O retry do setup (mesmo token) usa a efetiva, sem promover.
    let mut retry = f.hub.store.bot("dev").unwrap().unwrap().config;
    assert!(admissao::apply_choice(&f.hub.store.db.locked(), &lane, &mut retry).unwrap());
    assert_eq!(retry.fork_editor.engine_account_id, captured.fork_editor.engine_account_id);
    assert_eq!(effective_of(&f, &session).account_id.as_deref(), Some(account('b').as_str()));
    let id = queued(&f, &lane);
    let ids = [id.clone()];
    f.hub.prepare_bot_turn("dev", None, |r| r.lane = Some(lane.clone())).unwrap();
    let token = f.hub.runtime("dev").preparing_turn_token.unwrap();
    let ticket = admission::reserve_for_entries(&f.hub.store, &captured, "dev", &token, &Origin::Human, &ids).unwrap();
    let binding: BindingRef =
        bindings::issue(&f.hub.store, "dev", "dev", &session, Some(&format!("acp-{token}"))).unwrap();
    // Rename sobe a revisão sem trocar a seleção: uma configuração fora da efetiva continua recusada.
    let renamed = rev(&f, &session);
    store::rename(&f.hub.store.db.locked(), "dev", &session, "Renomeada", renamed).unwrap();
    let mut forged = captured.clone();
    forged.model = Some("model-a".into());
    forged.fork_editor.engine_account_id = Some(account('a'));
    let error = dispatch(&f, &turn(&forged, &lane, &ids, &token), &binding).unwrap_err();
    assert!(code(&error).contains("engine_choice_execution_mismatch"));
    let t = turn(&captured, &lane, &ids, &token);
    let mut hook = |c: &Connection, original: &BindingRef| admissao::dispatch(c, &t, original);
    let hooks = ClaimHooks { binding: Some(&binding), dispatch: Some(&mut hook), resume: None, ..Default::default() };
    assert!(f.hub.permit_bot_dispatch(&captured, &ticket, None, &ids, lane.session.as_deref(), hooks).unwrap());
    let saved = receipt::read_checkpoint(&f.hub.store.db.locked(), &key(&lane, &token)).unwrap().unwrap();
    assert_eq!(saved.captured().routing.admitted.account_id, account('b'), "o turno segue a seleção capturada");
    admission::finish(&f.hub.store, "dev", &ticket).unwrap();
    // Turno humano novo: promove uma vez, sem bump de revisão e com a mesma projeção.
    let (revision, shown) = (rev(&f, &session), engine_of(&f, &session));
    let mut next = f.hub.store.bot("dev").unwrap().unwrap().config;
    assert!(admissao::start_choice(&f.hub.store.db.locked(), &lane, &mut next).unwrap());
    assert!(!admissao::start_choice(&f.hub.store.db.locked(), &lane, &mut next.clone()).unwrap(), "uma vez só");
    assert_eq!((rev(&f, &session), engine_of(&f, &session)), (revision, shown.clone()));
    assert_eq!(effective_of(&f, &session), shown);
    assert_eq!(next.fork_editor.engine_account_id.as_deref(), Some(account('a').as_str()));
    // Troca concorrente com a promoção: a revisão não mudou, o CAS do dono passa e a próxima nova fica guardada.
    set_engine(&f, &session, &account('b'), Some("model-b"));
    let mut again = f.hub.store.bot("dev").unwrap().unwrap().config;
    admissao::apply_choice(&f.hub.store.db.locked(), &lane, &mut again).unwrap();
    assert_eq!(again.model.as_deref(), Some("model-a"), "a efetiva só muda no próximo turno novo");
    assert_eq!(engine_of(&f, &session).model.as_deref(), Some("model-b"));
    let id = queued(&f, &lane);
    let next = exec_cfg(&f, &lane).unwrap();
    assert_eq!(next.model.as_deref(), Some("model-b"));
    let (result, a, _) = admit(&f, &lane, std::slice::from_ref(&id), &next, None, None, lane.session.as_deref());
    assert!(result.unwrap());
    let saved = receipt::read_checkpoint(&f.hub.store.db.locked(), &key(&lane, &a.token)).unwrap().unwrap();
    assert_eq!(saved.captured().routing.admitted.selection.model.as_deref(), Some("model-b"));
    done(&f, &a);
}

/// Retomada D72 depois de reiniciar: a configuração volta à seleção admitida no despacho original, mesmo que a
/// conversa tenha trocado de seleção durante a espera; o permit confere essa mesma seleção. Uma configuração montada
/// pela seleção atual (troca posterior) não passa como prova do turno antigo. Sem recibo (legado), divergência pausa.
#[test]
fn retomada_usa_a_selecao_admitida_original_e_nao_a_atual() {
    let f = fixture(true);
    let main = Lane::main("dev");
    let id = queued(&f, &main);
    let cfg = exec_cfg(&f, &main).unwrap();
    let ids = [id.clone()];
    let (result, a, _) = admit(&f, &main, &ids, &cfg, None, None, None);
    assert!(result.unwrap());
    done(&f, &a);
    let before = checkpoints(&f);
    // Durante a espera, o dono troca a principal para A: fica na próxima; a retomada não promove.
    set_engine(&f, "dev", &account('a'), Some("model-a"));
    assert_eq!(engine_of(&f, "dev").account_id.as_deref(), Some(account('a').as_str()));
    let mut resumed = f.hub.store.bot("dev").unwrap().unwrap().config;
    let db = || f.hub.store.db.locked();
    admissao::apply_resume(&db(), &main, &mut resumed, Some(&a.token), true, ("codex", None)).unwrap();
    assert_eq!(
        (resumed.fork_editor.engine_account_id.as_deref(), resumed.model.as_deref()),
        (Some(account('b').as_str()), Some("model-b")),
        "a seleção admitida, não a atual"
    );
    let input = json!({"chat":"dev","entries":[id]});
    let waiting = json!({"phase":"retrying","executionToken":a.token,"routing":{"kind":"routed"},"input":input});
    f.hub.store.kv_set("turn.waiting.dev", &waiting.to_string()).unwrap();
    let raw = waiting.to_string();
    let (result, x, _) = admit(&f, &main, &ids, &resumed, Some(&a.token), Some(&raw), None);
    assert!(result.unwrap());
    assert_eq!(checkpoints(&f), before, "sem recibo novo");
    assert_eq!(admission::current(&f.hub.store, "dev").unwrap().unwrap().routing, Some(RoutingClass::Routed {}));
    assert_eq!(effective_of(&f, "dev").account_id.as_deref(), Some(account('b').as_str()), "boot não promove");
    done(&f, &x);
    // A seleção atual não serve de prova do turno antigo.
    let current = exec_cfg(&f, &main).unwrap();
    let (result, x, _) = admit(&f, &main, &ids, &current, Some(&a.token), Some(&raw), None);
    assert!(code(&result.unwrap_err()).contains("engine_choice_execution_mismatch"));
    done(&f, &x);
    // Token sem recibo: falha fechado, nunca vira legado.
    let mut cfg = f.hub.store.bot("dev").unwrap().unwrap().config;
    let error = admissao::apply_resume(&db(), &main, &mut cfg, Some("outro"), true, ("codex", None)).unwrap_err();
    assert!(code(&error).contains("routing_resume_checkpoint_missing"));
    // Legado (sem recibo): retoma só se a seleção atual ainda é a do turno.
    let legacy = fixture(false);
    let old = || legacy.hub.store.db.locked();
    let mut cfg = legacy.hub.store.bot("dev").unwrap().unwrap().config;
    admissao::apply_resume(&old(), &main, &mut cfg, None, false, ("codex", Some("model-b"))).unwrap();
    let mut cfg = legacy.hub.store.bot("dev").unwrap().unwrap().config;
    let error = admissao::apply_resume(&old(), &main, &mut cfg, None, false, ("codex", Some("outro"))).unwrap_err();
    assert!(code(&error).contains("engine_choice_execution_mismatch"));
}

/// Migração única: um banco anterior à D372 (principal e extra sem seleção) ganha o estado vigente uma vez; o padrão
/// editado depois do marcador não reescreve nada. Cobertura do formato antigo e ilegível: `pr4_tests`.
#[test]
fn migracao_da_store_real_preenche_uma_vez() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let raw = json!({"id":"dev","name":"dev","backend":"codex","model":"m1"});
    let cfg: BotConfig = serde_json::from_value(raw).unwrap();
    store.save_bot(&cfg).unwrap();
    {
        let c = store.db.locked();
        c.execute("UPDATE donna_bot_sessions SET engine_choice=NULL", []).unwrap();
        c.execute("DELETE FROM kv WHERE k='sessoes.engine.v2'", []).unwrap();
        super::migrate(&c).unwrap();
        assert_eq!(engine_choice::read(&c, "dev").unwrap().unwrap().model.as_deref(), Some("m1"));
    }
    let mut cfg = store.bot("dev").unwrap().unwrap().config;
    cfg.model = Some("m2".into());
    store.save_bot(&cfg).unwrap();
    let c = store.db.locked();
    super::migrate(&c).unwrap();
    assert_eq!(engine_choice::read(&c, "dev").unwrap().unwrap().model.as_deref(), Some("m1"), "uma vez só");
}

/// Decisão A no vínculo: rotina e canal têm slot próprio mesmo na sessão da principal. Com a principal em A e o
/// padrão do bot em B, o vínculo da rotina ativa pela configuração do padrão (nunca pela efetiva da principal) e é
/// revogado quando o padrão muda; o da principal ativa só pela efetiva dela e sobrevive à troca do padrão.
#[test]
fn vinculo_de_rotina_segue_o_padrao_e_o_da_principal_a_selecao_propria() {
    let f = fixture(true);
    set_engine(&f, "dev", &account('a'), Some("model-a"));
    let principal_cfg = exec_cfg(&f, &Lane::main("dev")).unwrap();
    assert_eq!(principal_cfg.fork_editor.engine_account_id.as_deref(), Some(account('a').as_str()));
    let default_cfg = f.hub.store.bot("dev").unwrap().unwrap().config;
    assert_eq!(default_cfg.fork_editor.engine_account_id.as_deref(), Some(account('b').as_str()));
    let routine =
        bindings::issue_in(&f.hub.store, "dev", "dev", "dev", Some("routine:r1"), Some("acp-rotina")).unwrap();
    assert!(bindings::activate_for_config(&f.hub.store, &routine, "acp-rotina", &principal_cfg).is_err());
    bindings::activate_for_config(&f.hub.store, &routine, "acp-rotina", &default_cfg).unwrap();
    let main = bindings::issue(&f.hub.store, "dev", "dev", "dev", Some("acp-principal")).unwrap();
    assert!(bindings::activate_for_config(&f.hub.store, &main, "acp-principal", &default_cfg).is_err());
    bindings::activate_for_config(&f.hub.store, &main, "acp-principal", &principal_cfg).unwrap();
    // O padrão muda: só o vínculo da rotina é revogado.
    let mut bot = f.hub.store.bot("dev").unwrap().unwrap().config;
    bot.model = Some("model-a".into());
    bot.fork_editor.engine_account_id = Some(account('a'));
    f.hub.store.save_bot(&bot).unwrap();
    assert!(bindings::resolve(&f.hub.store.db.locked(), routine.id.as_str(), "dev").is_err());
    assert!(bindings::resolve(&f.hub.store.db.locked(), main.id.as_str(), "dev").is_ok());
    // A principal com seleção própria grava o carimbo da sessão; sem seleção, o do bot.
    let db = f.hub.store.db.locked();
    assert!(admissao::principal_owns_stamp(&db, &principal_cfg).unwrap());
    db.execute("UPDATE donna_bot_sessions SET engine_choice=NULL WHERE id='dev'", []).unwrap();
    assert!(!admissao::principal_owns_stamp(&db, &principal_cfg).unwrap());
}

/// R4: cada chave do contrato `engine` é obrigatória (ausente ≠ `null`); falta recusa sem gravar, sem CAS nem evento.
/// Controles: `model: null` explícito com `{}` passa na conta gerenciada. R2: `model: null` com opções recusa (as
/// opções seriam ignoradas na execução), também no caminho sem conta da principal.
#[tokio::test]
async fn engine_exige_todas_as_chaves_e_modelo_nulo_sem_opcoes() {
    let f = fixture(true);
    let a = extra(&f).session_id().to_owned();
    let before = (rev(&f, &a), engine_of(&f, &a));
    let mut events = f.hub.events.subscribe();
    for key in engine_choice::ENGINE_KEYS {
        let mut engine = managed('a', "model-a");
        engine.as_object_mut().unwrap().remove(key);
        let error = set(&f, &Caller::Local, Some(&a), before.0, engine).await.unwrap_err();
        assert!(code(&error).contains("invalid_field"), "{key}");
        assert_eq!((rev(&f, &a), engine_of(&f, &a)), before, "{key}: nada gravado");
    }
    while let Ok(event) = events.try_recv() {
        assert_ne!(event["type"], "session", "nenhum evento de sessão: {event}");
    }
    // `model: null` com opções: recusa sem gravar.
    let mut options = req(Some(account('a').as_str()), "codex", &Value::Null, false);
    options["modelOptions"] = json!({"effort":"high"});
    let error = set(&f, &Caller::Local, Some(&a), before.0, options).await.unwrap_err();
    assert!(code(&error).contains("engine_unavailable"));
    assert_eq!((rev(&f, &a), engine_of(&f, &a)), before);
    // Controle: `model: null` explícito e `{}` passam.
    let null_model = req(Some(account('a').as_str()), "codex", &Value::Null, false);
    set(&f, &Caller::Local, Some(&a), before.0, null_model).await.unwrap();
    assert_eq!(engine_of(&f, &a).model, None);
    // Caminho sem conta da principal: idem.
    let legacy = fixture(false);
    let mut options = req(None, "codex", &Value::Null, true);
    options["modelOptions"] = json!({"effort":"high"});
    let error = set(&legacy, &Caller::Local, None, rev(&legacy, "dev"), options).await.unwrap_err();
    assert!(code(&error).contains("engine_not_allowed"));
    set(&legacy, &Caller::Local, None, rev(&legacy, "dev"), req(None, "codex", &Value::Null, true)).await.unwrap();
}
