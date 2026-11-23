#![allow(clippy::unwrap_used)]
use super::*;
use crate::{
    LockExt,
    store::{BotConfig, Lane, Store},
};
use chrono::TimeZone;
use serde_json::json;

type StoredTurn = (Option<f64>, String, Option<String>, Option<String>);
/// Conta da fixture no formato canônico (`ac_` + 64 hex): a conversa nova do Pessoal retrata o padrão do bot (D372) e
/// recusa um ID fora do formato. Não é conta cadastrada nem segredo.
const ACCOUNT: &str = "ac_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const ACCOUNT_OUTRA: &str = "ac_ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

fn config(id: &str) -> BotConfig {
    serde_json::from_value(json!({"id":id,"name":id,"backend":"claude","model":"modelo-real",
        "forkEditor":{"engineAccountId":ACCOUNT}}))
    .unwrap()
}
fn add(store: &Store, cfg: &BotConfig, lane: &Lane, acp: &str, fresh: bool, cost: Option<f64>) {
    let mut p = Pending::begin(store, cfg, lane, acp, fresh, None).unwrap();
    p.dispatched(store).unwrap();
    if let Some(amount) = cost {
        p.observe(&json!({"cost":{"amount":amount,"currency":"USD"}}));
    }
    p.finish(
        store,
        Some(&json!({"usage":{"inputTokens":10,"cachedReadTokens":20,"cachedWriteTokens":30,"outputTokens":40}})),
    )
    .unwrap();
}
#[test]
fn records_turn_motor_deltas_unpriced_and_never_rewrites_past() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let cfg = config("b");
    let lane = Lane::main("b");
    add(&store, &cfg, &lane, "new", true, Some(0.4));
    add(&store, &cfg, &lane, "new", false, Some(0.7));
    add(&store, &cfg, &lane, "new", false, None);
    // Sessão pré-C8: primeira leitura só estabelece referência; não cobra todo o passado.
    add(&store, &cfg, &lane, "old", false, Some(8.0));
    add(&store, &cfg, &lane, "old", false, Some(8.2));
    // Regressão e troca de moeda não fabricam um delta.
    add(&store, &cfg, &lane, "old", false, Some(1.0));
    let mut p = Pending::begin(&store, &cfg, &lane, "old", false, None).unwrap();
    p.dispatched(&store).unwrap();
    p.observe(&json!({"cost":{"amount":1.5,"currency":"EUR"}}));
    p.finish(&store, Some(&json!({"usage":{"outputTokens":2}}))).unwrap();
    add(&store, &cfg, &lane, "new", false, Some(1.0));
    add(&store, &cfg, &lane, "new", false, Some(1.1));
    let db = store.db.locked();
    let rows: Vec<StoredTurn> = db
        .prepare("SELECT amount,source,account_id,model FROM donna_turn_usage ORDER BY rowid")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(rows.len(), 9);
    assert_eq!(rows[0], (Some(0.4), "motor".into(), Some(ACCOUNT.into()), Some("modelo-real".into())));
    assert!((rows[1].0.unwrap() - 0.3).abs() < 1e-10);
    assert_eq!(rows[2].1, "sem preço");
    assert_eq!(rows[3].0, None);
    assert!((rows[4].0.unwrap() - 0.2).abs() < 1e-10);
    assert_eq!(rows[5].0, None);
    assert_eq!(rows[6].0, None);
    assert_eq!(rows[7].0, None, "a lacuna não é cobrada no próximo turno");
    assert!((rows[8].0.unwrap() - 0.1).abs() < 1e-10);
}

