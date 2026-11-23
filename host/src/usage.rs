// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
//! Subscription usage limits, read locally on the host — no extra login.
//!
//! Claude: passive status-line and rate-limit events. Codex: current
//! global CLI account limits, without a model turn or reading old rollouts.

use crate::LockExt;
use crate::hub::Hub;
use serde::Serialize;
use serde_json::Value;
#[cfg(all(test, unix))]
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

const POLL_EVERY: Duration = Duration::from_secs(5 * 60);

/// MEX-1294/F6 (m): janela a partir da qual a conta está "perto do limite".
pub const NEAR_LIMIT_PERCENT: f64 = 90.0;

/// Everything the apps show about limits (wire shape of the `usage` event).
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Usage {
    pub providers: Vec<Provider>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    pub windows: Vec<Window>,
    /// Where the numbers came from: `claude` (CLI), `statusline`, `agent`, `cli`.
    pub source: &'static str,
    pub updated_at: i64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    pub id: String,
    pub label: String,
    pub percent: f64,
    /// Epoch milliseconds, when known.
    pub resets_at: Option<i64>,
    /// Human text when only that is known ("Sep 26 at 12pm (Asia/Taipei)").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_text: Option<String>,
    /// Duration of the window in minutes when the provider says it (Codex); never serialized.
    #[serde(skip)]
    pub minutes: Option<i64>,
}

impl Window {
    fn new(id: impl Into<String>, label: impl Into<String>, percent: f64, resets_at: Option<i64>) -> Self {
        Self { id: id.into(), label: label.into(), percent, resets_at, resets_text: None, minutes: None }
    }
}

impl Usage {
    /// Replaces `new` windows (by id) in provider `id`, keeping the provider's other windows.
    fn merge(&mut self, id: &'static str, name: &'static str, source: &'static str, new: Vec<Window>) {
        let mut windows = self.providers.iter().find(|p| p.id == id).map(|p| p.windows.clone()).unwrap_or_default();
        windows.retain(|w| !new.iter().any(|n| n.id == w.id));
        windows.extend(new);
        windows.sort_by(|a, b| a.id.cmp(&b.id));
        self.providers.retain(|p| p.id != id);
        self.providers.push(Provider { id, name, windows, source, updated_at: crate::store::now_ms() });
        self.providers.sort_by_key(|p| p.id);
    }
}

fn merge(hub: &Hub, id: &'static str, name: &'static str, source: &'static str, windows: Vec<Window>) {
    crate::fork::uso_historico::record(hub, id, &windows);
    let mut usage = hub.usage.locked();
    usage.merge(id, name, source, windows);
    let _ = hub.events.send(serde_json::json!({"type":"usage", "usage": &*usage}));
}

pub async fn poll(hub: Arc<Hub>) {
    loop {
        refresh(&hub).await;
        tokio::time::sleep(POLL_EVERY).await;
    }
}

pub async fn refresh(hub: &Hub) {
    if crate::environment::Environment::is_open() {
        return;
    }
    crate::fork::codex_usage::refresh(hub).await;
}

/// Ponto de teste da chamada real, sem trocar PATH/ambiente global nem o parser.
#[cfg(all(test, unix))]
pub(crate) async fn claude_cli_usage_at(
    store: &crate::store::Store,
    bin: &Path,
    home: &Path,
    timeout: Duration,
) -> Option<Vec<Window>> {
    let env: Vec<(std::ffi::OsString, std::ffi::OsString)> = crate::agent::environment::for_cli(store, "claude");
    claude_usage_output(bin, env, home, timeout).await
}

