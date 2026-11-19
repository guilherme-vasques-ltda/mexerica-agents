//! Política efetiva de exposição; os defaults pertencem aos produtores existentes.
use crate::store::BotConfig;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MemoryPolicy {
    version: u32,
    layers: Vec<Option<String>>,
    harness: bool,
}

impl MemoryPolicy {
    pub(crate) fn of_config(cfg: &BotConfig) -> Self {
        Self {
            version: 1,
            layers: crate::chat::memory::files::layers(cfg),
            harness: crate::agent::own_memory::kept(cfg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn politica_efetiva_reusa_defaults_sem_conceder_memoria_nativa_por_flag_ineficaz() {
        let mut cfg: BotConfig = serde_json::from_str(
            r#"{"id":"bot","name":"Bot","backend":"codex","cwd":"/projeto","sharedMemory":false}"#,
        )
        .unwrap();
        let off = MemoryPolicy::of_config(&cfg);
        cfg.harness_memory = true;
        cfg.name = "Outro nome".into();
        cfg.avatar_color = "red".into();
        assert_eq!(off, MemoryPolicy::of_config(&cfg), "D60 e cosméticos não viram permissão");
        cfg.backend = "copilot".into();
        assert!(MemoryPolicy::of_config(&cfg).harness);
        cfg.harness_memory = false;
        assert!(!MemoryPolicy::of_config(&cfg).harness);
        cfg.context = Some(crate::fork::spaces::ExecutionContext {
            conversation_id: "conversa".into(),
            memory_id: "memoria".into(),
            foreign_identity: false,
        });
        cfg.shared_memory = None;
        let default_on = MemoryPolicy::of_config(&cfg);
        cfg.shared_memory = Some(true);
        assert_eq!(default_on, MemoryPolicy::of_config(&cfg), "None usa o default canônico contextual");
        cfg.shared_memory = Some(false);
        assert_ne!(default_on, MemoryPolicy::of_config(&cfg));
    }
}