#[test]
fn children_roll_up_to_parent_once_bot_total_is_not_duplicated_and_restart_keeps_ledger() {
    let dir = std::env::temp_dir().join(format!("cost-ledger-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.db");
    {
        let store = Store::open(&path).unwrap();
        let cfg = config("b");
        let other = config("other");
        // Catálogo descartável: exercita a referência explícita entre sessões.
        {
            let db = store.db.locked();
            db.execute(
                "INSERT INTO bots(id,config,rev) VALUES(?1,?2,1)",
                rusqlite::params![cfg.id, serde_json::to_string(&cfg).unwrap()],
            )
            .unwrap();
            db.execute(
                "INSERT INTO bots(id,config,rev) VALUES(?1,?2,2)",
                rusqlite::params![other.id, serde_json::to_string(&other).unwrap()],
            )
            .unwrap();
            crate::fork::sessoes::store::ensure_principal(&db, "b").unwrap();
            crate::fork::sessoes::store::ensure_principal(&db, "other").unwrap();
            crate::fork::sessoes::store::create(
                &db,
                &crate::fork::sessoes::store::NewSession {
                    chat: "b".into(),
                    origin: crate::fork::sessoes::origin::SessionOrigin::Subsession,
                    source_session: None,
                    parent_session: Some("b".into()),
                    title: "Ajudante".into(),
                    runs_on: None,
                },
            )
            .unwrap();
        }
        let child: String = store
            .db
            .locked()
            .query_row("SELECT id FROM donna_bot_sessions WHERE parent_session='b'", [], |r| r.get(0))
            .unwrap();
        add(&store, &cfg, &Lane::main("b"), "main", true, Some(0.2));
        add(&store, &cfg, &Lane { chat: "b".into(), session: Some(child.clone()) }, "child", true, Some(0.3));
        add(&store, &other, &Lane::main("other"), "else", true, None);
        let response =
            report::report(&store.db.locked(), "hub", &Read { bot_id: Some("b".into()), ..Read::default() }).unwrap();
        assert_eq!(response["total"]["turns"], 3);
        assert_eq!(response["total"]["inputTokens"], 30);
        assert_eq!(response["conversation"]["totals"]["turns"], 2);
        assert_eq!(response["conversation"]["totals"]["costs"][0]["amount"], 0.5);
        assert_eq!(response["helpers"][0]["sessionId"], child);
        assert_eq!(response["helpers"][0]["totals"]["costs"][0]["amount"], 0.3);
        assert_eq!(response["bots"][0]["totals"]["turns"], 2);
        assert!(
            report::report(
                &store.db.locked(),
                "hub",
                &Read { bot_id: Some("other".into()), session_id: Some(child), ..Read::default() }
            )
            .is_err()
        );
    }
    let reopened = Store::open(&path).unwrap();
    assert_eq!(
        reopened.db.locked().query_row("SELECT COUNT(*) FROM donna_turn_usage", [], |r| r.get::<_, i64>(0)).unwrap(),
        3
    );
    add(&reopened, &config("b"), &Lane::main("b"), "main", false, Some(0.25));
    let amount: f64 = reopened
        .db
        .locked()
        .query_row("SELECT amount FROM donna_turn_usage ORDER BY rowid DESC LIMIT 1", [], |r| r.get(0))
        .unwrap();
    assert!((amount - 0.05).abs() < 1e-10);
    drop(reopened);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn report_groups_by_account_and_model_once_per_turn_within_the_month() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let cfg = config("b");
    {
        let db = store.db.locked();
        db.execute(
            "INSERT INTO bots(id,config,rev) VALUES(?1,?2,1)",
            rusqlite::params![cfg.id, serde_json::to_string(&cfg).unwrap()],
        )
        .unwrap();
        crate::fork::sessoes::store::ensure_principal(&db, "b").unwrap();
        crate::fork::sessoes::store::create(
            &db,
            &crate::fork::sessoes::store::NewSession {
                chat: "b".into(),
                origin: crate::fork::sessoes::origin::SessionOrigin::Subsession,
                source_session: None,
                parent_session: Some("b".into()),
                title: "Ajudante".into(),
                runs_on: None,
            },
        )
        .unwrap();
    }
    let child: String = store
        .db
        .locked()
        .query_row("SELECT id FROM donna_bot_sessions WHERE parent_session='b'", [], |r| r.get(0))
        .unwrap();
    // Conta e modelo reais: principal, ajudante e um turno em EUR, todos no mesmo grupo.
    add(&store, &cfg, &Lane::main("b"), "main", true, Some(0.4));
    add(&store, &cfg, &Lane { chat: "b".into(), session: Some(child) }, "child", true, Some(0.3));
    let mut eur = Pending::begin(&store, &cfg, &Lane::main("b"), "eur", true, None).unwrap();
    eur.dispatched(&store).unwrap();
    eur.observe(&json!({"cost":{"amount":0.2,"currency":"EUR"}}));
    eur.finish(&store, Some(&json!({"usage":{"inputTokens":1}}))).unwrap();
    add(&store, &cfg, &Lane::main("b"), "free", true, None);
    // Bot sem conta nem modelo capturados.
    let anon: BotConfig = serde_json::from_value(json!({"id":"n","name":"n","backend":"claude"})).unwrap();
    add(&store, &anon, &Lane::main("n"), "s", true, Some(0.9));
    // Outro mês não entra no agrupamento do mês corrente.
    let old = chrono::Utc.with_ymd_and_hms(2020, 1, 15, 0, 0, 0).unwrap().timestamp_millis();
    store
        .db
        .locked()
        .execute(
            "INSERT INTO donna_turn_usage(id,bot_id,chat_id,session_id,account_id,model,at,amount,currency,source)
         VALUES('old','b','b','b',?1,'modelo_fora',?2,9.0,'USD','motor')",
            rusqlite::params![ACCOUNT_OUTRA, old],
        )
        .unwrap();

    let response = report::report(&store.db.locked(), "hub", &Read::default()).unwrap();
    assert_eq!(response["total"]["turns"], 5);
    // Sem turno sozinho numa moeda, o grupo anônimo ordena antes pelo mesmo compare das demais listas.
    let accounts = response["accounts"].as_array().unwrap();
    assert_eq!(accounts.len(), 2);
    assert_eq!(accounts[0]["accountId"], Value::Null);
    assert_eq!(accounts[0]["totals"]["turns"], 1);
    assert_eq!(accounts[0]["totals"]["costs"][0]["amount"], 0.9);
    assert_eq!(accounts[1]["accountId"], ACCOUNT);
    assert_eq!(accounts[1]["totals"]["turns"], 4, "ajudante conta uma vez no grupo e no total");
    assert_eq!(accounts[1]["totals"]["unpricedTurns"], 1);
    assert_eq!(accounts[1]["totals"]["inputTokens"], 31);
    let costs = accounts[1]["totals"]["costs"].as_array().unwrap();
    assert_eq!(costs.len(), 2, "moedas separadas, sem conversão");
    assert_eq!(costs[0]["currency"], "EUR");
    assert_eq!(costs[0]["amount"], 0.2);
    assert_eq!(costs[1]["currency"], "USD");
    assert!((costs[1]["amount"].as_f64().unwrap() - 0.7).abs() < 1e-10);
    let models = response["models"].as_array().unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0]["model"], Value::Null);
    assert_eq!(models[0]["totals"]["turns"], 1);
    assert_eq!(models[1]["model"], "modelo-real");
    assert_eq!(models[1]["totals"]["turns"], 4);
    // Recorte de outro mês só mostra o que foi gravado nele.
    let jan =
        report::report(&store.db.locked(), "hub", &Read { month: Some("2020-01".into()), ..Read::default() }).unwrap();
    assert_eq!(jan["total"]["turns"], 1);
    assert_eq!(jan["accounts"][0]["accountId"], ACCOUNT_OUTRA);
    assert_eq!(jan["models"][0]["model"], "modelo_fora");
}

