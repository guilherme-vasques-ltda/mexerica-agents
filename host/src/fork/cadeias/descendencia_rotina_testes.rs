//! Fronteiras do consumidor especializado, com dados sintéticos e sem motor.
#![allow(clippy::unwrap_used)]
use super::*;
use crate::{LockExt, store::Store};
use serde_json::json;
use std::{
    path::Path,
    sync::{Arc, Barrier},
};

fn db() -> Store {
    let store = Store::open(Path::new(":memory:")).unwrap();
    for bot in ["a", "b"] {
        store.save_bot(&serde_json::from_value(json!({"id":bot,"name":bot})).unwrap()).unwrap();
    }
    store
}
fn root(store: &Store, bot: &str) -> Recibo {
    let mut p = admissao::pedido(
        Tipo::Raiz,
        format!("root-{bot}"),
        admissao::digest(&"synthetic").unwrap(),
        bot,
        &format!("parent-run-{bot}"),
        None,
    );
    p.escopo = Escopo::ConversaDoBot { bot_id: bot.into(), space_id: Some(crate::fork::spaces::PERSONAL.into()) };
    p.rotina = Some("parent-routine".into());
    admissao::admitir(store, &p).unwrap()
}
fn origin(parent: &Recibo) -> RoutineOrigin {
    RoutineOrigin {
        version: 1,
        creator: parent.clone(),
        space_id: crate::fork::spaces::PERSONAL.into(),
        chat_id: parent.bot_id.clone(),
        session_id: parent.bot_id.clone(),
        execution_digest: "a".repeat(64),
    }
}
fn save_origin(store: &Store, id: &str, origin: &RoutineOrigin) {
    let mut state: Value = store
        .kv_read("routines.v1")
        .unwrap()
        .map_or_else(|| json!({"routines":[],"runs":[]}), |s| serde_json::from_str(&s).unwrap());
    state["routines"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":id,"botId":origin.creator.bot_id,"deleted":false,"_creationOrigin":origin}));
    store.kv_set("routines.v1", &state.to_string()).unwrap();
}
fn candidate(o: &RoutineOrigin, routine: &str, event: &str) -> Pedido {
    prepare(
        o,
        &o.creator.bot_id,
        routine,
        &uuid::Uuid::new_v4().to_string(),
        event.into(),
        admissao::digest(&(routine, event)).unwrap(),
    )
}
fn admit(store: &Store, mut p: Pedido) -> Result<Recibo> {
    let mut db = store.db.locked();
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let receipt = admit_on(&tx, &mut p)??;
    tx.commit()?;
    Ok(receipt)
}
fn link(store: &Store, r: &Recibo, routine: &str) {
    let raw = json!({"runs":[{"id":r.run_id,"botId":r.bot_id,"routineId":routine,"_chain":r}]}).to_string();
    store::link_runs(&store.db.locked(), &raw).unwrap();
}
fn used(store: &Store, r: &Recibo) -> i64 {
    store
        .db
        .locked()
        .query_row("SELECT used FROM donna_chain_roots WHERE id=?1", [&r.chain_id], |row| row.get(0))
        .unwrap()
}

#[test]
fn rotina_filha_neta_continuacao_e_replay_conservam_uma_raiz() {
    let s = db();
    let parent = root(&s, "a");
    let first_origin = origin(&parent);
    save_origin(&s, "child", &first_origin);
    let child = admit(&s, candidate(&first_origin, "child", "clock-1")).unwrap();
    assert_eq!((child.level, child.ordinal), (2, 2));
    assert_eq!(child.chain_id, parent.chain_id);
    // Antes do commit do JSON, a filha não pode despachar.
    assert_eq!(store::dispatch(&s.db.locked(), &child).unwrap(), Err(Recusa::EnvioPossivel));
    link(&s, &child, "child");
    store::dispatch(&s.db.locked(), &child).unwrap().unwrap();
    let mut resume = admissao::pedido(
        Tipo::Continuacao,
        "resume-child".into(),
        admissao::digest(&"resume").unwrap(),
        "a",
        &child.run_id,
        Some(child.clone()),
    );
    let ctx = admissao::contexto(&s, &child).unwrap();
    resume.escopo = ctx.escopo;
    resume.rotina = ctx.rotina;
    let resumed = admissao::admitir(&s, &resume).unwrap();
    assert_eq!((resumed.level, resumed.ordinal), (2, 3));
    link(&s, &resumed, "child");
    let next_origin = origin(&resumed);
    save_origin(&s, "grandchild", &next_origin);
    let grandchild = admit(&s, candidate(&next_origin, "grandchild", "clock-2")).unwrap();
    assert_eq!((grandchild.level, grandchild.ordinal), (3, 4));
    assert_eq!(grandchild.chain_id, parent.chain_id);
    assert!(store::contexto(&s.db.locked(), &grandchild).unwrap().is_some());
    assert_eq!(admit(&s, candidate(&first_origin, "child", "clock-1")).unwrap(), child);
    assert_eq!(used(&s, &parent), 4);
    let roots: i64 = s.db.locked().query_row("SELECT COUNT(*) FROM donna_chain_roots", [], |r| r.get(0)).unwrap();
    assert_eq!(roots, 1);
}

#[test]
fn team_para_rotina_despacha_sem_fingir_vinculo_da_raiz_team() {
    let s = db();
    let parent = root(&s, "a");
    let input =
        super::super::equipe::Entrada::nova("a", "b", "request", "ask_bot", "synthetic", None, Some(parent.clone()))
            .unwrap();
    let team = super::super::equipe::admitir_na(&s.db.locked(), &input, Some(crate::fork::spaces::PERSONAL), None)
        .unwrap()
        .unwrap();
    let inherited = origin(&team);
    save_origin(&s, "b-child", &inherited);
    let child = admit(&s, candidate(&inherited, "b-child", "clock")).unwrap();
    assert_eq!((child.level, child.ordinal), (3, 3));
    assert_eq!(child.chain_id, parent.chain_id);
    link(&s, &child, "b-child");
    let linked: i64 =
        s.db.locked()
            .query_row("SELECT linked FROM donna_chain_roots WHERE id=?1", [&parent.chain_id], |r| r.get(0))
            .unwrap();
    assert_eq!(linked, 0);
    store::dispatch(&s.db.locked(), &child).unwrap().unwrap();
    // A filha pode voltar ao consumidor Team, sem perder a cadeia ancestral.
    let next =
        super::super::equipe::Entrada::nova("b", "a", "back", "message_bot", "synthetic", None, Some(child.clone()))
            .unwrap();
    let back = super::super::equipe::admitir_na(&s.db.locked(), &next, Some(crate::fork::spaces::PERSONAL), None)
        .unwrap()
        .unwrap();
    assert_eq!((back.level, back.ordinal), (4, 4));
    assert_eq!(back.chain_id, parent.chain_id);
}

#[test]
fn ligacao_sobrevive_retencao_e_replay_nunca_reabre_ocorrencia() {
    let s = db();
    let parent = root(&s, "a");
    let o = origin(&parent);
    save_origin(&s, "child", &o);
    let r = admit(&s, candidate(&o, "child", "delivery-old")).unwrap();
    link(&s, &r, "child");
    // Retenção remove o transcript da Run, mas não o marcador por admissão.
    let raw = s.kv_read("routines.v1").unwrap().unwrap();
    let state: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(state["runs"].as_array().unwrap().as_slice(), [] as [Value; 0]);
    let replay = admit(&s, candidate(&o, "child", "delivery-old")).unwrap();
    assert_eq!(replay, r);
    assert!(store::vinculada(&s.db.locked(), &replay).unwrap());
    assert_eq!(used(&s, &parent), 2);
    store::dispatch(&s.db.locked(), &r).unwrap().unwrap();
    assert_eq!(store::dispatch(&s.db.locked(), &r).unwrap(), Err(Recusa::EnvioPossivel));
}

#[test]
fn ramo_especializado_recusa_raiz_nova_space_e_procedencia_adulterados() {
    let s = db();
    let parent = root(&s, "a");
    let o = origin(&parent);
    save_origin(&s, "child", &o);
    for mutation in 0..5 {
        let mut p = candidate(&o, "child", &format!("bad-{mutation}"));
        match mutation {
            0 => {
                p.tipo = Tipo::Raiz;
                p.anterior = None;
            }
            1 => p.candidato.bot_id = "b".into(),
            2 => p.candidato.run_id = parent.run_id.clone(),
            3 => {
                if let Escopo::RotinaDescendente { space_id, .. } = &mut p.escopo {
                    *space_id = "other-space".into();
                }
            }
            _ => {
                if let Escopo::RotinaDescendente { criacao, .. } = &mut p.escopo {
                    criacao.execution_digest = "b".repeat(64);
                }
            }
        }
        assert_eq!(admit(&s, p).unwrap_err().downcast_ref(), Some(&Recusa::ReciboInvalido), "mutação {mutation}");
    }
    assert_eq!(used(&s, &parent), 1);
}

#[test]
fn cinco_niveis_e_corrida_em_quarenta_e_nove_nao_admitem_cinquenta_e_um() {
    let s = Arc::new(db());
    let parent = root(&s, "a");
    let mut last = parent.clone();
    for level in 2..=5 {
        let o = origin(&last);
        let id = format!("level-{level}");
        save_origin(&s, &id, &o);
        last = admit(&s, candidate(&o, &id, "clock")).unwrap();
        assert_eq!(last.level, level);
    }
    let o = origin(&last);
    save_origin(&s, "sixth", &o);
    assert_eq!(admit(&s, candidate(&o, "sixth", "clock")).unwrap_err().downcast_ref(), Some(&Recusa::Nivel));
    let o = origin(&parent);
    save_origin(&s, "race-0", &o);
    save_origin(&s, "race-1", &o);
    s.db.locked().execute("UPDATE donna_chain_roots SET used=49 WHERE id=?1", [&parent.chain_id]).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|n| {
            let s = s.clone();
            let b = barrier.clone();
            let p = candidate(&o, &format!("race-{n}"), "race");
            std::thread::spawn(move || {
                b.wait();
                admit(&s, p)
            })
        })
        .collect();
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results.iter().filter(|r| r.as_ref().is_err_and(|e| e.downcast_ref() == Some(&Recusa::Execucoes))).count(),
        1
    );
    assert_eq!(used(&s, &parent), 50);
}

