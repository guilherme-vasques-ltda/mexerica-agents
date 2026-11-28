#![allow(clippy::unwrap_used)]
use super::*;
use crate::{
    LockExt,
    api::{devices, dispatch},
    fork::spaces::PERSONAL,
    remote::identity::Identity,
    store::{BotConfig, Device, DeviceSource, Scope, Store},
};
use futures::StreamExt;
use std::{path::Path, sync::Arc};

struct Fixture {
    hub: Arc<Hub>,
    dir: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        Self::with_hub_id("projects-fixture".into())
    }
    fn with_hub_id(host_id: String) -> Self {
        let dir = std::env::temp_dir().join(format!("d541-projects-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let hub = Hub::new(
            Store::open(Path::new(":memory:")).unwrap(),
            host_id,
            Identity::load_or_create(&dir).unwrap(),
            "fixture".into(),
            0,
        );
        hub.store.db.locked().execute_batch("PRAGMA foreign_keys=ON").unwrap();
        for (id, hidden) in [("visible", false), ("hidden", true)] {
            let cfg: BotConfig = serde_json::from_value(json!({"id":id,"name":id,"spaceId":PERSONAL,"backend":"fixture","hidden":hidden,"cwd":"/fixture-private","model":"fixture-private-model"})).unwrap();
            hub.store.save_bot(&cfg).unwrap();
        }
        Self { hub, dir }
    }
    async fn space(&self, name: &str) -> String {
        let state = self.hub.store.spaces().unwrap();
        let result = self.call("donnaSpaceCreate", json!({"hubId":self.hub.host_id,"expectedRevision":state.revision,"name":name,"tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"ES"}}})).await.unwrap();
        result["spaces"].as_array().unwrap().last().unwrap()["id"].as_str().unwrap().to_owned()
    }
    fn bot(&self, id: &str, space: &str) {
        let cfg: BotConfig =
            serde_json::from_value(json!({"id":id,"name":id,"spaceId":space,"backend":"fixture"})).unwrap();
        self.hub.store.save_bot(&cfg).unwrap();
    }
    fn context(&self) -> Value {
        json!({"hubId":self.hub.host_id,"spaceId":PERSONAL})
    }
    fn create(&self) -> Value {
        let mut body = self.context();
        body["clientRequestId"] = uuid::Uuid::new_v4().to_string().into();
        body["name"] = "Projeto fictício".into();
        body["objective"] = "Organizar entrega manual".into();
        body["coordinatorBotId"] = "visible".into();
        body
    }
    fn save(&self, snapshot: &Value) -> Value {
        let p = &snapshot["project"];
        json!({"hubId":self.hub.host_id,"spaceId":snapshot["spaceId"],"projectId":p["id"],"clientRequestId":uuid::Uuid::new_v4().to_string(),"expectedRevision":p["revision"],"name":p["name"],"objective":p["objective"],"coordinatorBotId":p["coordinatorBotId"],"state":p["state"],"tasks":snapshot["tasks"]})
    }
    fn owner(&self) -> (Device, Caller) {
        let device = Device {
            key: "projects-control".into(),
            name: "Fixture".into(),
            platform: "fixture".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![Scope::Control],
            lease_until: None,
            created_at: 0,
            last_seen_at: None,
        };
        self.hub.store.put_device(&device).unwrap();
        let (_, caller) = crate::fork::autoridade::autenticar(&self.hub, &device.key).unwrap();
        (device, caller)
    }
    fn account_owner(&self) -> (Device, Caller) {
        let (mut device, _) = self.owner();
        device.source = DeviceSource::Account;
        device.grant_id = Some("projects-grant".into());
        device.lease_until = Some(crate::store::now_ms() + 60_000);
        crate::remote::cloud::update_status(&self.hub, |status| {
            status.owner = Some(crate::remote::cloud::Owner { user_id: "projects-owner".into(), email: None });
        });
        self.hub.cloud_synced.store(true, std::sync::atomic::Ordering::Release);
        self.hub.store.put_device(&device).unwrap();
        let (_, caller) = crate::fork::autoridade::autenticar(&self.hub, &device.key).unwrap();
        (device, caller)
    }
    async fn call(&self, method: &str, body: Value) -> Result<Value> {
        dispatch(&self.hub, &Caller::Local, method, body).await
    }
    fn count(&self, table: &str) -> i64 {
        // Apenas nomes constantes destas fixtures, nunca entrada de RPC.
        self.hub.store.db.locked().query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).unwrap();
    }
}
fn task() -> Value {
    json!({"id":uuid::Uuid::new_v4().to_string(),"parentTaskId":null,"assigneeBotId":"visible","title":"Tarefa fictícia","description":"Descrição","acceptance":"Aceite","evidence":"Referência textual","state":"backlog"})
}

#[tokio::test]
async fn dono_local_edita_projetos_manuais_sem_disparar_bots() {
    let f = Fixture::new();
    let list = f.call("donnaProjects", f.context()).await.unwrap();
    assert_eq!(list["projects"], json!([]));
    let capabilities = &list["capabilities"];
    for cap in ["manualProjects", "taskList", "taskMeta", "defaultProject"] {
        assert!(capabilities.as_array().unwrap().contains(&json!(cap)), "{cap}");
    }
    for cap in ["taskComments", "taskStart", "taskSchedule", "taskAgenda", "taskSessions"] {
        assert_eq!(
            capabilities.as_array().unwrap().contains(&json!(cap)),
            !crate::environment::Environment::is_open(),
            "{cap}"
        );
    }
    let created = f.call("donnaProjectCreate", f.create()).await.unwrap();
    let mut update = f.save(&created["snapshot"]);
    let tarefa = task();
    update["tasks"] = json!([tarefa.clone()]);
    let saved = f.call("donnaProjectSave", update).await.unwrap();
    let mut query = f.context();
    query["projectId"] = saved["snapshot"]["project"]["id"].clone();
    let reopened = f.call("donnaProject", query.clone()).await.unwrap();
    assert_eq!(reopened["tasks"][0]["id"], tarefa["id"]);
    let tasks = f.call("donnaTasks", f.context()).await.unwrap();
    assert_eq!(tasks["page"]["tasks"][0]["title"], "Tarefa fictícia");
    query["spaceId"] = "space-inexistente".into();
    assert!(f.call("donnaProject", query).await.is_err());
    assert_eq!(f.count("donna_task_declarations"), 0);
    assert!(f.hub.store.history("visible", i64::MAX, 20).unwrap().is_empty());
}

#[tokio::test]
async fn tarefa_nova_do_dono_grava_origem_uma_vez() {
    use super::delegacao_meta::{OriginKind, RequesterKind, admit, load_space};
    let f = Fixture::new();
    let created = f.call("donnaProjectCreate", f.create()).await.unwrap();
    let mut save = f.save(&created["snapshot"]);
    let project_id = save["projectId"].as_str().unwrap().to_owned();
    let first = task();
    let task_id = first["id"].as_str().unwrap().to_owned();
    save["tasks"] = json!([first]);
    let saved = f.call("donnaProjectSave", save).await.unwrap();
    let mut owner_change = f.save(&saved["snapshot"]);
    owner_change["tasks"][0]["state"] = json!("cancelled");
    owner_change["tasks"][0]["assigneeBotId"] = Value::Null;
    f.call("donnaProjectSave", owner_change).await.unwrap();

    let c = f.hub.store.db.locked();
    let scope = (f.hub.host_id.as_str(), PERSONAL, project_id.as_str());
    admit(&c, scope, &task_id, 0, Some("bot-a"), None).unwrap();
    let metadata = load_space(&c, &f.hub.host_id, PERSONAL).unwrap();
    let meta = &metadata[&(project_id, task_id)];
    assert_eq!(meta.origin_kind, OriginKind::OwnerDirect);
    assert_eq!(meta.requester_kind, RequesterKind::Owner);
    assert_eq!(meta.root_bot_id, None);
    assert!(meta.admitted_at.unwrap() > 0);
    assert!(c.execute("UPDATE donna_task_delegation_meta SET origin_kind='delegated'", []).is_err());
}

#[tokio::test]
async fn task_attention_requires_an_open_owner_request_in_an_exclusive_scoped_session() {
    use crate::fork::sessoes::{
        origin::SessionOrigin,
        store::{self as sessions, NewSession},
    };
    let f = Fixture::with_hub_id(uuid::Uuid::new_v4().to_string());
    let created = f.call("donnaProjectCreate", f.create()).await.unwrap();
    let mut save = f.save(&created["snapshot"]);
    let first = task();
    let second = task();
    save["tasks"] = json!([first, second]);
    let id = first["id"].as_str().unwrap();
    let project = save["projectId"].as_str().unwrap().to_owned();
    f.call("donnaProjectSave", save).await.unwrap();
    let read = json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"projectId":project,"taskId":id});
    let attention = || async { f.call("donnaTaskSessions", read.clone()).await.unwrap()["attention"].clone() };
    assert_eq!(attention().await, Value::Null);
    let session = {
        let db = f.hub.store.db.locked();
        let s = sessions::create(
            &db,
            &NewSession {
                chat: "visible".into(),
                origin: SessionOrigin::Task { task_id: id.into() },
                source_session: None,
                parent_session: None,
                title: "Fictícia".into(),
                runs_on: None,
            },
        )
        .unwrap();
        db.execute(
            "INSERT INTO donna_task_sessions VALUES(?1,?2,?3,?4,'visible',?5,0)",
            rusqlite::params![f.hub.host_id, PERSONAL, project, id, s.id],
        )
        .unwrap();
        s.id
    };
    let insert = |entry, session: Option<&str>, kind, data: Value| {
        f.hub
            .store
            .db
            .locked()
            .execute(
                "INSERT INTO entries(id,bot_id,rev,kind,turn,data,created_at,updated_at,session)
             VALUES(?1,'visible',1,?2,1,?3,1,1,?4)",
                rusqlite::params![entry, kind, data.to_string(), session],
            )
            .unwrap();
    };
    insert("unrelated", None, "permission", json!({"status":"pending","author":"visible"}));
    insert("comment", Some(&session), "notice", json!({"text":"Pode decidir?"}));
    assert_eq!(attention().await, Value::Null);
    insert(
        "question",
        Some(&session),
        "notice",
        json!({"card":"ownerQuestion","author":"visible","ownerQuestion":{"status":"pending"},"text":"Não projetar"}),
    );
    let answer = f.call("donnaTaskSessions", read.clone()).await.unwrap();
    assert_eq!(answer["attention"], "owner");
    assert!(!answer.to_string().contains("Não projetar"));
    f.hub.store.db.locked().execute(
        "UPDATE donna_space_conversations SET context_start_seq=(SELECT seq+1 FROM entries WHERE id='question') WHERE id='visible'",
        [],
    ).unwrap();
    assert_eq!(attention().await, Value::Null);
    f.hub.store.db.locked().execute("DELETE FROM entries WHERE id='question'", []).unwrap();
    insert(
        "screen",
        Some(&session),
        "permission",
        json!({"status":"pending","author":"visible","toolKind":"screen_access"}),
    );
    insert("other-author", Some(&session), "permission", json!({"status":"pending","author":"hidden"}));
    assert_eq!(attention().await, Value::Null);
    insert("approval", Some(&session), "permission", json!({"status":"pending","author":"visible"}));
    assert_eq!(attention().await, "owner");
    f.hub.store.db.locked().execute("UPDATE donna_bot_sessions SET state='closed' WHERE id=?1", [&session]).unwrap();
    assert_eq!(attention().await, Value::Null);
    f.hub.store.db.locked().execute("UPDATE donna_bot_sessions SET state='open' WHERE id=?1", [&session]).unwrap();
    f.hub.store.db.locked().execute("UPDATE bots SET deleted=1 WHERE id='visible'", []).unwrap();
    assert_eq!(attention().await, Value::Null);
    f.hub.store.db.locked().execute("UPDATE bots SET deleted=0 WHERE id='visible'", []).unwrap();
    assert_eq!(attention().await, "owner");
    f.hub
        .store
        .db
        .locked()
        .execute(
            "INSERT INTO donna_task_sessions VALUES(?1,?2,?3,?4,'visible',?5,0)",
            rusqlite::params![f.hub.host_id, PERSONAL, project, second["id"].as_str().unwrap(), session],
        )
        .unwrap();
    assert_eq!(attention().await, Value::Null);
    f.hub.store.db.locked().execute("DELETE FROM donna_task_sessions WHERE task_id<>?1", [id]).unwrap();
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE entries SET data='{\"status\":\"approved\"}' WHERE id='approval'", [])
        .unwrap();
    assert_eq!(attention().await, Value::Null);
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE entries SET data='{\"status\":\"pending\",\"author\":\"visible\"}' WHERE id='approval'", [])
        .unwrap();
    f.hub.store.db.locked().execute("UPDATE donna_space_conversations SET closed=1 WHERE id='visible'", []).unwrap();
    assert_eq!(attention().await, Value::Null);
}

