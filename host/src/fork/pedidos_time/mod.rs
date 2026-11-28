//! Aba "Time" do painel do bot: os pedidos que o bot fez a colegas, com quem pegou, o que foi pedido e como está.
//! O estado vem do registro durável (`retornos::pendentes`); o texto, do aviso já gravado na conversa de quem
//! pediu. Só o dono e só o Space do bot. Pedido com restrição de privacidade fica de fora (`ler_na` sem leitor).
use crate::{
    api::devices::Caller,
    fork::{
        autoridade::Autoridade,
        retornos::{
            Desfecho, Modo,
            consulta::Estado,
            pendentes::{self, Filtro, Pedido},
        },
        spaces::{self, PERSONAL},
    },
    hub::Hub,
    store::{Store, now_ms},
};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

pub(crate) const METHOD: &str = "donnaTeamRequests";
/// Terminados aparecem por 24 h; abertos, enquanto o registro os tiver na janela de `pendentes`.
pub(crate) const TERMINADOS_MS: i64 = 24 * 60 * 60 * 1000;
pub(crate) const LIMITE: usize = 30;
/// Texto do pedido no painel; o inteiro continua na conversa.
const TEXTO_MAX: usize = 600;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Read {
    bot_id: String,
    space_id: Option<String>,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TeamRequests {
    pub bot_id: String,
    pub space_id: String,
    pub items: Vec<TeamRequest>,
    pub omitted: usize,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TeamRequest {
    /// ID opaco do pedido, só para a key da lista.
    pub key: String,
    pub target_bot_id: String,
    /// `None` quando o aviso sumiu ou não confere com o registro.
    pub text: Option<String>,
    /// `queued`, `running`, `finished`, `cancelled` ou `interrupted`.
    pub state: &'static str,
    pub outcome: Option<&'static str>,
    pub reason: Option<&'static str>,
    pub mode: &'static str,
    pub updated_at: i64,
}

fn desfecho(d: Desfecho) -> &'static str {
    match d {
        Desfecho::Concluiu => "completed",
        Desfecho::Falhou => "failed",
        Desfecho::Parou => "stopped",
        Desfecho::Incompleto => "incomplete",
    }
}

fn aberto(p: &Pedido) -> bool {
    matches!(p.estado, Estado::Aceito | Estado::EmExecucao)
}

fn no_space(c: &Connection, bot: &str, space: &str) -> bool {
    Store::bot_on(c, bot).ok().flatten().is_some_and(|b| !b.deleted && b.config.space_id == space)
}

/// Texto do aviso de origem, só se ele é deste pedido, deste bot e para este colega.
fn texto(c: &Connection, p: &Pedido) -> Result<Option<String>> {
    let data: Option<String> = c
        .query_row(
            "SELECT e.data FROM donna_session_requests r JOIN entries e ON e.id=r.origin_entry_id
             WHERE r.id=?1 AND r.requester_bot_id=?2 AND r.target_bot_id=?3",
            params![p.id, p.de, p.para],
            |r| r.get(0),
        )
        .optional()?;
    let Some(data) = data.and_then(|d| serde_json::from_str::<Value>(&d).ok()) else { return Ok(None) };
    let aviso = &data["botMessage"];
    if data["delegationId"] != p.id.as_str()
        || aviso["sourceBotId"] != p.de.as_str()
        || aviso["targetBotId"] != p.para.as_str()
    {
        return Ok(None);
    }
    Ok(aviso["text"].as_str().map(|t| {
        let t = t.trim();
        if t.chars().count() > TEXTO_MAX {
            format!("{}…", t.chars().take(TEXTO_MAX).collect::<String>())
        } else {
            t.to_owned()
        }
    }))
}

pub(crate) fn ler_na(c: &Connection, bot: &str, space: &str) -> Result<TeamRequests> {
    spaces::require_space(c, space)?;
    ensure!(no_space(c, bot, space), "bot_not_found");
    let desde = now_ms() - TERMINADOS_MS;
    let manter = |p: &Pedido| p.de == bot && (aberto(p) || p.atualizado_em >= desde) && no_space(c, &p.para, space);
    let lidos =
        pendentes::ler_na(c, &Filtro { desde: now_ms() - pendentes::JANELA_MS, limite: LIMITE, manter: &manter })?;
    let mut items = Vec::with_capacity(lidos.linhas.len());
    for p in lidos.linhas {
        let (state, outcome, reason) = match p.estado {
            Estado::Aceito => ("queued", None, None),
            Estado::EmExecucao => ("running", None, None),
            Estado::Encerrado(d, _) => ("finished", Some(desfecho(d)), None),
            Estado::Decidido(m) => (m.estado(), None, Some(m.sql())),
        };
        items.push(TeamRequest {
            text: texto(c, &p)?,
            key: p.id,
            target_bot_id: p.para,
            state,
            outcome,
            reason,
            mode: match p.modo {
                Modo::Espera => "wait",
                Modo::Assincrono => "async",
            },
            updated_at: p.atualizado_em,
        });
    }
    Ok(TeamRequests { bot_id: bot.to_owned(), space_id: space.to_owned(), omitted: lidos.total - items.len(), items })
}

pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, body: Value) -> Result<Value> {
    crate::fork::bot_editor::authorization::require_owner(caller)?;
    let autoridade = Autoridade::capturar(hub, caller)?;
    let read: Read =
        serde_json::from_value(body).map_err(|_| anyhow!("invalid_field: consulta de pedidos do time inválida."))?;
    let hub = hub.clone();
    tokio::task::spawn_blocking(move || -> Result<Value> {
        let space = read.space_id.as_deref().unwrap_or(PERSONAL);
        let answer = autoridade.with_db(&hub, None, |db| ler_na(db, &read.bot_id, space))?;
        Ok(serde_json::to_value(answer)?)
    })
    .await?
}

// Os testes gravam pedidos direto no registro: ficam na pasta de retornos, a única que escreve nessas tabelas
// (provas_privacidade_testes::p08_tripwire_sql_e_saida_de_eventos).
#[cfg(test)]
#[path = "../retornos/pedidos_time_testes.rs"]
mod tests;
