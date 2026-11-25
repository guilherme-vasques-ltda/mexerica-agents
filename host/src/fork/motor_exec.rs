//! Fronteira Unix dos processos Motor. A ausência da configuração conserva o fluxo legado.
use anyhow::Result;
use std::path::Path;
use tokio::process::Command;

mod child;
pub(crate) mod criador;
#[cfg(all(test, unix))]
pub(crate) use child::fixture;
#[cfg(unix)]
pub(crate) use child::start_open_codex;
#[cfg(unix)]
pub(crate) mod fd;
#[cfg(unix)]
pub(crate) mod login_cleanup;
pub(crate) mod read;
pub(crate) use child::{Child, Group, output, spawn};

pub(crate) const CONFIG: &str = "/etc/donnabot/motor.conf";
pub(crate) const HELPER: &str = "/opt/donnabot/bin/donnabot-motor-exec";
pub(crate) const HELPER_GERIDO: &str = "/opt/donnabot/releases/ativo/donnabot-motor-exec";

pub(crate) fn helper_path() -> std::path::PathBuf {
    if crate::environment::Environment::is_open() {
        return std::path::PathBuf::new();
    }
    #[cfg(test)]
    if let Some(path) = std::env::var_os("DONNABOT_MOTOR_TEST_HELPER") {
        return path.into();
    }
    // Instalação da atualização privilegiada: o ajudante autenticado fica no par ativo.
    let gerido = Path::new(HELPER_GERIDO);
    if std::fs::symlink_metadata("/opt/donnabot/releases/ativo").is_ok() && gerido.exists() {
        return gerido.into();
    }
    HELPER.into()
}

/// Builds de release recusam o modo separado até existir a separação de UID entre quem grava
/// a instalação e quem executa o agente (ver limitações do PR do registro de versões).
///
/// A recusa é explícita: uma instalação migrada, com configuração de motor presente, nunca
/// cai silenciosamente no UID do Hub nem num legado disfarçado.
pub(crate) fn modo_separado_proibido(ambiente: crate::environment::Environment, configurado: bool) -> bool {
    matches!(ambiente, crate::environment::Environment::Main) && configurado
}

pub(crate) fn exigir_modo_suportado_em(ambiente: crate::environment::Environment, configurado: bool) -> Result<()> {
    anyhow::ensure!(
        !modo_separado_proibido(ambiente, configurado),
        "modo de motor separado indisponível neste build; use o modo local ou uma build de desenvolvimento"
    );
    Ok(())
}

/// Fronteira de lançamento, instalação e terminal: recusa antes de qualquer caminho.
pub(crate) fn exigir_modo_suportado() -> Result<()> {
    exigir_modo_suportado_em(crate::environment::Environment::current(), modo_separado_configurado())
}

/// Configuração de motor separado presente na máquina (só consulta metadados).
pub(crate) fn modo_separado_configurado() -> bool {
    cfg!(target_os = "linux") && configured_at(Path::new(CONFIG))
}

pub(crate) fn configured() -> bool {
    if crate::environment::Environment::is_open() {
        return false;
    }
    #[cfg(test)]
    if std::env::var_os("DONNABOT_MOTOR_TEST_HELPER").is_some() {
        return true;
    }
    cfg!(target_os = "linux") && configured_at(Path::new(CONFIG))
}

fn configured_at(path: &Path) -> bool {
    // Só ENOENT desliga o modo; configuração ilegível/insegura falha no ajudante.
    !matches!(std::fs::symlink_metadata(path), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
}

pub(crate) fn command(program: impl AsRef<std::ffi::OsStr>, cwd: &Path) -> Command {
    let program = program.as_ref();
    if crate::fork::motor_install::worker() {
        let mut command = Command::new(program);
        command.current_dir(cwd);
        return command;
    }
    let resolved = if configured() && !Path::new(program).is_absolute() {
        program.to_str().map(crate::shell::program)
    } else {
        None
    };
    command_with(resolved.as_deref().map_or(program, Path::as_os_str), launch_cwd(cwd), configured(), &helper_path())
}

/// Defaults técnicos do Hub não são pastas de trabalho acessíveis ao motor. Também é a pasta que o
/// doutor de skills sonda: um só lugar decide onde o motor realmente roda.
pub(crate) fn launch_cwd(cwd: &Path) -> &Path {
    launch_cwd_em(cwd, configured(), &crate::service::data_dir(), dirs::home_dir().as_deref())
}

fn launch_cwd_em<'a>(cwd: &'a Path, separate: bool, data_dir: &Path, home: Option<&Path>) -> &'a Path {
    if separate && (cwd == data_dir || home == Some(cwd)) { Path::new(crate::fork::motor_roots::MOTOR) } else { cwd }
}

fn command_with(program: &std::ffi::OsStr, cwd: &Path, separate: bool, helper: &Path) -> Command {
    if separate {
        let mut command = Command::new(helper);
        command.args([std::ffi::OsStr::new("--cwd"), cwd.as_os_str(), std::ffi::OsStr::new("--"), program]);
        command
    } else {
        Command::new(program)
    }
}

/// Nunca atravessa no Hub o cwd privado do motor.
pub(crate) fn hub_cwd(cwd: &Path) -> &Path {
    if configured() { Path::new("/") } else { cwd }
}