fn assert_authority_ended(error: &anyhow::Error) {
    assert!(error.is::<crate::fork::autoridade::AutorizacaoEncerrada>());
    assert_eq!(
        crate::api::error_status(error),
        (axum::http::StatusCode::BAD_REQUEST, "not_allowed: autorização original encerrada".into())
    );
}

#[tokio::test]
async fn sincronizacao_passageira_recusa_dispatch_captura_gravacao_e_saida_sem_recapturar() {
    use std::sync::atomic::Ordering;
    for recuperar_antes_de_gravar in [false, true] {
        let f = Fixture::new();
        let (device, caller) = f.account_owner();
        let original = Autoridade::capturar(&f.hub, &caller).unwrap();
        let created = dispatch(&f.hub, &caller, "donnaProjectCreate", f.create()).await.unwrap();
        let save = f.save(&created["snapshot"]);
        let mut events = f.hub.events.subscribe();
        let guard = f.hub.store.team_lock.clone().lock_owned().await;
        let pending = dispatch(&f.hub, &caller, "donnaProjectSave", save.clone());
        tokio::pin!(pending);
        tokio::select! { biased; result = &mut pending => panic!("não esperou: {result:?}"), () = std::future::ready(()) => {} }
        f.hub.cloud_synced.store(false, Ordering::Release);
        if recuperar_antes_de_gravar {
            f.hub.cloud_synced.store(true, Ordering::Release);
        }
        drop(guard);
        assert_authority_ended(&pending.await.unwrap_err());
        // Entrada real (com_pedido/conferir_caller) e captura direta do consumidor têm o mesmo contrato.
        for method in METHODS {
            assert_authority_ended(&dispatch(&f.hub, &caller, method, f.context()).await.unwrap_err());
        }
        assert_authority_ended(&call(&f.hub, &caller, "donnaProjects", f.context()).await.unwrap_err());
        let sent = std::cell::Cell::new(false);
        assert_authority_ended(&original.com_saida(&f.hub, None, || sent.set(true)).unwrap_err());
        assert!(!sent.get());
        assert_eq!(f.count("donna_projects"), 1);
        assert_eq!(f.count("donna_project_receipts"), 1);
        assert_eq!(f.count("donna_project_history"), 1);
        assert!(events.try_recv().is_err());
        // Voltar a sincronizar não reabilita a época antiga. Só uma NOVA requisição autenticada lê/grava.
        f.hub.cloud_synced.store(true, Ordering::Release);
        assert_authority_ended(&dispatch(&f.hub, &caller, "donnaProjectSave", save.clone()).await.unwrap_err());
        let (_, fresh) = crate::fork::autoridade::autenticar(&f.hub, &device.key).unwrap();
        let read = dispatch(
            &f.hub,
            &fresh,
            "donnaProject",
            json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"projectId":created["snapshot"]["project"]["id"]}),
        )
        .await
        .unwrap();
        assert_eq!(read, created["snapshot"]);
        assert_eq!(f.count("donna_project_receipts"), 1);
        dispatch(&f.hub, &fresh, "donnaProjectSave", save).await.unwrap();
        assert_eq!(f.count("donna_project_receipts"), 2);
    }
}

