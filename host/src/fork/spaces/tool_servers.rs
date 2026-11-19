//! Decisão única de servidores efetivos. Contexto nunca cria autoridade.
use crate::store::{BotConfig, Store};

pub(crate) struct Servers<'a> {
    pub builtin: Vec<&'static str>,
    pub curated: bool,
    pub connectors: &'a [String],
}

/// `catchup` (MX-110): execução atrasada por indisponibilidade. Só leitura e
/// aviso: sem team/manage/routines/computers/computer. Os servidores
/// curados de conectores ficam — a proxy deles já abre cartão durável por efeito.
pub(crate) fn decide<'a>(store: &Store, cfg: &'a BotConfig, routine: bool, catchup: bool) -> Servers<'a> {
    if cfg.context.as_ref().is_some_and(|c| c.foreign_identity) {
        return Servers { builtin: vec![], curated: false, connectors: &[] };
    }
    if catchup {
        let builtin = [Some("connectors"), (cfg.space_id == super::PERSONAL).then_some("chat"), Some("memory")]
            .into_iter()
            .flatten()
            .collect();
        // Sem NENHUM conector: efeito externo zero em catch-up, mesmo com
        // cartão — a classificação é por construção, não por prompt.
        return Servers { builtin, curated: false, connectors: &[] };
    }
    let additional = cfg.context.is_some();
    let builtin = [
        Some("connectors"),
        (cfg.space_id == super::PERSONAL).then_some("chat"),
        (cfg.space_id == super::PERSONAL).then_some("team"),
        (!additional && crate::chat::manage::enabled(store, cfg)).then_some("manage"),
        Some("memory"),
        Some("tasks"),
        (!additional).then_some("routines"),
        (!additional && !routine && crate::computers::offered(store, cfg)).then_some("computers"),
        (!additional && cfg.computer).then_some("computer"),
    ]
    .into_iter()
    .flatten()
    .collect();
    Servers { builtin, curated: !additional, connectors: &cfg.connectors }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fork::spaces::{ExecutionContext, PERSONAL};
    use serde_json::json;
    #[test]
    fn mesma_decisao_conserva_principal_e_filtra_adicionais_sem_herdar_servidores_adiados() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let mut cfg: BotConfig = serde_json::from_value(
            json!({"id":"bot","name":"Fixture","computer":true,"connectors":["conector-fixture"]}),
        )
        .unwrap();
        assert!(decide(&store, &cfg, false, false).builtin.contains(&"computer"));
        assert!(decide(&store, &cfg, false, false).curated);
        assert_eq!(decide(&store, &cfg, false, false).connectors, cfg.connectors.as_slice());
        cfg.context = Some(ExecutionContext {
            conversation_id: "adicional".into(),
            memory_id: cfg.id.clone(),
            foreign_identity: false,
        });
        assert_eq!(cfg.space_id, PERSONAL);
        assert!(!decide(&store, &cfg, false, false).curated);
        assert_eq!(decide(&store, &cfg, false, false).builtin, ["connectors", "chat", "team", "memory", "tasks"]);
        assert_eq!(decide(&store, &cfg, false, false).connectors, cfg.connectors.as_slice());
        cfg.space_id = uuid::Uuid::new_v4().to_string();
        assert_eq!(decide(&store, &cfg, false, false).builtin, ["connectors", "memory", "tasks"]);
        cfg.context.as_mut().unwrap().foreign_identity = true;
        assert_eq!(decide(&store, &cfg, false, false).builtin, [] as [&str; 0]);
        assert!(!decide(&store, &cfg, false, false).curated);
    }
}
