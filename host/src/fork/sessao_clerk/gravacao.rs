//! Gravação do vínculo (`workspaceBind`), só no computador do Hub e atrás de
//! [`super::GRAVACAO_VINCULO_LIBERADA`], desligado. Grava o primeiro vínculo `personal` ou regrava o
//! mesmo workspace (rotação de emissor ou de JWKS). A troca de dono ou de workspace é recusada: ela
//! exige recomeçar com dados novos, numa fatia própria.
//!
//! A autoridade é a prova Clerk recente do dono do vínculo proposto: token verificado contra a JWKS
//! proposta (busca avulsa, só HTTPS, na origem do emissor e só para endereço público), `iat` de no
//! máximo 60 s e `sub` igual ao `ownerUserId`. `Caller::Local` não distingue o app de outro processo
//! local com o bearer; por isso a prova é obrigatória. O token nunca vai a log nem a erro.
//!
//! Na mesma Tx, sob o `team_lock`: relê o vínculo e confere o compare-and-swap, grava o vínculo,
//! apaga as ligações `userId` dos aparelhos e os papéis registrados e sobe o `workspaceEpoch`
//! (`workspace_revoked`). Canais, guardas e recados guardados de antes não atravessam o commit.
use super::{CAMPO_TOKEN, Recusa, token, vinculo};
use crate::LockExt;
use crate::api::GroupError;
use crate::api::devices::{Caller, Forbidden};
use crate::hub::Hub;
use crate::store::now_ms;
use anyhow::Result;
use rusqlite::TransactionBehavior;
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) const METHOD: &str = "workspaceBind";
/// Idade máxima do token que prova o dono (segundos desde o `iat`).
const RECENTE_S: i64 = 60;

fn erro(code: &'static str, message: &'static str) -> anyhow::Error {
    GroupError { code, message }.into()
}

fn recusa(r: Recusa) -> anyhow::Error {
    erro(r.code(), r.message())
}

/// O vínculo proposto, no formato gravado. Só `personal` nesta fatia.
fn proposto(b: &Value) -> Result<(String, vinculo::Vinculo)> {
    let invalido = || erro("invalid_field", "Vínculo proposto inválido.");
    if b["kind"].as_str() != Some("personal") {
        return Err(erro("workspace_kind_unsupported", "Só o vínculo pessoal pode ser gravado."));
    }
    let raw = json!({
        "schema": 1,
        "kind": "personal",
        "ownerUserId": b["ownerUserId"],
        "issuer": b["issuer"],
        "jwksUrl": b["jwksUrl"],
        "authorizedParties": b["authorizedParties"],
    })
    .to_string();
    let v = vinculo::interpretar(&raw).ok_or_else(invalido)?;
    // Nome DNS, nunca IP literal: o destino público é conferido na resolução.
    if v.jwks.host_str().is_none_or(|h| h.starts_with('[') || h.parse::<std::net::IpAddr>().is_ok()) {
        return Err(invalido());
    }
    Ok((raw, v))
}

pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, b: &Value) -> Result<Value> {
    if !matches!(caller, Caller::Local) {
        return Err(Forbidden.into());
    }
    if !hub.sessao_clerk.gravacao_liberada() {
        return Err(erro("workspace_bind_disabled", "A gravação do vínculo está desligada."));
    }
    let (raw, v) = proposto(b)?;
    vinculo::conferir_gravacao(&v, hub.sessao_clerk.time_liberado()).map_err(recusa)?;
    let esperado = match &b["expectedWorkspaceId"] {
        Value::Null => None,
        Value::String(id) if !id.is_empty() => Some(id.clone()),
        _ => return Err(erro("invalid_field", "expectedWorkspaceId inválido.")),
    };
    let token = b[CAMPO_TOKEN].as_str().ok_or_else(|| recusa(Recusa::AuthInvalid))?;
    let partes = token::separar(token).map_err(recusa)?;
    let chave = hub.sessao_clerk.jwks.chave_proposta(&v.jwks, &partes.kid).await.map_err(recusa)?;
    let agora = now_ms().div_euclid(1000);
    let afirmacoes = token::conferir(&partes, &chave, &v, agora).map_err(recusa)?;
    if agora.saturating_sub(afirmacoes.iat) > RECENTE_S {
        return Err(recusa(Recusa::AuthInvalid));
    }
    if v.papel(&afirmacoes) != Ok(vinculo::Papel::Dono) {
        return Err(recusa(Recusa::WorkspaceForbidden));
    }
    let novo = v.workspace_id();
    let _equipe = hub.store.travar_equipe().await;
    let mut db = hub.store.db.locked();
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let atual = match vinculo::ler_on(&tx) {
        vinculo::Estado::SemVinculo => None,
        vinculo::Estado::Vinculado(atual) => Some(atual.workspace_id()),
        vinculo::Estado::Ilegivel => return Err(erro("workspace_bind_conflict", "O vínculo gravado está ilegível.")),
    };
    if atual != esperado {
        return Err(erro("workspace_bind_conflict", "O vínculo mudou desde a leitura."));
    }
    if atual.as_ref().is_some_and(|id| *id != novo) {
        return Err(erro("workspace_swap_unsupported", "Trocar o dono do Hub ainda não é possível."));
    }
    if now_ms().div_euclid(1000) >= afirmacoes.exp {
        return Err(recusa(Recusa::AuthInvalid));
    }
    tx.execute(
        "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
        rusqlite::params![vinculo::CHAVE, raw],
    )?;
    tx.execute("DELETE FROM fork_sessao_aparelho_usuario", [])?;
    tx.execute("DELETE FROM fork_sessao_papel", [])?;
    let epoch = crate::fork::workspace::subir_na_tx(hub, tx, crate::fork::workspace::Motivo::AcessoRetirado)?;
    tracing::info!(kind = "personal", epoch, "workspace binding written");
    Ok(json!({"workspaceId": novo, "workspaceEpoch": epoch}))
}