#[tokio::test]
async fn status_generico_preservado_em_custo_armazenamento_na_queda_retorno_e_revogacao() {
    let f = Fixture::new();
    let (device, caller) = f.account_owner();
    // O fallback anterior continua 400; Forbidden explícito continua 403.
    assert_eq!(crate::api::error_status(&anyhow::anyhow!("fixture")).0, axum::http::StatusCode::BAD_REQUEST);
    assert_authority_ended(&crate::fork::autoridade::AutorizacaoEncerrada.into());
    assert_eq!(crate::api::error_status(&Forbidden.into()).0, axum::http::StatusCode::FORBIDDEN);
    for synced in [false, true] {
        f.hub.cloud_synced.store(synced, std::sync::atomic::Ordering::Release);
        for method in [crate::fork::custo::METHOD, crate::fork::armazenamento::METHOD] {
            assert_authority_ended(&dispatch(&f.hub, &caller, method, json!({})).await.unwrap_err());
        }
    }
    let (_, fresh) = crate::fork::autoridade::autenticar(&f.hub, &device.key).unwrap();
    f.hub.revoke_device(&device.key).unwrap();
    for method in [crate::fork::custo::METHOD, crate::fork::armazenamento::METHOD, "donnaProjects"] {
        assert_authority_ended(&dispatch(&f.hub, &fresh, method, f.context()).await.unwrap_err());
    }
    assert_eq!(f.count("donna_projects"), 0);
    assert_eq!(f.count("donna_project_receipts"), 0);
    assert_eq!(f.count("donna_project_history"), 0);
}

#[tokio::test]
async fn fluxo_manual_revisao_replay_atual_e_historico_sem_runtime() {
    let f = Fixture::new();
    let create = f.create();
    let first = f.call("donnaProjectCreate", create.clone()).await.unwrap();
    let mut save = f.save(&first["snapshot"]);
    save["tasks"] = json!([task(), task(), task(), task()]);
    save["tasks"][1]["parentTaskId"] = save["tasks"][0]["id"].clone();
    let second = f.call("donnaProjectSave", save.clone()).await.unwrap();
    let mut conflict = save.clone();
    conflict["clientRequestId"] = uuid::Uuid::new_v4().to_string().into();
    assert!(f.call("donnaProjectSave", conflict).await.unwrap_err().to_string().contains("projects_conflict"));
    let mut next = f.save(&second["snapshot"]);
    next["tasks"][0]["state"] = "done".into();
    next["tasks"][0]["title"] = "Título alterado".into();
    let third = f.call("donnaProjectSave", next).await.unwrap();
    let replay = f.call("donnaProjectSave", save.clone()).await.unwrap();
    assert_eq!(replay["receipt"], second["receipt"]);
    assert_eq!(replay["snapshot"], third["snapshot"]);
    let create_replay = f.call("donnaProjectCreate", create).await.unwrap();
    assert_eq!(create_replay["receipt"], first["receipt"]);
    assert_eq!(create_replay["snapshot"], third["snapshot"]);
    save["name"] = "Outro".into();
    assert!(f.call("donnaProjectSave", save).await.unwrap_err().to_string().contains("projects_idempotency"));
    let snapshot = &third["snapshot"];
    assert_eq!(snapshot["tasks"].as_array().unwrap().len(), 4);
    let summary = snapshot["history"][0]["summary"].as_str().unwrap();
    assert!(summary.contains("título") && summary.contains("A fazer → Concluída manualmente"));
    assert_eq!(f.count("entries"), 0);
    let stored: String = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT payload FROM donna_project_receipts LIMIT 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(stored.len(), 64);
    assert!(!snapshot.to_string().contains("fixture-private"));
    let read = f
        .call("donnaProject", json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"projectId":snapshot["project"]["id"]}))
        .await
        .unwrap();
    assert_eq!(&read, snapshot);
}

#[tokio::test]
async fn fronteiras_dto_uuid_texto_pai_ciclo_exclusao_e_atomicidade() {
    let f = Fixture::new();
    let first = f.call("donnaProjectCreate", f.create()).await.unwrap();
    let mut save = f.save(&first["snapshot"]);
    save["tasks"] = json!([task(), task()]);
    let second = f.call("donnaProjectSave", save).await.unwrap();
    let base = f.save(&second["snapshot"]);
    let mut cases = Vec::new();
    let mut bad = base.clone();
    bad["author"] = "owner".into();
    cases.push(bad);
    let mut bad = base.clone();
    bad["tasks"][0]["author"] = "owner".into();
    cases.push(bad);
    let mut bad = base.clone();
    bad["tasks"] = json!([]);
    cases.push(bad);
    let mut bad = base.clone();
    bad["tasks"][0]["parentTaskId"] = uuid::Uuid::new_v4().to_string().into();
    cases.push(bad);
    let mut bad = base.clone();
    bad["tasks"][0]["parentTaskId"] = bad["tasks"][1]["id"].clone();
    bad["tasks"][1]["parentTaskId"] = bad["tasks"][0]["id"].clone();
    cases.push(bad);
    let mut bad = base.clone();
    bad["tasks"][0]["assigneeBotId"] = "hidden".into();
    cases.push(bad);
    let mut bad = base.clone();
    bad["clientRequestId"] = "not-a-uuid".into();
    cases.push(bad);
    let mut bad = base.clone();
    bad["tasks"][0]["title"] = "x\u{202e}".into();
    cases.push(bad);
    let mut bad = base.clone();
    bad["objective"] = "x".repeat(2001).into();
    cases.push(bad);
    let mut bad = base.clone();
    bad["tasks"][0]["state"] = "ready".into();
    cases.push(bad);
    for bad in cases {
        assert!(f.call("donnaProjectSave", bad).await.is_err());
        assert_eq!(f.count("donna_project_receipts"), 2);
    }
    // Unicode é contado em caracteres, sem rejeitar acentos por comprimento UTF-8.
    let mut unicode = base.clone();
    unicode["name"] = "á".repeat(120).into();
    let saved = f.call("donnaProjectSave", unicode).await.unwrap();
    let mut fail = f.save(&saved["snapshot"]);
    fail["name"] = "Não confirmar".into();
    f.hub.store.db.locked().execute_batch("CREATE TEMP TRIGGER projects_fail BEFORE INSERT ON donna_project_receipts BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(f.call("donnaProjectSave", fail).await.is_err());
    let current = f
        .call(
            "donnaProject",
            json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"projectId":saved["snapshot"]["project"]["id"]}),
        )
        .await
        .unwrap();
    assert_eq!(current, saved["snapshot"]);
}