pub(crate) fn refuse(operation: &str) -> Result<()> {
    anyhow::ensure!(!configured(), "modo de motor separado: {operation} indisponível até a migração da raiz do motor");
    Ok(())
}

pub(crate) fn kill_group(pid: u32) -> std::io::Result<()> {
    crate::open_hub::require_executor()?;
    let status = std::process::Command::new(helper_path()).args(["kill-group", &pid.to_string()]).status()?;
    if status.success() { Ok(()) } else { Err(std::io::Error::other("ajudante não encerrou o grupo do motor")) }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{io::Read, os::unix::fs::PermissionsExt};
    #[test]
    fn instalacao_migrada_em_release_e_recusada_sem_fallback() {
        use crate::environment::Environment;
        // Configuração presente em build de release: recusa explícita, não modo ausente.
        assert!(modo_separado_proibido(Environment::Main, true));
        assert!(exigir_modo_suportado_em(Environment::Main, true).is_err());
        // Release sem configuração e build de desenvolvimento seguem como antes.
        assert!(!modo_separado_proibido(Environment::Main, false));
        assert!(exigir_modo_suportado_em(Environment::Main, false).is_ok());
        assert!(exigir_modo_suportado_em(Environment::Dev, true).is_ok());
        assert!(exigir_modo_suportado_em(Environment::Open, true).is_ok());
    }

    #[test]
    fn partida_da_candidata_em_release_com_modo_separado_recusa() {
        use crate::environment::Environment;
        // É a decisão que `serve` aplica na partida: release com a configuração do
        // modo separado presente não inicia; sem ela, ou em desenvolvimento, inicia.
        assert!(exigir_modo_suportado_em(Environment::Main, true).is_err());
        assert!(exigir_modo_suportado_em(Environment::Main, false).is_ok());
        assert!(exigir_modo_suportado_em(Environment::Dev, true).is_ok());
    }

    #[test]
    fn pasta_de_lancamento_troca_home_e_dados_so_no_modo_separado() {
        let (dados, home, bot) = (Path::new("/dados"), Path::new("/home/user"), Path::new("/home/user/bot"));
        let motor = Path::new(crate::fork::motor_roots::MOTOR);
        assert_eq!(launch_cwd_em(home, true, dados, Some(home)), motor);
        assert_eq!(launch_cwd_em(dados, true, dados, Some(home)), motor);
        assert_eq!(launch_cwd_em(bot, true, dados, Some(home)), bot);
        assert_eq!(launch_cwd_em(home, false, dados, Some(home)), home);
    }
    #[tokio::test]
    async fn ausente_preserva_comando_e_separado_chama_ajudante() {
        let dir = std::env::temp_dir().join(format!("motor-exec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        assert!(!configured_at(&dir.join("ausente")));
        let legacy = command_with("/bin/echo".as_ref(), &dir, false, Path::new("/nao-existe"));
        assert_eq!(legacy.as_std().get_program(), "/bin/echo");
        // Ajudante fictício devolve argv e ambiente, sem precisar de root.
        let helper = dir.join("helper");
        std::fs::write(&helper, "#!/bin/sh\nprintf '%s\\n' \"$@\" \"$MOTOR_TEST\"\n").unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
        let output = command_with("/bin/echo".as_ref(), &dir, true, &helper)
            .arg("argumento")
            .env_clear()
            .env("MOTOR_TEST", "explicito")
            .output()
            .await
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!("--cwd\n{}\n--\n/bin/echo\nargumento\nexplicito\n", dir.display())
        );
        // O PTY legado e terminal-auth usam este mesmo construtor. Um ajudante que
        // recusa não pode causar fallback e executar o comando pedido como Hub.
        let sentinel = dir.join("executou-como-hub");
        std::fs::write(&helper, "#!/bin/sh\necho recusado-pelo-ajudante\nexit 1\n").unwrap();
        let pair = portable_pty::native_pty_system().openpty(portable_pty::PtySize::default()).unwrap();
        let mut builder = pty_with("/bin/sh".as_ref(), &dir, true, &helper);
        builder.args(["-c", &format!("touch {}", sentinel.display())]);
        let mut child = pair.slave.spawn_command(builder).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        assert!(!child.wait().unwrap().success());
        let mut bytes = [0; 256];
        let count = reader.read(&mut bytes).unwrap();
        assert!(String::from_utf8_lossy(&bytes[..count]).contains("recusado-pelo-ajudante"));
        assert!(!sentinel.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

pub(crate) fn pty(program: impl AsRef<std::ffi::OsStr>, cwd: &Path) -> std::io::Result<portable_pty::CommandBuilder> {
    crate::open_hub::require_executor()?;
    exigir_modo_suportado().map_err(std::io::Error::other)?;
    Ok(pty_with(program.as_ref(), launch_cwd(cwd), configured(), &helper_path()))
}

fn pty_with(program: &std::ffi::OsStr, cwd: &Path, separate: bool, helper: &Path) -> portable_pty::CommandBuilder {
    let mut builder = if separate {
        let mut builder = portable_pty::CommandBuilder::new(helper);
        builder.args([std::ffi::OsStr::new("--cwd"), cwd.as_os_str(), std::ffi::OsStr::new("--"), program]);
        builder
    } else {
        portable_pty::CommandBuilder::new(program)
    };
    builder.cwd(if separate { Path::new("/") } else { cwd });
    builder
}

#[cfg(all(test, unix))]
pub(crate) mod test_support;
