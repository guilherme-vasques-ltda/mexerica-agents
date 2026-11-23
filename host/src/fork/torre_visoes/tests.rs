#![allow(clippy::unwrap_used)]

use super::{METHOD, call};
use crate::{
    LockExt,
    api::{devices::Caller, dispatch},
    fork::{autoridade, spaces::PERSONAL},
    hub::Hub,
    remote::identity::Identity,
    store::{BotConfig, Device, DeviceSource, Scope, Store},
};
use rusqlite::params;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

struct Fixture {
    hub: Arc<Hub>,
    dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("tower-views-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let hub = Hub::new(
            Store::open(Path::new(":memory:")).unwrap(),
            "fixture-hub".into(),
            Identity::load_or_create(&dir).unwrap(),
            "fixture".into(),
            0,
        );
        Self { hub, dir }
    }

    fn bot(&self, id: &str, space: &str) {
        let config: BotConfig = serde_json::from_value(json!({
            "id":id,"name":id,"spaceId":space,"backend":"fixture"
        }))
        .unwrap();
        self.hub.store.save_bot(&config).unwrap();
    }

    fn graph(&self, bot: &str, role: &str, manager: Option<&str>) {
        let db = self.hub.store.db.locked();
        db.execute("INSERT OR IGNORE INTO donna_team_revisions VALUES(?1,0)", [PERSONAL]).unwrap();
        db.execute(
            "INSERT OR REPLACE INTO donna_bot_org(space_id,bot_id,role,reports_to_bot_id,owner_contact)
             VALUES(?1,?2,?3,?4,'reports')",
            params![PERSONAL, bot, role, manager],
        )
        .unwrap();
    }

    async fn space(&self) -> String {
        let initial = self.hub.store.spaces().unwrap();
        let created = dispatch(
            &self.hub,
            &Caller::Local,
            "donnaSpaceCreate",
            json!({
                "hubId":self.hub.host_id,"expectedRevision":initial.revision,"name":"Outro Space",
                "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"OS"}}
            }),
        )
        .await
        .unwrap();
        created["spaces"].as_array().unwrap().last().unwrap()["id"].as_str().unwrap().to_owned()
    }

    fn project(&self, hub: &str, space: &str, id: &str, coordinator: Option<&str>, tasks: &[Value]) {
        self.hub
            .store
            .db
            .locked()
            .execute(
                "INSERT INTO donna_projects(hub_id,space_id,id,project,tasks) VALUES(?1,?2,?3,?4,?5)",
                params![
                    hub,
                    space,
                    id,
                    json!({
                        "id":id,"name":id,"state":"active","createdAt":10,"updatedAt":90,
                        "coordinatorBotId":coordinator,"objective":"objetivo-nao-projetado"
                    })
                    .to_string(),
                    serde_json::to_string(tasks).unwrap()
                ],
            )
            .unwrap();
    }

    async fn read(&self, body: Value) -> Value {
        call(&self.hub, &Caller::Local, body).await.unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).unwrap();
    }
}

fn task(id: &str, bot: Option<&str>, state: &str) -> Value {
    json!({
        "id":id,"title":id,"assigneeBotId":bot,"state":state,
        "description":"descricao-nao-projetada","acceptance":"criterios-nao-projetados",
        "evidence":"/pasta-ficticia/arquivo-privado"
    })
}

