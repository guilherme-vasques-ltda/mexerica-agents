//! D70 do PR4 com os produtores reais: `prepare_bot_turn` → reserva VG → `BindingRef` do #237 → permit IMMEDIATE com
//! `ClaimHooks` de Sessões → `record_dispatch`/`read_checkpoint` do #279. Sem ACP, envio ou Authority fabricados: a
//! ativação/envio não acontecem aqui; o que se prova é a transação do `queued` → `sent`.
use super::{
    admissao::{self, Turn},
    bindings::{self, BindingRef},
    engine_choice::{self, EngineRequest, EngineSource},
    origin::SessionOrigin,
    store::{self, NewSession, SessionRef},
};
use crate::{
    LockExt,
    api::devices::Caller,
    fork::{
        bot_admission::{self as admission, ClaimHooks, NotRoutedReason, Origin, Phase, RoutingClass, Ticket},
        engine_accounts,
        roteamento::{
            admission::{CheckpointKey, Destination},
            receipt,
        },
        spaces::PERSONAL,
    },
    hub::Hub,
    remote::identity::Identity,
    store::{BotConfig, EntryKind, Lane, Store},
};
use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;
use std::sync::Arc;

pub(super) struct F {
    pub(super) hub: Arc<Hub>,
    root: std::path::PathBuf,
}
impl F {
    /// Outro Hub sobre o mesmo banco em disco (só com `fixture_with(_, true)`).
    pub(super) fn reopen(&self) -> Arc<Hub> {
        let store = Store::open(&self.root.join("store.db")).unwrap();
        Hub::new(store, "pr4".into(), Identity::load_or_create(&self.root).unwrap(), "test".into(), 0)
    }
}
impl Drop for F {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub(super) fn account(letter: char) -> String {
    format!("ac_{}", letter.to_string().repeat(64))
}

/// Bot `dev` com Fixo provado (conta gerenciada `b`, `model-b`) ou legado (sem conta).
pub(super) fn fixture(managed: bool) -> F {
    fixture_with(managed, false)
}

/// Como [`fixture`]; `on_disk` grava o banco em `root` para [`F::reopen`] simular o reinício do Hub.
pub(super) fn fixture_with(managed: bool, on_disk: bool) -> F {
    let root = std::env::temp_dir().join(format!("sessoes-admissao-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = if on_disk { root.join("store.db") } else { std::path::PathBuf::from(":memory:") };
    let store = Store::open(&path).unwrap();
    let mut cfg = json!({"id":"dev","name":"dev","backend":"codex","cwd":root,"model":"model-b"});
    if managed {
        cfg["forkEditor"] = json!({"engineAccountId":account('b')});
    }
    store.save_bot(&serde_json::from_value::<BotConfig>(cfg).unwrap()).unwrap();
    store.kv_set(engine_accounts::SLOT, &json!({"salt":uuid::Uuid::new_v4().to_string(), "accounts":[
        {"id":account('a'),"backend":"codex","billing":"paid_api","state":"connected","launch":{"kind":"vault_profile","slot":"PRIVATE_A"},"scope":"owner","quotaIdentity":null,"verifiedAt":1,"subscriptionProof":null,"models":[{"id":"model-a","options":[]}]},
        {"id":account('b'),"backend":"codex","billing":"paid_api","state":"connected","launch":{"kind":"vault_profile","slot":"PRIVATE_B"},"scope":"owner","quotaIdentity":null,"verifiedAt":1,"subscriptionProof":null,"models":[{"id":"model-b","options":[]}]}
    ]}).to_string()).unwrap();
    let hub = Hub::new(store, "pr4".into(), Identity::load_or_create(&root).unwrap(), "test".into(), 0);
    F { hub, root }
}

pub(super) fn extra(f: &F) -> Lane {
    let c = f.hub.store.db.locked();
    let new = NewSession {
        chat: "dev".into(),
        origin: SessionOrigin::Owner,
        source_session: None,
        parent_session: None,
        title: String::new(),
        runs_on: None,
    };
    Lane { chat: "dev".into(), session: Some(store::create(&c, &new).unwrap().id) }
}

pub(super) fn queued(f: &F, lane: &Lane) -> String {
    let turn = f.hub.store.alloc_turn(&lane.chat).unwrap();
    f.hub.store.insert_entry(lane, EntryKind::User, turn, &json!({"text":"pedido","status":"queued"})).unwrap().id
}

/// Configuração de execução como o ator monta num turno novo (`start_prepared_turn`, promove a próxima seleção).
pub(super) fn exec_cfg(f: &F, lane: &Lane) -> Result<BotConfig> {
    let original = f.hub.store.bot("dev")?.unwrap().config;
    let mut cfg = crate::fork::spaces::execution_config(&f.hub.store, &original, &lane.chat)?;
    admissao::start_choice(&f.hub.store.db.locked(), lane, &mut cfg)?;
    Ok(cfg)
}

pub(super) struct Admitted {
    pub(super) ticket: Ticket,
    pub(super) token: String,
}

/// Troca a seleção de UMA conversa pelo escritor canônico de `sessionEngineSet` (sem a guarda RPC).
pub(super) fn set_engine(f: &F, session: &str, account: &str, model: Option<&str>) {
    let mut c = f.hub.store.db.locked();
    let tx = c.transaction().unwrap();
    let revision = store::get(&tx, session).unwrap().unwrap().revision;
    engine_choice::set(&tx, "dev", session, revision, &request(account, model)).unwrap();
    tx.commit().unwrap();
}

pub(super) fn request(account: &str, model: Option<&str>) -> EngineRequest {
    serde_json::from_value(
        json!({"accountId":account,"backend":"codex","model":model,"modelOptions":{},"fastMode":false}),
    )
    .unwrap()
}

/// Um permit real com os ganchos de Sessões. `expected` é a sessão esperada pelo claim da VG (normalmente a da lane).
/// A revisão de captura é a atual (sem troca durante o turno): a execução tem de ser a seleção gravada.
pub(super) fn admit(
    f: &F,
    lane: &Lane,
    ids: &[String],
    cfg: &BotConfig,
    restored: Option<&str>,
    checkpoint: Option<&str>,
    expected: Option<&str>,
) -> (Result<bool>, Admitted, BindingRef) {
    f.hub.prepare_bot_turn("dev", restored, |r| r.lane = Some(lane.clone())).unwrap();
    let token = f.hub.runtime("dev").preparing_turn_token.unwrap();
    let ticket = admission::reserve_for_entries(&f.hub.store, cfg, &lane.chat, &token, &Origin::Human, ids).unwrap();
    let binding =
        bindings::issue(&f.hub.store, "dev", &lane.chat, lane.session_id(), Some(&format!("acp-{token}"))).unwrap();
    let turn = Turn { cfg, lane, entries: ids, token: &token, account_revision: revision(cfg) };
    let mut dispatch = |c: &Connection, original: &BindingRef| admissao::dispatch(c, &turn, original);
    let mut resume = |c: &Connection, raw: &str| admissao::resume(c, &turn, raw);
    let hooks = ClaimHooks {
        binding: Some(&binding),
        dispatch: Some(&mut dispatch),
        resume: Some(&mut resume),
        ..Default::default()
    };
    let result = f.hub.permit_bot_dispatch(cfg, &ticket, checkpoint, ids, expected, hooks);
    (result, Admitted { ticket, token }, binding)
}

/// Revisão que o setup capturaria (`ensure_session`): as contas da fixture não são gerenciadas, então 1.
pub(super) fn revision(cfg: &BotConfig) -> Option<u64> {
    cfg.fork_editor.engine_account_id.as_ref().map(|_| 1)
}

pub(super) fn done(f: &F, a: &Admitted) {
    admission::finish(&f.hub.store, "dev", &a.ticket).unwrap();
}

pub(super) fn checkpoints(f: &F) -> i64 {
    f.hub.store.db.locked().query_row("SELECT COUNT(*) FROM donna_routing_checkpoints", [], |r| r.get(0)).unwrap()
}

pub(super) fn key(lane: &Lane, token: &str) -> CheckpointKey {
    CheckpointKey {
        destination: Destination {
            bot_id: "dev".into(),
            space_id: PERSONAL.into(),
            session: SessionRef { id: lane.session_id().into(), chat_id: lane.chat.clone() },
        },
        execution_token: token.into(),
    }
}

pub(super) fn status(f: &F, id: &str) -> String {
    f.hub.store.entry(id).unwrap().data["status"].as_str().unwrap().to_owned()
}

#[test]
fn principal_admitida_grava_recibo_com_token_e_binding_originais_e_retrato_do_padrao() {
    let f = fixture(true);
    let main = Lane::main("dev");
    let id = queued(&f, &main);
    let cfg = exec_cfg(&f, &main).unwrap();
    let (result, a, binding) = admit(&f, &main, std::slice::from_ref(&id), &cfg, None, None, None);
    assert!(result.unwrap());
    assert_eq!(status(&f, &id), "sent");
    // D305: a classificação nasce no mesmo permit, no registro de admissão.
    assert_eq!(admission::current(&f.hub.store, "dev").unwrap().unwrap().routing, Some(RoutingClass::Routed {}));
    let c = f.hub.store.db.locked();
    let saved = receipt::read_checkpoint(&c, &key(&main, &a.token)).unwrap().unwrap();
    let captured = serde_json::to_value(saved.captured()).unwrap();
    assert_eq!(captured["binding"]["bindingRef"], binding.id.as_str());
    assert_eq!(captured["key"]["executionToken"], a.token);
    assert_eq!(captured["routing"]["admitted"]["accountId"], account('b'));
    assert_eq!(captured["routing"]["decidedBy"], "fixo");
    // D372 (A): a principal com seleção própria grava o carimbo da própria sessão, nunca o do bot.
    assert_eq!(captured["privateStamp"]["source"], "sessionAccountStampV1");
    assert_eq!(captured["routing"]["turnProvenance"]["authorityOrigin"], "requiredFactNotAttested");
    assert_eq!(captured["routing"]["requiredFacts"]["coverageBefore"], "requiredFactNotAttested");
    // D372: a principal tem o retrato do padrão gravado na criação do bot; a admissão não o reescreve.
    let engine = engine_choice::read(&c, "dev").unwrap().unwrap();
    assert_eq!((engine.source, engine.account_id), (EngineSource::BotDefault, Some(account('b'))));
    assert!(f.hub.runtime("dev").memory_turn_token.is_none(), "o permit não ativa; a ativação é do ator pós-commit");
}

#[test]
fn extra_admitida_com_carimbo_da_sessao_usa_retrato_da_criacao_sem_cas_e_fica_congelada() {
    let f = fixture(true);
    let lane = extra(&f);
    // Carimbo próprio que o setup da extra grava (`account.stamp.<bot>/session:<id>`).
    let stamp = json!({"account":account('b'),"revision":1,"backend":"codex"}).to_string();
    f.hub.store.kv_set(&format!("account.stamp.dev/session:{}", lane.session_id()), &stamp).unwrap();
    let id = queued(&f, &lane);
    let cfg = exec_cfg(&f, &lane).unwrap();
    let before = store::get(&f.hub.store.db.locked(), lane.session_id()).unwrap().unwrap().revision;
    let (result, a, _) = admit(&f, &lane, std::slice::from_ref(&id), &cfg, None, None, lane.session.as_deref());
    assert!(result.unwrap());
    assert_eq!(status(&f, &id), "sent");
    assert_eq!(admission::current(&f.hub.store, "dev").unwrap().unwrap().routing, Some(RoutingClass::Routed {}));
    let c = f.hub.store.db.locked();
    let saved = receipt::read_checkpoint(&c, &key(&lane, &a.token)).unwrap().unwrap();
    let captured = serde_json::to_value(saved.captured()).unwrap();
    assert_eq!(captured["privateStamp"]["source"], "sessionAccountStampV1", "nunca o carimbo do bot na extra");
    assert_eq!(captured["privateStamp"]["capture"]["value"], stamp);
    assert_eq!(captured["routing"]["reason"], "frozenSessionSelection");
    let choice = engine_choice::read(&c, lane.session_id()).unwrap().unwrap();
    assert_eq!((choice.account_id, choice.model.as_deref()), (Some(account('b')), Some("model-b")));
    // D372: a seleção nasceu no `sessionCreate`; o despacho não grava CAS nem muda a revisão.
    assert_eq!(captured["intent"]["expectedSessionRevision"], before);
    assert_eq!(store::get(&c, lane.session_id()).unwrap().unwrap().revision, before);
    drop(c);
    done(&f, &a);
    // O dono troca o padrão do bot: a extra continua na própria seleção, sem novo CAS.
    let mut bot = f.hub.store.bot("dev").unwrap().unwrap().config;
    bot.model = Some("model-a".into());
    bot.fork_editor.engine_account_id = Some(account('a'));
    f.hub.store.save_bot(&bot).unwrap();
    let frozen = exec_cfg(&f, &lane).unwrap();
    assert_eq!(frozen.model.as_deref(), Some("model-b"));
    let fixed = store::get(&f.hub.store.db.locked(), lane.session_id()).unwrap().unwrap().revision;
    let id = queued(&f, &lane);
    let (result, a, _) = admit(&f, &lane, std::slice::from_ref(&id), &frozen, None, None, lane.session.as_deref());
    assert!(result.unwrap());
    let c = f.hub.store.db.locked();
    let saved = receipt::read_checkpoint(&c, &key(&lane, &a.token)).unwrap().unwrap();
    assert_eq!(serde_json::to_value(saved.captured()).unwrap()["routing"]["reason"], "frozenSessionSelection");
    assert_eq!(store::get(&c, lane.session_id()).unwrap().unwrap().revision, fixed, "sem novo CAS");
    drop(c);
    done(&f, &a);
}

#[test]
fn execucao_fora_da_selecao_gravada_sem_troca_falha_e_extra_sem_modelo_nao_segue_sem_recibo() {
    let f = fixture(true);
    let lane = extra(&f);
    // Um setup com a seleção da extra e outro fora dela, sem troca da conversa entre captura e encaixe.
    let first = exec_cfg(&f, &lane).unwrap();
    let mut loser = first.clone();
    loser.model = Some("model-a".into());
    loser.fork_editor.engine_account_id = Some(account('a'));
    let id = queued(&f, &lane);
    let (result, a, _) = admit(&f, &lane, std::slice::from_ref(&id), &first, None, None, lane.session.as_deref());
    assert!(result.unwrap());
    done(&f, &a);
    let revision = store::get(&f.hub.store.db.locked(), lane.session_id()).unwrap().unwrap().revision;
    let before = checkpoints(&f);
    let id = queued(&f, &lane);
    let (result, a, _) = admit(&f, &lane, std::slice::from_ref(&id), &loser, None, None, lane.session.as_deref());
    assert!(format!("{:#}", result.unwrap_err()).contains("engine_choice_execution_mismatch"));
    assert_eq!((checkpoints(&f), status(&f, &id).as_str()), (before, "queued"));
    let c = f.hub.store.db.locked();
    assert_eq!(store::get(&c, lane.session_id()).unwrap().unwrap().revision, revision);
    assert_eq!(engine_choice::read(&c, lane.session_id()).unwrap().unwrap().account_id, Some(account('b')));
    drop(c);
    assert!(f.hub.runtime("dev").memory_turn_token.is_none(), "nada ativado");
    done(&f, &a);
    // Extra sem modelo explícito segue roteada, com a ausência real registrada no recibo.
    let lane = extra(&f);
    set_engine(&f, lane.session_id(), &account('b'), None);
    let no_model = exec_cfg(&f, &lane).unwrap();
    assert!(no_model.model.is_none(), "nada é inventado a partir do catálogo");
    let id = queued(&f, &lane);
    let (result, a, _) = admit(&f, &lane, std::slice::from_ref(&id), &no_model, None, None, lane.session.as_deref());
    assert!(result.unwrap());
    assert_eq!(status(&f, &id), "sent");
    assert_eq!(admission::current(&f.hub.store, "dev").unwrap().unwrap().routing, Some(RoutingClass::Routed {}));
    let c = f.hub.store.db.locked();
    let saved = receipt::read_checkpoint(&c, &key(&lane, &a.token)).unwrap().unwrap();
    assert!(saved.captured().routing.admitted.selection.model.is_none());
    drop(c);
    assert!(engine_choice::read(&f.hub.store.db.locked(), lane.session_id()).unwrap().unwrap().model.is_none());
    done(&f, &a);
}

#[test]
fn falha_sql_depois_do_recibo_da_extra_desfaz_tudo() {
    let f = fixture(true);
    let lane = extra(&f);
    let id = queued(&f, &lane);
    let cfg = exec_cfg(&f, &lane).unwrap();
    let revision = store::get(&f.hub.store.db.locked(), lane.session_id()).unwrap().unwrap().revision;
    // O claim da VG espera a principal: falha DEPOIS do recibo, na mesma transação.
    let (result, a, _) = admit(&f, &lane, std::slice::from_ref(&id), &cfg, None, None, None);
    assert!(result.is_err());
    // `checkpoints` pega o mesmo mutex do banco: contar antes de segurar o guard (não é reentrante).
    assert_eq!(checkpoints(&f), 0, "recibo desfeito");
    let c = f.hub.store.db.locked();
    assert_eq!(
        engine_choice::read(&c, lane.session_id()).unwrap().unwrap().source,
        EngineSource::BotDefault,
        "seleção intacta"
    );
    assert_eq!(store::get(&c, lane.session_id()).unwrap().unwrap().revision, revision);
    drop(c);
    assert_eq!(status(&f, &id), "queued");
    let record = admission::current(&f.hub.store, "dev").unwrap().unwrap();
    assert_eq!((record.phase, record.routing), (Phase::Reserved, None));
    assert!(f.hub.runtime("dev").memory_turn_token.is_none());
    done(&f, &a);
}

#[test]
fn falha_sql_depois_do_recibo_desfaz_tudo_sem_ativar() {
    let f = fixture(true);
    let main = Lane::main("dev");
    let id = queued(&f, &main);
    let cfg = exec_cfg(&f, &main).unwrap();
    // O claim da VG espera outra sessão: falha DEPOIS do recibo, na mesma transação.
    let other = extra(&f);
    let (result, a, _) = admit(&f, &main, std::slice::from_ref(&id), &cfg, None, None, Some(other.session_id()));
    assert!(result.is_err());
    assert_eq!(checkpoints(&f), 0, "recibo desfeito");
    assert_eq!(status(&f, &id), "queued");
    let record = admission::current(&f.hub.store, "dev").unwrap().unwrap();
    assert_eq!((record.phase, record.routing), (Phase::Reserved, None), "classificação desfeita junto");
    let runtime = f.hub.runtime("dev");
    assert!(runtime.memory_turn_token.is_none());
    assert_eq!(runtime.preparing_turn_token.as_deref(), Some(a.token.as_str()));
    done(&f, &a);
}

#[test]
fn edicao_entre_setup_e_encaixe_grava_o_snapshot_executado_e_revogacao_falha_fechado() {
    let f = fixture(true);
    let main = Lane::main("dev");
    let id = queued(&f, &main);
    // Snapshot do setup (conta b), carimbo bruto já gravado pelo setup.
    let cfg = exec_cfg(&f, &main).unwrap();
    let stamp = json!({"account":account('b'),"revision":1,"backend":"codex"}).to_string();
    // D372 (A): o carimbo da principal com seleção própria é o da sessão dela.
    f.hub.store.kv_set("account.stamp.dev/session:dev", &stamp).unwrap();
    // O dono edita o padrão depois do setup: o turno admitido continua o do snapshot; nada sobrescreve o carimbo.
    let mut bot = f.hub.store.bot("dev").unwrap().unwrap().config;
    bot.model = Some("model-a".into());
    bot.fork_editor.engine_account_id = Some(account('a'));
    f.hub.store.save_bot(&bot).unwrap();
    let (result, a, _) = admit(&f, &main, std::slice::from_ref(&id), &cfg, None, None, None);
    assert!(result.unwrap());
    let c = f.hub.store.db.locked();
    let saved = receipt::read_checkpoint(&c, &key(&main, &a.token)).unwrap().unwrap();
    let captured = serde_json::to_value(saved.captured()).unwrap();
    assert_eq!(captured["routing"]["admitted"]["accountId"], account('b'));
    assert_eq!(captured["privateStamp"]["capture"]["value"], stamp);
    drop(c);
    assert_eq!(f.hub.store.kv_read("account.stamp.dev/session:dev").unwrap().as_deref(), Some(stamp.as_str()));
    done(&f, &a);
    // A conta do snapshot é retirada do cadastro entre o setup e o encaixe: falha fechado, sem recibo nem claim.
    let id = queued(&f, &main);
    let before = checkpoints(&f);
    let registry = json!({"salt":uuid::Uuid::new_v4().to_string(), "accounts":[
        {"id":account('a'),"backend":"codex","billing":"paid_api","state":"connected","launch":{"kind":"vault_profile","slot":"PRIVATE_A"},"scope":"owner","quotaIdentity":null,"verifiedAt":1,"subscriptionProof":null,"models":[{"id":"model-a","options":[]}]}
    ]});
    f.hub.store.kv_set(engine_accounts::SLOT, &registry.to_string()).unwrap();
    let (result, a, _) = admit(&f, &main, std::slice::from_ref(&id), &cfg, None, None, None);
    assert!(result.is_err());
    assert_eq!((checkpoints(&f), status(&f, &id).as_str()), (before, "queued"));
    assert!(f.hub.runtime("dev").memory_turn_token.is_none());
    done(&f, &a);
}

#[test]
fn principal_legada_segue_sem_roteamento_e_selecao_ilegivel_na_principal_falha_fechado() {
    let f = fixture(false);
    let main = Lane::main("dev");
    let id = queued(&f, &main);
    let cfg = exec_cfg(&f, &main).unwrap();
    let (result, a, _) = admit(&f, &main, std::slice::from_ref(&id), &cfg, None, None, None);
    assert!(result.unwrap());
    assert_eq!((checkpoints(&f), status(&f, &id).as_str()), (0, "sent"));
    assert_eq!(
        admission::current(&f.hub.store, "dev").unwrap().unwrap().routing,
        Some(RoutingClass::NotRouted { reason: NotRoutedReason::UnprovenSelection })
    );
    done(&f, &a);
    // Retomada D72 desse turno legado: segue o caminho da VG, sem Roteamento e sem recibo novo.
    let waiting = json!({"phase":"waiting","executionToken":a.token,
        "routing":{"kind":"notRouted","reason":"unprovenSelection"},"input":{"chat":"dev","entries":[id]}});
    f.hub.store.kv_set("turn.waiting.dev", &waiting.to_string()).unwrap();
    let mut retry = waiting.clone();
    retry["phase"] = json!("retrying");
    let ids = [id.clone()];
    let (result, resumed, _) = admit(&f, &main, &ids, &cfg, Some(&a.token), Some(&retry.to_string()), None);
    assert!(result.unwrap());
    assert_eq!(checkpoints(&f), 0);
    assert_eq!(
        admission::current(&f.hub.store, "dev").unwrap().unwrap().routing,
        Some(RoutingClass::NotRouted { reason: NotRoutedReason::UnprovenSelection }),
        "a retomada conserva a classificação original"
    );
    done(&f, &resumed);
    // Espera sem classificação (gravada depois da migração): falha fechado, sem claim nem permissão.
    let mut missing = retry.clone();
    missing.as_object_mut().unwrap().remove("routing");
    let (result, x, _) = admit(&f, &main, &ids, &cfg, Some(&a.token), Some(&missing.to_string()), None);
    assert!(format!("{:#}", result.unwrap_err()).contains("admission_routing_missing"));
    assert_eq!(admission::current(&f.hub.store, "dev").unwrap().unwrap().phase, Phase::Reserved);
    done(&f, &x);
    let raw = json!({"schemaVersion":1,"accountId":account('a'),"selection":{"backend":"codex","model":"model-a","modelOptions":{}}});
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE donna_bot_sessions SET engine_choice=?1 WHERE id='dev'", [raw.to_string()])
        .unwrap();
    // O formato v1 já foi migrado: gravado de novo na principal, é ilegível e falha fechado, sem cair no padrão.
    let id = queued(&f, &main);
    let (result, a, _) = admit(&f, &main, std::slice::from_ref(&id), &cfg, None, None, None);
    assert!(format!("{:#}", result.unwrap_err()).contains("engine_choice_invalid"));
    assert_eq!(status(&f, &id), "queued");
    done(&f, &a);
}

#[test]
fn sessao_encerrada_e_vinculo_revogado_falham_fechado() {
    let f = fixture(true);
    let main = Lane::main("dev");
    let cfg = exec_cfg(&f, &main).unwrap();
    // Vínculo original revogado depois do setup: o encaixe não adota outro.
    let id = queued(&f, &main);
    f.hub.prepare_bot_turn("dev", None, |r| r.lane = Some(main.clone())).unwrap();
    let token = f.hub.runtime("dev").preparing_turn_token.unwrap();
    let ids = [id.clone()];
    let ticket = admission::reserve_for_entries(&f.hub.store, &cfg, "dev", &token, &Origin::Human, &ids).unwrap();
    let binding = bindings::issue(&f.hub.store, "dev", "dev", "dev", Some("acp-revogada")).unwrap();
    bindings::revoke(&f.hub.store, "dev", None).unwrap();
    let turn = Turn { cfg: &cfg, lane: &main, entries: &ids, token: &token, account_revision: revision(&cfg) };
    let mut dispatch = |c: &Connection, original: &BindingRef| admissao::dispatch(c, &turn, original);
    let hooks =
        ClaimHooks { binding: Some(&binding), dispatch: Some(&mut dispatch), resume: None, ..Default::default() };
    assert!(f.hub.permit_bot_dispatch(&cfg, &ticket, None, &ids, None, hooks).is_err());
    assert_eq!((checkpoints(&f), status(&f, &id).as_str()), (0, "queued"));
    admission::finish(&f.hub.store, "dev", &ticket).unwrap();
    // Extra encerrada depois do setup (o vínculo nasce com a sessão aberta e o encerramento o revoga): recusa antes
    // do recibo, sem efeito.
    let lane = extra(&f);
    let id = queued(&f, &lane);
    let ids = [id.clone()];
    f.hub.prepare_bot_turn("dev", None, |r| r.lane = Some(lane.clone())).unwrap();
    let token = f.hub.runtime("dev").preparing_turn_token.unwrap();
    let ticket = admission::reserve_for_entries(&f.hub.store, &cfg, "dev", &token, &Origin::Human, &ids).unwrap();
    let binding = bindings::issue(&f.hub.store, "dev", "dev", lane.session_id(), Some("acp-encerrada")).unwrap();
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE donna_bot_sessions SET state='closed' WHERE id=?1", [lane.session_id()])
        .unwrap();
    let turn = Turn { cfg: &cfg, lane: &lane, entries: &ids, token: &token, account_revision: revision(&cfg) };
    let mut dispatch = |c: &Connection, original: &BindingRef| admissao::dispatch(c, &turn, original);
    let hooks =
        ClaimHooks { binding: Some(&binding), dispatch: Some(&mut dispatch), resume: None, ..Default::default() };
    assert!(f.hub.permit_bot_dispatch(&cfg, &ticket, None, &ids, lane.session.as_deref(), hooks).is_err());
    assert_eq!((checkpoints(&f), status(&f, &id).as_str()), (0, "queued"));
    let source = engine_choice::read(&f.hub.store.db.locked(), lane.session_id()).unwrap().unwrap().source;
    assert_eq!(source, EngineSource::BotDefault);
    admission::finish(&f.hub.store, "dev", &ticket).unwrap();
}

#[test]
fn resume_le_checkpoint_original_sem_novo_recibo_e_qualquer_erro_de_leitura_falha_fechado() {
    let f = fixture(true);
    let main = Lane::main("dev");
    let id = queued(&f, &main);
    let cfg = exec_cfg(&f, &main).unwrap();
    let ids = [id.clone()];
    let (result, a, _) = admit(&f, &main, &ids, &cfg, None, None, None);
    assert!(result.unwrap());
    done(&f, &a);
    // Retomada D72 com o MESMO token restaurado: só lê o checkpoint; nada é regravado nem escolhido.
    let waiting = json!({"phase":"waiting","executionToken":a.token,"routing":{"kind":"routed"},
        "input":{"chat":"dev","entries":[id]}});
    f.hub.store.kv_set("turn.waiting.dev", &waiting.to_string()).unwrap();
    let mut retry = waiting.clone();
    retry["phase"] = json!("retrying");
    let (result, resumed, _) = admit(&f, &main, &ids, &cfg, Some(&a.token), Some(&retry.to_string()), None);
    assert!(result.unwrap());
    assert_eq!(resumed.token, a.token);
    assert_eq!(checkpoints(&f), 1, "sem recibo inicial novo");
    assert_eq!(engine_choice::read(&f.hub.store.db.locked(), "dev").unwrap().unwrap().source, EngineSource::BotDefault);
    done(&f, &resumed);
    // Execução divergente da admitida: falha fechado, sem recalcular.
    let wrong = {
        let mut cfg = cfg.clone();
        cfg.model = Some("model-a".into());
        cfg.fork_editor.engine_account_id = Some(account('a'));
        cfg
    };
    let (result, x, _) = admit(&f, &main, &ids, &wrong, Some(&a.token), Some(&retry.to_string()), None);
    assert!(format!("{:#}", result.unwrap_err()).contains("engine_choice_execution_mismatch"));
    done(&f, &x);
    // Checkpoint corrompido: também na principal falha fechado (não vira legado).
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE donna_routing_checkpoints SET checkpoint_json='{}' WHERE execution_token=?1", [&a.token])
        .unwrap();
    let (result, x, _) = admit(&f, &main, &ids, &cfg, Some(&a.token), Some(&retry.to_string()), None);
    assert!(result.is_err());
    done(&f, &x);
    // Sem checkpoint (outro token): falha fechado, sem recibo novo.
    let other = json!({"phase":"retrying","executionToken":"outro-token","routing":{"kind":"routed"},
        "input":{"chat":"dev","entries":[id]}});
    let (result, x, _) = admit(&f, &main, &ids, &cfg, Some("outro-token"), Some(&other.to_string()), None);
    assert!(result.is_err());
    assert_eq!(checkpoints(&f), 1);
    done(&f, &x);
}

#[tokio::test]
async fn principal_de_space_usa_a_conversa_sem_selecao_pessoal() {
    let f = fixture(true);
    let created = crate::api::dispatch(
        &f.hub,
        &Caller::Local,
        "donnaConversationCreate",
        json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"botId":"dev","title":"Fixture"}),
    )
    .await
    .unwrap();
    let chat = created["conversation"]["id"].as_str().unwrap().to_owned();
    let selection = json!({"backend":"codex","model":"model-a","modelOptions":{}}).to_string();
    f.hub
        .store
        .db
        .locked()
        .execute(
            "UPDATE donna_space_conversations SET engine_account_id=?2, engine_selection=?3 WHERE id=?1",
            [chat.as_str(), account('a').as_str(), selection.as_str()],
        )
        .unwrap();
    let lane = Lane::main(&chat);
    let id = queued(&f, &lane);
    let cfg = exec_cfg(&f, &lane).unwrap();
    assert!(cfg.context.is_some());
    // Fonte única da principal de Space: a conversa (conta a), não o Fixo do bot (conta b).
    assert_eq!(cfg.fork_editor.engine_account_id.as_deref(), Some(account('a').as_str()));
    let binding = bindings::issue(&f.hub.store, "dev", &chat, &chat, Some("acp-space")).unwrap();
    let turn = Turn {
        cfg: &cfg,
        lane: &lane,
        entries: std::slice::from_ref(&id),
        token: "token-space",
        account_revision: revision(&cfg),
    };
    let mut db = f.hub.store.db.locked();
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).unwrap();
    assert_eq!(admissao::dispatch(&tx, &turn, &binding).unwrap(), RoutingClass::Routed {});
    tx.commit().unwrap();
    assert!(engine_choice::read(&db, &chat).unwrap().is_none(), "principal não ganha escolha própria");
    let saved = receipt::read_checkpoint(&db, &key(&lane, "token-space")).unwrap().unwrap();
    let captured = serde_json::to_value(saved.captured()).unwrap();
    assert_eq!(captured["privateStamp"]["source"], "spaceEngineStampV1");
    assert_eq!(captured["routing"]["admitted"]["accountId"], account('a'));
}

