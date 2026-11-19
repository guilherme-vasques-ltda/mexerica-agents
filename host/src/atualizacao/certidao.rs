//! Documentos públicos root-owned, sem paths ou dados privados do Hub.
use super::Hex as _;
use super::{
    armazenamento::{Diretorio, sem_acl},
    estado::Fase,
    journal::{Etapa, Journal},
    status::Status,
};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::{
    ffi::{CStr, CString},
    fs::File,
    io::Write,
    os::fd::{AsRawFd, FromRawFd},
};

#[derive(Serialize)]
struct Certidao<'a> {
    id: &'a str,
    sequencia_anterior: Option<u64>,
    sequencia_nova: u64,
    snapshot_sha256: &'a str,
}
fn hash_valido(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub fn gerar_id() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| anyhow::anyhow!("fonte aleatória indisponível"))?;
    Ok(bytes.hex())
}

#[derive(Clone, Copy)]
enum Nome {
    Certidao,
    Status,
    Release,
}
impl Nome {
    fn componente(self) -> &'static CStr {
        match self {
            Self::Certidao => c"transacao.json",
            Self::Status => c"status.json",
            Self::Release => c"release.json",
        }
    }
}

impl Diretorio {
    fn gravar_publico(&self, nome: Nome, bytes: &[u8]) -> Result<()> {
        ensure!(bytes.len() <= 4096, "documento público grande demais");
        let dir = self.descritor().as_raw_fd();
        let (uid, gid) = self.identidade();
        let alvo = nome.componente();
        // SAFETY: ponteiro e descritor válidos; não segue o objeto consultado.
        let mut existente: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: consulta sem seguir links em descritor válido; o ponteiro aponta para memória local inicializada.
        let status = unsafe { libc::fstatat(dir, alvo.as_ptr(), &raw mut existente, libc::AT_SYMLINK_NOFOLLOW) };
        if status == 0 {
            ensure!(
                existente.st_mode & libc::S_IFMT == libc::S_IFREG
                    && existente.st_uid == uid
                    && existente.st_gid == gid
                    && existente.st_nlink == 1
                    && existente.st_mode & 0o7777 == 0o644,
                "documento público existente inseguro"
            );
            // SAFETY: abre somente o nome fixo no diretório root, sem seguir links.
            let fd = unsafe { libc::openat(dir, alvo.as_ptr(), libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC) };
            ensure!(fd >= 0, "documento público inacessível");
            // SAFETY: descritor novo transferido ao File.
            sem_acl(&unsafe { File::from_raw_fd(fd) })?;
        } else {
            ensure!(
                std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT),
                "documento público inacessível"
            );
        }
        let temporario = CString::new(format!(".publico-{}", gerar_id()?))?;
        // SAFETY: criação exclusiva no diretório controlado por root.
        let fd = unsafe {
            libc::openat(
                dir,
                temporario.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        ensure!(fd >= 0, "não foi possível criar documento público");
        // SAFETY: openat devolveu um descritor novo.
        let mut file = unsafe { File::from_raw_fd(fd) };
        let resultado = (|| -> Result<()> {
            use std::os::unix::fs::MetadataExt;
            let meta = file.metadata()?;
            ensure!(
                meta.is_file() && meta.uid() == uid && meta.gid() == gid && meta.nlink() == 1,
                "temporário público inseguro"
            );
            file.write_all(bytes)?;
            ensure!(
                // SAFETY: descritor exclusivo de arquivo recém-criado.
                unsafe { libc::fchmod(file.as_raw_fd(), 0o644) } == 0,
                "não foi possível proteger documento público"
            );
            sem_acl(&file)?;
            file.sync_all()?;
            ensure!(
                // SAFETY: nomes fixos/aleatórios validados no mesmo diretório confiável.
                unsafe { libc::renameat(dir, temporario.as_ptr(), dir, alvo.as_ptr()) } == 0,
                "não foi possível publicar documento"
            );
            self.descritor().sync_all()?;
            Ok(())
        })();
        if resultado.is_err() {
            // SAFETY: remove apenas o arquivo temporário exclusivo.
            unsafe { libc::unlinkat(dir, temporario.as_ptr(), 0) };
        }
        resultado
    }

    pub fn exigir_sem_emergencia(&self) -> Result<()> {
        // SAFETY: consulta do nome fixo sem seguir links.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: consulta sem seguir links em descritor válido; o ponteiro aponta para memória local inicializada.
        let resultado = unsafe {
            libc::fstatat(
                self.descritor().as_raw_fd(),
                c"emergencia".as_ptr(),
                &raw mut stat,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        ensure!(
            resultado == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT),
            "bloqueio de emergência ativo"
        );
        Ok(())
    }

    /// Marca pública irreversível pelo wrapper. Pode ser criada sem a trava
    /// quando o próprio guard falha; somente root pode removê-la fora do fluxo.
    pub fn bloquear_emergencia(&self) -> Result<()> {
        ensure!(self.identidade() == (0, 0), "diretório de publicação não é root");
        let dir = self.descritor().as_raw_fd();
        // SAFETY: nome fixo em diretório root, criação exclusiva sem seguir links.
        let fd = unsafe {
            libc::openat(
                dir,
                c"emergencia".as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            let erro = std::io::Error::last_os_error();
            if erro.raw_os_error() == Some(libc::EEXIST) {
                return Ok(());
            }
            return Err(erro.into());
        }
        // SAFETY: descritor novo transferido ao File.
        let mut file = unsafe { File::from_raw_fd(fd) };
        file.write_all(b"bloqueado\n")?;
        // SAFETY: inode recém-criado, sob diretório root validado.
        ensure!(unsafe { libc::fchmod(file.as_raw_fd(), 0o644) } == 0, "falha ao proteger bloqueio público");
        sem_acl(&file)?;
        file.sync_all()?;
        self.descritor().sync_all()?;
        Ok(())
    }

    /// Certidão deriva apenas do journal persistido, sob a trava root.
    pub fn publicar_certidao(&self, estado: &Diretorio, trava: &File) -> Result<()> {
        estado.validar_trava(trava)?;
        let journal: Journal = estado.ler("journal")?;
        ensure!(
            hash_valido(&journal.transacao)
                && hash_valido(&journal.snapshot_sha256)
                && journal.sequencia_nova > journal.sequencia_anterior.unwrap_or(0),
            "journal incompatível com certidão"
        );
        let manifesto = estado.manifesto_assinado(trava, &journal)?;
        let release = serde_json::json!({
            "transacao": journal.transacao,
            "versao": manifesto.versao,
            "hub_sha256": manifesto.hub.sha256,
        });
        // Documento independente: o formato da certidão anterior permanece compatível
        // com o Hub anterior que pode precisar iniciar para retomar uma reversão.
        self.gravar_publico(Nome::Release, &serde_json::to_vec(&release)?)?;
        let certidao = Certidao {
            id: &journal.transacao,
            sequencia_anterior: journal.sequencia_anterior,
            sequencia_nova: journal.sequencia_nova,
            snapshot_sha256: &journal.snapshot_sha256,
        };
        self.gravar_publico(Nome::Certidao, &serde_json::to_vec(&certidao)?)
    }

    /// Um journal ausente, inconsistente ou bloqueado gera somente status bloqueado.
    pub fn publicar_status(&self, estado: &Diretorio, trava: &File) -> Result<()> {
        estado.validar_trava(trava)?;
        let status = if estado.exigir_sem_bloqueio().is_err() || self.exigir_sem_emergencia().is_err() {
            Status::Bloqueado
        } else if let Ok(journal) = estado.ler::<Journal>("journal") {
            match (journal.etapa, journal.estado.fase, journal.publicado_em) {
                (Etapa::Pendente, Fase::Pendente { prazo, .. }, Some(inicio))
                    if inicio < prazo && journal.estado.marca == journal.sequencia_nova =>
                {
                    Status::Pendente {
                        slot: journal.slot_novo.nome().into(),
                        sequencia: journal.sequencia_nova,
                        inicio,
                        prazo,
                    }
                }
                // Reversão dentro da janela interrompida (queda no meio dela): ninguém inicia,
                // mas o Hub distingue isso do bloqueio definitivo e agenda a retomada.
                (Etapa::ReversaoIniciada | Etapa::SeletorRestaurado, Fase::Pendente { prazo, .. }, Some(inicio))
                    if inicio < prazo && journal.estado.marca == journal.sequencia_nova =>
                {
                    Status::Revertendo {
                        slot: journal.slot_novo.nome().into(),
                        sequencia: journal.sequencia_nova,
                        inicio,
                        prazo,
                    }
                }
                (Etapa::Concluida, Fase::Aceita, _) if journal.estado.marca == journal.sequencia_nova => {
                    Status::Liberado { slot: journal.slot_novo.nome().into(), sequencia: journal.sequencia_nova }
                }
                // Após reversão ou cancelamento, só o par anterior autenticado volta a ser liberado.
                (Etapa::Concluida, Fase::Revertida, _)
                | (Etapa::Cancelada, Fase::Livre | Fase::Aceita | Fase::Revertida, _) => {
                    match (journal.slot_anterior, journal.sequencia_anterior) {
                        (Some(slot), Some(sequencia)) if sequencia > 0 => {
                            Status::Liberado { slot: slot.nome().into(), sequencia }
                        }
                        _ => Status::Bloqueado,
                    }
                }
                _ => Status::Bloqueado,
            }
        } else {
            Status::Bloqueado
        };
        self.gravar_publico(Nome::Status, &serde_json::to_vec(&status)?)
    }
}