#[tokio::test]
async fn referencia_indisponivel_preserva_ids_sem_nome_privado_e_save_incidental() {
    let f = Fixture::new();
    let first = f.call("donnaProjectCreate", f.create()).await.unwrap();
    let mut save = f.save(&first["snapshot"]);
    save["tasks"] = json!([task()]);
    let second = f.call("donnaProjectSave", save).await.unwrap();
    let mut cfg = f.hub.store.bot("visible").unwrap().unwrap().config;
    cfg.hidden = true;
    cfg.name = "NOME PRIVADO".into();
    f.hub.store.save_bot(&cfg).unwrap();
    let saved = f.call("donnaProjectSave", f.save(&second["snapshot"])).await.unwrap();
    assert_eq!(saved["snapshot"]["project"]["coordinatorBotId"], "visible");
    assert_eq!(saved["snapshot"]["tasks"][0]["assigneeBotId"], "visible");
    assert!(!saved.to_string().contains("NOME PRIVADO"));
    assert_eq!(saved["snapshot"]["bots"], json!([]));
    // Exclusão lógica pelo produtor real também preserva os registros do Projeto.
    f.hub.store.delete_bot("visible").unwrap();
    let saved = f.call("donnaProjectSave", f.save(&saved["snapshot"])).await.unwrap();
    assert_eq!(saved["snapshot"]["tasks"][0]["assigneeBotId"], "visible");
}

#[tokio::test]
async fn escopos_hub_space_e_exposicao_bot_mcp_fechados() {
    let f = Fixture::new();
    for method in METHODS {
        assert!(!crate::fork::ponte_credencial::PERMITIDOS.contains(method));
        for caller in [
            Caller::Pairing { key: "fixture".into() },
            Caller::Device { key: "fixture".into(), scopes: vec![], autoridade: None },
        ] {
            assert!(devices::permit(&caller, method).is_err());
            assert!(dispatch(&f.hub, &caller, method, f.context()).await.is_err());
        }
        assert!(crate::chat::team::call(&f.hub, "visible", method, &json!({})).await.is_err());
    }
    for methods in [crate::chat::team::tools(), crate::chat::manage::tools()] {
        assert!(!methods.to_string().contains("donnaProject"));
    }
    let (_, owner) = f.owner();
    for body in [
        json!({"hubId":"outro","spaceId":PERSONAL}),
        json!({"hubId":f.hub.host_id,"spaceId":"inexistente"}),
        json!({"hubId":f.hub.host_id,"spaceId":"trabalho"}),
    ] {
        for caller in [&Caller::Local, &owner] {
            let error = dispatch(&f.hub, caller, "donnaProjects", body.clone()).await.unwrap_err();
            assert!(error.downcast_ref::<Forbidden>().is_some());
            assert_eq!(crate::api::error_status(&error), crate::api::error_status(&Forbidden.into()));
        }
    }
    assert_eq!(f.count("donna_projects"), 0);
}

#[tokio::test]
async fn autoridade_original_revogada_durante_espera_nao_grava_nem_vaza() {
    let f = Fixture::new();
    let (mut device, caller) = f.owner();
    let mut events = f.hub.events.subscribe();
    let guard = f.hub.store.team_lock.clone().lock_owned().await;
    let pending = dispatch(&f.hub, &caller, "donnaProjectCreate", f.create());
    tokio::pin!(pending);
    tokio::select! { biased; result = &mut pending => panic!("não esperou: {result:?}"), () = std::future::ready(()) => {} }
    device.scopes.clear();
    f.hub.store.put_device(&device).unwrap();
    drop(guard);
    let error = pending.await.unwrap_err();
    assert_authority_ended(&error);
    let error = dispatch(&f.hub, &caller, "donnaProjects", f.context()).await.unwrap_err();
    assert_authority_ended(&error);
    assert_eq!(f.count("donna_projects"), 0);
    assert_eq!(f.count("donna_project_receipts"), 0);
    assert_eq!(f.count("donna_project_history"), 0);
    assert!(events.try_recv().is_err());
}

#[tokio::test]
async fn stream_filtra_audiencia_space_binding_revogado_e_payload() {
    let f = Fixture::new();
    let (_, owner) = f.owner();
    assert!(
        event(&f.hub, &Caller::Local, PERSONAL, &json!({"type":EVENT,"hubId":"outro-hub","spaceId":PERSONAL}))
            .is_none()
    );
    for (caller, allowed) in [
        (Caller::Local, true),
        (owner.clone(), true),
        (Caller::Device { key: "read".into(), scopes: vec![], autoridade: None }, false),
        (Caller::Pairing { key: "bot".into() }, false),
    ] {
        let stream = crate::api::scoped_events_stream(&f.hub, f.hub.store.current_rev(), None, &caller, Some(PERSONAL));
        let stream = match stream {
            Ok(stream) => stream,
            Err(_) if !allowed => continue,
            Err(error) => panic!("stream do dono recusado: {error}"),
        };
        tokio::pin!(stream);
        assert_eq!(stream.next().await.unwrap()["type"], "hello");
        let _ = f.hub.events.send(json!({"type":EVENT,"hubId":f.hub.host_id,"spaceId":"outro-space","name":"privado"}));
        let _ = f
            .hub
            .events
            .send(json!({"type":EVENT,"hubId":f.hub.host_id,"spaceId":PERSONAL,"snapshot":{"name":"privado"}}));
        let _ = f.hub.events.send(json!({"type":"resync","marker":"fim"}));
        let mut next = stream.next().await.unwrap();
        // O catch-up pode conter bots; só avançar sobre o head antes do marcador.
        while next["type"] == "bot" {
            next = stream.next().await.unwrap();
        }
        if allowed {
            assert_eq!(next, json!({"type":EVENT,"hubId":f.hub.host_id,"spaceId":PERSONAL}));
            assert_eq!(stream.next().await.unwrap()["marker"], "fim");
        } else {
            assert_eq!(next["marker"], "fim");
        }
    }
    let stream =
        crate::api::scoped_events_stream(&f.hub, f.hub.store.current_rev(), None, &owner, Some(PERSONAL)).unwrap();
    tokio::pin!(stream);
    stream.next().await.unwrap();
    f.hub.revoke_device(owner.device_key()).unwrap();
    assert!(event(&f.hub, &owner, PERSONAL, &json!({"type":EVENT,"hubId":f.hub.host_id,"spaceId":PERSONAL})).is_none());
    assert!(
        event(&f.hub, &Caller::Local, "outro-space", &json!({"type":EVENT,"hubId":f.hub.host_id,"spaceId":PERSONAL}))
            .is_none()
    );
    let _ = f.hub.events.send(json!({"type":EVENT,"hubId":f.hub.host_id,"spaceId":PERSONAL}));
    let _ = f.hub.events.send(json!({"type":"resync","marker":"fim"}));
    while let Some(next) = stream.next().await {
        assert_ne!(next["type"], EVENT);
        if next["marker"] == "fim" {
            break;
        }
    }
}

#[tokio::test]
async fn device_control_positivo_e_binding_antigo_nao_renova_apos_revogacao() {
    let f = Fixture::new();
    let (device, caller) = f.owner();
    let created = dispatch(&f.hub, &caller, "donnaProjectCreate", f.create()).await.unwrap();
    let save = f.save(&created["snapshot"]);
    let guard = f.hub.store.team_lock.clone().lock_owned().await;
    let pending = dispatch(&f.hub, &caller, "donnaProjectSave", save);
    tokio::pin!(pending);
    tokio::select! { biased; result = &mut pending => panic!("não esperou: {result:?}"), () = std::future::ready(()) => {} }
    f.hub.revoke_device(&device.key).unwrap();
    // Reconcessão de aparelho Local = o passo do pareamento por código (`readmitir_pareado`), que libera o bloqueio e grava o aparelho numa Tx.
    crate::fork::workspace::readmitir_pareado(&f.hub.store, &device).unwrap();
    drop(guard);
    let error = pending.await.unwrap_err();
    assert_authority_ended(&error);
    assert_eq!(f.count("donna_project_receipts"), 1);
    let (_, fresh) = crate::fork::autoridade::autenticar(&f.hub, &device.key).unwrap();
    let read = dispatch(&f.hub, &fresh, "donnaProjects", f.context()).await.unwrap();
    assert_eq!(read["projects"][0]["revision"], 1);
    assert_eq!(
        read["capabilities"],
        json!([
            "manualProjects",
            "taskList",
            "taskMeta",
            "taskComments",
            "defaultProject",
            "taskStart",
            "taskSchedule",
            "taskAgenda",
            "taskSessions"
        ])
    );
}

