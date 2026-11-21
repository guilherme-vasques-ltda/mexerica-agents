//! O código de sessão de um plano de código não chega ao histórico nem ao app, nem partido em vários pedaços de texto.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::*;
use crate::fork::spaces::tool_context::tests::Fixture;

fn codigo() -> String {
    format!("mxs_{}", "0f3a9c1d".repeat(8))
}

fn texto_da_entrada(f: &Fixture, id: &str) -> String {
    f.hub.store.entry(id).unwrap().data["text"].as_str().unwrap_or_default().to_owned()
}

#[tokio::test]
async fn codigo_partido_entre_pedacos_do_texto_nunca_e_publicado_nem_pela_metade() {
    let f = Fixture::new();
    let mut ator = new_actor(f.hub.clone(), f.cfg.clone());
    ator.lane = Lane::main(f.cfg.session_owner());
    ator.turn = Some(1);
    let codigo = codigo();
    let (primeira, segunda) = codigo.split_at(30);
    let pedaco =
        |texto: &str| json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": texto}});
    ator.on_update(&pedaco(&format!("o código é {primeira}")));
    let id = ator.last_text.clone().unwrap();
    tokio::time::sleep(Duration::from_millis(350)).await;
    ator.flush(false);
    let parcial = texto_da_entrada(&f, &id);
    assert!(!parcial.contains("mxs"), "o começo do código fica retido: {parcial}");
    assert!(parcial.starts_with("o código é"));
    ator.on_update(&pedaco(&format!("{segunda} fim")));
    ator.flush(true);
    let final_ = texto_da_entrada(&f, &id);
    assert_eq!(final_, "o código é mxs_[oculto] fim");
    assert!(!final_.contains(&codigo));
}

#[tokio::test]
async fn o_fechamento_do_segmento_republica_o_sufixo_retido() {
    let f = Fixture::new();
    let mut ator = new_actor(f.hub.clone(), f.cfg.clone());
    ator.lane = Lane::main(f.cfg.session_owner());
    ator.turn = Some(1);
    ator.append(SegKind::Text, "abc m");
    let id = ator.last_text.clone().unwrap();
    assert_eq!(texto_da_entrada(&f, &id), "abc ", "o fim que pode ser o começo de um código fica retido");
    ator.close_seg();
    assert_eq!(texto_da_entrada(&f, &id), "abc m", "o fechamento republica o texto completo com fim");
}

#[tokio::test]
async fn saida_de_ferramenta_com_o_codigo_vai_mascarada() {
    let mut dados = json!({});
    let codigo = codigo();
    let conteudo =
        json!([{"type": "content", "content": {"type": "text", "text": format!("OPENCODE_API_KEY={codigo}")}}]);
    updates::merge_tool_content(&mut dados, &conteudo);
    assert_eq!(dados["output"], "OPENCODE_API_KEY=mxs_[oculto]");
}