/// `Sep 26 at 12pm (Asia/Taipei)`, `5:30am (America/Sao_Paulo)` → instante em ms. Sem fuso ou fora desse formato,
/// `None` (desconhecido; nunca um palpite). Só hora do dia: hoje, ou amanhã se já passou. Com dia: neste ano, ou no
/// próximo se a data ficou mais de um dia para trás.
#[cfg(test)]
pub(crate) fn claude_reset_instant(text: &str, now_ms: i64) -> Option<i64> {
    use chrono::{Datelike, Days, NaiveDate, NaiveTime, TimeZone};
    let (when, zone) = text.trim().rsplit_once('(')?;
    let tz: chrono_tz::Tz = zone.strip_suffix(')')?.trim().parse().ok()?;
    let (date_part, clock) = match when.split_once(" at ") {
        Some((date, clock)) => (Some(date.trim()), clock.trim()),
        None => (None, when.trim()),
    };
    let clock = clock.to_ascii_lowercase();
    let (digits, pm) = match (clock.strip_suffix("am"), clock.strip_suffix("pm")) {
        (Some(d), _) => (d, false),
        (_, Some(d)) => (d, true),
        _ => return None,
    };
    let (hour, minute) = digits.trim().split_once(':').map_or((digits.trim(), "0"), |(h, m)| (h, m));
    let (hour, minute): (u32, u32) = (hour.parse().ok()?, minute.parse().ok()?);
    if !(1..=12).contains(&hour) || minute > 59 {
        return None;
    }
    let time = NaiveTime::from_hms_opt(hour % 12 + if pm { 12 } else { 0 }, minute, 0)?;
    let now = tz.timestamp_millis_opt(now_ms).single()?;
    let at = |date: NaiveDate| tz.from_local_datetime(&date.and_time(time)).earliest().map(|d| d.timestamp_millis());
    match date_part {
        None => {
            let today = at(now.date_naive())?;
            if today > now_ms { Some(today) } else { at(now.date_naive().checked_add_days(Days::new(1))?) }
        }
        Some(date) => {
            let (month, day) = date.split_once(' ')?;
            let month = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"]
                .iter()
                .position(|m| month.to_ascii_lowercase().starts_with(m))?;
            let month = u32::try_from(month).ok()? + 1;
            let day: u32 = day.trim().trim_end_matches(|c: char| c.is_ascii_alphabetic()).parse().ok()?;
            let this_year = at(NaiveDate::from_ymd_opt(now.year(), month, day)?)?;
            if this_year >= now_ms - 86_400_000 {
                Some(this_year)
            } else {
                at(NaiveDate::from_ymd_opt(now.year() + 1, month, day)?)
            }
        }
    }
}

#[cfg(all(test, unix))]
async fn claude_usage_output(
    bin: &Path,
    env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    home: &Path,
    timeout: Duration,
) -> Option<Vec<Window>> {
    let out = tokio::time::timeout(
        timeout,
        crate::fork::motor_exec::output(
            crate::fork::motor_exec::command(bin, home)
                .env_clear()
                .envs(env)
                // Não iniciar MCPs durante a consulta local.
                .args([
                    "-p",
                    "/usage",
                    "--output-format",
                    "json",
                    "--no-session-persistence",
                    "--setting-sources",
                    "",
                    "--strict-mcp-config",
                    "--mcp-config",
                    r#"{"mcpServers":{}}"#,
                ])
                .current_dir(crate::fork::motor_exec::hub_cwd(home))
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true),
        ),
    )
    .await
    .ok()?
    .ok()?;
    let v: Value = serde_json::from_slice(&out.stdout).ok()?;
    let windows = parse_claude_usage(v["result"].as_str()?);
    (!windows.is_empty()).then_some(windows)
}

/// Lines like `Current week (all models): 53% used · resets Sep 26 at 12pm (Asia/Taipei)`.
#[cfg(test)]
pub fn parse_claude_usage(text: &str) -> Vec<Window> {
    text.lines()
        .filter_map(|line| {
            let (name, rest) = line.trim().split_once(": ")?;
            let percent: f64 = rest.split_once("% used")?.0.trim().parse().ok()?;
            let resets = rest.split_once("resets ").map(|(_, r)| r.trim().to_owned());
            let (id, label) = match name {
                "Current session" => ("five_hour".to_owned(), "5-hour".to_owned()),
                "Current week (all models)" => ("seven_day".to_owned(), "Weekly".to_owned()),
                n => {
                    let model = n.strip_prefix("Current week (")?.strip_suffix(')')?;
                    (format!("seven_day_{}", model.to_lowercase()), format!("Weekly · {model}"))
                }
            };
            Some(Window { resets_text: resets, ..Window::new(id, label, percent, None) })
        })
        .collect()
}

