#![allow(clippy::unwrap_used)]
use super::*;
use crate::{
    LockExt,
    fork::sessoes::bindings,
    fork::spaces::PERSONAL,
    hub::Hub,
    remote::identity::Identity,
    store::{BotConfig, Store},
};
use rusqlite::params;
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

struct Fixture {
    hub: Arc<Hub>,
}

fn task(id: &str, parent: Option<&str>, assignee: Option<&str>, title: &str) -> Value {
    json!({
        "id": id, "parentTaskId": parent, "assigneeBotId": assignee,
        "title": title, "description": "", "acceptance": "", "evidence": "", "state": "backlog",
    })
}

impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("mx142-tarefas-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let hub = Hub::new(
            Store::open(Path::new(":memory:")).unwrap(),
            "tarefas-fixture".into(),
            Identity::load_or_create(&dir).unwrap(),
            "fixture".into(),
            0,
        );
        hub.store.db.locked().execute_batch("PRAGMA foreign_keys=ON").unwrap();
        for id in ["a", "b", "c", "boss"] {
            let cfg: BotConfig =
                serde_json::from_value(json!({"id":id,"name":id,"spaceId":PERSONAL,"backend":"fixture"})).unwrap();
            hub.store.save_bot(&cfg).unwrap();
        }
        Self { hub }
    }

    fn exec(&self, bot: &str) -> ExecutionRef {
        bindings::fixture_execution(&self.hub, bot)
    }

    async fn call(&self, bot: &str, name: &str, args: Value, execution: Option<&ExecutionRef>) -> Result<Value> {
        call_in_execution(&self.hub, bot, name, &args, execution).await
    }

    fn project(&self, space: &str, coordinator: Option<&str>, tasks: &[Value]) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let project = json!({
            "id": id, "name": "Plano", "objective": "", "coordinatorBotId": coordinator,
            "state": "active", "revision": 1, "createdAt": 0, "updatedAt": 0,
        });
        self.hub
            .store
            .db
            .locked()
            .execute(
                "INSERT INTO donna_projects(hub_id,space_id,id,project,tasks) VALUES(?1,?2,?3,?4,?5)",
                params![
                    self.hub.host_id,
                    space,
                    id,
                    serde_json::to_string(&project).unwrap(),
                    serde_json::to_string(&tasks).unwrap()
                ],
            )
            .unwrap();
        id
    }

    /// `reports_to`: o bot passa a reportar a esse bot (revisões/org do Space já existem ou são criadas).
    fn org(&self, bot: &str, reports_to: Option<&str>) {
        let c = self.hub.store.db.locked();
        c.execute("INSERT OR IGNORE INTO donna_team_revisions(space_id,revision) VALUES(?1,0)", [PERSONAL]).unwrap();
        c.execute(
            "INSERT OR IGNORE INTO donna_bot_org(space_id,bot_id,role,reports_to_bot_id,owner_contact) VALUES(?1,?2,'executor',NULL,'reports')",
            params![PERSONAL, bot],
        )
        .unwrap();
        if let Some(up) = reports_to {
            c.execute(
                "INSERT OR IGNORE INTO donna_bot_org(space_id,bot_id,role,reports_to_bot_id,owner_contact) VALUES(?1,?2,'executor',NULL,'reports')",
                params![PERSONAL, up],
            )
            .unwrap();
            c.execute(
                "UPDATE donna_bot_org SET reports_to_bot_id=?3 WHERE space_id=?1 AND bot_id=?2",
                params![PERSONAL, bot, up],
            )
            .unwrap();
        }
    }

    fn stored(&self, project: &str) -> (Value, Vec<Value>) {
        let c = self.hub.store.db.locked();
        let (p, t): (String, String) = c
            .query_row(
                "SELECT project,tasks FROM donna_projects WHERE hub_id=?1 AND space_id=?2 AND id=?3",
                params![self.hub.host_id, PERSONAL, project],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        (serde_json::from_str(&p).unwrap(), serde_json::from_str(&t).unwrap())
    }

    fn history(&self, project: &str) -> Vec<Value> {
        let c = self.hub.store.db.locked();
        let mut st = c
            .prepare("SELECT entry FROM donna_project_history WHERE hub_id=?1 AND space_id=?2 AND project_id=?3 ORDER BY seq")
            .unwrap();
        st.query_map(params![self.hub.host_id, PERSONAL, project], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| serde_json::from_str(&r.unwrap()).unwrap())
            .collect()
    }

    fn count(&self, sql: &str, project: Option<&str>) -> i64 {
        let c = self.hub.store.db.locked();
        match project {
            Some(p) => c.query_row(sql, params![self.hub.host_id, PERSONAL, p], |r| r.get(0)).unwrap(),
            None => c.query_row(sql, [], |r| r.get(0)).unwrap(),
        }
    }
}

#[tokio::test]
async fn dona_move_para_review_e_atualiza_descricao_com_historico() {
    let f = Fixture::new();
    let mut owned = task("t1", None, Some("a"), "Subtarefa");
    owned["metaX"] = json!({"preservar": true});
    let project = f.project(PERSONAL, Some("boss"), &[owned]);
    let exec = f.exec("a");
    let out = f
        .call(
            "a",
            "task_update",
            json!({"projectId": project, "taskId": "t1", "state": "review", "description": "nota"}),
            Some(&exec),
        )
        .await
        .unwrap();
    assert_eq!(out["task"]["state"], "review");
    let (p, tasks) = f.stored(&project);
    assert_eq!(p["revision"], 2);
    assert_eq!(tasks[0]["state"], "review");
    assert_eq!(tasks[0]["description"], "nota");
    assert_eq!(tasks[0]["metaX"]["preservar"], true);
    let history = f.history(&project);
    assert_eq!(history.len(), 1);
    assert_eq!(history[0]["author"], "bot:a");
    assert_eq!(history[0]["action"], "task_updated");
    assert_eq!(history[0]["taskId"], "t1");
}

