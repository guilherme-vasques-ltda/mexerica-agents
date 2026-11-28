//! Donnabot (D85): a visão de todas as rotinas para a tela central "Rotinas" do app.
//!
//! `routinesOverview {from, to, timeZone}` devolve, só para leitura, as rotinas de todos os bots
//! agentes (com a descrição em pt-BR e o Time), as ocorrências previstas em `[max(from, agora), to)`
//! e as execuções criadas no intervalo, mais as ativas. As ocorrências partem do `next_runs` que o
//! agendador usa, então o calendário mostra o que vai rodar de fato; o passado são as execuções.
//! `routineTeams` devolve só os Times (a barra lateral filtra por eles), e o `routineSchedule` do
//! editor ganha `resumo`, a mesma agenda em pt-BR.
use super::{Routine, Routines, Run, ScheduleDraft, Trigger, public};
use crate::{
    LockExt,
    store::{Store, now_ms},
};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use croner::Cron;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::str::FromStr;

const DAY_MS: i64 = 86_400_000;
const HOUR_MS: i64 = 3_600_000;
/// Mês com semanas completas (42 dias) e folga para o fuso.
const MAX_RANGE_MS: i64 = 50 * DAY_MS;
/// Mais que isto numa mesma hora (janela móvel) e a rotina vai para a faixa "Frequentes": os
/// cartões de 30 min se sobreporiam. Vale para a próxima semana e para o período visto.
const PER_HOUR: usize = 2;
/// Teto de cartões por resposta: acima dele, as rotinas com mais ocorrências saem da grade
/// (`truncated`), uma a uma, até caber. Não viram "Frequentes" e as execuções continuam.
const MAX_OCCURRENCES: usize = 5000;
/// D27/D28: até existir Time no Hub (Fase 2), todo bot é do Time "Pessoal".
const PERSONAL_TEAM: (&str, &str) = ("pessoal", "Pessoal");
const WEEKDAYS: [&str; 7] = ["domingo", "segunda", "terça", "quarta", "quinta", "sexta", "sábado"];
const WEEKDAYS_SHORT: [&str; 7] = ["dom", "seg", "ter", "qua", "qui", "sex", "sáb"];

impl Routines {
    pub fn overview(&self, store: &Store, body: &Value) -> Result<Value> {
        let from = body["from"].as_i64().context("Falta o início do período (from).")?;
        let to = body["to"].as_i64().context("Falta o fim do período (to).")?;
        if to <= from || to - from > MAX_RANGE_MS {
            bail!("O período do calendário deve terminar depois do início e ter até 50 dias.");
        }
        let zone = body["timeZone"].as_str().and_then(|z| z.parse::<Tz>().ok()).unwrap_or(Tz::UTC);
        let space = body["spaceId"].as_str().unwrap_or(crate::fork::spaces::PERSONAL);
        store.require_space(space)?;
        let bots = agent_bots_in_space(store, space)?;
        let known: HashSet<&str> = bots.iter().map(String::as_str).collect();
        // Copia com o lock e calcula fora dele: o agendador usa o mesmo estado a cada segundo.
        let (routines, runs) = {
            let state = self.state.locked();
            let routines: Vec<Routine> =
                state.routines.iter().filter(|r| !r.deleted && known.contains(r.bot_id.as_str())).cloned().collect();
            let ids: HashSet<&str> = routines.iter().map(|r| r.id.as_str()).collect();
            let runs: Vec<Value> = state
                .runs
                .iter()
                .filter(|run| ids.contains(run.routine_id.as_str()))
                .filter(|run| run.status.active() || (from..to).contains(&run.created_at))
                .map(run_summary)
                .collect();
            (routines, runs)
        };
        let now = now_ms();
        let projection = occurrences(&routines, from, to, now);
        Ok(json!({
            "now": now,
            "teams": if space==crate::fork::spaces::PERSONAL {team_list(&bots)}else{json!([])},
            "routines": routines.iter().map(|r| {
                let mut value = public(r);
                value["summary"] = summary(&r.triggers, zone).into();
                value["teamId"] = if space==crate::fork::spaces::PERSONAL {PERSONAL_TEAM.0.into()}else{Value::Null};
                // D84: "onde roda" chega com os computadores da D83; até lá, sempre o Hub.
                value["runsOn"] = Value::Null;
                value
            }).collect::<Vec<_>>(),
            "occurrences": projection.occurrences,
            "dense": projection.dense,
            "truncated": projection.truncated,
            "invalid": projection.invalid,
            "runs": runs,
        }))
    }

    /// `routineTeams`: os Times e seus bots, sem rotinas (o filtro da barra lateral).
    pub fn teams_in_space(store: &Store, space: &str) -> Result<Value> {
        store.require_space(space)?;
        Ok(
            json!({"teams": if space==crate::fork::spaces::PERSONAL {team_list(&agent_bots_in_space(store,space)?)}else{json!([])}}),
        )
    }