#[test]
fn principal_com_conta_gerenciada_quebrada_falha_fechado_e_so_login_legado_vira_nao_roteado() {
    // Conta `ac_` capturada, mas retirada do cadastro: falha fechado, sem rebaixar para legado.
    let f = fixture(true);
    let main = Lane::main("dev");
    let cfg = exec_cfg(&f, &main).unwrap();
    let registry = json!({"salt":uuid::Uuid::new_v4().to_string(), "accounts":[]});
    f.hub.store.kv_set(engine_accounts::SLOT, &registry.to_string()).unwrap();
    let id = queued(&f, &main);
    let (result, a, _) = admit(&f, &main, std::slice::from_ref(&id), &cfg, None, None, None);
    assert!(result.is_err());
    assert_eq!((checkpoints(&f), status(&f, &id).as_str()), (0, "queued"));
    assert_eq!(admission::current(&f.hub.store, "dev").unwrap().unwrap().routing, None);
    done(&f, &a);
    // Cadastro inteiro ausente com conta `ac_` capturada: também falha fechado.
    f.hub.store.kv_delete(engine_accounts::SLOT).unwrap();
    let id = queued(&f, &main);
    let (result, a, _) = admit(&f, &main, std::slice::from_ref(&id), &cfg, None, None, None);
    assert!(format!("{:#}", result.unwrap_err()).contains("routing_selection_unavailable"));
    assert_eq!(status(&f, &id), "queued");
    done(&f, &a);

    // Comando próprio com conta gerenciada: configuração recusada pelo produtor de contas; falha fechado.
    let mut with_command = cfg.clone();
    with_command.command = Some("agente-proprio".into());
    let f2 = fixture(true);
    let id = queued(&f2, &main);
    let (result, a, _) = admit(&f2, &main, std::slice::from_ref(&id), &with_command, None, None, None);
    assert!(format!("{:#}", result.unwrap_err()).contains("routing_selection_unavailable"));
    assert_eq!(status(&f2, &id), "queued");
    done(&f2, &a);

    // Conta válida sem modelo explícito: mapeada à parte como configuração legada (pendente de Modelos). D372: a
    // principal muda pela própria seleção, não pelo padrão do bot.
    let f = fixture(true);
    set_engine(&f, "dev", &account('b'), None);
    let main = Lane::main("dev");
    let cfg = exec_cfg(&f, &main).unwrap();
    let id = queued(&f, &main);
    let (result, a, _) = admit(&f, &main, std::slice::from_ref(&id), &cfg, None, None, None);
    assert!(result.unwrap());
    assert_eq!(
        admission::current(&f.hub.store, "dev").unwrap().unwrap().routing,
        Some(RoutingClass::NotRouted { reason: NotRoutedReason::UnprovenSelection })
    );
    assert_eq!(checkpoints(&f), 0);
    done(&f, &a);
}