/// MX-159: sem custo do motor, modelo com linha na tabela + tokens conhecidos
/// → `estimado`; motor com custo nunca estima; faltas → `sem preço`.
#[test]
fn estimates_from_table_only_without_motor_cost_and_never_rewrites_past() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let lane = Lane::main("b");
    let cfg = |model: &str| {
        serde_json::from_value::<BotConfig>(
            json!({"id":"b","name":"b","backend":"claude","model":model,"forkEditor":{"engineAccountId":ACCOUNT}}),
        )
        .unwrap()
    };
    // claude-haiku-4-5: $1 in, $0.10 cache-read, $1.25 cache-write, $5 out por MTok.
    // 10 in + 20 read + 30 write + 40 out = (10*1 + 20*0.10 + 30*1.25 + 40*5) / 1e6 = 0.000_249_5 USD.
    add(&store, &cfg("claude-haiku-4-5"), &lane, "s1", true, None);
    // Motor com custo manda: mesma tabela, mesmo modelo, nunca estima.
    add(&store, &cfg("claude-haiku-4-5"), &lane, "s2", true, Some(0.4));
    // Modelo fora da tabela segue sem preço.
    add(&store, &cfg("modelo-inexistente"), &lane, "s3", true, None);
    // Modelo com linha mas sem tokens de entrada+saída fica sem preço.
    let mut p = Pending::begin(&store, &cfg("claude-haiku-4-5"), &lane, "s4", true, None).unwrap();
    p.dispatched(&store).unwrap();
    p.finish(&store, Some(&json!({"usage":{"cachedReadTokens":20}}))).unwrap();
    // Sem resposta de uso também fica sem preço.
    let mut p = Pending::begin(&store, &cfg("claude-haiku-4-5"), &lane, "s5", true, None).unwrap();
    p.dispatched(&store).unwrap();
    p.finish(&store, None).unwrap();
    // Alias declarado na tabela estima pelo preço do modelo canônico.
    let deepseek = |model: &str| {
        serde_json::from_value::<BotConfig>(
            json!({"id":"b","name":"b","backend":"opencode","model":model,"forkEditor":{"engineAccountId":ACCOUNT}}),
        )
        .unwrap()
    };
    // deepseek-flash não tem preço de cache-write verificado; com a contagem
    // em 0 explícita a linha estima (cache-write contribui zero).
    let mut p = Pending::begin(&store, &deepseek("deepseek-v4-flash"), &lane, "s6", true, None).unwrap();
    p.dispatched(&store).unwrap();
    p.finish(
        &store,
        Some(&json!({"usage":{"inputTokens":10,"cachedReadTokens":20,"cachedWriteTokens":0,"outputTokens":40}})),
    )
    .unwrap();
    // Motor com custo NUNCA é estimado (P2-4): sessão antiga sem referência,
    // regressão e troca de moeda ficam `sem preço` mesmo com modelo na tabela.
    let haiku = cfg("claude-haiku-4-5");
    add(&store, &haiku, &lane, "velha", false, Some(8.0)); // só estabelece referência
    add(&store, &haiku, &lane, "velha", false, Some(1.0)); // regressão
    let mut p = Pending::begin(&store, &haiku, &lane, "velha", false, None).unwrap();
    p.dispatched(&store).unwrap();
    p.observe(&json!({"cost":{"amount":1.5,"currency":"EUR"}}));
    p.finish(
        &store,
        Some(&json!({"usage":{"inputTokens":10,"cachedReadTokens":20,"cachedWriteTokens":30,"outputTokens":40}})),
    )
    .unwrap();

    let db = store.db.locked();
    let rows: Vec<StoredTurn> = db
        .prepare("SELECT amount,source,account_id,model FROM donna_turn_usage ORDER BY rowid")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(rows.len(), 9);
    assert_eq!(rows[0].1, "estimado");
    assert!((rows[0].0.unwrap() - 0.000_249_5).abs() < 1e-9, "{}", rows[0].0.unwrap());
    assert_eq!(rows[1].1, "motor");
    assert_eq!(rows[1].0, Some(0.4));
    assert_eq!(rows[2].1, "sem preço");
    assert_eq!(rows[2].0, None);
    assert_eq!(rows[3].1, "sem preço");
    assert_eq!(rows[4].1, "sem preço");
    assert_eq!(rows[5].1, "estimado", "alias declarado segue o preço canônico");
    // deepseek-flash peak: $0.30 in, $0.006 cache-read, $1.20 out por MTok;
    // cache-write sem preço verificado e contagem 0 explícita → contribui zero.
    let esperado = (10.0 * 0.30 + 20.0 * 0.006 + 40.0 * 1.20) / 1e6;
    assert!((rows[5].0.unwrap() - esperado).abs() < 1e-9);
    // P2-4: os três turnos com `latest` inválido ficam `sem preço`, não `estimado`.
    for row in &rows[6..] {
        assert_eq!(*row, (None, "sem preço".into(), Some(ACCOUNT.into()), Some("claude-haiku-4-5".into())));
    }
}

