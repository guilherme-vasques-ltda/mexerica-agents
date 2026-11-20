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
    if !crate::fork::claude_ponte::legado() {
        return crate::fork::claude_ponte::comando(None).into_iter().collect();
    }
    legacy_commands()
}

fn legacy_commands() -> Vec<String> {
    let package = format!("{NAME}@{VERSION}");
    vec![Cmd { program: format!("npx -y {}", crate::shell::quote(&package)), args: String::new() }.acp()]
}

/// O conector de fundo é a ponte do programa oficial ou, com `MEXERICA_CLAUDE_SDK=1`, o adaptador antigo exato.
pub(crate) fn matches(init: &Value) -> bool {
    if !crate::fork::claude_ponte::legado() {
        return crate::fork::claude_ponte::e_a_ponte(init);
    }
    init["agentInfo"]["name"] == NAME && init["agentInfo"]["version"] == VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidato_unico_e_exato_sem_fallback() {
        let commands = legacy_commands();
        assert_eq!(commands.len(), 1);
        assert_eq!(
            shlex::split(&commands[0]).unwrap(),
            vec!["npx".to_owned(), "-y".to_owned(), format!("{NAME}@{VERSION}")]
        );
    }

    #[test]
    fn padrao_e_a_ponte_do_programa_oficial_e_so_ela_passa() {
        assert!(!crate::fork::claude_ponte::legado(), "os testes rodam sem MEXERICA_CLAUDE_SDK");
        let commands = commands();
        assert_eq!(commands, vec![crate::fork::claude_ponte::comando(None).unwrap()]);
        let ponte = serde_json::json!({"agentInfo": {"name": crate::fork::claude_ponte::NOME, "version": env!("CARGO_PKG_VERSION")}});
        assert!(matches(&ponte));
        let antigo = serde_json::json!({"agentInfo": {"name": NAME, "version": VERSION}});
        assert!(!matches(&antigo), "sem a opção, o adaptador do SDK não é aceito no fundo");
        let outra = serde_json::json!({"agentInfo": {"name": crate::fork::claude_ponte::NOME, "version": "0.0.0"}});
        assert!(!matches(&outra));
    }
}
