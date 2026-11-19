use anyhow::{Context, Result, ensure};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

/// Os bytes assinados são este prefixo seguido do JSON compacto produzido por
/// `serde_json::to_vec` com a ordem de campos definida pelas estruturas abaixo.
pub const DOMINIO: &[u8] = b"mexerica/atualizacao-privilegiada/manifesto/v1\0";
pub const MAX_MANIFESTO: usize = 16 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artefato {
    pub nome: String,
    pub tamanho: u64,
    pub sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifesto {
    pub produto: String,
    pub canal: String,
    pub arquitetura: String,
    pub versao: String,
    pub sequencia: u64,
    pub inicio: i64,
    pub expiracao: i64,
    pub hub: Artefato,
    pub ajudante: Artefato,
    pub sequencia_minima_reversivel: u64,
    pub wrapper_minimo: String,
}

pub struct Politica<'a> {
    pub canal: &'a str,
    pub arquitetura: &'a str,
    pub wrapper: &'a semver::Version,
    pub tamanho_maximo: u64,
    pub janela_minima: i64,
    pub janela_maxima: i64,
}

/// A assinatura é conferida sobre bytes limitados antes do parse do manifesto.
/// Esta função não substitui a verificação TUF obrigatória.
pub fn verificar(bytes: &[u8], assinatura: &[u8], chave: &[u8; 32]) -> Result<Manifesto> {
    ensure!(!bytes.is_empty() && bytes.len() <= MAX_MANIFESTO, "tamanho do manifesto inválido");
    let signature = Signature::from_slice(assinatura).context("assinatura inválida")?;
    let key = VerifyingKey::from_bytes(chave).context("chave inválida")?;
    let mut mensagem = Vec::with_capacity(DOMINIO.len() + bytes.len());
    mensagem.extend_from_slice(DOMINIO);
    mensagem.extend_from_slice(bytes);
    key.verify_strict(&mensagem, &signature).context("assinatura do manifesto recusada")?;
    let manifesto: Manifesto = serde_json::from_slice(bytes).context("manifesto inválido")?;
    ensure!(serde_json::to_vec(&manifesto)? == bytes, "codificação do manifesto não canônica");
    Ok(manifesto)
}

pub fn validar(m: &Manifesto, p: &Politica<'_>, agora: i64, marca: u64, piso: u64) -> Result<i64> {
    ensure!(p.janela_minima > 0 && p.janela_maxima >= p.janela_minima, "política de janela inválida");
    ensure!(m.produto == "mexerica" && m.canal == p.canal && m.arquitetura == p.arquitetura, "release incompatível");
    ensure!(m.sequencia > marca, "sequência não avança");
    ensure!(m.sequencia_minima_reversivel >= piso && m.sequencia_minima_reversivel <= m.sequencia, "piso inválido");
    ensure!(m.inicio <= agora && agora < m.expiracao, "manifesto fora da validade");
    let restante = m.expiracao.checked_sub(agora).context("validade inválida")?;
    ensure!(restante >= p.janela_minima, "validade restante insuficiente");
    ensure!(semver::Version::parse(&m.versao).is_ok(), "versão inválida");
    ensure!(semver::Version::parse(&m.wrapper_minimo)? <= *p.wrapper, "wrapper antigo");
    for (artefato, nome) in [(&m.hub, "donnabot-host"), (&m.ajudante, "donnabot-motor-exec")] {
        ensure!(
            artefato.nome == nome && artefato.tamanho > 0 && artefato.tamanho <= p.tamanho_maximo,
            "artefato inválido"
        );
        ensure!(
            artefato.sha256.len() == 64
                && artefato.sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "hash inválido"
        );
    }
    Ok(m.expiracao.min(agora.checked_add(p.janela_maxima).context("prazo inválido")?))
}