/// MX-159/P2-1: contagem de cache ausente quando a faixa tem preço (ou cache
/// reportado sem preço verificado) deixa o turno `sem preço`; P2-2: a faixa é
/// escolhida pelo tamanho real do prompt.
#[test]
fn missing_cache_counts_or_prices_stay_unpriced_and_tiers_follow_prompt_size() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let lane = Lane::main("b");
    let cfg = |backend: &str, model: &str| {
        serde_json::from_value::<BotConfig>(
            json!({"id":"b","name":"b","backend":backend,"model":model,"forkEditor":{"engineAccountId":ACCOUNT}}),
        )
        .unwrap()
    };
    let turn = |backend: &str, model: &str, acp: &str, versao: Option<&str>, usage: Value| {
        let mut p = Pending::begin(&store, &cfg(backend, model), &lane, acp, true, versao.map(str::to_owned)).unwrap();
        p.dispatched(&store).unwrap();
        p.finish(&store, Some(&json!({"usage": usage}))).unwrap();
    };
    // claude-haiku-4-5 tem preço de cache: contagem ausente → sem preço.
    turn("claude", "claude-haiku-4-5", "c1", None, json!({"inputTokens":10,"outputTokens":40}));
    // deepseek-flash não tem preço de cache-write: cache reportado → sem preço.
    turn(
        "opencode",
        "deepseek-flash",
        "c2",
        Some("2.2.7"),
        json!({"inputTokens":10,"cachedReadTokens":20,"cachedWriteTokens":30,"outputTokens":40}),
    );
    // gpt-5.5-pro não tem preço de cache: sem prova de omissão, caches
    // ausentes → sem preço mesmo sem preço de cache na linha (P2-1 rodada 3).
    turn("codex", "gpt-5.5-pro", "c3", Some("2.6.0"), json!({"inputTokens":10,"outputTokens":40}));
    // `0` explícito vale zero: mesmo modelo estima normalmente.
    turn(
        "codex",
        "gpt-5.5-pro",
        "c3b",
        Some("2.6.0"),
        json!({"inputTokens":10,"cachedReadTokens":0,"cachedWriteTokens":0,"outputTokens":40}),
    );
    // gemini-2.5-pro: prompt 250_000 (>200k) usa a faixa cara 2.50/15;
    // gravação 0 explícita (o gemini-cli nunca a emite → ausente seria
    // `sem preço`, ver missing_cache_follows_the_measured_adapter_policy).
    turn(
        "gemini",
        "gemini-2.5-pro",
        "c4",
        Some("0.68.0"),
        json!({"inputTokens":250_000,"cachedReadTokens":0,"cachedWriteTokens":0,"outputTokens":1_000}),
    );
    let db = store.db.locked();
    let rows: Vec<(Option<f64>, String)> = db
        .prepare("SELECT amount,source FROM donna_turn_usage ORDER BY rowid")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0], (None, "sem preço".into()));
    assert_eq!(rows[1], (None, "sem preço".into()));
    assert_eq!(rows[2], (None, "sem preço".into()), "ausência sem prova → sem preço");
    // gpt-5.5-pro faixa ≤272K: (10*30 + 40*180)/1e6 = 0.007_5
    assert_eq!(rows[3].1, "estimado");
    assert!((rows[3].0.unwrap() - 0.007_5).abs() < 1e-9);
    // faixa >200k: (250_000*2.50 + 1_000*15)/1e6 = 0.64
    assert_eq!(rows[4].1, "estimado");
    assert!((rows[4].0.unwrap() - 0.64).abs() < 1e-9);
    // P2-D: qwen3-coder-flash publica só até 1M; prompt acima → sem preço.
    drop(db);
    turn("qwen", "qwen3-coder-flash", "c5", None, json!({"inputTokens":1_500_000,"outputTokens":10}));
    let db = store.db.locked();
    let extra: Option<f64> =
        db.query_row("SELECT amount FROM donna_turn_usage ORDER BY rowid DESC LIMIT 1", [], |r| r.get(0)).unwrap();
    assert_eq!(extra, None);
}

