use super::{
    avaliacao::{self, Decisao, Mediacao, Origem, Pedido, Validade},
    floor::Efeito,
    rule::{ApprovalLevel, Campo, Effect, Rule},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Opaque,
    DeleteIrreversible,
    GitResetHard,
    GitBranchDelete,
    GitMerge,
    PackagePublish,
    RepositoryDelete,
    InfrastructureDestroy,
    Send,
    Pay,
    Commit,
    Push,
    OpenPullRequest,
}

impl Action {
    const fn policy(self) -> (&'static str, Efeito) {
        match self {
            Self::Opaque => ("opaco", Efeito::Opaco),
            Self::DeleteIrreversible => ("apagar", Efeito::ApagarOuSobrescreverIrreversivelmente),
            Self::GitResetHard => ("git_reset_hard", Efeito::ApagarOuSobrescreverIrreversivelmente),
            Self::GitBranchDelete => ("git_branch_delete", Efeito::ApagarOuSobrescreverIrreversivelmente),
            Self::GitMerge => ("git_merge", Efeito::ApagarOuSobrescreverIrreversivelmente),
            Self::PackagePublish => ("npm_publish", Efeito::AlterarExecucaoFutura),
            Self::RepositoryDelete => ("excluir_repositorio", Efeito::ApagarOuSobrescreverIrreversivelmente),
            Self::InfrastructureDestroy => ("destruir_infraestrutura", Efeito::ApagarOuSobrescreverIrreversivelmente),
            Self::Send => ("enviar", Efeito::EnviarNoComputadorDoDono),
            Self::Pay => ("pagar", Efeito::MoverDinheiro),
            Self::Commit => ("git_commit", Efeito::ComumComprovado),
            Self::Push => ("git_push", Efeito::ComumComprovado),
            Self::OpenPullRequest => ("abrir_pr", Efeito::ComumComprovado),
        }
    }

    pub(crate) const fn allows_persistent_rule(self) -> bool {
        matches!(self, Self::Commit | Self::Push | Self::OpenPullRequest)
    }
}

pub(crate) const TOOL: &str = "terminal";

pub(crate) fn decide(
    bot: &str,
    action: Action,
    target: &str,
    origin: Origem,
    authority: Validade,
    level: ApprovalLevel,
    rules: &[Rule],
) -> Decisao {
    let (acao, efeito) = action.policy();
    let applicable: Vec<_> = rules
        .iter()
        .filter(|rule| {
            rule.effect == Effect::Deny
                || (action.allows_persistent_rule()
                    && rule.tool.as_deref() == Some(TOOL)
                    && rule.bot_id.as_deref() == Some(bot)
                    && rule.interpretacao().is_some_and(|(campo, _)| campo == Campo::Acao))
        })
        .cloned()
        .collect();
    avaliacao::avaliar(
        &Pedido {
            bot,
            ferramenta: TOOL,
            acao,
            alvo: target,
            efeitos: &[efeito],
            origem: origin,
            mediacao: Mediacao::Comprovada,
            politica_e_autoridade: authority,
            nivel: level,
        },
        &applicable,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(field: &str, value: &str) -> Rule {
        Rule {
            id: "teste".into(),
            effect: Effect::Allow,
            field: field.into(),
            op: "igual".into(),
            value: value.into(),
            bot_id: Some("bot-teste".into()),
            tool: Some(TOOL.into()),
            scope: None,
            enabled: true,
            revision: 1,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn antiga_e_acao_distinta_nao_liberam_o_terminal() {
        let rules = [rule("ferramenta", TOOL), rule("acao", "git_commit")];
        for (action, expected) in [
            (Action::Commit, Decisao::Permitir),
            (Action::Push, Decisao::Perguntar),
            (Action::OpenPullRequest, Decisao::Perguntar),
            (Action::Opaque, Decisao::Perguntar),
        ] {
            assert_eq!(
                decide(
                    "bot-teste",
                    action,
                    "alvo-ficticio",
                    Origem::Derivada,
                    Validade::Atual,
                    ApprovalLevel::AutoApprove,
                    &rules
                ),
                expected
            );
        }
        assert!(!Action::Opaque.allows_persistent_rule());
    }

    #[test]
    fn efeitos_sensiveis_perguntam_em_todos_os_niveis_mesmo_com_regra() {
        for action in [
            Action::DeleteIrreversible,
            Action::GitResetHard,
            Action::GitBranchDelete,
            Action::GitMerge,
            Action::PackagePublish,
            Action::RepositoryDelete,
            Action::InfrastructureDestroy,
            Action::Send,
            Action::Pay,
        ] {
            assert!(!action.allows_persistent_rule());
            let rules = [rule("acao", action.policy().0)];
            for level in [ApprovalLevel::Ask, ApprovalLevel::AutoApprove, ApprovalLevel::FullAccess] {
                for origin in [Origem::DonoDigitadoAutenticado, Origem::Derivada] {
                    assert_eq!(
                        decide("bot-teste", action, "alvo-ficticio", origin, Validade::Atual, level, &rules),
                        Decisao::Perguntar,
                    );
                }
            }
        }
    }
}
