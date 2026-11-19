//! Processamento do atlas 3×3 → nove quadros × dois tamanhos (64/256) em WebP.
//! PENDENTE DA APROVAÇÃO DO DONO: a implementação usa o pacote `image` (proposta na
//! Geral `thr_eb7fjnzt3s`). Até lá a fronteira existe e falha explicitamente, sem
//! improvisar fatiamento fora do Hub nem decoder PNG escrito à mão.
use anyhow::Result;
use std::path::Path;

pub struct Medidas {
    pub bytes: u64,
}

/// Existe processador de atlas? Falso enquanto a dependência de imagem não é
/// aprovada — toda geração paga é recusada antes de reservar cota/chamada.
pub fn disponivel() -> bool {
    false
}

/// PNG do atlas → `<destino>/{64,256}/<quadro>.webp` + verificação de modo.
/// Enquanto a dependência de imagem não é aprovada, falha com código estável.
pub fn processar(atlas_png: &[u8], destino: &Path) -> Result<Medidas> {
    let _ = (atlas_png, destino);
    Err(super::falha("GERADOR_SEM_PROCESSADOR"))
}
