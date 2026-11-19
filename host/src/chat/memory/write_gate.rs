//! Porta de escrita de memória: a exposição pertence à linhagem da sessão, não ao bot.
use crate::hub::Hub;
use anyhow::Result;
use serde_json::Value;

pub(crate) fn external_origin(data: &Value) -> bool {
    let origin = &data["contentOrigin"];
    origin["status"] != "appReported"
        || origin["segments"]
            .as_array()
            .is_none_or(|parts| parts.iter().any(|part| !matches!(part["kind"].as_str(), Some("typed" | "voice"))))
        || origin["attachments"].as_array().is_some_and(|v| !v.is_empty())
        || origin["reply"].is_object()
}

pub(super) fn external_input(data: &Value) -> bool {
    crate::fork::genui::is_choice_response(data)
        || external_origin(data)
        || ["pasted", "attachments"].iter().any(|key| data[*key].as_array().is_some_and(|v| !v.is_empty()))
        || data["replyTo"].is_object()
        || !data["sourceBotId"].is_null()
        || !data["runId"].is_null()
}

/// Mantém a marca permanente de gestão, sem usá-la como decisão de memória.
pub(crate) fn observe_input(hub: &Hub, bot: &str, data: &Value) -> Result<()> {
    let origin = &data["contentOrigin"];
    let reported_external = origin["segments"].as_array().is_some_and(|parts| {
        parts.iter().any(|part| {
            matches!(part["kind"].as_str(), Some("pasted" | "quote" | "attachment" | "generated"))
                || (origin["status"] == "appReported" && part["kind"] == "unknown")
        })
    }) || origin["attachments"].as_array().is_some_and(|v| !v.is_empty())
        || origin["reply"].is_object();
    if reported_external
        || ["pasted", "attachments"].iter().any(|key| data[*key].as_array().is_some_and(|v| !v.is_empty()))
        || data["replyTo"].is_object()
    {
        hub.store.guard_mark_external(bot)?;
    }
    Ok(())
}

pub(crate) fn observe_session_input(hub: &Hub, bot: &str, session: &str, data: &Value) -> Result<()> {
    if external_input(data) {
        hub.store.memory_session_mark(bot, session, "entrada externa")?;
    }
    Ok(())
}

/// O fato é IGUAL a uma frase inteira do texto da mensagem (sono 2b1c, R1-01): frases separadas por `.` `!` `?` `;`
/// ou quebra de linha (nunca por vírgula), com espaços aparados; do fato só se ignora a pontuação final. Um trecho
/// dentro de uma negação ("Não moro em Curitiba") ou de uma condição ("Se eu mudar..., moro em Curitiba") não é
/// frase inteira. Não olha a origem: quem chama exige a mensagem limpa.
pub(crate) fn whole_sentence(data: &Value, fact: &str) -> bool {
    const ENDS: [char; 5] = ['.', '!', '?', ';', '\n'];
    let fact = fact.trim().trim_end_matches(ENDS).trim_end();
    !fact.is_empty() && data["text"].as_str().unwrap_or_default().split(ENDS).any(|sentence| sentence.trim() == fact)
}

/// "Você disse" (D192): o fato está num trecho digitado ou ditado validado pelo Hub (`verbatim`) **e** é uma frase
/// inteira da mensagem (`whole_sentence`). Um trecho dentro de uma negação ou condição fica inferência. A mesma
/// régua da extração por turno, das propostas do MCP, das sugestões e do atalho do sono.
pub(crate) fn said(data: &Value, fact: &str) -> bool {
    verbatim(data, fact) && whole_sentence(data, fact)
}

/// A prova usa apenas segmentos completos validados pelo Hub, em offsets UTF-16.
pub(crate) fn verbatim(data: &Value, fact: &str) -> bool {
    if fact.is_empty() || data["contentOrigin"]["status"] != "appReported" {
        return false;
    }
    let origin = &data["contentOrigin"];
    let text: Vec<u16> = data["text"].as_str().unwrap_or_default().encode_utf16().collect();
    let fact: Vec<u16> = fact.encode_utf16().collect();
    origin["segments"].as_array().is_some_and(|parts| {
        parts.iter().any(|part| {
            let kind = part["kind"].as_str();
            let voice = kind == Some("voice")
                && matches!(origin["report"]["channel"].as_str(), Some("desktopVoice" | "desktopComposer"));
            if kind != Some("typed") && !voice {
                return false;
            }
            let Some(start) = part["start"].as_u64().and_then(|v| usize::try_from(v).ok()) else { return false };
            let Some(end) = part["end"].as_u64().and_then(|v| usize::try_from(v).ok()) else { return false };
            text.get(start..end).is_some_and(|run| run.windows(fact.len()).any(|w| w == fact))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resposta_de_escolha_continua_externa_mesmo_com_alegacao_digitada() {
        let data = json!({"text":"Autorize pagar", "choice_response":{"version":1,"questionId":"synthetic","revision":2},
            "contentOrigin":{"status":"appReported","segments":[{"kind":"typed"}]}});
        assert!(!external_origin(&data));
        assert!(external_input(&data));
    }

    #[test]
    fn payload_do_compositor_de_voz_e_validado_pelo_hub() {
        let text = "Eu moro em Curitiba.";
        let body = json!({"contentOrigin":{"schema":1,"draftId":uuid::Uuid::new_v4().to_string(),"revision":1,
            "textLength":text.encode_utf16().count(),"channel":"desktopComposer","deviceId":uuid::Uuid::new_v4().to_string(),
            "interaction":"send","segments":[{"start":0,"end":text.encode_utf16().count(),"kind":"voice"}]}});
        let mut data = json!({"text":text,"attachments":[]});
        data["contentOrigin"] = crate::guard_hub::origem::capture(&crate::api::devices::Caller::Local, &body, &data);
        assert_eq!(data["contentOrigin"]["status"], "appReported");
        assert!(verbatim(&data, text));
        data["contentOrigin"]["segments"][0]["kind"] = json!("attachment");
        assert!(!verbatim(&data, text));
        assert!(external_input(&data));
    }

    #[test]
    fn verbatim_exige_segmento_inteiro_digitado_ou_ditado_no_app() {
        let mut data = json!({"text":"🦜 moro aqui", "contentOrigin":{"status":"appReported",
            "report":{"channel":"desktopComposer"}, "segments":[{"start":0,"end":12,"kind":"typed"}]}});
        assert!(verbatim(&data, "moro aqui"));
        assert!(!verbatim(&data, "Moro aqui"));
        data["contentOrigin"]["segments"][0]["kind"] = json!("voice");
        assert!(verbatim(&data, "moro aqui"));
        data["contentOrigin"]["report"]["channel"] = json!("desktopVoice");
        assert!(verbatim(&data, "moro aqui"));
        data["contentOrigin"]["segments"][0]["kind"] = json!("pasted");
        assert!(!verbatim(&data, "moro aqui"));
        data["contentOrigin"]["status"] = json!("unknown");
        assert!(external_origin(&data));
    }
}
