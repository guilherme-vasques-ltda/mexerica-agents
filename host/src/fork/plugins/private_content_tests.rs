use super::{Class, Destination, Operation, Requirement, probe, requirement};

#[test]
fn destinos_legados_proibem_todas_as_classes() {
    let destinations = [
        Destination::Entries,
        Destination::Checkpoint,
        Destination::RendererCache,
        Destination::HarnessTranscript,
        Destination::AutomaticMemory,
        Destination::Publication,
    ];
    let classes = [
        Class::HumanRequest,
        Class::ToolResult,
        Class::ModelResponse,
        Class::Summary,
        Class::ReviewCard,
        Class::Error,
        Class::Artifact,
    ];
    for destination in destinations {
        assert!(matches!(requirement(destination), Requirement::Forbidden));
        for class in classes {
            assert!(probe(Operation::CalendarRead, class, destination).is_err());
        }
    }
}

#[test]
fn destinos_especializados_exigem_caminho_sem_provar_qualificacao() {
    for destination in [Destination::PrivateStore, Destination::PrivateView, Destination::ModelInput] {
        assert!(matches!(requirement(destination), Requirement::QualifiedPathRequired));
        assert!(probe(Operation::WhatsappReadChats, Class::ToolResult, destination).is_err());
    }
}

#[test]
fn operacoes_distintas_nao_abrem_a_porta() {
    assert_ne!(Operation::CalendarCreate, Operation::CalendarUpdate);
    assert_ne!(Operation::WhatsappReadChats, Operation::WhatsappReplyChats);
    for operation in [
        Operation::CalendarCreate,
        Operation::CalendarUpdate,
        Operation::WhatsappReadChats,
        Operation::WhatsappReplyChats,
    ] {
        assert!(probe(operation, Class::ReviewCard, Destination::PrivateView).is_err());
    }
}

#[test]
fn erro_e_estatico_sem_detalhes_do_pedido() {
    let error = probe(Operation::WhatsappReplyChats, Class::HumanRequest, Destination::HarnessTranscript)
        .expect_err("a porta deve permanecer fechada");
    assert_eq!(super::Unavailable::code(), "private_content_unavailable");
    assert_eq!(error.to_string(), "Conteúdo privado indisponível.");
    assert_eq!(format!("{error:?}"), "Unavailable");
}