/// O `send` real: extra sem seleção roteável é recusada antes de gravar e nada chega ao ator; a principal e uma
/// extra válida seguem para a fila. A admissão do permit continua obrigatória depois disso (testes acima).
#[tokio::test]
async fn send_recusa_extra_nao_roteavel_sem_entrada_e_aceita_principal_e_extra_valida() {
    let send = |f: &F, body: serde_json::Value| {
        let hub = f.hub.clone();
        async move { crate::api::dispatch(&hub, &Caller::Local, "send", body).await }
    };
    // Bot sem conta gerenciada: extra recusada, principal aceita.
    let f = fixture(false);
    let mut commands = f.hub.test_bot_commands("dev");
    let lane = extra(&f);
    let before = f.hub.store.last_seq("dev");
    let error = send(&f, json!({"botId":"dev","text":"oi","sessionId":lane.session_id()})).await.unwrap_err();
    assert!(format!("{error:#}").contains("session_selection_unavailable"), "{error:#}");
    assert_eq!(f.hub.store.last_seq("dev"), before, "nenhuma entrada gravada");
    assert!(commands.try_recv().is_err(), "nada chega à fila do ator");
    assert!(admission::current(&f.hub.store, "dev").unwrap().is_none(), "nenhuma admissão pendente");
    send(&f, json!({"botId":"dev","text":"principal"})).await.unwrap();
    match commands.try_recv().unwrap() {
        crate::agent::bot::Cmd::Send { lane, .. } => assert_eq!(lane, Lane::main("dev")),
        _ => panic!("esperava o envio da principal"),
    }
    // Conta gerenciada com modelo: a extra entra na fila da própria sessão.
    let f = fixture(true);
    let mut commands = f.hub.test_bot_commands("dev");
    let lane = extra(&f);
    let response =
        send(&f, json!({"botId":"dev","text":"pedido da extra","sessionId":lane.session_id()})).await.unwrap();
    assert_eq!(response["entry"]["sessionId"], lane.session_id());
    assert_eq!(response["entry"]["data"]["status"], "queued");
    match commands.try_recv().unwrap() {
        crate::agent::bot::Cmd::Send { lane: queued, .. } => assert_eq!(queued, lane),
        _ => panic!("esperava o envio da extra"),
    }
    // Padrão implícito: a RPC cria o retrato do BOT, sem copiar model-b da principal.
    let mut f = fixture_with(true, true);
    let mut registry: serde_json::Value =
        serde_json::from_str(&f.hub.store.kv_read(engine_accounts::SLOT).unwrap().unwrap()).unwrap();
    registry["accounts"][1]["managed"] = json!({"label":"Sintética B","revision":1,"generation":3,
        "enabled":true,"requestId":"fixture-padrao","principal":"fixture-b"});
    f.hub.store.kv_set(engine_accounts::SLOT, &registry.to_string()).unwrap();
    let mut commands = f.hub.test_bot_commands("dev");
    let mut bot = f.hub.store.bot("dev").unwrap().unwrap().config;
    bot.model = None;
    f.hub.store.save_bot(&bot).unwrap();
    let created = crate::api::dispatch(&f.hub, &Caller::Local, "sessionCreate", json!({"botId":"dev"})).await.unwrap();
    let other = Lane { chat: "dev".into(), session: Some(created["session"]["id"].as_str().unwrap().into()) };
    assert_eq!(created["session"]["engine"]["accountId"], account('b'));
    assert_eq!(created["session"]["engine"]["model"], serde_json::Value::Null);
    let created_revision = created["session"]["revision"].as_i64().unwrap();
    assert_eq!(exec_cfg(&f, &Lane::main("dev")).unwrap().model.as_deref(), Some("model-b"));
    // Editar o padrão depois não muda a conta nem o modelo já capturados pela extra (D372).
    bot.model = Some("model-a".into());
    bot.fork_editor.engine_account_id = Some(account('a'));
    f.hub.store.save_bot(&bot).unwrap();
    let response = send(&f, json!({"botId":"dev","text":"oi","sessionId":other.session_id()})).await.unwrap();
    assert_eq!(response["entry"]["sessionId"], other.session_id());
    let id = response["entry"]["id"].as_str().unwrap().to_owned();
    assert_eq!(status(&f, &id), "queued");
    // O primeiro envio dá título à extra e avança sua revisão antes do permit.
    let titled = store::get(&f.hub.store.db.locked(), other.session_id()).unwrap().unwrap();
    assert_eq!(titled.title, "oi");
    assert_eq!(titled.title_source, "first_message");
    assert!(titled.revision > created_revision);
    let session_revision = titled.revision;
    match commands.try_recv().unwrap() {
        crate::agent::bot::Cmd::Send { lane, .. } => assert_eq!(lane, other),
        _ => panic!("esperava o envio da extra com padrão implícito"),
    }
    let cfg = exec_cfg(&f, &other).unwrap();
    assert!(cfg.model.is_none());
    assert_eq!(cfg.fork_editor.engine_account_id, Some(account('b')));
    let stamp = json!({"account":account('b'),"revision":1,"backend":"codex"}).to_string();
    f.hub.store.kv_set(&format!("account.stamp.dev/session:{}", other.session_id()), &stamp).unwrap();
    let ids = [id.clone()];
    let (result, a, binding) = admit(&f, &other, &ids, &cfg, None, None, other.session.as_deref());
    assert!(result.unwrap());
    assert_eq!(status(&f, &id), "sent");
    assert_eq!(admission::current(&f.hub.store, "dev").unwrap().unwrap().routing, Some(RoutingClass::Routed {}));
    let captured = {
        let c = f.hub.store.db.locked();
        let saved = receipt::read_checkpoint(&c, &key(&other, &a.token)).unwrap().unwrap();
        let captured = serde_json::to_value(saved.captured()).unwrap();
        for selection in ["requested", "admitted"] {
            assert_eq!(captured["routing"][selection]["accountId"], account('b'));
            assert_eq!(captured["routing"][selection]["selection"]["model"], serde_json::Value::Null);
        }
        assert_eq!(captured["key"]["executionToken"], a.token);
        assert_eq!(captured["key"]["destination"]["session"]["id"], other.session_id());
        assert_eq!(captured["binding"]["bindingRef"], binding.id.as_str());
        assert_eq!(captured["privateStamp"]["source"], "sessionAccountStampV1");
        assert_eq!(captured["privateStamp"]["capture"]["value"], stamp);
        assert_eq!(captured["intent"]["expectedSessionRevision"], session_revision);
        let public = receipt::entry_receipt(
            &c,
            &receipt::ReceiptQuery {
                bot: "dev",
                space: PERSONAL,
                conversation: "dev",
                session: other.session_id(),
                entry: &id,
            },
        )
        .unwrap();
        assert_eq!(public["state"], "routed");
        assert_eq!(public["receipt"]["admitted"]["selection"]["model"], serde_json::Value::Null);
        captured
    };
    done(&f, &a);
    // Persistência e retomada real pelo permit: mesma seleção nula, token e recibo, sem nova decisão.
    let waiting = json!({"phase":"waiting","executionToken":a.token,"routing":{"kind":"routed"},
        "input":{"chat":"dev","visibleSession":other.session_id(),"entries":[id]}});
    f.hub.store.kv_set("turn.waiting.dev", &waiting.to_string()).unwrap();
    f.hub = f.reopen();
    let mut restored = f.hub.store.bot("dev").unwrap().unwrap().config;
    admissao::apply_resume(&f.hub.store.db.locked(), &other, &mut restored, Some(&a.token), true, ("codex", None))
        .unwrap();
    assert!(restored.model.is_none());
    assert_eq!(restored.fork_editor.engine_account_id, Some(account('b')));
    let mut retry = waiting;
    retry["phase"] = json!("retrying");
    let (result, resumed, _) =
        admit(&f, &other, &ids, &restored, Some(&a.token), Some(&retry.to_string()), other.session.as_deref());
    assert!(result.unwrap());
    assert_eq!(resumed.token, a.token);
    assert_eq!(checkpoints(&f), 1);
    assert_eq!(admission::current(&f.hub.store, "dev").unwrap().unwrap().routing, Some(RoutingClass::Routed {}));
    {
        let c = f.hub.store.db.locked();
        let saved = receipt::read_checkpoint(&c, &key(&other, &a.token)).unwrap().unwrap();
        assert_eq!(serde_json::to_value(saved.captured()).unwrap(), captured);
        assert_eq!(store::get(&c, other.session_id()).unwrap().unwrap().revision, session_revision);
        assert!(engine_choice::read(&c, other.session_id()).unwrap().unwrap().model.is_none());
    }
    done(&f, &resumed);
    // Um modelo explícito não é equivalente ao padrão nulo admitido, mesmo na mesma conta.
    let mut wrong = restored.clone();
    wrong.model = Some("model-b".into());
    let (result, mismatch, _) =
        admit(&f, &other, &ids, &wrong, Some(&a.token), Some(&retry.to_string()), other.session.as_deref());
    assert!(format!("{:#}", result.unwrap_err()).contains("engine_choice_execution_mismatch"));
    assert_eq!(checkpoints(&f), 1);
    done(&f, &mismatch);
    // Retomada sem o checkpoint original continua recusada, sem criar outro recibo.
    f.hub.store.db.locked().execute("DELETE FROM donna_routing_checkpoints", []).unwrap();
    let (result, missing, _) =
        admit(&f, &other, &ids, &restored, Some(&a.token), Some(&retry.to_string()), other.session.as_deref());
    assert!(result.is_err());
    assert_eq!(checkpoints(&f), 0);
    done(&f, &missing);
}

