//! Pacote de snapshot v1 em memória; não lê Git nem materializa arquivos.
use anyhow::{Result, ensure};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const MAX_ARQUIVOS: usize = 4096;
pub(super) const MAX_ARQUIVO_BYTES: u64 = 16 * 1024 * 1024;
pub(super) const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 240;
const DOMINIO: &[u8] = b"donna/onde-roda/snapshot/v1\0";

pub(super) struct Entrada {
    pub path: String,
    pub mode: u32,
    pub digest: [u8; 32],
    pub size: u64,
}

pub(super) struct Manifesto {
    pub version: u32,
    pub repo: String,
    pub commit: String,
    pub tree: String,
    pub arquivos: Vec<Entrada>,
    pub digest_total: [u8; 32],
}

/// Um blob por entrada, na mesma ordem; não há deduplicação nem arquivos implícitos.
pub(super) struct Blob {
    pub path: String,
    pub bytes: Vec<u8>,
}

pub(super) struct Pacote {
    pub manifesto: Manifesto,
    pub blobs: Vec<Blob>,
}

/// Conferido somente contra o manifesto; não prova vínculo com commit/árvore Git.
/// Só `conferir_manifesto` constrói o retorno imutável, que não habilita execução.
pub(super) struct PacoteConferidoManifesto<'a> {
    pacote: &'a Pacote,
}

impl PacoteConferidoManifesto<'_> {
    pub(super) fn manifesto(&self) -> &Manifesto {
        &self.pacote.manifesto
    }

    pub(super) fn arquivos(&self) -> impl Iterator<Item = (&Entrada, &[u8])> {
        self.pacote
            .manifesto
            .arquivos
            .iter()
            .zip(&self.pacote.blobs)
            .map(|(entrada, blob)| (entrada, blob.bytes.as_slice()))
    }
}

/// Confere apenas consistência com o manifesto recebido, não com o commit.
/// Limites são inclusivos e verificados antes de calcular hashes de conteúdo.
/// Não prova proveniência Git, autoridade, conteúdo seguro ou ausência de segredos.
pub(super) fn conferir_manifesto(pacote: &Pacote) -> Result<PacoteConferidoManifesto<'_>> {
    let m = &pacote.manifesto;
    ensure!(m.version == 1, "snapshot_version");
    ensure!(!m.repo.is_empty() && m.repo.len() <= 128 && m.repo.bytes().all(|b| b.is_ascii_graphic()), "snapshot_repo");
    ensure!(oid(&m.commit) && oid(&m.tree) && m.commit.len() == m.tree.len(), "snapshot_oid");
    ensure!(m.arquivos.len() <= MAX_ARQUIVOS, "snapshot_file_limit");
    ensure!(m.arquivos.len() == pacote.blobs.len(), "snapshot_blob_count");
    let mut nomes = BTreeSet::new();
    let mut grafias: BTreeMap<String, &str> = BTreeMap::new();
    let mut anterior: Option<&str> = None;
    let mut total = 0_u64;
    for (entrada, blob) in m.arquivos.iter().zip(&pacote.blobs) {
        validar_path(&entrada.path)?;
        for fim in entrada.path.match_indices('/').map(|(i, _)| i).chain(std::iter::once(entrada.path.len())) {
            let prefixo = &entrada.path[..fim];
            if let Some(grafia) = grafias.insert(prefixo.to_ascii_lowercase(), prefixo) {
                ensure!(grafia == prefixo, "snapshot_collision");
            }
        }
        ensure!(anterior.is_none_or(|p| p < entrada.path.as_str()), "snapshot_order");
        anterior = Some(&entrada.path);
        ensure!(matches!(entrada.mode, 0o100_644 | 0o100_755), "snapshot_mode");
        ensure!(nomes.insert(entrada.path.to_ascii_lowercase()), "snapshot_collision");
        ensure!(entrada.path == blob.path, "snapshot_blob_path");
        ensure!(entrada.size <= MAX_ARQUIVO_BYTES, "snapshot_file_bytes");
        ensure!(u64::try_from(blob.bytes.len())? == entrada.size, "snapshot_size");
        total = total.checked_add(entrada.size).ok_or_else(|| anyhow::anyhow!("snapshot_overflow"))?;
        ensure!(total <= MAX_TOTAL_BYTES, "snapshot_total_bytes");
    }
    // Conferir todos os pais também detecta diferenças de caixa entre níveis.
    for nome in &nomes {
        for (indice, _) in nome.match_indices('/') {
            ensure!(!nomes.contains(&nome[..indice]), "snapshot_file_parent");
        }
    }
    for (entrada, blob) in m.arquivos.iter().zip(&pacote.blobs) {
        ensure!(!blob.bytes.starts_with(b"version https://git-lfs.github.com/"), "snapshot_lfs_pending");
        let digest: [u8; 32] = Sha256::digest(&blob.bytes).into();
        ensure!(digest == entrada.digest, "snapshot_blob_digest");
    }
    ensure!(digest_manifesto(m)? == m.digest_total, "snapshot_total_digest");
    Ok(PacoteConferidoManifesto { pacote })
}

fn oid(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn validar_path(path: &str) -> Result<()> {
    ensure!(!path.is_empty() && path.len() <= MAX_PATH_BYTES, "snapshot_path");
    // Alfabeto conservador: evita Unicode ambíguo, separadores Windows e controles.
    ensure!(path.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-/".contains(&b)), "snapshot_path");
    for componente in path.split('/') {
        let nome = componente.to_ascii_lowercase();
        ensure!(
            !componente.is_empty() && !matches!(componente, "." | "..") && !componente.ends_with('.') && nome != ".git",
            "snapshot_path"
        );
        // Lista estreita, não DLP. Vale em qualquer nível do pacote.
        ensure!(nome != ".env" && nome != "auth.json", "snapshot_known_secret");
        let base = nome.split('.').next().unwrap_or_default();
        let numerado = base.len() == 4
            && (base.starts_with("com") || base.starts_with("lpt"))
            && matches!(base.as_bytes()[3], b'1'..=b'9');
        ensure!(!matches!(base, "con" | "prn" | "aux" | "nul") && !numerado, "snapshot_reserved_name");
    }
    Ok(())
}

// Codificação independente de JSON/serde e de endianness da máquina.
fn digest_manifesto(m: &Manifesto) -> Result<[u8; 32]> {
    fn campo(hash: &mut Sha256, bytes: &[u8]) -> Result<()> {
        hash.update(u32::try_from(bytes.len())?.to_be_bytes());
        hash.update(bytes);
        Ok(())
    }
    let mut hash = Sha256::new();
    hash.update(DOMINIO);
    hash.update(m.version.to_be_bytes());
    campo(&mut hash, m.repo.as_bytes())?;
    campo(&mut hash, m.commit.as_bytes())?;
    campo(&mut hash, m.tree.as_bytes())?;
    hash.update(u32::try_from(m.arquivos.len())?.to_be_bytes());
    for entrada in &m.arquivos {
        campo(&mut hash, entrada.path.as_bytes())?;
        hash.update(entrada.mode.to_be_bytes());
        hash.update(entrada.digest);
        hash.update(entrada.size.to_be_bytes());
    }
    Ok(hash.finalize().into())
}

#[path = "pacote_git.rs"]
pub(super) mod vinculo_git;

#[cfg(test)]
#[path = "pacote_tests.rs"]
mod tests;
