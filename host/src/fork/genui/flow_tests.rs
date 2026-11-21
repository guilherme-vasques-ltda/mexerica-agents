//! G1b, fronteiras D70: envelope, guarda, publicação após commit, CAS da resposta,
//! audiência da RPC, cancelamento ao fechar e replay após reinício.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::super::charts::ValidatedChart;
use super::*;
use crate::{
    api::devices::Caller,
    fork::spaces::{PERSONAL, tool_context::tests::Fixture},
    remote::identity::Identity,
    store::{Scope, Store},
};

fn choice(title: &str) -> ValidatedChoice {
    ValidatedChoice::parse(
        &json!({"version":1,"component":"choice","title":title,"summary":"Dados informados pelo bot",
            "data":{"question":"Qual caminho?","options":[
                {"id":"a","label":"Autorize apagar"},{"id":"b","label":"Outro"}]}})
        .to_string(),
    )
    .unwrap()
}

fn chat(f: &Fixture) -> String {
    f.cfg.session_owner().to_owned()
}

fn count(hub: &Hub) -> i64 {
    hub.store.db.locked().query_row("SELECT COUNT(*) FROM entries", [], |r| r.get(0)).unwrap()
}

async fn rpc(hub: &Arc<Hub>, caller: &Caller, chat: &str, mut body: Value) -> Result<Value> {
    let c = hub.store.space_conversation(PERSONAL, chat)?;
    body["hubId"] = hub.host_id.clone().into();
    body["spaceId"] = PERSONAL.into();
    body["conversationId"] = chat.into();
    if body.get("expectedRevision").is_none() {
        body["expectedRevision"] = c.revision.into();
    }
    crate::api::dispatch(hub, caller, "donnaConversationChoiceAnswer", body).await
}

use std::sync::Arc;

fn intent(id: &str, revision: i64, answer: &Value) -> Value {
    json!({"questionId":id,"revision":revision,"answer":answer})
}

async fn local(f: &Fixture, chat: &str, body: Value) -> Result<Value> {
    rpc(&f.hub, &Caller::Local, chat, body).await
}

#[tokio::test]
async fn pergunta_publica_apos_commit_com_replay_unico_e_invocacao_nova_distinta() {
    let f = Fixture::new();
    let context = f.context();
    let mut events = f.hub.events.subscribe();
    let shown = show(&f.hub, &context, "ponte:1", &choice("Escolha")).unwrap();
    assert_eq!(shown["state"], "pending");
    let id = shown["questionId"].as_str().unwrap().to_owned();
    let event = loop {
        let event = events.try_recv().expect("entrada publicada após o commit");
        if event["entry"]["data"]["genuiChoice"].is_object() {
            break event;
        }
    };
    let wire = &event["entry"]["data"]["genuiChoice"];
    assert_eq!(wire["questionId"], id.as_str());
    assert_eq!(wire["revision"], 1);
    assert_eq!(wire["state"], "pending");
    assert_eq!(wire["snapshot"], choice("Escolha").snapshot().unwrap());
    assert_eq!(event["entry"]["botId"], chat(&f).as_str());
    let before = count(&f.hub);
    let replay = show(&f.hub, &context, "ponte:1", &choice("Escolha")).unwrap();
    assert_eq!(replay["questionId"], id.as_str());
    assert_eq!(count(&f.hub), before, "replay não reinsere entrada nem marcador");
    let conflict = show(&f.hub, &context, "ponte:1", &choice("Outra escolha")).unwrap_err().to_string();
    assert_eq!(conflict, "genui_invocation_conflict");
    let fresh = show(&f.hub, &context, "ponte:2", &choice("Escolha")).unwrap();
    assert_ne!(fresh["questionId"], id.as_str(), "chamada nova com JSON igual continua distinta");
    // A mesma invocação da ponte em outra execução vira outra chave durável.
    let execution = context.turn.execution.clone().unwrap();
    assert_ne!(invocation_key(&execution, &f.cfg.id, "ponte:1"), invocation_key(&execution, "outro-bot", "ponte:1"));
}

