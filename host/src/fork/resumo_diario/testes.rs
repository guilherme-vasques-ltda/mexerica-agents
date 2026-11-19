#![allow(clippy::unwrap_used)]
//! MB-3 (D70): só fronteiras. Motor simulado; nenhuma chamada real.
use super::{Fora, Motor, fora, plano, store, tick, torre};
use crate::{
    LockExt,
    fork::{control_tower::Observation, spaces::PERSONAL},
    hub::Hub,
    remote::identity::Identity,
    store::{BotConfig, EntryKind, Lane, Store, now_ms},
};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

const CONTA: &str = "ac_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn bot(store: &Store, id: &str, name: &str, space: &str, backend: &str) {
    let mut cfg = json!({"id":id,"name":name,"backend":backend,"spaceId":space,"model":"haiku"});
    if backend == "claude" {
        cfg["forkEditor"] = json!({"engineAccountId": CONTA});
    }
    store.save_bot(&serde_json::from_value::<BotConfig>(cfg).unwrap()).unwrap();
}

fn member(id: &str, team: Option<&str>, up: Option<&str>) -> Value {
    json!({
        "botId":id,"teamId":team,"parentBotId":null,"role":"executor",
        "reportsTo":up.map_or(json!({"kind":"owner"}), |b| json!({"kind":"bot","botId":b})),
        "ownerContact":"reports"
    })
}

struct F {
    hub: Arc<Hub>,
    work: String,
    calls: Arc<Mutex<Vec<(String, String)>>>,
    motor: Box<Motor>,
}

