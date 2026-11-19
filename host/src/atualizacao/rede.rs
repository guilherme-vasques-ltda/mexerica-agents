//! Transporte restrito à origem de uma política autenticada pelo administrador.
#[cfg(target_os = "linux")]
use super::Hex as _;
use anyhow::{Context, Result, ensure};
use reqwest::{Client, Url, redirect::Policy};
use std::{io::Write, time::Duration};

pub enum Recurso {
    Timestamp,
    Snapshot,
    Targets,
    Manifesto,
    Assinatura,
    Hub,
    Ajudante,
    Raiz(u64),
    Delegado(String),
}
impl Recurso {
    fn caminho(&self) -> Result<String> {
        let caminho = match self {
            Self::Timestamp => "timestamp.json".into(),
            Self::Snapshot => "snapshot.json".into(),
            Self::Targets => "targets.json".into(),
            Self::Manifesto => "manifesto.json".into(),
            Self::Assinatura => "manifesto.sig".into(),
            Self::Hub => "donnabot-host".into(),
            Self::Ajudante => "donnabot-motor-exec".into(),
            Self::Raiz(versao) => {
                ensure!(*versao > 0, "versão de raiz inválida");
                format!("{versao}.root.json")
            }
            Self::Delegado(nome) => {
                ensure!(
                    !nome.is_empty()
                        && nome.len() <= 64
                        && nome.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
                    "papel delegado inválido"
                );
                format!("{nome}.json")
            }
        };
        Ok(caminho)
    }
}

pub struct Transporte {
    cliente: Client,
    origem: Url,
    limite: u64,
}
fn mesma_origem(a: &Url, b: &Url) -> bool {
    a.scheme() == "https"
        && b.scheme() == "https"
        && a.host() == b.host()
        && a.port_or_known_default() == b.port_or_known_default()
        && b.username().is_empty()
        && b.password().is_none()
}
pub fn validar_endpoint(endpoint: &str, limite: u64, tempo_segundos: u64) -> Result<Url> {
    ensure!(limite > 0 && tempo_segundos > 0 && tempo_segundos <= 600, "limites de rede inválidos");
    let origem = Url::parse(endpoint).context("endpoint inválido")?;
    ensure!(
        origem.scheme() == "https"
            && origem.host().is_some()
            && origem.username().is_empty()
            && origem.password().is_none()
            && origem.query().is_none()
            && origem.fragment().is_none()
            && origem.path().ends_with('/'),
        "endpoint HTTPS inválido"
    );
    Ok(origem)
}
impl Transporte {
    pub fn novo(endpoint: &str, limite: u64, tempo_segundos: u64) -> Result<Self> {
        let origem = validar_endpoint(endpoint, limite, tempo_segundos)?;
        let permitida = origem.clone();
        let redirect = Policy::custom(move |tentativa| {
            if tentativa.previous().len() >= 5 || !mesma_origem(&permitida, tentativa.url()) {
                tentativa.error("redirecionamento recusado")
            } else {
                tentativa.follow()
            }
        });
        let cliente = Client::builder()
            .no_proxy()
            .https_only(true)
            .redirect(redirect)
            .connect_timeout(Duration::from_secs(tempo_segundos.min(30)))
            .timeout(Duration::from_secs(tempo_segundos))
            .build()?;
        Ok(Self { cliente, origem, limite })
    }

    /// Destino deve ser arquivo exclusivo já aberto no diretório temporário privado.
    pub async fn baixar(&self, recurso: Recurso, destino: &mut impl Write, maximo: u64) -> Result<u64> {
        self.baixar_se_existir(recurso, destino, maximo).await?.context("recurso ausente na origem")
    }

