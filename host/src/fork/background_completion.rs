//! Validação da conclusão ACP de tarefas de fundo.
pub(crate) fn validate(response: &serde_json::Value) -> anyhow::Result<()> {
    anyhow::ensure!(
        response.get("stopReason").and_then(serde_json::Value::as_str) == Some("end_turn"),
        "tarefa de fundo sem conclusão ACP normal"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate;
    use serde_json::json;

    #[test]
    fn exige_conclusao_normal() {
        assert!(validate(&json!({"stopReason":"end_turn"})).is_ok());
        for value in [json!({}), json!({"stopReason":null}), json!({"stopReason":"cancelled"})] {
            assert!(validate(&value).is_err());
        }
    }
}
