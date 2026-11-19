//! Conector auditado apenas para catálogo e tarefas de fundo Claude.
use crate::agent::registry::Cmd;
use serde_json::Value;

pub(crate) const NAME: &str = "@agentclientprotocol/claude-agent-acp";
pub(crate) const VERSION: &str = "0.94.0";

/// Sem registry/latest, executável ACP do PATH ou candidato alternativo.
/// O chamador confere a política antes de preparar o lançamento. O pacote
/// deve ser provisionado pelo operador antes do uso, para evitar que um
/// download consuma o prazo do handshake ACP.
pub(crate) fn commands() -> Vec<String> {
    let package = format!("{NAME}@{VERSION}");
    vec![Cmd { program: format!("npx -y {}", crate::shell::quote(&package)), args: String::new() }.acp()]
}

pub(crate) fn matches(init: &Value) -> bool {
    init["agentInfo"]["name"] == NAME && init["agentInfo"]["version"] == VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidato_unico_e_exato_sem_fallback() {
        let commands = commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(
            shlex::split(&commands[0]).unwrap(),
            vec!["npx".to_owned(), "-y".to_owned(), format!("{NAME}@{VERSION}")]
        );
    }
}