    /// Como `baixar`, mas devolve `None` somente para 404 (ex.: próxima raiz TUF ainda não publicada).
    pub async fn baixar_se_existir(
        &self,
        recurso: Recurso,
        destino: &mut impl Write,
        maximo: u64,
    ) -> Result<Option<u64>> {
        ensure!(maximo > 0 && maximo <= self.limite, "limite de download inválido");
        let url = self.origem.join(&recurso.caminho()?)?;
        ensure!(mesma_origem(&self.origem, &url), "origem divergente");
        let resposta = self.cliente.get(url).send().await?;
        if resposta.status() == reqwest::StatusCode::NOT_FOUND {
            ensure!(mesma_origem(&self.origem, resposta.url()), "origem final divergente");
            return Ok(None);
        }
        let mut resposta = resposta.error_for_status()?;
        ensure!(mesma_origem(&self.origem, resposta.url()), "origem final divergente");
        if let Some(tamanho) = resposta.content_length() {
            ensure!(tamanho <= maximo, "download grande demais");
        }
        let mut total = 0u64;
        while let Some(bytes) = resposta.chunk().await? {
            total = total.checked_add(u64::try_from(bytes.len())?).context("download grande demais")?;
            ensure!(total <= maximo, "download grande demais");
            destino.write_all(&bytes)?;
        }
        ensure!(total > 0, "download vazio");
        destino.flush()?;
        Ok(Some(total))
    }
}

#[cfg(target_os = "linux")]
struct HashWriter<'a> {
    file: &'a mut std::fs::File,
    hash: sha2::Sha256,
}
#[cfg(target_os = "linux")]
impl Write for HashWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let quantidade = self.file.write(bytes)?;
        sha2::Digest::update(&mut self.hash, &bytes[..quantidade]);
        Ok(quantidade)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

/// Escreve somente em inode root recém-criado; tamanho e hash vêm do manifesto
/// autenticado. A função não publica nem modifica permissões do arquivo.
#[cfg(target_os = "linux")]
pub async fn baixar_artefato(
    transporte: &Transporte,
    recurso: Recurso,
    destino: &mut std::fs::File,
    artefato: &super::manifesto::Artefato,
) -> Result<()> {
    use sha2::{Digest as _, Sha256};
    use std::{io::Seek, os::unix::fs::MetadataExt};
    ensure!(
        matches!(
            (&recurso, artefato.nome.as_str()),
            (Recurso::Hub, "donnabot-host") | (Recurso::Ajudante, "donnabot-motor-exec")
        ),
        "nome lógico divergente"
    );
    let meta = destino.metadata()?;
    ensure!(
        meta.is_file()
            && meta.uid() == 0
            && meta.gid() == 0
            && meta.nlink() == 1
            && meta.mode() & 0o7777 == 0o600
            && meta.len() == 0,
        "destino de download inseguro"
    );
    ensure!(destino.stream_position()? == 0, "posição de arquivo inválida");
    let mut escrita = HashWriter { file: destino, hash: Sha256::new() };
    let tamanho = transporte.baixar(recurso, &mut escrita, artefato.tamanho).await?;
    ensure!(tamanho == artefato.tamanho, "artefato incompleto");
    let digest = escrita.hash.finalize();
    let obtido: String = digest.hex();
    ensure!(obtido == artefato.sha256, "hash do artefato divergente");
    escrita.file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn origem_e_nomes_nao_aceitam_desvio() {
        let origem = Url::parse("https://exemplo.invalid/releases/").unwrap();
        assert!(mesma_origem(&origem, &Url::parse("https://exemplo.invalid/outro").unwrap()));
        assert!(!mesma_origem(&origem, &Url::parse("https://outro.invalid/releases/").unwrap()));
        assert!(!mesma_origem(&origem, &Url::parse("http://exemplo.invalid/releases/").unwrap()));
        assert!(Recurso::Delegado("../alvo".into()).caminho().is_err());
        assert!(Recurso::Delegado("alvo?x".into()).caminho().is_err());
        assert_eq!(Recurso::Delegado("release-1".into()).caminho().unwrap(), "release-1.json");
        assert!(Recurso::Raiz(0).caminho().is_err());
        assert_eq!(Recurso::Assinatura.caminho().unwrap(), "manifesto.sig");
    }
}
