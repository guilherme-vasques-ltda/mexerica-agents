//! MX-260: histórico de uso da assinatura POR PROVEDOR (login global, nunca por
//! conta) + RPC só leitura do dono `donnaUsageHistory` com agrupamento e previsão
//! calculados no Hub — o cliente só renderiza.
//!
//! Amostras: no máximo 1 por vaga de 10 min por (provedor, janela), cortadas
//! primeiro em memória e garantidas no banco; janela sem percent finito não
//! grava. Cada linha tem `slot` (instante que ABRIU a vaga — a chave de dedupe)
//! e `at` (instante da ÚLTIMA observação): uma leitura dentro da vaga ATUALIZA a
//! mesma linha — `at`, percent mais novo e `resets_at` quando a leitura o informa
//! (a CLI do Claude não informa reset; a statusline informa — alternar fontes
//! não apaga o reset conhecido nem abre amostra extra). Falha de gravação nunca
//! derruba a leitura de uso (aviso sem dado sensível) e o cache só é marcado
//! APÓS o commit. Retenção: amostras com mais de 31 dias são apagadas ao gravar,
//! no máximo 1 vez/hora e em lote limitado.
//!
//! Agrupamento (decisão): o valor de cada bloco é o percent da ÚLTIMA amostra do
//! bloco — reflete o estado mais recente visto. Eixo UTC em ms, agregado no SQL.
//!
//! Previsão (`forecast`): só sobre as amostras da janela VIGENTE — sufixo
//! cronológico que sempre inclui a última amostra, com resets conhecidos compatíveis
//! e `percent` sem queda olhando para trás (mudança de reset conhecido ou queda
//! marcam reinício). `exhausted: true` SOMENTE quando o último percent
//! OBSERVADO é ≥ 100 (fato, nunca extrapolação da reta); `exhaustsAt` é o
//! instante da primeira amostra ≥ 100 da vigente — nunca no futuro. Um 100%
//! antigo não persiste: se o `resetsAt` vigente já passou, a janela rolou e a
//! resposta é `exhausted:false`/`exhaustsAt:null` até chegar amostra da janela
//! nova. Fora isso a reta só FALA DO FUTURO: exige ≥4 amostras cobrindo ≥30
//! min, taxa positiva e resíduo da ÚLTIMA amostra ≤ 2 pontos percentuais (a
//! reta precisa explicar o dado mais recente; se não explica → null, nunca
//! afirmação). Regressão linear simples percent = a + b·at; `exhaustsAt` no
//! cruzamento com 100%, devolvido somente se `now < exhaustsAt < resetsAt`.

use crate::{
    LockExt,
    api::devices::Caller,
    hub::Hub,
    store::{Store, now_ms},
    usage::Window,
};
use anyhow::{Result, anyhow, bail};
use rusqlite::{Connection, OpenFlags, params};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc};

#[cfg(test)]
mod tests;

pub(crate) const METHOD: &str = "donnaUsageHistory";

/// Limite duro de amostragem: uma vaga de 10 min por (provedor, janela).
const INTERVALO_MS: i64 = 10 * 60 * 1000;
/// Retenção do dono: amostras mais antigas são apagadas.
const RETENCAO_MS: i64 = 31 * 24 * 60 * 60 * 1000;
/// A varredura de retenção roda junto da gravação, no máximo 1 vez/hora.
const LIMPEZA_CADA_MS: i64 = 60 * 60 * 1000;
/// Lote máximo apagado por varredura (o resto sai nas próximas gravações).
const LIMPEZA_LOTE: i64 = 500;
/// Previsão mínima: 4 amostras cobrindo 30 minutos da janela vigente, e a reta
/// tem que errar a última observação por no máximo 2 pontos percentuais.
const PREVISAO_AMOSTRAS: usize = 4;
const PREVISAO_COBERTURA_MS: i64 = 30 * 60 * 1000;
const RESIDUO_MAX_PP: f64 = 2.0;
/// Caps defensivos: janelas reais são ~4 (Claude e Codex 5h/7d); 8 cobre com
/// folga o pior caso e limita o trabalho dentro da transação autorizada. Corte
/// de janelas/blocos nunca é silencioso → `truncated: true`.
const MAX_JANELAS: usize = 8;
const MAX_BUCKETS: usize = 1024;
/// Amostras recentes por janela lidas para resetsAt/previsão (~50h de cobertura).
const MAX_AMOSTRAS: usize = 300;

