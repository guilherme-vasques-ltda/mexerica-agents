#![allow(clippy::unwrap_used)]
//! D320: o bot vê só a própria cadeia, sem dados pessoais, ocultos ou de outro Space.
use super::{cadeia::FERRAMENTA, model::Change, store};
use crate::{
    LockExt,
    chat::team::call,
    fork::spaces::PERSONAL,
    hub::Hub,
    remote::identity::Identity,
    store::{BotConfig, Store},
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

fn bot(store: &Store, id: &str, name: &str, space: &str, hidden: bool) {
    let cfg: BotConfig = serde_json::from_value(json!({
        "id":id,"name":name,"backend":"fixture","spaceId":space,"hidden":hidden,
        "cwd":"/pasta-privada","description":"prompt-privado","model":"modelo-privado"
    }))
    .unwrap();
    store.save_bot(&cfg).unwrap();
}

fn member(id: &str, role: &str, team: Option<&str>, up: Option<&str>) -> Value {
    json!({
        "botId":id,"teamId":team,"parentBotId":null,"role":role,
        "reportsTo":up.map_or(json!({"kind":"owner"}), |b| json!({"kind":"bot","botId":b})),
        "ownerContact":"reports"
    })
}

/// Donna principal; Ana (gerente, Você) → Angela (gerente) → Marisa; Sombra oculta sob Ana;
/// Zeca sob a Sombra; Nova sem registro; Fora em outro Space.
fn fixture() -> (Arc<Hub>, std::path::PathBuf, String) {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let mut spaces = store.spaces().unwrap();
    let mut work = spaces.spaces[0].clone();
    work.id = uuid::Uuid::new_v4().to_string();
    work.kind = "space".into();
    work.name = "Trabalho".into();
    spaces.spaces.push(work.clone());
    store.kv_set("donna.spaces.v1", &serde_json::to_string(&spaces).unwrap()).unwrap();
    for (id, name, hidden) in [
        ("donna", "Donna", false),
        ("ana", "Ana", false),
        ("angela", "Angela", false),
        ("marisa", "Marisa", false),
        ("sombra", "Sombra", true),
        ("zeca", "Zeca", false),
    ] {
        bot(&store, id, name, PERSONAL, hidden);
    }
    bot(&store, "fora", "Fora", &work.id, false);
    store.set_team_settings(&json!({"primaryBotId":"donna"})).unwrap();
    let snapshot = serde_json::to_value(store::snapshot(&store.db.locked(), PERSONAL).unwrap()).unwrap();
    let area = uuid::Uuid::new_v4().to_string();
    let vendas = Some(area.as_str());
    let request = json!({
        "spaceId":PERSONAL,"expectedRevision":snapshot["revision"],"catalogVersion":snapshot["catalogVersion"],
        "nodes":[{"id":area,"name":"Vendas","parentId":null,"managerBotId":"ana","position":0}],
        "members":[
            member("donna","manager",None,None),
            member("ana","manager",vendas,None),
            member("angela","manager",vendas,Some("ana")),
            member("marisa","executor",vendas,Some("angela")),
            member("sombra","manager",None,Some("ana")),
            member("zeca","executor",None,Some("sombra")),
        ],
        "routes":[{"fromBotId":"marisa","toBotId":"ana","state":"proposed"}],
        "clientRequestId":uuid::Uuid::new_v4().to_string()
    });
    {
        let mut db = store.db.locked();
        let tx = db.transaction().unwrap();
        store::change(&tx, &serde_json::from_value::<Change>(request).unwrap(), true).unwrap();
        tx.commit().unwrap();
    }
    // Sem registro no quadro: aparece reportando ao dono, como no app.
    bot(&store, "nova", "Nova", PERSONAL, false);
    let dir = std::env::temp_dir().join(format!("donnabot-times-cadeia-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let hub = Hub::new(store, "cadeia-fixture".into(), Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
    (hub, dir, work.id)
}

async fn chain(hub: &Arc<Hub>, id: &str) -> Value {
    call(hub, id, FERRAMENTA, &json!({})).await.unwrap()
}

#[tokio::test]
async fn cada_bot_ve_so_o_nivel_de_cima_e_os_diretos_de_baixo() {
    let (hub, dir, _) = fixture();
    let owner = json!({"kind":"owner","label":"Você (o dono)"});
    let ana = chain(&hub, "ana").await;
    assert_eq!(ana["role"], "manager");
    assert_eq!(ana["team"], "Vendas");
    assert_eq!(ana["reportsTo"], owner);
    assert_eq!(ana["directReports"], json!([{"id":"angela","name":"Angela"}]), "sem neta Marisa");
    assert_eq!(ana["hiddenDirectReports"], 1, "a Sombra só conta");
    let angela = chain(&hub, "angela").await;
    assert_eq!(angela["reportsTo"], json!({"kind":"bot","id":"ana","name":"Ana"}));
    assert_eq!(angela["directReports"], json!([{"id":"marisa","name":"Marisa"}]));
    assert_eq!(angela["hiddenDirectReports"], 0);
    let marisa = chain(&hub, "marisa").await;
    assert_eq!(marisa["role"], "executor");
    assert_eq!(marisa["reportsTo"], json!({"kind":"bot","id":"angela","name":"Angela"}), "sem avó Ana");
    assert_eq!(marisa["directReports"], json!([]));
    assert!(marisa["note"].as_str().unwrap().contains("não dá poder"));
    let zeca = chain(&hub, "zeca").await;
    assert_eq!(zeca["reportsTo"], json!({"kind":"hidden"}), "oculto em cima sem id/nome");
    assert_eq!(zeca["team"], Value::Null);
    for id in ["ana", "angela", "marisa", "zeca", "donna", "nova"] {
        let text = chain(&hub, id).await.to_string();
        for private in ["Sombra", "sombra", "Fora", "fora", "privad", "routes", "fromBotId", "@"] {
            assert!(!text.contains(private), "{id} vazou {private}: {text}");
        }
    }
    assert!(call(&hub, "sombra", FERRAMENTA, &json!({})).await.is_err(), "oculto não consulta");
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn principal_e_bot_sem_registro_reportam_ao_dono_sem_dado_pessoal() {
    let (hub, dir, _) = fixture();
    for id in ["donna", "nova"] {
        let answer = chain(&hub, id).await;
        assert_eq!(answer["reportsTo"], json!({"kind":"owner","label":"Você (o dono)"}));
        let mut keys: Vec<_> = answer.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["directReports", "hiddenDirectReports", "note", "reportsTo", "role", "team"]);
    }
    assert_eq!(chain(&hub, "donna").await["role"], "manager");
    assert_eq!(chain(&hub, "nova").await["role"], "executor");
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn outro_space_isolado_argumento_extra_recusado_e_nada_gravado() {
    let (hub, dir, work) = fixture();
    let fora = chain(&hub, "fora").await;
    assert_eq!(fora["reportsTo"], json!({"kind":"owner","label":"Você (o dono)"}));
    assert_eq!(fora["directReports"], json!([]));
    assert_eq!(fora["hiddenDirectReports"], 0);
    let revision = |space: &str| store::snapshot(&hub.store.db.locked(), space).unwrap().revision;
    let before = (revision(PERSONAL), revision(&work));
    let changes = hub.store.db.locked().total_changes();
    for args in [json!({"botId":"ana"}), json!({"x":1}), json!([]), json!("ana")] {
        let error = call(&hub, "angela", FERRAMENTA, &args).await.unwrap_err().to_string();
        assert!(error.contains("não recebe argumentos"), "{error}");
    }
    for id in ["ana", "angela", "marisa", "fora", "donna"] {
        chain(&hub, id).await;
    }
    assert!(call(&hub, "angela", FERRAMENTA, &Value::Null).await.is_ok());
    assert_eq!(hub.store.db.locked().total_changes(), changes, "nenhuma gravação");
    assert_eq!((revision(PERSONAL), revision(&work)), before, "revisão de Times intacta");
    let tools = crate::chat::team::tools();
    let tool = tools.as_array().unwrap().iter().find(|t| t["name"] == FERRAMENTA).unwrap();
    assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    assert_eq!(tool["annotations"], json!({"readOnlyHint":true,"idempotentHint":true}));
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn donna_lista_equipes_e_chefes_sem_ocultos_outro_space_ou_gravacao() {
    let (hub, dir, work) = fixture();
    bot(&hub.store, "ana", "Joio", PERSONAL, false);
    bot(&hub.store, "angela", "Crivo", PERSONAL, false);
    let snapshot = serde_json::to_value(store::snapshot(&hub.store.db.locked(), PERSONAL).unwrap()).unwrap();
    let youtube = uuid::Uuid::new_v4().to_string();
    let roteiros = uuid::Uuid::new_v4().to_string();
    let secreta = uuid::Uuid::new_v4().to_string();
    let vazia = uuid::Uuid::new_v4().to_string();
    let request = json!({
        "spaceId":PERSONAL,"expectedRevision":snapshot["revision"],"catalogVersion":snapshot["catalogVersion"],
        "nodes":[
            {"id":youtube,"name":"Youtube","parentId":null,"managerBotId":"ana","position":0},
            {"id":roteiros,"name":"Roteiros","parentId":youtube,"managerBotId":"angela","position":0},
            {"id":secreta,"name":"Equipe secreta","parentId":null,"managerBotId":"sombra","position":1},
            {"id":vazia,"name":"Equipe vazia","parentId":null,"managerBotId":null,"position":2}
        ],
        "members":[
            member("donna","manager",None,None),
            member("ana","manager",Some(&youtube),None),
            member("angela","manager",Some(&roteiros),None),
            member("marisa","executor",Some(&roteiros),Some("angela")),
            member("sombra","manager",Some(&secreta),None),
            member("zeca","executor",None,None),
            member("nova","executor",None,None)
        ],
        "routes":[],"clientRequestId":uuid::Uuid::new_v4().to_string()
    });
    {
        let mut db = hub.store.db.locked();
        let tx = db.transaction().unwrap();
        store::change(&tx, &serde_json::from_value::<Change>(request).unwrap(), true).unwrap();
        tx.commit().unwrap();
    }
    let revision = |space: &str| store::snapshot(&hub.store.db.locked(), space).unwrap().revision;
    let before = (revision(PERSONAL), revision(&work));
    let changes = hub.store.db.locked().total_changes();
    let answer = call(&hub, "donna", "list_bots", &json!({})).await.unwrap();
    let bots = answer["bots"].as_array().unwrap();
    assert_eq!(bots.len(), 5);
    for (id, team, chief) in [
        ("ana", json!("Youtube"), true),
        ("angela", json!("Youtube › Roteiros"), true),
        ("marisa", json!("Youtube › Roteiros"), false),
        ("zeca", Value::Null, false),
        ("nova", Value::Null, false),
    ] {
        let bot = bots.iter().find(|b| b["id"] == id).unwrap();
        assert_eq!(bot["team"], team);
        assert_eq!(bot["teamChief"], chief);
        let mut keys: Vec<_> = bot.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["backend", "cwd", "description", "id", "name", "status", "team", "teamChief"]);
    }
    for private in ["sombra", "Sombra", "Equipe secreta", "Equipe vazia", "fora", "Fora", "reportsTo", "routes"] {
        assert!(!answer.to_string().contains(private), "vazou {private}");
    }
    let other = call(&hub, "fora", "list_bots", &json!({})).await.unwrap();
    assert_eq!(other, json!({"bots":[]}));
    let from_joio = call(&hub, "ana", "list_bots", &json!({})).await.unwrap();
    let donna = from_joio["bots"].as_array().unwrap().iter().find(|b| b["id"] == "donna").unwrap();
    assert_eq!(donna["team"], Value::Null);
    assert_eq!(donna["teamChief"], false);
    assert_eq!(hub.store.db.locked().total_changes(), changes, "nenhuma gravação");
    assert_eq!((revision(PERSONAL), revision(&work)), before);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn list_bots_interrompe_caminho_com_ciclo_persistido_sem_gravar() {
    let (hub, dir, _) = fixture();
    {
        let c = hub.store.db.locked();
        let vendas: String = c
            .query_row("SELECT id FROM donna_team_nodes WHERE space_id=?1 AND name='Vendas'", [PERSONAL], |r| r.get(0))
            .unwrap();
        let roteiros = uuid::Uuid::new_v4().to_string();
        // Simula dados persistidos inválidos, fora da validação da gravação do Quadro.
        c.execute(
            "INSERT INTO donna_team_nodes(space_id,id,name,parent_id,position) VALUES(?1,?2,'Roteiros',?3,0)",
            rusqlite::params![PERSONAL, roteiros, vendas],
        )
        .unwrap();
        c.execute(
            "UPDATE donna_team_nodes SET parent_id=?1 WHERE space_id=?2 AND id=?3",
            rusqlite::params![roteiros, PERSONAL, vendas],
        )
        .unwrap();
    }
    let before = store::snapshot(&hub.store.db.locked(), PERSONAL).unwrap().revision;
    let changes = hub.store.db.locked().total_changes();
    let answer = call(&hub, "donna", "list_bots", &json!({})).await.unwrap();
    let ana = answer["bots"].as_array().unwrap().iter().find(|b| b["id"] == "ana").unwrap();
    assert_eq!(ana["team"], "Roteiros › Vendas", "para antes de repetir a equipe");
    assert_eq!(ana["teamChief"], true);
    assert_eq!(hub.store.db.locked().total_changes(), changes, "não repara nem grava o Quadro");
    assert_eq!(store::snapshot(&hub.store.db.locked(), PERSONAL).unwrap().revision, before);
    std::fs::remove_dir_all(dir).unwrap();
}
