//! Regra (o).2: cada troca de conta vira um cartão no Mission Board (origem, destino, motivo, hora).
//! Só leitura do diário de trocas do Space aberto, dos últimos 7 dias, com o rótulo seguro de cada conta.
use super::estado::{Estado, Motivo};
use crate::fork::control_tower::Observation;
use crate::fork::engine_accounts::Registry;
use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;

const JANELA_MS: i64 = 7 * 24 * 3_600_000;
const MAX_ITENS: usize = 20;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Trocas {
    pub items: Vec<Cartao>,
    pub omitted: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Cartao {
    pub id: String,
    pub at: i64,
    pub bot_id: String,
    pub from_label: String,
    pub to_label: String,
    pub reason: Motivo,
    pub back_at: Option<i64>,
}

pub(crate) fn projection(c: &Connection, space: &str, now: i64) -> Result<Observation<Trocas>> {
    let estado = Estado::carregar(c)?;
    let registry = Registry::load_db(c)?;
    let rotulo = |id: &str| registry.account(id).map_or_else(|_| "—".to_owned(), |a| a.dto().label);
    let mut recentes: Vec<_> = estado.trocas.iter().filter(|t| t.space == space && now - t.em <= JANELA_MS).collect();
    recentes.sort_by_key(|t| std::cmp::Reverse(t.em));
    let omitted = recentes.len().saturating_sub(MAX_ITENS);
    let items = recentes
        .into_iter()
        .take(MAX_ITENS)
        .map(|t| Cartao {
            id: t.id.clone(),
            at: t.em,
            bot_id: t.bot.clone(),
            from_label: rotulo(&t.de),
            to_label: rotulo(&t.para),
            reason: t.motivo,
            back_at: t.volta_em.filter(|at| *at > now),
        })
        .collect();
    Ok(Observation::Available {
        source: "fallback.trocas",
        coverage: "accountSwitchesInSpaceLast7Days",
        value: Trocas { items, omitted },
    })
}