/// Cache por Store (vive no `Store`, como `before_owner_write`): dedupe em
/// memória do limite de 10 min e horário da última limpeza de retenção.
#[derive(Default)]
pub(crate) struct Cache {
    ultimas: HashMap<(String, String), i64>,
    limpeza: Option<i64>,
}

pub(crate) fn migrate(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_usage_samples(
          provider TEXT NOT NULL, window_id TEXT NOT NULL, slot INTEGER NOT NULL,
          at INTEGER NOT NULL, percent REAL NOT NULL, resets_at INTEGER);
         CREATE INDEX IF NOT EXISTS donna_usage_samples_at ON donna_usage_samples(provider,window_id,at);
         CREATE INDEX IF NOT EXISTS donna_usage_samples_quando ON donna_usage_samples(at);",
    )?;
    // A normalização dos slots alinhados é necessária apenas para instalações
    // intermediárias do PR. O índice marca sua execução para não deslocar vagas
    // legítimas em aberturas posteriores; NULL continua sendo reparado sempre.
    let tx = c.unchecked_transaction()?;
    if tx.prepare("SELECT slot FROM donna_usage_samples LIMIT 0").is_err() {
        tx.execute_batch("ALTER TABLE donna_usage_samples ADD COLUMN slot INTEGER")?;
    }
    let normalizada: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='index' AND name='donna_usage_slots_normalizados')",
        [],
        |r| r.get(0),
    )?;
    tx.execute("UPDATE donna_usage_samples SET slot = at WHERE slot IS NULL", [])?;
    if !normalizada {
        tx.execute("UPDATE donna_usage_samples SET slot = at WHERE slot = at / 600000 * 600000 AND slot <> at", [])?;
        tx.execute_batch("CREATE INDEX IF NOT EXISTS donna_usage_slots_normalizados ON donna_usage_samples(slot)")?;
    }
    tx.commit()?;
    Ok(())
}

/// Gancho dos dois produtores: `merge` em `usage.rs` (Claude; a barra de status
/// chama várias vezes por minuto — o corte rígido mora aqui) e `replace` em
/// `codex_usage` (Codex). Nunca propaga erro.
pub(crate) fn record(hub: &Hub, provider: &str, windows: &[Window]) {
    if let Err(error) = record_at(&hub.store, provider, windows, now_ms()) {
        tracing::warn!(%error, provider, "amostra de uso não gravada");
    }
}

pub(crate) fn record_open_codex(hub: &Hub, account: &str, windows: &[Window]) {
    if !windows.is_empty()
        && account.strip_prefix("ac_").is_some_and(|id| id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()))
        && let Err(error) = record_at(&hub.store, &format!("codex:{account}"), windows, now_ms())
    {
        tracing::warn!(%error, "amostra de cota local não gravada");
    }
}

pub(crate) fn remover_conta(db: &Connection, account: &str) -> Result<()> {
    db.execute("DELETE FROM donna_usage_samples WHERE provider=?1", [format!("codex:{account}")])?;
    db.execute("DELETE FROM donna_turn_usage WHERE account_id=?1", [account])?;
    Ok(())
}

pub(crate) fn invalidar_conta(store: &Store, account: &str) {
    let provider = format!("codex:{account}");
    store.uso_historico.locked().ultimas.retain(|(p, _), _| p != &provider);
}