#[tokio::test]
async fn limites_arvore_projeto_e_historico_preservado_com_resposta_limitada() {
    let f = Fixture::new();
    let first = f.call("donnaProjectCreate", f.create()).await.unwrap();
    let other = f.call("donnaProjectCreate", f.create()).await.unwrap();
    let mut other_save = f.save(&other["snapshot"]);
    other_save["tasks"] = json!([task()]);
    let other = f.call("donnaProjectSave", other_save).await.unwrap();
    let base = f.save(&first["snapshot"]);
    let mut bad = base.clone();
    bad["tasks"] = json!([task()]);
    bad["tasks"][0]["parentTaskId"] = other["snapshot"]["tasks"][0]["id"].clone();
    assert!(f.call("donnaProjectSave", bad).await.is_err());
    let mut deep = base.clone();
    deep["tasks"] = json!([task(), task(), task(), task()]);
    for i in 1..4 {
        deep["tasks"][i]["parentTaskId"] = deep["tasks"][i - 1]["id"].clone();
    }
    assert!(f.call("donnaProjectSave", deep).await.is_err());
    let mut many = base.clone();
    many["tasks"] = (0..101).map(|_| task()).collect::<Vec<_>>().into();
    assert!(f.call("donnaProjectSave", many).await.is_err());
    // Simula trilha antiga sem executar bots: 1100 registros ficam no banco descartável.
    {
        let mut db = f.hub.store.db.locked();
        let tx = db.transaction().unwrap();
        for i in 0..1100 {
            tx.execute("INSERT INTO donna_project_history(hub_id,space_id,project_id,entry) VALUES(?1,?2,?3,?4)",rusqlite::params![f.hub.host_id,PERSONAL,first["snapshot"]["project"]["id"].as_str().unwrap(),json!({"id":uuid::Uuid::new_v4().to_string(),"at":i,"author":"owner","action":"fixture","summary":"Alteração fictícia"}).to_string()]).unwrap();
        }
        tx.commit().unwrap();
    }
    let saved = f.call("donnaProjectSave", base).await.unwrap();
    assert_eq!(saved["snapshot"]["history"].as_array().unwrap().len(), 100);
    assert_eq!(saved["snapshot"]["historyOmitted"], 1001);
    assert_eq!(f.count("donna_project_history"), 1103);
    assert_eq!(f.count("donna_project_receipts"), 4);
}