/// R1 (DON-833) + D372: trocar o padrão (conta/motor) do bot não derruba vínculos de conversas com seleção própria
/// (principal e extras do Pessoal, inclusive extra ainda sem envio). A ativação (ACP reaberta) segue a seleção
/// capturada pelo turno e confere a conta no cadastro; conta retirada, motor incompatível ou seleção ilegível recusam.
#[test]
fn padrao_do_bot_nao_revoga_vinculos_das_conversas_e_conta_retirada_recusa() {
    let f = fixture(true);
    let fixed = extra(&f);
    let id = queued(&f, &fixed);
    let cfg_fixed = exec_cfg(&f, &fixed).unwrap();
    let (result, a, _) = admit(&f, &fixed, std::slice::from_ref(&id), &cfg_fixed, None, None, fixed.session.as_deref());
    assert!(result.unwrap());
    done(&f, &a);
    let unsent = extra(&f);
    let main = Lane::main("dev");
    let issue =
        |lane: &Lane, acp: &str| bindings::issue(&f.hub.store, "dev", "dev", lane.session_id(), Some(acp)).unwrap();
    let b_fixed = issue(&fixed, "acp-extra");
    bindings::activate_for_config(&f.hub.store, &b_fixed, "acp-extra", &cfg_fixed).unwrap();
    let b_main = issue(&main, "acp-main");
    bindings::activate_for_config(&f.hub.store, &b_main, "acp-main", &exec_cfg(&f, &main).unwrap()).unwrap();
    let b_unsent = issue(&unsent, "acp-nula");
    // O dono troca o padrão do bot para a conta A (a conta B das conversas continua válida).
    let mut bot = f.hub.store.bot("dev").unwrap().unwrap().config;
    bot.model = Some("model-a".into());
    bot.fork_editor.engine_account_id = Some(account('a'));
    f.hub.store.save_bot(&bot).unwrap();
    // Nenhum vínculo das conversas abertas é revogado por mero efeito do padrão.
    assert_eq!(bindings::for_acp(&f.hub.store, "dev", "acp-extra").unwrap().id, b_fixed.id);
    assert_eq!(bindings::for_acp(&f.hub.store, "dev", "acp-main").unwrap().id, b_main.id);
    assert!(bindings::resolve(&f.hub.store.db.locked(), b_unsent.id.as_str(), "dev").is_ok());
    // Principal e extras continuam em B; nunca adotam o padrão novo.
    for lane in [&main, &fixed, &unsent] {
        let cfg = exec_cfg(&f, lane).unwrap();
        assert_eq!(
            (cfg.fork_editor.engine_account_id.as_deref(), cfg.model.as_deref()),
            (Some(account('b').as_str()), Some("model-b"))
        );
    }
    let frozen = exec_cfg(&f, &fixed).unwrap();
    let reopened = issue(&fixed, "acp-extra-2");
    bindings::activate_for_config(&f.hub.store, &reopened, "acp-extra-2", &frozen).unwrap();
    let mut incompatible = frozen.clone();
    incompatible.backend = "claude".into();
    let wrong = issue(&fixed, "acp-extra-3");
    assert!(bindings::activate_for_config(&f.hub.store, &wrong, "acp-extra-3", &incompatible).is_err());
    let new_main = issue(&main, "acp-main-2");
    bindings::activate_for_config(&f.hub.store, &new_main, "acp-main-2", &exec_cfg(&f, &main).unwrap()).unwrap();
    // A extra continua sendo admitida em B depois da troca do padrão.
    let id = queued(&f, &fixed);
    let (result, a, _) = admit(&f, &fixed, std::slice::from_ref(&id), &frozen, None, None, fixed.session.as_deref());
    assert!(result.unwrap());
    done(&f, &a);
    // Conta B retirada do cadastro: a ativação recusa (revogação real continua valendo).
    let registry = json!({"salt":uuid::Uuid::new_v4().to_string(), "accounts":[
        {"id":account('a'),"backend":"codex","billing":"paid_api","state":"connected","launch":{"kind":"vault_profile","slot":"PRIVATE_A"},"scope":"owner","quotaIdentity":null,"verifiedAt":1,"subscriptionProof":null,"models":[{"id":"model-a","options":[]}]}
    ]});
    f.hub.store.kv_set(engine_accounts::SLOT, &registry.to_string()).unwrap();
    let gone = issue(&fixed, "acp-extra-4");
    assert!(bindings::activate_for_config(&f.hub.store, &gone, "acp-extra-4", &frozen).is_err());
    // Seleção ilegível: recusa, sem cair no padrão do bot.
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE donna_bot_sessions SET engine_choice='{}' WHERE id=?1", [fixed.session_id()])
        .unwrap();
    let corrupt = issue(&fixed, "acp-extra-5");
    assert!(bindings::activate_for_config(&f.hub.store, &corrupt, "acp-extra-5", &frozen).is_err());
}