/// MX-159/P2-E: campo `cost` presente com valor inválido conta como informado
/// → `sem preço`, nunca estima; atualização inválida não apaga o último válido.
#[test]
fn invalid_cost_fields_count_as_informed_and_never_estimate() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let lane = Lane::main("b");
    let cfg = serde_json::from_value::<BotConfig>(
        json!({"id":"b","name":"b","backend":"claude","model":"claude-haiku-4-5","forkEditor":{"engineAccountId":ACCOUNT}}),
    )
    .unwrap();
    let usage = json!({"usage":{"inputTokens":10,"cachedReadTokens":20,"cachedWriteTokens":30,"outputTokens":40}});
    // Custo negativo: presente mas inválido → sem preço.
    let mut p = Pending::begin(&store, &cfg, &lane, "a", true, None).unwrap();
    p.dispatched(&store).unwrap();
    p.observe(&json!({"cost":{"amount":-1.0,"currency":"USD"}}));
    p.finish(&store, Some(&usage)).unwrap();
    // Moeda inválida: idem.
    let mut p = Pending::begin(&store, &cfg, &lane, "b", true, None).unwrap();
    p.dispatched(&store).unwrap();
    p.observe(&json!({"cost":{"amount":0.4,"currency":"usd"}}));
    p.finish(&store, Some(&usage)).unwrap();
    // Válido seguido de inválido: o acumulado final ficou desconhecido →
    // fail-closed, o turno fica sem preço e o cursor é invalidado.
    let mut p = Pending::begin(&store, &cfg, &lane, "c", true, None).unwrap();
    p.dispatched(&store).unwrap();
    p.observe(&json!({"cost":{"amount":0.4,"currency":"USD"}}));
    p.observe(&json!({"cost":{"amount":"lixo","currency":"USD"}}));
    p.finish(&store, Some(&usage)).unwrap();
    let db = store.db.locked();
    let rows: Vec<(Option<f64>, String)> = db
        .prepare("SELECT amount,source FROM donna_turn_usage ORDER BY rowid")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(rows[0], (None, "sem preço".into()));
    assert_eq!(rows[1], (None, "sem preço".into()));
    assert_eq!(rows[2], (None, "sem preço".into()));
    // O cursor foi invalidado: o turno seguinte não herda o consumo escondido
    // — primeira leitura válida só restabelece a referência → sem preço.
    drop(db);
    add(&store, &cfg, &lane, "c", false, Some(0.9));
    add(&store, &cfg, &lane, "c", false, Some(1.2));
    let db = store.db.locked();
    let ultimos: Vec<(Option<f64>, String)> = db
        .prepare("SELECT amount,source FROM donna_turn_usage ORDER BY rowid DESC LIMIT 2")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(ultimos[0].1, "motor");
    assert!((ultimos[0].0.unwrap() - 0.3).abs() < 1e-9);
    assert_eq!(ultimos[1], (None, "sem preço".into()));
}

/// MX-159: o relatório separa informado (`costs`) de estimado
/// (`estimatedCosts`) por moeda, no total, nas contas e nos modelos.
#[test]
fn report_separates_informed_from_estimated_by_currency() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let cfg = |model: &str| {
        serde_json::from_value::<BotConfig>(
            json!({"id":"b","name":"b","backend":"claude","model":model,"forkEditor":{"engineAccountId":ACCOUNT}}),
        )
        .unwrap()
    };
    add(&store, &cfg("claude-haiku-4-5"), &Lane::main("b"), "s1", true, Some(0.4));
    add(&store, &cfg("claude-haiku-4-5"), &Lane::main("b"), "s2", true, None);
    add(&store, &cfg("modelo-inexistente"), &Lane::main("b"), "s3", true, None);
    let response = report::report(&store.db.locked(), "hub", &Read::default()).unwrap();
    let total = &response["total"];
    assert_eq!(total["turns"], 3);
    assert_eq!(total["unpricedTurns"], 1);
    let costs = total["costs"].as_array().unwrap();
    assert_eq!(costs.len(), 1);
    assert_eq!(costs[0]["source"], "motor");
    assert_eq!(costs[0]["amount"], 0.4);
    let estimated = total["estimatedCosts"].as_array().unwrap();
    assert_eq!(estimated.len(), 1);
    assert_eq!(estimated[0]["source"], "estimado");
    assert_eq!(estimated[0]["currency"], "USD");
    assert!((estimated[0]["amount"].as_f64().unwrap() - 0.000_249_5).abs() < 1e-9);
    for group in ["accounts", "models"] {
        let g = &response[group][0]["totals"];
        assert_eq!(g["estimatedCosts"].as_array().unwrap().len(), 1, "{group}");
    }
}

