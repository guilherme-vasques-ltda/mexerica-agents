// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Validated schedules. Calendar schedules retain their IANA time zone across restarts.
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use croner::Cron;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::str::FromStr;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Trigger {
    Interval {
        seconds: i64,
    },
    Once {
        at: i64,
    },
    Cron {
        expression: String,
        time_zone: String,
    },
    Webhook,
    Event {
        source: String,
        event: String,
        #[serde(default)]
        filters: std::collections::BTreeMap<String, String>,
    },
}

/// Um cron de relógio cujos campos de minuto e hora são números ou listas de números
/// (`0 9,15 * * *`); passos (`*/15`), faixas (`9-17`) e nomes (`MON`) não entram aqui
/// e seguem como expressão livre.
pub(crate) struct CronTimes {
    pub minutes: Vec<u8>,
    pub hours: Vec<u8>,
    pub day: String,
    pub month: String,
    pub weekday: String,
}

fn number_list(field: &str, max: u8) -> Option<Vec<u8>> {
    let mut values = Vec::new();
    for part in field.split(',') {
        let value: u8 = part.parse().ok()?;
        if value > max {
            return None;
        }
        values.push(value);
    }
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    values.dedup();
    Some(values)
}

pub(crate) fn split_cron_lists(expression: &str) -> Option<CronTimes> {
    let fields: Vec<_> = expression.split_whitespace().collect();
    let [minute, hour, day, month, weekday] = fields.as_slice() else { return None };
    Some(CronTimes {
        minutes: number_list(minute, 59)?,
        hours: number_list(hour, 23)?,
        day: (*day).to_owned(),
        month: (*month).to_owned(),
        weekday: (*weekday).to_owned(),
    })
}

/// O teto de 20 gatilhos, em pt-BR leigo.
pub(crate) fn too_many_triggers(count: usize) -> String {
    format!("A rotina comporta até 20 gatilhos; esta tem {count}. Tire alguns ou divida em outra rotina.")
}

/// Fuso que não existe no banco IANA, em pt-BR leigo.
fn unknown_time_zone(time_zone: &str) -> String {
    format!("Fuso horário desconhecido: \"{time_zone}\". Use um nome como America/Sao_Paulo.")
}

/// Chaves canônicas de um cron de relógio, um item por horário: listas viram um item
/// por horário (`0 15,9` e `0 9,15` caem no mesmo) e `1-7` e `0-6` valem `*`. Fora de
/// números e listas (passos, faixas, nomes), a expressão entra como está.
fn expanded_keys(expression: &str, time_zone: &str) -> Vec<(String, String)> {
    let zone = time_zone.trim().to_owned();
    let Some(times) = split_cron_lists(expression) else {
        let normalized = expression.split_whitespace().collect::<Vec<_>>().join(" ");
        return vec![(normalized, zone)];
    };
    let weekday = match times.weekday.as_str() {
        "1-7" | "0-6" => "*".to_owned(),
        _ => times.weekday.clone(),
    };
    let mut keys = Vec::with_capacity(times.hours.len() * times.minutes.len());
    for hour in &times.hours {
        for minute in &times.minutes {
            keys.push((format!("{minute} {hour} {} {} {weekday}", times.day, times.month), zone.clone()));
        }
    }
    keys
}

/// O primeiro horário repetido entre os gatilhos de relógio, com a mesma mensagem leiga
/// do editor; `None` quando não há repetição. Compara pelas chaves canônicas, então
/// `0 9,15` ao lado de `0 9` recusa os 9h duplicados.
pub(crate) fn duplicate_cron_message(triggers: &[Trigger]) -> Option<String> {
    let mut seen = HashSet::new();
    for trigger in triggers {
        let Trigger::Cron { expression, time_zone } = trigger else { continue };
        let normalized = expression.split_whitespace().collect::<Vec<_>>().join(" ");
        for key in expanded_keys(&normalized, time_zone) {
            if !seen.insert(key) {
                return Some(format!(
                    "Dois horários iguais: {}. Troque um deles ou tire um.",
                    duplicate_label(&normalized)
                ));
            }
        }
    }
    None
}