/// Vendas: Ana (gerente, Você) → Angela → Marisa; Rota (Codex) → Ana.
/// Estoque: Opera (Codex, gerente) ← Cota. Solto sem Time (D329). Fora em outro Space.
fn fixture() -> F {
    let store = Store::open(Path::new(":memory:")).unwrap();
    store
        .kv_set(
            "fork.engine-accounts.v1",
            &json!({"salt":uuid::Uuid::new_v4().to_string(),"accounts":[{
                "id":CONTA,"backend":"claude","billing":"subscription","state":"connected",
                "launch":{"kind":"cli_profile","executable":"/bin/true","directory":"/tmp"},
                "scope":"owner","quotaIdentity":CONTA,"verifiedAt":1,
                "subscriptionProof":{"method":"claude_auth_status","plan":"max"},
                "models":[{"id":"haiku","options":[]}]
            }]})
            .to_string(),
        )
        .unwrap();
    let mut spaces = store.spaces().unwrap();
    let mut work = spaces.spaces[0].clone();
    work.id = uuid::Uuid::new_v4().to_string();
    work.kind = "space".into();
    work.name = "Trabalho".into();
    spaces.spaces.push(work.clone());
    store.kv_set("donna.spaces.v1", &serde_json::to_string(&spaces).unwrap()).unwrap();
    for (id, name, backend) in [
        ("donna", "Donna", "claude"),
        ("ana", "Ana", "claude"),
        ("angela", "Angela", "claude"),
        ("marisa", "Marisa", "claude"),
        ("rota", "Rota", "codex"),
        ("opera", "Opera", "codex"),
        ("cota", "Cota", "claude"),
        ("solto", "Solto", "claude"),
    ] {
        bot(&store, id, name, PERSONAL, backend);
    }
    bot(&store, "fora", "Fora", &work.id, "claude");
    store.set_team_settings(&json!({"primaryBotId":"donna"})).unwrap();
    let snapshot =
        serde_json::to_value(crate::fork::times::resumo_testes_snapshot(&store.db.locked(), PERSONAL)).unwrap();
    let (vendas, estoque) = (uuid::Uuid::new_v4().to_string(), uuid::Uuid::new_v4().to_string());
    let (v, e) = (Some(vendas.as_str()), Some(estoque.as_str()));
    let mut members = vec![
        member("donna", None, None),
        member("ana", v, None),
        member("angela", v, Some("ana")),
        member("marisa", v, Some("angela")),
        member("rota", v, Some("ana")),
        member("opera", e, None),
        member("cota", e, Some("opera")),
        member("solto", None, None),
    ];
    for m in &mut members {
        if ["donna", "ana", "angela", "opera"].contains(&m["botId"].as_str().unwrap()) {
            m["role"] = "manager".into();
        }
    }
    let request = json!({
        "spaceId":PERSONAL,"expectedRevision":snapshot["revision"],"catalogVersion":snapshot["catalogVersion"],
        "nodes":[
            {"id":vendas,"name":"Vendas","parentId":null,"managerBotId":"ana","position":0},
            {"id":estoque,"name":"Estoque","parentId":null,"managerBotId":"opera","position":1}
        ],
        "members":members,"routes":[],"clientRequestId":uuid::Uuid::new_v4().to_string()
    });
    crate::fork::times::resumo_testes_change(&store.db.locked(), request);
    // Fim do dia em UTC; os testes rodam em `agora()`, quando todos os níveis já venceram.
    store::salvar(&store, true, "23:45", "UTC").unwrap();
    let dir = std::env::temp_dir().join(format!("donnabot-resumo-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let hub = Hub::new(store, "resumo-fixture".into(), Identity::load_or_create(&dir).unwrap(), "fixture".into(), 0);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let seen = calls.clone();
    let motor: Box<Motor> = Box::new(move |cfg, user| {
        seen.lock().unwrap().push((cfg.id.clone(), user));
        let id = cfg.id;
        Box::pin(async move {
            Ok(json!({"bot":format!("{id} fez a parte dele"),"time":format!("time de {id}")}).to_string())
        })
    });
    F { hub, work: work.id, calls, motor }
}

fn fala(f: &F, chat: &str, text: &str) {
    f.hub.store.insert_entry(&Lane::main(chat), EntryKind::Agent, 1, &json!({"text":text,"final":true})).unwrap();
}

fn chamados(f: &F) -> Vec<String> {
    f.calls.lock().unwrap().iter().map(|(id, _)| id.clone()).collect()
}

/// Hoje, 23:45 UTC: o horário salvo na fixture.
fn agora() -> i64 {
    let hoje = chrono::Utc::now().date_naive().and_hms_opt(23, 45, 0).unwrap();
    hoje.and_utc().timestamp_millis()
}

fn valor(f: &F) -> Value {
    let roster: Vec<String> = f.hub.store.bots().unwrap().into_iter().map(|r| r.config.id).collect();
    match torre::torre_em(&f.hub, PERSONAL, &roster, agora()).unwrap() {
        Observation::Available { value, .. } => serde_json::to_value(value).unwrap(),
        Observation::Unavailable => panic!("Pessoal deve estar disponível"),
    }
}

#[test]
fn horarios_contam_15_min_por_nivel_a_partir_do_topo() {
    let f = fixture();
    let quadro = {
        let c = f.hub.store.db.locked();
        crate::fork::times::resumo::quadro(&c, PERSONAL).unwrap()
    };
    let cfg = store::Config { enabled: true, time: "19:00".into(), time_zone: "America/Sao_Paulo".into(), revision: 1 };
    let (_, fecha) = plano::dia_atual(&cfg, now_ms());
    let hora = |id: &str| {
        let vez = plano::vezes(&quadro).into_iter().find(|v| v.bot_id == id).unwrap();
        let at = chrono::DateTime::from_timestamp_millis(plano::horario(fecha, vez.nivel)).unwrap();
        at.with_timezone(&cfg.zona()).format("%H:%M").to_string()
    };
    assert_eq!([hora("ana"), hora("angela"), hora("marisa"), hora("solto")], ["18:45", "18:30", "18:15", "18:45"]);
}

#[tokio::test]
async fn de_baixo_para_cima_uma_chamada_por_bot_e_codex_fora() {
    let f = fixture();
    for chat in ["ana", "angela", "marisa", "rota", "cota", "opera"] {
        fala(&f, chat, &format!("{chat} trabalhou hoje"));
    }
    let entries = f.hub.store.entries_since(0, 1000).unwrap().len();
    let revision = serde_json::to_value(crate::fork::times::resumo_testes_snapshot(&f.hub.store.db.locked(), PERSONAL))
        .unwrap()["revision"]
        .clone();
    assert!(tick(&f.hub, agora(), &*f.motor).await.unwrap());
    let ordem = chamados(&f);
    let pos = |id: &str| ordem.iter().position(|x| x == id).unwrap();
    assert!(pos("marisa") < pos("angela") && pos("angela") < pos("ana"), "{ordem:?}");
    // Codex não chama; sem atividade (Donna, Solto) não chama; outro Space não entra.
    for id in ["rota", "opera", "donna", "solto", "fora"] {
        assert!(!ordem.contains(&id.to_owned()), "{id} não pode chamar: {ordem:?}");
    }
    assert_eq!(ordem.len(), 4, "uma chamada por bot que trabalhou: {ordem:?}");
    // A resposta de Angela sobe a Ana pelo Hub, sem ask_bot/message_bot.
    let ana = &f.calls.lock().unwrap().iter().find(|(id, _)| id == "ana").unwrap().1.clone();
    assert!(ana.contains("Angela: time de angela"), "{ana}");
    // Nada vai para conversa alguma (nem a do Berga); quadro e permissões intactos.
    assert_eq!(f.hub.store.entries_since(0, 1000).unwrap().len(), entries);
    let depois =
        serde_json::to_value(crate::fork::times::resumo_testes_snapshot(&f.hub.store.db.locked(), PERSONAL)).unwrap();
    assert_eq!(depois["revision"], revision);
    // D220: cada execução é uma cadeia própria, nível 1, ordinal 1.
    let (cadeias, niveis): (i64, i64) = f
        .hub
        .store
        .db
        .locked()
        .query_row(
            "SELECT COUNT(DISTINCT chain_id),COUNT(*) FROM donna_chain_admissions WHERE level=1 AND ordinal=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((cadeias, niveis), (4, 4));
    // Reinício: segunda passada não repete ninguém.
    assert!(!tick(&f.hub, agora(), &*f.motor).await.unwrap());
    assert_eq!(chamados(&f).len(), 4);
    let v = valor(&f);
    let out: Vec<&str> = v["out"].as_array().unwrap().iter().map(|o| o["botId"].as_str().unwrap()).collect();
    assert_eq!(out, ["opera", "rota"]);
    assert!(v["out"].as_array().unwrap().iter().all(|o| o["reason"] == "codex"));
}

#[tokio::test]
async fn bot_solto_sem_trabalho_nao_gera_chamada_e_com_trabalho_gera_uma() {
    let f = fixture();
    tick(&f.hub, agora(), &*f.motor).await.unwrap();
    assert_eq!(chamados(&f), Vec::<String>::new(), "dia parado = zero chamada");
    // Dia novo para o Solto: apaga a linha `skipped` do dia e registra trabalho.
    f.hub.store.db.locked().execute("DELETE FROM donna_daily_summaries WHERE bot_id='solto'", []).unwrap();
    fala(&f, "solto", "Solto conferiu 14 pedidos");
    tick(&f.hub, agora(), &*f.motor).await.unwrap();
    assert_eq!(chamados(&f), ["solto"]);
}

#[tokio::test]
async fn entrada_so_conversa_propria_no_pessoal() {
    let f = fixture();
    fala(&f, "solto", "PROPRIO");
    f.hub.store.insert_entry(&Lane::main("solto"), EntryKind::Thought, 1, &json!({"text":"PENSAMENTO"})).unwrap();
    f.hub.store.insert_entry(&Lane::main("solto"), EntryKind::Tool, 1, &json!({"text":"FERRAMENTA"})).unwrap();
    // Conversa do Solto em outro Space e um Canal com ele.
    let conv = uuid::Uuid::new_v4().to_string();
    f.hub
        .store
        .db
        .locked()
        .execute(
            "INSERT INTO donna_space_conversations(id,space_id,bot_id,title,revision,is_primary) VALUES(?1,?2,'solto','x',1,0)",
            rusqlite::params![conv, f.work],
        )
        .unwrap();
    fala(&f, &conv, "SPACE");
    fala(&f, "group:canal", "CANAL");
    f.hub.store.kv_set("memoria.fixture", "MEMORIA").unwrap();
    tick(&f.hub, agora(), &*f.motor).await.unwrap();
    let calls = f.calls.lock().unwrap();
    let (_, user) = calls.iter().find(|(id, _)| id == "solto").unwrap();
    assert!(user.contains("PROPRIO"));
    for fora in ["PENSAMENTO", "FERRAMENTA", "SPACE", "CANAL", "MEMORIA"] {
        assert!(!user.contains(fora), "{fora} vazou para a entrada");
    }
}

#[tokio::test]
async fn pausa_global_e_do_bot_impedem() {
    let f = fixture();
    fala(&f, "solto", "trabalhou");
    f.hub.routines.set_paused(&f.hub.store, true).unwrap();
    assert!(!tick(&f.hub, agora(), &*f.motor).await.unwrap());
    f.hub.routines.set_paused(&f.hub.store, false).unwrap();
    crate::fork::bot_admission::set_paused_for_test(&f.hub.store, "solto", true).unwrap();
    tick(&f.hub, agora(), &*f.motor).await.unwrap();
    assert_eq!(chamados(&f), Vec::<String>::new());
    // F2: a pausa do bot não consome o dia; despausado no mesmo dia, roda em atraso.
    crate::fork::bot_admission::set_paused_for_test(&f.hub.store, "solto", false).unwrap();
    tick(&f.hub, agora(), &*f.motor).await.unwrap();
    assert_eq!(chamados(&f), ["solto"]);
}

#[tokio::test]
async fn bot_pausado_nao_segura_o_de_cima() {
    let f = fixture();
    fala(&f, "marisa", "Marisa trabalhou");
    fala(&f, "angela", "Angela trabalhou");
    crate::fork::bot_admission::set_paused_for_test(&f.hub.store, "marisa", true).unwrap();
    tick(&f.hub, agora(), &*f.motor).await.unwrap();
    assert!(chamados(&f).contains(&"angela".to_owned()));
    assert!(!chamados(&f).contains(&"marisa".to_owned()));
}

#[tokio::test]
async fn pausa_global_durante_uma_chamada_vale_para_os_seguintes() {
    let mut f = fixture();
    for chat in ["marisa", "angela", "ana"] {
        fala(&f, chat, "trabalhou");
    }
    let (hub, seen) = (f.hub.clone(), f.calls.clone());
    f.motor = Box::new(move |cfg, user| {
        seen.lock().unwrap().push((cfg.id, user));
        // O dono aperta "Pausar tudo" enquanto a primeira chamada roda.
        hub.routines.set_paused(&hub.store, true).unwrap();
        Box::pin(async { Ok(r#"{"bot":"fez"}"#.to_owned()) })
    });
    tick(&f.hub, agora(), &*f.motor).await.unwrap();
    assert_eq!(chamados(&f), ["marisa"]);
}

#[test]
fn horario_nao_deixa_nivel_cruzar_a_meia_noite() {
    let f = fixture();
    for invalido in ["00:00", "01:45", "19:10", "24:00"] {
        assert!(store::salvar(&f.hub.store, true, invalido, "UTC").is_err(), "{invalido}");
    }
    assert!(store::salvar(&f.hub.store, true, "02:00", "UTC").is_ok());
    // O nível mais fundo (T − 2 h) de 02:00 ainda cai no mesmo dia local.
    let cfg = store::config(&f.hub.store).unwrap();
    let (dia, fecha) = plano::dia_atual(&cfg, agora());
    let fundo = chrono::DateTime::from_timestamp_millis(plano::horario(fecha, 99)).unwrap();
    assert_eq!(fundo.date_naive().to_string(), dia);
}

#[test]
fn sem_modelo_usa_haiku_com_esforco_baixo_ou_fica_fora() {
    let f = fixture();
    let sem_modelo = |id: &str| {
        let mut cfg = f.hub.store.bot(id).unwrap().unwrap().config;
        cfg.model = None;
        cfg
    };
    let registro = |models: Value| {
        json!({"salt":uuid::Uuid::new_v4().to_string(),"accounts":[{
            "id":CONTA,"backend":"claude","billing":"subscription","state":"connected",
            "launch":{"kind":"cli_profile","executable":"/bin/true","directory":"/tmp"},
            "scope":"owner","quotaIdentity":CONTA,"verifiedAt":1,
            "subscriptionProof":{"method":"claude_auth_status","plan":"max"},"models":models
        }]})
        .to_string()
    };
    f.hub
        .store
        .kv_set(
            "fork.engine-accounts.v1",
            &registro(json!([
                {"id":"opus","options":[]},
                {"id":"haiku","options":[{"id":"effort","values":["low","high"],"default":"high"}]}
            ])),
        )
        .unwrap();
    let resolved = crate::fork::background_selection::resolve_own(&f.hub.store, &sem_modelo("solto")).unwrap();
    assert_eq!(resolved.config.model.as_deref(), Some("haiku"));
    assert_eq!(resolved.config.model_options.get("effort").map(String::as_str), Some("low"));
    // Modelo escolhido pelo bot continua valendo.
    let mut proprio = f.hub.store.bot("solto").unwrap().unwrap().config;
    proprio.model = Some("opus".into());
    assert_eq!(
        crate::fork::background_selection::resolve_own(&f.hub.store, &proprio).unwrap().config.model.as_deref(),
        Some("opus")
    );
    f.hub.store.kv_set("fork.engine-accounts.v1", &registro(json!([{"id":"opus","options":[]}]))).unwrap();
    assert_eq!(fora(&f.hub.store, &sem_modelo("solto")), Some(Fora::SemModelo));
}

#[tokio::test]
async fn reinicio_retoma_uma_vez_e_sem_ia_quando_gerente_fora() {
    let f = fixture();
    fala(&f, "cota", "Cota pediu orçamentos");
    tick(&f.hub, agora(), &*f.motor).await.unwrap();
    assert_eq!(chamados(&f), ["cota"]);
    // Opera (Codex, gerente) não chama: a linha do Time é junta pelo computador.
    let v = valor(&f);
    let day = v["day"].as_str().unwrap().to_owned();
    // O dia mostrado é o último fechado; às 23:45 UTC é hoje.
    let (hoje, _) = plano::dia_atual(&store::config(&f.hub.store).unwrap(), agora());
    assert_eq!(day, hoje);
    let estoque = v["teams"].as_array().unwrap().iter().find(|t| t["name"] == "Estoque").unwrap();
    assert_eq!(estoque["line"]["joined"], true);
    assert!(estoque["line"]["text"].as_str().unwrap().contains("Cota: cota fez a parte dele"));
    // Queda no meio: `running` vira `interrupted` ao reabrir e roda uma vez só.
    {
        let c = f.hub.store.db.locked();
        c.execute("UPDATE donna_daily_summaries SET state='running' WHERE bot_id='cota'", []).unwrap();
        store::migrate(&c).unwrap();
    }
    tick(&f.hub, agora(), &*f.motor).await.unwrap();
    assert_eq!(chamados(&f), ["cota", "cota"]);
    {
        let c = f.hub.store.db.locked();
        c.execute("UPDATE donna_daily_summaries SET state='running' WHERE bot_id='cota'", []).unwrap();
        store::migrate(&c).unwrap();
    }
    tick(&f.hub, agora(), &*f.motor).await.unwrap();
    assert_eq!(chamados(&f).len(), 2, "duas tentativas no máximo por bot e dia");
}

#[test]
fn outro_space_fica_indisponivel() {
    let f = fixture();
    assert!(matches!(torre(&f.hub, &f.work, &["fora".into()]).unwrap(), Observation::Unavailable));
}