/// `POST /ingest/statusline` body: Claude Code's statusline JSON.
pub fn ingest_statusline(hub: &Hub, body: &Value) {
    let rl = &body["rate_limits"];
    let windows: Vec<Window> = [("five_hour", "5-hour"), ("seven_day", "Weekly")]
        .into_iter()
        .filter_map(|(key, label)| {
            let percent = rl[key]["used_percentage"].as_f64()?;
            Some(Window::new(key, label, percent, rl[key]["resets_at"].as_i64().map(|s| s.saturating_mul(1000))))
        })
        .collect();
    if !windows.is_empty() {
        merge(hub, "claude", "Claude", "statusline", windows);
    }
}

/// Claude ACP adapter: `usage_update._meta["_claude/rateLimit"]` (the SDK's `SDKRateLimitInfo`).
pub fn ingest_claude_rate_limit(hub: &Hub, info: &Value) {
    if let Some(w) = claude_window(info) {
        merge(hub, "claude", "Claude", "agent", vec![w]);
    }
}

fn claude_window(info: &Value) -> Option<Window> {
    let kind = info["rateLimitType"].as_str()?;
    let label = match kind {
        "five_hour" => "5-hour",
        "seven_day" => "Weekly",
        "seven_day_opus" => "Weekly · Opus",
        "seven_day_sonnet" => "Weekly · Sonnet",
        _ => return None,
    };
    let u = info["utilization"].as_f64().or_else(|| (info["status"] == "rejected").then_some(1.0))?;
    // The SDK reports a 0–1 fraction (from the unified rate-limit headers).
    let percent = if u <= 1.0 { u * 100.0 } else { u };
    // Seconds, but accept milliseconds too.
    let resets = info["resetsAt"].as_i64().map(|t| if t > 100_000_000_000 { t } else { t.saturating_mul(1000) });
    Some(Window::new(kind, label, percent, resets))
}

/// Duração (min) das janelas de sessão e de semana do plano.
const SESSION_MINUTES: i64 = 300;
const WEEK_MINUTES: i64 = 10080;

/// MEX-1294/F6-A: janelas normalizadas de qualquer backend com leitura — só `session` e `week`,
/// com o reset conhecido. Janela que o provedor não informou não aparece (desconhecido, nunca 0%).
/// Codex classifica pela DURAÇÃO da janela (300 min = sessão, 10080 min = semana): numa conta só semanal
/// a `primary` é a semana. O nome do campo (`primary`/`secondary`) só vale quando a duração não vem e o
/// lugar ainda está livre; duração de outro tamanho não é nem sessão nem semana.
pub fn session_week(backend_id: &str, windows: &[Window]) -> Vec<Window> {
    let by_id = |ids: &[&str]| ids.iter().find_map(|id| windows.iter().find(|w| w.id == *id));
    let (mut session, mut week) = match backend_id {
        "claude" => (by_id(&["five_hour"]), by_id(&["seven_day"])),
        "codex" => (
            windows.iter().find(|w| w.minutes == Some(SESSION_MINUTES)),
            windows.iter().find(|w| w.minutes == Some(WEEK_MINUTES)),
        ),
        _ => (None, None),
    };
    if backend_id == "codex" {
        let unknown = |name: &str| windows.iter().find(|w| w.id == name && w.minutes.is_none_or(|m| m <= 0));
        session = session.or_else(|| unknown("primary"));
        week = week.or_else(|| unknown("secondary"));
    }
    [("session", "Session", session), ("week", "Week", week)]
        .into_iter()
        .filter_map(|(id, label, found)| {
            let w = found?;
            Some(Window { id: id.into(), label: label.into(), ..w.clone() })
        })
        .collect()
}

