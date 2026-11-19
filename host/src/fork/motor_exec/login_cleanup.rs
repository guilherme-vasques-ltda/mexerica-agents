//! Remoção de um perfil de login identificado, ancorada em descritores sem seguir links.
use super::fd::{self, Directory};
use anyhow::{Result, ensure};
use std::{
    ffi::OsStr,
    io::{Read, Write},
    os::{fd::AsRawFd, unix::ffi::OsStrExt},
    path::Path,
};

fn unlink(dir: &Directory, name: &OsStr, directory: bool) -> Result<()> {
    let name = std::ffi::CString::new(name.as_bytes())?;
    // SAFETY: nome de um componente enumerado, relativo ao diretório aberto.
    let result =
        unsafe { libc::unlinkat(dir.0.as_raw_fd(), name.as_ptr(), if directory { libc::AT_REMOVEDIR } else { 0 }) };
    ensure!(result == 0, "remoção de perfil recusada: {}", std::io::Error::last_os_error());
    Ok(())
}
/// Tipo da entrada sem seguir link: `st_mode & S_IFMT`.
fn kind(dir: &Directory, name: &OsStr) -> Result<libc::mode_t> {
    let name = std::ffi::CString::new(name.as_bytes())?;
    // SAFETY: stat é preenchido pelo kernel; AT_SYMLINK_NOFOLLOW não segue a folha.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: nome de um componente enumerado, relativo ao diretório aberto.
    let result = unsafe { libc::fstatat(dir.0.as_raw_fd(), name.as_ptr(), &raw mut stat, libc::AT_SYMLINK_NOFOLLOW) };
    ensure!(result == 0, "entrada do perfil ilegível: {}", std::io::Error::last_os_error());
    Ok(stat.st_mode & libc::S_IFMT)
}
const MARKER: &str = ".donnabot-login-operation";
fn contents(dir: &Directory, preserve_marker: bool) -> Result<()> {
    fd::own(&dir.0)?;
    for name in dir.names()? {
        // O marcador é a prova que autoriza repetir a limpeza: ele sai por último, já no
        // `remove`, nunca no meio da varredura — uma falha aqui não pode órfã a pasta.
        if preserve_marker && name.as_os_str() == OsStr::new(MARKER) {
            continue;
        }
        match kind(dir, &name)? {
            libc::S_IFDIR => {
                contents(&dir.child(&name, None)?, false)?;
                unlink(dir, &name, true)?;
            }
            libc::S_IFREG => {
                let file = dir.file(&name, libc::O_RDONLY, 0)?;
                fd::own(&file)?;
                unlink(dir, &name, false)?;
            }
            // Link (por exemplo, de auto-update do CLI) ou arquivo especial: remove-se a própria
            // entrada, nunca o destino, e sem abrir nada.
            _ => unlink(dir, &name, false)?,
        }
    }
    Ok(())
}
pub(crate) fn count_unmarked(path: &Path) -> Result<usize> {
    let dir = Directory::open(path)?;
    fd::own(&dir.0)?;
    let mut count = 0;
    for name in dir.names()? {
        // Só pastas de verdade contam; link ou pasta ilegível não derruba o aviso.
        if let Ok(child) = dir.child(&name, None)
            && (fd::own(&child.0).is_err()
                || child.file(OsStr::new(".donnabot-login-operation"), libc::O_RDONLY, 0).is_err())
        {
            count += 1;
        }
    }
    Ok(count)
}
fn marker(dir: &Directory, operation: &str, deadline: &str) -> Result<String> {
    let file = dir.file(OsStr::new(".donnabot-login-operation"), libc::O_RDONLY, 0)?;
    fd::own(&file)?;
    let mut text = String::new();
    file.take(256).read_to_string(&mut text)?;
    ensure!(
        text == format!("{operation} {deadline}\n") || text == format!("{operation} {deadline} cancelado\n"),
        "marcador de operação não confere"
    );
    Ok(text)
}
pub(crate) fn cancel(path: &Path, deadline: &str) -> Result<()> {
    let operation = path.file_name().and_then(OsStr::to_str).ok_or_else(|| anyhow::anyhow!("operação inválida"))?;
    ensure!(uuid::Uuid::parse_str(operation).is_ok(), "operação inválida");
    let dir = Directory::open(path)?;
    fd::own(&dir.0)?;
    marker(&dir, operation, deadline)?;
    let mut file = dir.file(OsStr::new(".donnabot-login-operation"), libc::O_WRONLY, 0)?;
    fd::own(&file)?;
    file.set_len(0)?;
    writeln!(file, "{operation} {deadline} cancelado")?;
    file.sync_all()?;
    Ok(())
}
/// Apaga um perfil promovido que foi substituído: lê o prazo do próprio marcador (que precisa
/// pertencer à pasta) e remove sem seguir links.
pub(crate) fn remove_replaced(path: &Path) -> Result<()> {
    let name = path.file_name().ok_or_else(|| anyhow::anyhow!("perfil inválido"))?;
    let operation = name.to_str().ok_or_else(|| anyhow::anyhow!("operação inválida"))?;
    ensure!(uuid::Uuid::parse_str(operation).is_ok(), "operação inválida");
    let parent = Directory::open(path.parent().ok_or_else(|| anyhow::anyhow!("pai ausente"))?)?;
    fd::own(&parent.0)?;
    let dir = parent.child(name, None)?;
    fd::own(&dir.0)?;
    let file = match dir.file(OsStr::new(MARKER), libc::O_RDONLY, 0) {
        Ok(file) => file,
        // Interrupção entre apagar o marcador (último arquivo) e apagar a pasta: pasta
        // vazia sem marcador já teve o conteúdo removido e pode simplesmente sumir.
        // Com qualquer outra entrada, sem marcador não há prova e nada é tocado.
        Err(error) => {
            ensure!(dir.names()?.is_empty(), "marcador de operação ausente: {error:#}");
            return unlink(&parent, name, true);
        }
    };
    fd::own(&file)?;
    let mut text = String::new();
    file.take(256).read_to_string(&mut text)?;
    drop(dir);
    drop(parent);
    let mut fields = text.split_whitespace();
    let deadline = match (fields.next(), fields.next()) {
        (Some(op), Some(deadline)) if op == operation && deadline.bytes().all(|b| b.is_ascii_digit()) => deadline,
        _ => anyhow::bail!("marcador de operação não confere"),
    };
    remove(path, deadline, false)
}
pub(crate) fn remove(path: &Path, deadline: &str, require_cancelled: bool) -> Result<()> {
    let name = path.file_name().ok_or_else(|| anyhow::anyhow!("perfil inválido"))?;
    let operation = name.to_str().ok_or_else(|| anyhow::anyhow!("operação inválida"))?;
    ensure!(uuid::Uuid::parse_str(operation).is_ok(), "operação inválida");
    let parent = Directory::open(path.parent().ok_or_else(|| anyhow::anyhow!("pai ausente"))?)?;
    fd::own(&parent.0)?;
    let dir = parent.child(name, None)?;
    fd::own(&dir.0)?;
    // Mesma interrupção do `remove_replaced`, no caminho com prazo conhecido: pasta vazia
    // sem marcador só pode ser o resto da remoção anterior. Cancelamento exige a marca.
    if !require_cancelled && dir.file(OsStr::new(MARKER), libc::O_RDONLY, 0).is_err() && dir.names()?.is_empty() {
        return unlink(&parent, name, true);
    }
    let text = marker(&dir, operation, deadline)?;
    ensure!(
        !require_cancelled || text == format!("{operation} {deadline} cancelado\n"),
        "operação não marcada como cancelada"
    );
    contents(&dir, true)?;
    unlink(&dir, OsStr::new(MARKER), false)?;
    unlink(&parent, name, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nao_segue_link_dentro_do_perfil() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!("login-cleanup-{}", uuid::Uuid::new_v4()));
        let operation = uuid::Uuid::new_v4().to_string();
        let profile = root.join(&operation);
        let outside = root.join("fora");
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("credencial"), "preservada").unwrap();
        std::fs::write(profile.join(".donnabot-login-operation"), format!("{operation} 1\n")).unwrap();
        symlink(&outside, profile.join("link")).unwrap();
        remove(&profile, "1", false).unwrap();
        assert!(!profile.exists());
        assert!(outside.join("credencial").exists());
        assert_eq!(std::fs::read_to_string(outside.join("credencial")).unwrap(), "preservada");
        std::fs::remove_dir_all(root).unwrap();
    }
    fn perfil(root: &Path, operation: &str) -> std::path::PathBuf {
        let profile = root.join(operation);
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(profile.join(".donnabot-login-operation"), format!("{operation} 1\n")).unwrap();
        profile
    }
    #[test]
    fn falha_no_meio_da_remocao_preserva_o_marcador_e_a_repeticao_conclui() {
        let root = std::env::temp_dir().join(format!("login-cleanup-{}", uuid::Uuid::new_v4()));
        let operation = uuid::Uuid::new_v4().to_string();
        let profile = perfil(&root, &operation);
        std::fs::write(profile.join("credencial"), "provisoria").unwrap();
        // Hardlink é recusado pelo fd::own: a remoção falha no meio da varredura.
        let outside = root.join("original");
        std::fs::write(&outside, "de fora").unwrap();
        std::fs::hard_link(&outside, profile.join("ligado")).unwrap();
        assert!(remove_replaced(&profile).is_err());
        assert!(profile.join(".donnabot-login-operation").exists(), "o marcador sobrevive à falha");
        assert!(profile.exists());
        std::fs::remove_file(profile.join("ligado")).unwrap();
        remove_replaced(&profile).unwrap();
        assert!(!profile.exists(), "a repetição conclui a limpeza");
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "de fora");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn interrupcao_entre_marcador_e_pasta_e_sem_marcador_com_conteudo_recusa() {
        let root = std::env::temp_dir().join(format!("login-cleanup-{}", uuid::Uuid::new_v4()));
        // Só restou a pasta vazia: a remoção anterior apagou conteúdo e marcador.
        let operation = uuid::Uuid::new_v4().to_string();
        let profile = root.join(&operation);
        std::fs::create_dir_all(&profile).unwrap();
        remove_replaced(&profile).unwrap();
        assert!(!profile.exists());
        let vazia = root.join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&vazia).unwrap();
        remove(&vazia, "1", false).unwrap();
        assert!(!vazia.exists());
        // Sem marcador mas com conteúdo: sem prova, nada é tocado.
        let operation = uuid::Uuid::new_v4().to_string();
        let profile = root.join(&operation);
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(profile.join("credencial"), "preservada").unwrap();
        assert!(remove_replaced(&profile).is_err());
        assert_eq!(std::fs::read_to_string(profile.join("credencial")).unwrap(), "preservada");
        // Cancelamento continua exigindo a marca, mesmo com a pasta vazia.
        let operation = uuid::Uuid::new_v4().to_string();
        let profile = root.join(&operation);
        std::fs::create_dir_all(&profile).unwrap();
        assert!(remove(&profile, "1", true).is_err());
        assert!(profile.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