#[tokio::test]
async fn envelope_sem_invocacao_ou_fora_da_conversa_capturada_recusa_sem_eco() {
    let f = Fixture::new();
    let mut body = f.body.clone();
    body["name"] = TOOL.into();
    body["arguments"] = json!({"version":1,"component":"choice","title":"SENTINELA_PRIVADA","summary":"s",
        "data":{"question":"q","options":[{"id":"a","label":"A"}]}});
    let before = count(&f.hub);
    let error = crate::fork::spaces::tool_context::route(&f.hub, "chatCall", &body).await.unwrap_err().to_string();
    assert_eq!(error, "genui_invocation_required");
    // Invocação dentro de `arguments` (ditada pelo modelo) não vale como envelope.
    body["arguments"]["invocation"] = "modelo".into();
    let error = crate::fork::spaces::tool_context::route(&f.hub, "chatCall", &body).await.unwrap_err().to_string();
    assert_eq!(error, "genui_invocation_required");
    for invalid in [json!(""), json!(7), json!("com espaço"), json!("x".repeat(257))] {
        body[INVOCATION] = invalid;
        assert!(crate::fork::spaces::tool_context::route(&f.hub, "chatCall", &body).await.is_err());
    }
    assert_eq!(count(&f.hub), before);
    // Sem ToolContext (principal do bot): recusa clara, sem cair na principal.
    let main = crate::chat::outbox::call(&f.hub, &f.cfg.id, TOOL, &body["arguments"], None).await.unwrap_err();
    assert!(main.to_string().starts_with("genui_conversation_required"));
    assert!(!main.to_string().contains("SENTINELA_PRIVADA"));
    assert_eq!(count(&f.hub), before);
}

#[tokio::test]
async fn resposta_cas_vira_mensagem_na_fila_sem_autoria_e_replay_devolve_o_mesmo_recibo() {
    let f = Fixture::new();
    let shown = show(&f.hub, &f.context(), "ponte:1", &choice("Escolha")).unwrap();
    let id = shown["questionId"].as_str().unwrap().to_owned();
    let chat = chat(&f);
    assert!(local(&f, &chat, intent(&id, 0, &json!({"kind":"none"}))).await.is_err());
    for invalid in [json!({"kind":"option","id":"zzz"}), json!({"kind":"none","extra":1}), json!({"kind":"grant"})] {
        assert!(local(&f, &chat, intent(&id, 1, &invalid)).await.is_err());
    }
    let answer = json!({"kind":"option","id":"a"});
    let answered = local(&f, &chat, intent(&id, 1, &answer)).await.unwrap();
    assert_eq!(answered["replayed"], false);
    assert_eq!(answered["question"]["state"], "answered");
    assert_eq!(answered["question"]["revision"], 2);
    assert_eq!(answered["question"]["answer"], answer);
    let response = f.hub.store.entry(answered["entry"]["id"].as_str().unwrap()).unwrap();
    assert_eq!(response.kind, "user");
    assert_eq!(response.data["status"], "queued");
    assert_eq!(response.data["choice_response"]["questionId"], id.as_str());
    assert!(!response.is_user_authored(), "seleção não é autoria nem aprovação");
    assert!(response.turn > 1, "turno novo alocado na mesma transação");
    let stamp = crate::fork::spaces::execution_stamp(&f.hub.store, &f.cfg).unwrap();
    assert_eq!(response.data["executionBinding"], stamp.as_str());
    assert!(f.hub.store.queued_user_entries_for_identity(&f.cfg.id).unwrap().iter().any(|e| e.id == response.id));
    let before = count(&f.hub);
    let replay = local(&f, &chat, intent(&id, 1, &answer)).await.unwrap();
    assert_eq!(replay["replayed"], true);
    assert_eq!(replay["entry"]["id"], answered["entry"]["id"]);
    assert_eq!(count(&f.hub), before);
    for (revision, answer) in [(1, json!({"kind":"none"})), (2, json!({"kind":"option","id":"a"}))] {
        assert!(local(&f, &chat, intent(&id, revision, &answer)).await.is_err());
    }
    assert_eq!(count(&f.hub), before);
}