    /// `routineSchedule` (o editor) com `resumo`: a mesma agenda em pt-BR, no fuso do editor.
    pub fn schedule_preview_pt(body: &Value, now: i64) -> Result<Value> {
        let mut preview = super::schedule_preview(body, now)?;
        let zone = body["timeZone"]
            .as_str()
            .or_else(|| body["draft"]["zone"].as_str())
            .and_then(|z| z.parse::<Tz>().ok())
            .unwrap_or(Tz::UTC);
        if let Ok(triggers) = serde_json::from_value::<Vec<Trigger>>(preview["triggers"].clone()) {
            preview["resumo"] = summary(&triggers, zone).into();
        }
        Ok(preview)
    }
}

fn agent_bots_in_space(store: &Store, space: &str) -> Result<Vec<String>> {
    Ok(store
        .bots()?
        .into_iter()
        .filter(|b| !b.deleted && !b.config.is_group() && b.config.space_id == space)
        .map(|b| b.config.id)
        .collect())
}

fn team_list(bots: &[String]) -> Value {
    json!([{"id": PERSONAL_TEAM.0, "name": PERSONAL_TEAM.1, "botIds": bots}])
}

fn run_summary(run: &Run) -> Value {
    json!({"id": run.id, "routineId": run.routine_id, "botId": run.bot_id, "status": run.status,
        "createdAt": run.created_at, "startedAt": run.started_at, "finishedAt": run.finished_at, "detail": run.detail,
        "outcome": crate::fork::rotina_resultado::public(run.outcome.as_ref())})
}

#[derive(Default)]
struct Projection {
    occurrences: Vec<Value>,
    /// Mais de `PER_HOUR` vezes numa hora: faixa "Frequentes", sem cartões nem execuções na grade.
    dense: Vec<String>,
    /// Fora da grade só neste período, pelo teto de cartões; as execuções continuam.
    truncated: Vec<String>,
    /// A agenda não pôde ser projetada (cron ou fuso que deixou de valer): sem cartões.
    invalid: Vec<String>,
}

/// O teto de ocorrências de uma janela que não passa de `PER_HOUR` em nenhuma hora: cada hora
/// cheia tem no máximo `PER_HOUR`, então passar disto já é frequente.
fn hourly_cap(span: i64) -> usize {
    let hours = usize::try_from((span.max(0) + HOUR_MS - 1) / HOUR_MS).unwrap_or(usize::MAX);
    hours.max(1).saturating_mul(PER_HOUR)
}

/// Mais de `PER_HOUR` ocorrências dentro de uma mesma hora, em qualquer ponto.
fn crowded(found: &[(usize, i64)]) -> bool {
    let mut times: Vec<i64> = found.iter().map(|(_, at)| *at).collect();
    times.sort_unstable();
    let mut first = 0;
    for (last, at) in times.iter().enumerate() {
        while at - times[first] >= HOUR_MS {
            first += 1;
        }
        if last + 1 - first > PER_HOUR {
            return true;
        }
    }
    false
}

/// Ocorrências futuras de cada rotina, com as frequentes, as cortadas pelo teto e as inválidas à parte.
fn occurrences(routines: &[Routine], from: i64, to: i64, now: i64) -> Projection {
    let low = from.max(now);
    let mut result = Projection::default();
    let mut kept: Vec<(&Routine, Vec<(usize, i64)>)> = Vec::new();
    for routine in routines {
        // A próxima semana decide para todas as visões; o período visto também conta.
        let week_end = now + 7 * DAY_MS;
        let projected = project(routine, now, week_end, now, hourly_cap(week_end - now)).and_then(|week| {
            if week.is_none_or(|week| crowded(&week)) {
                return Ok(None);
            }
            Ok(project(routine, low, to, now, hourly_cap(to - low))?.filter(|found| !crowded(found)))
        });
        match projected {
            Ok(Some(found)) => kept.push((routine, found)),
            Ok(None) => result.dense.push(routine.id.clone()),
            Err(error) => {
                tracing::warn!(routine = %routine.id, error = format!("{error:#}"), "routine calendar projection skipped");
                result.invalid.push(routine.id.clone());
            }
        }
    }
    // Acima do teto saem primeiro as que têm mais cartões (empate pelo id), até caber.
    let mut total: usize = kept.iter().map(|(_, found)| found.len()).sum();
    if total > MAX_OCCURRENCES {
        let mut order: Vec<usize> = (0..kept.len()).collect();
        order.sort_by(|a, b| kept[*b].1.len().cmp(&kept[*a].1.len()).then_with(|| kept[*a].0.id.cmp(&kept[*b].0.id)));
        let mut cut = HashSet::new();
        for index in order {
            if total <= MAX_OCCURRENCES {
                break;
            }
            total -= kept[index].1.len();
            cut.insert(index);
        }
        let mut truncated: Vec<String> = cut.iter().map(|index| kept[*index].0.id.clone()).collect();
        truncated.sort();
        result.truncated = truncated;
        kept = kept.into_iter().enumerate().filter(|(index, _)| !cut.contains(index)).map(|(_, kept)| kept).collect();
    }
    for (routine, found) in kept {
        result.occurrences.extend(found.into_iter().map(|(index, at)| {
            json!({"routineId": routine.id, "botId": routine.bot_id, "at": at, "trigger": index, "enabled": routine.enabled})
        }));
    }
    result.occurrences.sort_by_key(|o| o["at"].as_i64());
    result
}

