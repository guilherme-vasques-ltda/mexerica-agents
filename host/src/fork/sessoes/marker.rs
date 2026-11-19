//! Marcador de sessão para apps antigos (D229-5, R4 #7, R5 #5, R6 #3). Decidido na inserção de toda entrada, na
//! mesma transação: quando a sessão da entrada não é a da última entrada do chat, um aviso "Sessão “X”" entra antes.
//! O próprio marcador leva a sessão nova, então A → B → A gera um marcador em cada troca, mesmo com gravações
//! concorrentes. O app novo o esconde (`isChat`).
use crate::store::Lane;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

/// Os dados do marcador, se a entrada seguinte muda de sessão.
pub(crate) fn needed(c: &Connection, lane: &Lane) -> Result<Option<Value>> {
    let last: Option<Option<String>> = c
        .query_row("SELECT session FROM entries WHERE bot_id=?1 ORDER BY seq DESC LIMIT 1", [&lane.chat], |r| r.get(0))
        .optional()?;
    if last.flatten() == lane.session {
        return Ok(None);
    }
    let text = match &lane.session {
        None => "Sessão principal".to_owned(),
        Some(id) => {
            let title: Option<String> =
                c.query_row("SELECT title FROM donna_bot_sessions WHERE id=?1", [id], |r| r.get(0)).optional()?;
            let title = title.filter(|t| !t.is_empty()).unwrap_or_else(|| "Nova sessão".into());
            format!("Sessão “{title}”")
        }
    };
    Ok(Some(json!({"text": text, "sessionMarker": {"sessionId": lane.session_id()}})))
}
