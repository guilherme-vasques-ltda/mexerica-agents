//! Política lida exclusivamente de estado administrado por root.
use super::{armazenamento::Diretorio, manifesto, rede};
use anyhow::{Context, Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Politica {
    pub endpoint: String,
    pub canal: String,
    pub arquitetura: String,
    pub uid_hub: u32,
    pub gid_hub: u32,
    pub chave_manifesto: String,
    pub tamanho_maximo: u64,
    pub tempo_rede_segundos: u64,
    pub janela_minima: i64,
    pub janela_maxima: i64,
    pub cgroup_memoria: u64,
    pub cgroup_processos: u64,
    pub cgroup_cpu_quota: u64,
    pub cgroup_cpu_periodo: u64,
}

impl Politica {
    pub fn ler(dir: &Diretorio) -> Result<Self> {
        let politica: Self = dir.ler("politica")?;
        politica.validar()?;
        Ok(politica)
    }

    pub fn validar(&self) -> Result<()> {
        ensure!(self.uid_hub != 0 && self.gid_hub != 0, "identidade do Hub inválida");
        ensure!(
            !self.canal.is_empty()
                && self.canal.len() <= 64
                && self.canal.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'),
            "canal inválido"
        );
        ensure!(self.arquitetura == std::env::consts::ARCH, "arquitetura incompatível");
        ensure!(self.tamanho_maximo > 0 && self.tamanho_maximo <= 2 * 1024 * 1024 * 1024, "tamanho máximo inválido");
        ensure!(
            self.janela_minima > 0
                && self.janela_maxima >= self.janela_minima
                && self.janela_maxima <= 7 * 24 * 60 * 60,
            "janela inválida"
        );
        ensure!(
            self.cgroup_memoria > 0
                && self.cgroup_processos > 0
                && self.cgroup_cpu_quota > 0
                && self.cgroup_cpu_periodo > 0,
            "cgroup inválido"
        );
        self.chave()?;
        rede::validar_endpoint(&self.endpoint, self.tamanho_maximo, self.tempo_rede_segundos)?;
        Ok(())
    }

    pub fn chave(&self) -> Result<[u8; 32]> {
        let bytes = B64.decode(&self.chave_manifesto).context("chave pública inválida")?;
        let bytes: [u8; 32] = bytes.try_into().map_err(|_| anyhow::anyhow!("tamanho de chave inválido"))?;
        ed25519_dalek::VerifyingKey::from_bytes(&bytes).context("chave pública inválida")?;
        Ok(bytes)
    }

    pub fn manifesto<'a>(&'a self, wrapper: &'a semver::Version) -> manifesto::Politica<'a> {
        manifesto::Politica {
            canal: &self.canal,
            arquitetura: &self.arquitetura,
            wrapper,
            tamanho_maximo: self.tamanho_maximo,
            janela_minima: self.janela_minima,
            janela_maxima: self.janela_maxima,
        }
    }
}
