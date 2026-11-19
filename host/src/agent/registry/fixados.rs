use super::PacoteBinario;
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::LazyLock;

#[derive(Deserialize)]
struct Fixados {
    cursor: Agente,
    devin: Agente,
}

#[derive(Deserialize)]
pub(super) struct Agente {
    versao: String,
    plataformas: BTreeMap<String, Artefato>,
}

#[derive(Deserialize)]
struct Artefato {
    url: String,
    sha256: String,
    cmd: String,
    bytes: usize,
    bytes_extraidos: u64,
}

static FIXADOS: LazyLock<Fixados> = LazyLock::new(|| {
    serde_json::from_str(include_str!("fixados.json")).expect("dados de pinagem compilados devem ser válidos")
});

pub(super) fn agente(entrada: &Value) -> Option<&'static Agente> {
    match super::id_normalizado(entrada)?.as_str() {
        "cursor" => Some(&FIXADOS.cursor),
        "devin" => Some(&FIXADOS.devin),
        _ => None,
    }
}

impl Agente {
    pub(super) fn selecionar(&self, entrada: &Value, plataforma: Option<&str>) -> Result<PacoteBinario<'_>> {
        ensure!(
            entrada["version"].as_str() == Some(&self.versao),
            "A versão do catálogo não é a versão verificada ({}). A instalação foi recusada.",
            self.versao
        );
        let erro = "Não há um binário verificado deste agente para esta plataforma. A instalação foi recusada.";
        let plataforma = plataforma.context(erro)?;
        let artefato = self.plataformas.get(plataforma).context(erro)?;
        ensure!(
            entrada["distribution"]["binary"][plataforma]["archive"].as_str() == Some(&artefato.url),
            "O endereço do binário mudou no catálogo. A instalação foi recusada."
        );
        ensure!(super::sha256_valido(&artefato.sha256), "O SHA-256 fixado para esta plataforma é inválido.");
        Ok(PacoteBinario {
            url: &artefato.url,
            hash: &artefato.sha256,
            cmd: &artefato.cmd,
            limite_download: artefato.bytes,
            limite_extraido: artefato.bytes_extraidos,
        })
    }
}

#[cfg(test)]
#[path = "fixados_testes.rs"]
mod testes;
