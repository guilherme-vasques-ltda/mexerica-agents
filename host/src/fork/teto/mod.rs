//! MX-145 E3: teto de gasto que trava trabalho autônomo (e, opcionalmente, as
//! conversas do dono) antes do envio do prompt. Estado fail-closed: qualquer
//! teto ativo que não possa ser conferido integralmente bloqueia o escopo coberto.
//! MX-159: o período do teto pode ser o mês civil UTC (padrão, inalterado) ou o
//! dia civil no fuso IANA do dono (`period`/`timeZone` na configuração).
//!
//! Persistência: configuração em `donna.spend-ceiling.v1` (kv), geração do estado
//! observado em `donna.ceiling.state.<mes>` (kv), avisos únicos em
//! `donna.ceiling.warned.<mes>` (kv), esperas duráveis na tabela
//! `donna_ceiling_holds` (UMA linha por trabalho travado, chave `(bot, trabalho)`)
//! e liberações na tabela append-only `donna_ceiling_releases` (gatilhos recusam
//! UPDATE/DELETE). O app lê tudo por RPC (`donnaCeiling`); nada entra no
//! `MIRROR_SCHEMA`.
//!
//! Toda conferência passa por [`evaluate_on`]: a mesma leitura grava a geração do
//! estado observado (inclusive o saudável e o desligado), marca avisos novos e
//! limpa esperas de fundo que deixaram de valer. A decisão que trava um envio é
//! tomada DENTRO da transação do permit (`decide_on`, pendurada em
//! `bot_admission::ClaimHooks::gate`). Um turno já permitido e em execução não é
//! interrompido — a conferência vale até o momento do envio.
use crate::{LockExt, api::devices::Caller, fork::bot_admission::Origin, hub::Hub};
use anyhow::{Result, anyhow, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::Arc;

const SETTINGS: &str = "donna.spend-ceiling.v1";
const TABLE: &str = "donna_ceiling_releases";
const HOLDS: &str = "donna_ceiling_holds";
/// Intenção durável de cancelar (Parar/recomeço/cancelamento) de um trabalho
/// parado: só INSERT, independente da linha da espera; sai junto com ela (gatilho).
const CANCELS: &str = "donna_ceiling_cancels";
/// Motivos persistidos: o app mostra o código, não uma frase.
pub(crate) const REASON_VALUE: &str = "valueReached";
pub(crate) const REASON_UNVERIFIED: &str = "unverifiable";

pub(crate) fn migrate(c: &Connection) -> Result<()> {
    // state: assinatura do estado liberado por checkFailure (`fingerprint#geração`)
    // ou, nas liberações de valor (plus25/plus50/setLimit), o novo limite exato em
    // texto. A geração anda a cada mudança do estado observado: uma checkFailure
    // velha nunca reabre um estado já superado.
    //
    // donna_ceiling_holds: uma linha por trabalho travado (rotina, pedido, turno de
    // grupo, captura humana/recuperação, job de fundo). `seq` guarda a ordem de
    // chegada; o upsert pela chave conserva a linha (e a ordem) quando o mesmo
    // trabalho volta a travar.
    //
    // donna_ceiling_cancels: intenção do dono de cancelar um trabalho parado,
    // gravada ANTES do efeito e por INSERT próprio (não depende de reescrever a
    // linha da espera); apagar a espera apaga a intenção no mesmo comando.
    //
    // donna_ceiling_ledger_gen: contador incrementado por gatilho em toda escrita
    // do ledger — a sonda do ator é O(1), sem varrer `donna_turn_usage`.
    c.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS {TABLE}(
          id TEXT PRIMARY KEY, at INTEGER NOT NULL, actor TEXT NOT NULL, kind TEXT NOT NULL,
          month TEXT NOT NULL, metric TEXT NOT NULL, limit_before REAL, limit_after REAL,
          overage_known REAL, scope TEXT NOT NULL, state TEXT);
         CREATE TRIGGER IF NOT EXISTS donna_ceiling_releases_no_update BEFORE UPDATE ON {TABLE}
          BEGIN SELECT RAISE(ABORT,'append_only'); END;
         CREATE TRIGGER IF NOT EXISTS donna_ceiling_releases_no_delete BEFORE DELETE ON {TABLE}
          BEGIN SELECT RAISE(ABORT,'append_only'); END;
         CREATE TABLE IF NOT EXISTS {HOLDS}(
          seq INTEGER PRIMARY KEY AUTOINCREMENT, bot_id TEXT NOT NULL, work TEXT NOT NULL,
          kind TEXT NOT NULL CHECK(kind IN ('turn','background')), body TEXT NOT NULL,
          UNIQUE(bot_id, work));
         CREATE TABLE IF NOT EXISTS {CANCELS}(
          bot_id TEXT NOT NULL, work TEXT NOT NULL, at INTEGER NOT NULL, PRIMARY KEY(bot_id, work));
         CREATE TRIGGER IF NOT EXISTS donna_ceiling_holds_cancel_gc AFTER DELETE ON {HOLDS}
          BEGIN DELETE FROM {CANCELS} WHERE bot_id=OLD.bot_id AND work=OLD.work; END;
         CREATE TABLE IF NOT EXISTS donna_ceiling_ledger_gen(id INTEGER PRIMARY KEY CHECK(id=1), n INTEGER NOT NULL);
         INSERT OR IGNORE INTO donna_ceiling_ledger_gen(id,n) VALUES(1,0);
         CREATE TRIGGER IF NOT EXISTS donna_ceiling_ledger_ins AFTER INSERT ON donna_turn_usage
          BEGIN UPDATE donna_ceiling_ledger_gen SET n=n+1 WHERE id=1; END;
         CREATE TRIGGER IF NOT EXISTS donna_ceiling_ledger_upd AFTER UPDATE ON donna_turn_usage
          BEGIN UPDATE donna_ceiling_ledger_gen SET n=n+1 WHERE id=1; END;
         CREATE TRIGGER IF NOT EXISTS donna_ceiling_ledger_del AFTER DELETE ON donna_turn_usage
          BEGIN UPDATE donna_ceiling_ledger_gen SET n=n+1 WHERE id=1; END;"
    ))?;
    migrate_kv_holds(c)?;
    migrate_legacy_d72(c)
}

/// Espera da retomada D72 gravada por versão anterior (sem o campo `waiting`):
/// captura humana/recuperação cujas entradas e conversa são exatamente as do
/// checkpoint D72 vivo do bot (`turn.waiting.<bot>`). Sem essa marca ela voltaria
/// como captura comum — um segundo consumidor do mesmo pedido.
pub(crate) fn legacy_d72(c: &Connection, bot: &str, body: &Value) -> Result<bool> {
    if body.get("waiting").is_some() || !matches!(body["origin"].as_str(), Some("human" | "recovery")) {
        return Ok(false);
    }
    if body["setup"]["entries"].as_array().is_none_or(Vec::is_empty) {
        return Ok(false);
    }
    let Some(raw) = read(c, &format!("turn.waiting.{bot}"))? else { return Ok(false) };
    let Ok(checkpoint) = serde_json::from_str::<Value>(&raw) else { return Ok(false) };
    Ok(matches!(checkpoint["phase"].as_str(), Some("waiting" | "retrying" | "paused"))
        && checkpoint["input"]["entries"] == body["setup"]["entries"]
        && checkpoint["input"]["chat"] == body["lane"]["chat"])
}

