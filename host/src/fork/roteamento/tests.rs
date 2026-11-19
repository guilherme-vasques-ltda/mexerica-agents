#![allow(clippy::unwrap_used)]
use super::{
    api,
    config::{self, Document, Mode, Policy, Reserve},
    selection,
};
use crate::{
    LockExt,
    api::{devices::Caller, dispatch},
    fork::{
        engine_accounts,
        spaces::{EngineSelection, PERSONAL},
    },
    hub::Hub,
    remote::identity::Identity,
    store::{BotConfig, Scope, Store},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

struct Fixture {
    hub: Arc<Hub>,
    root: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("donna-routing-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        for id in ["dev", "other"] {
            let cfg: BotConfig = serde_json::from_value(json!({"id":id,"name":id,"backend":"codex","cwd":root,"model":"model-a","forkEditor":{"engineAccountId":account_id('a')}})).unwrap();
            store.save_bot(&cfg).unwrap();
        }
        store.kv_set(engine_accounts::SLOT, &json!({"salt":uuid::Uuid::new_v4().to_string(), "accounts":[
            {"id":account_id('a'),"backend":"codex","billing":"paid_api","state":"connected","launch":{"kind":"vault_profile","slot":"PRIVATE_SECRET_SENTINEL"},"scope":"owner","quotaIdentity":null,"verifiedAt":1,"subscriptionProof":null,"models":[{"id":"model-a","options":[{"id":"effort","values":["high"],"default":"high"}]}]},
            {"id":account_id('b'),"backend":"codex","billing":"paid_api","state":"connected","launch":{"kind":"vault_profile","slot":"PRIVATE_OTHER_SENTINEL"},"scope":"owner","quotaIdentity":null,"verifiedAt":1,"subscriptionProof":null,"models":[{"id":"model-b","options":[]}]}
        ]}).to_string()).unwrap();
        // D372 (decisão A): a principal destes bots representa o caso sem seleção própria (fora do recorte ou
        // anterior à migração), domínio do carimbo do bot. A principal com seleção é coberta em
        // `principal_com_selecao_propria_usa_o_carimbo_da_sessao`.
        store.db.locked().execute("UPDATE donna_bot_sessions SET engine_choice=NULL WHERE principal=1", []).unwrap();
        let hub = Hub::new(store, "routing-test".into(), Identity::load_or_create(&root).unwrap(), "test".into(), 0);
        Self { hub, root }
    }
    fn read_body(&self, id: &str) -> Value {
        json!({"botId":id,"spaceId":PERSONAL,"hubId":self.hub.host_id})
    }
    fn save_body(&self) -> Value {
        let mut body = self.read_body("dev");
        body["expectedRevision"] = 0.into();
        body["expectedGeneralRevision"] = 0.into();
        body["policy"] = json!({"mode":"auto","guide":"Específico","reserves":[]});
        body["generalGuide"] = "Geral".into();
        body
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn account_id(letter: char) -> String {
    format!("ac_{}", letter.to_string().repeat(64))
}
fn reserve() -> Reserve {
    Reserve {
        account_id: account_id('a'),
        account_revision: 1,
        selection: EngineSelection {
            backend: "codex".into(),
            model: Some("model-a".into()),
            model_options: BTreeMap::from([("effort".into(), "high".into())]),
        },
    }
}

#[tokio::test]
async fn defaults_auto_persistido_e_conferencia_nao_mudam_execucao() {
    let f = Fixture::new();
    let before = serde_json::to_value(f.hub.store.bot("dev").unwrap().unwrap().config).unwrap();
    for _ in 0..2 {
        let read = dispatch(&f.hub, &Caller::Local, "donnaRouting", f.read_body("dev")).await.unwrap();
        assert_eq!(read["policy"]["revision"], 0);
        assert_eq!(read["policy"]["value"]["mode"], "fixed");
        assert_eq!(read["policy"]["value"]["reserves"], json!([]));
        assert_eq!(read["fixedCheck"]["status"], "checked");
        assert!(!read.to_string().contains("PRIVATE_"));
    }
    let (key, _) = config::keys(PERSONAL, "dev");
    assert!(f.hub.store.kv_read(&key).unwrap().is_none());
    let saved = dispatch(&f.hub, &Caller::Local, "donnaRoutingSave", f.save_body()).await.unwrap();
    assert_eq!(saved["auto"]["available"], false);
    assert_eq!(saved["execution"], "existing_fixed");
    assert_eq!(saved["effectiveGuide"], "Específico");
    let other = dispatch(&f.hub, &Caller::Local, "donnaRoutingCheck", f.read_body("other")).await.unwrap();
    assert_eq!(other["effectiveGuide"], "Geral");
    assert_eq!(serde_json::to_value(f.hub.store.bot("dev").unwrap().unwrap().config).unwrap(), before);
    assert_eq!(
        f.hub
            .store
            .db
            .locked()
            .query_row("SELECT COUNT(*) FROM entries WHERE bot_id='dev'", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn cas_concorrente_e_rollback_do_guia_na_mesma_transacao() {
    let f = Fixture::new();
    let (a, b) = tokio::join!(
        dispatch(&f.hub, &Caller::Local, "donnaRoutingSave", f.save_body()),
        dispatch(&f.hub, &Caller::Local, "donnaRoutingSave", f.save_body())
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let mut invalid = f.save_body();
    invalid["expectedRevision"] = 1.into();
    invalid["expectedGeneralRevision"] = 1.into();
    invalid["generalGuide"] = "não deve gravar".into();
    invalid["policy"]["reserves"] = json!([reserve()]);
    invalid["policy"]["reserves"][0]["accountRevision"] = 99.into();
    assert!(dispatch(&f.hub, &Caller::Local, "donnaRoutingSave", invalid).await.is_err());
    let read = dispatch(&f.hub, &Caller::Local, "donnaRouting", f.read_body("dev")).await.unwrap();
    assert_eq!(read["generalGuide"]["value"], "Geral");
    assert_eq!(read["policy"]["revision"], 1);
}

#[test]
fn conta_modelo_opcao_revisao_e_catalogo_da_conta_exata() {
    let f = Fixture::new();
    let cfg = f.hub.store.bot("dev").unwrap().unwrap().config;
    let mut registry = engine_accounts::Registry::load(&f.hub.store).unwrap();
    assert!(selection::validate(&registry, &cfg, &reserve()).is_ok());
    let mut r = reserve();
    r.account_id = account_id('b');
    assert!(selection::validate(&registry, &cfg, &r).is_err());
    r = reserve();
    r.account_revision = 2;
    assert!(selection::validate(&registry, &cfg, &r).is_err());
    r = reserve();
    r.selection.model_options.insert("effort".into(), "inventado".into());
    assert!(selection::validate(&registry, &cfg, &r).is_err());
    r = reserve();
    r.selection.backend = "claude".into();
    assert!(selection::validate(&registry, &cfg, &r).is_err());
    r = reserve();
    r.selection.model = None;
    assert!(selection::validate(&registry, &cfg, &r).is_err());
    registry.accounts[0].models.clear();
    assert!(selection::validate(&registry, &cfg, &reserve()).is_err());
    let mut registry = engine_accounts::Registry::load(&f.hub.store).unwrap();
    let a = &mut registry.accounts[0];
    a.backend = engine_accounts::Backend::Claude;
    a.launch = engine_accounts::LaunchReference::CliProfile {
        executable: "/fixture/claude".into(),
        directory: "/fixture/profile".into(),
    };
    a.subscription_proof = Some(engine_accounts::SubscriptionProof::ClaudeAuthStatus { plan: "fixture".into() });
    a.managed = Some(serde_json::from_value(json!({"label":"Conta","revision":1,"generation":1,"enabled":true,"requestId":"fixture","principal":"private","profile":null,"setupRequest":null})).unwrap());
    a.bind_catalog();
    let mut r = reserve();
    r.selection.backend = "claude".into();
    assert!(selection::validate(&registry, &cfg, &r).is_ok());
    registry.accounts[0].managed.as_mut().unwrap().revision = 2;
    r.account_revision = 2;
    assert!(selection::validate(&registry, &cfg, &r).is_err(), "catálogo anterior não prova nova revisão");
}

#[tokio::test]
async fn json_fechado_limites_e_erros_sem_eco_ou_gravacao() {
    let f = Fixture::new();
    for (path, value) in [
        ("extra", json!("SEGREDO_TESTE")),
        ("botId", json!("https://SEGREDO_TESTE.invalid")),
        ("generalGuide", json!("x".repeat(8001))),
    ] {
        let mut b = f.save_body();
        b[path] = value;
        let error = dispatch(&f.hub, &Caller::Local, "donnaRoutingSave", b).await.unwrap_err().to_string();
        assert!(!error.contains("SEGREDO_TESTE"));
    }
    for path in ["token", "url", "selection"] {
        let mut b = f.save_body();
        b["policy"][path] = json!("SEGREDO_TESTE");
        let error = dispatch(&f.hub, &Caller::Local, "donnaRoutingSave", b).await.unwrap_err().to_string();
        assert!(!error.contains("SEGREDO_TESTE"));
    }
    let mut b = f.save_body();
    b["policy"]["reserves"] = json!([reserve()]);
    b["policy"]["reserves"][0]["selection"]["token"] = "SEGREDO_TESTE".into();
    assert!(dispatch(&f.hub, &Caller::Local, "donnaRoutingSave", b).await.is_err());
    let mut r = reserve();
    r.selection.model = Some("https://SEGREDO_TESTE.invalid".into());
    assert!(selection::shape(&r).is_err());
    assert!(f.hub.store.kv_read(&config::keys(PERSONAL, "dev").0).unwrap().is_none());
}

#[tokio::test]
async fn control_owner_devices_e_contexto_falso_nao_dao_engine_accounts() {
    let f = Fixture::new();
    for caller in [
        Caller::Device { key: "unproved".into(), scopes: vec![Scope::Control, Scope::OwnerDevices], autoridade: None },
        Caller::Pairing { key: "pair".into() },
    ] {
        for method in ["donnaRouting", "donnaRoutingCheck", "donnaRoutingSave"] {
            assert!(api::call(&f.hub, &caller, method, f.save_body()).await.is_err());
        }
    }
    let mut b = f.read_body("dev");
    b["hubId"] = "other-hub".into();
    assert!(dispatch(&f.hub, &Caller::Local, "donnaRouting", b).await.is_err());
    let mut b = f.read_body("dev");
    b["spaceId"] = "other-space".into();
    assert!(dispatch(&f.hub, &Caller::Local, "donnaRouting", b).await.is_err());
}

#[test]
fn outro_space_nao_descobre_cadastro_e_preserva_somente_referencias_existentes() {
    let f = Fixture::new();
    let mut cfg = f.hub.store.bot("dev").unwrap().unwrap().config;
    cfg.space_id = uuid::Uuid::new_v4().to_string();
    let db = f.hub.store.db.locked();
    assert_eq!(selection::accounts(&db, &cfg).unwrap(), json!([]));
    assert_eq!(selection::fixed(&db, &cfg)["reason"], "scope_unproven");
    let policy = Policy { mode: Mode::Auto, guide: "desejo".into(), reserves: vec![reserve()] };
    assert!(selection::policy(&db, &cfg, &Policy::default(), &policy).is_err());
    assert!(selection::policy(&db, &cfg, &policy, &policy).is_ok());
    assert!(selection::policy(&db, &cfg, &policy, &Policy::default()).is_ok());
    let defaults: Document<Policy> = config::load(&db, &config::keys(&cfg.space_id, "dev").0).unwrap();
    assert_eq!(defaults.revision, 0);
}

#[tokio::test]
async fn hub_contextual_obrigatorio_antes_da_leitura_ou_gravacao() {
    let f = Fixture::new();
    for method in api::METHODS {
        for divergent in [false, true] {
            let mut body = if method == "donnaRoutingSave" { f.save_body() } else { f.read_body("dev") };
            // Um bot inexistente produziria outro erro se a leitura viesse primeiro.
            body["botId"] = "inexistente".into();
            if divergent {
                body["hubId"] = "outro-hub".into();
            } else {
                body.as_object_mut().unwrap().remove("hubId");
            }
            let error = dispatch(&f.hub, &Caller::Local, method, body).await.unwrap_err().to_string();
            assert_eq!(error, "hub_context_mismatch");
        }
    }
    assert!(f.hub.store.kv_read(&config::keys(PERSONAL, "dev").0).unwrap().is_none());
    let read = dispatch(&f.hub, &Caller::Local, "donnaRouting", f.read_body("dev")).await.unwrap();
    assert_eq!(read["hubId"], f.hub.host_id);
}

#[tokio::test]
async fn cadastro_ilegivel_nao_vira_vazio_nem_grava_ou_apaga_preferencias() {
    let f = Fixture::new();
    let mut initial = f.save_body();
    initial["policy"]["reserves"] = json!([reserve()]);
    dispatch(&f.hub, &Caller::Local, "donnaRoutingSave", initial).await.unwrap();
    let (key, guide_key) = config::keys(PERSONAL, "dev");
    let policy_before = f.hub.store.kv_read(&key).unwrap();
    let guide_before = f.hub.store.kv_read(&guide_key).unwrap();
    let corrupt = "REGISTRO_PRIVADO_ILEGIVEL_SENTINELA";
    f.hub.store.kv_set(engine_accounts::SLOT, corrupt).unwrap();
    let error = dispatch(&f.hub, &Caller::Local, "donnaRouting", f.read_body("dev")).await.unwrap_err().to_string();
    assert!(error.starts_with("accounts_unavailable:"));
    assert!(!error.contains(corrupt));
    // A resposta falha após os writes; a mesma transação precisa desfazer ambos.
    let mut change = f.save_body();
    change["expectedRevision"] = 1.into();
    change["expectedGeneralRevision"] = 1.into();
    change["policy"]["guide"] = "não deve substituir".into();
    change["generalGuide"] = "não deve substituir".into();
    let error = dispatch(&f.hub, &Caller::Local, "donnaRoutingSave", change).await.unwrap_err().to_string();
    assert!(error.starts_with("accounts_unavailable:"));
    assert!(!error.contains(corrupt));
    assert_eq!(f.hub.store.kv_read(&key).unwrap(), policy_before);
    assert_eq!(f.hub.store.kv_read(&guide_key).unwrap(), guide_before);
    assert_eq!(f.hub.store.kv_read(engine_accounts::SLOT).unwrap().as_deref(), Some(corrupt));
    let mut cfg = f.hub.store.bot("dev").unwrap().unwrap().config;
    cfg.space_id = "outro-space".into();
    assert_eq!(selection::accounts(&f.hub.store.db.locked(), &cfg).unwrap(), json!([]));
    f.hub
        .store
        .kv_set(engine_accounts::SLOT, &json!({"salt":uuid::Uuid::new_v4().to_string(),"accounts":[]}).to_string())
        .unwrap();
    let empty = dispatch(&f.hub, &Caller::Local, "donnaRouting", f.read_body("dev")).await.unwrap();
    assert_eq!(empty["accounts"], json!([]));
    assert_eq!(empty["policy"]["value"]["reserves"], json!([reserve()]));
    assert_eq!(f.hub.store.kv_read(&key).unwrap(), policy_before);
    assert_eq!(f.hub.store.kv_read(&guide_key).unwrap(), guide_before);
}

/// Fixtures de armazenamento; não provam token/binding, Authority, claim ou Resume real.
mod checkpoints {
    use super::*;
    use crate::{
        fork::{
            roteamento::{
                admission::{
                    AdmissionCapture, CapturedBinding, CapturedText, CheckpointKey, DOCUMENT_MAX, DecidedBy,
                    Destination, ENTRIES_MAX, EntryEvidence, EntryProvenance, ExecutionSeal, Intent, Reason,
                    RequiredFact, RoutingSnapshot, SelectionSnapshot, StampCapture, TurnProvenance, UnattestedFacts,
                    decode,
                },
                receipt::{self, NotRoutedReason, ResumeRoute, RoutingClass},
            },
            sessoes::{self, origin::SessionOrigin, store::NewSession},
        },
        store::{EntryKind, Lane},
    };
    use rusqlite::{Connection, TransactionBehavior, params};

    fn capture(store: &Store, bot: &str, lane: &Lane) -> AdmissionCapture {
        let turn = store.alloc_turn(&lane.chat).unwrap();
        let entry =
            store.insert_entry(lane, EntryKind::User, turn, &json!({"status":"queued","sourceBotId":"other"})).unwrap();
        let c = store.db.locked();
        let session = sessoes::store::get(&c, lane.session_id()).unwrap().unwrap();
        let r = reserve();
        let selection = SelectionSnapshot { account_id: r.account_id, selection: r.selection };
        let raw = store_stamp(&c, &format!("account.stamp.{bot}"));
        AdmissionCapture {
            schema_version: 1,
            key: CheckpointKey {
                destination: Destination {
                    bot_id: bot.into(),
                    space_id: PERSONAL.into(),
                    session: sessoes::store::SessionRef { id: session.id, chat_id: session.chat_id },
                },
                execution_token: format!("fixture-token-{bot}"),
            },
            intent: Intent {
                entry_ids: vec![entry.id.clone()],
                hub_turn: turn,
                expected_session_revision: session.revision,
            },
            binding: CapturedBinding { binding_ref: "fixture-binding-data-only".into() },
            account_revision: 1,
            private_stamp: StampCapture::BotAccountStampV1 { capture: CapturedText::from_captured(raw) },
            execution_seal: ExecutionSeal::NotCaptured,
            routing: RoutingSnapshot {
                requested: selection.clone(),
                admitted: selection,
                decided_by: DecidedBy::Fixo,
                reason: Reason::Fixed,
                policy_revision: 0,
                general_guide_revision: 0,
                bot_guide_revision: 0,
                session_origin: session.origin,
                turn_provenance: TurnProvenance {
                    entries: vec![EntryProvenance {
                        entry_id: entry.id,
                        evidence: EntryEvidence::HubEntrySourceBotV1 { source_bot_id: "other".into() },
                    }],
                    authority_origin: RequiredFact::RequiredFactNotAttested,
                },
                required_facts: UnattestedFacts {
                    coverage_before: RequiredFact::RequiredFactNotAttested,
                    coverage_after: RequiredFact::RequiredFactNotAttested,
                    account_identity_generation_catalog: RequiredFact::RequiredFactNotAttested,
                },
            },
        }
    }
    fn store_stamp(c: &Connection, key: &str) -> Option<String> {
        use rusqlite::OptionalExtension;
        c.query_row("SELECT v FROM kv WHERE k=?1", [key], |r| r.get(0)).optional().unwrap()
    }
    fn count(c: &Connection) -> i64 {
        c.query_row("SELECT COUNT(*) FROM donna_routing_checkpoints", [], |r| r.get(0)).unwrap()
    }
    fn value(a: &AdmissionCapture) -> Value {
        serde_json::to_value(a).unwrap()
    }
    fn persist(c: &Connection, a: &AdmissionCapture) {
        let tx = c.unchecked_transaction().unwrap();
        receipt::record_dispatch(&tx, a).unwrap();
        tx.commit().unwrap();
    }
    fn legacy(c: &Connection) -> Vec<String> {
        [
            "SELECT json_group_array(json_array(k,v)) FROM (SELECT * FROM kv ORDER BY k)",
            "SELECT json_group_array(json_array(id,rev,deleted,config,session_id,read_rev)) FROM (SELECT * FROM bots ORDER BY id)",
            "SELECT json_group_array(json_array(seq,id,bot_id,rev,kind,turn,data,created_at,updated_at,session)) FROM (SELECT * FROM entries ORDER BY seq)",
            "SELECT json_group_array(json_array(id,chat_id,principal,origin,title,state,revision,runs_on)) FROM (SELECT * FROM donna_bot_sessions ORDER BY id)",
        ]
        .iter()
        .map(|sql| c.query_row(sql, [], |r| r.get(0)).unwrap())
        .collect()
    }

    #[test]
    fn migracao_propria_repetida_preserva_schema_legado_e_dados_rota02() {
        let f = Fixture::new();
        let a = capture(&f.hub.store, "dev", &Lane::main("dev"));
        let c = f.hub.store.db.locked();
        let tx = c.unchecked_transaction().unwrap();
        api::save(&tx, PERSONAL, "dev", 0, 0, Policy::default(), "guia preservado".into()).unwrap();
        tx.commit().unwrap();
        c.execute("DROP TABLE donna_routing_checkpoints", []).unwrap();
        let schema = |c: &Connection| -> Vec<(String, String)> {
            c.prepare("SELECT name,sql FROM sqlite_master WHERE sql IS NOT NULL AND name NOT LIKE 'donna_routing_%' ORDER BY name")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        let before = legacy(&c);
        let before_schema = schema(&c);
        receipt::migrate(&c).unwrap();
        receipt::migrate(&c).unwrap();
        assert_eq!(legacy(&c), before);
        assert_eq!(schema(&c), before_schema);
        assert_eq!(count(&c), 0);
        assert!(receipt::read_checkpoint(&c, &a.key).unwrap().is_none());
    }

    #[test]
    fn tx_externa_obrigatoria_e_rollback_de_linhas_reais_na_mesma_db() {
        let f = Fixture::new();
        let a = capture(&f.hub.store, "dev", &Lane::main("dev"));
        f.hub.store.kv_set("fixture.control", "original").unwrap();
        let c = f.hub.store.db.locked();
        let before = legacy(&c);
        let changes = c.total_changes();
        assert!(receipt::record_dispatch(&c, &a).is_err());
        assert_eq!(c.total_changes(), changes);
        assert_eq!(count(&c), 0);
        for fail_inside in [true, false] {
            let attempt = || -> anyhow::Result<()> {
                let tx = c.unchecked_transaction()?;
                tx.execute("UPDATE kv SET v='alterado' WHERE k='fixture.control'", [])?;
                tx.execute(
                    "UPDATE entries SET data=json_set(data,'$.status','sent'),rev=rev+1 WHERE id=?1",
                    [&a.intent.entry_ids[0]],
                )?;
                let mut input = a.clone();
                if fail_inside {
                    input.routing.admitted.selection.model = Some("model-inexistente".into());
                }
                receipt::record_dispatch(&tx, &input)?;
                assert_eq!(count(&tx), 1);
                anyhow::bail!("fixture_falha_do_chamador_apos_insert");
            };
            assert!(attempt().is_err());
            assert!(c.is_autocommit());
            assert_eq!(legacy(&c), before);
            assert_eq!(count(&c), 0);
        }
        persist(&c, &a);
        assert_eq!(count(&c), 1);
    }

    #[test]
    fn idempotencia_imutavel_e_conflitos_nao_reescrevem_original() {
        let f = Fixture::new();
        let a = capture(&f.hub.store, "dev", &Lane::main("dev"));
        let c = f.hub.store.db.locked();
        persist(&c, &a);
        let original: String =
            c.query_row("SELECT checkpoint_json FROM donna_routing_checkpoints", [], |r| r.get(0)).unwrap();
        let changes = c.total_changes();
        let tx = c.unchecked_transaction().unwrap();
        let same = receipt::record_dispatch(&tx, &a).unwrap();
        assert_eq!(value(same.captured()), value(&a));
        tx.commit().unwrap();
        assert_eq!(c.total_changes(), changes);
        let mut proposals = Vec::new();
        let mut b = a.clone();
        b.intent.hub_turn += 1;
        proposals.push(b);
        let mut b = a.clone();
        b.intent.expected_session_revision += 1;
        proposals.push(b);
        let mut b = a.clone();
        b.binding.binding_ref = "outra-ref".into();
        proposals.push(b);
        let mut b = a.clone();
        b.routing.admitted.selection.model_options.clear();
        proposals.push(b);
        let mut b = a.clone();
        b.private_stamp = StampCapture::BotAccountStampV1 { capture: CapturedText::Present { value: "null".into() } };
        proposals.push(b);
        let mut b = a.clone();
        b.routing.reason = Reason::Explicit;
        proposals.push(b);
        for b in proposals {
            let tx = c.unchecked_transaction().unwrap();
            assert!(receipt::record_dispatch(&tx, &b).is_err());
            tx.rollback().unwrap();
        }
        assert_eq!(c.total_changes(), changes);
        let after: String =
            c.query_row("SELECT checkpoint_json FROM donna_routing_checkpoints", [], |r| r.get(0)).unwrap();
        assert_eq!(after, original);
        assert_eq!(count(&c), 1);
    }

    #[test]
    fn referencias_db_cruzadas_revisao_e_proveniencia_falham_sem_gravar() {
        let f = Fixture::new();
        let a = capture(&f.hub.store, "dev", &Lane::main("dev"));
        let other = capture(&f.hub.store, "other", &Lane::main("other"));
        let c = f.hub.store.db.locked();
        let tx = c.unchecked_transaction().unwrap();
        let extra = sessoes::store::create(
            &tx,
            &NewSession {
                chat: "dev".into(),
                origin: SessionOrigin::Owner,
                source_session: None,
                parent_session: None,
                title: "Extra".into(),
                runs_on: None,
            },
        )
        .unwrap();
        tx.commit().unwrap();
        let mut invalid = Vec::new();
        let mut b = a.clone();
        b.key.destination.bot_id = "other".into();
        invalid.push(b);
        let mut b = a.clone();
        b.key.destination.space_id = "outro-space".into();
        invalid.push(b);
        let mut b = a.clone();
        b.key.destination.session = extra;
        invalid.push(b);
        let mut b = a.clone();
        b.key.destination.session.chat_id = "other".into();
        invalid.push(b);
        let mut b = a.clone();
        b.intent.entry_ids = other.intent.entry_ids.clone();
        b.routing.turn_provenance = other.routing.turn_provenance.clone();
        invalid.push(b);
        let mut b = a.clone();
        b.intent.expected_session_revision += 1;
        invalid.push(b);
        let mut b = a.clone();
        b.routing.turn_provenance.entries[0].evidence =
            EntryEvidence::HubEntrySourceBotV1 { source_bot_id: "dev".into() };
        invalid.push(b);
        let mut b = a.clone();
        b.routing.session_origin = SessionOrigin::Continuation;
        invalid.push(b);
        let mut b = a.clone();
        b.intent.entry_ids.push(b.intent.entry_ids[0].clone());
        invalid.push(b);
        let mut b = a.clone();
        b.account_revision = 2;
        invalid.push(b);
        let mut b = a.clone();
        b.routing.admitted.account_id = account_id('b');
        invalid.push(b);
        let mut b = a.clone();
        b.routing.admitted.selection.backend = "claude".into();
        invalid.push(b);
        let mut b = a.clone();
        b.routing.admitted.selection.model_options.insert("effort".into(), "inventado".into());
        invalid.push(b);
        let mut b = a.clone();
        b.routing.admitted.selection.model = Some("PRIVATE_MODEL_SENTINEL".into());
        invalid.push(b);
        for b in invalid {
            let tx = c.unchecked_transaction().unwrap();
            let error = receipt::record_dispatch(&tx, &b).err().unwrap().to_string();
            assert!(!error.contains("PRIVATE_"));
            tx.rollback().unwrap();
        }
        assert_eq!(count(&c), 0);
        let tx = c.unchecked_transaction().unwrap();
        tx.execute("UPDATE donna_bot_sessions SET state='closed' WHERE id='dev'", []).unwrap();
        assert!(receipt::record_dispatch(&tx, &a).is_err());
        tx.rollback().unwrap();
        let tx = c.unchecked_transaction().unwrap();
        tx.execute("UPDATE bots SET deleted=1 WHERE id='dev'", []).unwrap();
        assert!(receipt::record_dispatch(&tx, &a).is_err());
        tx.rollback().unwrap();
        for accounts in [
            json!([]),
            json!([{
                "id":account_id('a'),"backend":"codex","billing":"paid_api","state":"disconnected",
                "launch":{"kind":"vault_profile","slot":"PRIVATE_SECRET_SENTINEL"},"scope":"owner",
                "quotaIdentity":null,"verifiedAt":1,"subscriptionProof":null,
                "models":[{"id":"model-a","options":[]}]
            }]),
        ] {
            let tx = c.unchecked_transaction().unwrap();
            tx.execute(
                "UPDATE kv SET v=json_set(v,'$.accounts',json(?2)) WHERE k=?1",
                [engine_accounts::SLOT, &accounts.to_string()],
            )
            .unwrap();
            assert!(receipt::record_dispatch(&tx, &a).is_err());
            tx.rollback().unwrap();
        }
        assert_eq!(count(&c), 0);
    }

    #[test]
    fn captura_bot_preserva_ausencia_null_e_campos_nulos_sem_virar_selecao() {
        for raw in [
            None,
            Some("null".to_owned()),
            Some(json!({"account":null,"revision":null,"backend":"claude"}).to_string()),
        ] {
            let f = Fixture::new();
            if let Some(raw) = &raw {
                f.hub.store.kv_set("account.stamp.dev", raw).unwrap();
            }
            let a = capture(&f.hub.store, "dev", &Lane::main("dev"));
            let c = f.hub.store.db.locked();
            persist(&c, &a);
            let checkpoint = receipt::read_checkpoint(&c, &a.key).unwrap().unwrap();
            let saved = value(checkpoint.captured());
            assert_eq!(saved["privateStamp"], value(&a)["privateStamp"]);
            assert_eq!(saved["routing"]["admitted"]["selection"]["backend"], "codex");
            match &raw {
                None => {
                    assert_eq!(saved["privateStamp"]["capture"]["state"], "absent");
                    assert!(saved["privateStamp"]["capture"].get("value").is_none());
                }
                Some(raw) => {
                    assert_eq!(saved["privateStamp"]["capture"]["state"], "present");
                    assert_eq!(saved["privateStamp"]["capture"]["value"], *raw);
                    let stamp: Value = serde_json::from_str(raw).unwrap();
                    if !stamp.is_null() {
                        assert_eq!(stamp.as_object().unwrap().len(), 3);
                        assert!(stamp["account"].is_null());
                        assert!(stamp["revision"].is_null());
                        assert_eq!(stamp["backend"], "claude");
                        assert!(stamp.get("model").is_none());
                        assert!(stamp.get("modelOptions").is_none());
                    }
                }
            }
        }
        let f = Fixture::new();
        for raw in [
            json!({"account":null,"backend":"codex"}),
            json!({"revision":null,"backend":"codex"}),
            json!({"account":null,"revision":null,"backend":"codex","model":"model-a"}),
        ] {
            f.hub.store.kv_set("account.stamp.dev", &raw.to_string()).unwrap();
            let a = capture(&f.hub.store, "dev", &Lane::main("dev"));
            let c = f.hub.store.db.locked();
            let tx = c.unchecked_transaction().unwrap();
            assert!(receipt::record_dispatch(&tx, &a).is_err());
            tx.rollback().unwrap();
            assert_eq!(count(&c), 0);
        }
    }

    /// Sessão extra do chat próprio, com o carimbo lido da fonte canônica dela (sem produtor aqui).
    fn extra_capture(f: &Fixture) -> (AdmissionCapture, String) {
        let extra = {
            let c = f.hub.store.db.locked();
            let tx = c.unchecked_transaction().unwrap();
            let new = NewSession {
                chat: "dev".into(),
                origin: SessionOrigin::Owner,
                source_session: None,
                parent_session: None,
                title: "Extra".into(),
                runs_on: None,
            };
            let extra = sessoes::store::create(&tx, &new).unwrap();
            // A seleção efetiva da extra é a que o turno executa (aqui, a combinação admitida da fixture).
            tx.execute("UPDATE donna_bot_sessions SET engine_choice=?2 WHERE id=?1", params![extra.id, effective()])
                .unwrap();
            tx.commit().unwrap();
            extra
        };
        let source = format!("account.stamp.dev/session:{}", extra.id);
        let mut a = capture(&f.hub.store, "dev", &Lane { chat: "dev".into(), session: Some(extra.id) });
        let raw = store_stamp(&f.hub.store.db.locked(), &source);
        a.private_stamp = StampCapture::SessionAccountStampV1 { capture: CapturedText::from_captured(raw) };
        (a, source)
    }
    /// A seleção efetiva v2 igual à combinação admitida da fixture (`reserve`), com a conta dada.
    fn effective_with(account: &str) -> String {
        json!({"schemaVersion":2,"source":"own","accountId":account,"backend":"codex","model":"model-a",
            "modelOptions":{"effort":"high"},"fastMode":false})
        .to_string()
    }
    fn effective() -> String {
        effective_with(&account_id('a'))
    }
    fn record_fails(c: &Connection, a: &AdmissionCapture) {
        let tx = c.unchecked_transaction().unwrap();
        assert!(receipt::record_dispatch(&tx, a).is_err());
        tx.rollback().unwrap();
        assert_eq!(count(c), 0);
    }

    #[test]
    fn carimbo_de_sessao_extra_usa_fonte_propria_sem_cair_no_bot() {
        let bot_stamp = json!({"account":null,"revision":null,"backend":"claude"}).to_string();
        let session_stamp = json!({"account":account_id('a'),"revision":1,"backend":"codex"}).to_string();
        for raw in [None, Some("null".to_owned()), Some(session_stamp.clone())] {
            let f = Fixture::new();
            // O carimbo do bot existe, mas não completa nem substitui o da sessão.
            f.hub.store.kv_set("account.stamp.dev", &bot_stamp).unwrap();
            let (mut a, source) = extra_capture(&f);
            if let Some(raw) = &raw {
                f.hub.store.kv_set(&source, raw).unwrap();
            }
            a.private_stamp = StampCapture::SessionAccountStampV1 { capture: CapturedText::from_captured(raw.clone()) };
            let c = f.hub.store.db.locked();
            // Ausente e texto "null" não se confundem; KV alterado após a captura é conflito.
            let mut swapped = a.clone();
            swapped.private_stamp = StampCapture::SessionAccountStampV1 {
                capture: match &raw {
                    None => CapturedText::Present { value: "null".into() },
                    Some(_) => CapturedText::Absent,
                },
            };
            record_fails(&c, &swapped);
            let mut fallback = a.clone();
            fallback.private_stamp =
                StampCapture::SessionAccountStampV1 { capture: CapturedText::Present { value: bot_stamp.clone() } };
            record_fails(&c, &fallback);
            let before = store_stamp(&c, &source);
            c.execute("INSERT OR REPLACE INTO kv(k,v) VALUES(?1,?2)", params![source, bot_stamp]).unwrap();
            record_fails(&c, &a);
            match &before {
                Some(v) => c.execute("UPDATE kv SET v=?2 WHERE k=?1", params![source, v]).unwrap(),
                None => c.execute("DELETE FROM kv WHERE k=?1", [&source]).unwrap(),
            };
            persist(&c, &a);
            let saved = value(receipt::read_checkpoint(&c, &a.key).unwrap().unwrap().captured());
            assert_eq!(saved, value(&a));
            assert_eq!(saved["privateStamp"]["source"], "sessionAccountStampV1");
            assert_eq!(saved["privateStamp"]["capture"]["state"], if raw.is_none() { "absent" } else { "present" });
        }
        // Campos faltantes não são completados.
        let f = Fixture::new();
        let (mut a, source) = extra_capture(&f);
        let partial = json!({"account":null,"backend":"codex"}).to_string();
        f.hub.store.kv_set(&source, &partial).unwrap();
        a.private_stamp = StampCapture::SessionAccountStampV1 { capture: CapturedText::Present { value: partial } };
        record_fails(&f.hub.store.db.locked(), &a);
        // A extra nunca grava com o carimbo do bot, com ou sem escolha fixada.
        let (mut a, _) = extra_capture(&f);
        a.private_stamp = StampCapture::BotAccountStampV1 { capture: CapturedText::Absent };
        record_fails(&f.hub.store.db.locked(), &a);
        let (a, _) = extra_capture(&f);
        let r = reserve();
        let choose = |c: &Connection, a: &AdmissionCapture, raw: &str| {
            c.execute(
                "UPDATE donna_bot_sessions SET engine_choice=?2 WHERE id=?1",
                params![a.key.destination.session.id, raw],
            )
            .unwrap();
        };
        {
            let c = f.hub.store.db.locked();
            choose(&c, &a, &effective_with(&r.account_id));
            let mut bot_stamped = a.clone();
            bot_stamped.private_stamp = StampCapture::BotAccountStampV1 { capture: CapturedText::Absent };
            record_fails(&c, &bot_stamped);
            // Seleção efetiva diferente da admitida, ilegível (inclusive o formato v1 já migrado) ou ausente: recusa,
            // nada gravado.
            choose(&c, &a, &effective_with(&account_id('b')));
            record_fails(&c, &a);
            choose(&c, &a, "{}");
            record_fails(&c, &a);
            let v1 = json!({"schemaVersion":1,"accountId":r.account_id,
                "selection":{"backend":"codex","model":"model-a","modelOptions":{"effort":"high"}}});
            choose(&c, &a, &v1.to_string());
            record_fails(&c, &a);
            c.execute("UPDATE donna_bot_sessions SET engine_choice=NULL WHERE id=?1", [&a.key.destination.session.id])
                .unwrap();
            record_fails(&c, &a);
            // Efetiva igual à admitida + carimbo da sessão: grava.
            choose(&c, &a, &effective_with(&r.account_id));
            persist(&c, &a);
            assert_eq!(count(&c), 1);
        }
        // Na principal sem seleção própria, a fonte de sessão é recusada mesmo com a mesma chave presente no KV.
        let mut main = capture(&f.hub.store, "dev", &Lane::main("dev"));
        let main_source = "account.stamp.dev/session:dev";
        f.hub.store.kv_set(main_source, "null").unwrap();
        main.private_stamp =
            StampCapture::SessionAccountStampV1 { capture: CapturedText::Present { value: "null".into() } };
        let c = f.hub.store.db.locked();
        let tx = c.unchecked_transaction().unwrap();
        assert!(receipt::record_dispatch(&tx, &main).is_err());
        tx.rollback().unwrap();
        assert_eq!(count(&c), 1);
    }

    /// D372 (decisão A): a principal do Pessoal com seleção efetiva própria grava o carimbo da própria sessão
    /// (`account.stamp.<bot>/session:<bot>`), nunca o do bot; a efetiva precisa ser igual à admitida e ter conta.
    #[test]
    fn principal_com_selecao_propria_usa_o_carimbo_da_sessao() {
        let f = Fixture::new();
        let source = "account.stamp.dev/session:dev";
        let session_stamp = json!({"account":account_id('a'),"revision":1,"backend":"codex"}).to_string();
        f.hub.store.kv_set(source, &session_stamp).unwrap();
        let mut a = capture(&f.hub.store, "dev", &Lane::main("dev"));
        a.private_stamp =
            StampCapture::SessionAccountStampV1 { capture: CapturedText::Present { value: session_stamp } };
        let c = f.hub.store.db.locked();
        let set = |raw: Option<String>| {
            c.execute("UPDATE donna_bot_sessions SET engine_choice=?1 WHERE id='dev'", [raw]).unwrap();
        };
        // Sem seleção: o carimbo de sessão é recusado na principal.
        record_fails(&c, &a);
        // Seleção efetiva diferente da admitida, ou sem conta: recusa.
        set(Some(effective_with(&account_id('b'))));
        record_fails(&c, &a);
        let mut legacy: Value = serde_json::from_str(&effective()).unwrap();
        legacy["accountId"] = Value::Null;
        set(Some(legacy.to_string()));
        record_fails(&c, &a);
        // Com a efetiva igual à admitida, o carimbo do bot é recusado e o da sessão grava.
        set(Some(effective()));
        let mut bot_stamped = a.clone();
        bot_stamped.private_stamp = StampCapture::BotAccountStampV1 { capture: CapturedText::Absent };
        record_fails(&c, &bot_stamped);
        persist(&c, &a);
        let saved = value(receipt::read_checkpoint(&c, &a.key).unwrap().unwrap().captured());
        assert_eq!(saved["privateStamp"]["source"], "sessionAccountStampV1");
    }

    #[tokio::test]
    async fn recibo_do_turno_para_o_app_projeta_sem_privados_e_falha_fechado() {
        let f = Fixture::new();
        let a = capture(&f.hub.store, "dev", &Lane::main("dev"));
        persist(&f.hub.store.db.locked(), &a);
        let entry = a.intent.entry_ids[0].clone();
        let body = |bot: &str, session: &str, entry: &str| json!({"botId":bot,"spaceId":PERSONAL,"hubId":f.hub.host_id,"conversationId":"dev","sessionId":session,"entryId":entry});
        let receipt = |b: Value| dispatch(&f.hub, &Caller::Local, "donnaRoutingReceipt", b);
        let routed = receipt(body("dev", "dev", &entry)).await.unwrap();
        assert_eq!(routed["state"], "routed");
        assert_eq!(routed["receipt"]["admitted"]["selection"]["model"], "model-a");
        assert_eq!(routed["receipt"]["decidedBy"], "fixo");
        assert_eq!(routed["receipt"]["destination"]["session"]["chatId"], "dev");
        let raw = routed.to_string();
        for private in [
            "executionToken",
            "fixture-token",
            "binding",
            "privateStamp",
            "executionSeal",
            "intent",
            "Provenance",
            "origin",
        ] {
            assert!(!raw.contains(private), "{private} vazou: {raw}");
        }
        // Fila sem recibo: pending; enviada sem recibo: notRouted.
        let lane = Lane::main("dev");
        let queued = f.hub.store.insert_entry(&lane, EntryKind::User, 1, &json!({"status":"queued"})).unwrap();
        let sent = f.hub.store.insert_entry(&lane, EntryKind::User, 1, &json!({"status":"sent"})).unwrap();
        let pending = receipt(body("dev", "dev", &queued.id)).await.unwrap();
        assert_eq!((pending["state"].clone(), pending["receipt"].clone()), (json!("pending"), Value::Null));
        let legacy = receipt(body("dev", "dev", &sent.id)).await.unwrap();
        assert_eq!((legacy["state"].clone(), legacy["receipt"].clone()), (json!("notRouted"), Value::Null));
        // Entrada de outro bot ou de outra sessão, e caller sem engine.accounts: Err.
        assert!(receipt(body("other", "dev", &entry)).await.is_err());
        assert!(receipt(body("dev", "outra-sessao", &entry)).await.is_err());
        let device = Caller::Device {
            key: "unproved".into(),
            scopes: vec![Scope::Control, Scope::OwnerDevices],
            autoridade: None,
        };
        assert!(api::call(&f.hub, &device, "donnaRoutingReceipt", body("dev", "dev", &entry)).await.is_err());
        // Conversa de Space cujo ID difere do executor: lida com o conversationId dela; divergente ⇒ Err.
        let created = dispatch(
            &f.hub,
            &Caller::Local,
            "donnaConversationCreate",
            json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"botId":"dev","title":"Fixture"}),
        )
        .await
        .unwrap();
        let chat = created["conversation"]["id"].as_str().unwrap().to_owned();
        let stamp =
            json!({"account":null,"revision":null,"backend":"codex","model":null,"modelOptions":{}}).to_string();
        f.hub.store.kv_set(&format!("donna.space.engine.{chat}"), &stamp).unwrap();
        let mut s = capture(&f.hub.store, "dev", &Lane::main(&chat));
        s.private_stamp = StampCapture::SpaceEngineStampV1 { capture: CapturedText::Present { value: stamp } };
        persist(&f.hub.store.db.locked(), &s);
        let mut space_body = body("dev", &chat, &s.intent.entry_ids[0]);
        space_body["conversationId"] = chat.clone().into();
        assert_eq!(receipt(space_body.clone()).await.unwrap()["receipt"]["destination"]["session"]["chatId"], chat);
        space_body["conversationId"] = "dev".into();
        assert!(receipt(space_body).await.is_err());
        // Outra linha da sessão com entryIds válido, sem esta entrada e corrompida no resto, não
        // afeta este recibo; o recibo dela é que falha.
        let mut other = capture(&f.hub.store, "dev", &Lane::main("dev"));
        other.key.execution_token = "fixture-token-outro".into();
        persist(&f.hub.store.db.locked(), &other);
        f.hub
            .store
            .db
            .locked()
            .execute(
                "UPDATE donna_routing_checkpoints SET checkpoint_json=json_set(checkpoint_json,'$.schemaVersion',9)
                 WHERE execution_token='fixture-token-outro'",
                [],
            )
            .unwrap();
        assert_eq!(receipt(body("dev", "dev", &entry)).await.unwrap()["state"], "routed");
        assert!(receipt(body("dev", "dev", &other.intent.entry_ids[0])).await.is_err());
        // Checkpoint ilegível (válido como JSON ou não) nunca vira "sem recibo", nem com a
        // entrada fora do documento ('{}' e '{"intent":{}}').
        // Estrutura sem filtro possível só na OUTRA linha já recusa a sessão; o candidato
        // corrompido (por último, em todas) também.
        for (corrupt, only) in [
            ("'{x'", " WHERE execution_token='fixture-token-outro'"),
            ("'{}'", " WHERE execution_token='fixture-token-outro'"),
            ("'{\"intent\":{}}'", " WHERE execution_token='fixture-token-outro'"),
            ("json_set(checkpoint_json,'$.schemaVersion',9)", ""),
        ] {
            f.hub
                .store
                .db
                .locked()
                .execute(&format!("UPDATE donna_routing_checkpoints SET checkpoint_json={corrupt}{only}"), [])
                .unwrap();
            let error = receipt(body("dev", "dev", &entry)).await.unwrap_err().to_string();
            assert!(error.contains("routing_receipt_unavailable") && !error.contains("fixture"), "{error}");
        }
    }

    #[tokio::test]
    async fn captura_space_e_selo_opaco_conservam_origem_sem_atestar_cobertura() {
        let f = Fixture::new();
        let result = dispatch(
            &f.hub,
            &Caller::Local,
            "donnaConversationCreate",
            json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"botId":"dev","title":"Fixture"}),
        )
        .await
        .unwrap();
        let chat = result["conversation"]["id"].as_str().unwrap();
        let raw = json!({"account":null,"revision":null,"backend":"claude","model":null,"modelOptions":{}}).to_string();
        f.hub.store.kv_set(&format!("donna.space.engine.{chat}"), &raw).unwrap();
        let mut a = capture(&f.hub.store, "dev", &Lane::main(chat));
        let seal = "opaque_Original-Bytes";
        a.private_stamp = StampCapture::SpaceEngineStampV1 { capture: CapturedText::Present { value: raw.clone() } };
        a.execution_seal =
            ExecutionSeal::SpaceExecutionStampV1 { capture: CapturedText::Present { value: seal.into() } };
        let c = f.hub.store.db.locked();
        persist(&c, &a);
        let saved = receipt::read_checkpoint(&c, &a.key).unwrap().unwrap();
        let json = value(saved.captured());
        assert_eq!(json["privateStamp"]["capture"]["value"], raw);
        assert_eq!(json["executionSeal"]["capture"]["value"], seal);
        assert_eq!(json["routing"]["requiredFacts"]["coverageBefore"], "requiredFactNotAttested");
        assert_eq!(json["routing"]["requiredFacts"]["coverageAfter"], "requiredFactNotAttested");
        assert_eq!(json["routing"]["requiredFacts"]["accountIdentityGenerationCatalog"], "requiredFactNotAttested");
        assert_eq!(json["routing"]["sessionOrigin"]["kind"], "owner");
        assert_eq!(json["routing"]["turnProvenance"]["entries"][0]["evidence"]["source"], "hubEntrySourceBotV1");
        assert_eq!(json["routing"]["turnProvenance"]["authorityOrigin"], "requiredFactNotAttested");
        let dto = serde_json::to_string(&saved.receipt()).unwrap();
        for private in
            [seal, "executionToken", "bindingRef", "privateStamp", "executionSeal", "turnProvenance", "PRIVATE_"]
        {
            assert!(!dto.contains(private));
        }
        for stamp in [
            StampCapture::BotAccountStampV1 { capture: CapturedText::Absent },
            StampCapture::SessionAccountStampV1 { capture: CapturedText::Absent },
        ] {
            let mut wrong = a.clone();
            wrong.key.execution_token = "another-fixture-token".into();
            wrong.private_stamp = stamp;
            let tx = c.unchecked_transaction().unwrap();
            assert!(receipt::record_dispatch(&tx, &wrong).is_err());
            tx.rollback().unwrap();
            assert_eq!(count(&c), 1);
        }
    }

    #[tokio::test]
    async fn principal_real_em_conversa_criada_pela_api_em_outro_space_sem_inferir_authority() {
        let f = Fixture::new();
        f.hub.store.set_team_settings(&json!({"primaryBotId":"dev"})).unwrap();
        let mut spaces = Vec::new();
        for name in ["Trabalho", "Outro"] {
            let before = dispatch(&f.hub, &Caller::Local, "donnaSpaces", json!({"hubId":f.hub.host_id})).await.unwrap();
            let created = dispatch(
                &f.hub,
                &Caller::Local,
                "donnaSpaceCreate",
                json!({"hubId":f.hub.host_id,"expectedRevision":before["revision"],"name":name,
                    "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}}}),
            )
            .await
            .unwrap();
            let space = created["spaces"].as_array().unwrap().iter().find(|s| s["name"] == name).unwrap();
            spaces.push(space["id"].as_str().unwrap().to_owned());
        }
        let work = &spaces[0];
        assert!(
            dispatch(
                &f.hub,
                &Caller::Local,
                "donnaConversationCreate",
                json!({"hubId":f.hub.host_id,"spaceId":work,"botId":"other"}),
            )
            .await
            .is_err()
        );
        let created = dispatch(
            &f.hub,
            &Caller::Local,
            "donnaConversationCreate",
            json!({"hubId":f.hub.host_id,"spaceId":work,"botId":"dev"}),
        )
        .await
        .unwrap();
        let chat = created["conversation"]["id"].as_str().unwrap();
        assert_eq!(created["conversation"]["botId"], "dev");
        assert_eq!(created["conversation"]["spaceId"], *work);
        assert_eq!(f.hub.store.bot("dev").unwrap().unwrap().config.space_id, PERSONAL);
        let mut a = capture(&f.hub.store, "dev", &Lane::main(chat));
        a.key.destination.space_id.clone_from(work);
        let raw = f.hub.store.kv_read(&format!("donna.space.engine.{chat}")).unwrap();
        a.private_stamp = StampCapture::SpaceEngineStampV1 { capture: CapturedText::from_captured(raw) };
        let mut invalid = Vec::new();
        for bot in ["other", "forged"] {
            let mut b = a.clone();
            b.key.destination.bot_id = bot.into();
            invalid.push(b);
        }
        let mut b = a.clone();
        b.key.destination.space_id.clone_from(&spaces[1]);
        invalid.push(b);
        let mut b = a.clone();
        b.key.destination.session.chat_id = "dev".into();
        invalid.push(b);
        {
            let mut c = f.hub.store.db.locked();
            for b in invalid {
                let tx = c.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
                assert!(receipt::record_dispatch(&tx, &b).is_err());
                tx.rollback().unwrap();
            }
            assert_eq!(count(&c), 0);
        }
        // A captura não muda para salvar a intenção quando o principal real é substituído.
        f.hub.store.sem_donna_fixa(); // D366: exercita a troca/exclusão legada, sem Donna fixa.
        f.hub.store.set_team_settings(&json!({"primaryBotId":"other"})).unwrap();
        {
            let mut c = f.hub.store.db.locked();
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
            assert!(receipt::record_dispatch(&tx, &a).is_err());
            tx.rollback().unwrap();
            assert_eq!(count(&c), 0);
        }
        f.hub.store.set_team_settings(&json!({"primaryBotId":"dev"})).unwrap();
        let mut c = f.hub.store.db.locked();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
        receipt::record_dispatch(&tx, &a).unwrap();
        tx.commit().unwrap();
        assert_eq!(count(&c), 1);
        assert_eq!(value(receipt::read_checkpoint(&c, &a.key).unwrap().unwrap().captured()), value(&a));
    }

    #[test]
    fn preimagem_record_antes_de_mutacoes_reais_na_tx_immediate_rollback_e_commit() {
        let f = Fixture::new();
        let stamp_key = "account.stamp.dev";
        let pre_stamp = json!({"account":null,"revision":null,"backend":"claude"}).to_string();
        let admitted_stamp = json!({"account":account_id('a'),"revision":1,"backend":"codex"}).to_string();
        f.hub.store.kv_set(stamp_key, &pre_stamp).unwrap();
        let a = capture(&f.hub.store, "dev", &Lane::main("dev"));
        let original = value(&a);
        let mut c = f.hub.store.db.locked();
        let before = legacy(&c);
        for abort_after_writes in [true, false] {
            let result = (|| -> anyhow::Result<()> {
                let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
                // Mesmo argumento capturado, antes de toda mutação controlada pelo teste.
                receipt::record_dispatch(&tx, &a)?;
                let revision = crate::store::next_rev(&tx)?;
                let changed = tx.execute(
                    "UPDATE donna_bot_sessions SET revision=?2 WHERE id=?1 AND revision=?3 AND state='open'",
                    params![a.key.destination.session.id, revision, a.intent.expected_session_revision],
                )?;
                anyhow::ensure!(changed == 1, "fixture_session_cas_failed");
                let changed = tx.execute("UPDATE kv SET v=?2 WHERE k=?1", [stamp_key, &admitted_stamp])?;
                anyhow::ensure!(changed == 1, "fixture_stamp_missing");
                let changed = tx.execute(
                    "UPDATE entries SET data=json_set(data,'$.status','sent'),rev=?2
                     WHERE id=?1 AND json_extract(data,'$.status')='queued'",
                    params![a.intent.entry_ids[0], revision],
                )?;
                anyhow::ensure!(changed == 1, "fixture_entry_precondition_failed");
                assert_eq!(count(&tx), 1);
                assert_eq!(value(receipt::read_checkpoint(&tx, &a.key)?.unwrap().captured()), original);
                if abort_after_writes {
                    anyhow::bail!("fixture_external_failure_after_record_and_writes");
                }
                tx.commit()?;
                Ok(())
            })();
            assert!(c.is_autocommit());
            if abort_after_writes {
                assert!(result.is_err());
                assert_eq!(legacy(&c), before);
                assert_eq!(count(&c), 0);
            } else {
                result.unwrap();
                assert_eq!(count(&c), 1);
                assert_eq!(store_stamp(&c, stamp_key).as_deref(), Some(admitted_stamp.as_str()));
                let current = sessoes::store::get(&c, "dev").unwrap().unwrap();
                assert_ne!(current.revision, a.intent.expected_session_revision);
                let changes = c.total_changes();
                let checkpoint = receipt::read_checkpoint(&c, &a.key).unwrap().unwrap();
                assert_eq!(value(checkpoint.captured()), original);
                assert_eq!(c.total_changes(), changes);
            }
        }
        // Apenas atomicidade/ordem SQL: não há engine_choice, claim VG, inflight, Authority ou envio.
    }

    #[test]
    fn alteracao_por_outra_conexao_antes_da_tx_immediate_recusa_preimagem_original() {
        let f = Fixture::new();
        for field in ["revision", "stamp"] {
            let path = f.root.join(format!("preimage-peer-{field}.sqlite"));
            let store = Store::open(&path).unwrap();
            store.save_bot(&f.hub.store.bot("dev").unwrap().unwrap().config).unwrap();
            store.kv_set(engine_accounts::SLOT, &f.hub.store.kv_read(engine_accounts::SLOT).unwrap().unwrap()).unwrap();
            let stamp = json!({"account":null,"revision":null,"backend":"codex"}).to_string();
            store.kv_set("account.stamp.dev", &stamp).unwrap();
            let a = capture(&store, "dev", &Lane::main("dev"));
            let original = value(&a);
            let peer = Connection::open(&path).unwrap();
            if field == "revision" {
                peer.execute("UPDATE donna_bot_sessions SET revision=revision+1 WHERE id='dev'", []).unwrap();
            } else {
                peer.execute("UPDATE kv SET v='null' WHERE k='account.stamp.dev'", []).unwrap();
            }
            let mut c = store.db.locked();
            let external_state = legacy(&c);
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
            assert!(receipt::record_dispatch(&tx, &a).is_err());
            tx.rollback().unwrap();
            assert_eq!(value(&a), original);
            assert_eq!(legacy(&c), external_state);
            assert_eq!(count(&c), 0);
        }
    }

    #[test]
    fn codec_fechado_ausente_nao_e_null_nem_fato_nao_atestado() {
        let f = Fixture::new();
        let a = capture(&f.hub.store, "dev", &Lane::main("dev"));
        let original = value(&a);
        for (reason, wire) in [
            (Reason::Fixed, "fixedSelection"),
            (Reason::Explicit, "explicitSelection"),
            (Reason::FrozenSession, "frozenSessionSelection"),
        ] {
            let persisted = json!(wire);
            assert_eq!(serde_json::to_value(reason).unwrap(), persisted);
            let restored: Reason = serde_json::from_value(persisted.clone()).unwrap();
            assert_eq!(serde_json::to_value(restored).unwrap(), persisted);
        }
        let mut invalid = Vec::new();
        for path in ["coverageBefore", "coverageAfter", "accountIdentityGenerationCatalog"] {
            for replacement in [Value::Null, json!(false), json!(0), json!("unknown"), json!("full")] {
                let mut b = original.clone();
                b["routing"]["requiredFacts"][path] = replacement;
                invalid.push(b);
            }
            let mut b = original.clone();
            b["routing"]["requiredFacts"].as_object_mut().unwrap().remove(path);
            invalid.push(b);
        }
        let mut b = original.clone();
        b["binding"].as_object_mut().unwrap().remove("bindingRef");
        invalid.push(b);
        let mut b = original.clone();
        b["routing"]["admitted"]["selection"].as_object_mut().unwrap().remove("model");
        invalid.push(b);
        let mut b = original.clone();
        b["routing"]["admitted"]["selection"]["approved"] = json!(true);
        invalid.push(b);
        let mut b = original.clone();
        b["schemaVersion"] = json!(2);
        invalid.push(b);
        let mut b = original.clone();
        b["audience"] = json!("livre");
        invalid.push(b);
        // Mesmo caminho de `receipt::load`: o codec recusa a forma; `validate`, o schema.
        for b in invalid {
            let error = decode::<AdmissionCapture>(&b.to_string(), DOCUMENT_MAX)
                .and_then(|c| c.validate())
                .err()
                .unwrap()
                .to_string();
            assert_eq!(error, "routing_document_invalid");
        }
        let decoded: AdmissionCapture = decode(&original.to_string(), DOCUMENT_MAX).unwrap();
        assert_eq!(value(&decoded), original);
        let duplicate =
            original.to_string().replacen("\"schemaVersion\":1", "\"schemaVersion\":1,\"schemaVersion\":1", 1);
        assert!(decode::<AdmissionCapture>(&duplicate, DOCUMENT_MAX).is_err());
        let raw = original.to_string();
        let at_limit = format!("{raw}{}", " ".repeat(DOCUMENT_MAX - raw.len()));
        assert!(decode::<AdmissionCapture>(&at_limit, DOCUMENT_MAX).is_ok());
        assert!(decode::<AdmissionCapture>(&" ".repeat(DOCUMENT_MAX + 1), DOCUMENT_MAX).is_err());
        let mut b = a.clone();
        b.intent.entry_ids = vec!["entry".into(); ENTRIES_MAX + 1];
        assert!(b.validate().is_err());
        let mut b = a.clone();
        b.binding.binding_ref = "x".repeat(257);
        assert!(b.validate().is_err());
        let mut b = a;
        b.intent.expected_session_revision = 0;
        assert!(b.validate().is_err());
    }

    #[test]
    fn revisoes_canonicas_so_na_transacao_e_politica_alterada_recusa_gravacao() {
        let f = Fixture::new();
        let mut a = capture(&f.hub.store, "dev", &Lane::main("dev"));
        let c = f.hub.store.db.locked();
        assert!(receipt::revisions(&c, PERSONAL, "dev").is_err());
        let guide = |text: &str| Policy { guide: text.into(), ..Policy::default() };
        let tx = c.unchecked_transaction().unwrap();
        api::save(&tx, PERSONAL, "dev", 0, 0, guide("a"), "geral".into()).unwrap();
        api::save(&tx, PERSONAL, "dev", 1, 1, guide("b"), "geral".into()).unwrap();
        let r = receipt::revisions(&tx, PERSONAL, "dev").unwrap();
        assert_eq!((r.policy, r.general_guide), (2, 1));
        a.routing.policy_revision = r.policy;
        a.routing.bot_guide_revision = r.policy;
        a.routing.general_guide_revision = r.general_guide;
        let mut ok = a.clone();
        ok.key.execution_token = "fixture-token-ok".into();
        receipt::record_dispatch(&tx, &ok).unwrap();
        // Política alterada depois da captura: a gravação recusa (routing_policy_conflict).
        api::save(&tx, PERSONAL, "dev", 2, 1, guide("c"), "geral".into()).unwrap();
        assert!(receipt::record_dispatch(&tx, &a).is_err());
        tx.rollback().unwrap();
        assert_eq!(count(&c), 0);
    }

    #[test]
    fn resume_classificacao_canonica_um_caso_por_ramo() {
        let f = Fixture::new();
        let a = capture(&f.hub.store, "dev", &Lane::main("dev"));
        let c = f.hub.store.db.locked();
        persist(&c, &a);
        let mut absent = a.key.clone();
        absent.execution_token = "fixture-token-absent".into();
        let routed = RoutingClass::Routed {};
        let legacy = RoutingClass::NotRouted { reason: NotRoutedReason::PreRoteamento };
        assert!(receipt::classify_resume(&c, &a.key, Some(&routed)).is_err());
        let tx = c.unchecked_transaction().unwrap();
        let error = |key: &CheckpointKey, class: Option<&RoutingClass>| {
            receipt::classify_resume(&tx, key, class).err().unwrap().to_string()
        };
        match receipt::classify_resume(&tx, &a.key, Some(&routed)).unwrap() {
            ResumeRoute::Routed(checkpoint) => assert_eq!(value(checkpoint.captured()), value(&a)),
            ResumeRoute::Legacy => panic!("routed_virou_legado"),
        }
        assert_eq!(error(&absent, Some(&routed)), "routing_resume_checkpoint_missing");
        assert!(matches!(receipt::classify_resume(&tx, &absent, Some(&legacy)).unwrap(), ResumeRoute::Legacy));
        assert_eq!(error(&a.key, Some(&legacy)), "routing_resume_contradiction");
        assert_eq!(error(&a.key, None), "routing_class_missing");
        tx.execute("UPDATE donna_routing_checkpoints SET checkpoint_json='{}'", []).unwrap();
        assert_eq!(error(&a.key, Some(&routed)), "routing_checkpoint_unavailable");
        assert_eq!(error(&a.key, Some(&legacy)), "routing_checkpoint_unavailable");
        tx.rollback().unwrap();
    }

    #[test]
    fn classe_na_admissao_um_caso_por_ramo() {
        let f = Fixture::new();
        let cfg = f.hub.store.bot("dev").unwrap().unwrap().config;
        let r = reserve();
        let id = r.account_id.clone();
        let c = f.hub.store.db.locked();
        let class =
            |cfg: &BotConfig, human: bool, principal: bool, account: Option<(&str, u64)>, sel: &EngineSelection| {
                receipt::admission_class(&c, cfg, human, principal, account, sel)
            };
        let unavailable = |result: anyhow::Result<RoutingClass>| {
            assert_eq!(result.unwrap_err().to_string(), "routing_selection_unavailable");
        };
        assert!(class(&cfg, true, true, Some((&id, 1)), &r.selection).is_err());
        let _tx = c.unchecked_transaction().unwrap();
        let unproven = RoutingClass::NotRouted { reason: NotRoutedReason::UnprovenSelection };
        let mut other_space = cfg.clone();
        other_space.space_id = "outro-space".into();
        let mut command = cfg.clone();
        command.command = Some("fixture-cli".into());
        let mut default_model = r.selection.clone();
        default_model.model = None;
        default_model.model_options.clear();
        let missing = account_id('f');
        assert_eq!(
            class(&cfg, false, true, Some((&id, 1)), &r.selection).unwrap(),
            RoutingClass::NotRouted { reason: NotRoutedReason::NotHumanTurn }
        );
        assert_eq!(class(&other_space, true, true, Some((&id, 1)), &r.selection).unwrap(), unproven);
        assert_eq!(class(&cfg, true, true, None, &r.selection).unwrap(), unproven);
        assert_eq!(class(&cfg, true, true, Some((&id, 1)), &default_model).unwrap(), unproven);
        unavailable(class(&cfg, true, true, Some((&missing, 1)), &default_model));
        assert_eq!(class(&cfg, true, false, Some((&id, 1)), &default_model).unwrap(), RoutingClass::Routed {});
        unavailable(class(&cfg, true, false, Some((&missing, 1)), &default_model));
        unavailable(class(&cfg, true, false, Some((&id, 2)), &default_model));
        unavailable(class(&command, true, false, Some((&id, 1)), &default_model));
        default_model.model_options.insert("effort".into(), "high".into());
        unavailable(class(&cfg, true, false, Some((&id, 1)), &default_model));
        unavailable(class(&command, true, true, Some((&id, 1)), &r.selection));
        unavailable(class(&cfg, true, true, Some((&id, 2)), &r.selection));
        assert_eq!(class(&cfg, true, false, Some((&id, 1)), &r.selection).unwrap(), RoutingClass::Routed {});
    }

    #[test]
    fn classe_de_roteamento_json_fechado() {
        for (class, raw) in [
            (RoutingClass::Routed {}, json!({"kind":"routed"})),
            (
                RoutingClass::NotRouted { reason: NotRoutedReason::PreRoteamento },
                json!({"kind":"notRouted","reason":"preRoteamento"}),
            ),
            (
                RoutingClass::NotRouted { reason: NotRoutedReason::UnprovenSelection },
                json!({"kind":"notRouted","reason":"unprovenSelection"}),
            ),
            (
                RoutingClass::NotRouted { reason: NotRoutedReason::NotHumanTurn },
                json!({"kind":"notRouted","reason":"notHumanTurn"}),
            ),
        ] {
            assert_eq!(serde_json::to_value(&class).unwrap(), raw);
            let back: RoutingClass = serde_json::from_value(raw.clone()).unwrap();
            assert_eq!(serde_json::to_value(&back).unwrap(), raw);
        }
        for raw in [
            json!({"kind":"notRouted","reason":"outra"}),
            json!({"kind":"notRouted","reason":"principalSemContaGerenciada"}),
            json!({"kind":"notRouted"}),
            json!({"kind":"notRouted","reason":"preRoteamento","extra":1}),
            json!({"kind":"routed","extra":1}),
            json!({"kind":"outro"}),
            json!({}),
        ] {
            assert!(serde_json::from_value::<RoutingClass>(raw).is_err());
        }
    }

    #[test]
    fn documento_corrompido_ou_chave_divergente_nao_reconstroi_nem_regrava() {
        let f = Fixture::new();
        let a = capture(&f.hub.store, "dev", &Lane::main("dev"));
        let c = f.hub.store.db.locked();
        persist(&c, &a);
        for corrupt in ["{PRIVATE_JSON_SENTINEL".into(), {
            let mut b = value(&a);
            b["key"]["destination"]["session"]["chatId"] = json!("other");
            b.to_string()
        }] {
            c.execute("UPDATE donna_routing_checkpoints SET checkpoint_json=?1", [&corrupt]).unwrap();
            let changes = c.total_changes();
            let error = receipt::read_checkpoint(&c, &a.key).err().unwrap().to_string();
            assert!(!error.contains("PRIVATE_"));
            let tx = c.unchecked_transaction().unwrap();
            assert!(receipt::record_dispatch(&tx, &a).is_err());
            tx.rollback().unwrap();
            assert_eq!(c.total_changes(), changes);
            assert_eq!(count(&c), 1);
        }
        // DB adulterada sem o CHECK de tamanho: documento oversize é indisponível, não ausente.
        let raw: String =
            c.query_row("SELECT checkpoint_json FROM donna_routing_checkpoints", [], |r| r.get(0)).unwrap();
        c.execute_batch(
            "ALTER TABLE donna_routing_checkpoints RENAME TO fixture_old;
             CREATE TABLE donna_routing_checkpoints(bot_id TEXT, space_id TEXT, chat_id TEXT, session_id TEXT,
                execution_token TEXT, checkpoint_json TEXT);
             INSERT INTO donna_routing_checkpoints SELECT * FROM fixture_old; DROP TABLE fixture_old;",
        )
        .unwrap();
        c.execute(
            "UPDATE donna_routing_checkpoints SET checkpoint_json=?1",
            [format!("{raw}{}", " ".repeat(DOCUMENT_MAX))],
        )
        .unwrap();
        let error = receipt::read_checkpoint(&c, &a.key).err().unwrap().to_string();
        assert_eq!(error, "routing_checkpoint_unavailable");
        let mut key = a.key.clone();
        key.execution_token = "outra-fixture".into();
        assert!(receipt::read_checkpoint(&c, &key).unwrap().is_none());
        let mut key = a.key.clone();
        key.destination.session.chat_id = "other".into();
        assert!(receipt::read_checkpoint(&c, &key).unwrap().is_none());
    }

    #[test]
    fn leitura_exata_apos_editar_fixo_guias_e_reabrir_db_sem_decisor_nem_write() {
        let f = Fixture::new();
        let path = f.root.join("routing-checkpoint.sqlite");
        let original;
        {
            let store = Store::open(&path).unwrap();
            store.save_bot(&f.hub.store.bot("dev").unwrap().unwrap().config).unwrap();
            store.kv_set(engine_accounts::SLOT, &f.hub.store.kv_read(engine_accounts::SLOT).unwrap().unwrap()).unwrap();
            // Ramo EXPLICITAMENTE legado (carimbo do bot), igual à convenção de `Fixture::new`: este segundo Store
            // criou a principal com retrato D372, que é zerado antes da captura. O caso moderno (carimbo da sessão)
            // e as negativas da matriz seguem em `principal_com_selecao_propria_usa_o_carimbo_da_sessao`.
            let without_choice = "UPDATE donna_bot_sessions SET engine_choice=NULL WHERE principal=1";
            store.db.locked().execute(without_choice, []).unwrap();
            original = capture(&store, "dev", &Lane::main("dev"));
            persist(&store.db.locked(), &original);
            let mut bot = store.bot("dev").unwrap().unwrap().config;
            bot.model = Some("model-b".into());
            bot.fork_editor.engine_account_id = Some(account_id('b'));
            store.save_bot(&bot).unwrap();
            let c = store.db.locked();
            let tx = c.unchecked_transaction().unwrap();
            api::save(
                &tx,
                PERSONAL,
                "dev",
                0,
                0,
                Policy { guide: "guia novo".into(), ..Policy::default() },
                "geral novo".into(),
            )
            .unwrap();
            tx.commit().unwrap();
        }
        let reopened = Store::open(&path).unwrap();
        let c = reopened.db.locked();
        // Cadastro ilegível prova que leitura e repetição não dependem de validar/escolher conta atual.
        c.execute("UPDATE kv SET v='PRIVATE_INVALID_REGISTRY' WHERE k=?1", [engine_accounts::SLOT]).unwrap();
        let before = legacy(&c);
        let changes = c.total_changes();
        let read = receipt::read_checkpoint(&c, &original.key).unwrap().unwrap();
        assert_eq!(value(read.captured()), value(&original));
        let tx = c.unchecked_transaction().unwrap();
        let same = receipt::record_dispatch(&tx, &original).unwrap();
        assert_eq!(value(same.captured()), value(&original));
        tx.commit().unwrap();
        assert_eq!(legacy(&c), before);
        assert_eq!(c.total_changes(), changes);
        assert_eq!(count(&c), 1);
    }

    #[test]
    fn limpeza_transacional_do_hook_remove_so_bot_exato() {
        let f = Fixture::new();
        let a = capture(&f.hub.store, "dev", &Lane::main("dev"));
        let b = capture(&f.hub.store, "other", &Lane::main("other"));
        {
            let c = f.hub.store.db.locked();
            persist(&c, &a);
            persist(&c, &b);
            assert!(receipt::on_bot_deleted(&c, "dev").is_err());
            let tx = c.unchecked_transaction().unwrap();
            crate::fork::on_bot_deleted(&tx, "dev").unwrap();
            assert_eq!(count(&tx), 1);
            tx.rollback().unwrap();
            assert_eq!(count(&c), 2);
        }
        f.hub.store.delete_bot("dev").unwrap();
        let c = f.hub.store.db.locked();
        assert_eq!(count(&c), 1);
        assert!(receipt::read_checkpoint(&c, &a.key).unwrap().is_none());
        assert_eq!(value(receipt::read_checkpoint(&c, &b.key).unwrap().unwrap().captured()), value(&b));
    }
}
