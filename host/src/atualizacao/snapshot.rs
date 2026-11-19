//! Validação de snapshot root-owned antes de qualquer recuperação.
use super::Hex as _;
use super::{
    armazenamento::{Diretorio, sem_acl},
    journal::Journal,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    os::{fd::AsRawFd, unix::fs::MetadataExt},
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Arquivo {
    pub tamanho: u64,
    pub sha256: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Par {
    pub sequencia: u64,
    pub slot: NomeSlot,
    pub hub: Arquivo,
    pub ajudante: Arquivo,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NomeSlot {
    A,
    B,
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "estado", rename_all = "snake_case", deny_unknown_fields)]
pub enum Configuracao {
    Ausente,
    Presente { base64: String },
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub anterior: Option<Par>,
    pub motor_conf: Configuracao,
}

fn digest_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).hex()
}
fn hash_valido(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Compara bytes exatos com o hash mantido no journal; não aceita fontes do Hub.
pub fn ler(bytes: &[u8], hash_journal: &str, sequencia_anterior: Option<u64>) -> Result<Snapshot> {
    use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
    ensure!(
        !bytes.is_empty()
            && bytes.len() <= 256 * 1024
            && hash_valido(hash_journal)
            && digest_hex(bytes) == hash_journal,
        "snapshot ausente ou adulterado"
    );
    let snapshot: Snapshot = serde_json::from_slice(bytes)?;
    ensure!(snapshot.anterior.as_ref().map(|p| p.sequencia) == sequencia_anterior, "sequência do snapshot divergente");
    if let Some(par) = &snapshot.anterior {
        ensure!(par.sequencia > 0, "sequência anterior inválida");
        for arquivo in [&par.hub, &par.ajudante] {
            ensure!(
                arquivo.tamanho > 0 && arquivo.tamanho <= 2 * 1024 * 1024 * 1024 && hash_valido(&arquivo.sha256),
                "arquivo do snapshot inválido"
            );
        }
    }
    if let Configuracao::Presente { base64 } = &snapshot.motor_conf {
        ensure!(
            base64.len() <= 128 * 1024 && B64.decode(base64)?.len() <= 64 * 1024,
            "configuração anterior grande demais"
        );
    }
    Ok(snapshot)
}

impl Diretorio {
    /// Lê somente o snapshot root da última transação, sem caminho do chamador.
    pub fn snapshot_transacao(&self, trava: &File, journal: &Journal) -> Result<Snapshot> {
        self.validar_trava(trava)?;
        let bytes = self.abrir_transacao(&journal.transacao)?.ler_bytes("snapshot", 256 * 1024)?;
        let snapshot = ler(&bytes, &journal.snapshot_sha256, journal.sequencia_anterior)?;
        let slot = snapshot.anterior.as_ref().map(|par| match par.slot {
            NomeSlot::A => super::slots::Slot::A,
            NomeSlot::B => super::slots::Slot::B,
        });
        ensure!(slot == journal.slot_anterior, "slot do snapshot divergente");
        Ok(snapshot)
    }
}

/// Ambos descritores devem ter sido abertos sem links no slot root 0700.
pub fn verificar_arquivo(file: &mut File, esperado: &Arquivo, gid: u32, modo: u32) -> Result<()> {
    let meta = file.metadata()?;
    ensure!(
        meta.is_file()
            && meta.uid() == 0
            && meta.gid() == gid
            && meta.nlink() == 1
            && meta.mode() & 0o7777 == modo
            && meta.len() == esperado.tamanho,
        "inode do snapshot inseguro"
    );
    sem_acl(file)?;
    // SAFETY: consulta de tamanho de atributo fixo em descritor válido.
    let caps = unsafe { libc::fgetxattr(file.as_raw_fd(), c"security.capability".as_ptr(), std::ptr::null_mut(), 0) };
    ensure!(
        caps == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENODATA),
        "inode anterior ainda possui capacidades ou não pode ser verificado"
    );
    conferir_hash(file, esperado)
}

/// Somente tamanho e SHA-256 do conteúdo lido pelo descritor já validado.
pub fn conferir_hash(file: &mut File, esperado: &Arquivo) -> Result<()> {
    ensure!(file.metadata()?.len() == esperado.tamanho, "tamanho do arquivo divergente");
    file.seek(SeekFrom::Start(0))?;
    let mut digest = Sha256::new();
    let mut restante = esperado.tamanho;
    let mut buffer = [0u8; 8192];
    while restante > 0 {
        let tamanho = usize::try_from(restante.min(buffer.len() as u64))?;
        let lidos = file.read(&mut buffer[..tamanho])?;
        ensure!(lidos > 0, "arquivo anterior incompleto");
        digest.update(&buffer[..lidos]);
        restante -= lidos as u64;
    }
    let obtido: String = digest.finalize().hex();
    ensure!(obtido == esperado.sha256 && file.metadata()?.len() == esperado.tamanho, "hash do par anterior divergente");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_sem_par_exige_hash_e_sequencia_corretos() {
        let snapshot = Snapshot { anterior: None, motor_conf: Configuracao::Ausente };
        let bytes = serde_json::to_vec(&snapshot).unwrap();
        let hash = digest_hex(&bytes);
        assert!(ler(&bytes, &hash, None).is_ok());
        assert!(ler(&bytes, &hash, Some(1)).is_err());
        assert!(ler(&bytes, &"0".repeat(64), None).is_err());
    }
}