#[tokio::test]
async fn visoes_filtram_space_mandato_executor_e_contadores() {
    let f = Fixture::new();
    let other_space = f.space().await;
    for id in ["diretor", "executor"] {
        f.bot(id, PERSONAL);
    }
    f.graph("diretor", "manager", None);
    f.graph("executor", "executor", Some("diretor"));
    let hub = &f.hub.host_id;
    let mut assigned = task("atribuida", Some("executor"), "in_progress");
    assigned["parentTaskId"] = "pai-fora-da-visao".into();
    f.project(
        hub,
        PERSONAL,
        "projeto",
        None,
        &[
            assigned,
            task("pai-fora-da-visao", Some("outro"), "review"),
            task("por-mandato", None, "review"),
            task("sem-roster", Some("ausente"), "backlog"),
        ],
    );
    f.project(hub, PERSONAL, "coordenado", Some("diretor"), &[task("por-projeto", None, "blocked")]);
    f.project(hub, &other_space, "projeto-reservado", None, &[task("tarefa-privada", None, "review")]);
    f.project("outro-hub", PERSONAL, "projeto-outro-hub", None, &[task("tarefa-outro-hub", None, "review")]);
    f.hub
        .store
        .db
        .locked()
        .execute(
            "INSERT INTO donna_task_delegation_meta(hub_id,space_id,project_id,task_id,origin_kind,requester_kind,
         requester_bot_id,root_bot_id,delegated_by_bot_id,mandate_director_bot_id,next_action)
         VALUES(?1,?2,'projeto','por-mandato','delegated','bot','solicitante','raiz','delegante','diretor','Conferir')",
            params![hub, PERSONAL],
        )
        .unwrap();
    crate::fork::projetos::delegacao_meta::admit(
        &f.hub.store.db.locked(),
        (hub, PERSONAL, "projeto"),
        "atribuida",
        9,
        None,
        None,
    )
    .unwrap();

    let owner = f.read(json!({"spaceId":PERSONAL,"perspective":"overview"})).await;
    let director = f.read(json!({"spaceId":PERSONAL,"perspective":"director","directorBotId":"diretor"})).await;
    let bot = f.read(json!({"spaceId":PERSONAL,"perspective":"bot","botId":"executor"})).await;
    assert_eq!(owner["counts"], json!({"needsYou":2,"observedWorking":1,"waitingOrBlocked":1,"totalVisible":5}));
    assert_eq!(director["counts"], json!({"needsYou":1,"observedWorking":1,"waitingOrBlocked":1,"totalVisible":3}));
    assert_eq!(bot["counts"], json!({"needsYou":0,"observedWorking":1,"waitingOrBlocked":0,"totalVisible":1}));
    assert_eq!(
        f.read(json!({"spaceId":PERSONAL,"perspective":"bot","botId":"ausente"})).await["cards"][0]["taskId"],
        "sem-roster"
    );
    assert_eq!(bot["cards"][0]["taskId"], "atribuida");
    assert_eq!(bot["cards"][0]["parentTaskId"], "pai-fora-da-visao");
    let assigned = &director["cards"][1];
    assert_eq!(assigned["taskId"], "atribuida");
    assert!(assigned["directorBotId"].is_null());
    assert_eq!(assigned["capabilities"]["canDirectorInterrupt"], false);
    let legacy = director["cards"].as_array().unwrap().iter().find(|card| card["taskId"] == "por-projeto").unwrap();
    assert_eq!(legacy["origin"]["kind"], "unknown");
    assert_eq!(legacy["capabilities"]["canDirectorInterrupt"], false);
    let mandate = &director["cards"][2];
    assert_eq!(mandate["taskId"], "por-mandato");
    assert_eq!(mandate["nextAction"], "Conferir");
    assert_eq!(mandate["requesterBotId"], "solicitante");
    assert_eq!(mandate["origin"]["rootBotId"], "raiz");
    assert_eq!(mandate["capabilities"], json!({"canDirectorInterrupt":true,"canOwnerAccept":false}));
    for reply in [&owner, &director, &bot] {
        let wire = reply.to_string();
        for secret in [
            "projeto-reservado",
            "outro-hub",
            "arquivo-privado",
            "descricao-nao-projetada",
            "criterios-nao-projetados",
            "objetivo-nao-projetado",
        ] {
            assert!(!wire.contains(secret), "não expor {secret}");
        }
    }
    assert!(!director["cards"].as_array().unwrap().iter().any(|card| card["taskId"] == "pai-fora-da-visao"));
    assert!(!bot.to_string().contains("por-mandato"));
}

