#![allow(clippy::unwrap_used)]
//! Testes do fluxo propor→decidir: a proposta pendente não cria nada; a
//! decisão do dono cria tarefas, histórico e agenda numa transação só.
use crate::{
    LockExt,
    api::dispatch,
    fork::{
        sessoes::{ExecutionRef, bindings},
        spaces::PERSONAL,
        tarefas::call_in_execution,
    },
    remote::identity::Identity,
    store::{BotConfig, Store},
};
use rusqlite::params;
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

struct Fixture {
    hub: Arc<crate::hub::Hub>,
    dir: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("fork-propostas-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let hub = crate::hub::Hub::new(
            Store::open(Path::new(":memory:")).unwrap(),
            "propostas-fixture".into(),
            Identity::load_or_create(&dir).unwrap(),
            "fixture".into(),
            0,
        );
        hub.store.db.locked().execute_batch("PRAGMA foreign_keys=ON").unwrap();
        for id in ["a", "b", "c"] {
            let cfg: BotConfig =
                serde_json::from_value(json!({"id":id,"name":id,"spaceId":PERSONAL,"backend":"fixture"})).unwrap();
            hub.store.save_bot(&cfg).unwrap();
        }
        Self { hub, dir }
    }

    fn exec(&self, bot: &str) -> ExecutionRef {
        bindings::fixture_execution(&self.hub, bot)
    }

    async fn call(&self, method: &str, body: Value) -> Result<Value, anyhow::Error> {
        dispatch(&self.hub, &crate::api::devices::Caller::Local, method, body).await
    }

    /// Projeto padrão do Space, criado pelo RPC do dono.
    async fn default_project(&self, space: &str) -> String {
        let out =
            self.call("donnaProjectEnsureDefault", json!({"hubId": self.hub.host_id, "spaceId": space})).await.unwrap();
        out["snapshot"]["project"]["id"].as_str().unwrap().to_owned()
    }

    /// Space extra para provar isolamento entre Spaces.
    async fn new_space(&self) -> String {
        let state = self.hub.store.spaces().unwrap();
        let out = self
            .call(
                "donnaSpaceCreate",
                json!({
                    "hubId": self.hub.host_id, "expectedRevision": state.revision, "name": "Oficina",
                    "tile": {"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"OF"}}
                }),
            )
            .await
            .unwrap();
        out["spaces"].as_array().unwrap().last().unwrap()["id"].as_str().unwrap().to_owned()
    }

    fn bot(&self, id: &str, space: &str) {
        let cfg: BotConfig =
            serde_json::from_value(json!({"id":id,"name":id,"spaceId":space,"backend":"fixture"})).unwrap();
        self.hub.store.save_bot(&cfg).unwrap();
    }

    async fn propose(&self, bot: &str, execution: Option<&ExecutionRef>, args: Value) -> Result<Value, anyhow::Error> {
        call_in_execution(&self.hub, bot, "task_propose", &args, execution).await
    }

    async fn decide(
        &self,
        proposal: &str,
        version: i64,
        key: &str,
        decision: &str,
        omit: Option<Value>,
    ) -> Result<Value, anyhow::Error> {
        let mut body = json!({
            "hubId": self.hub.host_id, "spaceId": PERSONAL, "proposalId": proposal,
            "expectedVersion": version, "clientRequestId": key, "decision": decision,
        });
        if let Some(omit) = omit {
            body["omitItemIds"] = omit;
        }
        self.call("donnaTaskProposalDecide", body).await
    }

    async fn proposals(&self, space: &str) -> Value {
        self.call("donnaTaskProposals", json!({"hubId": self.hub.host_id, "spaceId": space})).await.unwrap()
    }

    fn tasks_of(&self, project: &str) -> Vec<Value> {
        let c = self.hub.store.db.locked();
        let raw: String = c
            .query_row(
                "SELECT tasks FROM donna_projects WHERE hub_id=?1 AND space_id=?2 AND id=?3",
                params![self.hub.host_id, PERSONAL, project],
                |r| r.get(0),
            )
            .unwrap();
        serde_json::from_str(&raw).unwrap()
    }

    /// Linhas da agenda do Projeto: (task, bot, state, `proposal_id`).
    fn agenda_of(&self, project: &str) -> Vec<(String, String, String, Option<String>)> {
        let c = self.hub.store.db.locked();
        let mut st = c
            .prepare(
                "SELECT task_id,bot_id,state,proposal_id FROM donna_task_agenda
                 WHERE hub_id=?1 AND space_id=?2 AND project_id=?3",
            )
            .unwrap();
        st.query_map(params![self.hub.host_id, PERSONAL, project], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    fn history_count(&self, project: &str) -> i64 {
        let c = self.hub.store.db.locked();
        c.query_row(
            "SELECT COUNT(*) FROM donna_project_history WHERE hub_id=?1 AND space_id=?2 AND project_id=?3",
            params![self.hub.host_id, PERSONAL, project],
            |r| r.get(0),
        )
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).unwrap();
    }
}

fn items_2() -> Value {
    json!([
        {"title": "Levantar dados fictícios", "description": "Coletar números de exemplo", "acceptance": "Planilha pronta", "assigneeBotId": "b"},
        {"title": "Resumir em nota", "description": "", "acceptance": ""},
    ])
}

fn propose_args(request: &str, items: &Value) -> Value {
    json!({"title": "Lote fictício", "requestId": request, "items": items})
}

#[tokio::test]
async fn propor_registra_pendente_e_nao_cria_nada() {
    let f = Fixture::new();
    let project = f.default_project(PERSONAL).await;
    let exec = f.exec("a");
    let request = uuid::Uuid::new_v4().to_string();
    let args = propose_args(&request, &items_2());
    let out = f.propose("a", Some(&exec), args.clone()).await.unwrap();
    let message = out["message"].as_str().unwrap();
    assert!(message.contains("ok do dono"), "{message}");
    assert!(message.contains("Nada foi criado nem iniciado"), "{message}");
    assert_eq!(out["proposal"]["state"], "pending");
    assert_eq!(out["proposal"]["version"], 1);
    assert_eq!(out["proposal"]["projectId"], project.as_str());
    let proposal_id = out["proposal"]["id"].as_str().unwrap().to_owned();
    assert!(out["proposal"]["items"][0]["taskId"].is_null());
    // Propor não cria tarefa, agenda nem histórico.
    let baseline = f.history_count(&project);
    assert_eq!(f.tasks_of(&project).len(), 0);
    assert_eq!(f.agenda_of(&project).len(), 0);
    assert_eq!(f.history_count(&project), baseline);
    // Replay do mesmo pedido devolve a MESMA proposta, sem duplicar.
    let again = f.propose("a", Some(&exec), args.clone()).await.unwrap();
    assert_eq!(again["proposal"]["id"], proposal_id.as_str());
    let list = f.proposals(PERSONAL).await;
    assert_eq!(list["proposals"].as_array().unwrap().len(), 1);
    // Mesmo requestId com outro conteúdo é conflito.
    let mut divergent = args.clone();
    divergent["title"] = json!("Outro lote");
    let err = f.propose("a", Some(&exec), divergent).await.unwrap_err();
    assert!(format!("{err:#}").contains("idempotency_conflict"), "{err:#}");
}

#[tokio::test]
async fn propor_sem_projeto_padrao_recusa_e_campos_sao_validados() {
    let f = Fixture::new();
    let exec = f.exec("a");
    // Sem Projeto padrão e sem projectId: recusa orientando.
    let err =
        f.propose("a", Some(&exec), propose_args(&uuid::Uuid::new_v4().to_string(), &items_2())).await.unwrap_err();
    assert!(format!("{err:#}").contains("proposal_invalid"), "{err:#}");
    // Responsável fora do roster é recusado antes de gravar.
    f.default_project(PERSONAL).await;
    let mut args = propose_args(&uuid::Uuid::new_v4().to_string(), &items_2());
    args["items"][0]["assigneeBotId"] = json!("nao-existe");
    let err = f.propose("a", Some(&exec), args).await.unwrap_err();
    assert!(format!("{err:#}").contains("invalid_field"), "{err:#}");
    // Teto de 20 pendentes por Space.
    for _ in 0..20 {
        f.propose("a", Some(&exec), propose_args(&uuid::Uuid::new_v4().to_string(), &items_2())).await.unwrap();
    }
    let err =
        f.propose("a", Some(&exec), propose_args(&uuid::Uuid::new_v4().to_string(), &items_2())).await.unwrap_err();
    assert!(format!("{err:#}").contains("proposal_limit"), "{err:#}");
}

#[tokio::test]
async fn bot_sem_execucao_viva_nao_propoe() {
    let f = Fixture::new();
    let err = f.propose("a", None, propose_args(&uuid::Uuid::new_v4().to_string(), &items_2())).await.unwrap_err();
    assert!(format!("{err:#}").contains("execução ausente"), "{err:#}");
    let exec = f.exec("a");
    bindings::revoke(&f.hub.store, "a", None).unwrap();
    let err =
        f.propose("a", Some(&exec), propose_args(&uuid::Uuid::new_v4().to_string(), &items_2())).await.unwrap_err();
    assert!(format!("{err:#}").contains("chamada vencida"), "{err:#}");
    let list = f.proposals(PERSONAL).await;
    assert_eq!(list["proposals"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn aprovar_cria_tarefas_raiz_historico_e_agenda() {
    let f = Fixture::new();
    let project = f.default_project(PERSONAL).await;
    let baseline = f.history_count(&project);
    let exec = f.exec("a");
    let out = f.propose("a", Some(&exec), propose_args(&uuid::Uuid::new_v4().to_string(), &items_2())).await.unwrap();
    let proposal = out["proposal"]["id"].as_str().unwrap().to_owned();
    let key = uuid::Uuid::new_v4().to_string();
    let decided = f.decide(&proposal, 1, &key, "approve", None).await.unwrap();
    assert_eq!(decided["proposal"]["state"], "approved");
    assert_eq!(decided["proposal"]["version"], 2);
    // Tarefas raiz em backlog, com os campos da proposta.
    let tasks = f.tasks_of(&project);
    assert_eq!(tasks.len(), 2);
    for task in &tasks {
        assert_eq!(task["parentTaskId"], Value::Null);
        assert_eq!(task["state"], "backlog");
        assert_eq!(task["evidence"], "");
    }
    let by_title = |title: &str| tasks.iter().find(|t| t["title"] == title).cloned().unwrap();
    let with_assignee = by_title("Levantar dados fictícios");
    let unassigned = by_title("Resumir em nota");
    assert_eq!(with_assignee["assigneeBotId"], "b");
    assert_eq!(unassigned["assigneeBotId"], Value::Null);
    // Só a tarefa com responsável ganha intenção de início, ligada à proposta.
    let agenda = f.agenda_of(&project);
    assert_eq!(agenda.len(), 1);
    assert_eq!(agenda[0].1, "b");
    assert_eq!(agenda[0].2, "scheduled");
    assert_eq!(agenda[0].3.as_deref(), Some(proposal.as_str()));
    assert_eq!(agenda[0].0, with_assignee["id"].as_str().unwrap());
    // Histórico do Projeto no mesmo formato das criações manuais (além do que
    // já existia antes da aprovação).
    assert_eq!(f.history_count(&project), baseline + 2);
    // A visão devolve estado real: tarefa no Projeto e início na agenda.
    let list = f.proposals(PERSONAL).await;
    let view = &list["proposals"][0];
    assert_eq!(view["items"][0]["taskState"], "backlog");
    assert_eq!(view["items"][0]["startState"], "scheduled");
    assert_eq!(view["items"][1]["taskState"], "backlog");
    assert_eq!(view["items"][1]["startState"], Value::Null);
}

#[tokio::test]
async fn aprovar_invalido_e_atomico_nao_deixa_meio_estado() {
    let f = Fixture::new();
    let project = f.default_project(PERSONAL).await;
    let exec = f.exec("a");
    let out = f.propose("a", Some(&exec), propose_args(&uuid::Uuid::new_v4().to_string(), &items_2())).await.unwrap();
    let proposal = out["proposal"]["id"].as_str().unwrap().to_owned();
    let item_title = out["proposal"]["items"][0]["title"].as_str().unwrap().to_owned();
    // Responsável saiu do roster do Space: recusa nomeando o item.
    f.hub.store.db.locked().execute("UPDATE bots SET deleted=1 WHERE id='b'", []).unwrap();
    let err = f.decide(&proposal, 1, &uuid::Uuid::new_v4().to_string(), "approve", None).await.unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("proposal_invalid"), "{msg}");
    assert!(msg.contains(&item_title), "{msg}");
    // Nada foi gravado: sem tarefas, sem agenda, proposta segue pendente.
    assert_eq!(f.tasks_of(&project).len(), 0);
    assert_eq!(f.agenda_of(&project).len(), 0);
    let list = f.proposals(PERSONAL).await;
    assert_eq!(list["proposals"][0]["state"], "pending");
    assert_eq!(list["proposals"][0]["version"], 1);
    // Item a omitir inexistente: recusa sem gravar nada.
    let ghost = uuid::Uuid::new_v4().to_string();
    let err =
        f.decide(&proposal, 1, &uuid::Uuid::new_v4().to_string(), "approve", Some(json!([ghost]))).await.unwrap_err();
    assert!(format!("{err:#}").contains("proposal_invalid"), "{err:#}");
    assert_eq!(f.tasks_of(&project).len(), 0);
}

#[tokio::test]
async fn aprovar_duas_vezes_nunca_cria_dois_conjuntos() {
    let f = Fixture::new();
    let project = f.default_project(PERSONAL).await;
    let exec = f.exec("a");
    let out = f.propose("a", Some(&exec), propose_args(&uuid::Uuid::new_v4().to_string(), &items_2())).await.unwrap();
    let proposal = out["proposal"]["id"].as_str().unwrap().to_owned();
    let key = uuid::Uuid::new_v4().to_string();
    f.decide(&proposal, 1, &key, "approve", None).await.unwrap();
    // Mesma chave repetida: devolve a visão atual sem refazer.
    let again = f.decide(&proposal, 1, &key, "approve", None).await.unwrap();
    assert_eq!(again["proposal"]["state"], "approved");
    assert_eq!(f.tasks_of(&project).len(), 2);
    assert_eq!(f.agenda_of(&project).len(), 1);
    // Outra chave em proposta já decidida: recusa, sem refazer.
    let err = f.decide(&proposal, 2, &uuid::Uuid::new_v4().to_string(), "approve", None).await.unwrap_err();
    assert!(format!("{err:#}").contains("proposal_decided"), "{err:#}");
    assert_eq!(f.tasks_of(&project).len(), 2);
    assert_eq!(f.agenda_of(&project).len(), 1);
}

#[tokio::test]
async fn lista_nunca_esconde_pendente_atras_do_historico_decidido() {
    let f = Fixture::new();
    f.default_project(PERSONAL).await;
    let exec = f.exec("a");
    let first = f.propose("a", Some(&exec), propose_args(&uuid::Uuid::new_v4().to_string(), &items_2())).await.unwrap();
    let pending = first["proposal"]["id"].as_str().unwrap().to_owned();
    for _ in 0..52 {
        let out =
            f.propose("a", Some(&exec), propose_args(&uuid::Uuid::new_v4().to_string(), &items_2())).await.unwrap();
        let id = out["proposal"]["id"].as_str().unwrap().to_owned();
        f.decide(&id, 1, &uuid::Uuid::new_v4().to_string(), "reject", None).await.unwrap();
    }
    let list = f.proposals(PERSONAL).await;
    let all = list["proposals"].as_array().unwrap();
    assert!(all.iter().any(|p| p["id"] == pending.as_str() && p["state"] == "pending"));
    assert_eq!(all.len(), 51);
}

#[tokio::test]
async fn recusar_deixa_um_desfecho_so() {
    let f = Fixture::new();
    let project = f.default_project(PERSONAL).await;
    let exec = f.exec("a");
    let out = f.propose("a", Some(&exec), propose_args(&uuid::Uuid::new_v4().to_string(), &items_2())).await.unwrap();
    let proposal = out["proposal"]["id"].as_str().unwrap().to_owned();
    let key = uuid::Uuid::new_v4().to_string();
    let rejected = f.decide(&proposal, 1, &key, "reject", None).await.unwrap();
    assert_eq!(rejected["proposal"]["state"], "rejected");
    assert_eq!(f.tasks_of(&project).len(), 0);
    // Aprovar depois de recusar, com a mesma chave ou outra, nunca cria.
    let replay = f.decide(&proposal, 1, &key, "approve", None).await.unwrap();
    assert_eq!(replay["proposal"]["state"], "rejected");
    let err = f.decide(&proposal, 2, &uuid::Uuid::new_v4().to_string(), "approve", None).await.unwrap_err();
    assert!(format!("{err:#}").contains("proposal_decided"), "{err:#}");
    assert_eq!(f.tasks_of(&project).len(), 0);
    assert_eq!(f.agenda_of(&project).len(), 0);
}

#[tokio::test]
async fn versao_velha_recusa_sem_gravar() {
    let f = Fixture::new();
    let project = f.default_project(PERSONAL).await;
    let exec = f.exec("a");
    let out = f.propose("a", Some(&exec), propose_args(&uuid::Uuid::new_v4().to_string(), &items_2())).await.unwrap();
    let proposal = out["proposal"]["id"].as_str().unwrap().to_owned();
    let err = f.decide(&proposal, 0, &uuid::Uuid::new_v4().to_string(), "approve", None).await.unwrap_err();
    assert!(format!("{err:#}").contains("proposal_conflict"), "{err:#}");
    assert_eq!(f.tasks_of(&project).len(), 0);
    let decided = f.decide(&proposal, 1, &uuid::Uuid::new_v4().to_string(), "approve", None).await.unwrap();
    assert_eq!(decided["proposal"]["state"], "approved");
    assert_eq!(f.tasks_of(&project).len(), 2);
}

#[tokio::test]
async fn omit_item_ids_aprova_soh_o_subconjunto() {
    let f = Fixture::new();
    let project = f.default_project(PERSONAL).await;
    let exec = f.exec("a");
    let out = f
        .propose(
            "a",
            Some(&exec),
            json!({
                "title": "Lote fictício", "requestId": uuid::Uuid::new_v4().to_string(),
                "items": [
                    {"title": "Primeira", "assigneeBotId": "b"},
                    {"title": "Segunda", "assigneeBotId": "b"},
                    {"title": "Terceira"},
                ]
            }),
        )
        .await
        .unwrap();
    let proposal = out["proposal"]["id"].as_str().unwrap().to_owned();
    let omitted_id = out["proposal"]["items"][2]["id"].as_str().unwrap().to_owned();
    // Omitir TUDO não é aprovar tarefa nenhuma: recusa.
    let all: Vec<String> = out["proposal"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["id"].as_str().map(str::to_owned))
        .collect();
    let err = f.decide(&proposal, 1, &uuid::Uuid::new_v4().to_string(), "approve", Some(json!(all))).await.unwrap_err();
    assert!(format!("{err:#}").contains("proposal_invalid"), "{err:#}");
    assert_eq!(f.tasks_of(&project).len(), 0);
    // Aprova sem o item omitido: ficam as duas primeiras, ambas com responsável.
    let decided =
        f.decide(&proposal, 1, &uuid::Uuid::new_v4().to_string(), "approve", Some(json!([omitted_id]))).await.unwrap();
    assert_eq!(decided["proposal"]["state"], "approved");
    let tasks = f.tasks_of(&project);
    assert_eq!(tasks.len(), 2);
    assert_eq!(f.agenda_of(&project).len(), 2);
    let list = f.proposals(PERSONAL).await;
    let view = &list["proposals"][0];
    let omitted = view["items"].as_array().unwrap().iter().find(|i| i["id"] == omitted_id.as_str()).unwrap();
    assert_eq!(omitted["included"], false);
    assert!(omitted["taskId"].is_null());
    assert!(omitted["taskState"].is_null());
}

#[tokio::test]
async fn proposta_de_outro_space_e_invisivel() {
    let f = Fixture::new();
    let space2 = f.new_space().await;
    f.bot("z", &space2);
    let project2 = f.default_project(&space2).await;
    let exec_z = f.exec("z");
    let out = f
        .propose(
            "z",
            Some(&exec_z),
            json!({
                "projectId": project2, "title": "Lote da oficina", "requestId": uuid::Uuid::new_v4().to_string(),
                "items": [{"title": "Só da oficina", "assigneeBotId": "z"}]
            }),
        )
        .await
        .unwrap();
    let proposal = out["proposal"]["id"].as_str().unwrap().to_owned();
    // No Space pessoal a proposta do outro Space não aparece.
    let list = f.proposals(PERSONAL).await;
    assert_eq!(list["proposals"].as_array().unwrap().len(), 0);
    let list2 = f.proposals(&space2).await;
    assert_eq!(list2["proposals"].as_array().unwrap().len(), 1);
    // Decidir do Space pessoal com id do outro Space não acha nada.
    let err = f.decide(&proposal, 1, &uuid::Uuid::new_v4().to_string(), "approve", None).await.unwrap_err();
    assert!(format!("{err:#}").contains("proposal_not_found"), "{err:#}");
    // Bot do Space pessoal propondo com Projeto do outro Space: recusado.
    let project1 = f.default_project(PERSONAL).await;
    let exec_a = f.exec("a");
    let err = f
        .propose(
            "a",
            Some(&exec_a),
            json!({
                "projectId": project2, "title": "Invasão", "requestId": uuid::Uuid::new_v4().to_string(),
                "items": [{"title": "De fora"}]
            }),
        )
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("project_not_found"), "{err:#}");
    assert_eq!(f.tasks_of(&project1).len(), 0);
}
