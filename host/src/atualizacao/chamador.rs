//! Identidade para auditoria, capturada antes de descartar o ambiente.
use anyhow::{Result, ensure};
use std::ffi::OsStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Identidade {
    Sudo(u32),
    AdminRoot,
}
impl Identidade {
    pub fn uid(self) -> u32 {
        match self {
            Self::Sudo(uid) => uid,
            Self::AdminRoot => 0,
        }
    }
    pub fn origem(self) -> super::journal::Origem {
        match self {
            Self::Sudo(_) => super::journal::Origem::Sudo,
            Self::AdminRoot => super::journal::Origem::AdminRoot,
        }
    }
    pub fn autorizar(self, uid_hub: u32) -> Result<()> {
        ensure!(uid_hub != 0 && (self == Self::AdminRoot || self == Self::Sudo(uid_hub)), "chamador não autorizado");
        Ok(())
    }
}

fn interpretar(valor: Option<&OsStr>, uid_pai: u32, pai_sudo: bool) -> Result<Identidade> {
    if uid_pai == 0 && !pai_sudo {
        return Ok(Identidade::AdminRoot);
    }
    let valor = valor.ok_or_else(|| anyhow::anyhow!("SUDO_UID ausente"))?;
    let texto = valor.to_str().ok_or_else(|| anyhow::anyhow!("SUDO_UID inválido"))?;
    ensure!(!texto.is_empty() && texto.len() <= 10 && texto.bytes().all(|b| b.is_ascii_digit()), "SUDO_UID inválido");
    let uid: u32 = texto.parse()?;
    ensure!(
        uid.to_string() == texto && (uid_pai == uid || (uid_pai == 0 && pai_sudo)),
        "identidade do sudo inconsistente"
    );
    Ok(if uid == 0 { Identidade::AdminRoot } else { Identidade::Sudo(uid) })
}

/// Resolve `/usr/bin/sudo` mesmo quando é link (ex.: alternatives), exigindo que o
/// link, cada ancestral do alvo e o próprio executável sejam root e não graváveis
/// por grupo/outros. Sem isso, um sudo empacotado como link recusaria toda chamada.
pub(super) fn sudo_confiavel() -> Result<std::fs::Metadata> {
    use std::{fs, os::unix::fs::MetadataExt};
    let link = fs::symlink_metadata("/usr/bin/sudo")?;
    ensure!(link.uid() == 0 && (link.file_type().is_symlink() || link.mode() & 0o022 == 0), "sudo inseguro");
    let real = fs::canonicalize("/usr/bin/sudo")?;
    for ancestral in real.ancestors().skip(1) {
        let meta = fs::symlink_metadata(ancestral)?;
        ensure!(meta.is_dir() && meta.uid() == 0 && meta.mode() & 0o022 == 0, "ancestral do sudo inseguro");
    }
    let sudo = fs::symlink_metadata(&real)?;
    ensure!(sudo.is_file() && sudo.uid() == 0 && sudo.mode() & 0o022 == 0, "sudo inseguro");
    Ok(sudo)
}

/// `SUDO_UID` é capturado antes de limpar o ambiente. A comparação com o processo
/// pai distingue root direto de variável `SUDO_UID` herdada por um shell root.
pub fn capturar() -> Result<Identidade> {
    use std::{
        fs::{self, File},
        io::Read,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::MetadataExt,
        },
    };
    // SAFETY: getters de identidade e PID sem efeitos colaterais.
    let (real, efetivo, pai) = unsafe { (libc::getuid(), libc::geteuid(), libc::getppid()) };
    ensure!(real == 0 && efetivo == 0 && pai > 1, "execução exige sudo ou administrador root");
    let proc = File::open("/proc")?;
    // SAFETY: fstatfs preenche estrutura válida a partir do descritor aberto.
    let mut sistema: libc::statfs = unsafe { std::mem::zeroed() };
    ensure!(
        // SAFETY: descritor e ponteiro válidos.
        unsafe { libc::fstatfs(proc.as_raw_fd(), &raw mut sistema) } == 0 && sistema.f_type.cast_unsigned() == 0x9fa0,
        "procfs não confiável"
    );
    // SAFETY: pidfd_open captura identidade estável do processo pai.
    let pidfd = unsafe { libc::syscall(libc::SYS_pidfd_open, pai, 0) };
    ensure!(pidfd >= 0, "não foi possível identificar processo pai");
    // SAFETY: syscall retornou descritor novo e válido.
    let pidfd = unsafe { File::from_raw_fd(i32::try_from(pidfd)?) };
    let mut status = String::new();
    File::open(format!("/proc/{pai}/status"))?.take(65_537).read_to_string(&mut status)?;
    ensure!(status.len() <= 65_536, "credenciais do pai inválidas");
    let linha = status
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))
        .ok_or_else(|| anyhow::anyhow!("credenciais do pai ausentes"))?;
    let uid_pai: u32 = linha.split_whitespace().next().ok_or_else(|| anyhow::anyhow!("UID do pai ausente"))?.parse()?;
    let pai_sudo = if uid_pai == 0 {
        let sudo = sudo_confiavel()?;
        let exe = fs::metadata(format!("/proc/{pai}/exe"))?;
        exe.dev() == sudo.dev() && exe.ino() == sudo.ino()
    } else {
        false
    };
    let mut poll = libc::pollfd { fd: pidfd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
    ensure!(
        // SAFETY: consulta não bloqueante do pidfd para detectar saída do pai.
        unsafe { libc::poll(&raw mut poll, 1, 0) } == 0 && unsafe { libc::getppid() } == pai,
        "processo pai mudou durante auditoria"
    );
    let valor = std::env::var_os("SUDO_UID");
    interpretar(valor.as_deref(), uid_pai, pai_sudo)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identifica_sudo_e_admin_sem_aceitar_uid_arbitrario() {
        assert_eq!(interpretar(None, 0, false).unwrap(), Identidade::AdminRoot);
        assert_eq!(interpretar(Some(OsStr::new("0")), 0, true).unwrap(), Identidade::AdminRoot);
        assert_eq!(interpretar(Some(OsStr::new("1000")), 1000, false).unwrap(), Identidade::Sudo(1000));
        assert_eq!(serde_json::to_string(&Identidade::AdminRoot.origem()).unwrap(), "\"admin/root\"");
        assert_eq!(serde_json::to_string(&Identidade::Sudo(1000).origem()).unwrap(), "\"sudo\"");
        assert_eq!(interpretar(Some(OsStr::new("1000")), 0, false).unwrap(), Identidade::AdminRoot);
        assert_eq!(interpretar(Some(OsStr::new("1000")), 0, true).unwrap(), Identidade::Sudo(1000));
        assert!(interpretar(Some(OsStr::new("1000")), 1001, false).is_err());
        assert!(Identidade::Sudo(1000).autorizar(1000).is_ok());
        assert!(Identidade::Sudo(1001).autorizar(1000).is_err());
        for texto in ["", "-1", "01", " 1000", "4294967296", "1000\n"] {
            assert!(interpretar(Some(OsStr::new(texto)), 1000, false).is_err());
        }
    }
}
