//! Primitivas de slot; a troca permanece desabilitada até a recuperação estar integrada.
use super::Hex as _;
use anyhow::{Result, ensure};
use std::{
    fs,
    fs::File,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    A,
    B,
}
impl Slot {
    pub fn nome(self) -> &'static str {
        match self {
            Self::A => "slot-a",
            Self::B => "slot-b",
        }
    }
    pub fn outro(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
    pub fn do_seletor(alvo: &str) -> Option<Self> {
        match alvo {
            "slot-a" => Some(Self::A),
            "slot-b" => Some(Self::B),
            _ => None,
        }
    }
}

pub fn protected_hardlinks() -> Result<()> {
    let texto = fs::read_to_string("/proc/sys/fs/protected_hardlinks")?;
    ensure!(texto.trim() == "1", "fs.protected_hardlinks deve ser 1");
    Ok(())
}

/// O descritor deve vir de diretório privado e ser aberto sem seguir links.
/// A prova de ausência de processos com o inode antigo é um gate separado.
pub fn limpar_capacidades(file: &File) -> Result<()> {
    let meta = file.metadata()?;
    ensure!(meta.is_file() && meta.uid() == 0 && meta.nlink() == 1, "inode de ajudante inseguro");
    // SAFETY: descritor válido e nome de atributo fixo e terminado em NUL.
    let resultado = unsafe { libc::fremovexattr(file.as_raw_fd(), c"security.capability".as_ptr()) };
    if resultado != 0 {
        let erro = std::io::Error::last_os_error();
        ensure!(erro.raw_os_error() == Some(libc::ENODATA), "não foi possível remover capacidades");
    }
    // SAFETY: consulta de tamanho sem buffer em descritor validado.
    let resultado =
        unsafe { libc::fgetxattr(file.as_raw_fd(), c"security.capability".as_ptr(), std::ptr::null_mut(), 0) };
    ensure!(
        resultado == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENODATA),
        "capacidades ainda presentes ou não verificáveis"
    );
    file.sync_all()?;
    Ok(())
}

impl super::armazenamento::Diretorio {
    /// Só o seletor fixo é aceito. Sua ausência representa a primeira migração.
    pub fn seletor(&self) -> Result<Option<Slot>> {
        let dir = self.descritor().as_raw_fd();
        // SAFETY: descritor validado e ponteiro para stat inicializado.
        let mut meta: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: consulta o inode do seletor sem seguir o link.
        let status = unsafe { libc::fstatat(dir, c"ativo".as_ptr(), &raw mut meta, libc::AT_SYMLINK_NOFOLLOW) };
        if status != 0 {
            ensure!(std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT), "seletor inacessível");
            return Ok(None);
        }
        let (uid, gid) = self.identidade();
        ensure!(
            meta.st_mode & libc::S_IFMT == libc::S_IFLNK
                && meta.st_uid == uid
                && meta.st_gid == gid
                && meta.st_nlink == 1,
            "seletor inseguro"
        );
        let mut bytes = [0u8; 64];
        // SAFETY: buffer válido e tamanho limitado; readlinkat não inclui NUL.
        let tamanho = unsafe { libc::readlinkat(dir, c"ativo".as_ptr(), bytes.as_mut_ptr().cast(), bytes.len()) };
        let tamanho = usize::try_from(tamanho).unwrap_or(0);
        ensure!(tamanho > 0 && tamanho < bytes.len(), "alvo de seletor inválido");
        let alvo = std::str::from_utf8(&bytes[..tamanho])?;
        Ok(Some(Slot::do_seletor(alvo).ok_or_else(|| anyhow::anyhow!("alvo de seletor recusado"))?))
    }

    /// Exige trava, journal durável e slots previamente preparados pelo chamador.
    /// Não concede permissões nem capacidades e não conclui a transação.
    pub fn trocar_seletor(&self, esperado: Option<Slot>, novo: Slot) -> Result<()> {
        ensure!(self.seletor()? == esperado && esperado != Some(novo), "estado do seletor divergente");
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(|_| anyhow::anyhow!("fonte aleatória indisponível"))?;
        let temporario = std::ffi::CString::new(format!(".ativo-{}", random.hex()))?;
        let alvo = std::ffi::CString::new(novo.nome())?;
        let dir = self.descritor().as_raw_fd();
        // SAFETY: diretório protegido, nome temporário e alvo fixo validados.
        ensure!(unsafe { libc::symlinkat(alvo.as_ptr(), dir, temporario.as_ptr()) } == 0, "criação do seletor falhou");
        let resultado = (|| -> Result<()> {
            ensure!(
                // SAFETY: ambos nomes estão no mesmo diretório validado.
                unsafe { libc::renameat(dir, temporario.as_ptr(), dir, c"ativo".as_ptr()) } == 0,
                "troca do seletor falhou"
            );
            self.descritor().sync_all()?;
            ensure!(self.seletor()? == Some(novo), "seletor não confirmado");
            Ok(())
        })();
        if resultado.is_err() {
            // SAFETY: remove apenas o nome temporário exclusivo criado nesta chamada.
            unsafe { libc::unlinkat(dir, temporario.as_ptr(), 0) };
        }
        resultado
    }

    /// Retira o seletor quando a volta é ao legado (sem par anterior gerido).
    pub fn remover_seletor(&self, esperado: Slot) -> Result<()> {
        ensure!(self.seletor()? == Some(esperado), "estado do seletor divergente");
        ensure!(
            // SAFETY: remove somente o nome fixo do seletor, já validado como link root.
            unsafe { libc::unlinkat(self.descritor().as_raw_fd(), c"ativo".as_ptr(), 0) } == 0,
            "remoção do seletor falhou"
        );
        self.descritor().sync_all()?;
        ensure!(self.seletor()?.is_none(), "seletor ainda presente");
        Ok(())
    }
}

