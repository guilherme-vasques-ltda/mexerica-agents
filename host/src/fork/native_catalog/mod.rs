//! Biblioteca autoral: prévia e inclusão explícita; sem ferramenta de bot ou bootstrap.
mod add;
mod catalog;
mod model;
mod store;
pub(crate) mod workspace;
pub(crate) use add::add;
pub(crate) use store::{migrate, on_bot_deleted};

use crate::{LockExt, api::devices::Caller, hub::Hub};
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;

pub(crate) const METHOD: &str = "donnaNativeCatalog";
pub(crate) const INSTRUCTIONS: &str = "Biblioteca de especialistas da Mexerica: prévias são perfis, não bots instalados. \
Só o usuário inclui explicitamente pelo app, um por perfil/Space, em Pedir e sem acessos herdados. \
Adicionar não inicia trabalho, rotina ou gasto. Remover não recria; readicionar começa outro bot sem restaurar memória/histórico. \
Não use RPCs do catálogo pelo shell ou como ferramenta de bot; Local/Device não provam autoria humana de turno. \
Nome, função, Alma, backend e recibo não concedem conta, recurso ou permissão. Só uma RPC concluída comprova inclusão; \
resultado incerto conserva a chave e exige releitura no mesmo Hub/Space, sem nova inclusão por inferência. \
Recibo conhecido ainda resolve se o backend sair do registro, com autoridade/audiência atuais; isso não valida nova criação ou execução.";

pub(crate) const ADD_METHOD: &str = "donnaNativeCatalogAdd";
pub(crate) fn methods(method: &str) -> bool {
    matches!(method, METHOD | ADD_METHOD)
}

pub(crate) fn read(hub: &Arc<Hub>, caller: &Caller, body: Value) -> Result<Value> {
    super::bot_editor::authorization::require_owner(caller)?;
    let request: model::Read = serde_json::from_value(body)
        .map_err(|_| anyhow::anyhow!("invalid_field: solicitação do catálogo inválida ou campo desconhecido."))?;
    let profiles = catalog::profiles()?;
    let db = hub.store.db.locked();
    super::spaces::require_space(&db, &request.space_id)?;
    let links = profiles.iter().map(|p| store::link(&db, &request.space_id, p.catalog_key)).collect::<Result<_>>()?;
    // Sem editor read/reconcile, ator ou escrita. Somente vínculo do Space solicitado.
    serde_json::to_value(model::Preview {
        hub_id: hub.host_id.clone(),
        space_id: request.space_id,
        catalog_version: catalog::VERSION,
        profiles,
        links,
    })
    .map_err(Into::into)
}

#[cfg(test)]
mod add_tests;
#[cfg(test)]
mod tests;
