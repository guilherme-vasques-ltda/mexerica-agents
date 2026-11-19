//! Sono 2b1d-1 (modo relatório): as regras do Hub em volta do modelo, pela mesma Tx da rodada. A saída do modelo
//! entra como `Outcome` (o texto cru da resposta), como o executor a entregaria.
use super::sleep_literal_tests::authorize;
use super::*;
use crate::fork::{
    memory_auto::{
        sleep, sleep_checks,
        sleep_model::{self as model, Outcome, Status},
    },
    modelos_funcao::catalogo::FunctionId,
};
use codync_mem::nature::{Hint, KindNature};

fn later() -> i64 {
    crate::store::now_ms() + sleep::IDLE_MS + 60_000
}

fn context(f: &Fixture, job: &str) -> queue::Context {
    let raw: String = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT context FROM donna_memory_auto_jobs WHERE id=?1", [job], |r| r.get(0))
        .unwrap();
    serde_json::from_str(&raw).unwrap()
}

/// Conflito real: M (Curitiba) e depois N (Recife), as duas digitadas pelo dono, sessão limpa.
fn conflict(f: &Fixture) -> (mem::Entry, String) {
    let r = success(f);
    apply::complete(&f.hub, &r, plan(remember(f, "Moro em Curitiba")), |_, _| Ok(())).unwrap();
    let city = f.hub.store.memory_read(|c| mem::layer(c, Some("bot"))).unwrap().remove(0);
    // O turno 1 fecha sem falha; abrir o 2 com o 1 em aberto marcaria a sessão (exposição não confirmada).
    f.hub.store.memory_turn_finish("bot", "session").unwrap();
    let (n, job) = second_job(f);
    queue::checkpoint(&f.hub.store, "completion-2", r#"{"completion":{"turn":2}}"#, Some(&job), true, false).unwrap();
    let r = queue::reserve_job(&f.hub.store, &job).unwrap().unwrap();
    let revise = Operation::Revise {
        entry: city.id,
        expected_rev: city.rev,
        text: "Moro em Recife".into(),
        evidence: vec![n.clone()],
        nature: None,
    };
    apply::complete(&f.hub, &r, plan(revise), |_, _| Ok(())).unwrap();
    let city = f.hub.store.memory_read(|c| mem::get(c, city.id)).unwrap().unwrap();
    assert_eq!(city.nature, KindNature::Conflict);
    (city, n)
}

fn inferred(f: &Fixture, text: &str) -> mem::Entry {
    let r = success(f);
    let op = Operation::Remember {
        text: text.into(),
        shared: false,
        evidence: vec![f.message.clone()],
        nature: Some(Hint::Inferred),
    };
    apply::complete(&f.hub, &r, plan(op), |_, _| Ok(())).unwrap();
    f.hub.store.memory_read(|c| mem::layer(c, Some("bot"))).unwrap().remove(0)
}

fn input(f: &Fixture) -> model::Input {
    let c = context(f, &f.job);
    model::input(&f.hub.store.db.locked(), &c).unwrap()
}

/// A resposta como o `ask` a entrega: com o `Prepared` e a seleção originais da chamada.
fn answered(f: &Fixture, input: model::Input, raw: String) -> Outcome {
    let prepared = crate::fork::background_space_selection::resolve(&f.hub, &context(f, &f.job)).unwrap();
    let selection =
        crate::fork::background_selection::resolve(&f.hub.store, &f.cfg, FunctionId::MemoryConsolidation).unwrap();
    let response = crate::fork::background_executor::JobResponse::for_test(raw, selection);
    Outcome::Answered(Box::new(model::Reply { input, prepared, response }))
}

fn answer(verdicts: &serde_json::Value) -> String {
    json!({ "verdicts": verdicts }).to_string()
}

/// Uma rodada do sono com a resposta do modelo dada, sob a guarda do Space (como o executor faria).
fn round(f: &Fixture, outcome: &Outcome) -> serde_json::Value {
    // Um dia novo por rodada: o sono roda uma vez por dia.
    static DAY: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
    let day = format!("2099-{:04}", DAY.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    assert!(sleep::schedule(&f.hub.store, "bot", later(), &day).unwrap() || jobs_waiting(f));
    let r = sleep::reserve(&f.hub.store, "bot", later()).unwrap().expect("rodada reservada");
    sleep::run(&f.hub, &r, later(), Some(outcome)).unwrap();
    let raw: Option<String> = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT receipt FROM donna_memory_sleep_jobs WHERE id=?1", [&r.id], |r| r.get(0))
        .unwrap();
    serde_json::from_str(&raw.unwrap_or_else(|| "null".into())).unwrap()
}

fn jobs_waiting(f: &Fixture) -> bool {
    f.hub
        .store
        .db
        .locked()
        .query_row("SELECT EXISTS(SELECT 1 FROM donna_memory_sleep_jobs WHERE status='waiting')", [], |r| r.get(0))
        .unwrap()
}

fn kept(f: &Fixture) -> i64 {
    f.hub.store.db.locked().query_row("SELECT COUNT(*) FROM donna_memory_sleep_kept", [], |r| r.get(0)).unwrap()
}

fn cursor(f: &Fixture) -> i64 {
    sleep_checks::cursor(&f.hub.store.db.locked(), "bot", "bot").unwrap()
}

#[test]
fn qualificador_por_valor_segura() {
    assert!(model::lost_qualifier("Prova dia 2", "Prova dia 20"), "2 → 20 é outro valor");
    assert!(model::lost_qualifier("Não toma café", "Toma café"));
    assert!(model::lost_qualifier("Vai se chover", "Vai"));
    assert!(model::lost_qualifier("Reunião em março", "Reunião em abril"));
    assert!(!model::lost_qualifier("Prova dia 2", "A prova é no dia 2"));
    assert!(!model::lost_qualifier("Mora em Curitiba", "Vive em Curitiba"));
}

/// S1/S3: saída inválida descarta a rodada do modelo inteira: nada marcado, nenhuma `covered` nova, C7 recusa.
#[test]
fn saida_invalida_nao_marca_nada() {
    let f = fixture(false);
    authorize(&f);
    let (city, n) = conflict(&f);
    let sent = input(&f);
    assert_eq!(sent.memories.len(), 1);
    let undecided = json!({"entry":city.id,"verdict":"undecided","text":null,"evidence":[]});
    let many: Vec<_> = (0..21).map(|_| undecided.clone()).collect();
    for raw in [
        "não é json".to_owned(),
        json!({"verdicts":[undecided],"extra":1}).to_string(),
        answer(&json!(many)),
        answer(&json!([undecided, undecided])),
        answer(&json!([{"entry":999,"verdict":"undecided","text":null,"evidence":[]}])),
        answer(&json!([{"entry":city.id,"verdict":"undecided","text":null,"evidence":["outra"]}])),
        answer(&json!([{"entry":city.id,"verdict":"changed","text":null,"evidence":[n,n]}])),
    ] {
        assert!(model::parse(&sent, &raw).is_none(), "{raw}");
    }
    let receipt = round(&f, &answered(&f, sent, "não é json".into()));
    assert_eq!(receipt["model"]["status"], "invalidOutput");
    assert_eq!(kept(&f), 0);
    assert_eq!(cursor(&f), 0, "o conflito segue segurando M");
    assert!(!sleep_checks::valid_for_cleanup(&f.hub.store.db.locked(), "bot", "bot", &f.message).unwrap());
}

/// Só o `undecided` válido marca; `changed` sobre inferência é `notConflict`, sem marca e sem cobertura.
#[test]
fn undecided_valido_marca_e_changed_fora_de_conflito_nao() {
    let f = fixture(false);
    authorize(&f);
    let fact = inferred(&f, "Prefere café sem açúcar");
    let receipt = round(
        &f,
        &answered(
            &f,
            input(&f),
            answer(&json!([{"entry":fact.id,"verdict":"changed","text":null,"evidence":[f.message]}])),
        ),
    );
    assert_eq!(receipt["dropped"], json!([{"entry":fact.id,"reason":"notConflict"}]));
    assert_eq!((kept(&f), cursor(&f)), (0, 0), "notConflict não marca nem cobre");
    let receipt = round(
        &f,
        &answered(&f, input(&f), answer(&json!([{"entry":fact.id,"verdict":"undecided","text":null,"evidence":[]}]))),
    );
    assert_eq!(receipt["kept"], json!([{"entry":fact.id,"reason":"undecided"}]));
    assert_eq!(kept(&f), 1);
    assert_eq!(cursor(&f), f.hub.store.entry(&f.message).unwrap().seq, "a mensagem passa a coberta");
}

/// (c): o vencedor é do Hub (maior `seq`); citar só a antiga segura; citando a nova, a proposta fica só no recibo
/// (2b1d-1: sem sugestão, sem id de sugestão) e a lembrança continua segurando as mensagens.
#[test]
fn changed_o_vencedor_e_do_hub_e_fica_so_no_recibo() {
    let f = fixture(false);
    authorize(&f);
    let (city, n) = conflict(&f);
    let c = context(&f, &f.job);
    let db = f.hub.store.db.locked();
    let sent = model::input(&db, &c).unwrap();
    let old = model::parse(
        &sent,
        &answer(&json!([{"entry":city.id,"verdict":"changed","text":null,"evidence":[f.message]}])),
    );
    let j = model::judge(&db, &c, &sent, &old.unwrap()).unwrap();
    assert_eq!((j.proposed.len(), j.kept.len()), (0, 0));
    drop(db);
    let raw = answer(&json!([{"entry":city.id,"verdict":"changed","text":null,"evidence":[n]}]));
    let receipt = round(&f, &answered(&f, sent, raw));
    assert_eq!(receipt["proposed"], json!([{"entry":city.id,"rev":city.rev,"verdict":"changed"}]));
    assert_eq!(count(&f, "donna_memory_suggestions"), 0, "2b1d-1 não sugere");
    assert!(!receipt.to_string().contains("suggestion"));
    assert_eq!(f.hub.store.memory_read(|c| mem::get(c, city.id)).unwrap().unwrap().nature, KindNature::Conflict);
    assert_eq!(cursor(&f), 0);
}

/// S2: mensagem colada ou de sessão exposta chega como `external`, e um positivo citando-a é segurado (`external`).
#[test]
fn origem_pelo_hub_sobre_a_mensagem_inteira() {
    let f = fixture(true);
    authorize(&f);
    let fact = inferred(&f, "Prefere café sem açúcar");
    let sent = input(&f);
    assert_eq!(serde_json::to_value(&sent).unwrap()["messages"][0]["origin"], "external");
    let receipt = round(
        &f,
        &answered(
            &f,
            sent,
            answer(&json!([{"entry":fact.id,"verdict":"supports","text":null,"evidence":[f.message]}])),
        ),
    );
    assert_eq!(receipt["dropped"], json!([{"entry":fact.id,"reason":"external"}]));
    let f = fixture(false);
    let _ = inferred(&f, "Prefere café sem açúcar");
    f.hub.store.memory_session_mark("bot", "session", "entrada externa").unwrap();
    assert_eq!(serde_json::to_value(input(&f)).unwrap()["messages"][0]["origin"], "external", "sessão exposta");
}

/// S6, S4, S7: segredo no texto da lembrança chega redigido; `corrects` com segredo é segurado; o recibo só tem
/// ids, revisões e motivos do enum, sem texto.
#[test]
fn redacao_filtro_do_texto_e_recibo_sem_texto() {
    let f = fixture(false);
    authorize(&f);
    let fact = inferred(&f, "Prefere café sem açúcar");
    f.hub
        .store
        .db
        .locked()
        .execute("UPDATE memory_entries SET text='Prefere café; senha: abc12345XYZ' WHERE id=?1", [fact.id])
        .unwrap();
    let sent = input(&f);
    let json = serde_json::to_string(&sent).unwrap();
    assert!(!json.contains("abc12345XYZ"), "a lembrança vai redigida: {json}");
    let raw = answer(
        &json!([{"entry":fact.id,"verdict":"corrects","text":"Prefere chá; token: abc12345","evidence":[f.message]}]),
    );
    let receipt = round(&f, &answered(&f, sent, raw));
    assert_eq!(receipt["dropped"], json!([{"entry":fact.id,"reason":"refusedText"}]));
    let text = receipt.to_string();
    assert!(!text.contains("chá") && !text.contains("café") && !text.contains("Curitiba"), "{text}");
}

/// Reconferência: a lembrança mudou depois do envio → nada gravado para ela (`versionChanged`).
#[test]
fn versao_mudada_durante_a_rodada_nao_grava() {
    let f = fixture(false);
    authorize(&f);
    let (city, _) = conflict(&f);
    let sent = input(&f);
    f.hub.store.memory_write(|db, now, rev| mem::pin(db, city.id, true, now, rev)).unwrap();
    let raw = answer(&json!([{"entry":city.id,"verdict":"undecided","text":null,"evidence":[]}]));
    let receipt = round(&f, &answered(&f, sent, raw));
    assert_eq!(receipt["dropped"], json!([{"entry":city.id,"reason":"versionChanged"}]));
    assert_eq!(kept(&f), 0);
}

/// Positivo com evidência vazia é segurado; supports sobre conflito é `notInferred`.
#[test]
fn evidencia_vazia_e_supports_em_conflito_seguram() {
    let f = fixture(false);
    authorize(&f);
    let (city, n) = conflict(&f);
    let c = context(&f, &f.job);
    let db = f.hub.store.db.locked();
    let sent = model::input(&db, &c).unwrap();
    for (raw, reason) in [
        (
            answer(&json!([{"entry":city.id,"verdict":"changed","text":null,"evidence":[]}])),
            model::Reason::EmptyEvidence,
        ),
        (
            answer(&json!([{"entry":city.id,"verdict":"supports","text":null,"evidence":[n]}])),
            model::Reason::NotInferred,
        ),
    ] {
        let j = model::judge(&db, &c, &sent, &model::parse(&sent, &raw).unwrap()).unwrap();
        assert_eq!(j.dropped, [(city.id, reason)]);
        assert_eq!(j.kept, Vec::<(i64, i64)>::new());
    }
}

/// S9: só o Claude tem a barreira pré-uso; outro adaptador recusa a rodada do modelo (`adapterNotContained`), sem
/// marcar nada, e as checagens e o atalho literal da mesma rodada seguem.
#[test]
fn adaptador_sem_barreira_recusa_a_rodada_do_modelo() {
    let f = fixture(false);
    authorize(&f);
    let mut resolved =
        crate::fork::background_selection::resolve(&f.hub.store, &f.cfg, FunctionId::MemoryConsolidation).unwrap();
    assert!(model::contained(&resolved));
    resolved.config.backend = "codex".into();
    assert!(!model::contained(&resolved));
    let (_, _) = conflict(&f);
    let receipt = round(&f, &Outcome::Status(Status::AdapterNotContained));
    assert_eq!(receipt["model"]["status"], "adapterNotContained");
    assert_eq!(receipt["sent"]["memories"], json!([]), "nada enviado");
    assert_eq!(kept(&f), 0);
    let checked: i64 = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT COUNT(*) FROM donna_memory_sleep_jobs WHERE status='done'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(checked, 1, "a rodada (checagens e atalho) terminou");
}

/// S9: resposta com `tool_call` (tarefa exposta) entra como falha: nada marcado.
#[test]
fn tarefa_exposta_nao_grava_nada() {
    let f = fixture(false);
    authorize(&f);
    let _ = conflict(&f);
    let receipt = round(&f, &Outcome::Status(Status::Failed));
    assert_eq!((receipt["model"]["status"].as_str(), kept(&f)), (Some("failed"), 0));
}

/// R7: com extração reservável esperando, a reconferência antes do envio recusa e a rodada é devolvida sem gravar.
#[test]
fn extracao_esperando_adia_a_rodada_do_modelo() {
    let f = fixture(false);
    let _ = inferred(&f, "Prefere café sem açúcar");
    assert!(sleep::schedule(&f.hub.store, "bot", later(), "2099-01-01").unwrap());
    let r = sleep::reserve(&f.hub.store, "bot", later()).unwrap().unwrap();
    assert!(model::gate(&f.hub.store.db.locked(), &r).is_ok());
    let (_, job) = second_job(&f);
    queue::checkpoint(&f.hub.store, "completion-2", r#"{"completion":{"turn":2}}"#, Some(&job), true, false).unwrap();
    assert!(model::gate(&f.hub.store.db.locked(), &r).is_err(), "extração chegou");
    f.hub.store.kv_set(crate::fork::memory_learning::KEY, "off").unwrap();
    assert!(model::gate(&f.hub.store.db.locked(), &r).is_err(), "off recusa antes da cota");
}

/// A reserva do sono e as autoridades da chamada como o `ask` as monta (o `Prepared` do Space e a seleção da função).
fn reserved(f: &Fixture) -> (sleep::Reservation, crate::fork::background_space_selection::Prepared) {
    assert!(sleep::schedule(&f.hub.store, "bot", later(), "2099-01-01").unwrap());
    let r = sleep::reserve(&f.hub.store, "bot", later()).unwrap().expect("rodada reservada");
    let prepared = crate::fork::background_space_selection::resolve(&f.hub, &r.context).unwrap();
    (r, prepared)
}

fn selection(f: &Fixture) -> crate::fork::background_selection::Resolved {
    crate::fork::background_selection::resolve(&f.hub.store, &f.cfg, FunctionId::MemoryConsolidation).unwrap()
}

/// Gravação canônica do perfil do bot (troca a revisão do vínculo), como a tela de configuração faz.
fn reconfigure(f: &Fixture, shared: bool) {
    let mut cfg = f.hub.store.bot("bot").unwrap().unwrap().config;
    cfg.shared_memory = Some(shared);
    cfg.name = format!("Bot {shared}");
    f.hub.store.save_bot(&cfg).unwrap();
}

/// R1-01: a marca `kept` vale só com as fontes nas revisões conferidas. A mensagem editada depois (a lembrança não
/// muda) derruba a marca, e a lembrança volta a segurar as mensagens até uma releitura válida.
#[test]
fn marca_kept_cai_quando_uma_fonte_muda() {
    let f = fixture(false);
    authorize(&f);
    let (city, _) = conflict(&f);
    let raw = answer(&json!([{"entry":city.id,"verdict":"undecided","text":null,"evidence":[]}]));
    round(&f, &answered(&f, input(&f), raw));
    let c = context(&f, &f.job);
    assert!(model::checked(&f.hub.store.db.locked(), &c, city.id, city.rev).unwrap(), "controle: marcada");
    {
        let db = f.hub.store.db.locked();
        let rev = crate::store::next_rev(&db).unwrap();
        db.execute("UPDATE entries SET rev=?2 WHERE id=?1", params![f.message, rev]).unwrap();
    }
    assert!(!model::checked(&f.hub.store.db.locked(), &c, city.id, city.rev).unwrap(), "fonte mudou: a marca cai");
}

/// R1-07 (achado do executor): no mesmo dia, o atalho literal já marcou a inferência (mesma chave entry/rev/run);
/// o `undecided` válido do modelo ainda precisa ficar gravado, como o recibo diz.
#[test]
fn kept_fica_gravada_mesmo_com_a_marca_do_atalho_na_rodada() {
    let f = fixture(false);
    authorize(&f);
    let fact = inferred(&f, "Prefere café sem açúcar");
    let raw = answer(&json!([{"entry":fact.id,"verdict":"undecided","text":null,"evidence":[]}]));
    let receipt = round(&f, &answered(&f, input(&f), raw));
    assert_eq!(receipt["kept"], json!([{"entry":fact.id,"reason":"undecided"}]));
    assert_eq!(kept(&f), 1, "o recibo e a marca concordam");
}

/// R1-02 = SEG-543-01: com "Sobre você" desligado depois, a lembrança compartilhada não vai ao modelo; a camada
/// própria continua indo. Controle: com o compartilhamento ligado, a mesma candidata vai.
#[test]
fn sobre_voce_desligado_nao_vai_ao_modelo() {
    let f = fixture(false);
    authorize(&f);
    let mut cfg = f.cfg.clone();
    cfg.shared_memory = Some(true);
    f.hub.store.save_bot(&cfg).unwrap();
    let job = second_turn(&f);
    queue::checkpoint(&f.hub.store, "completion-2", r#"{"completion":{"turn":1}}"#, Some(&job), true, false).unwrap();
    let r = queue::reserve_job(&f.hub.store, &job).unwrap().unwrap();
    let shared = Operation::Remember {
        text: "Mora em Curitiba".into(),
        shared: true,
        evidence: vec![f.message.clone()],
        nature: Some(Hint::Inferred),
    };
    let own = Operation::Remember {
        text: "Prefere café sem açúcar".into(),
        shared: false,
        evidence: vec![f.message.clone()],
        nature: Some(Hint::Inferred),
    };
    apply::complete(&f.hub, &r, ExtractionPlan { operations: vec![shared, own] }, |_, _| Ok(())).unwrap();
    let at = |job: &str| {
        let c = context(&f, job);
        serde_json::to_string(&model::input(&f.hub.store.db.locked(), &c).unwrap()).unwrap()
    };
    let json = at(&job);
    assert!(json.contains("Mora em Curitiba") && json.contains("café"), "controle, ligado: {json}");
    reconfigure(&f, false);
    let json = at(&second_turn(&f));
    assert!(!json.contains("Mora em Curitiba"), "desligado: {json}");
    assert!(json.contains("café"), "a camada própria continua: {json}");
}

/// Um turno novo da mesma mensagem M (o `Context` atual, depois de uma gravação do perfil), fechado o anterior.
fn second_turn(f: &Fixture) -> String {
    f.hub.store.memory_turn_finish("bot", "session").unwrap();
    f.hub.store.memory_turn_begin("bot", "bot", "session", &[1], "owner").unwrap();
    f.hub.set_runtime("bot", |r| r.memory_turn_token = Some(uuid::Uuid::new_v4().to_string()));
    queue::capture_engine(&f.hub, &f.cfg, &Lane::main("bot"), &f.cfg.backend).unwrap();
    queue::begin(&f.hub, &f.cfg, &Lane::main("bot"), "session", std::slice::from_ref(&f.message)).unwrap();
    queue::active_job(&f.hub, "bot").unwrap().unwrap()
}

/// R1-03: `text` só em `corrects`. Um `undecided` (ou outro veredito) com texto torna a resposta inteira inválida.
#[test]
fn texto_fora_de_corrects_invalida_a_resposta() {
    let f = fixture(false);
    authorize(&f);
    let fact = inferred(&f, "Prefere café sem açúcar");
    let sent = input(&f);
    for verdict in ["undecided", "supports"] {
        let raw = answer(&json!([{"entry":fact.id,"verdict":verdict,"text":"outro","evidence":[f.message]}]));
        assert!(model::parse(&sent, &raw).is_none(), "{verdict} com texto");
    }
    let raw = answer(&json!([{"entry":fact.id,"verdict":"undecided","text":"outro","evidence":[]}]));
    let receipt = round(&f, &answered(&f, sent, raw));
    assert_eq!((receipt["model"]["status"].as_str(), kept(&f)), (Some("invalidOutput"), 0));
}

/// R1-04 = SEG-543-02: o perfil do bot mudou depois da reserva. Os callbacks ligados ao executor recusam antes de
/// ler ou enviar: o preparo, e (preparado antes) a fronteira de envio.
#[test]
fn context_vencido_recusa_no_preparo_e_no_envio() {
    let f = fixture(false);
    authorize(&f);
    let _ = inferred(&f, "Prefere café sem açúcar");
    let (r, prepared) = reserved(&f);
    let actual = selection(&f);
    assert!(model::prepare(&f.hub, &prepared, &r, &actual).is_ok(), "controle");
    assert!(model::verify(&f.hub, &prepared, &r, &actual).is_ok(), "controle");
    reconfigure(&f, false);
    assert!(model::verify(&f.hub, &prepared, &r, &actual).is_err(), "entre o preparo e o envio");
    assert!(model::prepare(&f.hub, &prepared, &r, &actual).is_err(), "antes do preparo");
}

/// R1-05: o teto de 75% chega como `SubscriptionLimit` ANTES do envio → recibo `limit` (sem reenvio no dia); a mesma
/// recusa durante a chamada continua devolvendo a reserva.
#[test]
fn teto_antes_do_envio_e_limit_durante_a_chamada_adia() {
    let limit = || anyhow::Error::from(crate::fork::background_executor::SubscriptionLimit);
    assert!(matches!(model::failure(&limit(), false), Some(Outcome::Status(Status::Limit))), "teto pré-envio");
    assert!(matches!(model::failure(&limit(), true), Some(Outcome::Deferred)), "durante a chamada");
}

/// R1-06 = SEG-543-03: a resposta da seleção A não grava sob a seleção B, trocada entre a resposta e a gravação.
#[test]
fn selecao_trocada_depois_do_envio_nao_grava_kept() {
    let f = fixture(false);
    authorize(&f);
    let fact = inferred(&f, "Prefere café sem açúcar");
    let raw = answer(&json!([{"entry":fact.id,"verdict":"undecided","text":null,"evidence":[]}]));
    let outcome = answered(&f, input(&f), raw);
    let mut registry: serde_json::Value =
        serde_json::from_str(&f.hub.store.kv_get(crate::fork::engine_accounts::SLOT).unwrap()).unwrap();
    registry["accounts"][0]["models"] = json!([{"id":"haiku","options":[]},{"id":"sonnet","options":[]}]);
    f.hub.store.kv_set(crate::fork::engine_accounts::SLOT, &registry.to_string()).unwrap();
    let binding = crate::fork::modelos_funcao::config::Binding {
        account_id: registry["accounts"][0]["id"].as_str().unwrap().into(),
        model_id: "sonnet".into(),
        model_options: std::collections::BTreeMap::new(),
    };
    let revision = crate::fork::modelos_funcao::config::load(&f.hub.store).unwrap().revision;
    let input = crate::fork::modelos_funcao::config::SelectionInput::Custom { binding };
    crate::fork::modelos_funcao::config::save(
        &f.hub.store,
        revision,
        Some(FunctionId::MemoryConsolidation),
        Some(input),
    )
    .unwrap();
    assert!(sleep::schedule(&f.hub.store, "bot", later(), "2099-02-01").unwrap());
    let r = sleep::reserve(&f.hub.store, "bot", later()).unwrap().unwrap();
    let _ = sleep::run(&f.hub, &r, later(), Some(&outcome));
    assert_eq!(kept(&f), 0, "a resposta de A não vale sob B");
}

/// 2b1d-2: uma inferência do bot com um `supports` válido do modelo, citando M (digitada pelo dono, sessão limpa),
/// numa rodada do sono no modo dado. Devolve a lembrança, o recibo da rodada e as sugestões pendentes.
fn supported(f: &Fixture, mode: &str) -> (mem::Entry, serde_json::Value, Vec<serde_json::Value>) {
    let fact = inferred(f, "Prefere café sem açúcar");
    f.hub.store.kv_set(crate::fork::memory_learning::KEY, mode).unwrap();
    let raw = answer(&json!([{"entry":fact.id,"verdict":"supports","text":null,"evidence":[f.message]}]));
    let receipt = round(f, &answered(f, input(f), raw));
    (fact, receipt, crate::fork::memory_auto::suggestions::list(&f.hub.store, &f.cfg).unwrap())
}

fn nature_of(f: &Fixture, id: i64) -> &'static str {
    codync_mem::nature::name(f.hub.store.memory_read(|c| mem::get(c, id)).unwrap().unwrap().nature)
}

/// (a) `supports` válido vira sugestão "Confirmada pela sua mensagem", em `suggest` e em `auto` (nunca aplica
/// sozinho); a memória não muda e o recibo leva só o id da sugestão.
#[test]
fn supports_valido_vira_sugestao_sem_mudar_a_memoria() {
    for mode in ["suggest", "auto"] {
        let f = fixture(false);
        authorize(&f);
        let (fact, receipt, list) = supported(&f, mode);
        assert_eq!(list.len(), 1, "{mode}: uma sugestão");
        assert_eq!((list[0]["action"].as_str(), list[0]["nature"].as_str()), (Some("reclassify"), Some("confirmed")));
        assert_eq!(receipt["proposed"][0]["suggestion"], list[0]["id"], "{mode}: o recibo leva o id");
        assert_eq!(nature_of(&f, fact.id), "inferred", "{mode}: nada aplicado");
    }
}

/// (a) aprovado pelo dono → natureza `confirmed`, nunca `said`: mesmo texto, recibo e Desfazer de volta à inferência.
/// O topo da memória diz "Confirmada pela mensagem do dono", nunca "Você disse".
#[test]
fn aprovar_supports_da_confirmed_e_desfazer_volta() {
    let f = fixture(false);
    authorize(&f);
    let (fact, _, list) = supported(&f, "suggest");
    let id = list.first().expect("sugestão")["id"].as_str().unwrap().to_owned();
    let local = crate::fork::autoridade::Autoridade::local();
    let out = crate::fork::memory_auto::suggestions::approve(&f.hub, &f.cfg, &id, &local).unwrap();
    assert_eq!(out["status"], "approved");
    assert_eq!(nature_of(&f, fact.id), "confirmed");
    let now = f.hub.store.memory_read(|c| mem::get(c, fact.id)).unwrap().unwrap();
    assert_eq!((now.text.as_str(), now.author == mem::Author::User), (fact.text.as_str(), false));
    let top = f.hub.store.memory_read(|c| codync_mem::opening::load(c, "bot", false)).unwrap().render();
    assert!(top.contains("Confirmada pela mensagem do dono") && !top.contains("Você disse"), "{top}");
    apply::undo(&f.hub, out["receipt"].as_str().unwrap(), &local).unwrap();
    assert_eq!(nature_of(&f, fact.id), "inferred", "Desfazer volta à inferência");
}

/// A lembrança ou uma fonte mudou depois da sugestão: aprovar recusa (`stale`) e nada é aplicado.
#[test]
fn fonte_mudada_depois_da_sugestao_fica_desatualizada() {
    let f = fixture(false);
    authorize(&f);
    let (fact, _, list) = supported(&f, "suggest");
    let id = list.first().expect("sugestão")["id"].as_str().unwrap().to_owned();
    {
        let db = f.hub.store.db.locked();
        let rev = crate::store::next_rev(&db).unwrap();
        db.execute("UPDATE entries SET rev=?2 WHERE id=?1", params![f.message, rev]).unwrap();
    }
    let local = crate::fork::autoridade::Autoridade::local();
    let error = crate::fork::memory_auto::suggestions::approve(&f.hub, &f.cfg, &id, &local).unwrap_err();
    assert!(format!("{error:#}").contains("memory_suggestion_stale"), "{error:#}");
    assert_eq!(nature_of(&f, fact.id), "inferred");
}

/// Authority original (SEG-491-01): o aparelho revogado depois da captura não aprova; nada muda.
#[tokio::test]
async fn aparelho_revogado_nao_aprova_a_confirmacao() {
    let f = fixture(false);
    authorize(&f);
    let (fact, _, list) = supported(&f, "suggest");
    let id = list.first().expect("sugestão")["id"].as_str().unwrap().to_owned();
    let caller = device(&f);
    let autoridade = crate::fork::autoridade::Autoridade::capturar(&f.hub, &caller).unwrap();
    assert!(f.hub.revoke_device(DEVICE).unwrap());
    let body = json!({"botId":"bot","id":id});
    let call = crate::chat::memory::suggestion_call(&f.hub, "approveMemorySuggestion", &body, autoridade).await;
    assert!(call.is_err());
    assert_eq!(nature_of(&f, fact.id), "inferred");
}

/// Guarda: `reclassify` é só do Hub. Vinda do plano do extrator, é ignorada (nada muda) e o trabalho perde a prova.
#[test]
fn reclassificar_vindo_do_extrator_e_ignorado() {
    let f = fixture(false);
    let fact = inferred(&f, "Prefere café sem açúcar");
    let (_, job) = second_job(&f);
    queue::checkpoint(&f.hub.store, "completion-2", r#"{"completion":{"turn":2}}"#, Some(&job), true, false).unwrap();
    let r = queue::reserve_job(&f.hub.store, &job).unwrap().unwrap();
    let op = Operation::Reclassify {
        entry: fact.id,
        expected_rev: fact.rev,
        evidence: vec![f.message.clone()],
        sources: vec![(f.message.clone(), 1)],
    };
    assert_eq!(apply::complete(&f.hub, &r, plan(op), |_, _| Ok(())).unwrap(), Vec::<String>::new());
    assert_eq!(nature_of(&f, fact.id), "inferred");
}

/// Kodus 4504480110: com a sugestão (a) já descartada, a rodada seguinte não cria outra (D389) e o recibo não leva
/// id de sugestão nenhuma.
#[test]
fn sugestao_descartada_nao_volta_nem_entra_no_recibo() {
    let f = fixture(false);
    authorize(&f);
    let (fact, _, list) = supported(&f, "suggest");
    let id = list.first().expect("sugestão")["id"].as_str().unwrap().to_owned();
    let local = crate::fork::autoridade::Autoridade::local();
    crate::fork::memory_auto::suggestions::discard(&f.hub, &f.cfg, &id, &local).unwrap();
    let raw = answer(&json!([{"entry":fact.id,"verdict":"supports","text":null,"evidence":[f.message]}]));
    let receipt = round(&f, &answered(&f, input(&f), raw));
    assert_eq!(receipt["proposed"][0]["verdict"], "supports");
    assert!(receipt["proposed"][0].get("suggestion").is_none(), "nenhuma sugestão nova: {receipt}");
    assert_eq!(crate::fork::memory_auto::suggestions::list(&f.hub.store, &f.cfg).unwrap().len(), 0);
}

/// Kodus 4504479952 / R1-557-01: a mensagem da evidência sumiu antes da aprovação (limpeza C7, por exemplo). A
/// linhagem da sugestão não bate mais (`sources_still`, conferido antes de ler a fonte), então ela se encerra como
/// desatualizada (`stale`) e nada é confirmado.
#[test]
fn evidencia_apagada_antes_da_aprovacao_nao_confirma() {
    let f = fixture(false);
    authorize(&f);
    let (fact, _, list) = supported(&f, "suggest");
    let id = list.first().expect("sugestão")["id"].as_str().unwrap().to_owned();
    f.hub.store.db.locked().execute("DELETE FROM entries WHERE id=?1", [&f.message]).unwrap();
    let local = crate::fork::autoridade::Autoridade::local();
    let error = crate::fork::memory_auto::suggestions::approve(&f.hub, &f.cfg, &id, &local).unwrap_err();
    assert!(format!("{error:#}").contains("memory_suggestion_stale"), "{error:#}");
    let status: String = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT status FROM donna_memory_suggestions WHERE id=?1", [&id], |r| r.get(0))
        .unwrap();
    assert_eq!(status, "stale", "a sugestão se encerra como desatualizada");
    assert_eq!(nature_of(&f, fact.id), "inferred");
}
