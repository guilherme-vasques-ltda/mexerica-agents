//! A escolha da pasta pela ferramenta é restrita; isso não é um sandbox do shell (D62).
use crate::store::{BotConfig, TeamSettings};
use anyhow::{Context, Result, bail};
use std::path::{Component, Path, PathBuf};

pub(super) fn root(settings: &TeamSettings) -> Result<PathBuf> {
    if crate::fork::motor_exec::configured() {
        return Ok(crate::fork::motor_roots::work());
    }
    settings.workspace_root.as_ref().map_or_else(
        || Ok(dirs::home_dir().context("home directory unavailable")?.join("Donnabot/workspaces")),
        |root| Ok(PathBuf::from(root)),
    )
}

pub(super) fn choose(
    settings: &TeamSettings,
    source: &BotConfig,
    requested: Option<&str>,
) -> Result<(PathBuf, PathBuf)> {
    let root = crate::fork::pasta::validar_criacao(&root(settings)?)?;
    let folder = requested.map_or_else(|| root.join(uuid::Uuid::new_v4().to_string()), PathBuf::from);
    let folder = check(&folder, &root, Path::new(&source.cwd), &crate::service::data_dir())?;
    Ok((folder, root))
}

/// Só remove a folha que esta operação criou, e apenas se continuar vazia.
pub(super) struct Created {
    path: Option<PathBuf>,
}

impl Created {
    pub(super) fn new(path: &Path) -> Result<Self> {
        if crate::fork::motor_exec::configured() {
            let existed = path.exists();
            crate::fork::motor_roots::prepare_workspace(path)?;
            return Ok(Self { path: (!existed).then(|| path.to_path_buf()) });
        }
        std::fs::create_dir_all(path.parent().context("A pasta precisa ter um diretório pai.")?)?;
        let path = match std::fs::create_dir(path) {
            Ok(()) => Some(path.to_path_buf()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && path.is_dir() => None,
            Err(error) => return Err(error).context("Não foi possível criar a pasta de trabalho."),
        };
        Ok(Self { path })
    }

    pub(super) fn keep(mut self) {
        self.path = None;
    }
}

impl Drop for Created {
    fn drop(&mut self) {
        if let Some(path) = &self.path
            && let Err(error) = std::fs::remove_dir(path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(%error, "a pasta recém-criada não pôde ser removida vazia após a falha");
        }
    }
}

/// Resolve links também nos ancestrais existentes de uma pasta ainda não criada.
fn resolved(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        bail!("folder must be absolute and must not contain parent traversal");
    }
    match std::fs::canonicalize(path) {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !path.is_symlink() => {
            let parent = path.parent().context("folder has no parent")?;
            Ok(resolved(parent)?.join(path.file_name().context("folder has no name")?))
        }
        Err(error) => Err(error).context("resolving workspace folder"),
    }
}

pub(super) fn check(folder: &Path, root: &Path, primary: &Path, data: &Path) -> Result<PathBuf> {
    if crate::fork::motor_exec::configured() {
        if !folder.starts_with(crate::fork::motor_roots::work())
            || folder.components().any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
        {
            bail!("pasta fora da raiz de trabalho");
        }
        #[cfg(unix)]
        {
            crate::fork::motor_exec::fd::Directory::open(root)?;
        }
    }
    let (folder, root, primary, data) = (resolved(folder)?, resolved(root)?, resolved(primary)?, resolved(data)?);
    if folder == root || !folder.starts_with(&root) {
        bail!("folder must be a subfolder of workspaceRoot (after resolving symlinks)");
    }
    // A principal pode trabalhar em HOME ou ~/Donnabot, ancestrais da raiz.
    // Isso não torna as subpastas da raiz parte do workspace reservado da principal.
    if folder == primary || folder.starts_with(&data) {
        bail!("folder cannot be the primary bot's folder or inside the Mexerica data folder");
    }
    Ok(folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_creation_cleanup_preserves_existing_folders_and_new_files() {
        let dir = std::env::temp_dir().join(format!("donna-folder-cleanup-{}", uuid::Uuid::new_v4()));
        let empty = dir.join("empty");
        drop(Created::new(&empty).unwrap());
        assert!(!empty.exists());
        let existing = dir.join("existing");
        std::fs::create_dir(&existing).unwrap();
        drop(Created::new(&existing).unwrap());
        assert!(existing.is_dir());
        let with_files = dir.join("with-files");
        let created = Created::new(&with_files).unwrap();
        std::fs::write(with_files.join("output"), "preservar").unwrap();
        drop(created);
        assert_eq!(std::fs::read_to_string(with_files.join("output")).unwrap(), "preservar");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reserves_the_entire_data_tree_when_workspace_root_is_an_ancestor() {
        let dir = std::env::temp_dir().join(format!("d67-data-{}", uuid::Uuid::new_v4()));
        let data = dir.join(".donnabot");
        let primary = dir.join("primary");
        std::fs::create_dir_all(data.join("bots/existing")).unwrap();
        std::fs::create_dir_all(&primary).unwrap();
        for folder in [&data, &data.join("bots/existing"), &data.join("bots/new/nested")] {
            assert!(check(folder, &dir, &primary, &data).is_err(), "{folder:?}");
        }
        assert!(check(&dir.join(".donnabot-backup/new"), &dir, &primary, &data).is_ok());
        #[cfg(unix)]
        {
            let alias = dir.join("data-alias");
            std::os::unix::fs::symlink(&data, &alias).unwrap();
            assert!(check(&alias.join("bots/new/nested"), &dir, &primary, &data).is_err());
            assert!(check(&data.join("bots/new/nested"), &dir, &primary, &alias).is_err());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn confines_folders_and_resolves_symlinks_before_checking() {
        let dir = std::env::temp_dir().join(format!("d67-folder-{}", uuid::Uuid::new_v4()));
        let root = dir.join("workspaces");
        let primary = dir.join("primary");
        let data = dir.join(".donnabot");
        for p in [&root, &primary, &data] {
            std::fs::create_dir_all(p).unwrap();
        }
        assert!(check(&root.join("new"), &root, &primary, &data).is_ok());
        for bad in
            [&dir, &root, &primary, &data, &data.join("bots/new"), &root.join("../primary"), Path::new("relative")]
        {
            assert!(check(bad, &root, &primary, &data).is_err(), "{bad:?}");
        }
        #[cfg(unix)]
        {
            for (name, target) in [("outside", &dir), ("primary", &primary), ("data", &data)] {
                let link = root.join(name);
                std::os::unix::fs::symlink(target, &link).unwrap();
                assert!(check(&link, &root, &primary, &data).is_err());
                assert!(check(&link.join("new"), &root, &primary, &data).is_err());
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn workspace_root_children_are_allowed_when_primary_is_an_ancestor() {
        let dir = std::env::temp_dir().join(format!("d67-parent-{}", uuid::Uuid::new_v4()));
        let root = dir.join("Donnabot/workspaces");
        let data = dir.join(".donnabot");
        std::fs::create_dir_all(&root).unwrap();
        for primary in [&dir, &dir.join("Donnabot"), &root] {
            assert!(check(&root.join("new"), &root, primary, &data).is_ok());
            assert!(check(primary, &root, primary, &data).is_err());
        }
        for protected in [&root.join("primary"), &root.join("data")] {
            assert!(check(protected, &root, protected, &data).is_err());
            assert!(check(protected, &root, &dir, protected).is_err());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
