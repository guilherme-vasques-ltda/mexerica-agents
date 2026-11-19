//! D70 C6 (casos 1/2/9 do R3): fixture e fronteiras do produtor da escolha na extra do Pessoal. O percurso do ator
//! (definições reais, Ponte, `chatCall` → `ContextTool` → `ShowChoice` → storage e a resposta) fica em
//! `agent/bot/fork_genui_tests.rs`, que precisa do `Actor`. Barreiras sem sleep; nada de pergunta, Authority, binding
//! ou grant semeados por SQL. O único SQL é a remoção NEGATIVA da `SessionRow` numa CÓPIA descartável do banco.
#![allow(clippy::unwrap_used)]
use super::super::admissao_tests::{F, extra, fixture_with};
use crate::{LockExt, hub::Hub, store::Lane};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
};

type Barrier = Box<dyn FnOnce() + Send>;
static AFTER_CAPTURE: LazyLock<Mutex<HashMap<usize, Barrier>>> = LazyLock::new(Mutex::default);

/// Barreira determinística entre a captura do contexto e a entrega ao ator (só o teste que a registrou).
pub(crate) fn after_capture(hub: &Arc<Hub>) {
    let action = AFTER_CAPTURE.locked().remove(&(Arc::as_ptr(hub) as usize));
    if let Some(action) = action {
        action();
    }
}

/// Bot `dev` agente do Pessoal com conta gerenciada (cadastro sintético de `admissao_tests`; nenhuma CLI é lançada).
pub(crate) struct Fixture {
    inner: F,
    pub(crate) hub: Arc<Hub>,
}

impl Fixture {
    pub(crate) fn new(on_disk: bool) -> Self {
        let inner = fixture_with(true, on_disk);
        let hub = inner.hub.clone();
        Self { inner, hub }
    }

    /// Extra aberta pelo produtor canônico (`store::create`).
    pub(crate) fn extra(&self) -> Lane {
        extra(&self.inner)
    }

    /// Outro Hub sobre o mesmo banco em disco (reinício).
    pub(crate) fn reopen(&self) -> Arc<Hub> {
        self.inner.reopen()
    }

    /// Ação única no ponto entre a captura e a entrega ao ator.
    pub(crate) fn after_capture(&self, action: impl FnOnce() + Send + 'static) {
        let previous = AFTER_CAPTURE.locked().insert(Arc::as_ptr(&self.hub) as usize, Box::new(action));
        assert!(previous.is_none());
    }
}

/// Ponte `chat`: o sim do catálogo depende do Hub (sem cache); recusa, `isError` ou Hub fora → sem descritor.
/// Fronteira declarada: o laço stdio e a leitura do env do processo (`mcp::serve`) não rodam aqui.
#[tokio::test]
async fn ponte_so_anuncia_com_sim_do_hub_e_falha_fechada() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let answers = Arc::new(Mutex::new(vec![
        json!({"available": true}),
        json!({"available": false}),
        json!({"error": "tool_binding_mismatch"}),
    ]));
    let replies = answers.clone();
    let router = axum::Router::new().route(
        "/api/{method}",
        axum::routing::post(move |axum::extract::Path(method): axum::extract::Path<String>| {
            let replies = replies.clone();
            async move {
                assert_eq!(method, "chatCall");
                let answer = replies.locked().remove(0);
                let status = if answer.get("error").is_some() {
                    axum::http::StatusCode::BAD_REQUEST
                } else {
                    axum::http::StatusCode::OK
                };
                (status, axum::Json(answer))
            }
        }),
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let descriptor = crate::fork::genui::contract::descriptor();
    let has = |tools: &Value| tools["tools"].as_array().unwrap().iter().any(|t| t["name"] == descriptor["name"]);
    for expected in [true, false, false] {
        let available = crate::mcp::session_catalog(port, "ponte_fixture", "dev").await;
        assert_eq!(available, expected);
        assert_eq!(has(&crate::mcp::session_tools(json!([]), available)), expected);
    }
    assert!(answers.locked().is_empty(), "cada consulta foi ao Hub");
    server.abort();
    let _ = server.await;
    assert!(!crate::mcp::session_catalog(port, "ponte_fixture", "dev").await, "Hub fora: sem descritor");
}

/// Controle Space: a varredura estendida não muda a conversa de Space (pergunta viva continua pendente).
#[test]
fn varredura_preserva_pergunta_viva_de_conversa_de_space() {
    let f = crate::fork::spaces::tool_context::tests::Fixture::new();
    let payload = crate::fork::genui::contract::ValidatedChoice::parse(
        &json!({"version":1,"component":"choice","title":"Escolha","summary":"Resumo",
            "data":{"question":"Qual?","options":[{"id":"a","label":"A"}]}})
        .to_string(),
    )
    .unwrap();
    let shown = crate::fork::genui::flow::show(&f.hub, &f.context(), "ponte:1", &payload).unwrap();
    let id = shown["questionId"].as_str().unwrap().to_owned();
    let mut db = f.hub.store.db.locked();
    let tx = db.transaction().unwrap();
    assert!(crate::fork::genui::flow::sweep_on(&tx).unwrap().is_empty());
    let state: String =
        tx.query_row("SELECT state FROM donna_genui_questions WHERE id=?1", [&id], |r| r.get(0)).unwrap();
    assert_eq!(state, "pending");
}