#[test]
fn persistencia_reabertura_e_migracao_idempotente_em_fixture_descartavel() {
    let dir = std::env::temp_dir().join(format!("d541-reopen-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fixture.db");
    let request = json!({"hubId":"fixture","spaceId":PERSONAL,"clientRequestId":uuid::Uuid::new_v4().to_string(),"name":"Persistência fictícia","objective":"Manual","coordinatorBotId":null});
    let snapshot;
    {
        let store = Store::open(&path).unwrap();
        let mut db = store.db.locked();
        let tx = db.transaction().unwrap();
        snapshot = store::call(&tx, "fixture", "donnaProjectCreate", request.clone()).unwrap().0;
        tx.commit().unwrap();
    }
    let store = Store::open(&path).unwrap();
    migrate(&store.db.locked()).unwrap();
    let mut db = store.db.locked();
    let tx = db.transaction().unwrap();
    let replay = store::call(&tx, "fixture", "donnaProjectCreate", request).unwrap();
    assert!(!replay.1);
    assert_eq!(replay.0, snapshot);
    tx.commit().unwrap();
    drop(db);
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn dois_spaces_isolam_projetos_roster_historico_receipts_e_ids() {
    let f = Fixture::new();
    let a = f.space("Estúdio").await;
    let b = f.space("Oficina").await;
    f.bot("native-a", &a);
    f.bot("native-b", &b);
    let request_id = uuid::Uuid::new_v4().to_string();
    let mut create_a = f.create();
    create_a["spaceId"] = a.clone().into();
    create_a["clientRequestId"] = request_id.clone().into();
    create_a["coordinatorBotId"] = "native-a".into();
    let mut create_b = create_a.clone();
    create_b["spaceId"] = b.clone().into();
    create_b["name"] = "Entrega da Oficina".into();
    create_b["coordinatorBotId"] = "native-b".into();
    let first_a = f.call("donnaProjectCreate", create_a.clone()).await.unwrap();
    let first_b = f.call("donnaProjectCreate", create_b.clone()).await.unwrap();
    assert_ne!(first_a["snapshot"]["project"]["id"], first_b["snapshot"]["project"]["id"]);
    let mut request_a = f.save(&first_a["snapshot"]);
    request_a["tasks"] = json!([task()]);
    request_a["tasks"][0]["assigneeBotId"] = "native-a".into();
    let saved_a = f.call("donnaProjectSave", request_a).await.unwrap();
    let mut request_b = f.save(&first_b["snapshot"]);
    request_b["tasks"] = json!([task()]);
    request_b["tasks"][0]["assigneeBotId"] = "native-b".into();
    let saved_b = f.call("donnaProjectSave", request_b).await.unwrap();
    for (space, own, bot) in [(&a, &saved_a, "native-a"), (&b, &saved_b, "native-b")] {
        let list = f.call("donnaProjects", json!({"hubId":f.hub.host_id,"spaceId":space})).await.unwrap();
        assert_eq!(list["projects"], json!([own["snapshot"]["project"]]));
        assert_eq!(list["bots"], json!([{"id":bot,"name":bot}]));
        assert_eq!(own["snapshot"]["history"].as_array().unwrap().len(), 2);
        assert_eq!(own["snapshot"]["historyOmitted"], 0);
    }
    for (create, current, original) in [(create_a, &saved_a, &first_a), (create_b, &saved_b, &first_b)] {
        let replay = f.call("donnaProjectCreate", create).await.unwrap();
        assert_eq!(replay["receipt"], original["receipt"]);
        assert_eq!(replay["snapshot"], current["snapshot"]);
    }
    let foreign_id = saved_b["snapshot"]["project"]["id"].clone();
    let foreign_read = json!({"hubId":f.hub.host_id,"spaceId":a,"projectId":foreign_id});
    let missing_read = json!({"hubId":f.hub.host_id,"spaceId":a,"projectId":uuid::Uuid::new_v4().to_string()});
    assert_eq!(
        f.call("donnaProject", foreign_read).await.unwrap_err().to_string(),
        f.call("donnaProject", missing_read).await.unwrap_err().to_string()
    );
    let mut foreign_save = f.save(&saved_b["snapshot"]);
    foreign_save["spaceId"] = a.clone().into();
    assert!(f.call("donnaProjectSave", foreign_save).await.unwrap_err().to_string().contains("project_not_found"));
    let base = f.save(&saved_a["snapshot"]);
    let mut coordinator = base.clone();
    coordinator["coordinatorBotId"] = "native-b".into();
    let mut assignee = base.clone();
    assignee["tasks"][0]["assigneeBotId"] = "native-b".into();
    let mut parent = base.clone();
    parent["tasks"][0]["parentTaskId"] = saved_b["snapshot"]["tasks"][0]["id"].clone();
    let mut personal = base;
    personal["coordinatorBotId"] = "visible".into();
    for bad in [coordinator, assignee, parent, personal] {
        assert!(f.call("donnaProjectSave", bad).await.unwrap_err().to_string().contains("invalid_field"));
    }
    assert_eq!(f.count("donna_project_receipts"), 4);
    assert_eq!(f.count("donna_project_history"), 4);
    assert_eq!(f.count("entries"), 0);
    let mut unavailable = f.hub.store.bot("native-a").unwrap().unwrap().config;
    unavailable.hidden = true;
    unavailable.name = "NOME PRIVADO DO ESTÚDIO".into();
    f.hub.store.save_bot(&unavailable).unwrap();
    let retained = f.call("donnaProjectSave", f.save(&saved_a["snapshot"])).await.unwrap();
    assert_eq!(retained["snapshot"]["project"]["coordinatorBotId"], "native-a");
    assert_eq!(retained["snapshot"]["tasks"][0]["assigneeBotId"], "native-a");
    assert_eq!(retained["snapshot"]["bots"], json!([]));
    assert!(!retained.to_string().contains("NOME PRIVADO"));
}

#[tokio::test]
async fn space_sem_bot_nao_herda_principal_e_limite_cinquenta_e_por_space() {
    let f = Fixture::new();
    let space = f.space("Estúdio").await;
    // Uma conversa estrangeira da principal pessoal não torna o bot nativo deste Space.
    f.hub.store.db.locked().execute(
        "INSERT INTO donna_space_conversations(id,space_id,bot_id,title,revision,is_primary) VALUES(?1,?2,'visible','Conversa estrangeira',1,0)",
        rusqlite::params![uuid::Uuid::new_v4().to_string(), space],
    ).unwrap();
    let empty = f.call("donnaProjects", json!({"hubId":f.hub.host_id,"spaceId":space})).await.unwrap();
    assert_eq!(empty["bots"], json!([]));
    let mut create = f.create();
    create["spaceId"] = space.clone().into();
    assert!(f.call("donnaProjectCreate", create.clone()).await.is_err());
    create["coordinatorBotId"] = Value::Null;
    let first = f.call("donnaProjectCreate", create.clone()).await.unwrap();
    assert!(first["snapshot"]["project"]["coordinatorBotId"].is_null());
    let mut save = f.save(&first["snapshot"]);
    save["tasks"] = json!([task()]);
    save["tasks"][0]["assigneeBotId"] = Value::Null;
    assert!(f.call("donnaProjectSave", save).await.is_ok());
    for _ in 1..50 {
        create["clientRequestId"] = uuid::Uuid::new_v4().to_string().into();
        f.call("donnaProjectCreate", create.clone()).await.unwrap();
    }
    // Replay continua válido no limite, sem consumir outra vaga.
    assert!(f.call("donnaProjectCreate", create.clone()).await.is_ok());
    create["clientRequestId"] = uuid::Uuid::new_v4().to_string().into();
    assert!(f.call("donnaProjectCreate", create.clone()).await.unwrap_err().to_string().contains("projects_limit"));
    create["spaceId"] = PERSONAL.into();
    assert!(f.call("donnaProjectCreate", create).await.is_ok());
    let list = f.call("donnaProjects", json!({"hubId":f.hub.host_id,"spaceId":space})).await.unwrap();
    assert_eq!(list["projects"].as_array().unwrap().len(), 50);
    assert_eq!(f.count("donna_projects"), 51);
}

#[tokio::test]
async fn stream_space_nativo_filtra_eventos_outro_space_e_revalida_control() {
    let f = Fixture::new();
    let a = f.space("Estúdio").await;
    let b = f.space("Oficina").await;
    let (_, owner) = f.owner();
    let stream = crate::api::scoped_events_stream(&f.hub, f.hub.store.current_rev(), None, &owner, Some(&b)).unwrap();
    tokio::pin!(stream);
    let hello = stream.next().await.unwrap();
    assert_eq!(hello["type"], "hello");
    assert_eq!(hello["hostId"], f.hub.host_id);
    assert_eq!(hello["spaceId"], b);
    let mut create = f.create();
    create["spaceId"] = a.clone().into();
    create["coordinatorBotId"] = Value::Null;
    let mut events = f.hub.events.subscribe();
    f.call("donnaProjectCreate", create.clone()).await.unwrap();
    assert_eq!(events.recv().await.unwrap(), json!({"type":EVENT,"hubId":f.hub.host_id,"spaceId":a}));
    create["spaceId"] = b.clone().into();
    f.call("donnaProjectCreate", create).await.unwrap();
    assert_eq!(stream.next().await.unwrap(), json!({"type":EVENT,"hubId":f.hub.host_id,"spaceId":b}));
    assert!(event(&f.hub, &owner, &a, &json!({"type":EVENT,"hubId":f.hub.host_id,"spaceId":b})).is_none());
    f.hub.revoke_device(owner.device_key()).unwrap();
    assert!(event(&f.hub, &owner, &b, &json!({"type":EVENT,"hubId":f.hub.host_id,"spaceId":b})).is_none());
    assert!(
        event(
            &f.hub,
            &Caller::Local,
            "inexistente",
            &json!({"type":EVENT,"hubId":f.hub.host_id,"spaceId":"inexistente"})
        )
        .is_none()
    );
}

#[test]
fn chaves_sql_isolam_hubs_receipts_historico_e_fks_imediatas() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let mut db = store.db.locked();
    db.execute_batch("PRAGMA foreign_keys=ON").unwrap();
    let enabled: bool = db.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
    assert!(enabled);
    let request = uuid::Uuid::new_v4().to_string();
    let create = |hub: &str| json!({"hubId":hub,"spaceId":PERSONAL,"clientRequestId":request,"name":"Manual","objective":"","coordinatorBotId":null});
    let tx = db.transaction().unwrap();
    let first = store::call(&tx, "hub-a", "donnaProjectCreate", create("hub-a")).unwrap().0;
    let second = store::call(&tx, "hub-b", "donnaProjectCreate", create("hub-b")).unwrap().0;
    assert_eq!(first["snapshot"]["history"].as_array().unwrap().len(), 1);
    assert_eq!(second["snapshot"]["history"].as_array().unwrap().len(), 1);
    let list = store::call(&tx, "hub-a", "donnaProjects", json!({"hubId":"hub-a","spaceId":PERSONAL})).unwrap().0;
    assert_eq!(list["projects"], json!([first["snapshot"]["project"]]));
    for table in ["donna_project_history", "donna_project_receipts"] {
        let sql = if table == "donna_project_history" {
            "INSERT INTO donna_project_history(hub_id,space_id,project_id,entry) VALUES('hub-a',?1,?2,'{}')"
        } else {
            "INSERT INTO donna_project_receipts(hub_id,space_id,project_id,request_id,payload,revision) VALUES('hub-a',?1,?2,'request','digest',1)"
        };
        assert!(
            tx.execute(sql, rusqlite::params![PERSONAL, second["snapshot"]["project"]["id"].as_str().unwrap()])
                .is_err()
        );
    }
    tx.commit().unwrap();
}

#[tokio::test]
async fn tarefas_paginam_projetos_ativos_e_respeitam_o_space() {
    let f = Fixture::new();
    let first = f.call("donnaProjectCreate", f.create()).await.unwrap();
    let second = f.call("donnaProjectCreate", f.create()).await.unwrap();
    let other_space = f.space("Outro").await;
    let mut other = f.create();
    other["spaceId"] = other_space.into();
    other["coordinatorBotId"] = Value::Null;
    f.call("donnaProjectCreate", other).await.unwrap();
    let mut save = f.save(&first["snapshot"]);
    save["tasks"] = json!([task()]);
    f.call("donnaProjectSave", save).await.unwrap();
    let mut ids = Vec::new();
    let mut cursor = Value::Null;
    let mut tasks_seen = 0;
    loop {
        let page = f
            .call("donnaTasks", json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"afterProjectId":cursor}))
            .await
            .unwrap();
        if !page["page"].is_null() {
            ids.push(page["page"]["project"]["id"].as_str().unwrap().to_owned());
            tasks_seen += page["page"]["tasks"].as_array().unwrap().len();
        }
        cursor = page["next"].clone();
        if cursor.is_null() {
            break;
        }
    }
    let mut expected = vec![
        first["snapshot"]["project"]["id"].as_str().unwrap().to_owned(),
        second["snapshot"]["project"]["id"].as_str().unwrap().to_owned(),
    ];
    expected.sort();
    assert_eq!(ids, expected);
    assert_eq!(tasks_seen, 1);
    assert!(
        f.call("donnaTasks", json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"afterProjectId":"inválido"}))
            .await
            .is_err()
    );
    assert!(f.call("donnaTasks", json!({"hubId":"outro","spaceId":PERSONAL,"afterProjectId":null})).await.is_err());
    let mut archive = f.save(&second["snapshot"]);
    archive["state"] = "archived".into();
    f.call("donnaProjectSave", archive).await.unwrap();
    let page =
        f.call("donnaTasks", json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"afterProjectId":null})).await.unwrap();
    assert_eq!(page["page"]["project"]["id"], first["snapshot"]["project"]["id"]);
    assert!(page["next"].is_null());
}