#[test]
fn migracao_linked_e_aditiva_e_idempotente() {
    let s = db();
    let r = root(&s, "a");
    store::migrate(&s.db.locked()).unwrap();
    store::migrate(&s.db.locked()).unwrap();
    assert_eq!(used(&s, &r), 1);
    let column: i64 = s
        .db
        .locked()
        .query_row("SELECT COUNT(*) FROM pragma_table_info('donna_chain_admissions') WHERE name='linked'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(column, 1);
}

// A captura usa os produtores de binding e dispatch com dados sintéticos; não
// interpreta texto do modelo nem se baseia somente no prefixo de um slot.
struct CaptureFixture {
    hub: Arc<crate::hub::Hub>,
    execution: ExecutionRef,
    parent: Recibo,
    dir: std::path::PathBuf,
}
impl CaptureFixture {
    fn new() -> Self {
        let s = db();
        let parent = root(&s, "a");
        let run =
            json!({"id":parent.run_id,"botId":"a","routineId":"parent-routine","status":"running","_chain":parent});
        s.kv_set("routines.v1", &json!({"routines":[],"runs":[run]}).to_string()).unwrap();
        let raw = s.kv_read("routines.v1").unwrap().unwrap();
        store::link_runs(&s.db.locked(), &raw).unwrap();
        store::dispatch(&s.db.locked(), &parent).unwrap().unwrap();
        let slot = crate::routines::session_slot(&parent.run_id);
        s.kv_set(&crate::store::run_session_key("a", &slot), "acp-synthetic").unwrap();
        let binding = bindings::issue_in(&s, "a", "a", "a", Some(&slot), None).unwrap();
        bindings::activate(&s, &binding, "acp-synthetic").unwrap();
        let dir = std::env::temp_dir().join(format!("routine-descendant-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let hub = crate::hub::Hub::new(
            s,
            "synthetic".into(),
            crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
            "synthetic".into(),
            0,
        );
        hub.set_runtime("a", |r| {
            r.status = crate::hub::BotStatus::Working;
            r.lane = Some(crate::store::Lane::main("a"));
            r.session = Some("acp-synthetic".into());
            r.memory_turn_token = Some("synthetic-turn".into());
        });
        let execution = bindings::capture(&hub, "a", binding.id.as_str()).unwrap();
        hub.store.kv_set("turn.inflight.a",&json!({"admissionVersion":1,"executionToken":"synthetic-turn","attempt":"synthetic-attempt","phase":"permitted","conversationId":"a","origin":{"kind":"routine","id":parent.run_id},"entryIds":[]}).to_string()).unwrap();
        Self { hub, execution, parent, dir }
    }
    fn capture(&self) -> Result<Option<RoutineOrigin>> {
        capture_creation_on(&self.hub.store.db.locked(), "a", crate::fork::spaces::PERSONAL, &self.execution, None)
    }
}
impl Drop for CaptureFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn captura_rotina_confere_run_slot_token_stop_space_e_revogacao() {
    let f = CaptureFixture::new();
    let captured = f.capture().unwrap().unwrap();
    assert_eq!(captured.creator, f.parent);
    for patch in [
        json!({"executionToken":"other-turn"}),
        json!({"stopRequested":true}),
        json!({"origin":{"kind":"routine","id":"missing-run"}}),
    ] {
        let before = f.hub.store.kv_read("turn.inflight.a").unwrap().unwrap();
        let mut raw: Value = serde_json::from_str(&before).unwrap();
        for (k, v) in patch.as_object().unwrap() {
            raw[k] = v.clone();
        }
        f.hub.store.kv_set("turn.inflight.a", &raw.to_string()).unwrap();
        assert!(f.capture().is_err());
        f.hub.store.kv_set("turn.inflight.a", &before).unwrap();
    }
    assert!(capture_creation_on(&f.hub.store.db.locked(), "a", "wrong-space", &f.execution, None).is_err());
    f.hub.store.set_session("a", Some("acp-synthetic")).unwrap();
    assert!(f.capture().is_err());
    f.hub.store.set_session("a", None).unwrap();
    bindings::revoke(&f.hub.store, "a", Some("acp-synthetic")).unwrap();
    assert!(f.capture().is_err());
}

#[test]
fn captura_team_exige_r1_em_execucao_e_entrada_destinataria_exata() {
    let f = CaptureFixture::new();
    // Aqui o mesmo bot a recebe um pedido de b; o recibo representa o destinatário.
    let parent = root(&f.hub.store, "b");
    let entrada =
        super::super::equipe::Entrada::nova("b", "a", "team-request", "ask_bot", "synthetic", None, Some(parent))
            .unwrap();
    let team =
        super::super::equipe::admitir_na(&f.hub.store.db.locked(), &entrada, Some(crate::fork::spaces::PERSONAL), None)
            .unwrap()
            .unwrap();
    f.hub.store.db.locked().execute("INSERT INTO donna_session_requests(id,schema_version,kind,mode,requester_bot_id,requester_chat_id,requester_session_id,requester_execution_ref,target_bot_id,target_chat_id,target_session_id,chain_id,admission_id,origin_entry_id,target_entry_id,attempt,fence,state,created_at,updated_at) VALUES('team-request',1,'inter_bot','wait','b','b','b','synthetic','a','a','a',?1,?2,'source-notice','target-notice',1,2,'running',0,0)",params![team.chain_id,team.admission_id]).unwrap();
    f.hub.store.kv_set("turn.inflight.a",&json!({"admissionVersion":1,"executionToken":"synthetic-turn","attempt":"synthetic-attempt","phase":"permitted","conversationId":"a","origin":{"kind":"ask"},"entryIds":["target-notice"]}).to_string()).unwrap();
    // Binding da sessão principal, não o antigo slot da rotina.
    let binding = bindings::issue_in(&f.hub.store, "a", "a", "a", None, None).unwrap();
    bindings::activate(&f.hub.store, &binding, "acp-synthetic").unwrap();
    let execution = bindings::capture(&f.hub, "a", binding.id.as_str()).unwrap();
    let capture =
        || capture_creation_on(&f.hub.store.db.locked(), "a", crate::fork::spaces::PERSONAL, &execution, None);
    assert_eq!(capture().unwrap().unwrap().creator, team);
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE donna_session_requests SET state='finished' WHERE id='team-request'", [])
        .unwrap();
    assert!(capture().is_err());
}

#[test]
fn continuacao_nao_troca_run_e_despacho_recusa_contexto_corrompido() {
    let s = db();
    let parent = root(&s, "a");
    let o = origin(&parent);
    save_origin(&s, "child", &o);
    let child = admit(&s, candidate(&o, "child", "one")).unwrap();
    link(&s, &child, "child");
    let ctx = admissao::contexto(&s, &child).unwrap();
    let mut p = admissao::pedido(
        Tipo::Continuacao,
        "wrong-resume".into(),
        admissao::digest(&"resume").unwrap(),
        "a",
        "other-run",
        Some(child.clone()),
    );
    p.escopo = ctx.escopo;
    p.rotina = ctx.rotina;
    assert_eq!(admissao::admitir(&s, &p).unwrap_err().downcast_ref(), Some(&Recusa::ReciboInvalido));
    let raw: String =
        s.db.locked()
            .query_row("SELECT scope FROM donna_chain_admissions WHERE id=?1", [&child.admission_id], |r| r.get(0))
            .unwrap();
    let mut raw: Value = serde_json::from_str(&raw).unwrap();
    raw["criacao"]["creator"]["chain_id"] = json!("foreign-chain");
    s.db.locked()
        .execute("UPDATE donna_chain_admissions SET scope=?2 WHERE id=?1", params![child.admission_id, raw.to_string()])
        .unwrap();
    assert!(store::contexto(&s.db.locked(), &child).unwrap().is_none());
    assert_eq!(store::dispatch(&s.db.locked(), &child).unwrap(), Err(Recusa::ReciboInvalido));
    assert_eq!(used(&s, &parent), 2);
}

#[test]
fn inflight_ausente_so_admite_humano_original_e_nunca_slot_de_rotina() {
    let f = CaptureFixture::new();
    f.hub.store.kv_delete("turn.inflight.a").unwrap();
    assert!(f.capture().is_err());
    let message = f
        .hub
        .store
        .insert_entry(
            &crate::store::Lane::main("a"),
            crate::store::EntryKind::User,
            1,
            &json!({"text":"Pedido sintético"}),
        )
        .unwrap();
    assert!(
        capture_creation_on(
            &f.hub.store.db.locked(),
            "a",
            crate::fork::spaces::PERSONAL,
            &f.execution,
            Some(&message.id)
        )
        .is_err()
    );
    let binding = bindings::issue_in(&f.hub.store, "a", "a", "a", None, None).unwrap();
    bindings::activate(&f.hub.store, &binding, "acp-synthetic").unwrap();
    let execution = bindings::capture(&f.hub, "a", binding.id.as_str()).unwrap();
    assert!(
        capture_creation_on(
            &f.hub.store.db.locked(),
            "a",
            crate::fork::spaces::PERSONAL,
            &execution,
            Some(&message.id)
        )
        .unwrap()
        .is_none()
    );
    assert!(
        capture_creation_on(&f.hub.store.db.locked(), "a", crate::fork::spaces::PERSONAL, &execution, None).is_err()
    );
}