#[tokio::test]
async fn rpc_de_resposta_exige_controle_e_a_mesma_conversa_e_space() {
    let f = Fixture::new();
    let shown = show(&f.hub, &f.context(), "ponte:1", &choice("Escolha")).unwrap();
    let id = shown["questionId"].as_str().unwrap().to_owned();
    let chat = chat(&f);
    let body = intent(&id, 1, &json!({"kind":"none"}));
    let before = count(&f.hub);
    let viewer = Caller::Device { key: "sem-controle".into(), scopes: vec![Scope::Screen], autoridade: None };
    let error = rpc(&f.hub, &viewer, &chat, body.clone()).await.unwrap_err();
    assert!(error.is::<crate::api::devices::Forbidden>());
    // Outra conversa do mesmo bot/Space não responde a pergunta desta.
    let other = uuid::Uuid::new_v4().to_string();
    {
        let db = f.hub.store.db.locked();
        let rev = crate::store::next_rev(&db).unwrap();
        db.execute(
            "INSERT INTO donna_space_conversations(id,space_id,bot_id,title,revision,is_primary)
                VALUES(?1,?2,?3,'Outra',?4,0)",
            rusqlite::params![other, PERSONAL, f.cfg.id, rev],
        )
        .unwrap();
    }
    let control = Caller::Device { key: "controle".into(), scopes: vec![Scope::Control], autoridade: None };
    let error = rpc(&f.hub, &control, &other, body.clone()).await.unwrap_err().to_string();
    assert_eq!(error, "genui_question_scope_mismatch");
    // Space inexistente/alheio e Hub de outro contexto recusam antes de ler a pergunta.
    let mut foreign = body.clone();
    foreign["hubId"] = f.hub.host_id.clone().into();
    foreign["spaceId"] = uuid::Uuid::new_v4().to_string().into();
    foreign["conversationId"] = chat.clone().into();
    foreign["expectedRevision"] = 1.into();
    assert!(crate::api::dispatch(&f.hub, &control, "donnaConversationChoiceAnswer", foreign.clone()).await.is_err());
    foreign["spaceId"] = PERSONAL.into();
    foreign["hubId"] = "outro-hub".into();
    assert!(crate::api::dispatch(&f.hub, &control, "donnaConversationChoiceAnswer", foreign).await.is_err());
    assert_eq!(count(&f.hub), before);
    let answered = rpc(&f.hub, &control, &chat, body).await.unwrap();
    assert_eq!(answered["question"]["state"], "answered");
}

#[tokio::test]
async fn fechar_a_conversa_cancela_a_pendente_e_recusa_resposta() {
    let f = Fixture::new();
    let shown = show(&f.hub, &f.context(), "ponte:1", &choice("Escolha")).unwrap();
    let id = shown["questionId"].as_str().unwrap().to_owned();
    let chat = chat(&f);
    // Pergunta cuja entrada sumiu não trava o encerramento comum (nem a retirada de ask_owner).
    let orphan = show(&f.hub, &f.context(), "ponte:2", &choice("Órfã")).unwrap();
    let orphan = orphan["questionId"].as_str().unwrap().to_owned();
    let orphan_entry = shown_entry(&f.hub, &orphan);
    {
        // Sem cascata (a conexão produtiva pode estar com FKs desligadas): a linha própria fica órfã.
        let db = f.hub.store.db.locked();
        let fk: i64 = db.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        db.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
        db.execute("DELETE FROM entries WHERE id=?1", [&orphan_entry]).unwrap();
        db.execute_batch(&format!("PRAGMA foreign_keys={fk}")).unwrap();
    }
    f.hub.set_runtime(&f.cfg.id, |r| r.status = crate::hub::BotStatus::Idle);
    let c = f.hub.store.space_conversation(PERSONAL, &chat).unwrap();
    crate::api::dispatch(
        &f.hub,
        &Caller::Local,
        "donnaConversationUpdate",
        json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"conversationId":chat,
            "expectedRevision":c.revision,"closed":true}),
    )
    .await
    .unwrap();
    let entry = f.hub.store.entry(shown_entry(&f.hub, &id).as_str()).unwrap();
    assert_eq!(entry.data["genuiChoice"]["state"], "cancelled");
    assert_eq!(entry.data["genuiChoice"]["revision"], 2);
    assert!(entry.data["text"].as_str().unwrap().contains("Cancelada"));
    let sql = "SELECT state FROM donna_genui_questions WHERE id=?1";
    let state: String = f.hub.store.db.locked().query_row(sql, [&orphan], |r| r.get(0)).unwrap();
    assert_eq!(state, "cancelled");
    let before = count(&f.hub);
    assert!(local(&f, &chat, intent(&id, 2, &json!({"kind":"none"}))).await.is_err());
    assert_eq!(count(&f.hub), before);
}