/// Marca na linha as esperas D72 antigas (`waiting: true`): o restore as devolve
/// à espera D72, que retoma uma vez só. Idempotente (a marca tira a linha do caso).
fn migrate_legacy_d72(c: &Connection) -> Result<()> {
    let rows = {
        let mut stmt = c.prepare(&format!("SELECT seq,bot_id,body FROM {HOLDS} WHERE kind='turn' ORDER BY seq"))?;
        stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (seq, bot, raw) in rows {
        let Ok(mut body) = serde_json::from_str::<Value>(&raw) else { continue };
        if legacy_d72(c, &bot, &body)? {
            body["waiting"] = json!(true);
            body["migratedWaiting"] = json!(true);
            c.execute(&format!("UPDATE {HOLDS} SET body=?2 WHERE seq=?1"), params![seq, body.to_string()])?;
        }
    }
    Ok(())
}

/// Esperas do formato anterior (`donna.ceiling.hold.<bot>` de turno e
/// `donna.ceiling.hold.bg.<bot>` de fundo, uma por bot no kv) viram linhas com
/// `work`, na ordem do instante gravado, com o corpo inteiro como evidência. A
/// origem no kv só sai depois que a linha existe; o que não dá para converter
/// (ilegível, sem origem, marcas `donna.ceiling.stop.*` presas a uma reserva
/// viva) fica no kv, intocado. Idempotente: rodar de novo só termina o que faltou.
fn migrate_kv_holds(c: &Connection) -> Result<()> {
    let rows = {
        let mut stmt = c.prepare("SELECT k,v FROM kv WHERE k GLOB 'donna.ceiling.hold.*'")?;
        stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut converted = Vec::new();
    for (key, raw) in rows {
        let Ok(mut body) = serde_json::from_str::<Value>(&raw) else {
            tracing::warn!(key, "espera do teto antiga ilegível; conservada no kv");
            continue;
        };
        let Some(rest) = key.strip_prefix("donna.ceiling.hold.") else { continue };
        let background = body["kind"] == "background";
        let (bot, work, kind) = match (background, rest.strip_prefix("bg."), body["job"].as_str()) {
            (true, Some(bot), Some(job)) => (bot.to_owned(), format!("bg:{job}"), "background"),
            (false, _, _) if body["origin"].is_string() => (rest.to_owned(), work_key(&body), "turn"),
            _ => {
                tracing::warn!(key, "espera do teto antiga sem origem reconhecível; conservada no kv");
                continue;
            }
        };
        body["work"] = json!(work);
        body["migratedFrom"] = json!(key);
        let at = body["at"].as_i64().unwrap_or(i64::MAX);
        converted.push((at, key, bot, work, kind, body));
    }
    // Ordem de chegada: `seq` segue o instante em que cada espera travou.
    converted.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    for (_, key, bot, work, kind, body) in converted {
        c.execute(
            &format!(
                "INSERT INTO {HOLDS}(bot_id,work,kind,body) VALUES(?1,?2,?3,?4) ON CONFLICT(bot_id,work) DO NOTHING"
            ),
            params![bot, work, kind, body.to_string()],
        )?;
        // A linha (nova ou já existente do mesmo trabalho) existe: só então a origem sai.
        if read_hold(c, &bot, &work)?.is_some() {
            c.execute("DELETE FROM kv WHERE k=?1", [&key])?;
        }
    }
    Ok(())
}

pub(crate) fn handles(method: &str) -> bool {
    matches!(method, "donnaCeiling" | "donnaCeilingSave" | "donnaCeilingRelease")
}

fn read(c: &Connection, key: &str) -> Result<Option<String>> {
    Ok(c.query_row("SELECT v FROM kv WHERE k=?1", [key], |r| r.get(0)).optional()?)
}
fn write(c: &Connection, key: &str, value: &Value) -> Result<()> {
    c.execute(
        "INSERT INTO kv(k,v) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
        [key, &value.to_string()],
    )?;
    Ok(())
}

// MARK: configuração

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Settings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub money: Vec<MoneyCeiling>,
    #[serde(default)]
    pub tokens: Option<u64>,
    #[serde(default = "default_warn_percent")]
    pub warn_percent: u8,
    #[serde(default)]
    pub apply_to_conversations: bool,
    /// Janela do teto: `month` (mês civil UTC, comportamento original) ou `day`
    /// (dia civil no fuso do dono). Hub antigo sem o campo = `month`.
    #[serde(default = "default_period")]
    pub period: String,
    /// Fuso IANA do período diário. Ausente: o do resumo diário do dono, senão
    /// `America/Sao_Paulo` (resolvido e gravado no `donnaCeilingSave`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,
}
const fn default_warn_percent() -> u8 {
    80
}
fn default_period() -> String {
    "month".into()
}
/// O padrão é o mesmo da desserialização: um `donnaCeiling` sem configuração
/// devolve algo que o próprio `donnaCeilingSave` aceita.
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            money: Vec::new(),
            tokens: None,
            warn_percent: default_warn_percent(),
            apply_to_conversations: false,
            period: default_period(),
            time_zone: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MoneyCeiling {
    pub currency: String,
    pub limit: f64,
}

fn valid_currency(s: &str) -> bool {
    s.len() == 3 && s.bytes().all(|b| b.is_ascii_uppercase())
}

/// Tudo é conferido na gravação; valor fora do contrato é recusado inteiro.
fn validate(s: &Settings) -> Result<()> {
    ensure!(matches!(s.warn_percent, 70 | 80 | 90), "invalid_ceiling_warn_percent");
    ensure!(matches!(s.period.as_str(), "month" | "day"), "invalid_ceiling_period");
    if let Some(zone) = &s.time_zone {
        ensure!(zone.parse::<chrono_tz::Tz>().is_ok(), "invalid_ceiling_timezone");
    }
    if let Some(tokens) = s.tokens {
        ensure!(tokens > 0, "invalid_ceiling_tokens");
    }
    for m in &s.money {
        ensure!(valid_currency(&m.currency), "invalid_ceiling_currency");
        ensure!(m.limit.is_finite() && m.limit > 0.0, "invalid_ceiling_limit");
        ensure!(
            s.money.iter().filter(|other| other.currency == m.currency).count() == 1,
            "invalid_ceiling_currency_dup"
        );
    }
    Ok(())
}

fn parse_settings(body: Value) -> Result<Settings> {
    let s: Settings = serde_json::from_value(body).map_err(|e| anyhow!("invalid_ceiling: {e}"))?;
    validate(&s)?;
    Ok(s)
}

pub(crate) fn settings_on(c: &Connection) -> Result<Settings> {
    Ok(read(c, SETTINGS)?.map(|raw| serde_json::from_str(&raw)).transpose()?.unwrap_or_default())
}

/// Política por origem: compactação nunca trava; a conversa do dono só com
/// `applyToConversations`; todo o resto (autônomo) sempre.
pub(crate) fn covers(origin: &str, settings: &Settings) -> bool {
    match origin {
        "compaction" => false,
        "human" => settings.apply_to_conversations,
        _ => true,
    }
}

pub(crate) fn origin_kind(origin: &Origin) -> &'static str {
    match origin {
        Origin::Routine { .. } => "routine",
        Origin::Ask => "ask",
        Origin::Message => "message",
        Origin::Group => "group",
        Origin::Compaction => "compaction",
        Origin::Recovery => "recovery",
        Origin::Human => "human",
    }
}

// MARK: janela do período

/// Janela de contagem do período. `month`: mês civil UTC (o mesmo do relatório
/// de custo), rótulo `YYYY-MM`. `day`: dia civil no fuso do dono, rótulo
/// `YYYY-MM-DD`; início e fim são os instantes das meias-noites locais — dias
/// de 23 ou 25 horas (troca de horário de verão) caem naturalmente.
#[derive(Clone, Debug, Default)]
pub(crate) struct Window {
    /// Rótulo da janela: chave das liberações, da geração do estado e dos
    /// avisos. Mês: `YYYY-MM` (inalterado, UTC). Dia: `YYYY-MM-DD@<iana>` — o
    /// fuso faz parte da identidade da janela (ver `window_on`).
    pub label: String,
    pub start: i64,
    pub end: i64,
    pub time_zone: String,
}

