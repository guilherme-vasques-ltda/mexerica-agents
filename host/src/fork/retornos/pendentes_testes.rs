#![allow(clippy::unwrap_used)]
//! D70 da seção "Pedidos entre bots" do Mission Board pela RPC real `donnaControlTower`:
//! só o dono, outro space sem leitura, roster dos dois lados, estados, limite, janela,
//! nenhum texto/ID interno e a transição terminal pelo percurso real do `ask_bot`.
use super::pendentes::{JANELA_MS, LIMITE};
use crate::LockExt;
use crate::api::{devices::Caller, dispatch};
use crate::fork::sessoes::bindings;
use crate::fork::spaces::PERSONAL;
use crate::hub::Hub;
use crate::remote::identity::Identity;
use crate::store::{BotConfig, Device, DeviceSource, Scope, Store, now_ms};
use rusqlite::params;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const SENTINELA: &str = "SENTINELA-MB2-PRIVADA";

struct Fixture {
    hub: Arc<Hub>,
    dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        std::fs::create_dir_all(crate::service::data_dir()).unwrap();
        let dir = std::env::temp_dir().join(format!("donna-mb2-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let hub = Hub::new(store, "mb2".into(), Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
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

    /// Pedido R1 gravado direto, com sentinelas em todo campo que não pode sair.
    fn pedido(&self, de: &str, para: &str, estado: &str, motivo: Option<&str>, desfecho: Option<&str>, idade: i64) {
        let id = uuid::Uuid::new_v4().to_string();
        let quando = now_ms() - idade;
        let db = self.hub.store.db.locked();
        db.execute(
            "INSERT INTO donna_session_requests(id,schema_version,kind,mode,requester_bot_id,requester_chat_id,
               requester_session_id,requester_execution_ref,target_bot_id,target_chat_id,target_session_id,chain_id,
               admission_id,origin_entry_id,target_entry_id,attempt,fence,state,end_reason,created_at,updated_at)
             VALUES(?1,1,'inter_bot','wait',?2,?4||'-chat',?4||'-sessao',?4||'-exec',?3,?4||'-chat-b',
               ?4||'-sessao-b',?4||'-cadeia',?4||'-adm',?4||'-aviso',?4||'-aviso-b',1,3,?5,?6,?7,?7)",
            params![id, de, para, SENTINELA, estado, motivo, quando],
        )
        .unwrap();
        if let Some(desfecho) = desfecho {
            db.execute(
                "INSERT INTO donna_session_results(id,request_id,schema_version,attempt,fence,outcome,end_reason,
                   content,digest,created_at) VALUES(?1||'#1',?1,1,1,2,?2,'engine_error','none',?3,?4)",
                params![id, desfecho, format!("{SENTINELA:0<64}"), quando],
            )
            .unwrap();
        }
    }

