//! Avisos duráveis para tarefas de fundo sem isolamento comprovado.
use crate::hub::Hub;
use crate::store::{BotConfig, EntryKind, Lane};
use serde_json::json;

const SUBSCRIPTION_UNAVAILABLE: &str = "background_subscription_unavailable: Esta tarefa de fundo está indisponível. Ela não aceita assinatura pessoal nem tem outra conexão compatível disponível. Nenhuma outra conta foi usada.";

#[derive(Debug)]
pub(crate) enum Problem {
    PersonalSubscription,
    NoClaude,
    Codex,
    Api,
    Model,
    Policy,
    #[cfg(test)]
    Quota,
    Version,
    Timeout,
    OutputLimit,
    Space,
    AccountChanged,
    /// A conta escolhida para a função está desconectada; `label` é o rótulo do cadastro.
    AccountDisconnected {
        label: String,
    },
}
impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::PersonalSubscription => SUBSCRIPTION_UNAVAILABLE,
            Self::NoClaude => "Não há conta compatível configurada para esta tarefa de fundo.",
            Self::Codex => "A assinatura Codex ainda não pode ser usada neste executor de fundo.",
            Self::Api => "Este executor de fundo não aceita esta conta com chave de API. Nenhuma outra conta foi usada.",
            Self::Model => "Não foi possível confirmar um modelo para a conta Claude. Confira a conta nas Configurações do sistema.",
            Self::Policy => "As regras deste computador impedem a tarefa de fundo ou não puderam ser conferidas.",
            #[cfg(test)]
            Self::Quota => "Não foi possível conferir o limite de uso da conta Claude. Vamos tentar novamente mais tarde.",
            Self::Version => {
                return write!(
                    f,
                    "Esta versão do conector do Claude não foi validada para tarefas de fundo. É necessária a versão {}.",
                    crate::fork::background_adapter::VERSION
                );
            }
            Self::Timeout => "A tarefa demorou mais que o esperado. Vamos tentar novamente mais tarde.",
            Self::OutputLimit => "A resposta ultrapassou o limite desta tarefa e não foi aplicada.",
            Self::Space => "As tarefas de fundo ainda não estão disponíveis neste espaço.",
            Self::AccountChanged => "A conta ou o modelo mudou durante a tarefa. Vamos tentar novamente.",
            Self::AccountDisconnected { label } => {
                return write!(
                    f,
                    "A conta {label} está desconectada; reconecte em Contas de IA ou escolha outra em Bot de fundo > Modelos por função."
                );
            }
        })
    }
}
impl std::error::Error for Problem {}

/// MEX-1355: texto de falha de seleção de "Organizar memória", ou `None` se o
/// problema não é de seleção. Mesmo texto na resolução de `functionModels`.
pub(crate) fn memory_selection_text(problem: &Problem) -> Option<String> {
    match problem {
        Problem::PersonalSubscription => Some(problem.to_string()),
        Problem::AccountDisconnected { label } => Some(format!(
            "Organizar memória: a conta {label} está desconectada; reconecte em Contas de IA ou escolha outra em Bot de fundo > Modelos por função"
        )),
        Problem::Model => {
            Some("Organizar memória precisa de um modelo: escolha em Bot de fundo > Modelos por função".into())
        }
        _ => None,
    }
}

pub(crate) fn supported(_hub: &Hub, _cfg: &BotConfig, _task: &str) -> bool {
    // O motor do trabalhador é global e pode diferir do motor da conversa.
    true
}

#[cfg(test)]
pub(crate) fn unavailable(hub: &Hub, cfg: &BotConfig, task: &str) {
    unavailable_with_error(hub, cfg, task, None);
}