/// Fuso padrão do teto diário: o MESMO do resumo diário — carrega o registro
/// `daily_summary.v1` pelo MESMO tipo tipado do resumo (`resumo_diario::Config`;
/// o que ele não carrega é erro aqui); sem registro, o fuso local do Hub.
/// Erro de leitura ou registro ilegível/IANA inválido é erro (fail-closed),
/// nunca fallback silencioso.
pub(crate) fn default_time_zone(c: &Connection) -> Result<String> {
    default_time_zone_with(c, crate::fork::resumo_diario::fuso_local)
}

/// `local` é injetável para teste (nunca se mexe na variável de ambiente `TZ`).
pub(crate) fn default_time_zone_with(c: &Connection, local: fn() -> String) -> Result<String> {
    let Some(raw) = read(c, "daily_summary.v1")? else { return Ok(local()) };
    let cfg: crate::fork::resumo_diario::Config =
        serde_json::from_str(&raw).map_err(|_| anyhow!("invalid_ceiling_timezone"))?;
    ensure!(cfg.time_zone.parse::<chrono_tz::Tz>().is_ok(), "invalid_ceiling_timezone");
    Ok(cfg.time_zone)
}

fn zone_on(c: &Connection, settings: &Settings) -> Result<(chrono_tz::Tz, String)> {
    let name = match &settings.time_zone {
        Some(zone) => zone.clone(),
        None => default_time_zone(c)?,
    };
    let zone: chrono_tz::Tz = name.parse().map_err(|_| anyhow!("invalid_ceiling_timezone"))?;
    Ok((zone, name))
}

fn local_date(zone: chrono_tz::Tz, t: i64) -> Option<chrono::NaiveDate> {
    Some(chrono::DateTime::from_timestamp_millis(t)?.with_timezone(&zone).date_naive())
}

/// Passo grosso da varredura da fronteira real entre dias locais.
const STEP: i64 = 30 * 60 * 1000;
/// Passo fino da checagem de contiguidade/monotonicidade da data na janela.
const FINE: i64 = 10 * 60 * 1000;

