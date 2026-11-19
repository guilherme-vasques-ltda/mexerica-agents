//! D520-B1: seleção sem leitura de dados, fallback ou migração.
use anyhow::{Result, ensure};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

pub(crate) fn select(default: PathBuf, mut variable: impl FnMut(&str) -> Option<OsString>) -> Result<PathBuf> {
    ensure!(
        variable("DONNABOT_HOME").is_none() && variable("CODYNC_HOME").is_none(),
        "DONNABOT_HOME/CODYNC_HOME não selecionam dados da Mexerica; remova-as e configure MEXERICA_HOME"
    );
    let Some(value) = variable("MEXERICA_HOME") else { return Ok(default) };
    ensure!(!value.is_empty(), "MEXERICA_HOME não pode ser vazio");
    let path = PathBuf::from(value);
    // Relativos conservam o CWD da invocação, antes do spawn de helpers.
    Ok(if path.is_absolute() { path } else { std::env::current_dir()?.join(path) })
}

pub(crate) fn validate_environment() -> Result<()> {
    select(PathBuf::new(), |key| std::env::var_os(key))?;
    Ok(())
}

pub(crate) fn read_text(path: &Path) -> Result<String> {
    #[cfg(unix)]
    {
        let bytes =
            crate::fork::private_fs::read(path)?.ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
        Ok(String::from_utf8(bytes)?)
    }
    #[cfg(not(unix))]
    Ok(std::fs::read_to_string(path)?)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    #[test]
    fn defaults_override_and_legacy_conflicts_do_not_read_data() {
        let default = PathBuf::from("/synthetic/home/usuario");
        assert_eq!(select(default.clone(), |_| None).unwrap(), default);
        assert_eq!(
            select(default.clone(), |key| (key == "MEXERICA_HOME").then(|| "/synthetic/override".into())).unwrap(),
            PathBuf::from("/synthetic/override")
        );
        for old in ["DONNABOT_HOME", "CODYNC_HOME"] {
            for value in ["", "/synthetic/old", "/synthetic/override"] {
                assert!(select(default.clone(), |key| { if key == old { Some(value.into()) } else { None } }).is_err());
                assert!(
                    select(default.clone(), |key| {
                        if key == old || key == "MEXERICA_HOME" { Some(value.into()) } else { None }
                    })
                    .is_err()
                );
            }
        }
        assert!(select(default, |key| (key == "MEXERICA_HOME").then(OsString::new)).is_err());
    }
}
