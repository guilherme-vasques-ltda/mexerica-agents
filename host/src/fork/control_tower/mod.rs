//! VG-01: leitura dos produtores atuais; sem fila, ledger ou estado próprio.
mod access;
mod attention;
mod delegated_permissions;
mod events;
mod model;
mod projection;
mod roster_events;

pub(crate) use access::{caller_copy, require};
pub(crate) use events::{attention_hint, attention_signal, event, runtime_changed};
pub(crate) use model::Observation;
pub(crate) use roster_events::{deletion, primary_change};

use crate::{api::devices::Caller, hub::Hub};
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;

pub(crate) const METHOD: &str = "donnaControlTower";
pub(crate) const ATTENTION_METHOD: &str = "donnaAttention";
pub(crate) const ATTENTION_EVENT: &str = "donnaAttentionChanged";
pub(crate) const EVENT: &str = "donnaControlTowerChanged";
pub(crate) const PENDING_PERMISSION: &str = "e.kind='permission' AND json_extract(e.data,'$.status')='pending'";
pub(crate) const INSTRUCTIONS: &str = "O Mission Board é uma tela do app na faixa lateral, que lê uma \
projeção do Hub; não há ferramenta para bots. Ela mostra estado e pendências; a pessoa pode responder ali ao \
mesmo cartão de permissão da conversa, com as mesmas regras, ou abrir a conversa. Parar é pelo controle que já \
existe na conversa (Abrir para parar). Abrir conversa do bot só navega para uma conversa \
já autorizada; não envia, aprova, retoma ou assume trabalho. Sem referência exata de execução, não prometa Ver \
andamento. Fila geral e uso diário por bot não estão disponíveis; ausência não é zero nem Livre. \
Pausa pública e Ver andamento estão indisponíveis nesta etapa. Não use RPCs por shell, não crie outra fila \
e não confunda worker de fundo por função com bot especialista. No Pessoal, o Mission Board mostra ao dono os \
pedidos entre bots dos últimos 7 dias que não terminaram bem, só com Abrir e sem retomar; o bot continua usando \
bot_request_status para o próprio pedido. No Pessoal, a coluna O que aconteceu mostra o resumo do último dia \
fechado, por Time: o Hub o escreve numa chamada isolada por bot, sem ferramentas e sem memória; não é ferramenta \
sua, não manda mensagem a ninguém e não muda o quadro nem permissões.";

pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, body: Value) -> Result<Value> {
    let request: model::Read = serde_json::from_value(body)
        .map_err(|_| anyhow::anyhow!("invalid_field: solicitação da Torre inválida ou campo desconhecido."))?;
    // Mesma fronteira de mutações administrativas; a espera não conserva autorização velha.
    let _guard = hub.store.team_lock.lock().await;
    require(hub, caller)?;
    let answer = projection::snapshot(hub, &request.space_id)?;
    require(hub, caller)?;
    hub.store.require_space(&request.space_id)?;
    Ok(serde_json::to_value(answer)?)
}

pub(crate) async fn attention_call(hub: &Arc<Hub>, caller: &Caller, body: Value) -> Result<Value> {
    let request: attention::Read = serde_json::from_value(body)
        .map_err(|_| anyhow::anyhow!("invalid_field: solicitação inválida ou campo desconhecido."))?;
    let _guard = hub.store.team_lock.lock().await;
    require(hub, caller)?;
    hub.store.require_space(&request.space_id)?;
    let value = attention::project(hub, &request.space_id, request.include_resolved_today)?;
    let answer = serde_json::to_value(value)?;
    hub.store.require_space(&request.space_id)?;
    require(hub, caller)?;
    Ok(answer)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod delegated_permissions_tests;

#[cfg(test)]
mod attention_tests;
