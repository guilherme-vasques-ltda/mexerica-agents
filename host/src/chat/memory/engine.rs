//! Memória legível pelos motores, sem concessão de escrita (D125).

use crate::store::BotConfig;
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::path::Path;

pub fn check_workspace(cfg: &BotConfig) -> Result<()> {
    let cwd = std::fs::canonicalize(&cfg.cwd)?;
    let mut folders = super::files::folders(cfg)?;
    folders.push(super::files::folder(None));
    for folder in folders {
        std::fs::create_dir_all(&folder)?;
        let folder = std::fs::canonicalize(folder)?;
        if folder.starts_with(&cwd) || cwd.starts_with(&folder) {
            bail!("A pasta de trabalho não pode conter memória nem ficar dentro dela. Escolha outra pasta.");
        }
    }
    Ok(())
}

fn rule(tool: &str, folder: &Path) -> String {
    format!("{tool}(//{}/**)", folder.display().to_string().trim_start_matches('/'))
}

pub fn protect_claude(meta: &mut Value, cfg: &BotConfig) {
    let permissions = &mut meta["claudeCode"]["options"]["settings"]["permissions"];
    for layer in super::files::layers(cfg) {
        if let Some(allow) = permissions["allow"].as_array_mut() {
            allow.push(json!(rule("Read", &super::files::folder(layer.as_deref()))));
        }
    }
    let mut deny = Vec::new();
    for layer in [Some(cfg.memory_id()), None] {
        for tool in ["Edit", "Write", "NotebookEdit"] {
            deny.push(rule(tool, &super::files::folder(layer)));
        }
    }
    permissions["deny"] = json!(deny);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_cannot_become_the_writable_workspace_even_when_shared_memory_is_off() {
        let id = uuid::Uuid::new_v4().to_string();
        let workspace = super::super::files::folder(Some(&id)).parent().unwrap().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let mut cfg: BotConfig =
            serde_json::from_value(json!({"id":id,"name":"synthetic","cwd":workspace,"sharedMemory":false})).unwrap();
        assert!(check_workspace(&cfg).is_ok());
        for folder in [super::super::files::folder(Some(&id)), super::super::files::folder(None)] {
            cfg.cwd = folder.to_string_lossy().into_owned();
            assert!(check_workspace(&cfg).is_err());
        }
        std::fs::remove_dir_all(workspace.parent().unwrap()).unwrap();
    }
}