/// `15:00` quando dá para ler um horário só; senão, a expressão como está.
fn duplicate_label(expression: &str) -> String {
    if let Some(times) = split_cron_lists(expression)
        && let ([minute], [hour]) = (times.minutes.as_slice(), times.hours.as_slice())
    {
        return format!("{hour:02}:{minute:02}");
    }
    expression.to_owned()
}

impl Trigger {
    pub fn validate(&self, now: i64) -> Result<()> {
        match self {
            Self::Interval { seconds } if !(1..=31_536_000).contains(seconds) => {
                bail!("interval must be between 1 second and 365 days")
            }
            Self::Once { at } if *at <= now => bail!("one-time trigger must be in the future"),
            Self::Cron { expression, time_zone } => {
                if expression.split_whitespace().count() != 5 {
                    bail!("use a five-field cron expression");
                }
                Cron::from_str(expression).context("invalid cron expression")?;
                time_zone.parse::<Tz>().with_context(|| unknown_time_zone(time_zone))?;
                self.next(now)?.context("schedule has no future occurrence")?;
            }
            Self::Event { source, event, filters } => {
                if !["slack", "github", "origin", "microsoftTeams", "linear", "sentry", "pagerduty", "email"]
                    .contains(&source.as_str())
                {
                    bail!("unsupported event source");
                }
                if event.is_empty()
                    || event.len() > 200
                    || filters.len() > 20
                    || filters.iter().any(|(k, v)| k.is_empty() || k.len() > 200 || v.len() > 4000)
                {
                    bail!("invalid event filter");
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub fn next(&self, after: i64) -> Result<Option<i64>> {
        Ok(match self {
            Self::Interval { seconds } => Some(
                after
                    .checked_add(seconds.checked_mul(1000).context("interval overflow")?)
                    .context("schedule overflow")?,
            ),
            Self::Once { at } => (*at > after).then_some(*at),
            Self::Cron { expression, time_zone } => {
                let zone = time_zone.parse::<Tz>().with_context(|| unknown_time_zone(time_zone))?;
                let date = DateTime::<Utc>::from_timestamp_millis(after)
                    .context("invalid schedule timestamp")?
                    .with_timezone(&zone);
                Some(Cron::from_str(expression)?.find_next_occurrence(&date, false)?.timestamp_millis())
            }
            Self::Webhook | Self::Event { .. } => None,
        })
    }

    /// Skip missed intervals without moving the original cadence forward on each tick.
    pub fn advance(&self, due: i64, now: i64) -> Result<Option<i64>> {
        if let Self::Interval { seconds } = self {
            let step = seconds.checked_mul(1000).filter(|v| *v > 0).context("invalid interval")?;
            let elapsed = now.checked_sub(due).context("schedule overflow")?.max(0);
            let count = elapsed.checked_div(step).and_then(|n| n.checked_add(1)).context("schedule overflow")?;
            return Ok(Some(
                due.checked_add(count.checked_mul(step).context("schedule overflow")?).context("schedule overflow")?,
            ));
        }
        self.next(now)
    }

    pub fn description(&self) -> String {
        match self {
            Self::Interval { seconds } if seconds % 86400 == 0 => format!("Every {} days", seconds / 86400),
            Self::Interval { seconds } if seconds % 3600 == 0 => format!("Every {} hours", seconds / 3600),
            Self::Interval { seconds } if seconds % 60 == 0 => format!("Every {} minutes", seconds / 60),
            Self::Interval { seconds } => format!("Every {seconds} seconds"),
            Self::Once { at } => DateTime::from_timestamp_millis(*at)
                .map_or_else(|| "Once".into(), |d| format!("Once at {}", d.to_rfc3339())),
            Self::Cron { expression, time_zone } => format!(
                "{} · {time_zone}",
                Cron::from_str(expression)
                    .map_or_else(|_| expression.clone(), |cron| cron.describe().trim_end_matches('.').to_owned())
            ),
            Self::Webhook => "When a webhook fires".into(),
            Self::Event { source, event, .. } => format!("{source} · {event}"),
        }
    }

    pub fn matches(&self, payload: &serde_json::Value) -> bool {
        let Self::Event { source, event, filters } = self else { return false };
        payload["source"].as_str() == Some(source)
            && (event == "*" || payload["event"].as_str() == Some(event))
            && filters.iter().all(|(key, value)| {
                if key == "messageContains" {
                    payload["text"].as_str().is_some_and(|text| text.contains(value))
                } else {
                    payload[key].as_str() == Some(value)
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn taipei_sunday_afternoon_uses_correct_utc_time() {
        let t = Trigger::Cron { expression: "20 14 * * 0".into(), time_zone: "Asia/Taipei".into() };
        let now = DateTime::parse_from_rfc3339("2026-09-26T12:00:00Z").unwrap().timestamp_millis();
        let expected = DateTime::parse_from_rfc3339("2026-09-27T06:20:00Z").unwrap().timestamp_millis();
        assert_eq!(t.next(now).unwrap(), Some(expected));
    }
    #[test]
    fn calendar_presets_follow_clock_boundaries() {
        let now = DateTime::parse_from_rfc3339("2026-09-27T01:07:00Z").unwrap().timestamp_millis();
        for (expression, expected) in [
            ("*/15 * * * *", "2026-09-27T01:15:00Z"),
            ("37 * * * *", "2026-09-27T01:37:00Z"),
            ("0 9 * * 1,3,5", "2026-09-28T01:00:00Z"),
            ("0 9 31 * *", "2026-10-31T01:00:00Z"),
        ] {
            let trigger = Trigger::Cron { expression: expression.into(), time_zone: "Asia/Taipei".into() };
            trigger.validate(now).unwrap();
            let expected = DateTime::parse_from_rfc3339(expected).unwrap().timestamp_millis();
            assert_eq!(trigger.next(now).unwrap(), Some(expected), "{expression}");
        }
    }

    #[test]
    fn event_filters_require_all_fields() {
        let t = Trigger::Event {
            source: "github".into(),
            event: "pull_request.opened".into(),
            filters: [("repo".into(), "owner/repo".into())].into(),
        };
        assert!(t.matches(&serde_json::json!({"source":"github","event":"pull_request.opened","repo":"owner/repo"})));
        assert!(!t.matches(&serde_json::json!({"source":"github","event":"pull_request.opened","repo":"other/repo"})));
    }
    #[test]
    fn invalid_schedules_are_rejected() {
        assert!(Trigger::Interval { seconds: 0 }.validate(0).is_err());
        assert!(Trigger::Once { at: 0 }.validate(0).is_err());
        assert!(Trigger::Cron { expression: "* * * * *".into(), time_zone: "not-a-zone".into() }.validate(0).is_err());
    }

    #[test]
    fn cron_lists_parse_sorted_and_deduped() {
        let times = split_cron_lists("0 9,15 * * *").unwrap();
        assert_eq!((times.minutes, times.hours), (vec![0], vec![9, 15]));
        let times = split_cron_lists("30,0 15,9 * * 1-5").unwrap();
        assert_eq!((times.minutes, times.hours), (vec![0, 30], vec![9, 15]));
        assert_eq!(split_cron_lists("0 9,9 * * *").unwrap().hours, vec![9]);
    }

    #[test]
    fn cron_lists_reject_steps_ranges_and_out_of_range_values() {
        for expression in [
            "*/15 * * * *",
            "0 9-17 * * *",
            "0 MON * * *",
            "61 9 * * *",
            "0 24 * * *",
            "0 9 * *",
            "0 0 9 * * *",
            "0 9,,15 * * *",
            "",
        ] {
            assert!(split_cron_lists(expression).is_none(), "{expression}");
        }
        assert!(split_cron_lists("0 9 * * *").is_some());
    }

    fn firings(trigger: &Trigger, mut after: i64, end: i64) -> Vec<i64> {
        let mut found = Vec::new();
        while let Some(at) = trigger.next(after).unwrap() {
            if at <= after || at >= end {
                break;
            }
            found.push(at);
            after = at;
        }
        found
    }

    /// O croner resolve avulsos e listas de jeitos diferentes na virada de primavera em
    /// Nova York (08/03/2026): cada avulso dispara também às 3h, a lista pula a hora que
    /// não existe. Por isso listas nunca são expandidas ao gravar. Se um dia os dois
    /// lados forem iguais, dá para reconsiderar a expansão.
    #[test]
    fn spring_forward_diverges_so_lists_are_never_expanded() {
        let now = DateTime::parse_from_rfc3339("2026-03-06T12:00:00Z").unwrap().timestamp_millis();
        let end = DateTime::parse_from_rfc3339("2026-03-10T12:00:00Z").unwrap().timestamp_millis();
        let single = Trigger::Cron { expression: "0,30 1,2 * * *".into(), time_zone: "America/New_York".into() };
        let expanded = ["0 1 * * *", "30 1 * * *", "0 2 * * *", "30 2 * * *"]
            .map(|expression| Trigger::Cron { expression: expression.into(), time_zone: "America/New_York".into() });
        let mut merged: Vec<i64> = expanded.iter().flat_map(|t| firings(t, now, end)).collect();
        merged.sort_unstable();
        assert_ne!(firings(&single, now, end), merged);
    }

    fn ms(rfc3339: &str) -> i64 {
        DateTime::parse_from_rfc3339(rfc3339).unwrap().timestamp_millis()
    }

    fn cron(expression: &str, time_zone: &str) -> Trigger {
        Trigger::Cron { expression: expression.into(), time_zone: time_zone.into() }
    }

    #[test]
    fn sao_paulo_sem_dst_o_ano_todo() {
        let noon_utc = "T12:00:00Z";
        for day in ["2026-01-10", "2026-07-10"] {
            let trigger = cron("0 9 * * *", "America/Sao_Paulo");
            trigger.validate(ms(&format!("{day}T11:00:00Z"))).unwrap();
            assert_eq!(trigger.next(ms(&format!("{day}T11:00:00Z"))).unwrap(), Some(ms(&format!("{day}{noon_utc}"))));
        }
    }

    #[test]
    fn lisbon_verao_e_inverno() {
        let trigger = cron("0 9 * * *", "Europe/Lisbon");
        assert_eq!(trigger.next(ms("2026-01-10T00:00:00Z")).unwrap(), Some(ms("2026-01-10T09:00:00Z")));
        assert_eq!(trigger.next(ms("2026-07-10T00:00:00Z")).unwrap(), Some(ms("2026-07-10T08:00:00Z")));
    }

    #[test]
    fn kolkata_offset_de_meia_hora() {
        let trigger = cron("0 9 * * *", "Asia/Kolkata");
        assert_eq!(trigger.next(ms("2026-06-10T00:00:00Z")).unwrap(), Some(ms("2026-06-10T03:30:00Z")));
    }

    #[test]
    fn utc_e_etc_utc_sao_equivalentes() {
        for after in ["2026-01-10T00:00:00Z", "2026-03-29T00:30:00Z", "2026-11-01T05:00:00Z"] {
            assert_eq!(
                cron("0 9 * * *", "UTC").next(ms(after)).unwrap(),
                cron("0 9 * * *", "Etc/UTC").next(ms(after)).unwrap(),
                "{after}"
            );
        }
    }

    #[test]
    fn primavera_lacuna_nova_york_encosta_sem_panico() {
        let trigger = cron("30 2 * * *", "America/New_York");
        trigger.validate(ms("2026-03-06T12:00:00Z")).unwrap();
        let zone: Tz = "America/New_York".parse().unwrap();
        let end = ms("2026-03-10T12:00:00Z");
        let mut found = Vec::new();
        let mut after = ms("2026-03-06T12:00:00Z");
        while let Some(at) = trigger.next(after).unwrap() {
            if at >= end {
                break;
            }
            found.push(at);
            after = at;
        }
        // 02:30 não existe em 08/03/2026: o horário fixo encosta nas 03:00 (EDT).
        assert!(found.contains(&ms("2026-03-08T07:00:00Z")), "{found:?}");
        for at in &found {
            let local = DateTime::<Utc>::from_timestamp_millis(*at).unwrap().with_timezone(&zone);
            let is_gap_day = local.format("%m-%d").to_string() == "03-08";
            assert!(!(is_gap_day && local.format("%H:%M").to_string() == "02:30"), "{found:?}");
        }
    }

    #[test]
    fn outono_repeticao_nova_york_dispara_uma_vez_no_horario_fixo() {
        let trigger = cron("30 1 * * *", "America/New_York");
        let end = ms("2026-11-03T12:00:00Z");
        let mut found = Vec::new();
        let mut after = ms("2026-10-31T12:00:00Z");
        while let Some(at) = trigger.next(after).unwrap() {
            if at >= end {
                break;
            }
            found.push(at);
            after = at;
        }
        // 01:30 acontece duas vezes em 01/11/2026: o horário fixo sai só na primeira (EDT).
        assert_eq!(found, vec![ms("2026-11-01T05:30:00Z"), ms("2026-11-02T06:30:00Z"), ms("2026-11-03T06:30:00Z")]);
    }

    /// Espelho de outono do tripwire da primavera (#980), com veredito oposto e
    /// verificado contra o iterador do croner: na repetição, lista e avulsos
    /// CONCORDAM — cada horário repetido sai uma vez só, na primeira ocorrência
    /// (EDT). A proibição de expandir listas ao gravar vale pela divergência da
    /// PRIMAVERA; este teste trava o acordo do outono para uma mudança futura
    /// do croner apitar aqui.
    #[test]
    fn fall_back_list_matches_expanded_first_occurrences() {
        let now = ms("2026-10-31T12:00:00Z");
        let end = ms("2026-11-03T12:00:00Z");
        let single = cron("0,30 1 * * *", "America/New_York");
        let expanded = ["0 1 * * *", "30 1 * * *"].map(|expression| cron(expression, "America/New_York"));
        let mut merged: Vec<i64> = expanded.iter().flat_map(|t| firings(t, now, end)).collect();
        merged.sort_unstable();
        let listed = firings(&single, now, end);
        assert_eq!(listed, merged);
        // 01/11/2026 (repetição): 01:00 e 01:30 saem uma vez cada, na primeira (EDT).
        assert_eq!(
            listed,
            vec![
                ms("2026-11-01T05:00:00Z"),
                ms("2026-11-01T05:30:00Z"),
                ms("2026-11-02T06:00:00Z"),
                ms("2026-11-02T06:30:00Z"),
                ms("2026-11-03T06:00:00Z"),
                ms("2026-11-03T06:30:00Z"),
            ]
        );
    }

    #[test]
    fn lord_howe_dst_de_30_minutos() {
        let trigger = cron("0 12 * * *", "Australia/Lord_Howe");
        // Virada 04/10/2026: +10:30 vira +11:00; do meio-dia de um dia ao outro vão 23h30.
        assert_eq!(trigger.next(ms("2026-10-03T00:00:00Z")).unwrap(), Some(ms("2026-10-03T01:30:00Z")));
        assert_eq!(trigger.next(ms("2026-10-03T01:30:00Z")).unwrap(), Some(ms("2026-10-04T01:00:00Z")));
    }

    #[test]
    fn fuso_invalido_da_erro_leigo_sem_panico() {
        for zone in ["Brasilia", "America/Sao_Paulo2", ""] {
            let trigger = cron("0 9 * * *", zone);
            let validation = format!("{:#}", trigger.validate(ms("2026-01-10T00:00:00Z")).unwrap_err());
            assert!(validation.contains("Fuso horário desconhecido"), "{validation}");
            assert!(validation.contains("America/Sao_Paulo"), "{validation}");
            assert!(trigger.next(ms("2026-01-10T00:00:00Z")).is_err());
        }
    }

    #[test]
    fn next_preserva_os_milissegundos_da_consulta() {
        // O croner zera os segundos mas mantém a parte sub-segundo da consulta:
        // a ocorrência "das 9h" sai 09:00:00.500 quando a consulta tem .500.
        // Verificado contra o iterador; dispara no mesmo minuto, sem efeito visível.
        let trigger = cron("0 9 * * *", "UTC");
        assert_eq!(trigger.next(ms("2026-01-10T00:00:00.500Z")).unwrap(), Some(ms("2026-01-10T09:00:00.500Z")));
    }

    #[test]
    fn fim_de_mes_e_29_de_fevereiro_em_ano_bissexto() {
        let last_day = cron("0 9 31 * *", "UTC");
        assert_eq!(last_day.next(ms("2026-02-01T00:00:00Z")).unwrap(), Some(ms("2026-03-31T09:00:00Z")));
        assert_eq!(last_day.next(ms("2026-04-15T00:00:00Z")).unwrap(), Some(ms("2026-05-31T09:00:00Z")));
        let feb29 = cron("0 9 29 2 *", "UTC");
        assert_eq!(feb29.next(ms("2023-01-01T00:00:00Z")).unwrap(), Some(ms("2024-02-29T09:00:00Z")));
        assert_eq!(feb29.next(ms("2025-06-01T00:00:00Z")).unwrap(), Some(ms("2028-02-29T09:00:00Z")));
    }

    #[test]
    fn duplicate_cron_names_the_repeated_time_in_plain_language() {
        let cron = |expression: &str, time_zone: &str| Trigger::Cron {
            expression: expression.into(),
            time_zone: time_zone.into(),
        };
        assert_eq!(
            duplicate_cron_message(&[cron("0 15 * * *", "UTC"), cron("0 15 * * *", "UTC")]).as_deref(),
            Some("Dois horários iguais: 15:00. Troque um deles ou tire um.")
        );
        assert_eq!(
            duplicate_cron_message(&[cron("30 9 * * 1-5", "UTC"), cron("30  9  *  *  1-5", "UTC")]).as_deref(),
            Some("Dois horários iguais: 09:30. Troque um deles ou tire um.")
        );
        assert_eq!(
            duplicate_cron_message(&[cron("0 9 * * *", "UTC"), cron("0 9 * * 1-7", "UTC")]).as_deref(),
            Some("Dois horários iguais: 09:00. Troque um deles ou tire um.")
        );
        assert_eq!(
            duplicate_cron_message(&[cron("00 9 * * *", "UTC"), cron("0 9 * * *", "UTC")]).as_deref(),
            Some("Dois horários iguais: 09:00. Troque um deles ou tire um.")
        );
        assert_eq!(
            duplicate_cron_message(&[cron("0 9,15 * * *", "UTC"), cron("0 9 * * *", "UTC")]).as_deref(),
            Some("Dois horários iguais: 09:00. Troque um deles ou tire um.")
        );
        assert_eq!(
            duplicate_cron_message(&[cron("0 15,9 * * *", "UTC"), cron("0 9,15 * * *", "UTC")]).as_deref(),
            Some("Dois horários iguais: 0 9,15 * * *. Troque um deles ou tire um.")
        );
        assert_eq!(
            duplicate_cron_message(&[cron("0 9,15 * * *", "UTC"), cron("0 9,15 * * *", "UTC")]).as_deref(),
            Some("Dois horários iguais: 0 9,15 * * *. Troque um deles ou tire um.")
        );
        assert_eq!(duplicate_cron_message(&[cron("0 15 * * *", "UTC"), cron("0 15 * * *", "America/Sao_Paulo")]), None);
        assert_eq!(duplicate_cron_message(&[cron("0 15 * * *", "UTC"), cron("30 15 * * *", "UTC")]), None);
        assert_eq!(duplicate_cron_message(&[cron("0 15 * * *", "UTC"), Trigger::Interval { seconds: 60 }]), None);
    }
}