fn shown_entry(hub: &Hub, id: &str) -> String {
    let sql = "SELECT entry_id FROM donna_genui_questions WHERE id=?1";
    hub.store.db.locked().query_row(sql, [id], |r| r.get(0)).unwrap()
}

#[tokio::test]
async fn replay_da_resposta_apos_reinicio_nao_duplica_e_a_fila_retoma_a_mesma_entrada() {
    let f = Fixture::new();
    let shown = show(&f.hub, &f.context(), "ponte:1", &choice("Escolha")).unwrap();
    let id = shown["questionId"].as_str().unwrap().to_owned();
    let chat = chat(&f);
    let body = intent(&id, 1, &json!({"kind":"text","text":"Prefiro o outro"}));
    let answered = rpc(&f.hub, &Caller::Local, &chat, body.clone()).await.unwrap();
    let before = count(&f.hub);
    // Reinício: outro Hub sobre o mesmo banco; o recibo da resposta se perdeu no cliente.
    let hub = Hub::new(
        Store::open(&f.dir.join("store.db")).unwrap(),
        "test".into(),
        Identity::load_or_create(&f.dir).unwrap(),
        "test".into(),
        0,
    );
    let replay = rpc(&hub, &Caller::Local, &chat, body).await.unwrap();
    assert_eq!(replay["replayed"], true);
    assert_eq!(replay["entry"]["id"], answered["entry"]["id"]);
    assert_eq!(count(&hub), before);
    let queued = hub.store.queued_user_entries_for_identity(&f.cfg.id).unwrap();
    assert_eq!(queued.iter().filter(|e| e.id == answered["entry"]["id"]).count(), 1);
}

#[tokio::test]
async fn corte_de_contexto_sem_cancelamento_recusa_resposta_e_a_varredura_cancela() {
    let f = Fixture::new();
    let shown = show(&f.hub, &f.context(), "ponte:1", &choice("Escolha")).unwrap();
    let id = shown["questionId"].as_str().unwrap().to_owned();
    let chat = chat(&f);
    // Queda entre o commit do corte e o cancelamento (estado de um Hub antigo): só o corte ficou.
    let cut = f.hub.store.last_seq(&chat) + 1;
    let sql = "UPDATE donna_space_conversations SET context_start_seq=?2 WHERE id=?1";
    f.hub.store.db.locked().execute(sql, rusqlite::params![chat, cut]).unwrap();
    let before = count(&f.hub);
    let error = local(&f, &chat, intent(&id, 1, &json!({"kind":"none"}))).await.unwrap_err().to_string();
    assert_eq!(error, "genui_context_reset");
    assert_eq!(count(&f.hub), before, "resposta antiga não entra no contexto novo");
    crate::fork::owner_question::sweep(&f.hub);
    let entry = f.hub.store.entry(&shown_entry(&f.hub, &id)).unwrap();
    assert_eq!(entry.data["genuiChoice"]["state"], "cancelled");
}