#[tokio::test]
async fn metadados_preservam_revisao_e_sobrevivem_a_cliente_antigo() {
    let f = Fixture::new();
    let created = f.call("donnaProjectCreate", f.create()).await.unwrap();
    let mut save = f.save(&created["snapshot"]);
    save["tasks"] = json!([task()]);
    let saved = f.call("donnaProjectSave", save).await.unwrap();
    let snapshot = &saved["snapshot"];
    let id = snapshot["tasks"][0]["id"].as_str().unwrap();
    let request = json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"projectId":snapshot["project"]["id"],"taskId":id,"clientRequestId":uuid::Uuid::new_v4().to_string(),"expectedRevision":snapshot["project"]["revision"],"meta":{"priority":"high","dueAt":2_000_000_000_000_i64,"phases":["Preparar","Revisar"],"currentPhase":1}});
    let updated = f.call("donnaTaskMetaSave", request.clone()).await.unwrap();
    assert_eq!(updated["meta"][id]["priority"], "high");
    assert_eq!(updated["snapshot"]["meta"][id]["dueAt"], 2_000_000_000_000_i64);
    assert_eq!(
        updated["snapshot"]["project"]["revision"].as_i64().unwrap(),
        snapshot["project"]["revision"].as_i64().unwrap() + 1
    );
    let mut stale = request.clone();
    stale["clientRequestId"] = uuid::Uuid::new_v4().to_string().into();
    assert!(f.call("donnaTaskMetaSave", stale).await.unwrap_err().to_string().contains("projects_conflict"));
    let mut old_client = f.save(&updated["snapshot"]);
    old_client["tasks"][0]["description"] = "Edição antiga".into();
    f.call("donnaProjectSave", old_client).await.unwrap();
    let fresh = f
        .call("donnaProject", json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"projectId":snapshot["project"]["id"]}))
        .await
        .unwrap();
    assert_eq!(fresh["meta"][id]["currentPhase"], 1);
    assert_eq!(fresh["meta"][id]["dueAt"], 2_000_000_000_000_i64);
    let replay = f.call("donnaTaskMetaSave", request).await.unwrap();
    assert_eq!(replay["receipt"], updated["receipt"]);
    assert_eq!(replay["meta"][id]["currentPhase"], 1);
    let page =
        f.call("donnaTasks", json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"afterProjectId":null})).await.unwrap();
    assert_eq!(page["page"]["meta"][id]["phases"], json!(["Preparar", "Revisar"]));
}

#[tokio::test]
async fn comentarios_dono_validam_mencoes_e_nunca_entregam_sem_mencao() {
    let f = Fixture::new();
    let created = f.call("donnaProjectCreate", f.create()).await.unwrap();
    let mut save = f.save(&created["snapshot"]);
    save["tasks"] = json!([task()]);
    let saved = f.call("donnaProjectSave", save).await.unwrap();
    let snapshot = &saved["snapshot"];
    let project = snapshot["project"]["id"].as_str().unwrap();
    let task_id = snapshot["tasks"][0]["id"].as_str().unwrap();
    let base = json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"projectId":project,"taskId":task_id,"clientRequestId":uuid::Uuid::new_v4().to_string(),"text":"Anotação","mentions":[],"onlyNote":false});
    let first = f.call("donnaTaskComment", base.clone()).await.unwrap();
    assert_eq!(first["comment"]["author"], "owner");
    assert_eq!(first["comment"]["mentionCheck"], 2);
    assert_eq!(first["comment"]["notify"]["state"], "not_notified");
    assert_eq!(first["comment"]["notify"]["reason"], "no_mention");
    let read = f
        .call(
            "donnaTaskComments",
            json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"projectId":project,"taskId":task_id}),
        )
        .await
        .unwrap();
    assert_eq!(read["comments"].as_array().unwrap().len(), 1);
    assert_eq!(read["omitted"], 0);
    let mut mention = base.clone();
    mention["clientRequestId"] = uuid::Uuid::new_v4().to_string().into();
    mention["text"] = "@visible chama o bot".into();
    mention["mentions"] = json!(["visible"]);
    let notified = f.call("donnaTaskComment", mention).await.unwrap();
    assert_eq!(notified["comment"]["notify"]["state"], "notifying");
    assert_eq!(notified["comment"]["notify"]["botIds"], json!(["visible"]));
    let mut note = base.clone();
    note["clientRequestId"] = uuid::Uuid::new_v4().to_string().into();
    note["text"] = "@visible anotação".into();
    note["mentions"] = json!(["visible"]);
    note["onlyNote"] = true.into();
    let only = f.call("donnaTaskComment", note).await.unwrap();
    assert_eq!(only["comment"]["notify"]["reason"], "only_note");
    let mut bad = base.clone();
    bad["clientRequestId"] = uuid::Uuid::new_v4().to_string().into();
    bad["mentions"] = json!(["hidden"]);
    assert!(f.call("donnaTaskComment", bad).await.unwrap_err().to_string().contains("invalid_field"));
    let mut forged = base.clone();
    forged["clientRequestId"] = uuid::Uuid::new_v4().to_string().into();
    forged["author"] = "bot:visible".into();
    assert!(f.call("donnaTaskComment", forged).await.unwrap_err().to_string().contains("invalid_field"));
    let mut forged_check = base.clone();
    forged_check["clientRequestId"] = uuid::Uuid::new_v4().to_string().into();
    forged_check["mentionCheck"] = 2.into();
    assert!(f.call("donnaTaskComment", forged_check).await.unwrap_err().to_string().contains("invalid_field"));
    let replay = f.call("donnaTaskComment", base).await.unwrap();
    assert_eq!(replay["comment"]["id"], first["comment"]["id"]);
    assert_eq!(replay["receipt"], first["receipt"]);
}

