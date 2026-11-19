#![allow(clippy::unwrap_used)]
//! 05a/D70: fronteiras do escopo pessoal positivo e do seam `CalendarRead` → `PrivateView`.
use super::{
    ContextRef, ResolvedScope, Revision,
    fontes::{
        PersonalOrigin as O, PersonalScopeExpectation, ScopeSourceError as E, SourceInvalidationReason as Reason,
    },
    resolver::{invalidate_personal_sources, resolve_personal},
    uso::read_private_view,
};
use crate::{
    api::devices::Caller,
    fork::{
        plugins::private_content::{self, Class, Destination, Operation},
        sessoes::{bindings, store as sessions},
        spaces::PERSONAL,
    },
    hub::Hub,
    remote::identity::Identity,
    store::{Scope, Store, now_ms},
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, sync::atomic::Ordering};

const BOT: &str = "00000000-0000-4000-8000-000000000012";
const OTHER: &str = "00000000-0000-4000-8000-000000000013";

struct F {
    h: Arc<Hub>,
    dir: PathBuf,
    raw: Value,
    space: Revision,
    bot: Revision,
}
fn rev(n: i64) -> Revision {
    Revision::try_from(u64::try_from(n).unwrap()).unwrap()
}
fn fixture() -> F {
    let dir = std::env::temp_dir().join(format!("times05a-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir.join("fixture.db")).unwrap();
    let hub_id = "00000000-0000-4000-8000-000000000011";
    store.kv_set("host_id", hub_id).unwrap();
    let cfg = serde_json::from_value(json!({"id":BOT,"name":"fixture","cwd":"/tmp","backend":"claude"})).unwrap();
    store.save_bot(&cfg).unwrap();
    store.kv_set("team.settings.v1", &json!({"primaryBotId":BOT}).to_string()).unwrap();
    let space = rev(store.spaces().unwrap().spaces[0].revision);
    let conversation = store.space_conversation(PERSONAL, BOT).unwrap().revision;
    let bot = rev(store.db.lock().unwrap().query_row("SELECT rev FROM bots WHERE id=?1", [BOT], |r| r.get(0)).unwrap());
    let raw = json!({"version":1,"hubId":hub_id,"spaceId":PERSONAL,"botId":BOT,"conversationId":BOT,
        "audience":{"kind":"principal"},"teamId":null,"privacyDomainId":PERSONAL,
        "domainRevision":u64::from(space),"conversationRevision":conversation});
    let h = Hub::new(store, hub_id.into(), Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
    F { h, dir, raw, space, bot }
}
impl F {
    fn input_from(&self, raw: Value) -> PersonalScopeExpectation {
        PersonalScopeExpectation {
            context: serde_json::from_value::<ContextRef>(raw).unwrap(),
            space_revision: self.space,
            bot_revision: self.bot,
        }
    }
    fn input(&self) -> PersonalScopeExpectation {
        self.input_from(self.raw.clone())
    }
    fn barrier(&self) -> Revision {
        rev(self.sql("SELECT revision FROM donna_scope_authority_state"))
    }
    fn global(&self) -> i64 {
        self.sql("SELECT CAST(v AS INTEGER) FROM kv WHERE k='rev'")
    }
    fn sql(&self, query: &str) -> i64 {
        self.h.store.db.lock().unwrap().query_row(query, [], |r| r.get(0)).unwrap()
    }
    fn exec(&self, query: &str, params: impl rusqlite::Params) {
        self.h.store.db.lock().unwrap().execute(query, params).unwrap();
    }
    fn activations(&self) -> i64 {
        self.sql("SELECT COUNT(*) FROM donna_scope_personal_activation")
    }
    fn app(&self, session: &str) -> Result<ResolvedScope, E> {
        resolve_personal(&self.h, Caller::Local, self.input(), O::App { session_id: session.into() })
    }
}
fn read(h: &Hub, scope: &ResolvedScope) -> Result<&'static str, E> {
    read_private_view(h, scope, Operation::CalendarRead, Destination::PrivateView, |_| Ok("sentinela"))
}
fn done(f: F) {
    drop(f.h);
    std::fs::remove_dir_all(f.dir).unwrap();
}

#[tokio::test]
async fn times05a_bootstrap_unico_app_e_turno_acp() {
    let mut f = fixture();
    let g = f.global();
    // Pedido sem ingresso ou com sessão inexistente não faz bootstrap.
    let pairing = Caller::Pairing { key: "fixture".into() };
    assert!(matches!(
        resolve_personal(&f.h, pairing, f.input(), O::App { session_id: BOT.into() }),
        Err(E::IngressDenied)
    ));
    assert!(matches!(f.app("00000000-0000-4000-8000-000000000099"), Err(E::ContextUnavailable)));
    assert_eq!((f.activations(), f.global()), (0, g));
    let scope = f.app(BOT).unwrap();
    assert_eq!((f.activations(), f.global()), (1, g + 1));
    // Bootstrap único: novos pedidos e leituras não gravam nem trocam a época.
    let again = f.app(BOT).unwrap();
    assert_eq!(read(&f.h, &scope), Ok("sentinela"));
    assert_eq!(read(&f.h, &again), Ok("sentinela"));
    assert_eq!((f.activations(), f.global()), (1, g + 1));

    let execution = bindings::fixture_execution(&f.h, BOT);
    // Working/lane emitem o bot e avançam `bots.rev` (vínculo M1a1): a expectativa usa a revisão atual.
    f.bot = rev(f.sql(&format!("SELECT rev FROM bots WHERE id='{BOT}'")));
    let g = f.global();
    let turn = resolve_personal(&f.h, Caller::Local, f.input(), O::Execution(execution.clone())).unwrap();
    assert_eq!(read(&f.h, &turn), Ok("sentinela"));
    assert_eq!(f.global(), g);
    f.h.set_runtime(BOT, |r| r.memory_turn_token = Some("outro-turno".into()));
    assert_eq!(read(&f.h, &turn), Err(E::ContextChanged));
    assert!(matches!(
        resolve_personal(&f.h, Caller::Local, f.input(), O::Execution(execution.clone())),
        Err(E::ContextChanged)
    ));
    f.h.set_runtime(BOT, |r| r.memory_turn_token = Some("fixture-turn".into()));
    assert_eq!(read(&f.h, &turn), Ok("sentinela"));
    bindings::revoke(&f.h.store, BOT, None).unwrap();
    assert_eq!(read(&f.h, &turn), Err(E::ContextUnavailable));
    assert!(matches!(
        resolve_personal(&f.h, Caller::Local, f.input(), O::Execution(execution)),
        Err(E::ContextUnavailable)
    ));
    done(f);
}

#[tokio::test]
async fn times05a_bootstrap_falho_reverte() {
    let f = fixture();
    let g = f.global();
    f.h.store.db.lock().unwrap().execute_batch("CREATE TRIGGER t05a_fault BEFORE INSERT ON donna_scope_personal_activation BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(matches!(f.app(BOT), Err(E::OutcomeUnknown)));
    assert_eq!((f.activations(), f.global()), (0, g));
    f.h.store.db.lock().unwrap().execute_batch("DROP TRIGGER t05a_fault").unwrap();
    assert_eq!(read(&f.h, &f.app(BOT).unwrap()), Ok("sentinela"));
    done(f);
}

#[tokio::test]
async fn times05a_sessao_canonica_e_audiencia() {
    let f = fixture();
    // Domínio de privacidade/revisão escolhidos pelo chamador não viram escopo.
    for (field, value) in [("privacyDomainId", json!(OTHER)), ("domainRevision", json!(u64::from(f.space) + 1))] {
        let mut forged = f.raw.clone();
        forged[field] = value;
        assert!(matches!(
            resolve_personal(&f.h, Caller::Local, f.input_from(forged), O::App { session_id: BOT.into() }),
            Err(E::ContextChanged)
        ));
    }
    let extra = {
        let db = f.h.store.db.lock().unwrap();
        sessions::create(
            &db,
            &sessions::NewSession {
                chat: BOT.into(),
                origin: crate::fork::sessoes::origin::SessionOrigin::Owner,
                source_session: None,
                parent_session: None,
                title: "extra".into(),
                runs_on: None,
            },
        )
        .unwrap()
    };
    let scope = f.app(&extra.id).unwrap();
    assert_eq!(read(&f.h, &scope), Ok("sentinela"));
    f.exec("UPDATE donna_bot_sessions SET state='closed' WHERE id=?1", [&extra.id]);
    assert_eq!(read(&f.h, &scope), Err(E::ContextUnavailable));

    // Conversa adicional do Pessoal: a sessão precisa ser do mesmo chat da conversa.
    f.exec(
        "INSERT INTO donna_space_conversations(id,space_id,bot_id,title,revision,is_primary) VALUES(?1,?2,?3,'extra',7,0)",
        rusqlite::params![OTHER, PERSONAL, BOT],
    );
    sessions::ensure_principal(&f.h.store.db.lock().unwrap(), OTHER).unwrap();
    assert!(matches!(f.app(OTHER), Err(E::ContextUnavailable)));
    let mut additional = f.raw.clone();
    additional["conversationId"] = json!(OTHER);
    additional["audience"] = json!({"kind":"additional"});
    additional["conversationRevision"] = json!(7);
    let other =
        resolve_personal(&f.h, Caller::Local, f.input_from(additional.clone()), O::App { session_id: OTHER.into() });
    assert_eq!(read(&f.h, &other.unwrap()), Ok("sentinela"));
    assert!(matches!(
        resolve_personal(&f.h, Caller::Local, f.input_from(additional), O::App { session_id: BOT.into() }),
        Err(E::ContextUnavailable)
    ));
    done(f);
}

#[tokio::test]
async fn times05a_fontes_mudadas_apos_captura_negam() {
    let f = fixture();
    let scope = f.app(BOT).unwrap();
    f.exec("UPDATE donna_space_conversations SET revision=revision+1 WHERE id=?1", [BOT]);
    assert_eq!(read(&f.h, &scope), Err(E::ContextChanged));
    f.exec("UPDATE donna_space_conversations SET revision=revision-1 WHERE id=?1", [BOT]);
    f.h.store.sem_donna_fixa(); // D366: exercita a troca/exclusão legada, sem Donna fixa.
    f.h.store.kv_set("team.settings.v1", &json!({"primaryBotId":OTHER}).to_string()).unwrap();
    assert_eq!(read(&f.h, &scope), Err(E::ContextChanged));
    f.h.store.kv_set("team.settings.v1", &json!({"primaryBotId":BOT}).to_string()).unwrap();
    let epoch: String =
        f.h.store
            .db
            .lock()
            .unwrap()
            .query_row("SELECT recovery_epoch FROM donna_scope_personal_activation", [], |r| r.get(0))
            .unwrap();
    f.exec("UPDATE donna_scope_personal_activation SET recovery_epoch=?1", [uuid::Uuid::new_v4().to_string()]);
    assert_eq!(read(&f.h, &scope), Err(E::SourcesInvalidated));
    f.exec("UPDATE donna_scope_personal_activation SET policy_revision=policy_revision+1,recovery_epoch=?1", [&epoch]);
    assert_eq!(read(&f.h, &scope), Err(E::SourcesInvalidated));
    f.exec("UPDATE donna_scope_personal_activation SET policy_revision=policy_revision-1", []);
    assert_eq!(read(&f.h, &scope), Ok("sentinela"));

    // Invalidação nunca é reaberta automaticamente, nem por pedido novo válido.
    let g = f.global();
    invalidate_personal_sources(&f.h, &Caller::Local, &f.input(), f.barrier(), Reason::PolicyChanged).unwrap();
    assert_eq!(read(&f.h, &scope), Err(E::SourcesInvalidated));
    assert!(matches!(f.app(BOT), Err(E::SourcesInvalidated)));
    invalidate_personal_sources(&f.h, &Caller::Local, &f.input(), f.barrier(), Reason::RecoveryRequired).unwrap();
    assert!(matches!(f.app(BOT), Err(E::SourcesInvalidated)));
    assert_eq!((f.activations(), f.global()), (1, g + 2));
    done(f);
}

#[tokio::test]
async fn times05a_ingresso_hub_uso_e_aninhamento() {
    let f = fixture();
    f.h.cloud_synced.store(true, Ordering::Release);
    f.exec(
        "INSERT INTO devices(key,name,platform,source,scopes,lease_until,created_at) VALUES('synthetic','test','test','account','[\"control\"]',?1,1)",
        [now_ms() + 3_600_000],
    );
    let device = Caller::Device { key: "synthetic".into(), scopes: vec![Scope::Control], autoridade: None };
    let scope = resolve_personal(&f.h, device, f.input(), O::App { session_id: BOT.into() }).unwrap();
    assert_eq!(read(&f.h, &scope), Ok("sentinela"));
    f.exec("UPDATE devices SET lease_until=1 WHERE key='synthetic'", []);
    assert_eq!(read(&f.h, &scope), Err(E::IngressDenied));

    let scope = f.app(BOT).unwrap();
    f.h.store.kv_set("host_id", "00000000-0000-4000-8000-000000000099").unwrap();
    assert_eq!(read(&f.h, &scope), Err(E::ContextChanged));
    f.h.store.kv_set("host_id", "00000000-0000-4000-8000-000000000011").unwrap();
    let mut team = f.raw.clone();
    team["teamId"] = json!("00000000-0000-4000-8000-000000000022");
    assert!(matches!(
        resolve_personal(&f.h, Caller::Local, f.input_from(team), O::App { session_id: BOT.into() }),
        Err(E::ContextUnavailable)
    ));

    // Só CalendarRead → PrivateView; escrita e outros destinos não chegam à leitura.
    for (operation, destination) in [
        (Operation::CalendarCreate, Destination::PrivateView),
        (Operation::CalendarUpdate, Destination::PrivateView),
        (Operation::WhatsappReadChats, Destination::PrivateView),
        (Operation::CalendarRead, Destination::ModelInput),
        (Operation::CalendarRead, Destination::Entries),
    ] {
        let mut ran = false;
        let out = read_private_view(&f.h, &scope, operation, destination, |_| {
            ran = true;
            Ok(())
        });
        assert_eq!((out, ran), (Err(E::UseDenied), false));
    }
    // O contrato de materialização continua negativo; o escopo não o abre.
    assert!(private_content::probe(Operation::CalendarRead, Class::ToolResult, Destination::PrivateView).is_err());
    // Revalidação nunca aninha Tx: a leitura interna falha e a externa vira rollback-only.
    let outer = read_private_view(&f.h, &scope, Operation::CalendarRead, Destination::PrivateView, |_| {
        assert_eq!(read(&f.h, &scope), Err(E::OutcomeUnknown));
        Ok("sentinela")
    });
    assert_eq!(outer, Err(E::OutcomeUnknown));
    assert_eq!(read(&f.h, &scope), Ok("sentinela"));
    done(f);
}
