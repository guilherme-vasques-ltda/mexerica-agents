//! R4a C1 (D302): os passos do turno guardam só um resumo da leitura do `WhatsApp`, pelo
//! caminho real do ator (`on_update`), nunca o texto, o nome ou o número.
#![allow(clippy::unwrap_used)]
use super::*;
use crate::fork::spaces::tool_context::tests::Fixture;

const TEXT: &str = "SENTINELA_TEXTO_W";
const NAME: &str = "Ana Sentinela";
const PHONE: &str = "5511988887777";

fn result(body: &Value) -> Value {
    let mut body = body.clone();
    body["_meta"] = json!({"donnabot/guard": {"origin": "external", "sourceId": "whatsapp"}});
    json!([{"type": "content", "content": {"type": "text", "text": body.to_string()}}])
}

/// O passo gravado (e espelhado ao app) da chamada `id`.
fn step(f: &Fixture, actor: &Actor, id: &str) -> Value {
    f.hub.store.entry(&actor.tools[id]).unwrap().data
}

fn clean(data: &Value) {
    let raw = data.to_string();
    for secret in [TEXT, NAME, PHONE] {
        assert!(!raw.contains(secret), "{secret} no passo: {raw}");
    }
}

#[tokio::test]
async fn the_steps_keep_only_a_summary_of_what_the_bot_read() {
    let f = Fixture::new();
    let mut actor = new_actor(f.hub.clone(), f.cfg.clone());
    actor.lane = Lane::main(f.cfg.session_owner());
    actor.turn = Some(1);
    actor.turn_session = Some("sessao-A".into());
    let chat = format!("{PHONE}@s.whatsapp.net");
    // Ler conversa: o título pode vir com o argumento; o resultado chega no update.
    actor.on_update(&json!({"sessionUpdate":"tool_call","toolCallId":"w1","status":"pending",
        "title":format!("mcp__connectors__whatsapp_read_chat {chat}"),
        "rawInput":{"tool":"whatsapp_read_chat","arguments":{"whatsappChatId":chat}}}));
    let page = json!({"whatsappChatId":chat,"name":NAME,"messages":[
        {"from":{"name":NAME,"phone":format!("+{PHONE}")},"body":TEXT},
        {"from":{"name":NAME,"phone":format!("+{PHONE}")},"body":TEXT}]});
    actor.on_update(&json!({"sessionUpdate":"tool_call_update","toolCallId":"w1","status":"completed",
        "content":result(&page)}));
    let read = step(&f, &actor, "w1");
    clean(&read);
    assert_eq!(read["output"], "Leu 2 mensagem(ns) de 1 conversa do WhatsApp.");
    assert_eq!(read["title"], "WhatsApp: ler conversa");
    // Uma atualização posterior sem conteúdo não traz o título cru de volta.
    actor.on_update(
        &json!({"sessionUpdate":"tool_call_update","toolCallId":"w1","title":format!("whatsapp_read_chat {chat}")}),
    );
    let read = step(&f, &actor, "w1");
    clean(&read);
    assert_eq!(read["output"], "Leu 2 mensagem(ns) de 1 conversa do WhatsApp.");
    // Listar conversas, reconhecida só pela marca do resultado (título genérico).
    let list = json!({"chats":[{"whatsappChatId":chat,"name":NAME,"phone":format!("+{PHONE}"),"last":{"body":TEXT}}]});
    actor.on_update(&json!({"sessionUpdate":"tool_call","toolCallId":"w2","title":"Tool","status":"completed",
        "content":result(&list)}));
    let listed = step(&f, &actor, "w2");
    clean(&listed);
    assert_eq!(listed["output"], "Leu a lista de conversas do WhatsApp (1).");
    // Outra ferramenta continua com a saída inteira.
    actor.on_update(&json!({"sessionUpdate":"tool_call","toolCallId":"o1","title":"Read a.txt","status":"completed",
        "content":[{"type":"content","content":{"type":"text","text":TEXT}}]}));
    assert_eq!(step(&f, &actor, "o1")["output"], TEXT);
}