/// R1: a migração substitui a definição já instalada do gatilho `donna_bindings_account` (a antiga revogava todas).
#[test]
fn migracao_substitui_o_gatilho_antigo_de_troca_de_conta() {
    let f = fixture(true);
    let fixed = extra(&f);
    let id = queued(&f, &fixed);
    let cfg = exec_cfg(&f, &fixed).unwrap();
    let (result, a, _) = admit(&f, &fixed, std::slice::from_ref(&id), &cfg, None, None, fixed.session.as_deref());
    assert!(result.unwrap());
    done(&f, &a);
    {
        let c = f.hub.store.db.locked();
        c.execute_batch(
            "DROP TRIGGER donna_bindings_account;
            CREATE TRIGGER donna_bindings_account AFTER UPDATE OF config ON bots
            WHEN json_extract(NEW.config,'$.forkEditor.engineAccountId') IS NOT json_extract(OLD.config,'$.forkEditor.engineAccountId')
                OR json_extract(NEW.config,'$.backend') IS NOT json_extract(OLD.config,'$.backend') BEGIN
                UPDATE donna_session_bindings SET revoked_at=COALESCE(revoked_at,strftime('%s','now')*1000) WHERE bot=NEW.id;
            END;",
        )
        .unwrap();
        super::migrate(&c).unwrap();
    }
    let binding = bindings::issue(&f.hub.store, "dev", "dev", fixed.session_id(), Some("acp-extra")).unwrap();
    let mut bot = f.hub.store.bot("dev").unwrap().unwrap().config;
    bot.model = Some("model-a".into());
    bot.fork_editor.engine_account_id = Some(account('a'));
    f.hub.store.save_bot(&bot).unwrap();
    assert!(
        bindings::resolve(&f.hub.store.db.locked(), binding.id.as_str(), "dev").is_ok(),
        "definição nova instalada"
    );
}

