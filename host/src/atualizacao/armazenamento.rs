//! Acesso a estado privilegiado por descritores, sem seguir links.
use super::Hex as _;
use anyhow::{Context, Result, bail, ensure};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    ffi::{CString, OsStr},
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::{Component, Path},
};

const LIMITE_ESTADO: u64 = 1024 * 1024;

pub struct Diretorio {
    fd: File,
    uid: u32,
    gid: u32,
}

fn nome(nome: &OsStr) -> Result<CString> {
    let bytes = nome.as_bytes();
    ensure!(!bytes.is_empty() && bytes != b"." && bytes != b".." && !bytes.contains(&b'/'), "nome inválido");
    Ok(CString::new(bytes)?)
}

fn arquivo(fd: libc::c_int) -> Result<File> {
    ensure!(fd >= 0, "abertura recusada: {}", std::io::Error::last_os_error());
    // SAFETY: open/openat retornou descritor novo cuja posse passa para File.
    Ok(unsafe { File::from_raw_fd(fd) })
}

pub(super) fn sem_acl(file: &File) -> Result<()> {
    for atributo in [c"system.posix_acl_access", c"system.posix_acl_default"] {
        // SAFETY: descritor válido e nome terminado em NUL, sem buffer de saída.
        let resultado = unsafe { libc::fgetxattr(file.as_raw_fd(), atributo.as_ptr(), std::ptr::null_mut(), 0) };
        let erro = std::io::Error::last_os_error();
        ensure!(
            resultado == -1 && matches!(erro.raw_os_error(), Some(libc::ENODATA | libc::ENOTSUP)),
            "ACL ou erro ao conferir ACL"
        );
    }
    Ok(())
}

fn validar_dir(file: &File, uid: u32, privado: bool) -> Result<()> {
    let m = file.metadata()?;
    ensure!(m.is_dir() && m.uid() == uid && m.gid() == 0 && m.mode() & 0o7022 == 0, "diretório inseguro");
    if privado {
        ensure!(m.mode() & 0o7777 == 0o700, "diretório de estado deve ser 0700");
    }
    sem_acl(file)
}

impl Diretorio {
    /// Exige caminho absoluto, ancestrais root-owned e ausência de links e ACLs.
    pub fn abrir(path: &Path) -> Result<Self> {
        Self::abrir_com_modo(path, 0o700)
    }