/// Concluir e registrar prova são gestos só do usuário: responsável,
/// supervisor e coordenador são recusados e nada muda no banco.
#[tokio::test]
async fn concluir_e_prova_sao_gestos_so_do_usuario() {
    let f = Fixture::new();
    // "a" é a responsável; "b" a supervisora direta; "boss" a coordenadora.
    f.org("a", Some("b"));
    let mut owned = task("t1", None, Some("a"), "Sub");
    owned["state"] = json!("in_progress");
    owned["evidence"] = json!("prova registrada pelo usuário");
    let project = f.project(PERSONAL, Some("boss"), &[owned]);
    crate::fork::projetos::delegacao_meta::admit(
        &f.hub.store.db.locked(),
        (&f.hub.host_id, PERSONAL, &project),
        "t1",
        1,
        Some("boss"),
        None,
    )
    .unwrap();
    for bot in ["a", "b", "boss"] {
        let exec = f.exec(bot);
        for extra in [
            json!({"state": "done"}),
            json!({"state": "done", "evidence": "PR #1 mergeado"}),
            json!({"evidence": "PR #1 mergeado"}),
            json!({"evidence": ""}),
            json!({"evidence": null}),
        ] {
            let mut args = json!({"projectId": project, "taskId": "t1"});
            args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            let err = f.call(bot, "task_update", args, Some(&exec)).await.unwrap_err();
            assert!(format!("{err:#}").contains("task_forbidden"), "{bot}: {err:#}");
        }
    }
    // Estado fora da lista de bot é invalid_field — nunca chega ao banco.
    let exec_a = f.exec("a");
    for extra in [json!({"state": "cancelled"}), json!({"state": "archived"}), json!({"state": 7})] {
        let mut args = json!({"projectId": project, "taskId": "t1"});
        args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let err = f.call("a", "task_update", args, Some(&exec_a)).await.unwrap_err();
        assert!(format!("{err:#}").contains("invalid_field"), "{err:#}");
    }
    let (p, tasks) = f.stored(&project);
    assert_eq!(p["revision"], 1);
    assert_eq!(tasks[0]["state"], "in_progress");
    assert_eq!(tasks[0]["evidence"], "prova registrada pelo usuário");
    assert_eq!(f.history(&project).len(), 0);
    assert_eq!(
        f.count(
            "SELECT COUNT(*) FROM donna_task_declarations WHERE hub_id=?1 AND space_id=?2 AND project_id=?3",
            Some(&project)
        ),
        0
    );
}

/// Tarefa concluída é do usuário: nenhum bot reabre, altera descrição ou
/// toca a prova — a recusa é `task_accepted` e a tarefa sai intacta.
#[tokio::test]
async fn tarefa_concluida_so_o_usuario_altera() {
    let f = Fixture::new();
    f.org("a", Some("b"));
    let mut done_task = task("t1", None, Some("a"), "Sub");
    done_task["state"] = json!("done");
    done_task["evidence"] = json!("prova registrada pelo usuário");
    let project = f.project(PERSONAL, Some("boss"), &[done_task]);
    // Declaração legada de conclusão: nenhum bot pode apagá-la.
    {
        let c = f.hub.store.db.locked();
        c.execute(
            "INSERT INTO donna_task_declarations(hub_id,space_id,project_id,task_id,bot_id,at) VALUES(?1,?2,?3,?4,'a',1)",
            params![f.hub.host_id, PERSONAL, project, "t1"],
        )
        .unwrap();
    }
    for bot in ["a", "b", "boss"] {
        let exec = f.exec(bot);
        for extra in [
            json!({"state": "in_progress"}),
            json!({"state": "review"}),
            json!({"state": "backlog"}),
            json!({"state": "blocked"}),
            json!({"description": "nota"}),
            json!({"evidence": "outra prova"}),
        ] {
            let mut args = json!({"projectId": project, "taskId": "t1"});
            args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            let err = f.call(bot, "task_update", args, Some(&exec)).await.unwrap_err();
            assert!(format!("{err:#}").contains("task_accepted"), "{bot}: {err:#}");
        }
    }
    let (p, tasks) = f.stored(&project);
    assert_eq!(p["revision"], 1);
    assert_eq!(tasks[0]["state"], "done");
    assert_eq!(tasks[0]["description"], "");
    assert_eq!(tasks[0]["evidence"], "prova registrada pelo usuário");
    assert_eq!(f.history(&project).len(), 0);
    assert_eq!(
        f.count(
            "SELECT COUNT(*) FROM donna_task_declarations WHERE hub_id=?1 AND space_id=?2 AND project_id=?3",
            Some(&project)
        ),
        1
    );
}

/// O schema publicado de `task_update` não oferece `done` nem `evidence`.
#[test]
fn schema_task_update_nao_expoe_done_nem_evidence() {
    let list = tools();
    let update = list.as_array().unwrap().iter().find(|t| t["name"] == "task_update").unwrap();
    let schema = &update["inputSchema"];
    let states = schema["properties"]["state"]["enum"].as_array().unwrap();
    assert!(!states.iter().any(|s| s == "done"), "{states:?}");
    assert_eq!(states, &json!(["backlog", "in_progress", "review", "blocked"]).as_array().unwrap().clone());
    assert!(schema["properties"].as_object().unwrap().get("evidence").is_none());
    assert_eq!(schema["additionalProperties"], false);
}

#[tokio::test]
async fn execucao_revogada_ou_ausente_recusa() {
    let f = Fixture::new();
    let project = f.project(PERSONAL, None, &[task("t1", None, Some("a"), "Sub")]);
    let exec = f.exec("a");
    bindings::revoke(&f.hub.store, "a", None).unwrap();
    let err = f
        .call("a", "task_update", json!({"projectId": project, "taskId": "t1", "state": "in_progress"}), Some(&exec))
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("chamada vencida"), "{err:#}");
    let err = f
        .call("a", "task_update", json!({"projectId": project, "taskId": "t1", "state": "in_progress"}), None)
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("execução ausente"), "{err:#}");
}

#[tokio::test]
async fn identidade_forjada_recusa() {
    let f = Fixture::new();
    let project = f.project(PERSONAL, None, &[task("t1", None, Some("a"), "Sub")]);
    let exec_a = f.exec("a");
    let err = f
        .call("b", "task_update", json!({"projectId": project, "taskId": "t1", "state": "in_progress"}), Some(&exec_a))
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("chamada vencida"), "{err:#}");
}

