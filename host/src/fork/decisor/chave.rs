//! Chave do decisor no cofre do produto (`market::vault`, o mesmo das outras chaves de provedor).
//! Nunca em log, erro, KV em texto plano ou resposta de API.
use super::{Chave, ProvedorDecisor};
use crate::{market::vault, store::Store};
use anyhow::{Context, Result};

fn slot(provedor: ProvedorDecisor) -> String {
    format!("fork.decisor.chave.v1:{}", provedor.id())
}

/// Costura da Cloud: "plataforma = `OpenRouter` com chave injetada pelo backend Cloud". A
/// implementação server-side (a chave real vem por formulário de segredos, só quando for ligar)
/// não existe nesta fatia; nenhum valor real é pedido ou configurado aqui.
pub(crate) trait FontePlataforma: Send + Sync {
    fn chave_openrouter(&self) -> Option<Chave>;
}

pub(crate) fn salvar(store: &Store, provedor: ProvedorDecisor, valor: &str) -> Result<()> {
    let chave = Chave::nova(valor).map_err(|e| anyhow::anyhow!("{e}"))?;
    vault::write(store, &slot(provedor), chave.expor()).context("Não foi possível guardar a chave do decisor")
}

pub(crate) fn remover(store: &Store, provedor: ProvedorDecisor) -> Result<()> {
    store.kv_delete(&slot(provedor))
}

/// Chave do USUÁRIO. Qualquer falha (cofre trancado, registro ruim) vira `None` = Fixo, sem detalhe.
pub(crate) fn do_usuario(store: &Store, provedor: ProvedorDecisor) -> Option<Chave> {
    let valor = vault::read(store, &slot(provedor)).ok()??;
    Chave::nova(&valor).ok()
}