#[tokio::test]
async fn declarado_nao_vira_execucao_e_entrega_nao_vira_conversa() {
    let f = Fixture::new();
    let hub = &f.hub.host_id;
    let mut active = task("andamento-declarado", Some("executor"), "in_progress");
    active["updatedAt"] = 33.into();
    f.project(
        hub,
        PERSONAL,
        "projeto",
        None,
        &[
            active,
            task("conferir", Some("executor"), "review"),
            task("aceita", None, "done"),
            task("pausada", Some("executor"), "in_progress"),
            task("cancelada", None, "cancelled"),
        ],
    );
    {
        let db = f.hub.store.db.locked();
        crate::fork::projetos::delegacao_meta::admit(
            &db,
            (hub, PERSONAL, "projeto"),
            "andamento-declarado",
            9,
            None,
            None,
        )
        .unwrap();
        db.execute(
            "INSERT INTO donna_task_delegation_meta(hub_id,space_id,project_id,task_id,origin_kind,requester_kind,blocker_kind,blocker_reason)
         VALUES(?1,?2,'projeto','pausada','unknown','unknown','permission_pause','Autorização pendente')",
            params![hub, PERSONAL],
        ).unwrap();
    }
    let snapshot = f.read(json!({"spaceId":PERSONAL,"perspective":"overview"})).await;
    let cards = snapshot["cards"].as_array().unwrap();
    assert_eq!(cards[0]["bucket"], "observed_working");
    assert_eq!(cards[0]["declaredUpdatedAt"], 33);
    assert_eq!(
        cards[0]["origin"],
        json!({"kind":"owner_direct","rootBotId":null,"delegatedByBotId":null,"routineId":null,"admittedAt":9})
    );
    assert_eq!(cards[1]["declaredUpdatedAt"], 90);
    assert_eq!(cards[1]["origin"]["kind"], "unknown");
    assert_eq!(cards[1]["bucket"], "needs_you");
    assert_eq!(cards[1]["capabilities"]["canOwnerAccept"], true);
    assert_eq!(cards[2]["delivery"], json!({"hasEvidence":true,"evidenceSummary":null,"acceptedByOwner":true}));
    assert_eq!(cards[3]["bucket"], "waiting_or_blocked");
    assert_eq!(
        cards[3]["blocker"],
        json!({"kind":"permission_pause","reason":"Autorização pendente","attentionItemId":null})
    );
    assert_eq!(cards.len(), 4);
    for card in cards {
        assert_eq!(
            card["observedExecution"],
            json!({"active":false,"status":"unavailable","observedAt":null,"machineLabel":null})
        );
        assert_eq!(card["capabilities"]["canDirectorInterrupt"], false);
    }
    assert!(!snapshot.to_string().contains("cancelada"));
}

#[tokio::test]
async fn recusa_seletores_incompativeis_e_autoridade_revogada() {
    let f = Fixture::new();
    let overview = json!({"spaceId":PERSONAL,"perspective":"overview"});
    for body in [
        json!({"spaceId":PERSONAL,"perspective":"director"}),
        json!({"spaceId":PERSONAL,"perspective":"bot","botId":"  "}),
        json!({"spaceId":PERSONAL,"perspective":"overview","directorBotId":"diretor"}),
        json!({"spaceId":PERSONAL,"perspective":"bot","botId":"executor","directorBotId":"diretor"}),
        json!({"spaceId":"inexistente","perspective":"overview"}),
        json!({"hubId":"hub-inexistente","spaceId":PERSONAL,"perspective":"overview"}),
        json!({"spaceId":PERSONAL,"perspective":"overview","inventado":true}),
    ] {
        assert!(call(&f.hub, &Caller::Local, body).await.is_err());
    }
    assert!(call(&f.hub, &Caller::Pairing { key: "pareamento".into() }, overview.clone()).await.is_err());
    let mut device = Device {
        key: "fixture-controle".into(),
        name: "fixture".into(),
        platform: "fixture".into(),
        source: DeviceSource::Local,
        grant_id: None,
        scopes: vec![Scope::Control],
        lease_until: None,
        created_at: 0,
        last_seen_at: None,
    };
    f.hub.store.put_device(&device).unwrap();
    let (_, caller) = autoridade::autenticar(&f.hub, &device.key).unwrap();
    assert!(
        dispatch(&f.hub, &caller, METHOD, json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"perspective":"overview"}))
            .await
            .is_ok()
    );
    let guard = f.hub.store.team_lock.lock().await;
    let read = call(&f.hub, &caller, overview);
    tokio::pin!(read);
    assert!(tokio::time::timeout(std::time::Duration::ZERO, &mut read).await.is_err());
    device.scopes = vec![Scope::Screen];
    f.hub.store.put_device(&device).unwrap();
    drop(guard);
    assert!(read.await.is_err());
}

#[tokio::test]
async fn falha_do_produtor_nao_vira_lista_vazia() {
    let f = Fixture::new();
    f.hub.store.db.locked().execute("DROP TABLE donna_task_delegation_meta", []).unwrap();
    let result = call(&f.hub, &Caller::Local, json!({"spaceId":PERSONAL,"perspective":"overview"})).await;
    assert!(result.is_err());
}