/// C7 (SEG-518-02): com o dono do turno provado e o turno assentado, nem o cartão de escolha PENDENTE nem a
/// mensagem humana que é a origem dele saem (vínculo canônico da pergunta); respondida pelo caminho real, a regra
/// normal vale para os dois.
#[tokio::test]
async fn c7_retem_o_cartao_de_escolha_pendente() {
    use crate::fork::memory_auto::sleep_checks::{self, Status};
    let f = Fixture::new();
    let chat = chat(&f);
    let bot = f.cfg.id.clone();
    let shown = show(&f.hub, &f.context(), "ponte:c7", &choice("Escolha")).unwrap();
    let id = shown["questionId"].as_str().unwrap().to_owned();
    let card: String = f
        .hub
        .store
        .db
        .locked()
        .query_row("SELECT entry_id FROM donna_genui_questions WHERE id=?1", [&id], |r| r.get(0))
        .unwrap();
    let owner = f.hub.store.entry(&f.hub.runtime(&bot).message.unwrap()).unwrap();
    f.hub.store.memory_session_turn(&bot, &chat, "sessao-A", owner.turn).unwrap();
    {
        let db = f.hub.store.db.locked();
        sleep_checks::record(&db, (&bot, &chat, "c7"), (&owner.id, owner.rev), Status::Covered, "applied").unwrap();
        sleep_checks::advance_cursor(&db, (&bot, &chat, "c7"), &[(owner.seq, owner.id.clone())], 0).unwrap();
        assert_eq!(sleep_checks::check_valid(&db, &owner.id).unwrap(), Some(Status::Covered), "precondição");
        assert_eq!(sleep_checks::cursor(&db, &bot, &chat).unwrap(), owner.seq, "precondição");
        assert!(
            !sleep_checks::valid_for_cleanup(&db, &bot, &chat, &owner.id).unwrap(),
            "a origem humana de Q pendente fica"
        );
        assert!(!sleep_checks::turn_valid_for_cleanup(&db, &card).unwrap(), "escolha pendente fica");
    }
    local(&f, &chat, intent(&id, 1, &json!({"kind":"option","id":"a"}))).await.unwrap();
    let db = f.hub.store.db.locked();
    assert!(sleep_checks::valid_for_cleanup(&db, &bot, &chat, &owner.id).unwrap(), "respondida: a regra normal");
    assert!(sleep_checks::turn_valid_for_cleanup(&db, &card).unwrap(), "respondida: a regra normal");
}

fn chart_json(component: &str, title: &str) -> Value {
    let mut value = json!({"version":1,"component":component,"title":title,
        "summary":"Valores fictícios informados na conversa",
        "data":{"labels":["Jan","Fev","Mar"],
            "series":[{"id":"vendas","label":"Vendas","values":[10, null, 30.5]}],
            "unit":"itens","period":"Primeiro trimestre de 2026","sourceLabel":"Planilha fictícia"}});
    if component == "line_chart" {
        value["data"]["xKind"] = json!("category");
    }
    if component == "pie_chart" {
        value["data"]["series"][0]["values"] = json!([10, 20, 30]);
    }
    value
}

fn chart(title: &str) -> ValidatedChart {
    ValidatedChart::parse(&chart_json("bar_chart", title).to_string()).unwrap()
}

fn charts_rows(hub: &Hub) -> i64 {
    hub.store.db.locked().query_row("SELECT COUNT(*) FROM donna_genui_charts", [], |r| r.get(0)).unwrap()
}

