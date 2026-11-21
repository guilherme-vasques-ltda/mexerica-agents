//! Claude por assinatura pelo programa oficial. O Hub conduz o Claude Code instalado, sem
//! modificação, no modo sem tela que ele documenta (`claude -p` com `stream-json`), sem passar pelo SDK de agentes.
//!
//! A ponte é o próprio executável do Hub num modo interno (`__claude-ponte`): fala ACP com o Hub, como o
//! adaptador anterior, e traduz cada sessão para um processo do programa oficial. Assim o resto do Hub
//! (cartões de permissão, porteiro de regras, custo, retomada, compactação) segue igual.
//!
//! - O login é sempre do programa oficial, no perfil isolado da conta (`CLAUDE_CONFIG_DIR`); a ponte só pergunta
//!   `claude auth status` (lê `loggedIn`) e nunca lê, guarda nem repassa token.
//! - Permissões usam a ferramenta documentada `--permission-prompt-tool`, servida pela ponte em `127.0.0.1`.
//! - `MEXERICA_CLAUDE_SDK=1` volta ao adaptador antigo (`claude-agent-acp`), para o que ainda não tem
//!   equivalente no modo sem tela.

mod agente;
pub(crate) mod opcoes;
mod permissao;
mod traducao;

#[cfg(test)]
#[cfg(unix)]
mod testes;

#[cfg(test)]
mod testes_cartao;

use serde_json::Value;

/// Argumento do executável do Hub que abre a ponte.
pub(crate) const MODO: &str = "__claude-ponte";
/// `agentInfo.name` da ponte no handshake ACP.
pub(crate) const NOME: &str = "mexerica-claude-ponte";
/// `1` mantém o caminho anterior (adaptador com o SDK de agentes).
pub(crate) const LEGADO_ENV: &str = "MEXERICA_CLAUDE_SDK";

/// O caminho anterior foi pedido explicitamente?
pub(crate) fn legado() -> bool {
    std::env::var_os(LEGADO_ENV).is_some_and(|v| v == "1")
}

/// Comando ACP da ponte: o executável do Hub no modo interno; `programa` fixa o Claude Code a conduzir (sem ele,
/// vale `CLAUDE_CODE_EXECUTABLE` e depois o `PATH`).
pub(crate) fn comando(programa: Option<&std::path::Path>) -> anyhow::Result<String> {
    let exe = crate::service::current_exe()?;
    let mut cmd = format!("{} {MODO}", crate::shell::quote(&exe.to_string_lossy()));
    if let Some(p) = programa {
        // Um item relativo no PATH devolve caminho relativo; a ponte só aceita caminho absoluto.
        let p = std::path::absolute(p)?;
        cmd.push(' ');
        cmd.push_str(&crate::shell::quote(&p.to_string_lossy()));
    }
    Ok(cmd)
}

/// O handshake veio desta ponte, nesta versão do Hub?
pub(crate) fn e_a_ponte(init: &Value) -> bool {
    init["agentInfo"]["name"] == NOME && init["agentInfo"]["version"] == env!("CARGO_PKG_VERSION")
}

/// Corpo do modo `__claude-ponte`: atende ACP na entrada/saída padrão até o Hub fechar a entrada.
pub(crate) async fn executar(args: &[String]) -> anyhow::Result<()> {
    let mut cfg = agente::Config::do_ambiente();
    if let Some(p) = args.first() {
        let p = std::path::PathBuf::from(p);
        anyhow::ensure!(p.is_absolute() && p.is_file(), "programa do Claude Code inválido");
        cfg.exe = Some(p);
    }
    agente::servir(tokio::io::stdin(), tokio::io::stdout(), cfg).await
}
