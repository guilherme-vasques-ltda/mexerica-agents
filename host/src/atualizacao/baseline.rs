//! Registro prévio da configuração administrativa, sem consultar dados do Hub.
use super::{
    armazenamento::{Diretorio, sem_acl},
    snapshot::Configuracao,
};
use anyhow::{Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Baseline {
    pub versao: u32,
    pub login: String,
    pub uid_hub: u32,
    pub gid_hub: u32,
    pub motor_conf: Configuracao,
    /// Presença de cada objeto de sistema antes da etapa administrativa.
    pub objetos: std::collections::BTreeMap<String, bool>,
    pub legado: Legado,
}

/// Instalação legada do Hub, só por hashes: conteúdo pode conter credenciais.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Legado {
    pub unidade_sha256: Option<String>,
    pub dropins_sha256: std::collections::BTreeMap<String, String>,
    pub link_app: Option<String>,
}

impl Diretorio {
    /// Somente diretório /etc/donnabot validado pelo chamador administrativo.
    pub fn capturar_motor_conf(&self) -> Result<Configuracao> {
        // SAFETY: nome fixo, diretório validado, sem seguir links nem bloquear em especiais.
        let fd = unsafe {
            libc::openat(
                self.descritor().as_raw_fd(),
                c"motor.conf".as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            )
        };
        if fd < 0 {
            let erro = std::io::Error::last_os_error();
            if erro.raw_os_error() == Some(libc::ENOENT) {
                return Ok(Configuracao::Ausente);
            }
            return Err(erro.into());
        }
        // SAFETY: openat devolveu descritor novo, cuja posse passa ao File.
        let file = unsafe { File::from_raw_fd(fd) };
        let meta = file.metadata()?;
        ensure!(
            meta.is_file()
                && meta.uid() == 0
                && meta.gid() == 0
                && meta.nlink() == 1
                && meta.mode() & 0o7777 == 0o644
                && meta.len() <= 64 * 1024,
            "motor.conf existente incompatível"
        );
        sem_acl(&file)?;
        let mut bytes = Vec::new();
        file.take(64 * 1024 + 1).read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 64 * 1024, "motor.conf grande demais");
        Ok(Configuracao::Presente { base64: B64.encode(bytes) })
    }

    /// A baseline nunca é sobrescrita. O chamador segura a trava de instalação.
    pub fn gravar_baseline_inicial(&self, baseline: &Baseline) -> Result<()> {
        let bytes = serde_json::to_vec(baseline)?;
        ensure!(bytes.len() <= 256 * 1024, "baseline grande demais");
        let (nome, temporario) = self.temporario()?;
        let mut arquivo = temporario.criar_regular("baseline")?;
        arquivo.write_all(&bytes)?;
        arquivo.sync_all()?;
        temporario.publicar_novo("baseline", self, "baseline")?;
        drop(temporario);
        let nome = std::ffi::CString::new(nome)?;
        // SAFETY: remove só o diretório temporário exclusivo, já vazio, criado acima.
        unsafe { libc::unlinkat(self.descritor().as_raw_fd(), nome.as_ptr(), libc::AT_REMOVEDIR) };
        Ok(())
    }
}
