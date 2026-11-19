//! Tratamento dos fatos externos propostos pela consolidação.

/// Qualquer ferramenta no agente de consolidação torna o resultado externo.
pub(crate) fn observe_update(method: &str, params: &serde_json::Value, exposed: &mut bool) {
    if method == "session/update"
        && matches!(params["update"]["sessionUpdate"].as_str(), Some("tool_call" | "tool_call_update"))
    {
        *exposed = true;
    }
}
