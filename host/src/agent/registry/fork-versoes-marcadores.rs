use super::{MARCADORES, Referencia, erro_marcador, marcador_obsoleto};
use crate::fork::motor_exec::fd::{Directory, own};
use crate::fork::private_fs;
use anyhow::{Result, ensure};
use std::ffi::OsStr;
use std::fs::File;
use std::io::ErrorKind;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

const LIMITE_ENTRADAS: usize = 4096;

pub(super) struct Marcadores {
    raiz: Directory,
    catalogo: Directory,
    agente: Directory,
}

fn verificar(arquivo: &File) -> Result<()> {
    own(arquivo)?;
    ensure!(arquivo.metadata()?.mode() & 0o022 == 0, "marcadores graváveis por terceiros");
    Ok(())
}

fn mesmo_arquivo(a: &File, b: &File) -> Result<bool> {
    let (a, b) = (a.metadata()?, b.metadata()?);
    Ok((a.dev(), a.ino()) == (b.dev(), b.ino()))
}

impl Marcadores {
    pub(super) fn abrir(raiz: &Path, id: &str) -> Result<(Self, File)> {
        let resumir = |operacao, e: anyhow::Error| {
            erro_marcador(
                operacao,
                id,
                "-",
                e.downcast_ref::<std::io::Error>().map_or(ErrorKind::Other, std::io::Error::kind),
            )
        };
        let pasta_segura = |pasta: Directory| {
            verificar(&pasta.0)?;
            Ok::<_, anyhow::Error>(pasta)
        };
        let raiz = private_fs::Directory::open(raiz, false)
            .or_else(|e| {
                if e.downcast_ref::<std::io::Error>().is_some_and(|e| e.kind() == ErrorKind::NotFound) {
                    return private_fs::Directory::open(raiz, true);
                }
                Err(e)
            })
            .map(|pasta| Directory(pasta.0))
            .and_then(pasta_segura)
            .map_err(|e| resumir("criar_raiz_trava", e))?;
        let catalogo = raiz
            .child(OsStr::new(MARCADORES), Some(0o700))
            .and_then(pasta_segura)
            .map_err(|e| resumir("criar_raiz_trava", e))?;
        let agente = catalogo
            .child(OsStr::new(id), Some(0o700))
            .and_then(pasta_segura)
            .map_err(|e| resumir("criar_pasta_trava", e))?;
        let lock = agente
            .file(OsStr::new("trava"), libc::O_RDWR | libc::O_CREAT, 0o600)
            .and_then(|lock| {
                verificar(&lock)?;
                Ok(lock)
            })
            .map_err(|e| resumir("abrir_trava", e))?;
        Ok((Self { raiz, catalogo, agente }, lock))
    }

    pub(super) fn em_uso(
        &self,
        id: &str,
        lock: &File,
        boot: Option<uuid::Uuid>,
        propria: Option<&Referencia>,
    ) -> Option<(bool, Vec<String>)> {
        let trava = OsStr::new("trava");
        let mut restante = LIMITE_ENTRADAS;
        let mut encontradas = false;
        let mut em_uso = Vec::new();
        let mut propria_confirmada = propria.is_none();
        let versoes = self.agente.names_limit(restante).ok()?;
        restante -= versoes.len();
        for nome in versoes {
            if nome == trava {
                continue;
            }
            let pasta = self.agente.child(&nome, None).ok()?;
            verificar(&pasta.0).ok()?;
            encontradas = true;
            let versao = nome.to_str()?;
            let marcadores = pasta.names_limit(restante).ok()?;
            restante -= marcadores.len();
            let mut ativa = false;
            for marcador in marcadores {
                let arquivo = pasta.file(&marcador, libc::O_RDONLY, 0).and_then(|arquivo| {
                    verificar(&arquivo)?;
                    Ok(arquivo)
                });
                if let Some(propria) =
                    propria.filter(|r| r.versao == versao && r.marcador.file_name() == Some(marcador.as_os_str()))
                {
                    if !mesmo_arquivo(&arquivo.ok()?, &propria.arquivo).ok()? {
                        return None;
                    }
                    propria_confirmada = true;
                } else if !arquivo.is_ok_and(|arquivo| {
                    marcador.to_str().is_some_and(|nome| marcador_obsoleto(nome, boot))
                        && pasta.file(&marcador, libc::O_RDONLY, 0).is_ok_and(|atual| {
                            mesmo_arquivo(&arquivo, &atual).unwrap_or(false) && pasta.unlink(&marcador, false).is_ok()
                        })
                }) {
                    ativa = true;
                }
            }
            if !mesmo_arquivo(&pasta.0, &self.agente.child(&nome, None).ok()?.0).ok()? {
                return None;
            }
            if ativa {
                em_uso.push(versao.to_owned());
                if em_uso.len() > 128 {
                    return None;
                }
            }
        }
        (propria_confirmada
            && mesmo_arquivo(lock, &self.agente.file(trava, libc::O_RDONLY, 0).ok()?).ok()?
            && mesmo_arquivo(&self.agente.0, &self.catalogo.child(OsStr::new(id), None).ok()?.0).ok()?
            && mesmo_arquivo(&self.catalogo.0, &self.raiz.child(OsStr::new(MARCADORES), None).ok()?.0).ok()?)
        .then_some((encontradas, em_uso))
    }
}