    #[cfg(test)]
    pub(crate) fn abrir_teste(path: &Path) -> Result<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        let fd = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
            .open(path)?;
        let meta = fd.metadata()?;
        // SAFETY: consultas de identidade do processo, sem efeitos colaterais.
        let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
        ensure!(
            meta.is_dir() && meta.uid() == uid && meta.gid() == gid && meta.mode() & 0o7777 == 0o700,
            "fixture insegura"
        );
        sem_acl(&fd)?;
        Ok(Self { fd, uid, gid })
    }

    pub fn abrir_publicacao() -> Result<Self> {
        Self::abrir_com_modo(Path::new("/opt/donnabot/releases"), 0o755)
    }

    pub fn abrir_configuracao() -> Result<Self> {
        Self::abrir_com_modo(Path::new("/etc/donnabot"), 0o755)
    }

    pub(super) fn descritor(&self) -> &File {
        &self.fd
    }
    pub(super) fn identidade(&self) -> (u32, u32) {
        (self.uid, self.gid)
    }

    /// Consulta sem seguir links; distingue ausência de erro de acesso.
    pub fn existe(&self, nome_arquivo: &str) -> Result<bool> {
        let componente = nome(OsStr::new(nome_arquivo))?;
        // SAFETY: ponteiro e descritor válidos; não segue o objeto consultado.
        let mut estatistica: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: consulta sem seguir links em descritor válido; o ponteiro aponta para memória local inicializada.
        if unsafe {
            libc::fstatat(self.fd.as_raw_fd(), componente.as_ptr(), &raw mut estatistica, libc::AT_SYMLINK_NOFOLLOW)
        } == 0
        {
            return Ok(true);
        }
        ensure!(std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT), "estado inacessível");
        Ok(false)
    }

    pub(super) fn abrir_com_modo(path: &Path, modo: u32) -> Result<Self> {
        ensure!(path.is_absolute(), "caminho deve ser absoluto");
        // SAFETY: pathname fixo, flags válidas e nenhuma criação.
        let mut atual = arquivo(unsafe {
            libc::open(c"/".as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW)
        })?;
        validar_dir(&atual, 0, false)?;
        for componente in path.components() {
            match componente {
                Component::RootDir => (),
                Component::Normal(parte) => {
                    let parte = nome(parte)?;
                    // SAFETY: descritor de diretório aberto, componente validado e flags sem seguir links.
                    atual = arquivo(unsafe {
                        libc::openat(
                            atual.as_raw_fd(),
                            parte.as_ptr(),
                            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                        )
                    })?;
                    validar_dir(&atual, 0, false)?;
                }
                _ => bail!("caminho inseguro"),
            }
        }
        validar_dir(&atual, 0, false)?;
        ensure!(atual.metadata()?.mode() & 0o7777 == modo, "modo do diretório final inválido");
        Ok(Self { fd: atual, uid: 0, gid: 0 })
    }

    /// Só nomes enumerados; um slot inativo nunca pode ser percorrido pelo Hub.
    pub fn abrir_slot_inativo(&self, slot: super::slots::Slot) -> Result<Self> {
        let componente = nome(OsStr::new(slot.nome()))?;
        // SAFETY: componente fixo, diretório protegido e abertura sem seguir links.
        let fd = arquivo(unsafe {
            libc::openat(
                self.fd.as_raw_fd(),
                componente.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        })?;
        validar_dir(&fd, 0, true)?;
        ensure!(fd.metadata()?.dev() == self.fd.metadata()?.dev(), "slot atravessa filesystem");
        Ok(Self { fd, uid: 0, gid: 0 })
    }

    /// Fecha o slot anterior antes de remover as capabilities do ajudante.
    /// O serviço e todos os seus filhos devem já estar parados e isolados.
    pub fn fechar_slot(&self, slot: super::slots::Slot) -> Result<Self> {
        let Self { fd, .. } = self.abrir_slot(slot)?;
        // SAFETY: modifica apenas o diretório aberto e validado.
        ensure!(unsafe { libc::fchmod(fd.as_raw_fd(), 0o700) } == 0, "não foi possível fechar slot anterior");
        validar_dir(&fd, 0, true)?;
        fd.sync_all()?;
        self.fd.sync_all()?;
        Ok(Self { fd, uid: 0, gid: 0 })
    }

    /// Abre slot root-owned 0700 ou 0755 sem alterar seu modo.
    pub fn abrir_slot(&self, slot: super::slots::Slot) -> Result<Self> {
        let componente = nome(OsStr::new(slot.nome()))?;
        // SAFETY: componente enumerado e abertura relativa sem seguir links.
        let fd = arquivo(unsafe {
            libc::openat(
                self.fd.as_raw_fd(),
                componente.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        })?;
        validar_dir(&fd, 0, false)?;
        let meta = fd.metadata()?;
        ensure!(
            matches!(meta.mode() & 0o7777, 0o700 | 0o755) && meta.dev() == self.fd.metadata()?.dev(),
            "slot inseguro"
        );
        Ok(Self { fd, uid: 0, gid: 0 })
    }

    /// Retorna inode fixo do ajudante para remover capabilities por descritor.
    pub fn ajudante_inativo(&self, gid_hub: u32) -> Result<File> {
        validar_dir(&self.fd, 0, true)?;
        ensure!(gid_hub != 0, "grupo do Hub inválido");
        // SAFETY: arquivo fixo sob slot privado, sem seguir links nem bloquear.
        let file = arquivo(unsafe {
            libc::openat(
                self.fd.as_raw_fd(),
                c"donnabot-motor-exec".as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        })?;
        let meta = file.metadata()?;
        ensure!(
            meta.is_file()
                && meta.uid() == 0
                && meta.gid() == gid_hub
                && meta.nlink() == 1
                && meta.mode() & 0o7777 == 0o750,
            "ajudante anterior inseguro"
        );
        sem_acl(&file)?;
        Ok(file)
    }

    /// Não recicla slot existente; limpeza de versões anteriores tem gate próprio.
    pub fn criar_slot_inativo(&self, slot: super::slots::Slot) -> Result<Self> {
        let componente = nome(OsStr::new(slot.nome()))?;
        ensure!(
            // SAFETY: mkdirat é exclusivo e usa componente fixo sob diretório root.
            unsafe { libc::mkdirat(self.fd.as_raw_fd(), componente.as_ptr(), 0o700) } == 0,
            "slot já existe ou não pode ser criado"
        );
        self.fd.sync_all()?;
        self.abrir_slot_inativo(slot)
    }

    fn subdiretorio_privado(&self, componente: &str) -> Result<Self> {
        let componente = nome(OsStr::new(componente))?;
        // SAFETY: componente validado relativo a pai já aberto, sem seguir links.
        let fd = arquivo(unsafe {
            libc::openat(
                self.fd.as_raw_fd(),
                componente.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        })?;
        let meta = fd.metadata()?;
        ensure!(
            meta.is_dir()
                && meta.uid() == self.uid
                && meta.gid() == self.gid
                && meta.mode() & 0o7777 == 0o700
                && meta.dev() == self.fd.metadata()?.dev(),
            "subdiretório privado inseguro"
        );
        sem_acl(&fd)?;
        Ok(Self { fd, uid: self.uid, gid: self.gid })
    }

    fn id_transacao(id: &str) -> Result<()> {
        ensure!(
            id.len() == 64 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "identificador de transação inválido"
        );
        Ok(())
    }

    /// A identidade vem do journal root-owned, nunca de argumento do Hub.
    pub fn abrir_transacao(&self, id: &str) -> Result<Self> {
        Self::id_transacao(id)?;
        self.subdiretorio_privado("transacoes")?.subdiretorio_privado(id)
    }

    /// O cofre `transacoes` é criado pela etapa administrativa.
    pub fn criar_transacao(&self, id: &str) -> Result<Self> {
        Self::id_transacao(id)?;
        let cofre = self.subdiretorio_privado("transacoes")?;
        let componente = nome(OsStr::new(id))?;
        ensure!(
            // SAFETY: diretório novo e exclusivo sob cofre root validado.
            unsafe { libc::mkdirat(cofre.fd.as_raw_fd(), componente.as_ptr(), 0o700) } == 0,
            "transação já existe ou não pode ser criada"
        );
        cofre.fd.sync_all()?;
        cofre.subdiretorio_privado(id)
    }

    fn abrir_regular(&self, nome_arquivo: &str, flags: libc::c_int) -> Result<File> {
        let nome_arquivo = nome(OsStr::new(nome_arquivo))?;
        // SAFETY: nome de componente validado e diretório controlado pelo root.
        let file = arquivo(unsafe {
            libc::openat(
                self.fd.as_raw_fd(),
                nome_arquivo.as_ptr(),
                flags | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            )
        })?;
        let m = file.metadata()?;
        ensure!(
            m.is_file() && m.uid() == self.uid && m.gid() == self.gid && m.mode() & 0o7777 == 0o600 && m.nlink() == 1,
            "arquivo de estado inseguro"
        );
        sem_acl(&file)?;
        Ok(file)
    }

    pub fn ler_bytes(&self, nome_arquivo: &str, limite: u64) -> Result<Vec<u8>> {
        ensure!(limite > 0 && limite <= LIMITE_ESTADO, "limite inválido");
        let file = self.abrir_regular(nome_arquivo, libc::O_RDONLY)?;
        ensure!(file.metadata()?.len() <= limite, "estado grande demais");
        let mut dados = Vec::new();
        file.take(limite + 1).read_to_end(&mut dados)?;
        ensure!(dados.len() as u64 <= limite, "estado grande demais");
        Ok(dados)
    }

    pub fn ler<T: DeserializeOwned>(&self, nome_arquivo: &str) -> Result<T> {
        serde_json::from_slice(&self.ler_bytes(nome_arquivo, LIMITE_ESTADO)?).context("estado inválido")
    }

    /// A trava exclusiva deve ser mantida pelo chamador durante toda a operação.
    pub fn gravar<T: Serialize>(&self, nome_arquivo: &str, valor: &T) -> Result<()> {
        self.gravar_bytes(nome_arquivo, &serde_json::to_vec(valor)?)
    }

    /// Preserva bytes autenticados exatamente, sem nova serialização.
    pub fn gravar_bytes(&self, nome_arquivo: &str, dados: &[u8]) -> Result<()> {
        let destino = nome(OsStr::new(nome_arquivo))?;
        // Recusar estado inseguro existente em vez de sobrescrevê-lo.
        // SAFETY: ponteiro e descritor válidos; AT_SYMLINK_NOFOLLOW consulta o próprio inode.
        let mut estatistica: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: consulta sem seguir links em descritor válido; o ponteiro aponta para memória local inicializada.
        let existe = unsafe {
            libc::fstatat(self.fd.as_raw_fd(), destino.as_ptr(), &raw mut estatistica, libc::AT_SYMLINK_NOFOLLOW)
        };
        if existe == 0 {
            let _ = self.abrir_regular(nome_arquivo, libc::O_RDONLY)?;
        } else {
            ensure!(
                std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT),
                "estado existente não verificável"
            );
        }
        ensure!(dados.len() as u64 <= LIMITE_ESTADO, "estado grande demais");
        let mut aleatorio = [0u8; 16];
        getrandom::fill(&mut aleatorio).map_err(|_| anyhow::anyhow!("fonte aleatória indisponível"))?;
        let temporario = CString::new(format!(".tmp-{}", aleatorio.hex()))?;
        // SAFETY: cria arquivo novo em diretório privado, sem seguir links.
        let mut file = arquivo(unsafe {
            libc::openat(
                self.fd.as_raw_fd(),
                temporario.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        })?;
        let resultado = (|| -> Result<()> {
            let m = file.metadata()?;
            ensure!(
                m.is_file()
                    && m.uid() == self.uid
                    && m.gid() == self.gid
                    && m.nlink() == 1
                    && m.mode() & 0o7777 == 0o600,
                "temporário inseguro"
            );
            sem_acl(&file)?;
            file.write_all(dados)?;
            file.sync_all()?;
            ensure!(
                // SAFETY: ambos os nomes são componentes e pertencem ao diretório privado aberto.
                unsafe {
                    libc::renameat(self.fd.as_raw_fd(), temporario.as_ptr(), self.fd.as_raw_fd(), destino.as_ptr())
                } == 0,
                "rename falhou: {}",
                std::io::Error::last_os_error()
            );
            self.fd.sync_all()?;
            Ok(())
        })();
        if resultado.is_err() {
            // SAFETY: remove apenas o temporário exclusivo criado acima.
            unsafe { libc::unlinkat(self.fd.as_raw_fd(), temporario.as_ptr(), 0) };
        }
        resultado
    }

    /// Remove somente arquivo regular autenticado e persiste a remoção.
    pub fn remover_regular(&self, nome_arquivo: &str) -> Result<()> {
        if !self.existe(nome_arquivo)? {
            return Ok(());
        }
        let _arquivo = self.abrir_regular(nome_arquivo, libc::O_RDONLY)?;
        let componente = nome(OsStr::new(nome_arquivo))?;
        // SAFETY: componente fixo validado sob diretório privado aberto.
        ensure!(unsafe { libc::unlinkat(self.fd.as_raw_fd(), componente.as_ptr(), 0) } == 0, "remoção recusada");
        self.fd.sync_all()?;
        Ok(())
    }

    /// Diretório temporário exclusivo no mesmo filesystem, sob pai privado.
    pub fn temporario(&self) -> Result<(String, Self)> {
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(|_| anyhow::anyhow!("fonte aleatória indisponível"))?;
        let texto = format!(".download-{}", random.hex());
        let componente = nome(OsStr::new(&texto))?;
        ensure!(
            // SAFETY: nome aleatório único e descritor de diretório privado.
            unsafe { libc::mkdirat(self.fd.as_raw_fd(), componente.as_ptr(), 0o700) } == 0,
            "mkdir temporário falhou"
        );
        // SAFETY: abre o diretório recém-criado sem seguir links.
        let fd = arquivo(unsafe {
            libc::openat(
                self.fd.as_raw_fd(),
                componente.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        })?;
        validar_dir(&fd, self.uid, true)?;
        ensure!(fd.metadata()?.dev() == self.fd.metadata()?.dev(), "filesystem temporário divergente");
        self.fd.sync_all()?;
        Ok((texto, Self { fd, uid: self.uid, gid: self.gid }))
    }

    pub fn criar_regular(&self, componente: &str) -> Result<File> {
        let componente = nome(OsStr::new(componente))?;
        // SAFETY: criação exclusiva relativa ao diretório privado.
        let file = arquivo(unsafe {
            libc::openat(
                self.fd.as_raw_fd(),
                componente.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        })?;
        let m = file.metadata()?;
        ensure!(
            m.is_file() && m.uid() == self.uid && m.gid() == self.gid && m.nlink() == 1 && m.mode() & 0o7777 == 0o600,
            "arquivo temporário inseguro"
        );
        sem_acl(&file)?;
        self.fd.sync_all()?;
        Ok(file)
    }

    /// Move somente arquivo regular validado para nome ainda ausente, no mesmo filesystem.
    pub fn publicar_novo(&self, origem: &str, destino_dir: &Self, destino: &str) -> Result<()> {
        let file = self.abrir_regular(origem, libc::O_RDONLY)?;
        ensure!(self.fd.metadata()?.dev() == destino_dir.fd.metadata()?.dev(), "filesystem divergente");
        file.sync_all()?;
        let origem = nome(OsStr::new(origem))?;
        let destino = nome(OsStr::new(destino))?;
        ensure!(
            // SAFETY: ambos são componentes em diretórios privados; RENAME_NOREPLACE recusa substituição.
            unsafe {
                libc::renameat2(
                    self.fd.as_raw_fd(),
                    origem.as_ptr(),
                    destino_dir.fd.as_raw_fd(),
                    destino.as_ptr(),
                    libc::RENAME_NOREPLACE,
                )
            } == 0,
            "publicação recusada: {}",
            std::io::Error::last_os_error()
        );
        self.fd.sync_all()?;
        destino_dir.fd.sync_all()?;
        Ok(())
    }

    /// Um bloqueio existente, inclusive inválido ou inacessível, recusa operação.
    pub fn exigir_sem_bloqueio(&self) -> Result<()> {
        // SAFETY: ponteiro e descritor válidos; não segue o objeto consultado.
        let mut estatistica: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: consulta sem seguir links em descritor válido; o ponteiro aponta para memória local inicializada.
        let resultado = unsafe {
            libc::fstatat(self.fd.as_raw_fd(), c"bloqueio".as_ptr(), &raw mut estatistica, libc::AT_SYMLINK_NOFOLLOW)
        };
        ensure!(
            resultado == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT),
            "atualização bloqueada; recuperação administrativa necessária"
        );
        Ok(())
    }

    pub fn tentar_travar(&self) -> Result<Option<File>> {
        let lock = self.abrir_regular("lock", libc::O_RDWR)?;
        // SAFETY: descritor válido e operação flock exclusiva.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(Some(lock));
        }
        let erro = std::io::Error::last_os_error();
        if erro.kind() == std::io::ErrorKind::WouldBlock {
            return Ok(None);
        }
        Err(erro.into())
    }

    pub fn travar(&self) -> Result<File> {
        self.tentar_travar()?.context("outra operação está em andamento")
    }

    pub fn validar_trava(&self, trava: &File) -> Result<()> {
        let atual = self.abrir_regular("lock", libc::O_RDWR)?;
        let esperado = atual.metadata()?;
        let recebido = trava.metadata()?;
        ensure!(
            esperado.dev() == recebido.dev() && esperado.ino() == recebido.ino(),
            "trava não corresponde ao estado"
        );
        ensure!(
            // SAFETY: reforça a trava exclusiva na descrição aberta do chamador.
            unsafe { libc::flock(trava.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
            "trava exclusiva indisponível"
        );
        Ok(())
    }

    pub fn auditar<T: Serialize>(&self, evento: &T) -> Result<()> {
        let mut file = self.abrir_regular("auditoria", libc::O_WRONLY | libc::O_APPEND)?;
        travar_auditoria(&file, ESPERA_AUDITORIA)?;
        let mut linha = serde_json::to_vec(evento)?;
        ensure!(linha.len() <= 4096, "evento grande demais");
        linha.push(b'\n');
        file.write_all(&linha)?;
        file.sync_all()?;
        Ok(())
    }
}

/// O guard audita a parada sem a trava do estado, em paralelo ao wrapper; cada
/// gravação segura a trava da auditoria só pelo append e o fsync.
const ESPERA_AUDITORIA: std::time::Duration = std::time::Duration::from_secs(5);

/// Trava exclusiva do inode da auditoria (impede linhas intercaladas), com espera
/// limitada: contenção curta entre guard e wrapper não é falha.
fn travar_auditoria(file: &File, espera: std::time::Duration) -> Result<()> {
    let limite = std::time::Instant::now() + espera;
    loop {
        // SAFETY: descritor válido e operação flock exclusiva não bloqueante.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(());
        }
        let erro = std::io::Error::last_os_error();
        ensure!(erro.kind() == std::io::ErrorKind::WouldBlock, "auditoria indisponível: {erro}");
        ensure!(std::time::Instant::now() < limite, "auditoria ocupada");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atualizacao::{
        estado::{Estado, Fase},
        journal::{Etapa, Evento, Journal, Motivo, Operacao, Origem, Resultado},
    };
    use std::{
        fs::{self, OpenOptions},
        os::unix::fs::{DirBuilderExt, OpenOptionsExt, symlink},
    };

    #[test]
    fn aceite_e_duravel_e_repeticao_e_auditada() -> Result<()> {
        use crate::atualizacao::{
            manifesto::{self, Artefato, Manifesto},
            snapshot::{Arquivo, Configuracao, NomeSlot, Par, Snapshot},
        };
        use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
        use ed25519_dalek::{Signer as _, SigningKey};
        use sha2::{Digest as _, Sha256};
        let path = std::env::temp_dir().join(format!("mx365-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&path)?;
        let fd = OpenOptions::new().read(true).custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW).open(&path)?;
        // SAFETY: consultas de identidade do processo, sem efeitos colaterais.
        let armazenamento = Diretorio { fd, uid: unsafe { libc::geteuid() }, gid: unsafe { libc::getegid() } };
        for nome in ["auditoria", "lock"] {
            OpenOptions::new().write(true).create_new(true).mode(0o600).open(path.join(nome))?;
        }
        let trava = armazenamento.travar()?;
        let journal = Journal {
            estado: Estado { marca: 5, piso: 3, fase: Fase::Pendente { anterior: Some(4), prazo: 110 } },
            etapa: Etapa::Pendente,
            transacao: "c".repeat(64),
            sequencia_nova: 5,
            publicado_em: Some(100),
            sequencia_anterior: Some(4),
            slot_anterior: Some(crate::atualizacao::slots::Slot::A),
            slot_novo: crate::atualizacao::slots::Slot::B,
            manifesto_sha256: "a".repeat(64),
            snapshot_sha256: "b".repeat(64),
        };
        // A fixture corresponde à release autenticada e ao snapshot da mesma
        // transação, sem depender de chaves ou caminhos do ambiente de produção.
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).map_err(|_| anyhow::anyhow!("fonte aleatória indisponível"))?;
        let chave = SigningKey::from_bytes(&seed);
        let politica = serde_json::json!({"endpoint":"https://exemplo.invalid/", "canal":"estavel", "arquitetura":std::env::consts::ARCH,
            "uid_hub":1234,"gid_hub":1234,"chave_manifesto":B64.encode(chave.verifying_key().as_bytes()),
            "tamanho_maximo":1024,"tempo_rede_segundos":10,"janela_minima":10,"janela_maxima":100,
            "cgroup_memoria":1024,"cgroup_processos":2,"cgroup_cpu_quota":1000,"cgroup_cpu_periodo":1000});
        armazenamento.gravar("politica", &politica)?;
        fs::DirBuilder::new().mode(0o700).create(path.join("transacoes"))?;
        let transacao = armazenamento.criar_transacao(&journal.transacao)?;
        let artefato = |nome| Artefato { nome, tamanho: 10, sha256: "a".repeat(64) };
        let manifesto = Manifesto {
            produto: "mexerica".into(),
            canal: "estavel".into(),
            arquitetura: std::env::consts::ARCH.into(),
            versao: "1.0.0".into(),
            sequencia: 5,
            inicio: 90,
            expiracao: 200,
            hub: artefato("donnabot-host".into()),
            ajudante: artefato("donnabot-motor-exec".into()),
            sequencia_minima_reversivel: 3,
            wrapper_minimo: "0.1.0".into(),
        };
        let bytes = serde_json::to_vec(&manifesto)?;
        let mut mensagem = manifesto::DOMINIO.to_vec();
        mensagem.extend_from_slice(&bytes);
        transacao.gravar_bytes("manifesto", &bytes)?;
        transacao.gravar_bytes("assinatura", &chave.sign(&mensagem).to_bytes())?;
        let arquivo = || Arquivo { tamanho: 10, sha256: "a".repeat(64) };
        let snapshot = Snapshot {
            anterior: Some(Par { sequencia: 4, slot: NomeSlot::A, hub: arquivo(), ajudante: arquivo() }),
            motor_conf: Configuracao::Ausente,
        };
        let snapshot = serde_json::to_vec(&snapshot)?;
        transacao.gravar_bytes("snapshot", &snapshot)?;
        let mut journal = journal;
        journal.manifesto_sha256 = Sha256::digest(&bytes).hex();
        journal.snapshot_sha256 = Sha256::digest(&snapshot).hex();
        armazenamento.gravar("journal", &journal)?;
        symlink("slot-a", path.join("ativo"))?;
        assert!(
            armazenamento
                .aceitar_transacao(
                    &armazenamento,
                    &trava,
                    super::super::chamador::Identidade::Sudo(armazenamento.uid),
                    100
                )
                .is_err()
        );
        let nao_aceito: Journal = armazenamento.ler("journal")?;
        assert!(matches!(nao_aceito.estado.fase, Fase::Pendente { .. }));
        fs::remove_file(path.join("ativo"))?;
        symlink("slot-b", path.join("ativo"))?;
        transacao.gravar_bytes("snapshot", b"{}")?;
        assert!(
            armazenamento
                .aceitar_transacao(
                    &armazenamento,
                    &trava,
                    super::super::chamador::Identidade::Sudo(armazenamento.uid),
                    100
                )
                .is_err()
        );
        transacao.gravar_bytes("snapshot", &snapshot)?;
        assert!(
            super::super::verificacao::conferir(&armazenamento, &armazenamento, &trava, &journal, 100).is_err(),
            "sem registro o aceite deve ser recusado"
        );
        armazenamento.aceitar_transacao(
            &armazenamento,
            &trava,
            super::super::chamador::Identidade::Sudo(armazenamento.uid),
            100,
        )?;
        let gravado: Journal = armazenamento.ler("journal")?;
        assert_eq!(gravado.estado.fase, Fase::Aceita);
        fs::remove_file(path.join("status.json"))?;
        armazenamento.aceitar_transacao(
            &armazenamento,
            &trava,
            super::super::chamador::Identidade::Sudo(armazenamento.uid),
            101,
        )?;
        assert!(path.join("status.json").is_file());
        let mut inconsistente: Journal = armazenamento.ler("journal")?;
        inconsistente.estado.marca += 1;
        armazenamento.gravar("journal", &inconsistente)?;
        assert!(
            armazenamento
                .aceitar_transacao(
                    &armazenamento,
                    &trava,
                    super::super::chamador::Identidade::Sudo(armazenamento.uid),
                    102
                )
                .is_err()
        );
        let auditoria = fs::read_to_string(path.join("auditoria"))?;
        assert_eq!(auditoria.lines().count(), 10);
        assert!(auditoria.contains("\"resultado\":\"recusada\""));
        fs::remove_dir_all(path)?;
        Ok(())
    }

    #[test]
    fn expiracao_sem_aceite_sobrevive_a_reabertura_e_bloqueia_operacoes() -> Result<()> {
        let path = std::env::temp_dir().join(format!("mx365-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&path)?;
        for nome in ["auditoria", "lock"] {
            OpenOptions::new().write(true).create_new(true).mode(0o600).open(path.join(nome))?;
        }
        // SAFETY: consultas de identidade do processo, sem efeitos colaterais.
        let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
        let abrir = || -> Result<Diretorio> {
            let fd = OpenOptions::new().read(true).custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW).open(&path)?;
            Ok(Diretorio { fd, uid, gid })
        };
        {
            let armazenamento = abrir()?;
            let trava = armazenamento.travar()?;
            let mut journal = Journal {
                estado: Estado { marca: 5, piso: 3, fase: Fase::Pendente { anterior: Some(4), prazo: 110 } },
                etapa: Etapa::Pendente,
                transacao: "c".repeat(64),
                sequencia_nova: 5,
                publicado_em: Some(100),
                sequencia_anterior: Some(4),
                slot_anterior: Some(crate::atualizacao::slots::Slot::A),
                slot_novo: crate::atualizacao::slots::Slot::B,
                manifesto_sha256: "a".repeat(64),
                snapshot_sha256: "b".repeat(64),
            };
            armazenamento.gravar("journal", &journal)?;
            assert!(armazenamento.expirar_com(&trava, &mut journal, 110, |_| Ok(()))?);
        }
        {
            let armazenamento = abrir()?;
            let trava = armazenamento.travar()?;
            assert!(armazenamento.exigir_sem_bloqueio().is_err());
            assert!(
                armazenamento
                    .aceitar_transacao(&armazenamento, &trava, super::super::chamador::Identidade::Sudo(uid), 111)
                    .is_err()
            );
            let journal: Journal = armazenamento.ler("journal")?;
            assert_eq!(journal.estado.fase, Fase::Expirada);
            let mut estado = journal.estado;
            assert!(estado.reverter(111).is_err());
            assert!(estado.iniciar(6, 3, Some(5), 200, 111).is_err());
        }
        fs::remove_dir_all(path)?;
        Ok(())
    }

    /// Guard (sem a trava do estado) e wrapper auditando ao mesmo tempo: a contenção
    /// curta espera em vez de falhar; ocupação além do prazo continua recusada.
    #[test]
    fn auditoria_concorrente_espera_a_vez() -> Result<()> {
        let path = std::env::temp_dir().join(format!("mx365-aud-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&path)?;
        OpenOptions::new().write(true).create_new(true).mode(0o600).open(path.join("auditoria"))?;
        let ocupante = OpenOptions::new().write(true).open(path.join("auditoria"))?;
        // SAFETY: descritor válido; simula o append em curso do outro processo.
        assert_eq!(unsafe { libc::flock(ocupante.as_raw_fd(), libc::LOCK_EX) }, 0);
        let segundo = OpenOptions::new().write(true).open(path.join("auditoria"))?;
        assert!(travar_auditoria(&segundo, std::time::Duration::from_millis(50)).is_err());
        let liberar = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            drop(ocupante);
        });
        travar_auditoria(&segundo, std::time::Duration::from_secs(5))?;
        liberar.join().map_err(|_| anyhow::anyhow!("thread falhou"))?;
        fs::remove_dir_all(path)?;
        Ok(())
    }

    #[test]
    fn auditoria_sem_links_e_expiracao_gravada() -> Result<()> {
        let path = std::env::temp_dir().join(format!("mx365-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&path)?;
        let dir = OpenOptions::new().read(true).custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW).open(&path)?;
        // SAFETY: consultas de identidade do processo, sem efeitos colaterais.
        let armazenamento = Diretorio { fd: dir, uid: unsafe { libc::geteuid() }, gid: unsafe { libc::getegid() } };
        OpenOptions::new().write(true).create_new(true).mode(0o600).open(path.join("auditoria"))?;
        let evento = Evento {
            horario_unix: 100,
            uid: armazenamento.uid,
            origem: Origem::Sudo,
            operacao: Operacao::Atualizar,
            sequencia_atual: Some(5),
            sequencia_anterior: Some(4),
            resultado: Resultado::Inicio,
            motivo: Motivo::Solicitacao,
        };
        armazenamento.inicio(&evento)?;
        assert!(fs::read_to_string(path.join("auditoria"))?.contains("\"resultado\":\"inicio\""));
        fs::remove_file(path.join("auditoria"))?;
        symlink("ausente", path.join("auditoria"))?;
        assert!(armazenamento.inicio(&evento).is_err());
        OpenOptions::new().write(true).create_new(true).mode(0o600).open(path.join("lock"))?;
        let trava = armazenamento.travar()?;
        let mut journal = Journal {
            estado: Estado { marca: 5, piso: 3, fase: Fase::Pendente { anterior: Some(4), prazo: 110 } },
            etapa: Etapa::Pendente,
            transacao: "c".repeat(64),
            sequencia_nova: 5,
            publicado_em: Some(100),
            sequencia_anterior: Some(4),
            slot_anterior: Some(crate::atualizacao::slots::Slot::A),
            slot_novo: crate::atualizacao::slots::Slot::B,
            manifesto_sha256: "a".repeat(64),
            snapshot_sha256: "b".repeat(64),
        };
        assert!(armazenamento.expirar_com(&trava, &mut journal, 110, |_| Ok(()))?);
        let gravado: Journal = armazenamento.ler("journal")?;
        assert_eq!(gravado.estado.fase, Fase::Expirada);
        assert!(path.join("bloqueio").is_file());
        assert!(armazenamento.exigir_sem_bloqueio().is_err());
        fs::remove_dir_all(path)?;
        Ok(())
    }
}