/// Linux `VFS_CAP_REVISION_2` | EFFECTIVE; somente `CAP_SETGID` e `CAP_SETUID`.
fn capacidades_esperadas() -> [u8; 20] {
    let mut caps = [0u8; 20];
    caps[..4].copy_from_slice(&0x0200_0001u32.to_le_bytes());
    caps[4..8].copy_from_slice(&0x0000_00c0u32.to_le_bytes());
    caps
}

/// Confere que o inode tem exatamente as capabilities do ajudante, nada além.
pub fn exigir_capacidades(file: &File) -> Result<()> {
    let caps = capacidades_esperadas();
    let mut recebidas = [0u8; 24];
    // SAFETY: buffer de saída tem exatamente o tamanho declarado.
    let tamanho = unsafe {
        libc::fgetxattr(
            file.as_raw_fd(),
            c"security.capability".as_ptr(),
            recebidas.as_mut_ptr().cast(),
            recebidas.len(),
        )
    };
    ensure!(
        usize::try_from(tamanho).ok() == Some(caps.len()) && recebidas[..caps.len()] == caps,
        "capabilities do ajudante divergentes"
    );
    Ok(())
}

/// Prepara inode exclusivo ainda inacessível dentro do slot 0700.
/// O chamador deve manter o diretório fechado ao Hub até o commit do journal.
pub fn preparar_ajudante(file: &mut File, esperado: &super::snapshot::Arquivo, gid_hub: u32) -> Result<()> {
    armar_ajudante(file, esperado, gid_hub, 0, 0o600)
}

/// Reaplica as capabilities somente após conferir conteúdo, dono e modo atuais
/// contra o registro autenticado. Usado também para restabelecer o par anterior.
pub fn armar_ajudante(
    file: &mut File,
    esperado: &super::snapshot::Arquivo,
    gid_hub: u32,
    gid_atual: u32,
    modo_atual: u32,
) -> Result<()> {
    ensure!(gid_hub != 0, "grupo do Hub inválido");
    super::snapshot::verificar_arquivo(file, esperado, gid_atual, modo_atual)?;
    // SAFETY: fstatvfs preenche a estrutura para o descritor validado.
    let mut vfs: libc::statvfs = unsafe { std::mem::zeroed() };
    ensure!(
        // SAFETY: descritor e ponteiro válidos.
        unsafe { libc::fstatvfs(file.as_raw_fd(), &raw mut vfs) } == 0 && vfs.f_flag & libc::ST_NOSUID == 0,
        "filesystem sem suporte a privilégios"
    );
    // SAFETY: só altera o inode root que foi aberto e validado, nunca um caminho externo.
    ensure!(unsafe { libc::fchown(file.as_raw_fd(), 0, gid_hub) } == 0, "não foi possível ajustar grupo do ajudante");
    // SAFETY: mesmo descritor validado, ainda em diretório inacessível ao Hub.
    ensure!(unsafe { libc::fchmod(file.as_raw_fd(), 0o750) } == 0, "não foi possível ajustar modo do ajudante");
    let caps = capacidades_esperadas();
    // SAFETY: buffer de capabilities no formato v2 e descritor já validado.
    let aplicado = unsafe {
        libc::fsetxattr(file.as_raw_fd(), c"security.capability".as_ptr(), caps.as_ptr().cast(), caps.len(), 0)
    };
    if aplicado != 0 {
        limpar_capacidades(file)?;
        anyhow::bail!("não foi possível aplicar capacidades do ajudante");
    }
    if exigir_capacidades(file).is_err() {
        limpar_capacidades(file)?;
        anyhow::bail!("capacidades do ajudante não confirmadas");
    }
    let meta = file.metadata()?;
    ensure!(
        meta.uid() == 0 && meta.gid() == gid_hub && meta.nlink() == 1 && meta.mode() & 0o7777 == 0o750,
        "atributos do ajudante divergentes"
    );
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seletor_so_aceita_nomes_fixos() {
        assert_eq!(Slot::do_seletor("slot-a"), Some(Slot::A));
        assert_eq!(Slot::A.outro().nome(), "slot-b");
        for alvo in ["/slot-a", "../slot-a", "slot-c", "slot-a/x", ""] {
            assert_eq!(Slot::do_seletor(alvo), None);
        }
    }
}
