#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::{
    api,
    autoridade::{self, Authority},
    guia, protocolo as p, runner, store,
    tipos::{AccountState, ConnectionPackage, METHODS, Observation, Operation, PURPOSE, Phase, Place},
    transporte,
};
use crate::{
    LockExt as _,
    api::{devices::Caller, dispatch},
    hub::Hub,
    remote::identity::Identity,
    store::{Scope, Store},
};
use ed25519_dalek::SigningKey;
use serde_json::{Value, json};
use std::{fmt::Write as _, path::PathBuf, sync::Arc};

fn vectors() -> Value {
    serde_json::from_str(include_str!("../../../tests/fixtures/onde_roda_runner_v1.json")).unwrap()
}
#[test]
fn guia_compilado_exige_marcadores_unicos_texto_limitado_e_versao_do_hub() {
    let start = "<!-- donna-workplace-guide:v1:start -->";
    let end = "<!-- donna-workplace-guide:v1:end -->";
    assert_eq!(guia::extract(&format!("antes{start}\nroteiro público\n{end}depois")).unwrap(), "roteiro público");
    for source in [
        String::new(),
        format!("{start}{start}texto{end}"),
        format!("{start}texto{end}{end}"),
        format!("{end}texto{start}"),
        format!("{start}\n \n{end}"),
        format!("{start}{}{end}", "á".repeat(32_769)),
    ] {
        assert!(guia::extract(&source).is_err());
    }
    assert_eq!(guia::extract(&format!("{start}{}{end}", "a".repeat(65_536))).unwrap().len(), 65_536);
    let response = guia::response().unwrap();
    assert_eq!(response["guide"]["schema"], 1);
    assert_eq!(response["guide"]["hubVersion"], env!("CARGO_PKG_VERSION"));
    let text = response["guide"]["text"].as_str().unwrap();
    assert!(
        text.contains("Este roteiro acompanha a versão do Hub")
            && text.contains("runner reconfigure")
            && text.len() <= 65_536
    );
    for internal in ["AGUARDANDO PROVA", "AGUARDA PROVA", "Provas e handoff", "recibos por snapshot"] {
        assert!(!text.contains(internal));
    }
}
#[tokio::test]
async fn guia_local_so_corpo_vazio_sem_efeito_no_cofre_ou_db() {
    let fixture = Fixture::new();
    *fixture.hub.store.secret_key.locked() = None;
    let before = fixture.hub.store.db.locked().total_changes();
    let response = fixture.call("donnaWorkplaceGuide", json!({})).await;
    assert_eq!(response["hubId"], "fixture-hub");
    assert_eq!(response["guide"]["schema"], 1);
    for method in ["donnaWorkplaceGuide", "donnaWorkplaceAccess", "donnaWorkplaceList"] {
        for body in [json!([]), Value::Null, json!({"path":"not-admitted"})] {
            assert!(api::call(&fixture.hub, &Caller::Local, method, body).await.is_err());
        }
    }
    assert_eq!(fixture.hub.store.db.locked().total_changes(), before);
    assert!(fixture.hub.store.secret_key.locked().is_none());
    assert!(fixture.hub.store.kv_read("vault.id").unwrap().is_none());
}
fn place() -> Place {
    Place {
        id: "11111111-1111-4111-8111-111111111111".into(),
        hub_id: "fixture-hub".into(),
        resource_id: "bx_fixture01".into(),
        name: "Oficina".into(),
        revision: 1,
        admission_revision: 1,
        connection_revision: 0,
        capacity_revision: 0,
        removed: false,
        ssh_public: None,
        control_public: None,
        runner_pin: None,
        manifest: None,
        connection: None,
    }
}
struct Fixture {
    hub: Arc<Hub>,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        // AF_UNIX limita o caminho completo; nomes longos impedem até o controle local.
        let root = std::env::temp_dir().join(format!("dwp-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]));
        transporte::private_create(&root).unwrap();
        let hub = Hub::new(
            Store::open(std::path::Path::new(":memory:")).unwrap(),
            "fixture-hub".into(),
            Identity::load_or_create(&root).unwrap(),
            "fixture".into(),
            0,
        );
        Self { hub, root }
    }
    async fn call(&self, method: &str, body: Value) -> Value {
        dispatch(&self.hub, &Caller::Local, method, body).await.unwrap()
    }
    async fn create(&self) -> Value {
        self.call(
            "donnaWorkplaceCreate",
            json!({"clientRequestId":uuid::Uuid::new_v4().to_string(),"target":null,
            "intent":{"type":"create","name":"Oficina","resource_id":"bx_fixture01"}}),
        )
        .await
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn target(p: &Value) -> Value {
    json!({"id":p["id"],"revision":p["revision"],"admissionRevision":p["admissionRevision"],"connectionRevision":p["connectionRevision"]})
}
fn request(p: &Value, intent: &Value) -> Value {
    json!({"clientRequestId":uuid::Uuid::new_v4().to_string(),"target":target(p),"intent":intent})
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(hex, "{byte:02x}").unwrap();
    }
    hex
}

#[test]
fn ambos_lados_consumem_vetores_independentes_e_cada_direcao_comeca_em_um() {
    let fixtures = vectors();
    let hello: p::Hello = serde_json::from_value(fixtures["hello"].clone()).unwrap();
    let welcome: p::Welcome = serde_json::from_value(fixtures["welcome"].clone()).unwrap();
    let transcript = p::transcript(&hello, &welcome).unwrap();
    let hex = hex_bytes(&transcript);
    assert_eq!(hex, fixtures["transcript_hex"]);
    let channel = p::hash(&transcript);
    assert_eq!(p::b64(&channel), fixtures["channel_id"]);
    p::verify(
        &p::fixed(&welcome.runner_key).unwrap(),
        &p::hello_input(p::Direction::Runner, &channel).unwrap(),
        &welcome.signature,
    )
    .unwrap();
    p::verify(
        &p::fixed(&hello.control_key).unwrap(),
        &p::hello_input(p::Direction::Control, &channel).unwrap(),
        fixtures["confirm"]["signature"].as_str().unwrap(),
    )
    .unwrap();
    let mut control = p::Channel::new(
        channel,
        p::Direction::Control,
        SigningKey::from_bytes(&p::fixed(fixtures["control_seed"].as_str().unwrap()).unwrap()),
        p::fixed(&welcome.runner_key).unwrap(),
    );
    let mut runner = p::Channel::new(
        channel,
        p::Direction::Runner,
        SigningKey::from_bytes(&p::fixed(fixtures["runner_seed"].as_str().unwrap()).unwrap()),
        p::fixed(&hello.control_key).unwrap(),
    );
    for item in fixtures["messages"].as_array().unwrap() {
        let envelope: p::Envelope = serde_json::from_value(item["envelope"].clone()).unwrap();
        let payload: p::Payload = serde_json::from_value(item["payload"].clone()).unwrap();
        let hex = hex_bytes(&envelope.input().unwrap());
        assert_eq!(hex, item["preimage_hex"]);
        let (sender, receiver) = if envelope.direction == p::Direction::Control {
            (&mut control, &mut runner)
        } else {
            (&mut runner, &mut control)
        };
        let emitted = sender.seal(envelope.operation_id, &payload).unwrap();
        assert_eq!(serde_json::to_value(&emitted).unwrap(), item["envelope"]);
        assert_eq!(
            serde_json::to_value(receiver.open(&envelope, envelope.operation_id).unwrap()).unwrap(),
            item["payload"]
        );
        assert!(receiver.open(&envelope, envelope.operation_id).is_err());
    }
    let bytes = p::fields(&[&1_u32.to_be_bytes(), &1_u64.to_be_bytes()]).unwrap();
    let hex = hex_bytes(&bytes);
    assert_eq!(hex, fixtures["f_u32_u64_hex"]);
}
#[test]
fn json_estrito_assinaturas_hashes_canal_op_e_sequencia_nao_podem_mudar() {
    let v = vectors();
    for bad in v["negative"].as_array().unwrap() {
        assert!(serde_json::from_str::<p::Envelope>(bad["wire"].as_str().unwrap()).is_err(), "{}", bad["name"]);
    }
    for (field, value) in [
        ("version", json!(2)),
        ("sequence", json!(2)),
        ("direction", json!("runner")),
        ("channel_id", json!(p::b64(&[0; 32]))),
        ("operation_id", json!(uuid::Uuid::new_v4().to_string())),
        ("payload_sha256", json!(p::b64(&[0; 32]))),
        ("signature", json!(p::b64(&[0; 64]))),
        ("payload_b64", json!("AA==")),
    ] {
        let mut raw = v["messages"][0]["envelope"].clone();
        raw[field] = value;
        let e: p::Envelope = serde_json::from_value(raw).unwrap();
        let mut runner = p::Channel::new(
            p::fixed(v["channel_id"].as_str().unwrap()).unwrap(),
            p::Direction::Runner,
            SigningKey::from_bytes(&[0; 32]),
            p::fixed(v["hello"]["control_key"].as_str().unwrap()).unwrap(),
        );
        assert!(
            runner
                .open(
                    &e,
                    uuid::Uuid::parse_str(v["messages"][0]["envelope"]["operation_id"].as_str().unwrap()).unwrap()
                )
                .is_err(),
            "{field}"
        );
    }
    for data in
        [r#"{"type":"inspect","prompt":"não"}"#, r#"{"type":"inspect","type":"inspect"}"#, r#"{"type":"task_grant"}"#]
    {
        assert!(serde_json::from_str::<p::Payload>(data).is_err());
    }
}
#[tokio::test]
async fn frame_limita_antes_de_alocar_e_recusa_utf8_duplicatas() {
    for n in [0, p::FRAME_LIMIT + 1] {
        let bytes = u32::try_from(n).unwrap().to_be_bytes();
        let mut input = &bytes[..];
        assert!(p::read::<_, p::Envelope>(&mut input).await.is_err());
    }
    let mut bytes = 1_u32.to_be_bytes().to_vec();
    bytes.push(255);
    let mut input = &bytes[..];
    assert!(p::read::<_, p::Envelope>(&mut input).await.is_err());
    let v = vectors();
    let raw = v["negative"][0]["wire"].as_str().unwrap().as_bytes();
    let mut bytes = u32::try_from(raw.len()).unwrap().to_be_bytes().to_vec();
    bytes.extend(raw);
    let mut input = &bytes[..];
    assert!(p::read::<_, p::Envelope>(&mut input).await.is_err());
}
#[test]
fn desafio_uso_unico_prazo_proposito_boot_operacao_nonce_e_revisoes() {
    let op = uuid::Uuid::new_v4();
    let intent = uuid::Uuid::new_v4();
    let boot = uuid::Uuid::new_v4();
    let generation = p::b64(&[2; 32]);
    for mutation in 0..9 {
        let mut c = p::Challenge::issue(op, intent, boot, generation.clone(), 3, 2).unwrap();
        let mut grant = json!({"type":"inspect_grant","purpose":PURPOSE,"intent":intent.to_string(),"nonce":c.nonce,
            "boot_id":boot.to_string(),"generation":generation,"duration_ms":60000,"admission_revision":3,"connection_revision":2});
        match mutation {
            1 => grant["purpose"] = json!("task_grant"),
            2 => grant["nonce"] = json!(p::b64(&[0; 32])),
            3 => grant["boot_id"] = json!(uuid::Uuid::new_v4().to_string()),
            4 => grant["generation"] = json!(p::b64(&[9; 32])),
            5 => grant["duration_ms"] = json!(60001),
            6 => grant["admission_revision"] = json!(4),
            7 => grant["intent"] = json!(uuid::Uuid::new_v4().to_string()),
            8 => grant["connection_revision"] = json!(8),
            _ => {}
        }
        let grant: p::Payload = serde_json::from_value(grant).unwrap();
        assert_eq!(c.grant(op, &grant).is_ok(), mutation == 0);
        assert!(c.grant(op, &grant).is_err());
    }
}
#[tokio::test]
async fn toda_origem_nao_local_recusada_antes_de_permit_cofre_e_io() {
    let f = Fixture::new();
    *f.hub.store.secret_key.locked() = None;
    let scopes = [Scope::Control, Scope::Screen, Scope::Executor, Scope::OwnerDevices];
    let mut origins = vec![
        Caller::Pairing { key: "fixture".into() },
        Caller::Bootstrap { key: "fixture".into(), bootstrap_id: "PL1".into() },
    ];
    for mask in 0..16 {
        origins.push(Caller::Device {
            key: "fixture".into(),
            scopes: scopes.iter().enumerate().filter(|(i, _)| mask & (1 << i) != 0).map(|(_, v)| *v).collect(),
            autoridade: None,
        });
    }
    for caller in origins {
        for method in METHODS.iter().copied().chain(["donnaWorkplaceUnknown"]) {
            let err = dispatch(&f.hub, &caller, method, json!({"secret":"not-real"})).await.unwrap_err();
            assert_eq!(err.to_string(), "origin_not_admitted");
        }
    }
    assert!(f.hub.store.secret_key.locked().is_none());
    assert!(f.hub.store.kv_read("vault.id").unwrap().is_none());
    assert_eq!(f.call("donnaWorkplaceList", json!({})).await["places"], json!([]));
    assert!(autoridade::require(&Caller::Local, "donnaWorkplaceUnknown").is_err());
}
#[tokio::test]
async fn cadastro_real_cas_idempotencia_tombstone_e_erro_json_nao_viram_ausencia() {
    let f = Fixture::new();
    let created = f.create().await;
    assert_eq!(created["hubId"], "fixture-hub");
    let place = &created["place"];
    let rename = request(place, &json!({"type":"rename","name":"Outro"}));
    let first = f.call("donnaWorkplaceRename", rename.clone()).await;
    assert_eq!(first, f.call("donnaWorkplaceRename", rename.clone()).await);
    let mut changed = rename.clone();
    changed["intent"]["name"] = json!("Diferente");
    assert!(api::call(&f.hub, &Caller::Local, "donnaWorkplaceRename", changed).await.is_err());
    assert!(
        api::call(
            &f.hub,
            &Caller::Local,
            "donnaWorkplaceRename",
            request(place, &json!({"type":"rename","name":"Perdido"}))
        )
        .await
        .is_err()
    );
    let remove = request(&first["place"], &json!({"type":"remove"}));
    let removed = f.call("donnaWorkplaceRemove", remove.clone()).await;
    assert_eq!(removed, f.call("donnaWorkplaceRemove", remove).await);
    assert!(api::call(&f.hub,&Caller::Local,"donnaWorkplaceCreate",json!({"clientRequestId":uuid::Uuid::new_v4().to_string(),"target":null,"intent":{"type":"create","name":"clone","resource_id":"bx_fixture01"}})).await.is_err());
    f.hub.store.db.locked().execute("UPDATE donna_workplaces SET record='{'", []).unwrap();
    assert!(api::call(&f.hub, &Caller::Local, "donnaWorkplaceList", json!({})).await.is_err());
}
#[test]
fn guarda_original_cerca_remove_rotaciona_e_reinicio_nao_repete() {
    let s = Store::open(std::path::Path::new(":memory:")).unwrap();
    let a = Authority::Local { hub_id: "fixture-hub".into() };
    let mut place = place();
    let op = Operation {
        id: uuid::Uuid::new_v4().to_string(),
        place_id: place.id.clone(),
        hub_id: place.hub_id.clone(),
        admission_revision: 1,
        connection_revision: 0,
        authority: "local".into(),
        intent: uuid::Uuid::new_v4().to_string(),
        phase: Phase::Verificando,
        observed_at: None,
        deadline: crate::store::now_ms() + 60000,
        observation: Observation::default(),
        reason: None,
    };
    store::transaction(&s, |db| {
        store::save(db, &place)?;
        store::save_operation(db, &op)
    })
    .unwrap();
    assert!(store::live(&s, &a, &op.id).is_ok());
    place.connection_revision += 1;
    assert!(a.operation(&place, &op).is_err());
    place.connection_revision -= 1;
    place.admission_revision += 1;
    assert!(a.operation(&place, &op).is_err());
    place.admission_revision -= 1;
    place.removed = true;
    assert!(a.operation(&place, &op).is_err());
    store::migrate(&s.db.locked()).unwrap();
    assert_eq!(store::operation(&s.db.locked(), &op.id).unwrap().phase, Phase::InterrompidaIncerta);
    assert!(store::live(&s, &a, &op.id).is_err());
    assert!(store::runtime_root(&s).is_err());
}
#[test]
fn ultima_consulta_usa_criacao_persistida_sem_empate_relogio_ou_upsert_promoverem_antiga() {
    let fixture = Fixture::new();
    let path = fixture.root.join("creation-order.db");
    let database = Store::open(&path).unwrap();
    let place = place();
    let authority = Authority::Local { hub_id: place.hub_id.clone() };
    let make_operation = |id: &str, deadline| Operation {
        id: id.into(),
        place_id: place.id.clone(),
        hub_id: place.hub_id.clone(),
        admission_revision: place.admission_revision,
        connection_revision: place.connection_revision,
        authority: "local".into(),
        intent: "44444444-4444-4444-8444-444444444444".into(),
        phase: Phase::CanceladaIncerta,
        observed_at: None,
        deadline,
        observation: Observation::default(),
        reason: None,
    };
    // UUID lexicograficamente menor é criado depois, no MESMO milissegundo.
    let mut first = make_operation("ffffffff-ffff-4fff-8fff-ffffffffffff", 60_000);
    let second = make_operation("11111111-1111-4111-8111-111111111111", 60_000);
    let third = make_operation("22222222-2222-4222-8222-222222222222", 59_000);
    store::transaction(&database, |db| {
        store::save(db, &place)?;
        store::save_operation(db, &first)?;
        store::save_operation(db, &second)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(store::latest_operations(&database.db.locked(), &authority).unwrap()[0].id, second.id);
    let first_order: i64 = database
        .db
        .locked()
        .query_row("SELECT creation_order FROM donna_workplace_inspections WHERE id=?", [&first.id], |row| row.get(0))
        .unwrap();
    // A terceira consulta foi criada após um recuo do relógio de parede.
    store::transaction(&database, |db| store::save_operation(db, &third)).unwrap();
    assert_eq!(store::latest_operations(&database.db.locked(), &authority).unwrap()[0].id, third.id);
    first.phase = Phase::Concluida;
    first.deadline = 999_999;
    first.observed_at = Some(999_999);
    store::transaction(&database, |db| store::save_operation(db, &first)).unwrap();
    let after_update: i64 = database
        .db
        .locked()
        .query_row("SELECT creation_order FROM donna_workplace_inspections WHERE id=?", [&first.id], |row| row.get(0))
        .unwrap();
    assert_eq!(after_update, first_order);
    assert_eq!(store::latest_operations(&database.db.locked(), &authority).unwrap()[0].id, third.id);
    drop(database);
    let reopened = Store::open(&path).unwrap();
    let latest = store::latest_operations(&reopened.db.locked(), &authority).unwrap();
    assert_eq!(latest.len(), 1);
    assert_eq!(latest[0].id, third.id);
    assert_eq!(store::operation(&reopened.db.locked(), &first.id).unwrap().phase, Phase::Concluida);
}
#[test]
fn status_do_journal_persistido_so_responde_result_com_conclusao_e_observacao() {
    let fixture = Fixture::new();
    let operation = uuid::Uuid::new_v4();
    for state in ["accepted", "cancelled_uncertain", "complete"] {
        let path = fixture.root.join(format!("{state}.json"));
        let record = json!({"operation": operation.to_string(),"intent":"44444444-4444-4444-8444-444444444444",
            "state":state,"observation":null});
        let bytes = serde_json::to_vec(&record).unwrap();
        transporte::write_private(&path, &bytes).unwrap();
        for _ in 0..2 {
            assert!(runner::status_from_journal(&path, operation).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    }
    let observation = vectors()["messages"][4]["payload"]["observation"].clone();
    let record = json!({"operation":operation.to_string(),"intent":"44444444-4444-4444-8444-444444444444",
        "state":"complete","observation":observation});
    let bytes = serde_json::to_vec(&record).unwrap();
    let path = fixture.root.join("observed.json");
    transporte::write_private(&path, &bytes).unwrap();
    let response = runner::status_from_journal(&path, operation).unwrap();
    assert_eq!(serde_json::to_value(response).unwrap(), json!({"type":"result","observation":observation}));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert!(runner::status_from_journal(&path, uuid::Uuid::new_v4()).is_err());
}
#[test]
fn pin_reservado_permanece_apos_remocao_e_resumos_nao_contem_conexao_ou_cofre() {
    let s = Store::open(std::path::Path::new(":memory:")).unwrap();
    let mut p = place();
    let pin = p::b64(&[5; 32]);
    store::transaction(&s, |db| {
        store::save(db, &p)?;
        store::reserve(db, &p, &pin)
    })
    .unwrap();
    p.removed = true;
    store::transaction(&s, |db| store::save(db, &p)).unwrap();
    p.id = uuid::Uuid::new_v4().to_string();
    p.resource_id = "bx_clone".into();
    p.removed = false;
    assert!(store::transaction(&s, |db| store::reserve(db, &p, &pin)).is_err());
    let summary = p.summary().to_string();
    for forbidden in ["endpoint", "runnerPin", "slot", "known_hosts", "ownerUserId"] {
        assert!(!summary.contains(forbidden));
    }
    let (origin, owner): (String, Option<String>) =
        s.db.locked()
            .query_row("SELECT created_origin,owner_user_id FROM donna_workplaces", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
    assert_eq!(origin, "local");
    assert!(owner.is_none());
}
#[tokio::test]
async fn recupera_material_publico_sem_desbloquear_cofre_e_cancela_sem_resposta_antiga() {
    let f = Fixture::new();
    let mut p = place();
    p.ssh_public = Some("ssh-ed25519 fixture-public".into());
    p.control_public = Some(p::b64(&[3; 32]));
    p.runner_pin = Some(p::b64(&[4; 32]));
    p.manifest = Some(serde_json::from_value(vectors()["welcome"]["manifest"].clone()).unwrap());
    p.connection = Some(ConnectionPackage {
        sandbox_id: p.resource_id.clone(),
        host: "192.0.2.1".into(),
        port: 22,
        known_hosts_line: "fixture".into(),
    });
    store::transaction(&f.hub.store, |db| store::save(db, &p)).unwrap();
    *f.hub.store.secret_key.locked() = None;
    let recovered = f
        .call(
            "donnaWorkplacePrepareConnection",
            request(&p.summary(), &json!({"type":"prepare_connection","rotate":false})),
        )
        .await;
    assert_eq!(recovered["publicMaterial"]["controlPublic"], p.control_public.clone().unwrap());
    assert_eq!(recovered["publicMaterial"]["manifest"], serde_json::to_value(&p.manifest).unwrap());
    assert!(f.hub.store.secret_key.locked().is_none());
    assert!(f.hub.store.kv_read("vault.id").unwrap().is_none());
    let one = request(&p.summary(), &json!({"type":"inspect_start"}));
    let two = request(&p.summary(), &json!({"type":"inspect_start"}));
    let (a, b) = tokio::join!(
        api::call(&f.hub, &Caller::Local, "donnaWorkplaceInspectStart", one),
        api::call(&f.hub, &Caller::Local, "donnaWorkplaceInspectStart", two)
    );
    assert!(a.is_ok());
    assert!(b.is_err());
    let op = a.unwrap()["operation"]["id"].as_str().unwrap().to_owned();
    let cancelled = f
        .call(
            "donnaWorkplaceInspectCancel",
            json!({"operationId":op,"clientRequestId":uuid::Uuid::new_v4().to_string()}),
        )
        .await;
    assert_eq!(cancelled["operation"]["phase"], "cancelada_incerta");
    tokio::task::yield_now().await;
    assert_eq!(
        f.call("donnaWorkplaceInspectStatus", json!({"operationId":op})).await["operation"]["phase"],
        "cancelada_incerta"
    );
}
#[test]
fn selagem_do_cofre_e_pura_e_abertura_estrita_nao_migra_plaintext() {
    let s = Store::open(std::path::Path::new(":memory:")).unwrap();
    let slot = api::slot(&place().id, "control").unwrap();
    let sealed = crate::market::vault::seal(&s, &slot, "FICTIONAL").unwrap();
    assert!(s.kv_read(&slot).unwrap().is_none());
    assert_eq!(crate::market::vault::open_ciphertext(&s, &slot, &sealed).unwrap(), "FICTIONAL");
    assert!(crate::market::vault::open_ciphertext(&s, &slot, "FICTIONAL").is_err());
    assert!(s.kv_read(&slot).unwrap().is_none());
    assert!(crate::market::vault::open_ciphertext(&s, "other-slot", &sealed).is_err());
}
#[test]
fn pacote_boat_estrito_e_api_key_nunca_assinatura() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let mut key = b"\0\0\0\x0bssh-ed25519\0\0\0\x20".to_vec();
    key.extend(SigningKey::from_bytes(&[7; 32]).verifying_key().as_bytes());
    let package = ConnectionPackage {
        sandbox_id: "bx_fixture01".into(),
        host: "2001:db8::1".into(),
        port: 22,
        known_hosts_line: format!("bx_fixture01 ssh-ed25519 {}", STANDARD.encode(key)),
    };
    package.validate("bx_fixture01").unwrap();
    for host in ["-oProxyCommand=sh", "x\nInclude evil", "[2001:db8::1]:22", "a b", "$(id)", "a/b", "x%h"] {
        let mut bad = package.clone();
        bad.host = host.into();
        assert!(bad.validate("bx_fixture01").is_err());
    }
    for suffix in ["\n", " comment", "\r", "\nProxyCommand x"] {
        let mut bad = package.clone();
        bad.known_hosts_line += suffix;
        assert!(bad.validate("bx_fixture01").is_err());
    }
    assert!(package.validate("bx_other").is_err());
    let mut raw = serde_json::to_value(&package).unwrap();
    raw["pin"] = json!("extra");
    assert!(serde_json::from_value::<ConnectionPackage>(raw).is_err());
    assert_eq!(
        runner::account_state(&json!({"account":{"type":"apiKey","planType":"pro"}})),
        AccountState::ApiNaoAdmitida
    );
    assert_eq!(
        runner::account_state(&json!({"account":{"type":"chatgpt","planType":"pro","email":"never-projected"}})),
        AccountState::AssinaturaOficial
    );
    assert_eq!(runner::account_state(&json!({"account":{"type":"chatgpt"}})), AccountState::NaoVerificado);
    assert_eq!(runner::account_state(&json!({"account":null})), AccountState::Desconectado);
    assert_eq!(runner::account_state(&json!({})), AccountState::Indisponivel);
}
#[test]
fn sonda_unix_erro_de_diretorio_nao_prova_politica_e_arquivos_sao_isolados() {
    let f = Fixture::new();
    let root = f.root.join("runtime");
    let dir = transporte::PrivateDir::hub(&root).unwrap();
    let path = dir.file("identity", b"fictional").unwrap();
    assert!(path.starts_with(&f.root));
    assert!(path.exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
    }
    drop(dir);
    assert!(!path.exists());
    let transcript = "Authenticated to fixture\ndebug1: remote forward failure for: listen /nonexistent/probe\nError: remote port forwarding failed for listen path /nonexistent/probe\n";
    assert!(!transporte::remote_socket_denied(false, "/nonexistent/probe", false, "", transcript));
    assert!(!transporte::denied(
        transporte::Probe::OtherTarget,
        false,
        "",
        "Authenticated to fixture\nconnect failed: Connection refused"
    ));
}
#[cfg(unix)]
#[test]
fn sonda_unix_uid_distinto_ou_root_nao_comprova_permissao_da_conta_ssh() {
    let listen = "/fixture/listen.sock";
    let transcript = format!(
        "Authenticated to fixture\nremote forward failure for: listen {listen}\nremote port forwarding failed for listen path {listen}\n"
    );
    for (effective_uid, ssh_uid) in [(0, Some(1001)), (0, Some(0)), (1002, Some(1001)), (1001, None)] {
        let ready = runner::probe_uid_matches(effective_uid, ssh_uid);
        assert!(!ready);
        assert!(!transporte::remote_socket_denied(ready, listen, false, "", &transcript));
    }
    assert!(runner::probe_uid_matches(1001, Some(1001)));
}
#[test]
fn nenhuma_ferramenta_call_encaminha_workplace_e_runner_entra_antes_do_hub() {
    let main = include_str!("../../main.rs");
    assert!(main.contains("Sub::Runner { action } => fork::onde_roda::runner::run(action).await"));
    for source in [include_str!("../../mcp.rs"), include_str!("../../api/mod.rs")] {
        assert!(!source.contains("\"donnaWorkplaceCall\""));
    }
    let runner = include_str!("runner.rs");
    for forbidden in [
        "engine_accounts::",
        "resolve_for_bot(",
        "open_store(",
        "model/list",
        "thread/start",
        "turn/start",
        "account/login",
        "account/logout",
    ] {
        assert!(!runner.contains(forbidden), "{forbidden}");
    }
}

#[cfg(unix)]
fn fake(f: &Fixture, name: &str) -> PathBuf {
    let path = f.root.join(name);
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    crate::fork::test_support::write_executable(&path, &std::fs::read(source).unwrap(), 0o700);
    path
}
#[cfg(unix)]
#[tokio::test]
async fn ssh_falso_exige_pin_e_sentinelas_sem_servico_real() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let f = Fixture::new();
    let executable = fake(&f, "onde_roda_ssh_fake.py");
    let mut key = b"\0\0\0\x0bssh-ed25519\0\0\0\x20".to_vec();
    key.extend(SigningKey::from_bytes(&[7; 32]).verifying_key().as_bytes());
    let package = ConnectionPackage {
        sandbox_id: "bx_fixture01".into(),
        host: "192.0.2.1".into(),
        port: 22,
        known_hosts_line: format!("bx_fixture01 ssh-ed25519 {}", STANDARD.encode(key)),
    };
    let transport =
        transporte::Transport::new(&f.root.join("runtime"), &package, "bx_fixture01", "fixture-private-not-a-key")
            .unwrap()
            .with_test_executable(executable);
    transport.probes(&|| Ok(())).await.unwrap();
    let op = uuid::Uuid::new_v4();
    assert!(transport.remote_socket_probe(op, false, &|| Ok(())).await.is_err());
    transport.remote_socket_probe(op, true, &|| Ok(())).await.unwrap();
    assert!(transport.probes(&|| anyhow::bail!("revoked")).await.is_err());
}
#[cfg(unix)]
#[tokio::test]
async fn consulta_cli_ficticia_so_status_sem_prompt_e_filtra_conta_crua() {
    let f = Fixture::new();
    let executable = fake(&f, "onde_roda_codex_fake.py");
    let digest = runner::file_digest(&executable).unwrap();
    let profile = f.root.join("profile");
    transporte::private_create(&profile).unwrap();
    std::fs::write(
        profile.join("fixture-state.json"),
        r#"{"account":{"type":"apiKey","email":"ficticio@example.invalid","token":"FICTIONAL"}}"#,
    )
    .unwrap();
    let state = runner::official_query(&executable, &profile, &digest).await.unwrap();
    assert_eq!(state, AccountState::ApiNaoAdmitida);
    let transcript = std::fs::read_to_string(profile.join("requests.jsonl")).unwrap();
    let methods: Vec<_> = transcript
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap()["method"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(methods, ["initialize", "initialized", "account/read"]);
    assert!(!serde_json::to_string(&state).unwrap().contains("FICTIONAL"));
    assert!(runner::official_query(&executable, &profile, &"0".repeat(64)).await.is_err());
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "alvo opt-in: executar SOMENTE no servidor sem sudo e após autorização; exige sshd real"]
fn onde_roda_sshd_opt_in() {
    let output = std::process::Command::new("/usr/bin/python3")
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/onde_roda_sshd_opt_in.py"))
        .arg("--run")
        .env_clear()
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let cases: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(cases.len(), 7);
    for case in cases {
        let success = case["success"].as_bool().unwrap();
        let stdout = case["stdout"].as_str().unwrap();
        let stderr = case["stderr"].as_str().unwrap();
        let kind = case["kind"].as_str().unwrap();
        if kind.starts_with("remote_socket") {
            let denied = transporte::remote_socket_denied(
                case["ready"].as_bool().unwrap(),
                case["listen"].as_str().unwrap(),
                success,
                stdout,
                stderr,
            );
            assert_eq!(denied, kind == "remote_socket", "{kind}");
        } else {
            let probe = match kind {
                "exec" => transporte::Probe::Exec,
                "shell" => transporte::Probe::Shell,
                "pty" => transporte::Probe::Pty,
                "remote" => transporte::Probe::Remote,
                _ => transporte::Probe::OtherTarget,
            };
            assert!(transporte::denied(probe, success, stdout, stderr), "{kind}");
        }
    }
}
