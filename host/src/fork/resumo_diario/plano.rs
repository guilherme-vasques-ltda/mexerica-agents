//! Quem escreve e quando: só cálculo sobre o quadro já lido, sem I/O.
use super::store::Config;
use crate::fork::times::resumo::Quadro;
use chrono::{Duration, NaiveDate, TimeZone};

/// Cada nível escreve 15 min antes do de cima; o topo, 15 min antes do horário escolhido.
pub(super) const PASSO_MS: i64 = 15 * 60 * 1000;
pub(super) const MAX_NIVEL: u32 = 8;

pub(super) struct Vez {
    pub bot_id: String,
    /// 1 = reporta a Você (topo); cada nível abaixo soma 1.
    pub nivel: u32,
    /// Bot de cima no quadro, se existir no Pessoal.
    pub up: Option<String>,
}

/// Todos os bots do quadro, de baixo para cima (nível maior primeiro).
pub(super) fn vezes(q: &Quadro) -> Vec<Vez> {
    let up = |id: &str| {
        q.elos
            .iter()
            .find(|e| e.bot_id == id)
            .and_then(|e| e.up.clone())
            .filter(|u| q.elos.iter().any(|e| &e.bot_id == u))
    };
    let mut out: Vec<Vez> = q
        .elos
        .iter()
        .map(|e| {
            let mut nivel = 1;
            let mut atual = up(&e.bot_id);
            // O quadro já recusa ciclos; o limite só fecha a conta.
            while let Some(u) = atual {
                nivel += 1;
                if nivel > 64 {
                    break;
                }
                atual = up(&u);
            }
            Vez { bot_id: e.bot_id.clone(), nivel, up: up(&e.bot_id) }
        })
        .collect();
    out.sort_by(|a, b| b.nivel.cmp(&a.nivel).then_with(|| a.bot_id.cmp(&b.bot_id)));
    out
}

/// Área mostrada para o bot: a própria ou a do primeiro de cima que tem área.
pub(super) fn area_de<'a>(q: &'a Quadro, bot: &str) -> Option<&'a str> {
    let mut atual = q.elos.iter().find(|e| e.bot_id == bot);
    for _ in 0..64 {
        let e = atual?;
        if let Some(team) = e.team_id.as_deref() {
            return Some(team);
        }
        atual = e.up.as_deref().and_then(|u| q.elos.iter().find(|x| x.bot_id == u));
    }
    None
}

pub(super) fn horario(fechamento: i64, nivel: u32) -> i64 {
    fechamento - PASSO_MS * i64::from(nivel.min(MAX_NIVEL))
}

fn fechamento_em(cfg: &Config, dia: NaiveDate) -> i64 {
    let zona = cfg.zona();
    let local = dia.and_hms_opt(0, 0, 0).unwrap_or_default() + Duration::minutes(i64::from(cfg.minutos()));
    // Hora inexistente na troca de horário de verão: a hora seguinte.
    zona.from_local_datetime(&local)
        .earliest()
        .or_else(|| zona.from_local_datetime(&(local + Duration::hours(1))).earliest())
        .map_or_else(|| local.and_utc().timestamp_millis(), |t| t.timestamp_millis())
}

fn hoje(cfg: &Config, agora: i64) -> NaiveDate {
    chrono::DateTime::from_timestamp_millis(agora).unwrap_or_default().with_timezone(&cfg.zona()).date_naive()
}

/// Dia local de `agora` e o fechamento dele.
pub(super) fn dia_atual(cfg: &Config, agora: i64) -> (String, i64) {
    let dia = hoje(cfg, agora);
    (dia.to_string(), fechamento_em(cfg, dia))
}

/// Último dia já fechado (hoje, se já passou do horário; senão ontem) e o próximo fechamento.
pub(super) fn ultimo_fechado(cfg: &Config, agora: i64) -> (String, i64, i64) {
    let dia = hoje(cfg, agora);
    let fecha = fechamento_em(cfg, dia);
    if agora >= fecha {
        let amanha = dia.succ_opt().unwrap_or(dia);
        (dia.to_string(), fecha, fechamento_em(cfg, amanha))
    } else {
        let ontem = dia.pred_opt().unwrap_or(dia);
        (ontem.to_string(), fechamento_em(cfg, ontem), fecha)
    }
}
