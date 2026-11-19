//! D520-B1: CLI e Hub com HOME/arquivos sintéticos; sem SSH, serviço ou cofre reais.
use std::{path::Path, process::Command};

fn info(home: &Path, variables: &[(&str, &Path)]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_donnabot-host"));
    command.env_clear().env("HOME", home).args(["info", "--json"]);
    for (key, value) in variables {
        command.env(key, value);
    }
    command.output().expect("não foi possível executar o subprocesso sintético do teste de CLI")
}

#[test]
fn d520_cli_refuses_legacy_variables_paths_and_aliases_before_reading() {
    let root = std::env::temp_dir().join(format!("d520-cli-{}", uuid::Uuid::new_v4()));
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    for family in [".codync", ".donnabot", ".DONNABOT-dev"] {
        let old = home.join(family);
        std::fs::create_dir(&old).unwrap();
        std::fs::write(old.join("identity.json"), b"SENTINELA_SINTETICA_NAO_INTERPRETAR").unwrap();
        let selected = root.join("novo");
        for variables in [
            vec![("MEXERICA_HOME", old.as_path())],
            vec![("DONNABOT_HOME", old.as_path())],
            vec![("CODYNC_HOME", old.as_path()), ("MEXERICA_HOME", selected.as_path())],
        ] {
            let output = info(&home, &variables);
            assert!(!output.status.success());
            assert_eq!(output.stdout, [] as [u8; 0]);
            assert!(String::from_utf8_lossy(&output.stderr).contains("MEXERICA_HOME"));
            assert!(!selected.exists());
            assert_eq!(std::fs::read(old.join("identity.json")).unwrap(), b"SENTINELA_SINTETICA_NAO_INTERPRETAR");
            assert_eq!(std::fs::read_dir(&old).unwrap().count(), 1);
        }
    }
    #[cfg(unix)]
    {
        let outside = root.join("externa");
        std::fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, home.join(".donnabot-backup-alias")).unwrap();
        let output = info(&home, &[("MEXERICA_HOME", outside.as_path())]);
        assert!(!output.status.success());
        assert_eq!(output.stdout, [] as [u8; 0]);
        assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
    }
    std::fs::remove_dir_all(root).unwrap();
}
