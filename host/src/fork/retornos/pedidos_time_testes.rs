#![allow(clippy::unwrap_used)]
//! `donnaTeamRequests` pela RPC real: só o dono, só o Space do bot, abertos e terminados nas últimas 24 h, o
//! texto do aviso de origem quando confere e nada de pedido com restrição de privacidade nem ID interno.
use super::{METHOD, TERMINADOS_MS};
use crate::LockExt;
use crate::api::{devices::Caller, dispatch};
use crate::fork::spaces::PERSONAL;
use crate::hub::Hub;
use crate::remote::identity::Identity;
use crate::store::{BotConfig, Device, DeviceSource, Scope, Store, now_ms};
use rusqlite::params;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;

const SENTINELA: &str = "SENTINELA-TIME-INTERNA";
const HORA: i64 = 60 * 60 * 1000;

struct Fixture {
    hub: Arc<Hub>,
    dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        std::fs::create_dir_all(crate::service::data_dir()).unwrap();
        let dir = std::env::temp_dir().join(format!("donna-time-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let hub = Hub::new(store, "time".into(), Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
        Self { hub, dir }
    }

    fn bot(&self, id: &str, space: &str) {
        let cfg: BotConfig =
            serde_json::from_value(json!({"id": id, "name": id, "spaceId": space, "backend": "fixture"})).unwrap();
        self.hub.store.save_bot(&cfg).unwrap();
    }

    async fn space(&self) -> String {
        let initial = self.hub.store.spaces().unwrap();
        let created = dispatch(
            &self.hub,
            &Caller::Local,
            "donnaSpaceCreate",
            json!({
                "hubId":self.hub.host_id,"expectedRevision":initial.revision,"name":"Trabalho",
                "tile":{"color":"oceano","style":"soft","symbol":{"kind":"letters","value":"TR"}}
            }),
        )
        .await
        .unwrap();
        created["spaces"][1]["id"].as_str().unwrap().into()
    }

    /// Pedido R1 gravado direto, com o aviso de origem; sentinelas nos campos internos que não podem sair.
    #[expect(clippy::too_many_arguments, reason = "fixture de uma linha do registro")]
    fn pedido(
        &self,
        de: &str,
        para: &str,
        estado: &str,
        motivo: Option<&str>,
        desfecho: Option<&str>,
        idade: i64,
        texto: &str,
    ) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let quando = now_ms() - idade;
        let aviso = format!("{id}-aviso");
        let db = self.hub.store.db.locked();
        db.execute(
            "INSERT INTO donna_session_requests(id,schema_version,kind,mode,requester_bot_id,requester_chat_id,
               requester_session_id,requester_execution_ref,target_bot_id,target_chat_id,target_session_id,chain_id,
               admission_id,origin_entry_id,target_entry_id,attempt,fence,state,end_reason,created_at,updated_at)
             VALUES(?1,1,'inter_bot','wait',?2,?4||'-chat',?4||'-sessao',?4||'-exec',?3,?4||'-chat-b',
               ?4||'-sessao-b',?4||'-cadeia',?4||'-adm',?8,?4||'-aviso-b',1,3,?5,?6,?7,?7)",
            params![id, de, para, SENTINELA, estado, motivo, quando, aviso],
        )
        .unwrap();
        let data = json!({
            "text": format!("Asked {para}: {texto}"), "style": "info", "status": "queued", "delegationId": id,
            "sourceBotId": de, "targetBotId": para,
            "botMessage": {"sourceBotId": de, "targetBotId": para, "text": texto},
        });
        db.execute(
            "INSERT INTO entries(id,bot_id,rev,kind,turn,data,created_at,updated_at) VALUES(?1,?2,1,'notice',1,?3,?4,?4)",
            params![aviso, de, data.to_string(), quando],
        )
        .unwrap();
        if let Some(desfecho) = desfecho {
            db.execute(
                "INSERT INTO donna_session_results(id,request_id,schema_version,attempt,fence,outcome,end_reason,
                   content,digest,created_at) VALUES(?1||'#1',?1,1,1,2,?2,'end_turn','none',?3,?4)",
                params![id, desfecho, format!("{SENTINELA:0<64}"), quando],
            )
            .unwrap();
        }
        id
    }

    async fn ler(&self, caller: &Caller, bot: &str, space: Option<&str>) -> anyhow::Result<Value> {
        let mut body = json!({"botId": bot});
        if let Some(space) = space {
            body["spaceId"] = space.into();
        }
        dispatch(&self.hub, caller, METHOD, body).await
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn linhas(v: &Value) -> Vec<(String, String, Value, Value)> {
    v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["targetBotId"].as_str().unwrap().to_owned(),
                i["state"].as_str().unwrap().to_owned(),
                i["outcome"].clone(),
                i["text"].clone(),
            )
        })
        .collect()
}

