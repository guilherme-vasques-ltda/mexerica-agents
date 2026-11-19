//! Prova interna de vínculo Git; necessária, nunca suficiente para execução.
use super::{Pacote, PacoteConferidoManifesto};
use anyhow::{Result, anyhow, ensure};
use std::path::Path;
use std::sync::atomic::AtomicBool;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "snapshot_git.rs"]
mod git;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "snapshot_linux.rs"]
mod linux;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "snapshot_odb.rs"]
mod odb;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "snapshot.rs"]
mod snapshot;

/// Formato admitido pelo chamador confiável; nunca inferido dos OIDs recebidos.
#[derive(Clone, Copy)]
pub(in super::super) enum FormatoGit {
    Sha1,
    Sha256,
}

impl FormatoGit {
    fn largura(self) -> usize {
        match self {
            Self::Sha1 => 40,
            Self::Sha256 => 64,
        }
    }
}

/// Seleção feita pelo chamador confiável, separada do pacote não confiável.
/// Não deriva de flags, manifesto, configuração ou revisão simbólica Git.
pub(in super::super) struct SelecaoGit<'a> {
    pub repo: &'a str,
    pub formato: FormatoGit,
    pub commit: &'a str,
    pub tree: &'a str,
}

impl SelecaoGit<'_> {
    fn conferir(&self) -> Result<()> {
        ensure!(super::oid(self.commit) && super::oid(self.tree), "snapshot_git_oid");
        ensure!(
            self.commit.len() == self.formato.largura() && self.tree.len() == self.formato.largura(),
            "snapshot_git_format"
        );
        ensure!(
            !self.repo.is_empty() && self.repo.len() <= 128 && self.repo.bytes().all(|b| b.is_ascii_graphic()),
            "snapshot_repo"
        );
        Ok(())
    }
}

/// Campo privado; somente este módulo emite após conferir o grafo completo.
/// O empréstimo conserva exatamente o pacote associado à prova.
pub(in super::super) struct PacoteConferidoGit<'a> {
    _pacote: PacoteConferidoManifesto<'a>,
}

/// Operação interna síncrona e cancelável; não extrai nem executa ajudantes.
/// `odb` é a localização admitida pelo chamador, nunca obtida do pacote.
pub(in super::super) fn conferir_git<'a>(
    pacote: &'a Pacote,
    selecao: &SelecaoGit<'_>,
    odb: &Path,
    cancelar: &AtomicBool,
) -> Result<PacoteConferidoGit<'a>> {
    selecao.conferir()?;
    ensure!(pacote.manifesto.repo == selecao.repo, "snapshot_git_repo");
    ensure!(
        pacote.manifesto.commit == selecao.commit && pacote.manifesto.tree == selecao.tree,
        "snapshot_git_selection"
    );
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        let manifesto = snapshot::conferir(pacote, selecao, odb, cancelar)?;
        Ok(PacoteConferidoGit { _pacote: manifesto })
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        let _ = (odb, cancelar);
        anyhow::bail!("snapshot_git_platform");
    }
}

/// Não aceita prova somente de manifesto. Não inicia nem autoriza execução.
pub(in super::super) fn exigir_vinculo_git(pacote: Option<PacoteConferidoGit<'_>>) -> Result<PacoteConferidoGit<'_>> {
    pacote.ok_or_else(|| anyhow!("snapshot_git_nao_conferido"))
}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
#[path = "snapshot_tests.rs"]
mod tests;