    async fn pedidos(&self, caller: &Caller, space: &str) -> anyhow::Result<Value> {
        let snapshot = dispatch(
            &self.hub,
            caller,
            crate::fork::control_tower::METHOD,
            json!({"hubId": self.hub.host_id, "spaceId": space}),
        )
        .await?;
        Ok(snapshot["interBotRequests"].clone())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn linhas(pedidos: &Value) -> Vec<(String, String, String, Value, Value)> {
    pedidos["value"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            let s = |k: &str| i[k].as_str().unwrap().to_owned();
            (s("requesterBotId"), s("targetBotId"), s("state"), i["reason"].clone(), i["outcome"].clone())
        })
        .collect()
}

#[tokio::test]
async fn so_dono_e_outro_space_nao_le_o_registro() {
    let f = Fixture::new();
    let work = f.space().await;
    f.bot("a", PERSONAL);
    f.bot("b", PERSONAL);
    f.bot("x", &work);
    f.bot("y", &work);
    f.pedido("a", "b", "cancelled", Some("timeout"), None, 0);
    f.pedido("x", "y", "cancelled", Some("timeout"), None, 0);

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
    assert!(f.pedidos(&outro, PERSONAL).await.is_err(), "só o dono pela access::require existente");

    let pessoal = f.pedidos(&Caller::Local, PERSONAL).await.unwrap();
    assert_eq!(pessoal["source"], "retornos.pendentes");
    assert_eq!(linhas(&pessoal).len(), 1);
    // Outro space: indisponível, nem vazio nem com os bots de lá.
    assert_eq!(f.pedidos(&Caller::Local, &work).await.unwrap(), json!({"availability":"unavailable"}));
}

#[tokio::test]
async fn roster_dos_dois_lados_estados_sem_sucesso_janela_e_sem_texto() {
    let f = Fixture::new();
    let work = f.space().await;
    f.bot("a", PERSONAL);
    f.bot("b", PERSONAL);
    f.bot("fora", &work);
    let hora = 60 * 60 * 1000;
    // Fora: abertos, concluído, bot fora do roster (nos dois sentidos) e mais de 7 dias.
    f.pedido("a", "b", "accepted", None, None, 1);
    f.pedido("a", "b", "running", None, None, 1);
    f.pedido("a", "b", "finished", None, Some("completed"), 1);
    f.pedido("a", "fora", "cancelled", Some("timeout"), None, 1);
    f.pedido("fora", "a", "cancelled", Some("timeout"), None, 1);
    f.pedido("a", "removido", "cancelled", Some("cancelled"), None, 1);
    f.pedido("a", "b", "cancelled", Some("timeout"), None, JANELA_MS + hora);
    // Dentro, do mais antigo para o mais novo.
    f.pedido("a", "b", "finished", None, Some("failed"), 10 * hora);
    f.pedido("b", "a", "finished", None, Some("stopped"), 9 * hora);
    f.pedido("a", "b", "cancelled", Some("cancelled"), None, 8 * hora);
    f.pedido("a", "b", "interrupted", Some("restart_uncertain"), None, 7 * hora);
    f.pedido("a", "b", "cancelled", Some("timeout"), None, JANELA_MS - hora);

    let pedidos = f.pedidos(&Caller::Local, PERSONAL).await.unwrap();
    let null = Value::Null;
    let s = |v: &str| v.to_owned();
    assert_eq!(
        linhas(&pedidos),
        [
            (s("a"), s("b"), s("interrupted"), json!("restart_uncertain"), null.clone()),
            (s("a"), s("b"), s("cancelled"), json!("cancelled"), null.clone()),
            (s("b"), s("a"), s("finished"), null.clone(), json!("stopped")),
            (s("a"), s("b"), s("finished"), null.clone(), json!("failed")),
            (s("a"), s("b"), s("cancelled"), json!("timeout"), null.clone()),
        ]
    );
    assert_eq!(pedidos["value"]["omitted"], 0);
    let item = &pedidos["value"]["items"][0];
    let mut chaves: Vec<_> = item.as_object().unwrap().keys().cloned().collect();
    chaves.sort();
    assert_eq!(chaves, ["endedAt", "key", "mode", "outcome", "reason", "requesterBotId", "state", "targetBotId"]);
    assert_eq!(item["mode"], "wait");
    let wire = pedidos.to_string();
    assert!(!wire.contains(SENTINELA) && !wire.contains("fora") && !wire.contains("removido"), "{wire}");
}

#[tokio::test]
async fn limite_de_vinte_com_omitidos_do_mais_novo_ao_mais_antigo() {
    let f = Fixture::new();
    f.bot("a", PERSONAL);
    f.bot("b", PERSONAL);
    for idade in 0..25 {
        f.pedido("a", "b", "cancelled", Some("recipient_stopped"), None, 1000 * (idade + 1));
    }
    let pedidos = f.pedidos(&Caller::Local, PERSONAL).await.unwrap();
    let items = pedidos["value"]["items"].as_array().unwrap();
    assert_eq!((items.len(), pedidos["value"]["omitted"].as_u64()), (LIMITE, Some(5)));
    assert!(items.windows(2).all(|w| w[0]["endedAt"].as_i64() >= w[1]["endedAt"].as_i64()));
}

/// Percurso real: o `ask_bot` estoura o prazo, o R1 grava `timeout` e o aviso final chega como
/// `entry` que passa pelo filtro do stream do Pessoal (evento em `REREAD_EVENTS` do app).
#[tokio::test]
async fn transicao_terminal_emite_entry_no_pessoal_e_aparece_no_mission_board() {
    std::fs::create_dir_all(crate::service::data_dir()).unwrap();
    let dir = std::env::temp_dir().join(format!("donna-mb2-real-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir.join("test.db")).unwrap();
    let agent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/team_agent.py");
    for id in ["a", "b"] {
        let cwd = dir.join(id);
        std::fs::create_dir_all(&cwd).unwrap();
        let cfg: BotConfig = serde_json::from_value(json!({
            "id": id, "name": id, "backend": "fixture", "cwd": cwd,
            "command": crate::shell::python(&agent), "notify": false, "permission": "ask",
        }))
        .unwrap();
        store.save_bot(&cfg).unwrap();
    }
    let hub = Hub::new(store, "test-host".into(), Identity::load_or_create(&dir).unwrap(), "test-token".into(), 19223);
    hub.start().unwrap();
    bindings::fixture_execution(&hub, "a");
    hub.team.start_turn("a", None);
    let principal = bindings::for_acp(&hub.store, "a", "fixture-acp").unwrap();

    let mut eventos = hub.events.subscribe();
    let execucao = bindings::capture(&hub, "a", principal.id.as_str()).unwrap();
    let args = json!({"botId": "b", "message": format!("BLOCK {SENTINELA} prazo")});
    let prazo = crate::chat::team::ask_in_execution(&hub, "a", &args, &execucao, Duration::from_millis(200));
    assert!(prazo.await.is_err());

    let avisou = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let Ok(evento) = eventos.recv().await else { continue };
            if evento["type"] == "entry"
                && evento["entry"]["botId"] == "a"
                && crate::fork::spaces::filter_event(&hub, PERSONAL, evento).is_some()
            {
                break;
            }
        }
    })
    .await;
    assert!(avisou.is_ok(), "transição terminal chega ao stream do Pessoal como entry");
    let snapshot = dispatch(
        &hub,
        &Caller::Local,
        crate::fork::control_tower::METHOD,
        json!({"hubId": hub.host_id, "spaceId": PERSONAL}),
    )
    .await
    .unwrap();
    let pedidos = &snapshot["interBotRequests"];
    assert_eq!(
        linhas(pedidos),
        [("a".into(), "b".into(), "cancelled".into(), json!("timeout"), Value::Null)],
        "{pedidos}"
    );
    assert!(!pedidos.to_string().contains(SENTINELA));
    hub.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}
