//! Ferramentas do Claude Code que nenhum bot usa: negadas para não irem ao modelo em toda chamada (DON-910).

/// Nomes conferidos por leitura no SDK 0.3.303 (Claude Code 2.1.303), via claude-agent-acp 0.94.0,
/// que soma `disallowedTools` às negações próprias.
/// `Agent` e as skills ficam de propósito. Revalidar ao atualizar o adaptador.
pub const CLAUDE_UNUSED_TOOLS: &[&str] = &["Workflow", "ListAgents", "ReportFindings"];

/// Negações do lançamento e da retomada: agendamento nativo mais as ferramentas sem uso.
pub fn disallowed() -> Vec<&'static str> {
    crate::fork::native_scheduling::CLAUDE_DISALLOWED_TOOLS.iter().chain(CLAUDE_UNUSED_TOOLS).copied().collect()
}