#[tokio::test]
async fn isolamento_entre_spaces() {
    let f = Fixture::new();
    let other = uuid::Uuid::new_v4().to_string();
    let project = f.project(&other, None, &[task("t1", None, Some("a"), "Sub")]);
    let exec = f.exec("a");
    let err = f.call("a", "task_read", json!({"projectId": project}), Some(&exec)).await.unwrap_err();
    assert!(format!("{err:#}").contains("project_not_found"), "{err:#}");
}

#[tokio::test]
async fn matriz_coordenador_supervisor_e_sem_autoridade() {
    let f = Fixture::new();
    f.org("a", None);
    f.org("b", Some("a"));
    let project = f.project(PERSONAL, Some("boss"), &[task("t1", None, Some("b"), "Sub de b")]);
    crate::fork::projetos::delegacao_meta::admit(
        &f.hub.store.db.locked(),
        (&f.hub.host_id, PERSONAL, &project),
        "t1",
        1,
        Some("boss"),
        None,
    )
    .unwrap();
    // Coordenador do Projeto atualiza a subtarefa de outro bot.
    let exec_boss = f.exec("boss");
    f.call("boss", "task_update", json!({"projectId": project, "taskId": "t1", "state": "review"}), Some(&exec_boss))
        .await
        .unwrap();
    // Supervisor direto do responsável ("a" é reports_to de "b") também.
    let exec_a = f.exec("a");
    f.call("a", "task_update", json!({"projectId": project, "taskId": "t1", "state": "blocked"}), Some(&exec_a))
        .await
        .unwrap();
    // Bot sem relação ("c") é recusado na escrita e na leitura da subtarefa.
    let exec_c = f.exec("c");
    let err = f
        .call("c", "task_update", json!({"projectId": project, "taskId": "t1", "state": "in_progress"}), Some(&exec_c))
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("task_forbidden"), "{err:#}");
    let err = f.call("c", "task_read", json!({"projectId": project, "taskId": "t1"}), Some(&exec_c)).await.unwrap_err();
    assert!(format!("{err:#}").contains("task_forbidden"), "{err:#}");
    // O responsável lê a própria completa.
    let exec_b = f.exec("b");
    let out = f.call("b", "task_read", json!({"projectId": project, "taskId": "t1"}), Some(&exec_b)).await.unwrap();
    assert_eq!(out["task"]["title"], "Sub de b");
    assert!(out["task"].get("evidence").is_some());
}

#[tokio::test]
async fn limites_100_tarefas_e_3_niveis() {
    let f = Fixture::new();
    let mut tasks: Vec<Value> = (0..99).map(|i| task(&format!("t{i}"), None, Some("a"), "x")).collect();
    tasks.push(task("root", None, Some("a"), "raiz"));
    let project = f.project(PERSONAL, None, &tasks);
    let exec = f.exec("a");
    // 100 tarefas: criar a 101ª recusa.
    let err = f
        .call(
            "a",
            "task_create_child",
            json!({"projectId": project, "parentTaskId": "root", "title": "nova"}),
            Some(&exec),
        )
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("task_limit"), "{err:#}");
    // Profundidade: r → f1 → f2 já ocupa os 3 níveis; filha de f2 recusa.
    let project2 = f.project(
        PERSONAL,
        None,
        &[
            task("r", None, Some("a"), "r"),
            task("f1", Some("r"), Some("a"), "f1"),
            task("f2", Some("f1"), Some("a"), "f2"),
        ],
    );
    let err = f
        .call(
            "a",
            "task_create_child",
            json!({"projectId": project2, "parentTaskId": "f2", "title": "f3"}),
            Some(&exec),
        )
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("task_limit"), "{err:#}");
    // Sob f1 ainda cabe mais uma (nível 3).
    let out = f
        .call(
            "a",
            "task_create_child",
            json!({"projectId": project2, "parentTaskId": "f1", "title": "f2b", "assigneeBotId": "b"}),
            Some(&exec),
        )
        .await
        .unwrap();
    assert_eq!(out["task"]["parentTaskId"], "f1");
    assert_eq!(out["task"]["assigneeBotId"], "b");
}

#[tokio::test]
async fn ordem_direta_protege_estado_e_responsavel_sem_impedir_conferencia() {
    use crate::fork::projetos::delegacao_meta;
    let f = Fixture::new();
    f.org("a", Some("b"));
    let mut owned = task("ordem", None, Some("a"), "Ordem direta");
    owned["state"] = json!("in_progress");
    let project = f.project(PERSONAL, Some("boss"), &[owned]);
    delegacao_meta::admit(&f.hub.store.db.locked(), (&f.hub.host_id, PERSONAL, &project), "ordem", 1, None, None)
        .unwrap();

    for bot in ["a", "b", "boss"] {
        let exec = f.exec(bot);
        for extra in [
            json!({"state": "backlog"}),
            json!({"state": "blocked"}),
            json!({"state": "cancelled"}),
            json!({"assigneeBotId": "c"}),
            json!({"assigneeBotId": null}),
        ] {
            let mut args = json!({"projectId": project, "taskId": "ordem"});
            args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            let err = f.call(bot, "task_update", args, Some(&exec)).await.unwrap_err();
            assert!(err.to_string().starts_with("direct_owner_order_protected:"), "{err}");
        }
    }
    assert_eq!(f.stored(&project).1[0]["state"], "in_progress");
    let exec = f.exec("a");
    f.call(
        "a",
        "task_update",
        json!({"projectId": project, "taskId": "ordem", "state": "review", "description": "Nota de progresso"}),
        Some(&exec),
    )
    .await
    .unwrap();
    f.call("a", "task_comment", json!({"projectId": project, "taskId": "ordem", "text": "Nota"}), Some(&exec))
        .await
        .unwrap();
    assert_eq!(f.stored(&project).1[0]["state"], "review");

    f.hub.store.db.locked().execute_batch("DROP TABLE donna_task_delegation_meta").unwrap();
    let err = f
        .call("a", "task_update", json!({"projectId": project, "taskId": "ordem", "state": "blocked"}), Some(&exec))
        .await
        .unwrap_err();
    assert!(err.to_string().starts_with("direct_owner_order_protected:"), "{err}");
    assert_eq!(f.stored(&project).1[0]["state"], "review");
}