/// Um gatilho pronto para projetar: o cron e o fuso são lidos uma vez, não a cada ocorrência.
/// Segue `Trigger::next` (o agendador), que o teste de São Paulo confere.
enum Step {
    Interval(i64),
    Once(i64),
    Cron(Box<Cron>, Tz),
    None,
}

impl Step {
    fn new(trigger: &Trigger) -> Result<Self> {
        Ok(match trigger {
            Trigger::Interval { seconds } => {
                Self::Interval(seconds.checked_mul(1000).filter(|s| *s > 0).context("invalid interval")?)
            }
            Trigger::Once { at } => Self::Once(*at),
            Trigger::Cron { expression, time_zone } => Self::Cron(
                Box::new(Cron::from_str(expression).context("invalid cron expression")?),
                time_zone.parse::<Tz>().map_err(|e| anyhow::anyhow!("invalid time zone: {e}"))?,
            ),
            Trigger::Webhook | Trigger::Event { .. } => Self::None,
        })
    }

    fn next(&self, after: i64) -> Result<Option<i64>> {
        Ok(match self {
            Self::Interval(step) => Some(after.checked_add(*step).context("schedule overflow")?),
            Self::Once(at) => (*at > after).then_some(*at),
            Self::Cron(cron, zone) => {
                let date = DateTime::<Utc>::from_timestamp_millis(after)
                    .context("invalid schedule timestamp")?
                    .with_timezone(zone);
                Some(cron.find_next_occurrence(&date, false)?.timestamp_millis())
            }
            Self::None => None,
        })
    }
}

/// `None` quando a rotina passa de `cap` ocorrências no intervalo. Ligada, parte do `next_runs`
/// do agendador. Pausada, mostra quando rodaria se fosse religada: o cron a partir de agora; o
/// intervalo na cadência salva (ou a da última edição), para os cartões não andarem a cada consulta.
fn project(routine: &Routine, low: i64, to: i64, now: i64, cap: usize) -> Result<Option<Vec<(usize, i64)>>> {
    let mut found = Vec::new();
    for (index, trigger) in routine.triggers.iter().enumerate() {
        let step = Step::new(trigger)?;
        let saved = routine.next_runs.get(index).copied().flatten();
        let start = match (&step, routine.enabled) {
            (_, true) => saved,
            (Step::Interval(step), false) => Some(saved.unwrap_or(routine.updated_at.saturating_add(*step))),
            (_, false) => step.next(now)?,
        };
        let Some(mut at) = start else { continue };
        if at < low {
            at = match &step {
                Step::Interval(step) => at + (low - at + step - 1) / step * step,
                // O croner mantém os milissegundos do ponto de partida: parte do segundo cheio.
                Step::Cron(..) => match step.next((low - 1).div_euclid(1000) * 1000)? {
                    Some(next) => next,
                    None => continue,
                },
                _ => continue,
            };
        }
        while at < to {
            if found.len() >= cap {
                return Ok(None);
            }
            found.push((index, at));
            match step.next(at)? {
                Some(next) if next > at => at = next,
                _ => break,
            }
        }
    }
    Ok(Some(found))
}

/// A rotina sem horário nem gatilho: só roda quando o dono manda (botão ou pedido ao bot).
pub(super) const MANUAL_TEXT: &str = "Só quando eu mandar";

/// A agenda em português, no fuso de quem vê; um fuso diferente aparece no fim.
pub(super) fn summary(triggers: &[Trigger], zone: Tz) -> String {
    if triggers.is_empty() {
        return MANUAL_TEXT.into();
    }
    triggers.iter().map(|t| describe(t, zone)).collect::<Vec<_>>().join(" · ")
}

/// Um grupo de horários com os mesmos dias e fuso, e seus relógios.
type ClockGroup = (DayPart, String, Vec<(u8, u8)>);

enum AgendaSlot {
    Group(usize),
    Phrase(String),
}