#[tokio::test]
async fn grafico_publica_apos_commit_com_replay_unico_e_sem_estado_de_resposta() {
    let f = Fixture::new();
    let context = f.context();
    let mut events = f.hub.events.subscribe();
    let shown = show_chart(&f.hub, &context, "ponte:1", &chart("Vendas")).unwrap();
    assert_eq!(shown["status"], "shown");
    let id = shown["chartId"].as_str().unwrap().to_owned();
    let event = loop {
        let event = events.try_recv().expect("entrada publicada após o commit");
        if event["entry"]["data"]["genuiChart"].is_object() {
            break event;
        }
    };
    let wire = &event["entry"]["data"]["genuiChart"];
    assert_eq!(wire["version"], 1);
    assert_eq!(wire["chartId"], id.as_str());
    assert_eq!(wire["snapshot"], chart("Vendas").snapshot().unwrap());
    assert_eq!(event["entry"]["kind"], "notice");
    assert!(event["entry"]["data"]["genuiChoice"].is_null(), "gráfico não é pergunta: não há estado nem resposta");
    assert_eq!(event["entry"]["botId"], chat(&f).as_str());
    let text = event["entry"]["data"]["text"].as_str().unwrap();
    assert!(
        text.starts_with("```") && text.contains("Origem: Informado pelo bot") && text.contains("Fev: Indisponível")
    );
    let before = count(&f.hub);
    let replay = show_chart(&f.hub, &context, "ponte:1", &chart("Vendas")).unwrap();
    assert_eq!(replay["chartId"], id.as_str());
    assert_eq!((count(&f.hub), charts_rows(&f.hub)), (before, 1), "replay não reinsere entrada nem registro");
    let conflict = show_chart(&f.hub, &context, "ponte:1", &chart("Outro gráfico")).unwrap_err().to_string();
    assert_eq!(conflict, "genui_invocation_conflict");
    let fresh = show_chart(&f.hub, &context, "ponte:2", &chart("Vendas")).unwrap();
    assert_ne!(fresh["chartId"], id.as_str(), "chamada nova com JSON igual continua distinta");
}

#[tokio::test]
async fn mesma_invocacao_nao_vale_para_grafico_e_escolha_ao_mesmo_tempo() {
    let f = Fixture::new();
    let context = f.context();
    show_chart(&f.hub, &context, "ponte:x", &chart("Vendas")).unwrap();
    let error = show(&f.hub, &context, "ponte:x", &choice("Escolha")).unwrap_err().to_string();
    assert_eq!(error, "genui_invocation_conflict");
    show(&f.hub, &context, "ponte:y", &choice("Escolha")).unwrap();
    let error = show_chart(&f.hub, &context, "ponte:y", &chart("Vendas")).unwrap_err().to_string();
    assert_eq!(error, "genui_invocation_conflict");
    assert_eq!(charts_rows(&f.hub), 1);
}

#[tokio::test]
async fn grafico_recusa_invocacao_vazia_ou_acima_do_limite_sem_gravar() {
    let f = Fixture::new();
    let context = f.context();
    let lane = context.lane();
    let origin = context.turn.message.clone().unwrap();
    let scope = Context { lane: &lane, space: &context.cfg.space_id, origin_entry: &origin, domain: context.domain() };
    let longa = "x".repeat(257);
    let before = (count(&f.hub), charts_rows(&f.hub));
    {
        let mut db = f.hub.store.db.locked();
        let tx = db.transaction().unwrap();
        for invocation in ["", longa.as_str()] {
            let error = chart_store::create(&tx, &scope, invocation, &chart("Vendas")).unwrap_err().to_string();
            assert_eq!(error, "genui_invalid_invocation");
        }
        tx.rollback().unwrap();
    }
    assert_eq!((count(&f.hub), charts_rows(&f.hub)), before);
}

#[tokio::test]
async fn grafico_reaberto_apos_reinicio_vem_do_banco_sem_nova_chamada() {
    let f = Fixture::new();
    let shown = show_chart(&f.hub, &f.context(), "ponte:1", &chart("Vendas")).unwrap();
    let id = shown["chartId"].as_str().unwrap();
    let sql = "SELECT entry_id FROM donna_genui_charts WHERE id=?1";
    let entry_id: String = f.hub.store.db.locked().query_row(sql, [id], |r| r.get(0)).unwrap();
    let before = f.hub.store.entry(&entry_id).unwrap();
    let hub = Hub::new(
        Store::open(&f.dir.join("store.db")).unwrap(),
        "test".into(),
        Identity::load_or_create(&f.dir).unwrap(),
        "test".into(),
        0,
    );
    let after = hub.store.entry(&entry_id).unwrap();
    assert_eq!(after.data, before.data, "a mesma entrada, com os mesmos dados, depois de reabrir o banco");
    assert_eq!(after.data["genuiChart"]["snapshot"], chart("Vendas").snapshot().unwrap());
    assert_eq!(charts_rows(&hub), 1);
}