#[tokio::test]
async fn tarefa_sem_metadados_nao_pode_ser_interrompida_por_bot() {
    let f = Fixture::new();
    let mut legacy = task("legada", None, Some("a"), "Tarefa legada");
    legacy["state"] = json!("in_progress");
    let project = f.project(PERSONAL, Some("boss"), &[legacy]);
    assert_eq!(
        f.count(
            "SELECT COUNT(*) FROM donna_task_delegation_meta WHERE hub_id=?1 AND space_id=?2 AND project_id=?3",
            Some(&project),
        ),
        0
    );
    for bot in ["a", "boss"] {
        let exec = f.exec(bot);
        for state in ["backlog", "blocked", "cancelled"] {
            let err = f
                .call(bot, "task_update", json!({"projectId":project,"taskId":"legada","state":state}), Some(&exec))
                .await
                .unwrap_err();
            assert!(err.to_string().starts_with("direct_owner_order_protected:"), "{err}");
        }
    }
    assert_eq!(f.stored(&project).1[0]["state"], "in_progress");
    assert_eq!(f.history(&project), [] as [Value; 0]);
}

#[tokio::test]
async fn governanca_limita_admissao_de_filhos_no_space_e_na_cadeia() {
    use crate::api::{devices::Caller, dispatch};
    use crate::fork::projetos::delegacao_meta::{OriginKind, load_space};
    let f = Fixture::new();
    let one = f.project(PERSONAL, None, &[task("raiz", None, Some("a"), "Raiz")]);
    let two = f.project(PERSONAL, None, &[task("raiz", None, Some("a"), "Outra raiz")]);
    let mut policy = json!({"titleInstruction":"", "autoArchiveIdleDays":3, "preservePinnedAndOpenWork":true,
        "maxConcurrentSubagents":2, "maxSubagentDepth":1});
    dispatch(
        &f.hub,
        &Caller::Local,
        "donnaGovernanceSave",
        json!({"hubId":f.hub.host_id, "spaceId":PERSONAL,
        "clientRequestId":uuid::Uuid::new_v4().to_string(), "expectedRevision":0, "policy":policy}),
    )
    .await
    .unwrap();
    let exec = f.exec("a");
    let args = json!({"projectId":one,"parentTaskId":"raiz","title":"Filha","assigneeBotId":"a",
        "requestId":uuid::Uuid::new_v4().to_string()});
    let child = f.call("a", "task_create_child", args.clone(), Some(&exec)).await.unwrap();
    let child_id = child["task"]["id"].as_str().unwrap();
    let err = f
        .call("a", "task_create_child", json!({"projectId":one,"parentTaskId":child_id,"title":"Neta"}), Some(&exec))
        .await
        .unwrap_err();
    assert!(err.to_string().starts_with("governance_depth_limit:"), "{err}");
    f.call("a", "task_create_child", json!({"projectId":two,"parentTaskId":"raiz","title":"Outra filha"}), Some(&exec))
        .await
        .unwrap();
    let err = f
        .call("a", "task_create_child", json!({"projectId":one,"parentTaskId":"raiz","title":"Excedente"}), Some(&exec))
        .await
        .unwrap_err();
    assert!(err.to_string().starts_with("governance_concurrency_limit:"), "{err}");
    let repeated = f.call("a", "task_create_child", args, Some(&exec)).await.unwrap();
    assert_eq!(repeated["task"]["id"], child_id);
    assert_eq!(f.stored(&one).1.len(), 2);
    assert_eq!(f.stored(&two).1.len(), 2);
    let meta = load_space(&f.hub.store.db.locked(), &f.hub.host_id, PERSONAL).unwrap();
    let admitted = &meta[&(one.clone(), child_id.to_owned())];
    assert_eq!(admitted.origin_kind, OriginKind::Delegated);
    assert_eq!(admitted.root_bot_id.as_deref(), Some("a"));
    assert_eq!(admitted.delegated_by_bot_id.as_deref(), Some("a"));

    policy["maxSubagentDepth"] = json!(2);
    dispatch(
        &f.hub,
        &Caller::Local,
        "donnaGovernanceSave",
        json!({"hubId":f.hub.host_id, "spaceId":PERSONAL,
        "clientRequestId":uuid::Uuid::new_v4().to_string(), "expectedRevision":1, "policy":policy}),
    )
    .await
    .unwrap();
    let (p, mut tasks) = f.stored(&one);
    tasks[1]["state"] = json!("done");
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE donna_projects SET tasks=?1 WHERE id=?2", params![json!(tasks).to_string(), p["id"].as_str()])
        .unwrap();
    f.call("a", "task_create_child", json!({"projectId":one,"parentTaskId":child_id,"title":"Neta"}), Some(&exec))
        .await
        .unwrap();
}

#[tokio::test]
async fn mencao_sem_arroba_nao_persiste_e_renomeacao_mantem() {
    let f = Fixture::new();
    let project = f.project(PERSONAL, None, &[task("t1", None, Some("a"), "Sub"), task("t2", None, Some("b"), "De b")]);
    // "a" é supervisor direto de "b": acesso pleno à subtarefa dele.
    f.org("b", Some("a"));
    let exec = f.exec("a");
    // Lista declarada sem @ real no texto: recusada, nada é persistido.
    let err = f
        .call(
            "a",
            "task_comment",
            json!({"projectId": project, "taskId": "t1", "text": "nota sem arroba", "mentions": ["b"]}),
            Some(&exec),
        )
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("invalid_field"), "{err:#}");
    // @ real persiste o botId verificado.
    let out = f
        .call(
            "a",
            "task_comment",
            json!({"projectId": project, "taskId": "t2", "text": "@b chamado", "mentions": ["b"]}),
            Some(&exec),
        )
        .await
        .unwrap();
    assert_eq!(out["comment"]["mentions"], json!(["b"]));
    assert_eq!(out["comment"]["mentionCheck"], json!(2));
    let comment_id = out["comment"]["id"].as_str().unwrap().to_owned();
    // Renomear o bot não apaga a menção persistida: a leitura é por botId.
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE bots SET config=json_set(config,'$.name','beta') WHERE json_extract(config,'$.id')='b'", [])
        .unwrap();
    let exec_b = f.exec("b");
    let read = f.call("b", "task_read", json!({"projectId": project, "taskId": "t2"}), Some(&exec_b)).await.unwrap();
    let comments = read["task"]["comments"].as_array().unwrap();
    let ids: Vec<&str> = comments.iter().filter_map(|c| c["id"].as_str()).collect();
    assert!(ids.contains(&comment_id.as_str()), "{ids:?}");
}