/// R2 (DON-833): na retomada, o vínculo do setup desta tentativa revogado entre o setup e o permit (por exemplo, o
/// dono troca a conta do bot durante os awaits) recusa antes do claim: sem ativação, envio, recibo novo ou CAS, e sem
/// recapturar o vínculo reemitido. Controle: a mesma retomada com o vínculo vivo passa.
#[test]
fn resume_com_vinculo_do_setup_revogado_recusa_sem_efeito_e_vinculo_vivo_passa() {
    let f = fixture(true);
    let main = Lane::main("dev");
    let id = queued(&f, &main);
    let cfg = exec_cfg(&f, &main).unwrap();
    let ids = [id.clone()];
    let (result, a, _) = admit(&f, &main, &ids, &cfg, None, None, None);
    assert!(result.unwrap());
    done(&f, &a);
    let waiting = json!({"phase":"waiting","executionToken":a.token,"routing":{"kind":"routed"},
        "input":{"chat":"dev","entries":[id]}});
    f.hub.store.kv_set("turn.waiting.dev", &waiting.to_string()).unwrap();
    let mut retry = waiting.clone();
    retry["phase"] = json!("retrying");
    let checkpoint = retry.to_string();
    let attempt = |binding: &BindingRef| {
        f.hub.prepare_bot_turn("dev", Some(&a.token), |r| r.lane = Some(main.clone())).unwrap();
        let ticket = admission::reserve_for_entries(&f.hub.store, &cfg, "dev", &a.token, &Origin::Human, &ids).unwrap();
        let turn = Turn { cfg: &cfg, lane: &main, entries: &ids, token: &a.token, account_revision: revision(&cfg) };
        let mut dispatch = |c: &Connection, original: &BindingRef| admissao::dispatch(c, &turn, original);
        let mut resume = |c: &Connection, raw: &str| admissao::resume(c, &turn, raw);
        let hooks = ClaimHooks {
            binding: Some(binding),
            dispatch: Some(&mut dispatch),
            resume: Some(&mut resume),
            ..Default::default()
        };
        let result = f.hub.permit_bot_dispatch(&cfg, &ticket, Some(&checkpoint), &ids, None, hooks);
        (result, ticket)
    };
    let before = (checkpoints(&f), f.hub.store.kv_read("turn.waiting.dev").unwrap());
    // Setup emitiu A; durante os awaits, A é revogado e B é emitido. A tentativa conserva A: recusa.
    let setup = bindings::issue(&f.hub.store, "dev", "dev", "dev", Some("acp-a")).unwrap();
    bindings::revoke(&f.hub.store, "dev", Some("acp-a")).unwrap();
    let _reissued = bindings::issue(&f.hub.store, "dev", "dev", "dev", Some("acp-b")).unwrap();
    let (result, ticket) = attempt(&setup);
    assert!(result.is_err());
    assert_eq!(admission::current(&f.hub.store, "dev").unwrap().unwrap().phase, Phase::Reserved);
    assert_eq!((checkpoints(&f), f.hub.store.kv_read("turn.waiting.dev").unwrap()), before, "sem recibo nem claim");
    assert!(f.hub.runtime("dev").memory_turn_token.is_none(), "nada ativado");
    admission::finish(&f.hub.store, "dev", &ticket).unwrap();
    // Controle positivo: vínculo vivo do setup desta tentativa, no destino original.
    let live = bindings::issue(&f.hub.store, "dev", "dev", "dev", Some("acp-c")).unwrap();
    let (result, ticket) = attempt(&live);
    assert!(result.unwrap());
    assert_eq!(checkpoints(&f), before.0, "retomada não grava recibo novo");
    admission::finish(&f.hub.store, "dev", &ticket).unwrap();
}

