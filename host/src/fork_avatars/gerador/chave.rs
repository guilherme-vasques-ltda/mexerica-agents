//! A chave `OpenAI` do usuário no cofre existente, em slot dedicado do gerador.
//! Não reaproveita o slot de voz nem o de contas de motor (reuso por inferência proibido).
//! Nada além do Hub lê este registro; o renderer só escreve e pergunta o estado.
use crate::market::vault;
use crate::store::Store;
use anyhow::Result;
use serde::{Deserialize, Serialize};

const SLOT: &str = "mexeriquinhas";

#[derive(Default, Serialize, Deserialize)]
struct Chaves {
    #[serde(default)]
    openai: String,
}

fn load(store: &Store) -> Result<Chaves> {
    Ok(vault::read(store, SLOT)?
        .filter(|s| !s.is_empty())
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or_default())
}

/// `true` quando a credencial abre neste cofre; `Err` = cofre indisponível/fechado.
pub fn configurada(store: &Store) -> Result<bool> {
    Ok(!load(store)?.openai.is_empty())
}

/// Grava ou limpa (`""`) a chave do gerador; o valor nunca é devolvido nem registrado.
pub fn gravar(store: &Store, chave: &str) -> Result<()> {
    let mut chaves = load(store)?;
    chaves.openai = String::from(chave);
    if chaves.openai.is_empty() {
        store.kv_delete(SLOT)
    } else {
        vault::write(store, SLOT, &serde_json::to_string(&chaves)?)
    }
}

/// A credencial só sai daqui dentro do adaptador do provedor.
pub fn resolver(store: &Store) -> Result<zeroize::Zeroizing<String>> {
    let key = load(store)?.openai;
    if key.is_empty() {
        return Err(super::falha("GERADOR_INDISPONIVEL"));
    }
    Ok(zeroize::Zeroizing::new(key))
}