#[tokio::test]
async fn comentario_registra_sem_notificar_e_idempotente() {
    let f = Fixture::new();
    let project = f.project(PERSONAL, None, &[task("t1", None, Some("a"), "Sub")]);
    let exec = f.exec("a");
    let request = uuid::Uuid::new_v4().to_string();
    let args = json!({"projectId": project, "taskId": "t1", "text": "@b progresso parcial", "mentions": ["b"], "requestId": request});
    let out = f.call("a", "task_comment", args.clone(), Some(&exec)).await.unwrap();
    let comment = &out["comment"];
    assert_eq!(comment["author"], "bot:a");
    assert_eq!(comment["notify"]["state"], "not_notified");
    assert_eq!(comment["notify"]["reason"], "bot_comment");
    assert_eq!(comment["mentions"], json!(["b"]));
    let again = f.call("a", "task_comment", args, Some(&exec)).await.unwrap();
    assert_eq!(again["comment"]["id"], comment["id"]);
    assert_eq!(
        f.count(
            "SELECT COUNT(*) FROM donna_task_comments WHERE hub_id=?1 AND space_id=?2 AND project_id=?3",
            Some(&project)
        ),
        1
    );
    // Comentário de bot não cria pedido de sessão para o citado.
    assert_eq!(f.count("SELECT COUNT(*) FROM donna_session_requests", None), 0);
}

#[tokio::test]
async fn leitura_respeita_matriz() {
    let f = Fixture::new();
    let project = f.project(
        PERSONAL,
        None,
        &[
            task("plan", None, Some("boss"), "Plano"),
            task("mine", Some("plan"), Some("a"), "Minha"),
            task("sib", Some("plan"), Some("b"), "Irmã"),
            task("other", None, Some("c"), "Outra árvore"),
        ],
    );
    let exec = f.exec("a");
    let out = f.call("a", "task_read", json!({"projectId": project}), Some(&exec)).await.unwrap();
    let tasks = out["tasks"].as_array().unwrap();
    let ids: Vec<&str> = tasks.iter().filter_map(|t| t["id"].as_str()).collect();
    assert!(ids.contains(&"mine") && ids.contains(&"sib") && ids.contains(&"plan"));
    assert!(!ids.contains(&"other"));
    let mine = tasks.iter().find(|t| t["id"] == "mine").unwrap();
    let sib = tasks.iter().find(|t| t["id"] == "sib").unwrap();
    assert!(mine.get("evidence").is_some());
    assert!(sib.get("evidence").is_none());
    // "c" só tem a própria árvore: vê "other" e nada da árvore do plano.
    let exec_c = f.exec("c");
    let out_c = f.call("c", "task_read", json!({"projectId": project}), Some(&exec_c)).await.unwrap();
    let ids_c: Vec<&str> = out_c["tasks"].as_array().unwrap().iter().filter_map(|t| t["id"].as_str()).collect();
    assert_eq!(ids_c, ["other"]);
}