pub(crate) fn unavailable_with_error(hub: &Hub, cfg: &BotConfig, task: &str, error: Option<&anyhow::Error>) {
    let name = match task {
        "nome" => "Nome automático",
        _ => "Organização da memória",
    };
    let text = if error.is_some_and(|e| matches!(e.downcast_ref::<Problem>(), Some(Problem::PersonalSubscription))) {
        SUBSCRIPTION_UNAVAILABLE.to_owned()
    } else if error.is_some_and(|e| e.downcast_ref::<crate::chat::oneshot::SubscriptionLimit>().is_some()) {
        format!("{name} indisponível: limite da assinatura atingido. Vamos tentar novamente mais tarde.")
    } else if let Some(problem) = error.and_then(|e| e.downcast_ref::<Problem>()) {
        match memory_selection_text(problem).filter(|_| task != "nome") {
            Some(text) => text,
            None => format!("{name} indisponível: {problem}"),
        }
    } else {
        format!(
            "{name} indisponível: o Bot de fundo não conseguiu concluir esta tarefa. Confira a conta nas Configurações do sistema."
        )
    };
    let key = notice_key(cfg, task);
    let reason = crate::agent::registry::sha256_hex(text.as_bytes());
    if hub.store.kv_get(&key).as_deref() == Some(reason.as_str()) {
        return;
    }
    if hub.add_entry(&Lane::main(&cfg.id), EntryKind::Notice, 0, &json!({"text":text})).is_some()
        && let Err(error) = hub.store.kv_set(&key, &reason)
    {
        tracing::warn!(bot = %cfg.id, %error, "não foi possível registrar o aviso de tarefa de fundo");
    }
}

pub(crate) fn recovered(hub: &Hub, cfg: &BotConfig, task: &str) {
    if let Err(error) = hub.store.kv_delete(&notice_key(cfg, task)) {
        tracing::warn!(bot = %cfg.id, %error, "não foi possível limpar o aviso de tarefa de fundo");
    }
}

fn notice_key(cfg: &BotConfig, task: &str) -> String {
    format!(
        "background-unavailable-v2-{}-{}-{}",
        cfg.id,
        task,
        crate::agent::registry::sha256_hex(cfg.backend.as_bytes())
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::identity::Identity;
    use crate::store::Store;
    use std::path::Path;

    #[test]
    fn avisa_de_novo_depois_de_recuperar() {
        let dir = std::env::temp_dir().join(format!("donnabot-aviso-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let store = Store::open(Path::new(":memory:")).unwrap();
        let cfg: BotConfig = serde_json::from_value(json!({"id":"b", "name":"Bot", "backend":"custom"})).unwrap();
        store.save_bot(&cfg).unwrap();
        let hub = Hub::new(store, "teste".into(), Identity::load_or_create(&dir).unwrap(), "teste".into(), 0);
        unavailable(&hub, &cfg, "nome");
        unavailable(&hub, &cfg, "nome");
        assert_eq!(hub.store.entries_since(0, 100).unwrap().len(), 1);
        let policy: anyhow::Error = Problem::Policy.into();
        unavailable_with_error(&hub, &cfg, "nome", Some(&policy));
        assert_eq!(hub.store.entries_since(0, 100).unwrap().len(), 2, "motivo mudou e deve ser informado");
        recovered(&hub, &cfg, "nome");
        unavailable(&hub, &cfg, "nome");
        assert_eq!(hub.store.entries_since(0, 100).unwrap().len(), 3);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn mensagens_explicam_causa_sem_jargao_de_configuracao() {
        for (problem, expected) in [
            (Problem::NoClaude, "Não há conta compatível configurada"),
            (Problem::PersonalSubscription, "Esta tarefa de fundo está indisponível"),
            (Problem::Codex, "assinatura Codex ainda não pode"),
            (Problem::Api, "não aceita esta conta com chave de API"),
            (Problem::Policy, "regras deste computador"),
            (Problem::Quota, "limite de uso"),
            (Problem::Version, "não foi validada para tarefas de fundo"),
            (Problem::Timeout, "demorou mais"),
            (Problem::Space, "neste espaço"),
            (Problem::AccountDisconnected { label: "Claude".into() }, "conta Claude está desconectada"),
        ] {
            assert!(problem.to_string().contains(expected));
        }
    }

    #[test]
    fn recusa_de_fundo_nao_recomenda_credencial_incompativel() {
        let text = Problem::PersonalSubscription.to_string();
        assert!(text.starts_with("background_subscription_unavailable: "));
        assert!(!text.contains("chave de API"));
        assert!(!text.contains("outro motor"));
    }
}