/// MX-159: trabalho de fundo com modelo na tabela também estima; sem modelo
/// conhecido a linha segue `fundo` sem preço.
#[test]
fn background_estimates_with_priced_model_and_stays_unpriced_without() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    record_background(
        &store,
        "b",
        "titles",
        Some("claude-haiku-4-5"),
        None,
        Some(UsoFundo { input: 60, output: 50, read: Some(7), write: Some(0), invalid: false, ..Default::default() }),
    )
    .unwrap();
    record_background(
        &store,
        "b",
        "memory",
        Some("modelo-inexistente"),
        None,
        Some(UsoFundo { input: 60, output: 50, read: Some(7), write: Some(0), invalid: false, ..Default::default() }),
    )
    .unwrap();
    // MX-159/P2-A: modelo com preço de cache mas uso sem contagem de cache →
    // ausência chega como ausência ao estimador → sem preço (não estima).
    record_background(
        &store,
        "b",
        "titles2",
        Some("claude-haiku-4-5"),
        None,
        Some(UsoFundo { input: 60, output: 50, read: None, write: None, invalid: false, ..Default::default() }),
    )
    .unwrap();
    // MX-159/P2-B: campo de cache presente mas não representável → sem preço
    // sempre, mesmo sem preço de cache na linha do modelo.
    record_background(
        &store,
        "b",
        "titles3",
        Some("qwen3-coder-flash"),
        None,
        Some(UsoFundo { input: 60, output: 50, read: None, write: None, invalid: true, ..Default::default() }),
    )
    .unwrap();
    record_background(&store, "b", "digest", None, None, None).unwrap();
    let db = store.db.locked();
    let rows: Vec<(Option<f64>, String)> = db
        .prepare("SELECT amount,source FROM donna_turn_usage ORDER BY rowid")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0].1, "estimado");
    // 60 in + 50 out + 7 read, haiku-4-5: (60*1 + 50*5 + 7*0.10)/1e6 = 0.000_310_7
    assert!((rows[0].0.unwrap() - 0.000_310_7).abs() < 1e-9);
    assert_eq!(rows[1], (None, "fundo".into()));
    assert_eq!(rows[2], (None, "fundo".into()), "cache ausente com preço na tabela → sem preço");
    assert_eq!(rows[3], (None, "fundo".into()), "contagem inválida → sem preço");
    assert_eq!(rows[4], (None, "fundo".into()));
}

// MX-159/P2-5: a validação da tabela embutida vive em `precos::tests`, que
// desserializa com o mesmo tipo tipado da produção (texto inválido falha ali).

/// MX-159 (regra do coordenador): cache ausente só vale zero nos adaptadores
/// medidos que OMITEM o campo quando zero (gemini 0.68.0, opencode 2.2.7 para
/// leitura). Emite-sempre (claude-acp, codex-acp), "sem prova", backend
/// desconhecido ou versão divergente da pinada → ausente = `sem preço`.
#[test]
fn missing_cache_follows_the_measured_adapter_policy() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let lane = Lane::main("b");
    let cfg = |backend: &str, model: &str| {
        serde_json::from_value::<BotConfig>(
            json!({"id":"b","name":"b","backend":backend,"model":model,"forkEditor":{"engineAccountId":ACCOUNT}}),
        )
        .unwrap()
    };
    let usage_sem_cache = json!({"usage":{"inputTokens":10,"outputTokens":40}});
    let turn = |backend: &str, model: &str, versao: Option<&str>, usage: &Value, acp: &str| {
        let mut p = Pending::begin(&store, &cfg(backend, model), &lane, acp, true, versao.map(str::to_owned)).unwrap();
        p.dispatched(&store).unwrap();
        p.finish(&store, Some(usage)).unwrap();
    };
    // claude-acp 0.106.0 emite sempre os 4 campos: ausência é anômala → sem preço.
    turn("claude", "claude-haiku-4-5", Some("0.106.0"), &usage_sem_cache, "p1");
    // codex-acp 2.6.0 emite cachedReadTokens sempre: ausente → sem preço.
    turn("codex", "gpt-6.1-sol", Some("2.6.0"), &usage_sem_cache, "p2");
    // codex-acp: cachedWriteTokens não provado; com leitura 0 explícita mas
    // gravação ausente numa linha com preço de escrita → sem preço.
    turn(
        "codex",
        "gpt-6.1-sol",
        Some("2.6.0"),
        &json!({"usage":{"inputTokens":10,"cachedReadTokens":0,"outputTokens":40}}),
        "p3",
    );
    // gemini-cli 0.68.0 omite cachedReadTokens quando 0 (ausente = zero) mas
    // NUNCA emite cachedWriteTokens — ausência não prova escrita zero →
    // `sem preço` mesmo sem preço de escrita na linha (P2 rodada 4).
    turn("gemini", "gemini-2.5-flash", Some("0.68.0"), &usage_sem_cache, "p4");
    // Com gravação 0 explícita a leitura ausente continua valendo zero →
    // estima.
    turn(
        "gemini",
        "gemini-2.5-flash",
        Some("0.68.0"),
        &json!({"usage":{"inputTokens":10,"cachedWriteTokens":0,"outputTokens":40}}),
        "p4b",
    );
    // Versão diferente da medida → sem prova → sem preço.
    turn("gemini", "gemini-2.5-flash", Some("0.69.0"), &usage_sem_cache, "p5");
    // opencode 2.2.7 omite a leitura quando 0, mas a gravação ficou "sem
    // prova": ausente = sem preço mesmo sem preço de escrita na linha.
    turn("opencode", "deepseek-v4-flash", Some("2.2.7"), &usage_sem_cache, "p6");
    // grok (categoria C) e backend desconhecido → sem preço.
    turn("grok", "grok-4.7", Some("1.0.82"), &usage_sem_cache, "p7");
    turn("minimax", "MiniMax-M3.1-Flash", Some("0.2.7"), &usage_sem_cache, "p8");
    let db = store.db.locked();
    let rows: Vec<(Option<f64>, String)> = db
        .prepare("SELECT amount,source FROM donna_turn_usage ORDER BY rowid")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(rows.len(), 9);
    assert_eq!(rows[0], (None, "sem preço".into()), "claude emite sempre");
    assert_eq!(rows[1], (None, "sem preço".into()), "codex emite read sempre");
    assert_eq!(rows[2], (None, "sem preço".into()), "codex write sem prova");
    assert_eq!(rows[3], (None, "sem preço".into()), "gemini write nunca emitida → ausente sem prova");
    // gemini-2.5-flash: (10*0.3 + 40*2.5)/1e6 = 0.000_103
    assert_eq!(rows[4].1, "estimado");
    assert!((rows[4].0.unwrap() - 0.000_103).abs() < 1e-9, "{}", rows[4].0.unwrap());
    assert_eq!(rows[5], (None, "sem preço".into()), "versão divergente → C");
    assert_eq!(rows[6], (None, "sem preço".into()), "opencode write sem prova → sem preço");
    assert_eq!(rows[7], (None, "sem preço".into()), "grok categoria C");
    assert_eq!(rows[8], (None, "sem preço".into()), "backend sem entrada → C");
}

