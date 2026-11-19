//! Coluna "O que aconteceu" do Mission Board: o último dia fechado, por Time. Fora do Pessoal
//! devolve `Unavailable` sem ler nada. Só textos curtos já guardados e ids do roster autorizado.
use super::{entrada, plano, store};
use crate::fork::{control_tower::Observation, spaces::PERSONAL};
use crate::{LockExt, hub::Hub, store::now_ms};
use anyhow::Result;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WhatHappened {
    pub enabled: bool,
    pub time: String,
    pub time_zone: String,
    pub day: String,
    pub closes_at: i64,
    pub next_at: i64,
    pub teams: Vec<Team>,
    /// D329: bots fora de Time e sem ninguém reportando a eles.
    pub loose: Vec<BotLine>,
    pub out: Vec<Out>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Team {
    #[serde(rename = "teamId")]
    pub id: String,
    pub name: String,
    pub line: Option<Line>,
    pub bots: Vec<BotLine>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BotLine {
    pub bot_id: String,
    pub line: Line,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Line {
    pub text: String,
    /// Junto pelo computador, sem IA.
    pub joined: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Out {
    pub bot_id: String,
    pub reason: &'static str,
}

/// `roster`: bots já autorizados pela projeção do Mission Board. Chamado sem o DB travado.
pub(crate) fn torre(hub: &Hub, space: &str, roster: &[String]) -> Result<Observation<WhatHappened>> {
    torre_em(hub, space, roster, now_ms())
}

pub(super) fn torre_em(hub: &Hub, space: &str, roster: &[String], agora: i64) -> Result<Observation<WhatHappened>> {
    if space != PERSONAL {
        return Ok(Observation::Unavailable);
    }
    let cfg = store::config(&hub.store)?;
    let (day, closes_at, next_at) = plano::ultimo_fechado(&cfg, agora);
    let (quadro, linhas) = {
        let c = hub.store.db.locked();
        let tx = c.unchecked_transaction()?;
        (crate::fork::times::resumo::quadro(&tx, PERSONAL)?, store::dia_na(&tx, &day)?)
    };
    let nivel: std::collections::HashMap<_, _> =
        plano::vezes(&quadro).into_iter().map(|v| (v.bot_id, v.nivel)).collect();
    let mut elos: Vec<_> = quadro.elos.iter().filter(|e| roster.contains(&e.bot_id)).collect();
    elos.sort_by(|a, b| (nivel.get(&a.bot_id), &a.name).cmp(&(nivel.get(&b.bot_id), &b.name)));
    let mut out = Vec::new();
    for e in &elos {
        let cfg = hub.store.bot(&e.bot_id)?.map(|r| r.config);
        if let Some(motivo) = cfg.as_ref().and_then(|cfg| super::fora(&hub.store, cfg)) {
            out.push(Out { bot_id: e.bot_id.clone(), reason: motivo.sql() });
        }
    }
    let linha = |bot: &str| linhas.iter().find(|l| l.bot_id == bot && l.estado == store::Estado::Feito);
    let bot_line = |bot: &str| {
        linha(bot)
            .and_then(|l| l.bot_line.clone())
            .map(|text| BotLine { bot_id: bot.to_owned(), line: Line { text, joined: false } })
    };
    let mut teams = Vec::new();
    for area in &quadro.areas {
        let bots: Vec<BotLine> = elos
            .iter()
            .filter(|e| plano::area_de(&quadro, &e.bot_id) == Some(area.id.as_str()))
            .filter_map(|e| bot_line(&e.bot_id))
            .collect();
        let gerente = area
            .manager
            .as_deref()
            .filter(|m| roster.iter().any(|r| r == m))
            .and_then(linha)
            .and_then(|l| l.team_line.clone().map(|text| Line { text, joined: l.reason.is_some() }));
        // Gerente sem linha do Time (fora, cota, falha ou sem gerente): o Hub junta, sem IA.
        let line = gerente.or_else(|| {
            let partes: Vec<(String, String)> = bots
                .iter()
                .filter_map(|b| {
                    quadro.elos.iter().find(|e| e.bot_id == b.bot_id).map(|e| (e.name.clone(), b.line.text.clone()))
                })
                .collect();
            (partes.len() > 1).then(|| entrada::juntar(&partes)).flatten().map(|text| Line { text, joined: true })
        });
        if line.is_some() || !bots.is_empty() {
            teams.push(Team { id: area.id.clone(), name: area.name.clone(), line, bots });
        }
    }
    let loose = elos
        .iter()
        .filter(|e| plano::area_de(&quadro, &e.bot_id).is_none())
        .filter_map(|e| bot_line(&e.bot_id))
        .collect();
    Ok(Observation::Available {
        source: "resumo_diario",
        coverage: "personalLastClosedDay",
        value: WhatHappened {
            enabled: cfg.enabled,
            time: cfg.time,
            time_zone: cfg.time_zone,
            day,
            closes_at,
            next_at,
            teams,
            loose,
            out,
        },
    })
}