/// Primeiro instante depois de `hint` em que a data local satisfaz `pred`:
/// varredura de 30 min + refinamento binário ao ms. Pesquisa o instante, sem
/// assumir que a meia-noite local existe — um dia civil pode ser pulado no fuso
/// (Pacific/Apia pulou 2011-12-30 inteiro) e a troca pode não cair em 00:00.
/// Fronteira não achada em 8 dias falha fechado (`ceiling_window`).
fn boundary(zone: chrono_tz::Tz, hint: i64, pred: impl Fn(chrono::NaiveDate) -> bool) -> Result<i64> {
    let mut t = hint;
    for _ in 0..8 * 48 {
        if local_date(zone, t).is_some_and(&pred) {
            let (mut lo, mut hi) = (t - STEP, t);
            while hi - lo > 1 {
                let mid = lo + (hi - lo) / 2;
                if local_date(zone, mid).is_some_and(&pred) {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            return Ok(hi);
        }
        t += STEP;
    }
    Err(anyhow!("ceiling_window"))
}

/// A janela [início, fim) que contém `at`, no período configurado. No período
/// diário o rótulo carrega o fuso (`YYYY-MM-DD@<iana>`): trocar o `timeZone`
/// muda a janela EFETIVA — liberações, geração de estado e avisos de outra
/// janela nunca se aplicam, e voltar ao fuso na mesma data reencontra a mesma.
pub(crate) fn window_on(c: &Connection, settings: &Settings, at: i64) -> Result<Window> {
    if settings.period == "day" {
        let (zone, name) = zone_on(c, settings)?;
        let day = local_date(zone, at).ok_or_else(|| anyhow!("ceiling_window"))?;
        // Um ponto do dia civil anterior contíguo a `at` (data < dia, indo para
        // trás), depois a fronteira real seguinte: o início EFETIVO do dia —
        // certo mesmo quando a meia-noite não existe no fuso.
        let mut back = at;
        let prev = loop {
            back -= STEP;
            ensure!(back >= at - 8 * 86_400_000, "ceiling_window");
            if local_date(zone, back).is_some_and(|x| x != day) {
                break back;
            }
        };
        let prev_date = local_date(zone, prev).ok_or_else(|| anyhow!("ceiling_window"))?;
        let start = boundary(zone, prev, move |x| x > prev_date)?;
        ensure!(local_date(zone, start) == Some(day), "ceiling_window");
        let end = boundary(zone, at, move |x| x > day)?;
        // A data local nem sempre avança (ex.: America/Goose_Bay recuou o
        // relógio cruzando a meia-noite em 30/10/1988). Se dentro da janela
        // pesquisada a data não é `day` por inteiro (recuo/repetição), ou se a
        // data ocupa intervalos disjuntos (aparece antes de `start`), a janela
        // NÃO é determinável: falha fechada. A checagem é amostrada: anomalias
        // mais curtas que os passos de 10/30 min podem passar despercebidas.
        let mut t = start;
        while t < end {
            ensure!(local_date(zone, t) == Some(day), "ceiling_window");
            t += FINE;
        }
        let mut t = start;
        // 288 passos de 30 min = 6 dias anteriores ao início.
        for _ in 0..4 * 24 * 3 {
            t -= STEP;
            ensure!(local_date(zone, t) != Some(day), "ceiling_window");
        }
        return Ok(Window { label: format!("{day}@{name}"), start, end, time_zone: name });
    }
    let label = chrono::DateTime::from_timestamp_millis(at).unwrap_or_default().format("%Y-%m").to_string();
    let (label, start, end) = crate::fork::custo::month(Some(&label))?;
    Ok(Window { label, start, end, time_zone: "UTC".into() })
}

// MARK: estado do período

/// Métrica inteira: contagens de token são exatas (a comparação e o aviso nunca
/// passam por f64 — acima de 2^53 a mantissa perderia o passo do limite).
#[derive(Clone, Copy, Debug)]
pub(crate) struct IntMetric {
    pub used: u64,
    pub effective: u64,
}

#[derive(Clone, Debug)]
#[allow(clippy::struct_excessive_bools, reason = "Flags independentes de uma métrica conferida.")]
pub(crate) struct Metric {
    /// `money:USD` ou `tokens`.
    pub id: String,
    pub used: f64,
    pub limit: f64,
    pub effective_limit: f64,
    /// Presente só na métrica `tokens` — os campos f64 dela são só exibição.
    pub int: Option<IntMetric>,
    pub verified: bool,
    /// Preenchido quando `verified == false`: `semPreco`, `tokensNulos` ou `limiteInvalido`.
    pub reason: Option<&'static str>,
    /// O valor conhecido já atingiu o limite efetivo (piso: parcial também trava por valor).
    pub reached: bool,
    /// Conhecido >= warnPercent do limite efetivo.
    pub warn: bool,
}

/// Estado observado + decisão. `fingerprint` assina (mês, métricas que atingiram,
/// métricas não conferidas), `{mês}|off` quando desligado, ou `readFailure:{erro}`
/// quando a leitura falhou. `signature` acrescenta a geração COMPROMETIDA daquela
/// impressão (`fp#n`): checkFailure vale só para a geração viva; a mesma impressão
/// de volta numa geração nova exige decisão nova.
#[derive(Clone, Debug)]
pub(crate) struct Gate {
    /// Rótulo da janela vigente (`YYYY-MM` no mês, `YYYY-MM-DD` no dia): chave
    /// de liberações, geração e avisos. O nome `month` é histórico.
    pub month: String,
    /// `month` ou `day`, como configurado.
    pub period: String,
    /// [início, fim) da janela vigente, em ms epoch.
    pub start: i64,
    pub end: i64,
    /// `UTC` no período mensal; o fuso IANA do dono no diário.
    pub time_zone: String,
    pub metrics: Vec<Metric>,
    pub blocked: Option<Blocked>,
    pub fingerprint: String,
    pub signature: String,
    /// Métricas no/depois do aviso cujo marcador do mês ainda não existe.
    pub warn_pending: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Blocked {
    /// `valueReached` ou `unverifiable`.
    pub reason: &'static str,
    pub metrics: Vec<String>,
}

struct ReleaseRow {
    kind: String,
    metric: String,
    /// checkFailure: assinatura `fingerprint#geração` liberada. Liberação de valor:
    /// o novo limite exato escolhido (texto — tokens além de 2^53 sem perda).
    state: Option<String>,
}

/// Liberações de valor ("Continuar mesmo assim" com novo limite, MX-160): o dono
/// escolhe +25%, +50% ou um limite livre; cada uma grava o novo limite exato.
const VALUE_RELEASES: [&str; 3] = ["plus25", "plus50", "setLimit"];

fn releases_on(c: &Connection, month: &str) -> Result<Vec<ReleaseRow>> {
    let mut stmt = c.prepare(&format!("SELECT kind,metric,state FROM {TABLE} WHERE month=?1 ORDER BY rowid"))?;
    let rows = stmt
        .query_map([month], |r| Ok(ReleaseRow { kind: r.get(0)?, metric: r.get(1)?, state: r.get(2)? }))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn warned_key(month: &str) -> String {
    format!("donna.ceiling.warned.{month}")
}
fn warned_marker(metric: &str, effective_limit: &str) -> String {
    format!("{metric}:{effective_limit}")
}

fn state_key(month: &str) -> String {
    format!("donna.ceiling.state.{month}")
}

fn stored_state(c: &Connection, month: &str) -> Result<(String, u64)> {
    let Some(raw) = read(c, &state_key(month))? else { return Ok((String::new(), 0)) };
    let value: Value = serde_json::from_str(&raw)?;
    Ok((value["fingerprint"].as_str().unwrap_or_default().to_string(), value["generation"].as_u64().unwrap_or(0)))
}

/// Compromete a geração da impressão observada: mesma impressão conserva o
/// número; impressão nova (incluindo a volta de uma já vista) incrementa.
pub(crate) fn observe_state(c: &Connection, month: &str, fingerprint: &str) -> Result<u64> {
    let (stored, generation) = stored_state(c, month)?;
    if stored == fingerprint {
        return Ok(generation.max(1));
    }
    let generation = generation + 1;
    write(c, &state_key(month), &json!({"fingerprint": fingerprint, "generation": generation}))?;
    Ok(generation)
}

/// Mudança de configuração é transição: a próxima observação, qualquer que seja,
/// começa geração nova — desligar e religar nunca ressuscita uma checkFailure.
fn invalidate_state(c: &Connection, settings: &Settings) -> Result<()> {
    let label = window_on(c, settings, crate::store::now_ms())?.label;
    let (_, generation) = stored_state(c, &label)?;
    write(c, &state_key(&label), &json!({"fingerprint": "", "generation": generation + 1}))
}

/// Avalia configuração + ledger do mês civil UTC e COMPROMETE a geração do
/// estado observado. Falha de leitura invalida a decisão: quem chama usa
/// [`evaluate_on`], que dá identidade liberável a esse estado (`failure_gate`).
pub(crate) fn gate_on(c: &Connection, settings: &Settings) -> Result<Gate> {
    let window = window_on(c, settings, crate::store::now_ms())?;
    let month = window.label;
    let (start, end) = (window.start, window.end);
    let mut gate = Gate {
        month: month.clone(),
        period: settings.period.clone(),
        start,
        end,
        time_zone: window.time_zone,
        metrics: Vec::new(),
        blocked: None,
        fingerprint: String::new(),
        signature: String::new(),
        warn_pending: Vec::new(),
    };
    if !settings.enabled || (settings.money.is_empty() && settings.tokens.is_none()) {
        // Desligado também é um estado observado: religar começa geração nova.
        gate.fingerprint = format!("{month}|off");
        gate.signature = format!("{}#{}", gate.fingerprint, observe_state(c, &month, &gate.fingerprint)?);
        return Ok(gate);
    }
    let releases = releases_on(c, &month)?;
    let (unpriced, tokens_null, tokens_used): (i64, i64, i64) = c.query_row(
        "SELECT COALESCE(SUM(CASE WHEN amount IS NULL OR currency IS NULL THEN 1 ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN input_tokens IS NULL OR output_tokens IS NULL THEN 1 ELSE 0 END),0),
                COALESCE(SUM(COALESCE(input_tokens,0)+COALESCE(output_tokens,0)),0)
         FROM donna_turn_usage WHERE at>=?1 AND at<?2",
        params![start, end],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let mut money_used = std::collections::HashMap::<String, f64>::new();
    let mut stmt =
        c.prepare("SELECT currency, SUM(amount) FROM donna_turn_usage WHERE at>=?1 AND at<?2 AND amount IS NOT NULL GROUP BY currency")?;
    let sums = stmt
        .query_map(params![start, end], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (currency, used) in sums {
        money_used.insert(currency, used);
    }
    let mut metrics = Vec::new();
    for ceiling in &settings.money {
        let id = format!("money:{}", ceiling.currency);
        metrics.push(metric_state(
            id,
            money_used.get(&ceiling.currency).copied().unwrap_or(0.0),
            ceiling.limit,
            unpriced == 0,
            if unpriced == 0 { None } else { Some("semPreco") },
            settings.warn_percent,
            &releases,
        ));
    }
    if let Some(limit) = settings.tokens {
        metrics.push(int_metric_state(
            "tokens".into(),
            tokens_used.max(0).unsigned_abs(),
            limit,
            tokens_null == 0,
            if tokens_null == 0 { None } else { Some("tokensNulos") },
            settings.warn_percent,
            &releases,
        ));
    }
    // A assinatura é do estado OBSERVADO (conhecido >= limite efetivo; não conferido):
    // liberações de valor não a mudam; checkFailure expira quando ela muda.
    let observed_reached: Vec<String> = metrics.iter().filter(|m| m.reached).map(|m| m.id.clone()).collect();
    let unverified: Vec<String> = metrics.iter().filter(|m| !m.verified).map(|m| m.id.clone()).collect();
    gate.fingerprint = fingerprint(&month, &observed_reached, &unverified);
    gate.signature = format!("{}#{}", gate.fingerprint, observe_state(c, &month, &gate.fingerprint)?);
    let blocking: Vec<String> = metrics.iter().filter(|m| m.reached).map(|m| m.id.clone()).collect();
    // checkFailure vale só para a geração do estado que existia quando foi gravada.
    let check_failure_applies =
        releases.iter().any(|r| r.kind == "checkFailure" && r.state.as_deref() == Some(gate.signature.as_str()));
    gate.blocked = if !blocking.is_empty() {
        Some(Blocked { reason: REASON_VALUE, metrics: blocking })
    } else if !unverified.is_empty() && !check_failure_applies {
        Some(Blocked { reason: REASON_UNVERIFIED, metrics: unverified })
    } else {
        None
    };
    let warned: Vec<String> = {
        let raw = read(c, &warned_key(&month))?;
        serde_json::from_str::<Vec<String>>(&raw.unwrap_or_else(|| "[]".into()))?
    };
    for m in &metrics {
        let effective = m.int.map_or(m.effective_limit.to_string(), |i| i.effective.to_string());
        let marker = warned_marker(&m.id, &effective);
        if m.warn && !warned.contains(&marker) {
            gate.warn_pending.push(marker);
        }
    }
    gate.metrics = metrics;
    Ok(gate)
}

/// Sonda O(1) da geração do que decide o teto — mês, contador de escritas do
/// ledger (gatilho), liberações e configuração crus. Idêntica => a decisão
/// parada estável não precisa reagregar o ledger; diferente => reavaliamos. O
/// ator ainda força a avaliação completa periodicamente (estado de geração
/// mudado por outro caminho só pode manter a espera, nunca abri-la).
pub(crate) fn probe_on(c: &Connection) -> Result<String> {
    let ledger: i64 = c.query_row("SELECT n FROM donna_ceiling_ledger_gen WHERE id=1", [], |r| r.get(0))?;
    let rel: i64 = c.query_row(&format!("SELECT COALESCE(MAX(rowid),0) FROM {TABLE}"), [], |r| r.get(0))?;
    let raw = read(c, SETTINGS)?.unwrap_or_default();
    let settings: Settings = serde_json::from_str(&raw).unwrap_or_default();
    let window = window_label_on(c, &settings, crate::store::now_ms());
    Ok(format!("{window}|{ledger}|{rel}|{raw}"))
}

/// Identidade do estado de falha de leitura: liberável só por checkFailure e
/// expira com a geração — falha nova ou estado novo exigem decisão nova.
fn failure_gate(c: &Connection, settings: &Settings, error: &anyhow::Error) -> Result<Gate> {
    failure_gate_at(c, settings, error, crate::store::now_ms())
}

pub(crate) fn failure_gate_at(c: &Connection, settings: &Settings, error: &anyhow::Error, now: i64) -> Result<Gate> {
    let window = window_on(c, settings, now).unwrap_or_default();
    // Sem janela determinável o rótulo ainda precisa expirar: `falha@<dia UTC>`.
    let month = if window.label.is_empty() { failure_label(now) } else { window.label };
    let fingerprint = format!("readFailure:{error:#}");
    let signature = format!("{fingerprint}#{}", observe_state(c, &month, &fingerprint)?);
    let releases = releases_on(c, &month)?;
    let failure_released =
        releases.iter().any(|r| r.kind == "checkFailure" && r.state.as_deref() == Some(signature.as_str()));
    let blocked = (settings.enabled && !failure_released)
        .then(|| Blocked { reason: REASON_UNVERIFIED, metrics: vec!["ledger".into()] });
    Ok(Gate {
        month,
        period: settings.period.clone(),
        start: window.start,
        end: window.end,
        time_zone: window.time_zone,
        metrics: Vec::new(),
        blocked,
        fingerprint,
        signature,
        warn_pending: Vec::new(),
    })
}

/// Rótulo do estado "janela indeterminável" (leitura da janela falhou):
/// verificável e expira na virada do dia UTC — uma liberação `checkFailure`
/// desse estado nunca sobrevive à virada nem autoriza nada depois dela.
fn failure_label(at: i64) -> String {
    let day = chrono::DateTime::from_timestamp_millis(at).unwrap_or_default().date_naive();
    format!("falha@{day}")
}

/// O rótulo vigente para chaves de estado/liberações/avisos — o da janela ou,
/// na falha dela, o rótulo de falha que expira diariamente.
pub(crate) fn window_label_on(c: &Connection, settings: &Settings, at: i64) -> String {
    window_on(c, settings, at).map_or_else(|_| failure_label(at), |w| w.label)
}

/// Resultado de uma conferência: configuração lida, estado efetivo (o MESMO em
/// todo caminho — permit, fundo, ator, painel, liberação) e se algo visível mudou
/// (aviso novo marcado ou espera de fundo apagada) — o chamador emite o evento.
pub(crate) struct Eval {
    pub settings: Settings,
    pub gate: Gate,
    pub changed: bool,
}

/// A conferência única. Compromete a geração (saída saudável, falha, desligado:
/// toda transição invalida checkFailure antiga), marca avisos novos, e apaga as
/// esperas de fundo quando o teto não trava mais (elas são só evidência: o job de
/// fundo tenta de novo no próprio ciclo).
pub(crate) fn evaluate_on(c: &Connection, settings: Settings) -> Result<Eval> {
    let gate = match gate_on(c, &settings) {
        Ok(gate) => gate,
        Err(error) => failure_gate(c, &settings, &error)?,
    };
    let mut changed = mark_warned(c, &gate.month, &gate.warn_pending)?;
    if gate.blocked.is_none() {
        changed |= c.execute(&format!("DELETE FROM {HOLDS} WHERE kind='background'"), [])? > 0;
    }
    Ok(Eval { settings, gate, changed })
}

/// Assinatura do estado observado; a liberação grava `fingerprint#geração`.
fn fingerprint(month: &str, reached: &[String], unverified: &[String]) -> String {
    format!("{month}|v:{}|u:{}", reached.join(","), unverified.join(","))
}

/// Limite efetivo do mês: o maior entre o teto configurado e os limites que o
/// dono escolheu nas liberações de valor deste mês (`state` guarda o valor exato).
/// Tipo antigo/desconhecido ou valor ilegível nunca sobe o limite (fail-closed).
fn effective_limit(limit: f64, metric: &str, releases: &[ReleaseRow]) -> f64 {
    released(metric, releases).filter_map(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite()).fold(limit, f64::max)
}

/// Mesmo critério em inteiro exato para tokens.
fn effective_tokens(limit: u64, metric: &str, releases: &[ReleaseRow]) -> u64 {
    released(metric, releases).filter_map(|v| v.parse::<u64>().ok()).fold(limit, u64::max)
}

fn released<'a>(metric: &'a str, releases: &'a [ReleaseRow]) -> impl Iterator<Item = &'a str> {
    releases
        .iter()
        .filter(move |r| r.metric == metric && VALUE_RELEASES.contains(&r.kind.as_str()))
        .filter_map(|r| r.state.as_deref())
}

/// Atalho sobre o limite vigente ("teto anterior" × 1,25 ou × 1,5).
fn shortcut_money(kind: &str, effective: f64) -> f64 {
    if kind == "plus25" { effective * 1.25 } else { effective * 1.5 }
}

/// O mesmo atalho em inteiro, arredondando para cima; `None` se estoura u64.
fn shortcut_tokens(kind: &str, effective: u64) -> Option<u64> {
    effective.checked_add(if kind == "plus25" { effective.div_ceil(4) } else { effective.div_ceil(2) })
}

fn metric_state(
    id: String,
    used: f64,
    limit: f64,
    verified: bool,
    reason: Option<&'static str>,
    warn_percent: u8,
    releases: &[ReleaseRow],
) -> Metric {
    let effective = effective_limit(limit, &id, releases);
    // Limite efetivo não finito nunca seria atingido: fecha como não conferido.
    let finite = effective.is_finite();
    Metric {
        id,
        used,
        limit,
        effective_limit: effective,
        int: None,
        verified: verified && finite,
        reason: if finite { reason } else { Some("limiteInvalido") },
        reached: used >= effective,
        warn: used >= effective * f64::from(warn_percent) / 100.0,
    }
}

/// Caminho inteiro da métrica `tokens`: alcance, aviso e limite efetivo sem
/// passar por ponto flutuante (precisão garantida além de 2^53).
fn int_metric_state(
    id: String,
    used: u64,
    limit: u64,
    verified: bool,
    reason: Option<&'static str>,
    warn_percent: u8,
    releases: &[ReleaseRow],
) -> Metric {
    let effective = effective_tokens(limit, &id, releases);
    let reached = used >= effective;
    // u128 evita o estouro de used*100 perto de u64::MAX.
    let warn = u128::from(used) * 100 >= u128::from(effective) * u128::from(warn_percent);
    #[allow(clippy::cast_precision_loss, reason = "Só exibição: decisão e aviso usam os inteiros.")]
    let metric = Metric {
        id,
        used: used as f64,
        limit: limit as f64,
        effective_limit: effective as f64,
        int: Some(IntMetric { used, effective }),
        verified,
        reason,
        reached,
        warn,
    };
    metric
}

// MARK: esperas duráveis e avisos

/// Identidade do trabalho travado, derivada do corpo da espera: o mesmo trabalho
/// travando de novo (retry, re-drive do agendador após restart) cai na MESMA
/// linha — nunca duplica nem sobrescreve outro trabalho do bot.
pub(crate) fn work_key(body: &Value) -> String {
    let detail = &body["originDetail"];
    let attempt = || format!("attempt:{}", body["attempt"].as_str().unwrap_or_default());
    match body["origin"].as_str().unwrap_or_default() {
        "routine" => detail["routineId"].as_str().map_or_else(attempt, |id| format!("routine:{id}")),
        "ask" | "message" => detail["requestId"].as_str().map_or_else(attempt, |id| format!("request:{id}")),
        "group" => match (detail["chat"].as_str(), detail["epoch"].as_u64()) {
            (Some(chat), Some(epoch)) => format!("group:{chat}:{epoch}"),
            _ => attempt(),
        },
        "recovery" => format!(
            "recovery:{}:{}",
            body["lane"]["chat"].as_str().unwrap_or_default(),
            body["lane"]["session"].as_str().unwrap_or_default()
        ),
        _ => {
            let entries: Vec<&str> = body["setup"]["entries"]
                .as_array()
                .map(|v| v.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            if entries.is_empty() { attempt() } else { format!("human:{}", entries.join(",")) }
        }
    }
}

/// Uma espera durável de turno, na ordem de chegada.
pub(crate) struct HoldRow {
    pub work: String,
    pub body: Value,
}

/// Grava a espera de um trabalho. O instante `at` (e a linha, com sua ordem) é
/// preservado enquanto a assinatura (razão + estado) não muda — retentativa nunca
/// muda a marca nem volta a emitir evento. Devolve se o conteúdo mudou.
pub(crate) fn write_hold(c: &Connection, bot: &str, work: &str, kind: &str, hold: &Value) -> Result<bool> {
    let current = read_hold(c, bot, work)?;
    let mut hold = hold.clone();
    hold["work"] = json!(work);
    // Marca de cancelamento/liquidação de versão anterior nunca é perdida numa regravação.
    if let Some(saved) = &current {
        for flag in ["cancelling", "settled"] {
            if saved[flag] == true {
                hold[flag] = json!(true);
            }
        }
    }
    if let Some(saved) = &current
        && saved["signature"] == hold["signature"]
        && saved["reason"] == hold["reason"]
    {
        hold["at"] = saved["at"].clone();
    }
    if current.as_ref() == Some(&hold) {
        return Ok(false);
    }
    c.execute(
        &format!(
            "INSERT INTO {HOLDS}(bot_id,work,kind,body) VALUES(?1,?2,?3,?4)
             ON CONFLICT(bot_id,work) DO UPDATE SET kind=excluded.kind, body=excluded.body"
        ),
        params![bot, work, kind, hold.to_string()],
    )?;
    Ok(true)
}

pub(crate) fn read_hold(c: &Connection, bot: &str, work: &str) -> Result<Option<Value>> {
    let raw: Option<String> = c
        .query_row(&format!("SELECT body FROM {HOLDS} WHERE bot_id=?1 AND work=?2"), [bot, work], |r| r.get(0))
        .optional()?;
    Ok(raw.map(|raw| serde_json::from_str(&raw)).transpose()?)
}

/// Reescreve a espera de um trabalho que mudou de identidade (lote humano que
/// perdeu uma entrada) NO LUGAR: a linha conserva `seq`, logo a ordem de chegada.
pub(crate) fn rework_hold(c: &Connection, bot: &str, work: &str, next: &str, body: &Value) -> Result<()> {
    let changed = c.execute(
        &format!("UPDATE {HOLDS} SET work=?3, body=?4 WHERE bot_id=?1 AND work=?2"),
        params![bot, work, next, body.to_string()],
    )?;
    ensure!(changed == 1, "ceiling_hold_missing");
    Ok(())
}

/// Grava a intenção do dono de cancelar este trabalho parado (idempotente).
pub(crate) fn record_cancel(c: &Connection, bot: &str, work: &str) -> Result<()> {
    c.execute(
        &format!("INSERT OR IGNORE INTO {CANCELS}(bot_id,work,at) VALUES(?1,?2,?3)"),
        params![bot, work, crate::store::now_ms()],
    )?;
    Ok(())
}

/// O dono cancelou este trabalho e o efeito ainda não foi confirmado com a
/// saída da linha: intenção gravada, ou marca `cancelling`/`settled` na linha
/// (formato da versão anterior).
pub(crate) fn cancelled(c: &Connection, bot: &str, work: &str) -> Result<bool> {
    let intent: bool = c.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM {CANCELS} WHERE bot_id=?1 AND work=?2)"),
        [bot, work],
        |r| r.get(0),
    )?;
    Ok(intent || read_hold(c, bot, work)?.is_some_and(|body| body["cancelling"] == true || body["settled"] == true))
}

/// Apaga a espera de UM trabalho; as demais do bot ficam.
pub(crate) fn clear_hold(c: &Connection, bot: &str, work: &str) -> Result<bool> {
    Ok(c.execute(&format!("DELETE FROM {HOLDS} WHERE bot_id=?1 AND work=?2"), [bot, work])? > 0)
}

/// Esperas de turno do bot, na ordem em que travaram (fonte do restore).
pub(crate) fn turn_holds_of(c: &Connection, bot: &str) -> Result<Vec<HoldRow>> {
    let mut stmt = c.prepare(&format!("SELECT work,body FROM {HOLDS} WHERE bot_id=?1 AND kind='turn' ORDER BY seq"))?;
    let rows = stmt.query_map([bot], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut holds = Vec::new();
    for row in rows {
        let (work, raw) = row?;
        match serde_json::from_str(&raw) {
            Ok(body) => holds.push(HoldRow { work, body }),
            // Ilegível fica na tabela (evidência) e aparece no painel; não vira trabalho.
            Err(error) => tracing::warn!(%error, work, "espera do teto ilegível; preservada"),
        }
    }
    Ok(holds)
}

/// Marca os avisos novos do mês; devolve se gravou algo (o app relê uma vez).
pub(crate) fn mark_warned(c: &Connection, month: &str, markers: &[String]) -> Result<bool> {
    if markers.is_empty() {
        return Ok(false);
    }
    let key = warned_key(month);
    let mut warned: Vec<String> = serde_json::from_str(&read(c, &key)?.unwrap_or_else(|| "[]".into()))?;
    let mut changed = false;
    for marker in markers {
        if !warned.iter().any(|w| w == marker) {
            warned.push(marker.clone());
            changed = true;
        }
    }
    if changed {
        write(c, &key, &json!(warned))?;
    }
    Ok(changed)
}

/// Todas as esperas vivas (turno e fundo), com ou sem ator: a linha vale por si
/// só (a reserva original já foi apagada na Tx que a criou).
pub(crate) fn holds_on(c: &Connection) -> Result<Vec<Value>> {
    let mut stmt = c.prepare(&format!("SELECT bot_id,work,kind,body FROM {HOLDS} ORDER BY seq"))?;
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
    })?;
    let mut holds = Vec::new();
    for row in rows {
        let (bot, work, kind, raw) = row?;
        let mut mark: Value = serde_json::from_str(&raw).unwrap_or_else(|error| {
            tracing::warn!(%error, work, "espera do teto ilegível; listada sem detalhe");
            json!({"unreadable": true})
        });
        mark["botId"] = json!(bot);
        mark["work"] = json!(work);
        mark["kind"] = json!(kind);
        holds.push(mark);
    }
    Ok(holds)
}

fn stopped_bots(c: &Connection) -> Result<Vec<String>> {
    let mut stmt = c.prepare(&format!("SELECT DISTINCT bot_id FROM {HOLDS} WHERE kind='turn'"))?;
    let bots = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(bots)
}

// MARK: decisão dentro da transação do permit

/// Resultado da conferência atômica: o despacho segue ou vira espera durável.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    Pass,
    Hold,
}

/// Veredito + se algo visível mudou na mesma Tx (espera nova/alterada, aviso novo,
/// espera de fundo apagada): o ator emite `{"type":"ceiling"}` só nesse caso.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Decision {
    pub verdict: Verdict,
    pub changed: bool,
}

/// A conferência do teto dentro da Tx do permit: mesma leitura que autoriza o
/// despacho grava a espera e apaga a reserva — nenhum gasto ou mudança de teto
/// entra entre a decisão e a autorização. `hold_body` traz os dados do trabalho
/// estacionado (origem, lane e insumos do setup) para a retomada.
pub(crate) fn decide_on(c: &Connection, bot: &str, origin: &Origin, hold_body: &Value) -> Result<Decision> {
    let pass = |changed| Ok(Decision { verdict: Verdict::Pass, changed });
    let kind = origin_kind(origin);
    if kind == "compaction" {
        return pass(false);
    }
    let work = work_key(hold_body);
    // Trabalho que o dono cancelou: nenhuma captura nova apaga a linha (teto
    // aberto) nem a sobrescreve (teto fechado) — fica parado até o efeito valer.
    if cancelled(c, bot, &work)? {
        crate::fork::bot_admission::drop_inflight(c, bot)?;
        return Ok(Decision { verdict: Verdict::Hold, changed: false });
    }
    let settings = settings_on(c)?;
    if !covers(kind, &settings) {
        return pass(false);
    }
    let eval = evaluate_on(c, settings)?;
    let Some(blocked) = eval.gate.blocked.as_ref() else {
        // Saudável: uma marca velha do MESMO trabalho não sobrevive à passagem.
        let cleared = clear_hold(c, bot, &work)?;
        return pass(eval.changed || cleared);
    };
    let mut hold = hold_body.clone();
    hold["reason"] = json!(blocked.reason);
    hold["metrics"] = json!(blocked.metrics);
    hold["signature"] = json!(eval.gate.signature);
    hold["at"] = json!(crate::store::now_ms());
    let written = write_hold(c, bot, &work, "turn", &hold)?;
    crate::fork::bot_admission::drop_inflight(c, bot)?;
    Ok(Decision { verdict: Verdict::Hold, changed: eval.changed || written })
}

/// Mesma decisão fail-closed na fronteira dos envios de fundo (títulos,
/// consolidação de memória, resumo diário): trabalho autônomo não passa enquanto
/// o teto trava ou não pode ser conferido. `Err` nunca envia. Emite o evento só
/// quando a espera/aviso mudou de fato.
pub(crate) fn background_gate(hub: &Hub, bot: &str, job: &str) -> Result<()> {
    background_boundary(hub, bot, job, |_| Ok(()))
}

/// Fronteira da conversa do dono na Open: sem fila de despacho, o turno confere o teto aqui, antes de abrir o Codex.
/// Só vale com `applyToConversations`; teto atingido ou não conferido (`Err`) recusa o turno, e nada fica estacionado.
pub(crate) fn conversation_gate(hub: &Hub) -> Result<()> {
    let (changed, blocked) = {
        let db = hub.store.db.locked();
        let tx = db.unchecked_transaction()?;
        let settings = settings_on(&tx)?;
        if !covers("human", &settings) {
            return Ok(());
        }
        let eval = evaluate_on(&tx, settings)?;
        tx.commit()?;
        (eval.changed, eval.gate.blocked.is_some())
    };
    if changed {
        emit(hub);
    }
    ensure!(!blocked, "open_chat_ceiling");
    Ok(())
}

pub(crate) fn background_boundary<T>(
    hub: &Hub,
    bot: &str,
    job: &str,
    authorize: impl FnOnce(&Connection) -> Result<T>,
) -> Result<T> {
    let (changed, result) = {
        let db = hub.store.db.locked();
        let tx = db.unchecked_transaction()?;
        let eval = evaluate_on(&tx, settings_on(&tx)?)?;
        let outcome = match eval.gate.blocked.as_ref() {
            None => {
                tx.execute_batch("SAVEPOINT background_authorization")?;
                let result = authorize(&tx);
                if result.is_err() {
                    tx.execute_batch("ROLLBACK TO background_authorization")?;
                }
                tx.execute_batch("RELEASE background_authorization")?;
                (eval.changed, result)
            }
            Some(blocked) => {
                let hold = json!({"job":job,"reason":blocked.reason,"metrics":blocked.metrics,
                    "signature":eval.gate.signature,"at":crate::store::now_ms()});
                let written = write_hold(&tx, bot, &format!("bg:{job}"), "background", &hold)?;
                (eval.changed || written, Err(anyhow!("ceiling_held:{job}")))
            }
        };
        tx.commit()?;
        outcome
    };
    if changed {
        emit(hub);
    }
    result
}

// MARK: RPC

fn actor_id(caller: &Caller) -> String {
    match caller {
        Caller::Device { key, .. } => key.clone(),
        _ => "local".into(),
    }
}

/// O app relê quando recebe `{"type":"ceiling"}`; estado novo sai por evento, não por poll.
pub(crate) fn emit(hub: &Hub) {
    let _ = hub.events.send(json!({"type":"ceiling"}));
}

/// O que cada atalho daria nesta métrica e se o Hub o aceitaria: o app desliga o
/// atalho recusado (novo limite <= gasto conhecido) sem refazer a regra.
fn shortcut_json(metric: &Metric, kind: &str) -> Value {
    match value_release(metric, kind, None) {
        Ok((after, _)) => json!({"limitAfter": after, "allowed": true}),
        Err(error) => json!({"limitAfter": Value::Null, "allowed": false, "reason": error.to_string()}),
    }
}

fn gate_json(gate: &Gate) -> Value {
    let metrics: Vec<Value> = gate
        .metrics
        .iter()
        .map(|m| {
            json!({"id":m.id,"used":m.used,"limit":m.limit,"effectiveLimit":m.effective_limit,
                "verified":m.verified,"unverifiedReason":m.reason,"reached":m.reached,"warn":m.warn,
                "shortcuts":{"plus25":shortcut_json(m, "plus25"),"plus50":shortcut_json(m, "plus50")}})
        })
        .collect();
    let blocked = gate.blocked.as_ref().map_or(
        Value::Null,
        |b| json!({"kind":b.reason,"metrics":b.metrics,"fingerprint":gate.fingerprint,"signature":gate.signature}),
    );
    json!({"month":gate.month,"period":gate.period,
        "window":{"start":gate.start,"end":gate.end,"timeZone":gate.time_zone},
        "metrics":metrics,"blocked":blocked,
        "warn":gate.metrics.iter().filter(|m| m.warn).map(|m| m.id.clone()).collect::<Vec<_>>()})
}

/// Painel: o MESMO estado efetivo do gate (falha de leitura inclusa, com a
/// checkFailure dela). Devolve também se a conferência mudou algo visível.
fn read_state(hub: &Hub) -> Result<Value> {
    let (state, changed) = {
        let db = hub.store.db.locked();
        let tx = db.unchecked_transaction()?;
        let eval = evaluate_on(&tx, settings_on(&tx)?)?;
        let holds = holds_on(&tx)?;
        let mut stmt = tx.prepare(&format!(
            "SELECT id,at,actor,kind,month,metric,limit_before,limit_after,overage_known,scope FROM {TABLE} WHERE month=?1 ORDER BY rowid",
        ))?;
        let releases = stmt
            .query_map([&eval.gate.month], |r| {
                Ok(json!({"id":r.get::<_,String>(0)?,"at":r.get::<_,i64>(1)?,"actor":r.get::<_,String>(2)?,
                    "kind":r.get::<_,String>(3)?,"month":r.get::<_,String>(4)?,"metric":r.get::<_,String>(5)?,
                    "limitBefore":r.get::<_,Option<f64>>(6)?,"limitAfter":r.get::<_,Option<f64>>(7)?,
                    "overageKnown":r.get::<_,Option<f64>>(8)?,"scope":r.get::<_,String>(9)?}))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        let state = json!({"config":serde_json::to_value(&eval.settings)?,"state":gate_json(&eval.gate),
            "stopped":holds,"releases":releases});
        tx.commit()?;
        (state, eval.changed)
    };
    if changed {
        emit(hub);
    }
    Ok(state)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReleaseRequest {
    kind: String,
    alvo: String,
    /// Só em `setLimit`: o novo limite escolhido (dinheiro na moeda da métrica; tokens inteiro).
    #[serde(default)]
    new_limit: Option<serde_json::Number>,
}

/// Novo limite de uma liberação de valor sobre a métrica atingida: `plus25`/`plus50`
/// multiplicam o limite vigente; `setLimit` usa o valor do dono. Todo novo limite
/// precisa ser finito/representável, maior que o gasto conhecido e maior que o
/// limite vigente — senão a liberação é recusada inteira. Devolve o valor para
/// exibição/auditoria e o valor exato que vai para `state`.
fn value_release(metric: &Metric, kind: &str, new_limit: Option<&serde_json::Number>) -> Result<(f64, String)> {
    if let Some(int) = metric.int {
        let after = match kind {
            "setLimit" => new_limit
                .and_then(serde_json::Number::as_u64)
                .ok_or_else(|| anyhow!("invalid_ceiling_release_limit"))?,
            _ => shortcut_tokens(kind, int.effective).ok_or_else(|| anyhow!("ceiling_release_overflow"))?,
        };
        ensure!(after > int.used, "ceiling_release_below_spend");
        ensure!(after > int.effective, "ceiling_release_below_limit");
        #[allow(clippy::cast_precision_loss, reason = "Só exibição/auditoria; o exato vai em texto.")]
        return Ok((after as f64, after.to_string()));
    }
    let after = match kind {
        "setLimit" => {
            new_limit.and_then(serde_json::Number::as_f64).ok_or_else(|| anyhow!("invalid_ceiling_release_limit"))?
        }
        _ => shortcut_money(kind, metric.effective_limit),
    };
    ensure!(after.is_finite(), "ceiling_release_overflow");
    ensure!(after > metric.used, "ceiling_release_below_spend");
    ensure!(after > metric.effective_limit, "ceiling_release_below_limit");
    Ok((after, after.to_string()))
}

/// Grava a liberação só quando o estado EFETIVO em Tx (o mesmo do gate, falha de
/// leitura inclusa) ainda está bloqueado no alvo pedido: nenhuma liberação é
/// cheque em branco, nem repetida sobre estado já liberado. Devolve a linha
/// gravada e os bots em espera.
fn release_on(tx: &Connection, actor: &str, request: &ReleaseRequest) -> Result<Value> {
    ensure!(
        matches!(request.kind.as_str(), "plus25" | "plus50" | "setLimit" | "checkFailure"),
        "invalid_ceiling_release"
    );
    // O novo limite livre só existe em `setLimit` — e lá é obrigatório.
    ensure!(request.new_limit.is_some() == (request.kind == "setLimit"), "invalid_ceiling_release");
    let settings = settings_on(tx)?;
    ensure!(settings.enabled, "ceiling_not_blocked");
    let scope = if settings.apply_to_conversations { "all" } else { "autonomous" };
    let eval = evaluate_on(tx, settings)?;
    let gate = &eval.gate;
    let blocked = gate.blocked.as_ref().ok_or_else(|| anyhow!("ceiling_not_blocked"))?;
    let (metric, limit_before, limit_after, overage, state) = match request.kind.as_str() {
        "plus25" | "plus50" | "setLimit" => {
            ensure!(blocked.reason == REASON_VALUE, "ceiling_not_blocked");
            let metric = gate
                .metrics
                .iter()
                .find(|m| m.id == request.alvo && m.reached)
                .ok_or_else(|| anyhow!("ceiling_release_target"))?;
            let (after, exact) = value_release(metric, &request.kind, request.new_limit.as_ref())?;
            let overage = match metric.int {
                #[allow(clippy::cast_precision_loss, reason = "Só auditoria/exibição do excedente inteiro.")]
                Some(int) => int.used.saturating_sub(int.effective) as f64,
                None => (metric.used - metric.effective_limit).max(0.0),
            };
            (metric.id.clone(), Some(metric.effective_limit), Some(after), Some(overage), Some(exact))
        }
        _ => {
            // Vale para não conferido e para falha de leitura (`readFailure:{erro}`).
            // O alvo é a assinatura COMPLETA que o dono viu (`fingerprint#geração`,
            // `blocked.signature` de donnaCeiling), conferida nesta mesma Tx contra a
            // atual: confirmação atrasada/repetida de uma geração já superada (F#1 ->
            // saudável -> F#3) nunca libera a geração nova — o app relê e pergunta.
            ensure!(blocked.reason == REASON_UNVERIFIED, "ceiling_not_blocked");
            ensure!(request.alvo == gate.signature, "ceiling_state_changed");
            (blocked.metrics.join(","), None, None, None, Some(gate.signature.clone()))
        }
    };
    let id = uuid::Uuid::new_v4().to_string();
    let at = crate::store::now_ms();
    tx.execute(
        &format!(
            "INSERT INTO {TABLE}(id,at,actor,kind,month,metric,limit_before,limit_after,overage_known,scope,state)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)"
        ),
        params![id, at, actor, request.kind, gate.month, metric, limit_before, limit_after, overage, scope, state],
    )?;
    Ok(json!({"release":{"id":id,"at":at,"actor":actor,"kind":request.kind,"month":gate.month,
        "metric":metric,"limitBefore":limit_before,"limitAfter":limit_after,"overageKnown":overage,
        "scope":scope},"stoppedBots":stopped_bots(tx)?}))
}

/// Acorda os atores com espera: liberação ou configuração nova é reavaliada já.
fn wake(hub: &Hub, bots: &[String]) {
    for bot in bots {
        if let Err(error) = hub.send_cmd(bot, crate::agent::bot::Cmd::CeilingChanged) {
            tracing::warn!(%error, "teto mudou; ator retenta no próximo ciclo");
        }
    }
}

#[allow(clippy::unused_async, reason = "Assinatura uniforme do dispatch RPC; o chamador sempre faz await.")]
pub(crate) async fn call(hub: &Arc<Hub>, caller: &Caller, method: &str, body: Value) -> Result<Value> {
    super::bot_editor::authorization::require_owner(caller)?;
    match method {
        "donnaCeiling" => {
            ensure!(body.as_object().is_some_and(serde_json::Map::is_empty), "invalid_ceiling_request");
            read_state(hub)
        }
        "donnaCeilingSave" => {
            let mut settings = parse_settings(body)?;
            // O resumo diário só é consultado quando o período diário precisa
            // dele: mês/legado/desligar nunca depende do registro do resumo.
            if settings.time_zone.is_none() && settings.period == "day" {
                settings.time_zone = Some(default_time_zone(&hub.store.db.locked())?);
            }
            let answer = serde_json::to_value(&settings)?;
            // D368: autoridade original reconferida dentro da transação de gravação.
            let bots = crate::fork::donna_fixa::pedido::gravar(&hub.store, |c| {
                if read(c, SETTINGS)?.as_deref() != Some(answer.to_string().as_str()) {
                    write(c, SETTINGS, &answer)?;
                    invalidate_state(c, &settings)?;
                }
                stopped_bots(c)
            })?;
            wake(hub, &bots);
            emit(hub);
            Ok(answer)
        }
        _ => {
            let request: ReleaseRequest =
                serde_json::from_value(body).map_err(|e| anyhow!("invalid_ceiling_release: {e}"))?;
            let actor = actor_id(caller);
            let mut answer =
                crate::fork::donna_fixa::pedido::gravar(&hub.store, |tx| release_on(tx, &actor, &request))?;
            let bots: Vec<String> = serde_json::from_value(answer["stoppedBots"].take()).unwrap_or_default();
            wake(hub, &bots);
            emit(hub);
            if let Some(object) = answer.as_object_mut() {
                object.remove("stoppedBots");
            }
            answer["state"] = read_state(hub)?["state"].clone();
            Ok(answer)
        }
    }
}

/// Exclusão de bot: as esperas morrem com ele; liberações são auditoria e ficam.
pub(crate) fn on_bot_deleted(c: &Connection, bot: &str) -> Result<()> {
    c.execute(&format!("DELETE FROM {HOLDS} WHERE bot_id=?1"), [bot])?;
    c.execute(&format!("DELETE FROM {CANCELS} WHERE bot_id=?1"), [bot])?;
    Ok(())
}
