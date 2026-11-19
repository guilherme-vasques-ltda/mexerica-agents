//! Quem pediu a sessão. Enum fechado, validado na gravação e na leitura, como `runs_on`.
use anyhow::{Result, anyhow, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Só `Owner` e `Continuation` nascem no 2a. Os outros já passam pela validação para Times e o 2b gravarem sem migrar.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase", deny_unknown_fields)]
pub(crate) enum SessionOrigin {
    Owner,
    Continuation,
    Bot {
        bot_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from_session: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from_execution: Option<String>,
    },
    Routine {
        routine_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_id: Option<String>,
    },
    Notice,
    Group,
    Subsession,
    Suggestion {
        entry_ids: Vec<String>,
    },
    Routed {
        rule: String,
    },
    /// Sessão vinculada a uma tarefa do Hub (MX-142): nasce só no início
    /// explícito/agendado, nunca por atribuição.
    Task {
        task_id: String,
    },
}

impl SessionOrigin {
    fn check(&self) -> Result<()> {
        let filled = |s: &str| !s.trim().is_empty();
        let ok = match self {
            Self::Owner | Self::Continuation | Self::Notice | Self::Group | Self::Subsession => true,
            Self::Bot { bot_id, from_session, from_execution } => {
                filled(bot_id)
                    && from_session.as_deref().is_none_or(filled)
                    && from_execution.as_deref().is_none_or(filled)
            }
            Self::Routine { routine_id, run_id } => filled(routine_id) && run_id.as_deref().is_none_or(filled),
            Self::Suggestion { entry_ids } => !entry_ids.is_empty() && entry_ids.iter().all(|id| filled(id)),
            Self::Routed { rule } => filled(rule),
            Self::Task { task_id } => filled(task_id),
        };
        ensure!(ok, "invalid_session_origin");
        Ok(())
    }

    /// O texto da coluna: só o que esta função produz é gravado.
    pub(crate) fn column(&self) -> Result<String> {
        self.check()?;
        Ok(serde_json::to_string(self)?)
    }
}

/// Lê a coluna. JSON ilegível, `kind` desconhecido, chave extra ou campo obrigatório ausente falham, nunca viram `owner`.
pub(crate) fn parse(raw: &str) -> Result<SessionOrigin> {
    let value: Value = serde_json::from_str(raw).map_err(|_| anyhow!("invalid_session_origin"))?;
    let origin: SessionOrigin = serde_json::from_value(value.clone()).map_err(|_| anyhow!("invalid_session_origin"))?;
    // A volta exata recusa o que o enum aceitaria sem reproduzir (`null` em campo opcional, por exemplo).
    ensure!(serde_json::to_value(&origin)? == value, "invalid_session_origin");
    origin.check()?;
    Ok(origin)
}
