//! Prova do cartão de ferramenta genérica com a projeção da ponte: sem campo de comando, com o resumo
//! dentro do limite da tela e, quando só existe a opção de negar, guardando só ela.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use crate::fork::spaces::tool_context::tests::Fixture;

/// Resumo no formato que a ponte manda para uma ferramenta genérica.
fn projecao_exemplo() -> String {
    let sha = "ab".repeat(32);
    format!(
        "Servidor: loja\nFerramenta: apagar_pedido\nArgumentos (SHA-256 dos argumentos inteiros: {sha}):\n{{\n  \"acao\": \"apagar\"\n}}"
    )
}

/// Ator real com ACP de fixture atendendo a sessão principal da conversa.
fn ator(f: &Fixture) -> Actor {
    let (acp, rx) = Acp::spawn("cat", f.dir.to_str().unwrap(), &[]).unwrap();
    let mut actor = new_actor(f.hub.clone(), f.cfg.clone());
    actor.lane = Lane::main(f.cfg.session_owner());
    actor.turn = Some(1);
    actor.turn_session = Some("sessao-A".into());
    actor.conn = Some(Conn {
        ponte: None,
        plano: None,
        curated: crate::fork::conectores::Pool::default(),
        acp,
        rx,
        skill_reads: tokio::task::JoinSet::new(),
        sessions: SessionCaps { load: true, close: false, system_prompt: false },
        adapter: Adapter::Other,
        adapter_version: None,
        codex_config: false,
        config_warnings: vec![],
        loaded: HashMap::new(),
        started: Instant::now(),
        fast_options: HashMap::new(),
        reports_compaction: false,
        compaction_usage: CompactionUsage::Idle,
        usage: HashMap::new(),
    });
    actor
}

async fn cartao(f: &Fixture, params: Value) -> Entry {
    let mut actor = ator(f);
    actor.on_permission(json!("rpc-1"), params).await;
    f.hub
        .store
        .history(f.cfg.session_owner(), i64::MAX, 20)
        .unwrap()
        .into_iter()
        .find(|e| e.kind == "permission")
        .expect("cartão de permissão gravado")
}

#[tokio::test]
async fn cartao_de_projecao_vai_sem_command_e_com_o_resumo_dentro_do_limite() {
    let f = Fixture::new();
    let texto = projecao_exemplo();
    let params = json!({
        "sessionId": "sessao-A",
        "toolCall": {
            "toolCallId": "toolu_1", "status": "pending", "title": "mcp__loja__apagar_pedido", "kind": "other",
            "rawInput": {"servidor": "loja", "ferramenta": "apagar_pedido", "resumo": texto,
                "completo": true, "sha256": "ab".repeat(32)},
            "content": [{"type": "content", "content": {"type": "text", "text": texto}}],
            "_meta": {"claudeCode": {"toolName": "mcp__loja__apagar_pedido"}}
        },
        "options": [
            {"optionId": "allow-once", "name": "Yes", "kind": "allow_once"},
            {"optionId": "reject", "name": "No", "kind": "reject_once"}
        ]
    });
    let card = cartao(&f, params).await;
    assert_eq!(card.data["status"], "pending", "cartão esperando a decisão: {}", card.data);
    assert!(card.data["command"].is_null(), "sem campo de comando no cartão: {}", card.data);
    let detalhe = card.data["detail"].as_str().unwrap();
    assert_eq!(detalhe, texto, "o detalhe vem do conteúdo projetado");
    assert!(detalhe.chars().count() <= 2000, "cabe na tela sem corte: {}", detalhe.chars().count());
}

#[tokio::test]
async fn cartao_com_opcao_so_de_recusar_guarda_somente_a_recusa() {
    let f = Fixture::new();
    let texto = projecao_exemplo();
    let params = json!({
        "sessionId": "sessao-A",
        "toolCall": {
            "toolCallId": "toolu_1", "status": "pending", "title": "mcp__loja__apagar_pedido", "kind": "other",
            "rawInput": {"servidor": "loja", "ferramenta": "apagar_pedido", "resumo": texto,
                "completo": false, "sha256": "ab".repeat(32)},
            "content": [{"type": "content", "content": {"type": "text", "text": texto}}],
            "_meta": {"claudeCode": {"toolName": "mcp__loja__apagar_pedido"}}
        },
        "options": [{"optionId": "reject", "name": "No", "kind": "reject_once"}]
    });
    let card = cartao(&f, params).await;
    assert_eq!(
        card.data["options"],
        json!([{"optionId": "reject", "name": "No", "kind": "reject_once"}]),
        "sem aprovação possível, o cartão guarda só negar: {}",
        card.data
    );
}