/// F6-B, regra (k): sob uma troca de conta, os vínculos da reserva são conferidos DENTRO do permit. Revogar entre a prova
/// do ator e o despacho (aqui: a conta sai da ordem do Space) não envia nada, sem recibo e com a mensagem ainda na fila.
#[test]
fn troca_de_conta_revogada_entre_a_prova_e_o_despacho_nao_envia_nada() {
    use crate::fork::fallback_contas::{
        Motivo, Sobreposicao,
        estado::{self, chave_ordem},
    };
    let f = fixture(true);
    let conta = |letra: char| {
        let launch = json!({"kind":"cli_profile","executable":"/fixture/codex","directory":"/fixture/c"});
        let proof = json!({"method":"codex_account_read","plan":"pro"});
        json!({"id":account(letra),"backend":"codex","billing":"subscription","state":"connected","launch":launch,
            "scope":"owner","quotaIdentity":account(letra),"verifiedAt":1,"subscriptionProof":proof,
            "models":[{"id":"model-b","options":[]}]})
    };
    f.hub
        .store
        .kv_set(
            engine_accounts::SLOT,
            &json!({"salt":uuid::Uuid::new_v4().to_string(),"accounts":[conta('b'),conta('c')]}).to_string(),
        )
        .unwrap();
    // Um bot do Space usa a reserva `c`: vínculo de verdade, independente da lista.
    f.hub
        .store
        .save_bot(
            &serde_json::from_value::<BotConfig>(
                json!({"id":"alt","name":"alt","backend":"codex","model":"model-b","forkEditor":{"engineAccountId":account('c')}}),
            )
            .unwrap(),
        )
        .unwrap();
    estado::alterar(&f.hub.store, |e| {
        e.ordens.insert(chave_ordem(PERSONAL, "codex"), vec![account('c')]);
        Ok(())
    })
    .unwrap();
    let sobre = Sobreposicao {
        space: PERSONAL.into(),
        bot: "dev".into(),
        origem: account('b'),
        atual: account('c'),
        desde: 1,
        motivo: Motivo::PertoDoLimite,
    };
    estado::gravar_sobreposicao(&f.hub.store.db.locked(), "dev", Some(&sobre)).unwrap();
    let main = Lane::main("dev");
    let cfg = exec_cfg(&f, &main).unwrap();
    assert_eq!(cfg.fork_editor.engine_account_id, Some(account('c')), "a conversa roda na reserva");
    // Com os vínculos vivos o despacho passa.
    let id = queued(&f, &main);
    let (result, a, _) = admit(&f, &main, std::slice::from_ref(&id), &cfg, None, None, None);
    assert!(result.unwrap(), "vínculos vivos: despacha");
    assert_eq!(status(&f, &id), "sent");
    done(&f, &a);
    // Revogação no meio: a conta sai da ordem do Space entre a prova e o permit.
    estado::alterar(&f.hub.store, |e| {
        e.ordens.clear();
        Ok(())
    })
    .unwrap();
    let before = checkpoints(&f);
    let id = queued(&f, &main);
    let (result, a, _) = admit(&f, &main, std::slice::from_ref(&id), &cfg, None, None, None);
    let error = result.unwrap_err();
    assert!(format!("{error:#}").contains("fallback_no_proof"), "{error:#}");
    assert_eq!((checkpoints(&f), status(&f, &id).as_str()), (before, "queued"), "nada enviado, sem recibo");
    done(&f, &a);
}