#[tokio::test]
async fn so_o_dono_le() {
    let f = Fixture::new();
    f.bot("berga", PERSONAL);
    f.bot("ana", PERSONAL);
    f.pedido("berga", "ana", "running", None, None, 0, "Revise o contrato");
    f.hub
        .store
        .put_device(&Device {
            key: "sem-controle".into(),
            name: "fixture".into(),
            platform: "fixture".into(),
            source: DeviceSource::Local,
            grant_id: None,
            scopes: vec![Scope::OwnerDevices],
            lease_until: None,
            created_at: now_ms(),
            last_seen_at: None,
        })
        .unwrap();
    let outro = Caller::Device { key: "sem-controle".into(), scopes: vec![Scope::OwnerDevices], autoridade: None };
    assert!(f.ler(&outro, "berga", None).await.is_err());
    let ponte = Caller::Ponte {
        bot: "ana".into(),
        sessao: "binding".into(),
        origem: crate::fork::ponte_credencial::Origem::ficticia(),
    };
    assert!(f.ler(&ponte, "berga", None).await.is_err(), "bot não lê o painel do dono");
    assert_eq!(linhas(&f.ler(&Caller::Local, "berga", None).await.unwrap()).len(), 1);
}

#[tokio::test]
async fn abertos_e_terminados_de_24h_com_texto_do_aviso() {
    let f = Fixture::new();
    for b in ["berga", "ana", "rui"] {
        f.bot(b, PERSONAL);
    }
    f.pedido("berga", "ana", "accepted", None, None, 3 * 24 * HORA, "Junte as notas");
    f.pedido("berga", "rui", "running", None, None, 2 * HORA, "Revise o contrato");
    f.pedido("berga", "ana", "finished", None, Some("completed"), HORA, "Mande o resumo");
    f.pedido("berga", "rui", "cancelled", Some("timeout"), None, 3 * HORA, "Ligue para o banco");
    f.pedido("berga", "rui", "finished", None, Some("failed"), TERMINADOS_MS + HORA, "Velho");
    f.pedido("ana", "berga", "running", None, None, 0, "Pedido de outro bot");

    let v = f.ler(&Caller::Local, "berga", None).await.unwrap();
    assert_eq!(v["botId"], "berga");
    assert_eq!(v["spaceId"], PERSONAL);
    assert_eq!(v["omitted"], 0);
    assert_eq!(
        linhas(&v),
        vec![
            ("ana".into(), "finished".into(), json!("completed"), json!("Mande o resumo")),
            ("rui".into(), "running".into(), Value::Null, json!("Revise o contrato")),
            ("rui".into(), "cancelled".into(), Value::Null, json!("Ligue para o banco")),
            ("ana".into(), "queued".into(), Value::Null, json!("Junte as notas")),
        ]
    );
    assert_eq!(v["items"][2]["reason"], "timeout");
    assert!(!v.to_string().contains(SENTINELA), "nenhum ID interno sai: {v}");
}

#[tokio::test]
async fn aviso_que_nao_confere_nao_da_texto() {
    let f = Fixture::new();
    f.bot("berga", PERSONAL);
    f.bot("ana", PERSONAL);
    let id = f.pedido("berga", "ana", "running", None, None, 0, "Texto certo");
    f.hub
        .store
        .db
        .locked()
        .execute(
            "UPDATE entries SET data=json_set(data,'$.botMessage.targetBotId','rui') WHERE id=?1",
            [format!("{id}-aviso")],
        )
        .unwrap();
    let v = f.ler(&Caller::Local, "berga", None).await.unwrap();
    assert_eq!(linhas(&v), vec![("ana".into(), "running".into(), Value::Null, Value::Null)]);
}

#[tokio::test]
async fn pedido_com_restricao_de_privacidade_fica_de_fora() {
    let f = Fixture::new();
    f.bot("berga", PERSONAL);
    f.bot("ana", PERSONAL);
    let privado = f.pedido("berga", "ana", "running", None, None, 0, "Privado");
    f.pedido("berga", "ana", "running", None, None, 0, "Comum");
    f.hub
        .store
        .db
        .locked()
        .execute(
            "INSERT INTO donna_session_privacy(request_id,privacy_class,body_disposition,audience_json,
               owner_principal_id,owner_epoch,policy_revision,hub_generation,delivery_state,revoked_at)
             VALUES(?1,'restricted_private','body_retained',?2,'dono',1,1,1,'retained',0)",
            params![
                privado,
                json!({"kind":"owner_only","space_id":PERSONAL,"owner_epoch":1,"owner_principal_id":"dono"})
                    .to_string()
            ],
        )
        .unwrap();
    let v = f.ler(&Caller::Local, "berga", None).await.unwrap();
    assert_eq!(linhas(&v), vec![("ana".into(), "running".into(), Value::Null, json!("Comum"))]);
    assert!(!v.to_string().contains("Privado"));
}

#[tokio::test]
async fn so_o_space_do_bot() {
    let f = Fixture::new();
    let work = f.space().await;
    f.bot("berga", PERSONAL);
    f.bot("ana", PERSONAL);
    f.bot("x", &work);
    f.pedido("berga", "ana", "running", None, None, 0, "Do Pessoal");
    f.pedido("berga", "x", "running", None, None, 0, "Para outro Space");
    assert!(f.ler(&Caller::Local, "berga", Some(&work)).await.is_err(), "bot de outro Space");
    assert!(f.ler(&Caller::Local, "x", None).await.is_err(), "Pessoal por padrão");
    assert!(f.ler(&Caller::Local, "sumiu", None).await.is_err());
    let v = f.ler(&Caller::Local, "berga", Some(PERSONAL)).await.unwrap();
    assert_eq!(linhas(&v), vec![("ana".into(), "running".into(), Value::Null, json!("Do Pessoal"))]);
    assert_eq!(linhas(&f.ler(&Caller::Local, "x", Some(&work)).await.unwrap()), vec![]);
}
