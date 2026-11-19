//! Recursos especulativos da inclusão: só diretórios novos e vazios podem ser removidos.
use crate::store::BotConfig;
use anyhow::{Context, Result, ensure};
use std::{fs, path::PathBuf};

#[derive(Default)]
pub(crate) struct PreparedWorkspace {
    created: Vec<PathBuf>,
}

impl PreparedWorkspace {
    /// Invocado pela fábrica MAIN depois de emitir o UUID, antes de persistir.
    pub(crate) fn prepare(&mut self, cfg: &mut BotConfig) -> Result<()> {
        ensure!(!cfg.is_group() && cfg.cwd.is_empty(), "native_workspace_unavailable");
        uuid::Uuid::parse_str(&cfg.id).context("native_workspace_unavailable")?;
        let workspace = crate::agent::workspace::path(&cfg.id);
        let bot = workspace.parent().context("native_workspace_unavailable")?;
        // Raiz compartilhada não pertence à operação e nunca entra na lista de limpeza.
        fs::create_dir_all(bot.parent().context("native_workspace_unavailable")?)?;
        // create_dir é exclusivo: diretório pré-existente (mesmo vazio) nunca é adotado.
        fs::create_dir(bot).context("criando diretório do bot nativo")?;
        self.created.push(bot.to_owned());
        fs::create_dir(&workspace).context("criando workspace do bot nativo")?;
        self.created.push(workspace);
        // Mesma preparação/normalização canônica; erro também passa pelo Drop.
        crate::agent::workspace::prepare(cfg)
    }

    pub(crate) fn keep(&mut self) {
        self.created.clear();
    }
}

impl Drop for PreparedWorkspace {
    fn drop(&mut self) {
        for path in self.created.iter().rev() {
            // Sem remove_dir_all: conteúdo concorrente/do dono impede a remoção.
            if let Err(error) = fs::remove_dir(path)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                tracing::debug!(%error, "diretório especulativo preservado; limpeza somente se vazio");
            }
        }
    }
}
