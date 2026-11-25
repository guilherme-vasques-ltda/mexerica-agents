#[cfg(any(target_os = "linux", all(test, unix)))]
use std::{fs, io::ErrorKind, path::Path};

#[cfg(target_os = "linux")]
pub(crate) const CONFIG: &str = "/etc/donnabot/motor.conf";

#[cfg(any(target_os = "linux", all(test, unix)))]
pub(crate) fn configurado_em(caminho: &Path) -> bool {
    !matches!(fs::symlink_metadata(caminho), Err(e) if e.kind() == ErrorKind::NotFound)
}

pub(crate) fn exigir_local(configurado: bool) -> anyhow::Result<()> {
    anyhow::ensure!(
        !configurado,
        "motor.conf presente ou inacessível: esta release só aceita o modo local. +         Preserve a instalação atual e consulte o administrador antes de migrar; a configuração não será removida automaticamente."
    );
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn somente_ausencia_comprovada_permite_instalacao_e_atualizacao() {
        let raiz = tempfile::tempdir().unwrap();
        let conf = raiz.path().join("motor.conf");
        assert!(exigir_local(configurado_em(&conf)).is_ok());
        fs::write(&conf, b"configuracao anterior").unwrap();
        let erro = exigir_local(configurado_em(&conf)).unwrap_err();
        assert!(erro.to_string().contains("motor.conf"));
        assert_eq!(fs::read(&conf).unwrap(), b"configuracao anterior");
        assert!(exigir_local(configurado_em(&conf.join("inacessivel"))).is_err());
        fs::remove_file(&conf).unwrap();
        std::os::unix::fs::symlink(raiz.path().join("ausente"), &conf).unwrap();
        assert!(exigir_local(configurado_em(&conf)).is_err());
    }
}