/// A agenda em pt-BR no fuso da própria rotina, para `list_routines`: o bot repete ao
/// usuário o mesmo texto da tela. Gatilhos de relógio com os mesmos dias e fuso viram
/// uma frase só (`Todo dia às 9h e às 15h`); o resto segue a descrição de cada um.
pub(super) fn agenda(triggers: &[Trigger]) -> String {
    if triggers.is_empty() {
        return MANUAL_TEXT.into();
    }
    let zone = triggers
        .iter()
        .find_map(|trigger| match trigger {
            Trigger::Cron { time_zone, .. } => time_zone.parse::<Tz>().ok(),
            _ => None,
        })
        .unwrap_or(Tz::UTC);
    let mut groups: Vec<ClockGroup> = Vec::new();
    let mut slots: Vec<AgendaSlot> = Vec::new();
    for trigger in triggers {
        let grouped = match trigger {
            Trigger::Cron { expression, time_zone } => {
                super::schedule::split_cron_lists(expression).and_then(|times| {
                    day_part(&times.day, &times.month, &times.weekday).map(|part| {
                        let mut clocks = Vec::with_capacity(times.hours.len() * times.minutes.len());
                        for hour in &times.hours {
                            for minute in &times.minutes {
                                clocks.push((*hour, *minute));
                            }
                        }
                        (part, time_zone.clone(), clocks)
                    })
                })
            }
            _ => None,
        };
        match grouped {
            Some((part, zone_name, clocks)) => {
                let index = groups
                    .iter()
                    .position(|(known, known_zone, _)| *known == part && known_zone == &zone_name)
                    .unwrap_or_else(|| {
                        groups.push((part, zone_name, Vec::new()));
                        groups.len() - 1
                    });
                groups[index].2.extend(clocks);
                if !slots.iter().any(|slot| matches!(slot, AgendaSlot::Group(known) if *known == index)) {
                    slots.push(AgendaSlot::Group(index));
                }
            }
            None => slots.push(AgendaSlot::Phrase(describe(trigger, zone))),
        }
    }
    slots
        .into_iter()
        .map(|slot| match slot {
            AgendaSlot::Phrase(text) => text,
            AgendaSlot::Group(index) => {
                let (part, zone_name, clocks) = &groups[index];
                let mut clocks = clocks.clone();
                clocks.sort_unstable();
                clocks.dedup();
                let text = day_phrase(part, &join_times(&clocks));
                if zone_name.parse::<Tz>().ok() == Some(zone) { text } else { format!("{text} ({zone_name})") }
            }
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

fn describe(trigger: &Trigger, zone: Tz) -> String {
    match trigger {
        Trigger::Interval { seconds } => interval(*seconds),
        Trigger::Once { at } => DateTime::from_timestamp_millis(*at).map_or_else(
            || "Uma vez".into(),
            |d| format!("Uma vez, {}", d.with_timezone(&zone).format("%d/%m/%Y às %H:%M")),
        ),
        Trigger::Cron { expression, time_zone } => {
            let text = cron(expression);
            if time_zone.parse::<Tz>().ok() == Some(zone) { text } else { format!("{text} ({time_zone})") }
        }
        Trigger::Webhook => "Quando o webhook for chamado".into(),
        Trigger::Event { source, event, .. } => {
            let event = if event == "*" { "qualquer evento" } else { event.as_str() };
            format!("Evento do {} · {event}", source_name(source))
        }
    }
}

fn interval(seconds: i64) -> String {
    let (amount, one, many) = match seconds {
        s if s % 86_400 == 0 => (s / 86_400, "dia", "dias"),
        s if s % 3600 == 0 => (s / 3600, "hora", "horas"),
        s if s % 60 == 0 => (s / 60, "minuto", "min"),
        s => (s, "segundo", "s"),
    };
    if amount == 1 { format!("A cada {one}") } else { format!("A cada {amount} {many}") }
}

/// A parte dos dias de um cron de relógio, com as mesmas regras do editor
/// (`ScheduleDraft::read_cron`): dia e mês livres com semana livre, útil, um dia ou dias
/// em ordem; ou dia do mês com semana livre.
#[derive(Clone, PartialEq, Eq)]
enum DayPart {
    Daily,
    Weekdays,
    Weekly(u8),
    Days(Vec<u8>),
    Monthly(u8),
}

fn day_part(day: &str, month: &str, weekday: &str) -> Option<DayPart> {
    if day == "*" {
        if month != "*" {
            return None;
        }
        if weekday == "*" {
            return Some(DayPart::Daily);
        }
        if weekday == "1-5" {
            return Some(DayPart::Weekdays);
        }
        if let Ok(single) = weekday.parse::<u8>() {
            return (single <= 6).then_some(DayPart::Weekly(single));
        }
        let days: Vec<u8> = weekday.split(',').map(str::parse::<u8>).collect::<Result<_, _>>().ok()?;
        (!days.is_empty() && days.iter().all(|day| *day <= 6) && days.windows(2).all(|pair| pair[0] < pair[1]))
            .then_some(DayPart::Days(days))
    } else if weekday == "*" && month == "*" {
        day.parse::<u8>().ok().filter(|day| (1..=31).contains(day)).map(DayPart::Monthly)
    } else {
        None
    }
}

/// Os horários de um cron de relógio em ordem, produto de horas por minutos;
/// `None` fora de números e listas (`split_cron_lists`).
fn clock_times(expression: &str) -> Option<Vec<(u8, u8)>> {
    let times = super::schedule::split_cron_lists(expression)?;
    Some(
        times
            .hours
            .iter()
            .flat_map(|hour| {
                let hour = *hour;
                times.minutes.iter().map(move |minute| (hour, *minute))
            })
            .collect(),
    )
}

/// Teto de horários por frase: o resto vira `e mais N horários`, para a frase caber
/// no contexto do bot (um cron de lista pode ter ~1440 horários).
const MAX_TIMES_SHOWN: usize = 6;

/// `às 9h e às 15h`; `à` antes de 0h/1h, como o bot fala.
fn join_times(clocks: &[(u8, u8)]) -> String {
    let shown = clocks.len().min(MAX_TIMES_SHOWN);
    let mut items: Vec<String> = clocks[..shown]
        .iter()
        .map(|(hour, minute)| {
            let article = if *hour <= 1 { "à" } else { "às" };
            let clock = if *minute == 0 { format!("{hour}h") } else { format!("{hour}h{minute:02}") };
            format!("{article} {clock}")
        })
        .collect();
    let rest = clocks.len() - shown;
    if rest > 0 {
        items.push(if rest == 1 { "mais 1 horário".into() } else { format!("mais {rest} horários") });
    }
    match items.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} e {last}", rest.join(", ")),
        _ => items.join(""),
    }
}

fn day_phrase(part: &DayPart, times: &str) -> String {
    let weekday = |day: u8| WEEKDAYS.get(usize::from(day)).copied().unwrap_or("?");
    match part {
        DayPart::Daily => format!("Todo dia {times}"),
        DayPart::Weekdays => format!("Dias úteis {times}"),
        DayPart::Weekly(day) if matches!(day, 0 | 6) => format!("Todo {} {times}", weekday(*day)),
        DayPart::Weekly(day) => format!("Toda {} {times}", weekday(*day)),
        DayPart::Days(days) => {
            let names: Vec<_> =
                days.iter().map(|day| WEEKDAYS_SHORT.get(usize::from(*day)).copied().unwrap_or("?")).collect();
            let list = match names.split_last() {
                Some((last, rest)) if !rest.is_empty() => format!("{} e {last}", rest.join(", ")),
                _ => names.join(""),
            };
            let mut text = format!("{list} {times}");
            text[..1].make_ascii_uppercase();
            text
        }
        DayPart::Monthly(day) => format!("Todo dia {day} do mês {times}"),
    }
}

/// Vários horários numa expressão só (`0 9,15 * * *`, `0,30 9 * * *`): uma frase.
/// Passos, faixas e o resto fora dos estilos voltam `None` e seguem como `Cron ...`.
fn cron_lists(expression: &str) -> Option<String> {
    let clocks = clock_times(expression)?;
    if clocks.len() < 2 {
        return None;
    }
    let times = super::schedule::split_cron_lists(expression)?;
    let part = day_part(&times.day, &times.month, &times.weekday)?;
    Some(day_phrase(&part, &join_times(&clocks)))
}

/// Os estilos do editor (`ScheduleDraft::read_cron`); os demais aparecem como cron.
fn cron(expression: &str) -> String {
    let mut draft =
        ScheduleDraft { expression: expression.into(), calendar_style: "custom".into(), ..Default::default() };
    draft.read_cron();
    let time = format!("{:02}:{:02}", draft.hour, draft.minute);
    let weekday = |day: u8| WEEKDAYS.get(usize::from(day)).copied().unwrap_or("?");
    match draft.calendar_style.as_str() {
        "minutes" => format!("A cada {} min", draft.minute_step),
        "hourly" if draft.minute == 0 => "A cada hora, em ponto".into(),
        "hourly" => format!("A cada hora, aos {} min", draft.minute),
        "daily" => format!("Todo dia às {time}"),
        "weekdays" => format!("Dias úteis às {time}"),
        "weekly" if matches!(draft.weekday, 0 | 6) => format!("Todo {} às {time}", weekday(draft.weekday)),
        "weekly" => format!("Toda {} às {time}", weekday(draft.weekday)),
        "days" => {
            let days: Vec<_> = draft
                .selected_days
                .iter()
                .map(|d| WEEKDAYS_SHORT.get(usize::from(*d)).copied().unwrap_or("?"))
                .collect();
            let list = match days.split_last() {
                Some((last, rest)) if !rest.is_empty() => format!("{} e {last}", rest.join(", ")),
                _ => days.join(""),
            };
            let mut text = format!("{list} às {time}");
            text[..1].make_ascii_uppercase();
            text
        }
        "monthly" => format!("Todo dia {} do mês às {time}", draft.month_day),
        _ => cron_lists(expression).unwrap_or_else(|| format!("Cron {expression}")),
    }
}

fn source_name(source: &str) -> &str {
    match source {
        "slack" => "Slack",
        "github" => "GitHub",
        "microsoftTeams" => "Microsoft Teams",
        "linear" => "Linear",
        "sentry" => "Sentry",
        "pagerduty" => "PagerDuty",
        "email" => "e-mail",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::routine;
    use super::*;

    fn ms(rfc3339: &str) -> i64 {
        DateTime::parse_from_rfc3339(rfc3339).unwrap().timestamp_millis()
    }

    #[test]
    fn cron_in_sao_paulo_starts_at_the_scheduler_next_run_and_respects_the_range() {
        let now = ms("2026-10-20T12:00:00Z");
        let mut r = routine();
        r.triggers = vec![Trigger::Cron { expression: "0 9 * * 1-5".into(), time_zone: "America/Sao_Paulo".into() }];
        r.next_runs = vec![r.triggers[0].next(now).unwrap()];
        // De sexta 23/10 (BRT) até a terça seguinte, exclusivo: sex, seg; nada no fim de semana.
        let found =
            project(&r, ms("2026-10-23T00:00:00-03:00"), ms("2026-10-27T09:00:00-03:00"), now, 100).unwrap().unwrap();
        let at: Vec<_> = found.iter().map(|(_, at)| *at).collect();
        assert_eq!(at, [ms("2026-10-23T09:00:00-03:00"), ms("2026-10-26T09:00:00-03:00")]);
        assert_eq!(summary(&r.triggers, "America/Sao_Paulo".parse().unwrap()), "Dias úteis às 09:00");
        assert_eq!(
            summary(&r.triggers, Tz::UTC),
            "Dias úteis às 09:00 (America/Sao_Paulo)",
            "outro fuso aparece no fim"
        );
    }

    #[test]
    fn leitura_e_edicao_sem_mudanca_de_agenda_mantem_ocorrencias() {
        let now = ms("2026-10-20T12:00:00Z");
        let mut r = routine();
        // Rotina com dois fusos: cada gatilho projeta no próprio fuso.
        r.triggers = vec![
            Trigger::Cron { expression: "0 9 * * *".into(), time_zone: "America/Sao_Paulo".into() },
            Trigger::Cron { expression: "30 15 * * *".into(), time_zone: "America/New_York".into() },
        ];
        r.next_runs = vec![r.triggers[0].next(now).unwrap(), r.triggers[1].next(now).unwrap()];
        let (low, to) = (ms("2026-10-21T00:00:00Z"), ms("2026-10-23T00:00:00Z"));
        let before = project(&r, low, to, now, 100).unwrap().unwrap();
        assert_eq!(before.len(), 4, "dois dias x dois fusos");
        let mut edited = r.clone();
        edited.name = "Outro nome".into();
        edited.instruction = "Outra instrução".into();
        edited.updated_at += 1;
        let after = project(&edited, low, to, now, 100).unwrap().unwrap();
        assert_eq!(before, after, "ler ou renomear não muda as ocorrências previstas");
    }

    #[test]
    fn rotina_com_varios_horarios_ganha_um_cartao_em_cada_horario_da_semana() {
        let now = ms("2026-10-18T12:00:00Z");
        let (from, to) = (ms("2026-10-19T00:00:00-03:00"), ms("2026-10-26T00:00:00-03:00"));
        let mut lista = routine();
        lista.id = "a-lista".into();
        lista.triggers =
            vec![Trigger::Cron { expression: "0 8,12,18 * * *".into(), time_zone: "America/Sao_Paulo".into() }];
        lista.next_runs = vec![lista.triggers[0].next(now).unwrap()];
        let mut gatilhos = routine();
        gatilhos.id = "b-gatilhos".into();
        gatilhos.triggers = ["0 9 * * 1-5", "30 13 * * 1-5", "15 17 * * 1-5"]
            .map(|expression| Trigger::Cron { expression: expression.into(), time_zone: "America/Sao_Paulo".into() })
            .to_vec();
        gatilhos.next_runs = gatilhos.triggers.iter().map(|t| t.next(now).unwrap()).collect();
        let projection = occurrences(&[lista, gatilhos], from, to, now);
        assert!(projection.dense.is_empty(), "poucos horários por hora não viram faixa");
        let horas = |id: &str| -> Vec<String> {
            projection
                .occurrences
                .iter()
                .filter(|o| o["routineId"] == id)
                .map(|o| {
                    let at = DateTime::<Utc>::from_timestamp_millis(o["at"].as_i64().unwrap()).unwrap();
                    at.with_timezone(&Tz::America__Sao_Paulo).format("%a %H:%M").to_string()
                })
                .collect()
        };
        let lista = horas("a-lista");
        assert_eq!(lista.len(), 21, "7 dias x 3 horários");
        assert_eq!(lista[..3], ["Mon 08:00", "Mon 12:00", "Mon 18:00"]);
        let gatilhos = horas("b-gatilhos");
        assert_eq!(gatilhos.len(), 15, "5 dias úteis x 3 gatilhos");
        assert_eq!(gatilhos[..3], ["Mon 09:00", "Mon 13:30", "Mon 17:15"]);
        assert!(!gatilhos.iter().any(|h| h.starts_with("Sat") || h.starts_with("Sun")));
        assert!(projection.occurrences.iter().all(|o| o["botId"].is_string()), "cada cartão diz quem roda");
    }

    #[test]
    fn list_expressions_describe_every_time_in_portuguese() {
        let zone: Tz = "America/Sao_Paulo".parse().unwrap();
        for (expression, expected) in [
            ("0 9,15 * * *", "Todo dia às 9h e às 15h"),
            ("0,30 9 * * *", "Todo dia às 9h e às 9h30"),
            ("30 9,12,15 * * 1-5", "Dias úteis às 9h30, às 12h30 e às 15h30"),
            ("0 0,1 * * *", "Todo dia à 0h e à 1h"),
            ("0 9,15 * * 0", "Todo domingo às 9h e às 15h"),
            ("0 9,15 * * 3", "Toda quarta às 9h e às 15h"),
            ("0 9,15 * * 1,3,5", "Seg, qua e sex às 9h e às 15h"),
            ("0 9,15 12 * *", "Todo dia 12 do mês às 9h e às 15h"),
            ("0,15,30,45 0,6,12,18 * * *", "Todo dia à 0h, à 0h15, à 0h30, à 0h45, às 6h, às 6h15 e mais 10 horários"),
        ] {
            let triggers = vec![Trigger::Cron { expression: expression.into(), time_zone: "America/Sao_Paulo".into() }];
            assert_eq!(summary(&triggers, zone), expected, "{expression}");
        }
    }

    #[test]
    fn list_expressions_keep_the_timezone_suffix_and_steps_stay_cron() {
        let triggers = vec![Trigger::Cron { expression: "0 9,15 * * *".into(), time_zone: "America/Sao_Paulo".into() }];
        assert_eq!(summary(&triggers, Tz::UTC), "Todo dia às 9h e às 15h (America/Sao_Paulo)");
        for expression in ["0 9-17 * * *", "*/20 9 * * *", "0 9 * * 5,3,1", "0 9 0 * *", "0 9,15 * * MON"] {
            let triggers = vec![Trigger::Cron { expression: expression.into(), time_zone: "UTC".into() }];
            assert_eq!(summary(&triggers, Tz::UTC), format!("Cron {expression}"), "{expression}");
        }
    }

    #[test]
    fn agenda_groups_same_days_and_timezone_in_one_phrase() {
        let cron = |expression: &str, time_zone: &str| Trigger::Cron {
            expression: expression.into(),
            time_zone: time_zone.into(),
        };
        assert_eq!(
            agenda(&[cron("0 9 * * *", "America/Sao_Paulo"), cron("0 15 * * *", "America/Sao_Paulo")]),
            "Todo dia às 9h e às 15h"
        );
        assert_eq!(agenda(&[cron("30 9 * * 1-5", "America/Sao_Paulo")]), "Dias úteis às 9h30");
        assert_eq!(
            agenda(&[cron("0 9,15 * * *", "UTC"), cron("0 12 * * *", "UTC")]),
            "Todo dia às 9h, às 12h e às 15h"
        );
        assert_eq!(
            agenda(&[cron("0 9 * * 1", "UTC"), cron("0 9 * * 2", "UTC")]),
            "Toda segunda às 9h · Toda terça às 9h"
        );
        assert_eq!(
            agenda(&[cron("0 9 * * *", "UTC"), cron("0 9 * * *", "America/Sao_Paulo")]),
            "Todo dia às 9h · Todo dia às 9h (America/Sao_Paulo)"
        );
        assert_eq!(
            agenda(&[cron("0 9 * * *", "UTC"), Trigger::Interval { seconds: 3600 }, cron("0 15 * * *", "UTC")]),
            "Todo dia às 9h e às 15h · A cada hora"
        );
        assert_eq!(
            agenda(&[cron("*/15 * * * *", "UTC"), cron("0 9-17 * * *", "UTC")]),
            "A cada 15 min · Cron 0 9-17 * * *"
        );
    }

    #[test]
    fn agenda_caps_long_time_lists_with_and_more() {
        let cron = |expression: &str| Trigger::Cron { expression: expression.into(), time_zone: "UTC".into() };
        let eight: Vec<_> = (9..=16).map(|hour| cron(&format!("0 {hour} * * *"))).collect();
        assert_eq!(agenda(&eight), "Todo dia às 9h, às 10h, às 11h, às 12h, às 13h, às 14h e mais 2 horários");
        let seven: Vec<_> = (9..=15).map(|hour| cron(&format!("0 {hour} * * *"))).collect();
        assert_eq!(agenda(&seven), "Todo dia às 9h, às 10h, às 11h, às 12h, às 13h, às 14h e mais 1 horário");
        assert_eq!(
            agenda(&[cron("0,15,30,45 0,6,12,18 * * *")]),
            "Todo dia à 0h, à 0h15, à 0h30, à 0h45, às 6h, às 6h15 e mais 10 horários"
        );
    }

    #[test]
    fn interval_keeps_the_scheduler_cadence_and_dense_routines_have_no_cards() {
        let now = ms("2026-10-20T12:00:00Z");
        let mut r = routine();
        r.triggers = vec![Trigger::Interval { seconds: 3600 }];
        r.next_runs = vec![Some(now + 25 * 60_000)];
        let found = project(&r, now + 3 * 3_600_000, now + 6 * 3_600_000, now, 100).unwrap().unwrap();
        let at: Vec<_> = found.iter().map(|(_, at)| *at).collect();
        assert_eq!(at, [now + 205 * 60_000, now + 265 * 60_000, now + 325 * 60_000], "mantém o :25 do agendador");
        r.triggers = vec![Trigger::Interval { seconds: 60 }];
        let projection = occurrences(&[r.clone()], now, now + DAY_MS, now);
        assert_eq!(projection.occurrences.len(), 0);
        assert_eq!(projection.dense, [r.id.clone()]);
    }

    #[test]
    fn webhooks_have_no_occurrences_and_paused_routines_project_from_now() {
        let now = ms("2026-10-20T12:00:00Z");
        let mut r = routine();
        r.triggers = vec![Trigger::Webhook];
        r.next_runs = vec![None];
        assert_eq!(project(&r, now, now + DAY_MS, now, 100).unwrap().unwrap(), []);
        r.triggers = vec![Trigger::Cron { expression: "30 14 * * *".into(), time_zone: "UTC".into() }];
        r.next_runs = vec![Some(ms("2026-10-01T14:30:00Z"))];
        r.enabled = false;
        let cards = occurrences(&[r.clone()], now - DAY_MS, now + DAY_MS, now).occurrences;
        assert_eq!(cards.len(), 1, "só o futuro: o passado são as execuções");
        assert_eq!(cards[0]["at"], ms("2026-10-20T14:30:00Z"));
        assert_eq!(cards[0]["enabled"], false);
        // Pausada de intervalo: a cadência salva, a mesma em consultas seguidas.
        r.triggers = vec![Trigger::Interval { seconds: 6 * 3600 }];
        r.next_runs = vec![Some(ms("2026-10-19T03:00:00Z"))];
        let first = occurrences(&[r.clone()], now, now + DAY_MS, now).occurrences;
        let later = occurrences(&[r], now, now + DAY_MS, now + 5_000).occurrences;
        assert_eq!(first[0]["at"], ms("2026-10-20T15:00:00Z"));
        assert_eq!(first, later);
    }

    #[test]
    fn frequent_is_per_hour_and_the_cap_cuts_the_busiest_without_calling_them_frequent() {
        let now = ms("2026-10-19T23:59:59Z");
        let from = ms("2026-10-20T00:00:00Z");
        let cron = |id: &str, expression: &str| {
            let mut r = routine();
            r.id = id.into();
            r.triggers = vec![Trigger::Cron { expression: expression.into(), time_zone: "UTC".into() }];
            r.next_runs = vec![r.triggers[0].next(now).unwrap()];
            r
        };
        // 24 por dia, mas 12 por hora entre 9 e 11: frequente. De meia em meia hora não é.
        let burst = cron("a-burst", "*/5 9-10 * * *");
        let mut broken = cron("e-broken", "0 9 * * *");
        broken.triggers = vec![Trigger::Cron { expression: "0 9 * * *".into(), time_zone: "Nowhere/Zone".into() }];
        let routines = [
            burst,
            cron("b-half", "*/30 * * * *"),
            cron("c-half", "0,30 * * * *"),
            cron("d-hourly", "0 * * * *"),
            broken,
        ];
        let projection = occurrences(&routines, from, from + 50 * DAY_MS, now);
        assert_eq!(projection.dense, ["a-burst"]);
        assert_eq!(projection.invalid, ["e-broken"], "uma agenda inválida não derruba a visão");
        // 2400 + 2400 + 1200 passa de 5000: sai uma das de 2400 (a de menor id), não a de hora em hora.
        assert_eq!(projection.truncated, ["b-half"]);
        assert_eq!(projection.occurrences.len(), 2400 + 1200);
    }

    #[test]
    fn manual_routine_reads_only_when_i_say_so() {
        assert_eq!(summary(&[], Tz::UTC), "Só quando eu mandar");
        assert_eq!(agenda(&[]), "Só quando eu mandar");
        let preview = Routines::schedule_preview_pt(&json!({"draft":{"kind":"manual"}}), 1_790_000_000_000).unwrap();
        assert_eq!(preview["resumo"], "Só quando eu mandar");
        let mut r = routine();
        r.triggers = vec![];
        r.next_runs = vec![];
        assert_eq!(project(&r, 0, i64::MAX / 2, 1_790_000_000_000, 100).unwrap(), Some(vec![]));
    }
}
