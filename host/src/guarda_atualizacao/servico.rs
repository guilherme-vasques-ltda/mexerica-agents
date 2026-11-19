//! Parada de unidade de usuário pelo guard root, nunca pelo wrapper.
use anyhow::{Result, ensure};
use std::{
    ffi::{CStr, CString},
    fs,
    io::Read,
    os::unix::fs::MetadataExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn login_valido(s: &str) -> bool {
    let mut bytes = s.bytes();
    s.len() <= 32
        && bytes.next().is_some_and(|b| b.is_ascii_lowercase() || b == b'_')
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

fn login(uid: u32) -> Result<String> {
    ensure!(uid != 0, "UID do Hub inválido");
    // SAFETY: NSS é consultado em processo monothread e a string é copiada imediatamente.
    let entrada = unsafe { libc::getpwuid(uid) };
    ensure!(!entrada.is_null(), "conta do Hub indisponível");
    // SAFETY: NSS retornou entrada válida; pw_name é uma string terminada em NUL.
    let nome = unsafe { CStr::from_ptr((*entrada).pw_name) }.to_str()?.to_owned();
    ensure!(login_valido(&nome), "login do Hub inválido");
    let texto = CString::new(nome.as_bytes())?;
    // SAFETY: ponteiro da string é válido e a resposta é conferida imediatamente.
    let reverso = unsafe { libc::getpwnam(texto.as_ptr()) };
    ensure!(!reverso.is_null(), "conta do Hub indisponível");
    // SAFETY: entrada NSS não nula, lida antes de outra consulta NSS.
    ensure!(unsafe { (*reverso).pw_uid } == uid, "login do Hub não corresponde ao UID");
    Ok(nome)
}

fn conferir_systemctl() -> Result<()> {
    for path in ["/usr", "/usr/bin", "/usr/bin/systemctl"] {
        let m = fs::symlink_metadata(path)?;
        ensure!(
            m.uid() == 0 && m.gid() == 0 && m.mode() & 0o7022 == 0 && !m.file_type().is_symlink(),
            "systemctl ou ancestral inseguro"
        );
        if path == "/usr/bin/systemctl" {
            ensure!(m.is_file() && m.nlink() == 1, "systemctl inválido");
        } else {
            ensure!(m.is_dir(), "ancestral do systemctl inválido");
        }
    }
    Ok(())
}

/// Somente o estado explícito de desligamento permite evitar emergência no reboot.
pub fn sistema_encerrando() -> bool {
    if conferir_systemctl().is_err() {
        return false;
    }
    let Ok(mut filho) = Command::new("/usr/bin/systemctl")
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .args(["--no-pager", "--no-ask-password", "is-system-running"])
        .spawn()
    else {
        return false;
    };
    let limite = Instant::now() + Duration::from_secs(3);
    loop {
        match filho.try_wait() {
            Ok(Some(_)) => {
                let Some(saida) = filho.stdout.take() else { return false };
                let mut bytes = Vec::new();
                if saida.take(65).read_to_end(&mut bytes).is_err() {
                    return false;
                }
                return bytes == b"stopping\n";
            }
            Ok(None) if Instant::now() < limite => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = filho.kill();
                let _ = filho.wait();
                return false;
            }
        }
    }
}

/// Solicita parada sem bloquear. O chamador deve conferir o término dos filhos.
/// Unidade inexistente ou não carregada no gerenciador do Hub (instalação limpa,
/// antes da primeira unidade) não tem nada a parar e conta como sucesso.
pub fn solicitar_parada(uid: u32) -> Result<()> {
    let nome = login(uid)?;
    conferir_systemctl()?;
    let filho = Command::new("/usr/bin/systemctl")
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .arg("--user")
        .arg(format!("--machine={nome}@.host"))
        .args(["--no-block", "--no-pager", "--no-ask-password", "stop", "donnabot-host.service"])
        .spawn()?;
    concluir_parada(filho, Duration::from_secs(10))
}

/// Saída do `systemctl stop` de uma unidade que o gerenciador não conhece.
const NAO_CARREGADA: u8 = 5;

/// Aguarda o `systemctl` no prazo; só o próprio filho é encerrado no estouro.
fn concluir_parada(mut filho: std::process::Child, prazo: Duration) -> Result<()> {
    let limite = Instant::now() + prazo;
    loop {
        if let Some(resultado) = filho.try_wait()? {
            let mut erro = Vec::new();
            if let Some(saida) = filho.stderr.take() {
                // Leitura limitada: só a primeira linha de diagnóstico interessa.
                let _ = saida.take(4096).read_to_end(&mut erro);
            }
            return interpretar_parada(resultado.code(), &erro);
        }
        if Instant::now() >= limite {
            let _ = filho.kill();
            let _ = filho.wait();
            anyhow::bail!("pedido de parada excedeu o prazo");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Idempotente: unidade não carregada (saída 5 com o diagnóstico do systemd) não
/// tem processo a parar. Qualquer outra falha continua recusando a parada.
fn interpretar_parada(codigo: Option<i32>, erro: &[u8]) -> Result<()> {
    if codigo == Some(0) {
        return Ok(());
    }
    let texto = String::from_utf8_lossy(erro);
    if codigo == Some(i32::from(NAO_CARREGADA)) && texto.contains("donnabot-host.service not loaded") {
        return Ok(());
    }
    let linha: String = texto.lines().next().unwrap_or_default().chars().take(200).collect();
    anyhow::bail!("pedido de parada do serviço recusado (saída {codigo:?}): {linha}")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn systemctl_falso(script: &str) -> Result<std::process::Child> {
        Ok(Command::new("/bin/sh")
            .args(["-c", script])
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?)
    }

    /// Instalação limpa: o gerenciador do Hub ainda não conhece a unidade.
    #[test]
    fn parada_de_unidade_nao_carregada_e_sucesso() -> Result<()> {
        let filho = systemctl_falso(
            "echo 'Failed to stop donnabot-host.service: Unit donnabot-host.service not loaded.' >&2; exit 5",
        )?;
        concluir_parada(filho, Duration::from_secs(10))?;
        concluir_parada(systemctl_falso("exit 0")?, Duration::from_secs(10))
    }

    #[test]
    fn outras_falhas_da_parada_continuam_recusadas() -> Result<()> {
        for script in [
            "echo 'Failed to connect to bus: No medium found' >&2; exit 1",
            "echo 'Failed to stop donnabot-host.service: Access denied' >&2; exit 4",
            // Saída 5 sem o diagnóstico de unidade não carregada não basta.
            "echo 'Failed to stop donnabot-host.service: Unit is masked.' >&2; exit 5",
            "exit 5",
            "kill -9 $$",
        ] {
            assert!(concluir_parada(systemctl_falso(script)?, Duration::from_secs(10)).is_err(), "{script}");
        }
        assert!(concluir_parada(systemctl_falso("sleep 5")?, Duration::from_millis(100)).is_err());
        Ok(())
    }

    #[test]
    fn login_nao_permite_opcoes_ou_metacaracteres() {
        assert!(login_valido("hublogin"));
        for nome in ["", "-root", "hub@host", "hub name", "hub\nroot", "hub/abc"] {
            assert!(!login_valido(nome));
        }
    }
}
