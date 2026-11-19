//! Fachada de leitura sem migração: DB é solto antes do FS; não inicializa catálogo/locks.
use super::{State, entrega, installation, storage};
use crate::{market::Skill, store::Store};
use anyhow::Result;
use rusqlite::Connection;

pub(crate) const WARNING: &str = "Skills indisponíveis neste preparo: a observação original não pôde ser confirmada. Não procure seus arquivos; avise o usuário se precisar delas. Caminhos já recebidos continuam mutáveis.";

pub(crate) struct Observed {
    state: State,
    items: Vec<(Skill, Option<String>)>,
    missing: bool,
}
impl Observed {
    pub(crate) fn read(c: &Connection, enabled: &[String]) -> Result<Self> {
        let (all, state) = storage::current_on(c)?;
        let missing = enabled.iter().any(|id| !all.iter().any(|s| &s.id == id));
        Ok(Self {
            state,
            items: all.into_iter().filter(|s| enabled.contains(&s.id)).map(|s| (s, None)).collect(),
            missing,
        })
    }
    /// Só fora do lock DB. Nenhum corpo em logs ou saída ao motor.
    pub(crate) fn read_documents(&mut self, store: &Store) {
        for (skill, bytes) in &mut self.items {
            *bytes = installation::checked_document(store, skill, &self.state).ok();
        }
    }
    pub(crate) fn check(&self, c: &Connection) -> Result<()> {
        // Além da comparação bruta, conservar as validações estruturais do leitor canônico.
        storage::current_on(c)?;
        if let Some(observed) = &self.state.observed {
            storage::unchanged(c, observed)?;
        }
        Ok(())
    }
    /// Só os bytes conferidos no preparo; nenhum path ou FS nesta leitura.
    pub(crate) fn document(&self, id: &str) -> Result<String> {
        self.items
            .iter()
            .find(|(skill, _)| skill.id == id)
            .and_then(|(_, bytes)| bytes.clone())
            .ok_or_else(|| anyhow::anyhow!("skills_read_unavailable"))
    }
    pub(crate) fn project(&self, store: &Store) -> Option<String> {
        let mut lines = Vec::new();
        let mut unavailable = self.missing;
        for (skill, original) in &self.items {
            let current = installation::checked_document(store, skill, &self.state).ok();
            if original.is_some() && current == *original {
                #[cfg(unix)]
                if crate::fork::motor_exec::configured()
                    && crate::fork::motor_publish::skill(&super::dir(store, skill), &skill.id).is_err()
                {
                    unavailable = true;
                    continue;
                }
                lines.push(entrega::line(store, skill));
            } else {
                unavailable = true;
            }
        }
        let mut text =
            if lines.is_empty() { String::new() } else { format!("{}\n{}", entrega::GUIDANCE, lines.join("\n")) };
        if unavailable {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(WARNING);
        }
        (!text.is_empty()).then_some(text)
    }
}
