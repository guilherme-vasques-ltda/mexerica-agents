//! Diagnóstico da última falha em arquivo root 0600 fixo; sem saída ao chamador.
use super::{armazenamento::Diretorio, executar::ESTADO};
use std::path::Path;

/// Melhor esforço: a falha original continua decidindo o código de saída.
pub fn registrar(erro: &anyhow::Error) {
    let mut texto = format!("{erro:#}");
    if texto.len() > 4000 {
        let mut fim = 4000;
        while !texto.is_char_boundary(fim) {
            fim -= 1;
        }
        texto.truncate(fim);
    }
    if let Ok(estado) = Diretorio::abrir(Path::new(ESTADO)) {
        let _ = estado.gravar_bytes("diagnostico", texto.as_bytes());
    }
}