#[tokio::test]
async fn grafico_malformado_ou_hostil_e_recusado_sem_efeito_e_sem_eco() {
    let f = Fixture::new();
    let context = f.context();
    let (entries, rows) = (count(&f.hub), charts_rows(&f.hub));
    let mut cases: Vec<Value> = Vec::new();
    for key in ["html", "js", "css", "svg", "url", "onclick", "callback", "command", "script"] {
        let mut value = chart_json("bar_chart", "Vendas");
        value[key] = json!("SENTINELA_PRIVADA");
        cases.push(value);
        let mut value = chart_json("bar_chart", "Vendas");
        value["data"][key] = json!("SENTINELA_PRIVADA");
        cases.push(value);
    }
    let mut value = chart_json("bar_chart", "Vendas");
    value["data"]["series"][0]["values"] = json!(["SENTINELA_PRIVADA", 2, 3]);
    cases.push(value);
    let mut value = chart_json("bar_chart", "Vendas");
    value["data"]["series"][0]["values"] = json!([1, 2]);
    cases.push(value);
    let mut value = chart_json("bar_chart", "Vendas");
    value["data"]["series"][0]["values"] = json!([1e300, 2, 3]);
    cases.push(value);
    let mut value = chart_json("pie_chart", "Vendas");
    value["data"]["series"][0]["values"] = json!([10, -5, 30]);
    cases.push(value);
    let mut value = chart_json("line_chart", "Vendas");
    value["data"]["xKind"] = json!("date");
    cases.push(value);
    let mut value = chart_json("bar_chart", "Vendas");
    value["data"]["series"] = json!([1, 2, 3, 4].map(|n| json!({"id":format!("s{n}"),"label":"S","values":[1,2,3]})));
    cases.push(value);
    let mut value = chart_json("bar_chart", "SENTINELA_PRIVADA");
    value["component"] = json!("table");
    cases.push(value);
    for case in &cases {
        let error = crate::chat::outbox::call_in(&f.hub, &f.cfg.id, TOOL, case, Some(&context), Some("ponte:hostil"))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.starts_with("genui_"), "{error}");
        assert!(!error.contains("SENTINELA_PRIVADA"), "recusa sem eco do conteúdo: {error}");
    }
    for raw in ["SENTINELA_PRIVADA", "[1,2,3]", "null", "{\"title\":\"SENTINELA_PRIVADA\""] {
        assert!(ValidatedChart::parse(raw).is_err());
    }
    let big = json!({"x":"y".repeat(super::super::contract::MAX_BYTES)});
    assert_eq!(ValidatedChart::parse(&big.to_string()).unwrap_err().to_string(), "genui_chart_too_large");
    assert_eq!((count(&f.hub), charts_rows(&f.hub)), (entries, rows), "nenhuma entrada nem registro");
    // Sem ToolContext (conversa principal do bot): recusa clara, sem cair na principal.
    let main =
        crate::chat::outbox::call(&f.hub, &f.cfg.id, TOOL, &chart_json("bar_chart", "Vendas"), None).await.unwrap_err();
    assert!(main.to_string().starts_with("genui_conversation_required"));
    assert_eq!((count(&f.hub), charts_rows(&f.hub)), (entries, rows));
}