#[tokio::test]
async fn reply_to_comment_id_exige_prova_causal() {
    let f = Fixture::new();
    let project = f.project(PERSONAL, None, &[task("t1", None, Some("a"), "Sub")]);
    let exec = f.exec("a");
    let session = {
        let c = f.hub.store.db.locked();
        bindings::assert_execution_live(&c, &exec).unwrap().id
    };
    // Comentário do dono com menção: o trigger do runtime cria a entrega.
    let owner_comment = json!({
        "id": uuid::Uuid::new_v4().to_string(), "at": 1, "author": "owner",
        "text": "como vai?", "mentions": ["a"], "onlyNote": false,
        "notify": {"botIds": ["a"], "state": "sent"},
    });
    let comment_a = owner_comment["id"].as_str().unwrap().to_owned();
    let poke_a = uuid::Uuid::new_v4().to_string();
    {
        let c = f.hub.store.db.locked();
        c.execute(
            "INSERT INTO donna_task_comments(hub_id,space_id,project_id,task_id,comment_id,request_id,payload,entry) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                f.hub.host_id, PERSONAL, project, "t1", comment_a,
                uuid::Uuid::new_v4().to_string(), "payload",
                serde_json::to_string(&owner_comment).unwrap()
            ],
        )
        .unwrap();
        // Fluxo real do runtime: claim deixa a entrega 'sending' na sessão,
        // o poke entra (guard exige 'sending') e a entrega conclui 'sent'.
        c.execute(
            "UPDATE donna_task_deliveries SET status='sending',session_id=?3 WHERE hub_id=?1 AND comment_id=?2",
            params![f.hub.host_id, comment_a, session],
        )
        .unwrap();
        c.execute(
            "INSERT INTO entries(id,bot_id,rev,kind,turn,data,created_at,updated_at,session) VALUES(?1,?2,1,'user',7,?3,0,0,?4)",
            params![
                poke_a, "a",
                serde_json::to_string(&json!({"clientNonce": format!("mx142-task-aviso-{comment_a}-a")})).unwrap(),
                session
            ],
        )
        .unwrap();
        c.execute(
            "UPDATE donna_task_deliveries SET status='sent' WHERE hub_id=?1 AND comment_id=?2",
            params![f.hub.host_id, comment_a],
        )
        .unwrap();
        // Admissão viva do turno: mesmo execution_token e a entry-poke admitida.
        c.execute(
            "INSERT INTO kv(k,v) VALUES(?1,?2)",
            params![
                "turn.inflight.a",
                serde_json::to_string(&json!({
                    "admissionVersion": 1, "executionToken": exec.turn_token(), "attempt": "att",
                    "phase": "permitted", "conversationId": "a", "origin": {"kind": "message"},
                    "entryIds": [poke_a.clone()],
                }))
                .unwrap()
            ],
        )
        .unwrap();
    }
    // Sem prova (sem admissão) o reply não marca nada.
    {
        let c = f.hub.store.db.locked();
        c.execute("DELETE FROM kv WHERE k='turn.inflight.a'", []).unwrap();
    }
    let out = f
        .call(
            "a",
            "task_comment",
            json!({"projectId": project, "taskId": "t1", "text": "sem prova", "replyToCommentId": comment_a}),
            Some(&exec),
        )
        .await
        .unwrap();
    assert_eq!(out["answered"], false);
    // Admissão de OUTRA execução (token divergente) também não prova.
    {
        let c = f.hub.store.db.locked();
        c.execute(
            "INSERT INTO kv(k,v) VALUES('turn.inflight.a',?1)",
            [serde_json::to_string(&json!({
                "admissionVersion": 1, "executionToken": "outro-turno", "attempt": "att",
                "phase": "permitted", "conversationId": "a", "origin": {"kind": "message"},
                "entryIds": [poke_a.clone()],
            }))
            .unwrap()],
        )
        .unwrap();
    }
    let out = f
        .call(
            "a",
            "task_comment",
            json!({"projectId": project, "taskId": "t1", "text": "token errado", "replyToCommentId": comment_a}),
            Some(&exec),
        )
        .await
        .unwrap();
    assert_eq!(out["answered"], false);
    // Turno certo mas a entry-poke de OUTRO comentário não está admitida.
    {
        let c = f.hub.store.db.locked();
        c.execute("DELETE FROM kv WHERE k='turn.inflight.a'", []).unwrap();
        c.execute(
            "INSERT INTO kv(k,v) VALUES('turn.inflight.a',?1)",
            [serde_json::to_string(&json!({
                "admissionVersion": 1, "executionToken": exec.turn_token(), "attempt": "att",
                "phase": "permitted", "conversationId": "a", "origin": {"kind": "message"},
                "entryIds": [uuid::Uuid::new_v4().to_string()],
            }))
            .unwrap()],
        )
        .unwrap();
    }
    let out = f
        .call(
            "a",
            "task_comment",
            json!({"projectId": project, "taskId": "t1", "text": "entry errada", "replyToCommentId": comment_a}),
            Some(&exec),
        )
        .await
        .unwrap();
    assert_eq!(out["answered"], false);
    // Prova completa: token + entry-poke admitida -> answered, na mesma tx.
    {
        let c = f.hub.store.db.locked();
        c.execute("DELETE FROM kv WHERE k='turn.inflight.a'", []).unwrap();
        c.execute(
            "INSERT INTO kv(k,v) VALUES('turn.inflight.a',?1)",
            [serde_json::to_string(&json!({
                "admissionVersion": 1, "executionToken": exec.turn_token(), "attempt": "att",
                "phase": "permitted", "conversationId": "a", "origin": {"kind": "message"},
                "entryIds": [poke_a.clone()],
            }))
            .unwrap()],
        )
        .unwrap();
    }
    let out = f
        .call(
            "a",
            "task_comment",
            json!({"projectId": project, "taskId": "t1", "text": "respondido", "replyToCommentId": comment_a}),
            Some(&exec),
        )
        .await
        .unwrap();
    assert_eq!(out["answered"], true);
    let status: String = f
        .hub
        .store
        .db
        .locked()
        .query_row(
            "SELECT status FROM donna_task_deliveries WHERE hub_id=?1 AND comment_id=?2",
            params![f.hub.host_id, comment_a],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "answered");
}

#[tokio::test]
async fn reply_to_comment_id_dois_avisos_na_mesma_sessao() {
    let f = Fixture::new();
    let project = f.project(PERSONAL, None, &[task("t1", None, Some("a"), "Sub")]);
    let exec = f.exec("a");
    let session = {
        let c = f.hub.store.db.locked();
        bindings::assert_execution_live(&c, &exec).unwrap().id
    };
    // Dois comentários do dono -> duas entreguras `sent` na MESMA sessão.
    let mut comments = Vec::new();
    let mut pokes = Vec::new();
    {
        let c = f.hub.store.db.locked();
        for text in ["primeiro?", "e agora?"] {
            let comment = json!({
                "id": uuid::Uuid::new_v4().to_string(), "at": 1, "author": "owner",
                "text": text, "mentions": ["a"], "onlyNote": false,
                "notify": {"botIds": ["a"], "state": "sent"},
            });
            let cid = comment["id"].as_str().unwrap().to_owned();
            let poke = uuid::Uuid::new_v4().to_string();
            c.execute(
                "INSERT INTO donna_task_comments(hub_id,space_id,project_id,task_id,comment_id,request_id,payload,entry) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![f.hub.host_id, PERSONAL, project, "t1", cid, uuid::Uuid::new_v4().to_string(), "payload", serde_json::to_string(&comment).unwrap()],
            )
            .unwrap();
            c.execute(
                "UPDATE donna_task_deliveries SET status='sending',session_id=?3 WHERE hub_id=?1 AND comment_id=?2",
                params![f.hub.host_id, cid, session],
            )
            .unwrap();
            c.execute(
                "INSERT INTO entries(id,bot_id,rev,kind,turn,data,created_at,updated_at,session) VALUES(?1,?2,1,'user',?5,?3,0,0,?4)",
                params![poke, "a", serde_json::to_string(&json!({"clientNonce": format!("mx142-task-aviso-{cid}-a")})).unwrap(), session, i64::try_from(comments.len()).unwrap() + 7],
            )
            .unwrap();
            c.execute(
                "UPDATE donna_task_deliveries SET status='sent' WHERE hub_id=?1 AND comment_id=?2",
                params![f.hub.host_id, cid],
            )
            .unwrap();
            comments.push(cid);
            pokes.push(poke);
        }
        // O turno atual só admitiu o poke do SEGUNDO comentário.
        c.execute(
            "INSERT INTO kv(k,v) VALUES('turn.inflight.a',?1)",
            [serde_json::to_string(&json!({
                "admissionVersion": 1, "executionToken": exec.turn_token(), "attempt": "att",
                "phase": "permitted", "conversationId": "a", "origin": {"kind": "message"},
                "entryIds": [pokes[1].clone()],
            }))
            .unwrap()],
        )
        .unwrap();
    }
    // Reply alegando o PRIMEIRO comentário: mesma sessão, mas a entry dele
    // não foi admitida neste turno -> não marca.
    let out = f
        .call(
            "a",
            "task_comment",
            json!({"projectId": project, "taskId": "t1", "text": "resposta", "replyToCommentId": comments[0]}),
            Some(&exec),
        )
        .await
        .unwrap();
    assert_eq!(out["answered"], false);
    let status: String = f
        .hub
        .store
        .db
        .locked()
        .query_row(
            "SELECT status FROM donna_task_deliveries WHERE hub_id=?1 AND comment_id=?2",
            params![f.hub.host_id, comments[0]],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "sent");
    // Reply ao comentário cuja entry-poke ESTE turno consumiu -> answered.
    let out = f
        .call(
            "a",
            "task_comment",
            json!({"projectId": project, "taskId": "t1", "text": "resposta certa", "replyToCommentId": comments[1]}),
            Some(&exec),
        )
        .await
        .unwrap();
    assert_eq!(out["answered"], true);
}

