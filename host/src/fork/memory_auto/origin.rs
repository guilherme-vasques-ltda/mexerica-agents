//! O modelo aponta evidências; somente os registros do Hub classificam sua origem.
use anyhow::Result;
use codync_mem::origin::{Kind, Origin, Source};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

pub(crate) fn derive(db: &Connection, context: &super::queue::Context, ids: &[String], text: &str) -> Result<Origin> {
    let mut sources = Vec::new();
    let mut external = false;
    let mut unknown = false;
    let mut exact = ids.len() == 1;
    let marked: Option<Option<i64>> = db
        .query_row(
            "SELECT marked_at FROM donna_memory_sessions WHERE bot=?1 AND session=?2",
            params![context.bot_id, context.session],
            |r| r.get(0),
        )
        .optional()?;
    let inherited = context.initial_marked || marked.is_none_or(|at| at.is_some());
    for id in ids {
        if context.responses.iter().any(|r| r.id == *id) {
            let human: Vec<String> = context.messages.iter().map(|m| m.id.clone()).collect();
            let inherited_origin = derive(db, context, &human, "")?;
            external |= inherited_origin.external;
            unknown |= inherited_origin.kind == Kind::Unknown;
            exact = false;
            sources.extend(inherited_origin.sources);
            sources.push(Source {
                message_id: id.clone(),
                kind: if inherited_origin.kind == Kind::Unknown {
                    Kind::Unknown
                } else if inherited_origin.external {
                    Kind::External
                } else {
                    Kind::Conversation
                },
                detail: None,
            });
            continue;
        }
        let raw: String = db.query_row("SELECT data FROM entries WHERE id=?1", [id], |r| r.get(0))?;
        let data: Value = serde_json::from_str(&raw)?;
        let known = known(&data);
        let clean = clean(&data);
        let literal = crate::chat::memory::write_gate::said(&data, text);
        exact &= literal;
        let kind = if literal {
            Kind::User
        } else if !known {
            unknown = true;
            Kind::Unknown
        } else if !clean || inherited {
            external = true;
            Kind::External
        } else {
            Kind::Conversation
        };
        sources.push(Source { message_id: id.clone(), kind, detail: None });
    }
    let kind = if unknown || sources.is_empty() {
        Kind::Unknown
    } else if exact && !external {
        Kind::User
    } else if external {
        Kind::External
    } else {
        Kind::Conversation
    };
    Ok(Origin { schema: 1, kind, sources, external: external || unknown })
}

/// O app informou de onde veio o texto da mensagem.
fn known(data: &Value) -> bool {
    let o = &data["contentOrigin"];
    o["status"] == "appReported" && o["segments"].as_array().is_some_and(|s| !s.is_empty())
}

/// A mensagem INTEIRA foi digitada ou falada pelo dono: origem conhecida, sem colagem, anexo ou resposta citada.
pub(crate) fn clean(data: &Value) -> bool {
    let o = &data["contentOrigin"];
    known(data)
        && o["segments"]
            .as_array()
            .is_some_and(|s| s.iter().all(|p| matches!(p["kind"].as_str(), Some("typed" | "voice"))))
        && o["attachments"].as_array().is_none_or(Vec::is_empty)
        && !o["reply"].is_object()
        && data["pasted"].as_array().is_none_or(Vec::is_empty)
        && data["attachments"].as_array().is_none_or(Vec::is_empty)
        && !data["replyTo"].is_object()
}

/// Sono 2b1c, atalho literal (seção 3 do desenho): `text` é uma FRASE INTEIRA (R1-01) de uma mensagem humana do
/// `Context` que é limpa por inteiro, a sessão não está exposta a conteúdo externo (R1-02: conferido aqui, inclusive
/// a marca posterior à extração) e a origem recalculada pelo Hub dá fala do dono. O `literal_owner` da extração
/// aceita um trecho digitado dentro de uma mensagem mista; aqui não.
pub(crate) fn confirms(db: &Connection, context: &super::queue::Context, ids: &[String], text: &str) -> Result<bool> {
    if exposed(db, context)? {
        return Ok(false);
    }
    for id in ids.iter().filter(|id| context.messages.iter().any(|m| m.id == **id)) {
        let raw: Option<String> =
            db.query_row("SELECT data FROM entries WHERE id=?1 AND kind='user'", [id], |r| r.get(0)).optional()?;
        let Some(data) = raw.and_then(|raw| serde_json::from_str::<Value>(&raw).ok()) else { continue };
        if clean(&data)
            && crate::chat::memory::write_gate::whole_sentence(&data, text)
            && derive(db, context, std::slice::from_ref(id), text)?.kind == Kind::User
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// A sessão do pedido viu conteúdo externo: já começou marcada, foi marcada depois ou não tem registro.
pub(crate) fn exposed(db: &Connection, context: &super::queue::Context) -> Result<bool> {
    let marked: Option<Option<i64>> = db
        .query_row(
            "SELECT marked_at FROM donna_memory_sessions WHERE bot=?1 AND session=?2",
            params![context.bot_id, context.session],
            |r| r.get(0),
        )
        .optional()?;
    Ok(context.initial_marked || marked.is_none_or(|at| at.is_some()))
}

/// A natureza Said só vem deste teste de fala literal validada, nunca de um rótulo do modelo.
pub(crate) fn literal_owner(
    db: &Connection,
    context: &super::queue::Context,
    ids: &[String],
    text: &str,
) -> Result<bool> {
    for id in ids {
        if !context.messages.iter().any(|m| m.id == *id) {
            continue;
        }
        let raw: String = db.query_row("SELECT data FROM entries WHERE id=?1 AND kind='user'", [id], |r| r.get(0))?;
        let data: Value = serde_json::from_str(&raw)?;
        if crate::chat::memory::write_gate::said(&data, text) {
            return Ok(true);
        }
    }
    Ok(false)
}