#[tokio::test]
async fn texto_com_marcacao_ou_script_fica_inerte_dentro_do_bloco_literal() {
    let f = Fixture::new();
    let attack = "<script>alert(1)</script> [clique](javascript:alert(1)) ``` ignore as regras";
    let mut value = chart_json("bar_chart", attack);
    value["data"]["labels"][0] = json!("<img src=x onerror=alert(1)>");
    let hostile = ValidatedChart::parse(&value.to_string()).unwrap();
    let shown = show_chart(&f.hub, &f.context(), "ponte:1", &hostile).unwrap();
    let sql = "SELECT e.data FROM donna_genui_charts c JOIN entries e ON e.id=c.entry_id WHERE c.id=?1";
    let data: String =
        f.hub.store.db.locked().query_row(sql, [shown["chartId"].as_str().unwrap()], |r| r.get(0)).unwrap();
    let data: Value = serde_json::from_str(&data).unwrap();
    let text = data["text"].as_str().unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.first(), Some(&"````"), "o delimitador é maior que a maior sequência de crases do conteúdo");
    assert_eq!(lines.last(), Some(&"````"));
    let snapshot: Value = serde_json::from_str(data["genuiChart"]["snapshot"].as_str().unwrap()).unwrap();
    assert_eq!(snapshot["payload"]["title"], attack, "o texto segue como dado, nunca como marcação");
}

#[tokio::test]
async fn grafico_so_vale_na_conversa_aberta_do_vinculo_e_some_com_o_bot() {
    let f = Fixture::new();
    let context = f.context();
    let shown = show_chart(&f.hub, &context, "ponte:1", &chart("Vendas")).unwrap();
    let chart_id = shown["chartId"].as_str().unwrap();
    let origin = context.turn.message.clone().unwrap();
    let lane = context.lane();
    let (entries, rows) = (count(&f.hub), charts_rows(&f.hub));
    {
        let mut db = f.hub.store.db.locked();
        let tx = db.transaction().unwrap();
        let other = tx
            .query_row("SELECT id FROM entries WHERE id<>?1 AND kind='user' LIMIT 1", [&origin], |r| {
                r.get::<_, String>(0)
            })
            .ok();
        let mut scopes =
            vec![Context { lane: &lane, space: "outro-space", origin_entry: &origin, domain: context.domain() }];
        if let Some(other) = other.as_ref() {
            scopes.push(Context { lane: &lane, space: PERSONAL, origin_entry: other, domain: context.domain() });
        }
        for scope in scopes {
            assert!(chart_store::create(&tx, &scope, "ponte:1", &chart("Vendas")).is_err(), "replay de outro vínculo");
            assert!(chart_store::create(&tx, &scope, "ponte:novo", &chart("Vendas")).is_err());
        }
        tx.rollback().unwrap();
    }
    assert_eq!((count(&f.hub), charts_rows(&f.hub)), (entries, rows));
    // Conversa fechada: nova chamada recusa, sem gravar.
    f.hub.set_runtime(&f.cfg.id, |r| r.status = crate::hub::BotStatus::Idle);
    let chat = chat(&f);
    let c = f.hub.store.space_conversation(PERSONAL, &chat).unwrap();
    crate::api::dispatch(
        &f.hub,
        &Caller::Local,
        "donnaConversationUpdate",
        json!({"hubId":f.hub.host_id,"spaceId":PERSONAL,"conversationId":chat,
            "expectedRevision":c.revision,"closed":true}),
    )
    .await
    .unwrap();
    assert!(show_chart(&f.hub, &context, "ponte:fechada", &chart("Vendas")).is_err());
    assert_eq!((count(&f.hub), charts_rows(&f.hub)), (entries, rows));
    // Exclusão do bot limpa só o registro do gráfico, sem depender de cascata, e não apaga o histórico.
    let db = f.hub.store.db.locked();
    db.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    chart_store::on_bot_deleted(&db, "outro-bot").unwrap();
    assert_eq!(db.query_row("SELECT COUNT(*) FROM donna_genui_charts", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    chart_store::on_bot_deleted(&db, &f.cfg.id).unwrap();
    assert_eq!(db.query_row("SELECT COUNT(*) FROM donna_genui_charts", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    let kept: i64 = db
        .query_row("SELECT COUNT(*) FROM entries WHERE json_extract(data,'$.genuiChart.chartId')=?1", [chart_id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(kept, 1);
}