#[tokio::test]
async fn owner_only_read_rpc_rejects_devices_without_control_and_ponte() {
    use crate::api::devices::Caller;
    use crate::store::Scope;
    let dir = std::env::temp_dir().join(format!("cost-authority-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let hub = Hub::new(
        Store::open(&dir.join("test.db")).unwrap(),
        "hub".into(),
        crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
        "fixture".into(),
        0,
    );
    let denied = Caller::Device { key: "device".into(), scopes: vec![], autoridade: None };
    assert!(crate::api::dispatch(&hub, &denied, METHOD, json!({})).await.is_err());
    let owner = Caller::Device { key: "device".into(), scopes: vec![Scope::Control], autoridade: None };
    assert!(crate::api::dispatch(&hub, &owner, METHOD, json!({})).await.is_ok());
    assert!(crate::api::dispatch(&hub, &Caller::Local, METHOD, json!({})).await.is_ok());
    assert!(!crate::fork::ponte_credencial::PERMITIDOS.contains(&METHOD));
    assert!(crate::api::dispatch(&hub, &Caller::Local, METHOD, json!({"month":"2026-13"})).await.is_err());
    assert!(crate::api::dispatch(&hub, &Caller::Local, METHOD, json!({"month":"中-xx"})).await.is_err());
    drop(hub);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn interrupted_or_failed_finish_does_not_charge_the_next_turn() {
    let dir = std::env::temp_dir().join(format!("cost-interrupted-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.db");
    let cfg = config("b");
    let lane = Lane::main("b");
    {
        let store = Store::open(&path).unwrap();
        add(&store, &cfg, &lane, "same-acp", true, Some(0.4));
        let mut lost = Pending::begin(&store, &cfg, &lane, "same-acp", false, None).unwrap();
        lost.dispatched(&store).unwrap();
        lost.observe(&json!({"cost":{"amount":0.7,"currency":"USD"}}));
        // Shutdown/crash antes de finish: a referência já está inválida no disco.
        drop(lost);
    }
    let store = Store::open(&path).unwrap();
    add(&store, &cfg, &lane, "same-acp", false, Some(1.0));
    let last = || {
        store
            .db
            .locked()
            .query_row("SELECT amount FROM donna_turn_usage ORDER BY rowid DESC LIMIT 1", [], |r| {
                r.get::<_, Option<f64>>(0)
            })
            .unwrap()
    };
    assert_eq!(last(), None, "T3 não recebe os 0,60 que incluem T2 perdido");
    add(&store, &cfg, &lane, "same-acp", false, Some(1.1));
    assert!((last().unwrap() - 0.1).abs() < 1e-10);
    let mut failed = Pending::begin(&store, &cfg, &lane, "same-acp", false, None).unwrap();
    failed.dispatched(&store).unwrap();
    failed.observe(&json!({"cost":{"amount":1.2,"currency":"USD"}}));
    store
        .db
        .locked()
        .execute_batch(
            "CREATE TEMP TRIGGER fail_cost BEFORE INSERT ON donna_turn_usage BEGIN SELECT RAISE(ABORT,'fixture'); END;",
        )
        .unwrap();
    assert!(failed.finish(&store, Some(&json!({"usage":{"outputTokens":2}}))).is_err());
    store.db.locked().execute_batch("DROP TRIGGER fail_cost;").unwrap();
    add(&store, &cfg, &lane, "same-acp", false, Some(1.3));
    assert_eq!(last(), None, "falha de commit não conserva uma referência válida");
    add(&store, &cfg, &lane, "same-acp", false, Some(1.4));
    assert!((last().unwrap() - 0.1).abs() < 1e-10);
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn envio_de_fundo_sem_resposta_permanece_no_ledger() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let store = Store::open(&path).unwrap();
    let (notify, recorded) = tokio::sync::oneshot::channel();
    let request = async {
        let id = begin_background(&store, "bot", "memory", None, None).unwrap();
        notify.send(id).unwrap();
        std::future::pending::<()>().await;
    };
    let id = {
        tokio::pin!(request);
        tokio::select! {
            id = recorded => id.unwrap(),
            () = &mut request => panic!("o executor fictício não deve responder"),
        }
    };
    drop(store);
    let store = Store::open(&path).unwrap();
    let row = store.db.locked().query_row(
        "SELECT bot_id, session_id, input_tokens IS NULL AND output_tokens IS NULL FROM donna_turn_usage WHERE id=?1",
        [&id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, bool>(2)?)),
    ).unwrap();
    assert_eq!(row, ("bot".into(), "background:memory".into(), true));
}

/// MX-145 P3: a consulta individual da sessão sintética de fundo (`background:<job>`)
/// é aceita quando o ledger tem a linha daquele bot na principal dele; sessão
/// sintética sem linha, de outro bot ou fora da principal continua recusada.
#[test]
fn report_accepts_background_synthetic_session_validated_by_ledger() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    for id in ["b", "other"] {
        let cfg = config(id);
        let db = store.db.locked();
        db.execute(
            "INSERT INTO bots(id,config,rev) VALUES(?1,?2,1)",
            rusqlite::params![cfg.id, serde_json::to_string(&cfg).unwrap()],
        )
        .unwrap();
        crate::fork::sessoes::store::ensure_principal(&db, id).unwrap();
    }
    record_background(
        &store,
        "b",
        "titles",
        Some("m"),
        None,
        Some(UsoFundo { input: 60, output: 50, read: Some(7), write: Some(0), invalid: false, ..Default::default() }),
    )
    .unwrap();
    add(&store, &config("b"), &Lane::main("b"), "main", true, Some(0.2));
    let read = |bot: &str, session: &str| Read {
        bot_id: Some(bot.into()),
        session_id: Some(session.into()),
        ..Read::default()
    };
    let response = report::report(&store.db.locked(), "hub", &read("b", "background:titles")).unwrap();
    let selected = &response["conversation"];
    assert_eq!(selected["sessionId"], "background:titles");
    assert_eq!(selected["title"], "Trabalho de fundo (titles)");
    assert_eq!(selected["totals"]["turns"], 1, "só a linha de fundo, nunca a principal");
    assert_eq!(selected["totals"]["inputTokens"], 60);
    for (bot, session) in [("b", "background:memory"), ("other", "background:titles")] {
        let error = report::report(&store.db.locked(), "hub", &read(bot, session)).unwrap_err();
        assert!(format!("{error:#}").contains("unknown_cost_session"), "{bot}/{session}: {error:#}");
    }
}

#[test]
fn uso_do_turno_soma_so_o_que_o_motor_informou() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let cfg = config("b");
    let lane = Lane::main("b");
    let run = |turn: Option<i64>, usage: Value| {
        let mut p = Pending::begin(&store, &cfg, &lane, "s", true, None).unwrap().with_turn(turn);
        p.dispatched(&store).unwrap();
        p.finish(&store, Some(&json!({ "usage": usage }))).unwrap();
    };
    run(Some(7), json!({"inputTokens":10,"cachedReadTokens":20,"cachedWriteTokens":30,"outputTokens":40}));
    run(Some(8), json!({"inputTokens":5,"outputTokens":6}));
    run(None, json!({"inputTokens":99,"outputTokens":99}));
    let usage = |turn: i64| turn_usage(&store.db.locked(), "b", "b", "b", turn).unwrap();
    assert_eq!(usage(7).unwrap(), json!({"input":10,"cacheRead":20,"cacheWrite":30,"output":40}));
    // Cache que o motor não informou fica null: nunca vira zero nem estimativa.
    assert_eq!(usage(8).unwrap(), json!({"input":5,"cacheRead":null,"cacheWrite":null,"output":6}));
    assert!(usage(9).is_none(), "turno sem linha de uso");
    assert!(turn_usage(&store.db.locked(), "b", "b", "outra-sessao", 7).unwrap().is_none());
    assert!(turn_usage(&store.db.locked(), "outro-bot", "b", "b", 7).unwrap().is_none(), "uso de outro bot não vaza");
    // Duas linhas no mesmo turno: soma o que as duas informaram; o que uma não informou fica null.
    run(Some(7), json!({"inputTokens":1,"outputTokens":2}));
    assert_eq!(usage(7).unwrap(), json!({"input":11,"cacheRead":null,"cacheWrite":null,"output":42}));
    // Soma que não cabe em i64: contagem indisponível, sem derrubar o recibo.
    run(Some(10), json!({"inputTokens":i64::MAX,"outputTokens":1}));
    run(Some(10), json!({"inputTokens":1,"outputTokens":1}));
    assert_eq!(usage(10).unwrap(), json!({"input":null,"cacheRead":null,"cacheWrite":null,"output":2}));
}

#[test]
fn banco_antigo_ganha_a_coluna_do_turno_uma_vez() {
    let c = rusqlite::Connection::open_in_memory().unwrap();
    c.execute_batch(
        "CREATE TABLE donna_turn_usage(
          id TEXT PRIMARY KEY, bot_id TEXT NOT NULL, chat_id TEXT NOT NULL, session_id TEXT NOT NULL,
          parent_session_id TEXT, account_id TEXT, model TEXT, at INTEGER NOT NULL,
          input_tokens INTEGER, cached_read_tokens INTEGER, cached_write_tokens INTEGER, output_tokens INTEGER,
          amount REAL, currency TEXT, source TEXT NOT NULL);",
    )
    .unwrap();
    migrate(&c).unwrap();
    migrate(&c).unwrap();
    let columns: i64 = c
        .query_row("SELECT COUNT(*) FROM pragma_table_info('donna_turn_usage') WHERE name='turn'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(columns, 1);
}