/// F6-B, regra (k) na RETOMADA D72: a conversa já na reserva que entra em espera e retoma também tem os vínculos conferidos
/// DENTRO do permit. Revogar entre a reprova do ator e o permit recusa a retomada, sem recibo e sem tocar a espera.
#[test]
fn retomada_na_reserva_com_vinculo_revogado_entre_a_prova_e_o_permit_nao_envia_nada() {
    use crate::fork::fallback_contas::{
        Motivo, Sobreposicao,
        estado::{self, chave_ordem},
    };
    let f = fixture(true);
    let conta = |letra: char| {
        let launch = json!({"kind":"cli_profile","executable":"/fixture/codex","directory":"/fixture/c"});
        let proof = json!({"method":"codex_account_read","plan":"pro"});
        json!({"id":account(letra),"backend":"codex","billing":"subscription","state":"connected","launch":launch,
            "scope":"owner","quotaIdentity":account(letra),"verifiedAt":1,"subscriptionProof":proof,
            "models":[{"id":"model-b","options":[]}]})
    };
    f.hub
        .store
        .kv_set(
            engine_accounts::SLOT,
            &json!({"salt":uuid::Uuid::new_v4().to_string(),"accounts":[conta('b'),conta('c')]}).to_string(),
        )
        .unwrap();
    f.hub
        .store
        .save_bot(
            &serde_json::from_value::<BotConfig>(
                json!({"id":"alt","name":"alt","backend":"codex","model":"model-b","forkEditor":{"engineAccountId":account('c')}}),
            )
            .unwrap(),
        )
        .unwrap();
    estado::alterar(&f.hub.store, |e| {
        e.ordens.insert(chave_ordem(PERSONAL, "codex"), vec![account('c')]);
        Ok(())
    })
    .unwrap();
    let sobre = Sobreposicao {
        space: PERSONAL.into(),
        bot: "dev".into(),
        origem: account('b'),
        atual: account('c'),
        desde: 1,
        motivo: Motivo::PertoDoLimite,
    };
    estado::gravar_sobreposicao(&f.hub.store.db.locked(), "dev", Some(&sobre)).unwrap();
    let main = Lane::main("dev");
    let id = queued(&f, &main);
    let cfg = exec_cfg(&f, &main).unwrap();
    assert_eq!(cfg.fork_editor.engine_account_id, Some(account('c')), "a conversa roda na reserva");
    let ids = [id.clone()];
    let (result, a, _) = admit(&f, &main, &ids, &cfg, None, None, None);
    assert!(result.unwrap());
    done(&f, &a);
    // A espera D72 (limite) gravou o checkpoint; a retomada reenvia o mesmo pedido pelo mesmo token.
    let waiting = json!({"phase":"waiting","executionToken":a.token,"routing":{"kind":"routed"},
        "input":{"chat":"dev","entries":[id]}});
    f.hub.store.kv_set("turn.waiting.dev", &waiting.to_string()).unwrap();
    let mut retry = waiting.clone();
    retry["phase"] = json!("retrying");
    let checkpoint = retry.to_string();
    let attempt = |binding: &BindingRef| {
        f.hub.prepare_bot_turn("dev", Some(&a.token), |r| r.lane = Some(main.clone())).unwrap();
        let ticket = admission::reserve_for_entries(&f.hub.store, &cfg, "dev", &a.token, &Origin::Human, &ids).unwrap();
        let turn = Turn { cfg: &cfg, lane: &main, entries: &ids, token: &a.token, account_revision: revision(&cfg) };
        let mut dispatch = |c: &Connection, original: &BindingRef| admissao::dispatch(c, &turn, original);
        let mut resume = |c: &Connection, raw: &str| admissao::resume(c, &turn, raw);
        let hooks = ClaimHooks {
            binding: Some(binding),
            dispatch: Some(&mut dispatch),
            resume: Some(&mut resume),
            ..Default::default()
        };
        let result = f.hub.permit_bot_dispatch(&cfg, &ticket, Some(&checkpoint), &ids, None, hooks);
        (result, ticket)
    };
    let before = (checkpoints(&f), f.hub.store.kv_read("turn.waiting.dev").unwrap());
    // Revogação no meio: a conta sai da ordem do Space depois da prova do ator e antes do permit.
    estado::alterar(&f.hub.store, |e| {
        e.ordens.clear();
        Ok(())
    })
    .unwrap();
    let binding = bindings::issue(&f.hub.store, "dev", "dev", "dev", Some("acp-revogada")).unwrap();
    let (result, ticket) = attempt(&binding);
    let error = result.unwrap_err();
    assert!(format!("{error:#}").contains("fallback_no_proof"), "{error:#}");
    assert_eq!(admission::current(&f.hub.store, "dev").unwrap().unwrap().phase, Phase::Reserved, "nada permitido");
    assert_eq!((checkpoints(&f), f.hub.store.kv_read("turn.waiting.dev").unwrap()), before, "sem recibo nem claim");
    admission::finish(&f.hub.store, "dev", &ticket).unwrap();
    // Controle positivo: com o vínculo de volta, a mesma retomada passa.
    estado::alterar(&f.hub.store, |e| {
        e.ordens.insert(chave_ordem(PERSONAL, "codex"), vec![account('c')]);
        Ok(())
    })
    .unwrap();
    let live = bindings::issue(&f.hub.store, "dev", "dev", "dev", Some("acp-viva")).unwrap();
    let (result, ticket) = attempt(&live);
    assert!(result.unwrap(), "vínculo vivo: a retomada passa");
    assert_eq!(checkpoints(&f), before.0, "retomada não grava recibo novo");
    admission::finish(&f.hub.store, "dev", &ticket).unwrap();
}
