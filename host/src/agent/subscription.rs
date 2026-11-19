//! Limites da assinatura informados pelos adapters; nunca escolhe provedor ou modelo.
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limit {
    pub resets_at: Option<i64>,
}

/// Respostas do adapter, não texto de ferramentas ou de uma conversa normal.
pub fn detect(backend: &str, kind: Option<&str>, message: &str, data: &Value, now: i64) -> Option<Limit> {
    if !matches!(backend, "codex" | "claude") {
        return None;
    }
    let info = &data["_meta"]["_claude/rateLimit"];
    let rejected = backend == "claude" && info["status"] == "rejected";
    let limited = kind.is_some_and(limit_code)
        || structured_limit(data, 0)
        || known_message(message)
        || data["message"].as_str().is_some_and(known_message)
        || data["details"].as_str().is_some_and(known_message)
        || rejected;
    if !limited {
        return None;
    }
    let windows = crate::usage::parse_codex(&data["rate_limits"], now);
    let resets_at = windows
        .iter()
        .filter(|w| w.percent >= 100.0)
        .filter_map(|w| w.resets_at)
        .chain(reset_at(data, now))
        .chain(rejected.then(|| reset_at(info, now)).flatten())
        .filter(|at| *at > now)
        .max();
    Some(Limit { resets_at })
}

pub(super) fn limit_code(code: &str) -> bool {
    matches!(
        code.to_ascii_lowercase().replace(['_', '-'], "").as_str(),
        "ratelimit" | "ratelimitexceeded" | "usagelimitexceeded" | "usagelimitreached" | "subscriptionlimitreached"
    )
}

fn structured_limit(value: &Value, depth: usize) -> bool {
    if depth > 4 {
        return false;
    }
    match value {
        Value::Object(fields) => fields.iter().any(|(key, v)| {
            if matches!(key.as_str(), "codex_error_info" | "errorKind" | "error_kind" | "code" | "type" | "stopReason")
            {
                v.as_str().is_some_and(limit_code) || v.as_object().is_some_and(|o| o.keys().any(|k| limit_code(k)))
            } else if matches!(key.as_str(), "error" | "data") {
                structured_limit(v, depth + 1)
            } else {
                false
            }
        }),
        _ => false,
    }
}

fn known_message(message: &str) -> bool {
    // A mensagem precisa ser a resposta inteira do adapter. Texto citado, erros
    // de ferramentas e diagnósticos concatenados nunca são evidência de assinatura.
    matches!(
        message.trim().to_lowercase().replace('’', "'").trim_end_matches('.'),
        "you've hit your usage limit"
            | "you have hit your usage limit"
            | "you've hit your session limit"
            | "you've hit your weekly limit"
            | "you've reached your usage limit"
            | "usage limit reached"
            | "usage limit exceeded"
            | "subscription limit reached"
    )
}

fn reset_at(data: &Value, now: i64) -> Option<i64> {
    ["resetsAt", "resets_at", "reset_at"]
        .iter()
        .filter_map(|key| {
            let v = &data[*key];
            v.as_i64().map(|at| if at > 100_000_000_000 { at } else { at.saturating_mul(1000) }).or_else(|| {
                v.as_str().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|t| t.timestamp_millis())
            })
        })
        .chain(data["retry_after_seconds"].as_i64().map(|s| now.saturating_add(s.saturating_mul(1000))))
        .max()
}

/// Só janelas esgotadas do modelo selecionado completam o reset. Um aviso de 75% não bloqueia um turno.
pub fn usage_reset(usage: &crate::usage::Usage, backend: &str, model: Option<&str>, now: i64) -> Option<i64> {
    let model = model.unwrap_or_default().to_lowercase();
    usage
        .providers
        .iter()
        .filter(|p| p.id == backend)
        .flat_map(|p| &p.windows)
        .filter(|w| w.percent >= 100.0)
        .filter(|w| !w.id.contains("opus") || model.contains("opus"))
        .filter(|w| !w.id.contains("sonnet") || model.contains("sonnet"))
        .filter_map(|w| w.resets_at)
        .filter(|at| *at > now)
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reconhece_limites_dos_dois_adapters_e_preserva_reset() {
        let codex = json!({"message": "You've hit your usage limit", "codex_error_info": "UsageLimitExceeded", "rate_limits": {
            "primary": {"used_percent": 100, "resets_at": 1_800_000_010},
            "secondary": {"used_percent": 100, "resets_at": 1_800_000_020}
        }});
        assert_eq!(
            detect("codex", None, "Internal error", &codex, 1_800_000_000_000),
            Some(Limit { resets_at: Some(1_800_000_020_000) })
        );
        assert_eq!(
            detect("claude", Some("rate_limit"), "Internal error", &json!({}), 0),
            Some(Limit { resets_at: None })
        );
        assert_eq!(
            detect("claude", None, "You've hit your session limit", &json!({}), 0),
            Some(Limit { resets_at: None })
        );
        let event = json!({"_meta": {"_claude/rateLimit": {"status": "rejected", "resetsAt": 1_800_000_030}}});
        assert_eq!(
            detect("claude", None, "", &event, 1_800_000_000_000),
            Some(Limit { resets_at: Some(1_800_000_030_000) })
        );
    }

    #[test]
    fn nao_confunde_erros_comuns_ou_aviso_de_uso_com_bloqueio() {
        for (backend, kind, message, data) in [
            ("claude", Some("overloaded"), "Service unavailable", json!({})),
            ("codex", None, "429 Too Many Requests", json!({})),
            ("claude", None, "MCP: usage limit exceeded", json!({})),
            ("codex", None, "tool failed: You've hit your usage limit", json!({})),
            ("claude", None, "rate limit exceeded", json!({})),
            ("codex", None, "context window exceeded", json!({})),
            ("api", Some("rate_limit"), "usage limit reached", json!({})),
            (
                "claude",
                None,
                "",
                json!({"_meta": {"_claude/rateLimit": {"status": "allowed_warning", "utilization": 1.0}}}),
            ),
        ] {
            assert!(detect(backend, kind, message, &data, 0).is_none());
        }
    }
}