#[tokio::test]
async fn create_child_request_id_idempotente() {
    let f = Fixture::new();
    let project = f.project(PERSONAL, None, &[task("t1", None, Some("a"), "Pai")]);
    let exec = f.exec("a");
    let request = uuid::Uuid::new_v4().to_string();
    let args = json!({
        "projectId": project, "parentTaskId": "t1", "title": "Filha",
        "description": "d", "acceptance": "a", "assigneeBotId": "b",
        "requestId": request,
    });
    let first = f.call("a", "task_create_child", args.clone(), Some(&exec)).await.unwrap();
    let task_id = first["task"]["id"].as_str().unwrap().to_owned();
    // Replay idêntico (resposta perdida / retry): mesma subtarefa, sem duplicar.
    let second = f.call("a", "task_create_child", args.clone(), Some(&exec)).await.unwrap();
    assert_eq!(second["task"]["id"].as_str().unwrap(), task_id);
    let children: i64 = {
        let c = f.hub.store.db.locked();
        c.query_row(
            "SELECT COUNT(*) FROM json_each((SELECT tasks FROM donna_projects WHERE hub_id=?1 AND space_id=?2 AND id=?3)) WHERE json_extract(value,'$.parentTaskId')='t1'",
            params![f.hub.host_id, PERSONAL, project],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(children, 1);
    // Mesmo requestId com payload divergente é conflito de idempotência.
    let mut divergent = args.clone();
    divergent["title"] = json!("Outra");
    let err = f.call("a", "task_create_child", divergent, Some(&exec)).await.unwrap_err();
    assert!(format!("{err:#}").contains("task_idempotency"), "{err:#}");
    // requestId inválido é recusado antes de gravar.
    let mut bad = args.clone();
    bad["requestId"] = json!("nao-e-uuid");
    bad["title"] = json!("Válida?");
    let err = f.call("a", "task_create_child", bad, Some(&exec)).await.unwrap_err();
    assert!(format!("{err:#}").contains("invalid_field") || format!("{err:#}").contains("uuid"), "{err:#}");
}

#[tokio::test]
async fn create_child_start_recusado_orienta_a_propor() {
    let f = Fixture::new();
    let project = f.project(PERSONAL, None, &[task("t1", None, Some("a"), "Pai")]);
    let exec = f.exec("a");
    // start=true é recusado antes de qualquer gravação — com ou sem requestId.
    for request in [Some(uuid::Uuid::new_v4().to_string()), None] {
        let mut args = json!({"projectId": project, "parentTaskId": "t1", "title": "Com início", "start": true});
        if let Some(id) = request {
            args["requestId"] = json!(id);
        }
        let err = f.call("a", "task_create_child", args, Some(&exec)).await.unwrap_err();
        assert!(format!("{err:#}").contains("proposal_required"), "{err:#}");
    }
    // Nada foi criado nem iniciado: só a tarefa original, sem agenda.
    let (_, tasks) = f.stored(&project);
    assert_eq!(tasks.len(), 1);
    assert_eq!(
        f.count(
            "SELECT COUNT(*) FROM donna_task_agenda WHERE hub_id=?1 AND space_id=?2 AND project_id=?3",
            Some(&project)
        ),
        0
    );
    assert_eq!(
        f.count(
            "SELECT COUNT(*) FROM donna_task_requests WHERE hub_id=?1 AND space_id=?2 AND project_id=?3",
            Some(&project)
        ),
        0
    );
}

#[tokio::test]
async fn task_read_comentarios_respeita_audiencia() {
    let f = Fixture::new();
    let project = f.project(PERSONAL, None, &[task("t1", None, Some("a"), "Sub")]);
    let exec = f.exec("a");
    let insert = |author: &str, mentions: Value, only_note: bool, text: &str| {
        let entry = json!({
            "id": uuid::Uuid::new_v4().to_string(), "at": 1, "author": author,
            "text": text, "mentions": mentions, "onlyNote": only_note,
            "mentionCheck": 2,
            "notify": {"botIds": [], "state": "not_notified"},
        });
        let c = f.hub.store.db.locked();
        c.execute(
            "INSERT INTO donna_task_comments(hub_id,space_id,project_id,task_id,comment_id,request_id,payload,entry) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                f.hub.host_id, PERSONAL, project, "t1", entry["id"].as_str().unwrap(),
                uuid::Uuid::new_v4().to_string(), "payload",
                serde_json::to_string(&entry).unwrap()
            ],
        )
        .unwrap();
        entry["id"].as_str().unwrap().to_owned()
    };
    let sem_mencao = insert("owner", json!([]), false, "sem @ para ninguém");
    let so_anotar = insert("owner", json!(["a"]), true, "@a só anotar");
    let mencao_b = insert("owner", json!(["b"]), false, "@b aviso só para b");
    let mencao_a = insert("owner", json!(["a"]), false, "@a aviso para a");
    let proprio = insert("bot:a", json!([]), false, "meu próprio comentário");
    // Legado sem `mentionCheck`: menção declarada não é prova — falha fechado.
    let legado = {
        let entry = json!({
            "id": uuid::Uuid::new_v4().to_string(), "at": 1, "author": "owner",
            "text": "@a nota antiga sem prova", "mentions": json!(["a"]), "onlyNote": false,
            "notify": {"botIds": [], "state": "not_notified"},
        });
        let c = f.hub.store.db.locked();
        c.execute(
            "INSERT INTO donna_task_comments(hub_id,space_id,project_id,task_id,comment_id,request_id,payload,entry) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                f.hub.host_id, PERSONAL, project, "t1", entry["id"].as_str().unwrap(),
                uuid::Uuid::new_v4().to_string(), "payload",
                serde_json::to_string(&entry).unwrap()
            ],
        )
        .unwrap();
        entry["id"].as_str().unwrap().to_owned()
    };
    let out = f.call("a", "task_read", json!({"projectId": project, "taskId": "t1"}), Some(&exec)).await.unwrap();
    let comments = out["task"]["comments"].as_array().unwrap();
    let ids: Vec<&str> = comments.iter().filter_map(|c| c["id"].as_str()).collect();
    // Visíveis: menção a 'a' sem só-anotar + o que o próprio bot escreveu.
    assert!(ids.contains(&mencao_a.as_str()), "{ids:?}");
    assert!(ids.contains(&proprio.as_str()), "{ids:?}");
    assert_eq!(comments.len(), 2, "{ids:?}");
    // Invisíveis: sem @, só-anotar mesmo mencionado, e menção a outro bot.
    for hidden in [&sem_mencao, &so_anotar, &mencao_b, &legado] {
        assert!(!ids.contains(&hidden.as_str()), "{hidden} vazou: {ids:?}");
    }
    // O texto oculto não vaza nem como campo/erro: nada além dos 2 ids acima.
    assert!(!serde_json::to_string(&out).unwrap().contains("sem @ para ninguém"));
    assert!(!serde_json::to_string(&out).unwrap().contains("só anotar para a"));
    // Bot sem autoridade plena na tarefa não recebe comentários (já coberto por
    // task_forbidden); menção não eleva acesso.
    let exec_b = f.exec("b");
    let err = f.call("b", "task_read", json!({"projectId": project, "taskId": "t1"}), Some(&exec_b)).await.unwrap_err();
    assert!(format!("{err:#}").contains("task_forbidden"), "{err:#}");
}

