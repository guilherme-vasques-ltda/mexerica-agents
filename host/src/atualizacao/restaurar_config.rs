//! Restaura exclusivamente a configuração root da última transação.
use super::Hex as _;
use super::{
    armazenamento::{Diretorio, sem_acl},
    estado::Fase,
    journal::{Etapa, Journal},
    snapshot::Configuracao,
};
use anyhow::{Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use std::{
    ffi::CString,
    fs::File,
    io::Write,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
};

impl Diretorio {
    /// O chamador já auditou o início e deve manter o serviço parado e a trava.
    pub fn restaurar_motor_conf(
        &self,
        estado: &Diretorio,
        publicacao: &Diretorio,
        trava: &File,
        agora: i64,
    ) -> Result<()> {
        estado.validar_trava(trava)?;
        estado.exigir_sem_bloqueio()?;
        publicacao.exigir_sem_emergencia()?;
        let mut journal: Journal = estado.ler("journal")?;
        if estado.expirar_journal(trava, &mut journal, agora)? {
            publicacao.publicar_status(estado, trava)?;
            anyhow::bail!("janela expirada; recuperação administrativa necessária");
        }
        ensure!(
            matches!(journal.etapa, Etapa::ReversaoIniciada | Etapa::SeletorRestaurado)
                && matches!(journal.estado.fase, Fase::Pendente { .. }),
            "fase não permite restaurar configuração"
        );
        estado.manifesto_transacao(trava, &journal, agora)?;
        let mut permitido = journal.estado.clone();
        permitido.reverter(agora)?;
        let snapshot = estado.snapshot_transacao(trava, &journal)?;
        // Testes usam diretório root temporário; em produção, só /etc/donnabot.
        #[cfg(not(test))]
        {
            let confiavel = Diretorio::abrir_configuracao()?;
            let (a, b) = (self.descritor().metadata()?, confiavel.descritor().metadata()?);
            ensure!(a.dev() == b.dev() && a.ino() == b.ino(), "diretório da configuração divergente");
        }
        self.aplicar_configuracao(&snapshot.motor_conf)
    }

    fn aplicar_configuracao(&self, configuracao: &Configuracao) -> Result<()> {
        let dir = self.descritor().as_raw_fd();
        // SAFETY: abre apenas o nome fixo, sem seguir links ou bloquear em especiais.
        let fd = unsafe {
            libc::openat(
                dir,
                c"motor.conf".as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd >= 0 {
            // SAFETY: descritor recém-aberto passa ao File.
            let atual = unsafe { File::from_raw_fd(fd) };
            let meta = atual.metadata()?;
            ensure!(
                meta.is_file()
                    && meta.uid() == 0
                    && meta.gid() == 0
                    && meta.nlink() == 1
                    && meta.mode() & 0o7777 == 0o644,
                "configuração atual insegura"
            );
            sem_acl(&atual)?;
        } else {
            ensure!(
                std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT),
                "configuração atual inacessível"
            );
        }
        match configuracao {
            Configuracao::Ausente => {
                // SAFETY: remove somente o nome fixo em diretório root validado.
                if unsafe { libc::unlinkat(dir, c"motor.conf".as_ptr(), 0) } != 0 {
                    ensure!(
                        std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT),
                        "remoção da configuração falhou"
                    );
                }
                self.descritor().sync_all()?;
            }
            Configuracao::Presente { base64 } => {
                ensure!(base64.len() <= 128 * 1024, "configuração anterior grande demais");
                let dados = B64.decode(base64)?;
                ensure!(dados.len() <= 64 * 1024, "configuração anterior grande demais");
                let mut aleatorio = [0u8; 16];
                getrandom::fill(&mut aleatorio).map_err(|_| anyhow::anyhow!("fonte aleatória indisponível"))?;
                let tmp = CString::new(format!(".motor-{}", aleatorio.hex()))?;
                // SAFETY: criação exclusiva relativa a diretório root validado.
                let fd = unsafe {
                    libc::openat(
                        dir,
                        tmp.as_ptr(),
                        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                        0o600,
                    )
                };
                ensure!(fd >= 0, "falha ao preparar configuração anterior");
                // SAFETY: descritor recém-criado passa ao File.
                let mut arquivo = unsafe { File::from_raw_fd(fd) };
                let resultado = (|| -> Result<()> {
                    let meta = arquivo.metadata()?;
                    ensure!(
                        meta.is_file() && meta.uid() == 0 && meta.gid() == 0 && meta.nlink() == 1,
                        "temporário de configuração inseguro"
                    );
                    sem_acl(&arquivo)?;
                    arquivo.write_all(&dados)?;
                    // SAFETY: inode novo e conferido, ainda sem exposição pelo nome final.
                    ensure!(unsafe { libc::fchmod(arquivo.as_raw_fd(), 0o644) } == 0, "modo da configuração recusado");
                    arquivo.sync_all()?;
                    ensure!(
                        // SAFETY: os dois nomes são fixo/aleatório no mesmo diretório confiável.
                        unsafe { libc::renameat(dir, tmp.as_ptr(), dir, c"motor.conf".as_ptr()) } == 0,
                        "publicação da configuração falhou"
                    );
                    self.descritor().sync_all()?;
                    Ok(())
                })();
                if resultado.is_err() {
                    // SAFETY: remove apenas o temporário exclusivo desta chamada.
                    unsafe { libc::unlinkat(dir, tmp.as_ptr(), 0) };
                }
                resultado?;
            }
        }
        Ok(())
    }
}