#[tokio::test]
async fn geral_padrao_e_idempotente_e_continua_editavel() {
    let f = Fixture::new();
    let projects = f.call("donnaProjects", f.context()).await.unwrap();
    assert!(projects["defaultProjectId"].is_null());
    let mut extended = f.context();
    extended["historyAuthors"] = true.into();
    let ensured = f.call("donnaProjectEnsureDefault", extended.clone()).await.unwrap();
    assert_eq!(ensured["created"], true);
    let project = &ensured["snapshot"]["project"];
    assert_eq!(project["name"], "Geral");
    assert_eq!(project["isDefault"], true);
    assert_eq!(ensured["snapshot"]["history"][0]["author"], "system");
    let again = f.call("donnaProjectEnsureDefault", extended).await.unwrap();
    assert_eq!(again["created"], false);
    assert_eq!(again["snapshot"]["project"]["id"], project["id"]);
    let list = f.call("donnaProjects", f.context()).await.unwrap();
    assert_eq!(list["defaultProjectId"], project["id"]);
    let mut save = f.save(&ensured["snapshot"]);
    save["name"] = "Geral editado".into();
    let saved = f.call("donnaProjectSave", save).await.unwrap();
    assert_eq!(saved["snapshot"]["project"]["name"], "Geral editado");
    assert_eq!(saved["snapshot"]["project"]["isDefault"], true);
    let list = f.call("donnaProjects", f.context()).await.unwrap();
    assert_eq!(list["defaultProjectId"], project["id"]);
}

#[tokio::test]
async fn historico_sem_historyauthors_projeta_apenas_autor_dono() {
    let f = Fixture::new();
    let created = f.call("donnaProjectCreate", f.create()).await.unwrap();
    let project = created["snapshot"]["project"]["id"].as_str().unwrap().to_owned();
    {
        let db = f.hub.store.db.locked();
        store::history_as(
            &db,
            (f.hub.host_id.as_str(), PERSONAL, project.as_str()),
            crate::store::now_ms(),
            "system",
            "updated",
            None,
            "Entrada de sistema.",
        )
        .unwrap();
        store::history_as(
            &db,
            (f.hub.host_id.as_str(), PERSONAL, project.as_str()),
            crate::store::now_ms(),
            "bot:visible",
            "task_updated",
            None,
            "Entrada de bot.",
        )
        .unwrap();
    }
    let mut read = f.context();
    read["projectId"] = project.clone().into();
    let legacy = f.call("donnaProject", read.clone()).await.unwrap();
    let authors: Vec<&str> =
        legacy["history"].as_array().unwrap().iter().filter_map(|e| e["author"].as_str()).collect();
    assert_ne!(authors, Vec::<&str>::new());
    assert!(authors.iter().all(|author| *author == "owner"));
    assert_eq!(legacy["historyPartial"], true);
    assert_eq!(legacy["historyHidden"], 2);
    let mut extended = read;
    extended["historyAuthors"] = true.into();
    let full = f.call("donnaProject", extended).await.unwrap();
    let authors: Vec<&str> = full["history"].as_array().unwrap().iter().filter_map(|e| e["author"].as_str()).collect();
    assert!(authors.contains(&"owner"));
    assert!(authors.contains(&"system"));
    assert!(authors.contains(&"bot:visible"));
    assert!(full["historyPartial"].is_null());
}

#[tokio::test]
async fn comentario_exige_arroba_real_e_renomeacao_exige_nome_atual() {
    let f = Fixture::new();
    let created = f.call("donnaProjectCreate", f.create()).await.unwrap();
    let mut save = f.save(&created["snapshot"]);
    save["tasks"] = json!([task()]);
    let saved = f.call("donnaProjectSave", save).await.unwrap();
    let snapshot = &saved["snapshot"];
    let project = snapshot["project"]["id"].as_str().unwrap();
    let task_id = snapshot["tasks"][0]["id"].as_str().unwrap();
    let write = |text: &str, mentions: Value, only_note: bool| json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"projectId":project,"taskId":task_id,"clientRequestId":uuid::Uuid::new_v4().to_string(),"text":text,"mentions":mentions,"onlyNote":only_note});
    // Lista divergente: mentions elegível sem @ no corpo é recusada na gravação.
    assert!(
        f.call("donnaTaskComment", write("nota privada sem arroba", json!(["visible"]), false))
            .await
            .unwrap_err()
            .to_string()
            .contains("invalid_field")
    );
    // @ em contexto de palavra não conta; onlyNote com @ real segue aceita.
    assert!(
        f.call("donnaTaskComment", write("email@visible.com", json!(["visible"]), false))
            .await
            .unwrap_err()
            .to_string()
            .contains("invalid_field")
    );
    let note = f.call("donnaTaskComment", write("@visible só anotar", json!(["visible"]), true)).await.unwrap();
    assert_eq!(note["comment"]["notify"]["reason"], "only_note");
    // Renomear o bot não reescreve a audiência gravada, mas novas menções
    // passam a exigir o nome atual.
    let cfg: BotConfig =
        serde_json::from_value(json!({"id":"visible","name":"Renomeado","spaceId":PERSONAL,"backend":"fixture"}))
            .unwrap();
    f.hub.store.save_bot(&cfg).unwrap();
    assert!(
        f.call("donnaTaskComment", write("@visible antigo", json!(["visible"]), false))
            .await
            .unwrap_err()
            .to_string()
            .contains("invalid_field")
    );
    let renamed = f.call("donnaTaskComment", write("@Renomeado atual", json!(["visible"]), false)).await.unwrap();
    assert_eq!(renamed["comment"]["mentions"], json!(["visible"]));
}

#[tokio::test]
async fn declaracoes_de_conclusao_por_bot_sao_fato_gravado() {
    let f = Fixture::new();
    let created = f.call("donnaProjectCreate", f.create()).await.unwrap();
    let mut save = f.save(&created["snapshot"]);
    save["tasks"] = json!([task()]);
    let saved = f.call("donnaProjectSave", save).await.unwrap();
    let snapshot = &saved["snapshot"];
    let project = snapshot["project"]["id"].as_str().unwrap().to_owned();
    let task_id = snapshot["tasks"][0]["id"].as_str().unwrap().to_owned();
    // Sem declaração gravada o mapa existe e vem vazio (legado = desconhecido).
    assert_eq!(snapshot["declarations"], json!({}));
    {
        let db = f.hub.store.db.locked();
        store::declare_task_done(&db, f.hub.host_id.as_str(), PERSONAL, &project, &task_id, "visible").unwrap();
    }
    let read = f
        .call(
            "donnaProject",
            json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"projectId":project,"historyAuthors":true}),
        )
        .await
        .unwrap();
    assert_eq!(read["declarations"][task_id.as_str()]["botId"], "visible");
    let page =
        f.call("donnaTasks", json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"afterProjectId":null})).await.unwrap();
    assert_eq!(page["page"]["declarations"][task_id.as_str()]["botId"], "visible");
    // Save do dono que mantém done→done preserva a declaração.
    let mut keep = f.save(&read);
    keep["tasks"][0]["state"] = "done".into();
    let kept = f.call("donnaProjectSave", keep).await.unwrap();
    // A tarefa estava backlog: done agora é transição do dono → declaração some.
    assert_eq!(kept["snapshot"]["declarations"], json!({}));
    // Redeclara e salva com done→done: preserva.
    {
        let db = f.hub.store.db.locked();
        store::declare_task_done(&db, f.hub.host_id.as_str(), PERSONAL, &project, &task_id, "visible").unwrap();
    }
    let mut again = f.save(&kept["snapshot"]);
    again["tasks"][0]["state"] = "done".into();
    let preserved = f.call("donnaProjectSave", again).await.unwrap();
    assert_eq!(preserved["snapshot"]["declarations"][task_id.as_str()]["botId"], "visible");
    // Sair de done limpa.
    let mut reopen = f.save(&preserved["snapshot"]);
    reopen["tasks"][0]["state"] = "in_progress".into();
    let reopened = f.call("donnaProjectSave", reopen).await.unwrap();
    assert_eq!(reopened["snapshot"]["declarations"], json!({}));
}