#[tokio::test]
async fn replay_create_child_revalida_audiencia_apos_reatribuicao() {
    let f = Fixture::new();
    // "a" supervisiona "b" (dona da mãe): acesso pleno à mãe. A filha nasce
    // atribuída a "a" e depois é reatribuída a "c" — no replay "a" não pode
    // mais receber descrição/evidência atuais (fail-closed).
    f.org("b", Some("a"));
    let project = f.project(PERSONAL, None, &[task("pai", None, Some("b"), "Mãe")]);
    let exec = f.exec("a");
    let request = uuid::Uuid::new_v4().to_string();
    let args = json!({
        "projectId": project, "parentTaskId": "pai", "title": "Filha",
        "description": "rascunho de a", "requestId": request, "assigneeBotId": "a",
    });
    let created = f.call("a", "task_create_child", args.clone(), Some(&exec)).await.unwrap();
    let child = created["task"]["id"].as_str().unwrap().to_owned();
    // Replay com acesso ainda válido: idempotente, devolve a mesma filha.
    let replay = f.call("a", "task_create_child", args.clone(), Some(&exec)).await.unwrap();
    assert_eq!(replay["task"]["id"].as_str().unwrap(), child);
    // Reatribuição direta no storage (caminho do dono/apps): "a" perde a filha.
    {
        let (_, mut tasks) = f.stored(&project);
        for t in &mut tasks {
            if t["id"].as_str() == Some(child.as_str()) {
                t["assigneeBotId"] = json!("c");
            }
        }
        f.hub
            .store
            .db
            .locked()
            .execute(
                "UPDATE donna_projects SET tasks=?4 WHERE hub_id=?1 AND space_id=?2 AND id=?3",
                params![f.hub.host_id, PERSONAL, project, serde_json::to_string(&tasks).unwrap()],
            )
            .unwrap();
    }
    // "c" reescreve a descrição — o replay de "a" não pode devolvê-la.
    let exec_c = f.exec("c");
    f.call(
        "c",
        "task_update",
        json!({"projectId": project, "taskId": child, "description": "vazado-por-c"}),
        Some(&exec_c),
    )
    .await
    .unwrap();
    let err = f.call("a", "task_create_child", args, Some(&exec)).await.unwrap_err();
    assert!(format!("{err:#}").contains("task_forbidden"), "{err:#}");
}

#[tokio::test]
async fn replay_create_child_degrada_para_summary_sem_vazar_campos() {
    let f = Fixture::new();
    // "a" é dona da mãe: na filha de "c" ela fica só com a visão de sumário
    // (mesma raiz tem tarefa dela). O replay preserva o id — idempotência —
    // mas não devolve descrição/evidência atuais.
    let project = f.project(PERSONAL, None, &[task("pai", None, Some("a"), "Mãe")]);
    let exec = f.exec("a");
    let request = uuid::Uuid::new_v4().to_string();
    let args = json!({
        "projectId": project, "parentTaskId": "pai", "title": "Filha",
        "description": "rascunho", "requestId": request, "assigneeBotId": "c",
    });
    let created = f.call("a", "task_create_child", args.clone(), Some(&exec)).await.unwrap();
    let child = created["task"]["id"].as_str().unwrap().to_owned();
    let exec_c = f.exec("c");
    f.call(
        "c",
        "task_update",
        json!({"projectId": project, "taskId": child, "description": "vazado-por-c"}),
        Some(&exec_c),
    )
    .await
    .unwrap();
    let replay = f.call("a", "task_create_child", args, Some(&exec)).await.unwrap();
    assert_eq!(replay["task"]["id"].as_str().unwrap(), child);
    assert!(replay["task"].get("description").is_none(), "{}", replay["task"]);
    assert!(replay["task"].get("evidence").is_none(), "{}", replay["task"]);
    assert!(!serde_json::to_string(&replay).unwrap().contains("vazado-por-c"));
}