pub fn parse_codex(rl: &Value, observed_ms: i64) -> Vec<Window> {
    ["primary", "secondary"]
        .into_iter()
        .filter_map(|k| {
            let w = &rl[k];
            let percent = w["used_percent"].as_f64().or_else(|| w["usedPercent"].as_f64())?;
            if !percent.is_finite() || !(0.0..=100.0).contains(&percent) {
                return None;
            }
            let minutes = w["window_minutes"].as_i64().or_else(|| w["windowDurationMins"].as_i64()).unwrap_or(0);
            let label = match minutes {
                300 => "5-hour".to_owned(),
                10080 => "Weekly".to_owned(),
                m if m > 0 && m % 1440 == 0 => format!("{}-day", m / 1440),
                m if m > 0 => format!("{}-hour", m / 60),
                _ => k.to_owned(),
            };
            let resets =
                w["resets_at"].as_i64().or_else(|| w["resetsAt"].as_i64()).map(|s| s.saturating_mul(1000)).or_else(
                    || w["resets_in_seconds"].as_i64().map(|s| observed_ms.saturating_add(s.saturating_mul(1000))),
                );
            Some(Window { minutes: (minutes > 0).then_some(minutes), ..Window::new(k, label, percent, resets) })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_codex_rollout_limits() {
        let x = parse_codex(
            &json!({"primary": {"used_percent": 3.0, "window_minutes": 300, "resets_in_seconds": 10},
                    "secondary": {"used_percent": 40.0, "window_minutes": 10080, "resets_at": 100}}),
            1000,
        );
        assert_eq!(x[0].label, "5-hour");
        assert_eq!(x[0].resets_at, Some(11_000));
        assert_eq!(x[1].resets_at, Some(100_000));
    }

    #[test]
    fn parses_claude_usage_report() {
        let w = parse_claude_usage(
            "You are currently using your subscription\n\nCurrent session: 11% used · resets Sep 25 at 5am (Asia/Taipei)\nCurrent week (all models): 53% used · resets Sep 26 at 12pm (Asia/Taipei)\nCurrent week (Fable): 83% used · resets Sep 26 at 12pm (Asia/Taipei)\n\nLast 24h · 794 requests",
        );
        assert_eq!(w.len(), 3);
        assert_eq!(w[0].id, "five_hour");
        assert!((w[0].percent - 11.0).abs() < f64::EPSILON);
        assert_eq!(w[0].resets_text.as_deref(), Some("Sep 25 at 5am (Asia/Taipei)"));
        assert_eq!(w[2].label, "Weekly · Fable");
    }

    #[test]
    fn claude_rate_limit_fraction_becomes_percent() {
        let w = claude_window(&json!({"rateLimitType": "five_hour", "utilization": 0.42, "resetsAt": 1_790_000_000}))
            .unwrap();
        assert!((w.percent - 42.0).abs() < 1e-9);
        assert_eq!(w.resets_at, Some(1_790_000_000_000));
        assert!(claude_window(&json!({"rateLimitType": "overage", "utilization": 0.1})).is_none());
    }

    #[test]
    fn merge_grava_amostra_no_historico_de_uso() {
        let dir = std::env::temp_dir().join(format!("uso-historico-merge-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = crate::store::Store::open(Path::new(":memory:")).unwrap();
        let hub = Hub::new(
            store,
            "uso-historico-merge".into(),
            crate::remote::identity::Identity::load_or_create(&dir).unwrap(),
            "t".into(),
            0,
        );
        merge(&hub, "claude", "Claude", "statusline", vec![Window::new("five_hour", "5-hour", 42.0, Some(9))]);
        let amostras: i64 = hub
            .store
            .db
            .locked()
            .query_row(
                "SELECT COUNT(*) FROM donna_usage_samples WHERE provider='claude' AND window_id='five_hour'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(amostras, 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn merge_replaces_only_matching_windows() {
        let mut u = Usage::default();
        u.merge(
            "claude",
            "Claude",
            "claude",
            vec![Window::new("five_hour", "5-hour", 10.0, None), Window::new("seven_day", "Weekly", 50.0, None)],
        );
        u.merge("claude", "Claude", "statusline", vec![Window::new("five_hour", "5-hour", 12.0, None)]);
        let c = &u.providers[0];
        assert_eq!((c.windows.len(), c.source), (2, "statusline"));
        assert!((c.windows[0].percent - 12.0).abs() < f64::EPSILON, "five_hour updated");
        assert_eq!(serde_json::to_value(&u).unwrap()["providers"][0]["windows"][1]["resetsAt"], Value::Null);
    }

    // MEX-1294/F6-A: normalização session/week por conta — 89/90%, reseta e "desconhecido".
    fn win(id: &str, pct: f64) -> Window {
        Window::new(id, id, pct, None)
    }

    #[test]
    fn f6_normaliza_sessao_e_semana_89_e_90() {
        for backend in ["claude", "codex"] {
            let (s, w) = if backend == "claude" { ("five_hour", "seven_day") } else { ("primary", "secondary") };
            for pct in [89.0, 90.0] {
                let n = session_week(backend, &[win(s, pct), win(w, pct)]);
                assert_eq!(n.len(), 2, "{backend} {pct}");
                assert_eq!(n[0].id, "session");
                assert!((n[0].percent - pct).abs() < f64::EPSILON);
                assert_eq!(n[0].percent >= NEAR_LIMIT_PERCENT, pct >= 90.0);
                assert_eq!(n[1].id, "week");
            }
        }
    }

    #[test]
    fn f6_reset_vem_junto_na_janela() {
        let mut s = win("five_hour", 42.0);
        s.resets_at = Some(1_790_000_000_000);
        s.resets_text = Some("sex 14:00".into());
        let n = session_week("claude", &[s]);
        assert_eq!(n[0].resets_at, Some(1_790_000_000_000));
        assert_eq!(n[0].resets_text.as_deref(), Some("sex 14:00"));
    }

    #[test]
    fn f6_codex_classifica_pela_duracao_da_janela() {
        let rl = |primary: i64, secondary: Option<i64>| {
            let mut v = json!({"primary": {"usedPercent": 95.0, "windowDurationMins": primary}});
            if let Some(m) = secondary {
                v["secondary"] = json!({"usedPercent": 10.0, "windowDurationMins": m});
            }
            parse_codex(&v, 0)
        };
        // Conta só semanal: a `primary` de 10080 min é a semana.
        let n = session_week("codex", &rl(10080, None));
        assert_eq!(n.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(), ["week"]);
        // Ordem invertida: a duração manda.
        let n = session_week("codex", &rl(10080, Some(300)));
        assert_eq!((n[0].id.as_str(), n[0].percent), ("session", 10.0));
        assert_eq!((n[1].id.as_str(), n[1].percent), ("week", 95.0));
        // Duração que não é sessão nem semana não vira nenhuma das duas.
        assert_eq!(session_week("codex", &rl(1440, None)).len(), 0);
    }

    #[test]
    fn f6_claude_reset_em_texto_vira_instante() {
        // 2026-11-17 12:00 UTC.
        let now = 1_794_916_800_000;
        // Taipei = UTC+8: 26/11 às 12h local = 04h UTC.
        assert_eq!(claude_reset_instant("Nov 26 at 12pm (Asia/Taipei)", now), Some(1_795_665_600_000));
        // Só hora: hoje se ainda não passou, amanhã se passou (São Paulo = UTC−3: 14h local = 17h UTC, depois de agora).
        assert_eq!(claude_reset_instant("2pm (America/Sao_Paulo)", now), Some(now + 5 * 3_600_000));
        assert_eq!(claude_reset_instant("5:30am (America/Sao_Paulo)", now), Some(now + 73_800_000));
        // Data que já passou há mais de um dia = ano que vem; sem fuso, fora do formato ou hora inválida = desconhecido.
        assert!(claude_reset_instant("Jan 3 at 9am (UTC)", now).unwrap() > now);
        for ruim in [
            "Nov 26 at 12pm",
            "em breve (UTC)",
            "Nov 26 at 13pm (UTC)",
            "Nov 26 at 12 (UTC)",
            "Xyz 5 at 1pm (UTC)",
            "Nov 26 at 1pm (Mars/Base)",
        ] {
            assert_eq!(claude_reset_instant(ruim, now), None, "{ruim}");
        }
    }

    #[test]
    fn f6_sem_leitura_e_resposta_invalida_sao_desconhecidas() {
        // Janela que o provedor não informou não aparece; backend sem leitura = lista vazia.
        assert_eq!(session_week("claude", &[]).len(), 0, "sem leitura = desconhecido");
        assert_eq!(session_week("codex", &[win("primary", 12.0)])[0].id, "session");
        assert_eq!(session_week("codex", &[win("primary", 12.0)]).len(), 1, "semana ausente fica de fora");
        assert_eq!(session_week("desconhecido", &[win("primary", 50.0)]).len(), 0);
        // Resposta inválida do provedor não vira janela nenhuma.
        assert_eq!(parse_codex(&json!({}), 0).len(), 0);
        assert_eq!(parse_codex(&json!({"primary": {"usedPercent": "não-número"}}), 0).len(), 0);
        assert_eq!(parse_claude_usage("não é um relatório de uso").len(), 0);
        assert!(claude_window(&json!({"rateLimitType": "five_hour"})).is_none());
    }
}