fn record_at(store: &Store, provider: &str, windows: &[Window], now: i64) -> Result<()> {
    let pendentes: Vec<&Window> = windows.iter().filter(|w| w.percent.is_finite()).collect();
    if pendentes.is_empty() {
        return Ok(());
    }
    let corte = now - INTERVALO_MS;
    let mut db = store.db.locked();
    let tx = db.transaction()?;
    let mut gravadas: Vec<(String, String)> = Vec::new();
    let mut limpou = false;
    {
        let cache = store.uso_historico.locked();
        for w in pendentes {
            let chave = (provider.to_owned(), w.id.clone());
            let recente_cache = cache.ultimas.get(&chave).is_some_and(|&slot| slot > corte);
            // Dedupe pela vaga (slot); linha sem slot migrada vale pela
            // observação — a vaga legada abre em `at` (ver migração).
            let ultimo: Option<i64> = tx.query_row(
                "SELECT MAX(COALESCE(slot,at)) FROM donna_usage_samples
                 WHERE provider=?1 AND window_id=?2",
                params![provider, w.id],
                |r| r.get(0),
            )?;
            if recente_cache || ultimo.is_some_and(|slot| slot > corte) {
                // Dentro da vaga: a mesma linha recebe o percent mais novo e
                // resets_at quando a leitura o informa (a CLI do Claude não
                // informa; a statusline informa). `at` avança com a observação,
                // MAS para no primeiro ≥ 100 — somente se a janela continua a
                // mesma (novo ≥100 e resets_at igual OU o primeiro a chegar:
                // a CLI não informa reset, então NULL→conhecido NÃO é troca
                // de janela): queda ou troca de reset avança `at`. E leitura
                // SEM reset não herda resets_at já vencido da vaga (era da
                // janela rolada) — senão um 100% observado agora responderia
                // exhausted:false pela régua do reset passado.
                tx.execute(
                    "UPDATE donna_usage_samples
                     SET at = CASE WHEN percent >= 100 AND ?4 >= 100
                                    AND (resets_at IS NULL OR resets_at IS COALESCE(?5,resets_at))
                                    AND (resets_at IS NULL OR resets_at > ?3)
                                   THEN at ELSE ?3 END,
                         percent = ?4,
                         resets_at = CASE WHEN ?5 IS NULL AND resets_at <= ?3
                                          THEN NULL ELSE COALESCE(?5,resets_at) END
                     WHERE provider=?1 AND window_id=?2
                       AND rowid = (SELECT rowid FROM donna_usage_samples
                                    WHERE provider=?1 AND window_id=?2
                                    ORDER BY COALESCE(slot,at) DESC, at DESC LIMIT 1)",
                    params![provider, w.id, now, w.percent, w.resets_at],
                )?;
                continue;
            }
            tx.execute(
                "INSERT INTO donna_usage_samples(provider,window_id,slot,at,percent,resets_at)
                 VALUES(?1,?2,?3,?3,?4,?5)",
                params![provider, w.id, now, w.percent, w.resets_at],
            )?;
            gravadas.push(chave);
        }
        if cache.limpeza.is_none_or(|at| now.saturating_sub(at) >= LIMPEZA_CADA_MS) {
            // Lote limitado por varredura; o índice `quando` cobre o filtro `at`.
            tx.execute(
                "DELETE FROM donna_usage_samples
                 WHERE rowid IN (SELECT rowid FROM donna_usage_samples WHERE at < ?1 LIMIT ?2)",
                params![now - RETENCAO_MS, LIMPEZA_LOTE],
            )?;
            limpou = true;
        }
    }
    tx.commit()?;
    // O cache só é marcado depois do commit: rollback não supime novas tentativas.
    let mut cache = store.uso_historico.locked();
    for chave in gravadas {
        cache.ultimas.insert(chave, now);
    }
    if limpou {
        cache.limpeza = Some(now);
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Faixa {
    H24,
    D7,
    D30,
}
impl Faixa {
    fn parse(valor: &str) -> Result<Self> {
        Ok(match valor {
            "24h" => Self::H24,
            "7d" => Self::D7,
            "30d" => Self::D30,
            _ => bail!("invalid_usage_history_range"),
        })
    }
    fn span_ms(self) -> i64 {
        match self {
            Self::H24 => 24 * 60 * 60 * 1000,
            Self::D7 => 7 * 24 * 60 * 60 * 1000,
            Self::D30 => 30 * 24 * 60 * 60 * 1000,
        }
    }
    fn bloco_ms(self) -> i64 {
        match self {
            Self::H24 => 10 * 60 * 1000,
            Self::D7 => 60 * 60 * 1000,
            Self::D30 => 6 * 60 * 60 * 1000,
        }
    }
    fn nome(self) -> &'static str {
        match self {
            Self::H24 => "24h",
            Self::D7 => "7d",
            Self::D30 => "30d",
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Read {
    range: String,
}

/// RPC só leitura do dono, no padrão de `donnaCostReport`. A autoridade original
/// é revalidada na transação do ponto de gravação do dono (`pedido::gravar` →
/// `Autoridade::with_db`), que só captura a IDENTIDADE do arquivo do banco
/// (dev+inode, custo ~stat). A consulta então roda numa conexão própria
/// `READ_ONLY`, fora da transação de escrita e sem segurar o mutex: antes de ler,
/// `IdentidadeBanco::conferir` recheca dev+inode do caminho e
/// `SQLITE_FCNTL_HAS_MOVED` no handle — arquivo trocado nesse intervalo falha
/// fechado (mesmo mecanismo de `store/personal_database.rs`). Banco sem arquivo
/// real (`:memory:` nos testes) cai para a leitura na conexão autorizada.
pub(crate) fn call(hub: &Arc<Hub>, caller: &Caller, body: Value) -> Result<Value> {
    super::bot_editor::authorization::require_owner(caller)?;
    let read: Read = serde_json::from_value(body).map_err(|_| anyhow!("invalid_usage_history_request"))?;
    let faixa = Faixa::parse(&read.range)?;
    let now = now_ms();
    let identidade =
        crate::fork::donna_fixa::pedido::gravar(&hub.store, crate::store::personal_database::identidade_da)?;
    let Some(identidade) = identidade else {
        return crate::fork::donna_fixa::pedido::gravar(&hub.store, |c| report(c, faixa, now));
    };
    let c = Connection::open_with_flags(identidade.path(), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    identidade.conferir(&c)?;
    let tx = c.unchecked_transaction()?;
    let resultado = report(&tx, faixa, now)?;
    tx.commit()?;
    Ok(resultado)
}

#[derive(Clone, Copy)]
struct Amostra {
    at: i64,
    percent: f64,
    resets_at: Option<i64>,
}

/// Sufixo vigente: reset desconhecido não significa troca de janela.
fn vigente(amostras: &[Amostra]) -> &[Amostra] {
    let Some(ultima) = amostras.last() else { return amostras };
    let mut reset = ultima.resets_at;
    let mut indice_reset = reset.map(|_| amostras.len() - 1);
    let mut inicio = amostras.len() - 1;
    for i in (0..amostras.len() - 1).rev() {
        let (atual, proxima) = (amostras[i], amostras[i + 1]);
        if atual.percent > proxima.percent {
            break;
        }
        if let Some(r) = atual.resets_at {
            if r <= ultima.at {
                inicio = amostras.partition_point(|a| a.at < r);
                break;
            }
            if reset.is_some_and(|conhecido| r != conhecido) {
                inicio = indice_reset.unwrap_or(inicio);
                break;
            }
            reset = Some(r);
            indice_reset = Some(i);
        }
        inicio = i;
    }
    &amostras[inicio..]
}

/// Primeiro 100% da vigente, varrendo o período retido pelo índice sem carregar
/// todas as linhas em memória. Reset ausente não identifica outra janela.
fn primeiro_cem(c: &Connection, provider: &str, window: &str, now: i64) -> Result<(Option<i64>, Option<i64>)> {
    let mut stmt = c.prepare(
        "SELECT at,percent,resets_at FROM donna_usage_samples
         WHERE provider=?1 AND window_id=?2 AND at>=?3 ORDER BY at DESC",
    )?;
    let mut rows = stmt.query(params![provider, window, now - RETENCAO_MS])?;
    let Some(row) = rows.next()? else { return Ok((None, None)) };
    let ultima = Amostra { at: row.get(0)?, percent: row.get(1)?, resets_at: row.get(2)? };
    let mut proxima = ultima;
    let mut reset = ultima.resets_at;
    let mut at_reset = reset.map(|_| ultima.at);
    let mut limite = now - RETENCAO_MS;
    while let Some(row) = rows.next()? {
        let atual = Amostra { at: row.get(0)?, percent: row.get(1)?, resets_at: row.get(2)? };
        if atual.percent > proxima.percent {
            limite = proxima.at;
            break;
        }
        if let Some(r) = atual.resets_at {
            if r <= ultima.at {
                limite = r;
                break;
            }
            if reset.is_some_and(|conhecido| r != conhecido) {
                limite = at_reset.unwrap_or(proxima.at);
                break;
            }
            reset = Some(r);
            at_reset = Some(atual.at);
        }
        proxima = atual;
    }
    let primeiro = c.query_row(
        "SELECT MIN(at) FROM donna_usage_samples WHERE provider=?1 AND window_id=?2 AND at>=?3 AND percent>=100",
        params![provider, window, limite],
        |r| r.get(0),
    )?;
    Ok((primeiro, reset))
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Previsao {
    /// Último percent OBSERVADO ≥ 100: `at` = primeira amostra ≥ 100 da vigente.
    Esgotada { at: i64 },
    /// Reta vigente consistente cruza 100% depois de agora e antes do reset.
    Estimada { at: i64 },
    /// Sem dado suficiente, condição não atendida ou reta inconsistente.
    Sem,
}

/// Regras da previsão documentadas no topo do módulo; conservador, sem inventar.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "Amostras ≤ 31 dias; deslocamentos em ms cabem em 2^52 e a truncagem é o arredondamento querido."
)]
fn prever(amostras: &[Amostra], now: i64) -> Previsao {
    let janela = vigente(amostras);
    let Some(ultima) = janela.last() else { return Previsao::Sem };
    // resetsAt vigente já passou: a janela rolou — nada se afirma sobre a nova.
    let resets_at = janela.iter().rev().find_map(|a| a.resets_at);
    if resets_at.is_some_and(|resets| resets <= now) {
        return Previsao::Sem;
    }
    // Esgotada é fato observado (último percent ≥ 100), nunca extrapolação.
    if ultima.percent >= 100.0 {
        let at = janela.iter().find(|a| a.percent >= 100.0).map_or(ultima.at, |a| a.at);
        return Previsao::Esgotada { at };
    }
    if janela.len() < PREVISAO_AMOSTRAS
        || ultima.at - janela.first().map_or(ultima.at, |a| a.at) < PREVISAO_COBERTURA_MS
    {
        return Previsao::Sem;
    }
    // Eixo recentrado na primeira amostra: a regressão sai igual e os deltas ficam exatos em f64.
    let x0 = janela.first().map_or(ultima.at, |a| a.at);
    let n = janela.len() as f64;
    let mx = janela.iter().map(|a| (a.at - x0) as f64).sum::<f64>() / n;
    let my = janela.iter().map(|a| a.percent).sum::<f64>() / n;
    let (mut num, mut den) = (0.0, 0.0);
    for a in janela {
        let x = (a.at - x0) as f64 - mx;
        num += x * (a.percent - my);
        den += x.powi(2);
    }
    if den <= 0.0 {
        return Previsao::Sem;
    }
    let b = num / den;
    if b <= 0.0 {
        return Previsao::Sem;
    }
    let a = my - b * mx;
    // A reta só fala do futuro se explicar o dado mais recente (resíduo ≤ 2pp).
    if (ultima.percent - (a + b * (ultima.at - x0) as f64)).abs() > RESIDUO_MAX_PP {
        return Previsao::Sem;
    }
    let cruza = x0.saturating_add(((100.0 - a) / b).round() as i64);
    match resets_at {
        Some(resets_at) if cruza > now && cruza < resets_at => Previsao::Estimada { at: cruza },
        _ => Previsao::Sem,
    }
}

/// Relatório agregado no SQL, sobre conexão cuja identidade já foi conferida
/// (a autorizada em `:memory:` ou a reabertura `READ_ONLY` verificada): linhas
/// devolvidas ≤ 8 janelas × (1025 blocos + 301 amostras) e cada consulta é
/// varredura de range no índice. Corte de janelas/blocos vira `truncated: true`
/// — nunca silencioso — e a janela afetada recebe `forecast: null`; incerteza
/// real no recorte de amostras idem. `exhausted` só sai de fato observado
/// (último percent ≥ 100) e sobrevive a qualquer recorte.
fn report(c: &Connection, faixa: Faixa, now: i64) -> Result<Value> {
    let inicio = now - faixa.span_ms();
    let bloco = faixa.bloco_ms();
    let mut truncated = false;
    // Janelas do range: +1 só para detectar o corte. O filtro `at` usa `quando`.
    let mut janelas: Vec<(String, String)> = c
        .prepare(
            "SELECT provider,window_id FROM donna_usage_samples INDEXED BY donna_usage_samples_quando
             WHERE at>=?1 GROUP BY provider,window_id ORDER BY provider,window_id LIMIT ?2",
        )?
        .query_map(params![inicio, i64::try_from(MAX_JANELAS + 1).unwrap_or(i64::MAX)], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    if janelas.len() > MAX_JANELAS {
        truncated = true;
        janelas.truncate(MAX_JANELAS);
    }
    // Valor do bloco = percent da amostra com MAX(at) do bloco (coluna bare do
    // agregado do SQLite sai da linha do máximo — comportamento documentado).
    let mut stmt_blocos = c.prepare(
        "SELECT (at/?3)*?3 AS start, percent, MAX(at) FROM donna_usage_samples
         WHERE provider=?1 AND window_id=?2 AND at>=?4 GROUP BY start ORDER BY start LIMIT ?5",
    )?;
    // Amostras recentes da vigente para resetsAt e previsão: +1 detecta corte.
    let mut stmt_amostras = c.prepare(
        "SELECT at,percent,resets_at FROM donna_usage_samples
         WHERE provider=?1 AND window_id=?2 AND at>=?3 ORDER BY at DESC LIMIT ?4",
    )?;
    let mut saida = Vec::with_capacity(janelas.len());
    for (provider, window) in janelas {
        let mut cortada = false;
        let mut buckets: Vec<Value> = stmt_blocos
            .query_map(
                params![provider, window, bloco, inicio, i64::try_from(MAX_BUCKETS + 1).unwrap_or(i64::MAX)],
                |r| Ok(json!({"start":r.get::<_,i64>(0)?,"percent":r.get::<_,f64>(1)?})),
            )?
            .collect::<rusqlite::Result<_>>()?;
        if buckets.len() > MAX_BUCKETS {
            cortada = true;
            truncated = true;
            buckets.truncate(MAX_BUCKETS);
        }
        let mut amostras: Vec<Amostra> = stmt_amostras
            .query_map(params![provider, window, inicio, i64::try_from(MAX_AMOSTRAS + 1).unwrap_or(i64::MAX)], |r| {
                Ok(Amostra { at: r.get(0)?, percent: r.get(1)?, resets_at: r.get(2)? })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let recorte_amostras = amostras.len() > MAX_AMOSTRAS;
        if recorte_amostras {
            amostras.truncate(MAX_AMOSTRAS);
        }
        amostras.reverse();
        // Incerteza real: o recorte esconde amostras mais antigas, a vigência
        // alcança a borda (o sufixo pode continuar fora de vista) E o que se vê
        // não basta para nenhuma conclusão → `forecast: null`, nunca silencioso.
        let incerto = recorte_amostras
            && !amostras.is_empty()
            && vigente(&amostras).len() == amostras.len()
            && matches!(prever(&amostras, now), Previsao::Sem);
        // `exhausted` é fato OBSERVADO: último percent ≥ 100 mesmo com recorte
        // — mas um 100% anterior a um resetsAt já passado não vale (mesma
        // régua de `prever`: a janela rolou sem leitura da nova).
        let mut reset = vigente(&amostras).iter().rev().find_map(|a| a.resets_at);
        let previsao = if amostras.last().is_some_and(|a| a.percent >= 100.0) {
            let (at, reset_historico) = primeiro_cem(c, &provider, &window, now)?;
            reset = reset_historico;
            if reset.is_some_and(|r| r <= now) {
                json!({"exhausted":false,"exhaustsAt":null})
            } else {
                json!({"exhausted":true,"exhaustsAt":at.unwrap_or(now)})
            }
        } else if cortada || incerto {
            Value::Null
        } else {
            match prever(&amostras, now) {
                Previsao::Estimada { at } => json!({"exhausted":false,"exhaustsAt":at}),
                _ => json!({"exhausted":false,"exhaustsAt":null}),
            }
        };
        let (provedor, conta) =
            provider.strip_prefix("codex:").map_or((provider.as_str(), None), |c| ("codex", Some(c)));
        let mut janela = json!({
            "provider": provedor,
            "windowId": window,
            "resetsAt": reset,
            "buckets": buckets,
            "forecast": previsao,
        });
        if let Some(conta) = conta {
            janela["accountId"] = conta.into();
        }
        saida.push(janela);
    }
    Ok(json!({
        "range": faixa.nome(),
        "bucketMs": bloco,
        "measuredAt": now,
        "truncated": truncated,
        "windows": saida,
    }))
}
