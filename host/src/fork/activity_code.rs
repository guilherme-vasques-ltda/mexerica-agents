//! MX-147: código estável opcional da atividade exibida (`bot.activityCode`).
//! O texto legado (`activity`) continua sendo a linha de trabalho exibida; o
//! código nunca é inferido por comparação de frases nem traduz títulos livres:
//! só o produtor o define, na mesma mutação que escreve o texto.

use serde::Serialize;

/// Vocabulário estável do `kind` no fio (`snake_case`). Clientes tratam
/// kinds desconhecidos como ausência de código.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActivityKind {
    /// "Needs approval: {title}" — título do cartão de permissão.
    NeedsApproval { title: String },
    /// "Typing…" — compondo mensagem pela ferramenta de envio.
    Typing,
    /// "Downloading {item}…" — adaptador ou Node.js em download.
    Downloading { item: String },
    /// "Installing…" — extração ou provisionamento do que foi baixado.
    Installing { item: String },
    /// Título livre da ferramenta do agente, repassado sem traduzir.
    Tool { title: String },
    /// "Thinking…" / "Pensando…" — inclusive o trabalho de memória.
    Thinking,
    /// "Writing a reply…" — streaming da resposta do agente.
    WritingReply,
    /// "Working…" — envio concluído ou arquivo publicado no turno.
    Working,
    /// "Continuing…" — permissão respondida, turno retomado.
    Continuing,
    /// "Starting…" — turno admitido, antes de subir o agente.
    Starting,
    /// "Starting agent…" — processo do agente subindo.
    StartingAgent,
    /// "Picking up where it left off…" — turno oculto retomado.
    ResumingWork,
    /// "Resuming session…" — sessão ACP recarregada.
    ResumingSession,
    /// "Summarizing the earlier conversation…" — compactação.
    Summarizing,
    /// "Aguardando admissão do próximo turno."
    WaitingAdmission,
    /// "Aguardando execução nesta sessão."
    WaitingSession,
    /// "Aguardando · …" — limite de assinatura; a hora vai em `resumeAt`.
    SubscriptionWait,
    /// "Limite da assinatura · tentativa automática suspensa".
    SubscriptionPaused,
    /// Preparo do pedido humano falhou após as tentativas previstas.
    SetupFailed,
    /// Preparação da rotina não persistiu; nova tentativa pendente.
    RoutineSetupFailed,
    /// Rotina interrompida depois da permissão durável.
    RoutineInterrupted,
    /// "Couldn't start" — turno falhou antes do prompt.
    StartFailed,
    /// "Parado pelo teto de gasto." — envio segurado pelo teto até decisão do dono.
    CeilingStopped,
}

/// Objeto `activityCode` no fio: `kind` e seus parâmetros, mais `member` quando
/// a projeção agrega o membro ocupado no cartão do grupo (o texto ganha o
/// mesmo nome como prefixo).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ActivityCode {
    #[serde(flatten)]
    pub kind: ActivityKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
}

impl ActivityCode {
    /// Código sem vínculo de grupo; a agregação preenche `member` na projeção.
    pub const fn of(kind: ActivityKind) -> Self {
        Self { kind, member: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializacao_no_fio() {
        let cases = [
            (ActivityKind::Typing, json!({"kind": "typing"})),
            (ActivityKind::Thinking, json!({"kind": "thinking"})),
            (ActivityKind::WritingReply, json!({"kind": "writing_reply"})),
            (ActivityKind::Working, json!({"kind": "working"})),
            (ActivityKind::Continuing, json!({"kind": "continuing"})),
            (ActivityKind::Starting, json!({"kind": "starting"})),
            (ActivityKind::StartingAgent, json!({"kind": "starting_agent"})),
            (ActivityKind::ResumingWork, json!({"kind": "resuming_work"})),
            (ActivityKind::ResumingSession, json!({"kind": "resuming_session"})),
            (ActivityKind::Summarizing, json!({"kind": "summarizing"})),
            (ActivityKind::WaitingAdmission, json!({"kind": "waiting_admission"})),
            (ActivityKind::WaitingSession, json!({"kind": "waiting_session"})),
            (ActivityKind::SubscriptionWait, json!({"kind": "subscription_wait"})),
            (ActivityKind::SubscriptionPaused, json!({"kind": "subscription_paused"})),
            (ActivityKind::SetupFailed, json!({"kind": "setup_failed"})),
            (ActivityKind::RoutineSetupFailed, json!({"kind": "routine_setup_failed"})),
            (ActivityKind::RoutineInterrupted, json!({"kind": "routine_interrupted"})),
            (ActivityKind::StartFailed, json!({"kind": "start_failed"})),
            (ActivityKind::CeilingStopped, json!({"kind": "ceiling_stopped"})),
            (
                ActivityKind::NeedsApproval { title: "rodar cargo test".into() },
                json!({"kind": "needs_approval", "title": "rodar cargo test"}),
            ),
            (ActivityKind::Downloading { item: "Codex".into() }, json!({"kind": "downloading", "item": "Codex"})),
            (ActivityKind::Installing { item: "Node.js".into() }, json!({"kind": "installing", "item": "Node.js"})),
            (ActivityKind::Tool { title: "Terminal".into() }, json!({"kind": "tool", "title": "Terminal"})),
        ];
        for (kind, expected) in cases {
            assert_eq!(serde_json::to_value(ActivityCode::of(kind)).unwrap(), expected);
        }
        let mut grouped = ActivityCode::of(ActivityKind::NeedsApproval { title: "apagar".into() });
        grouped.member = Some("Berga".into());
        assert_eq!(
            serde_json::to_value(&grouped).unwrap(),
            json!({"kind": "needs_approval", "title": "apagar", "member": "Berga"})
        );
    }
}
