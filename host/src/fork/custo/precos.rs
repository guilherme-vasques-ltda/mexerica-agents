//! MX-159: tabela versionada de preços de API (ver `precos.json` e `PRECOS.md`),
//! usada somente quando o motor não informa custo nenhum. Nunca inventa preço:
//! modelo sem linha exata nem alias declarado fica sem preço.
//!
//! Precisão: cada preço é convertido para micro-USD por milhão de tokens
//! (inteiro; a tabela só aceita até 6 casas decimais, validada no parse e em
//! teste) e a multiplicação por tokens é feita em inteiro. O erro máximo é a
//! fração desprezada abaixo de 1 micro-USD por turno, nunca acumulado.
use super::ledger::{Cost, Tokens};
use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};

const TABELA: &str = include_str!("precos.json");
const MICROS_POR_USD: f64 = 1_000_000.0;
/// Maior inteiro exato em f64 (2^53): preços cabem com folga.
const MAX_MICROS: f64 = 9_007_199_254_740_992.0;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Faixa {
    /// Tamanho máximo do prompt (input + cache lido + cache gravado) coberto
    /// por esta faixa; ausente na última (cobre qualquer prompt maior).
    ate: Option<i64>,
    input: f64,
    output: f64,
    cache_read: Option<f64>,
    cache_write: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entrada {
    /// Linha sem faixa: os campos de preço no nível da entrada.
    input: Option<f64>,
    output: Option<f64>,
    cache_read: Option<f64>,
    cache_write: Option<f64>,
    /// Linha com faixa por tamanho de prompt (preço progressivo).
    faixas: Option<Vec<Faixa>>,
    #[allow(dead_code)]
    fonte: String,
    #[allow(dead_code)]
    data: String,
}

/// Política de omissão de cache medida por adaptador (ver `politicaCache` no
/// JSON e PRECOS.md). `read`/`write`: `"sempre"` = o adaptador emite a
/// contagem mesmo em zero; `"omite"` = suprime o campo quando zero (ausente
/// vale zero); `"sem prova"` = não se sabe (ausente = `sem preço`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Politica {
    versao: String,
    read: String,
    write: String,
    #[allow(dead_code)]
    fonte: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Tabela {
    #[allow(dead_code)]
    unidade: String,
    #[allow(dead_code)]
    politica: String,
    #[serde(default, rename = "politicaCache")]
    politica_cache: BTreeMap<String, Politica>,
    modelos: BTreeMap<String, Entrada>,
    #[serde(default)]
    aliases: BTreeMap<String, String>,
}

// Os casts f64→i64 são seguros pelos guardas (finito, não negativo, ≤ 2⁵³ e
// inteiro exato em micro-USD); a conversão i64→f64 só exibe o valor em USD.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn micros(v: f64) -> Option<i64> {
    let m = (v * MICROS_POR_USD).round();
    (v.is_finite() && v >= 0.0 && (m - v * MICROS_POR_USD).abs() < 1e-3 && m <= MAX_MICROS).then_some(m as i64)
}

/// Normaliza a entrada em uma lista não vazia de faixas: linha plana vira uma
/// faixa única sem teto. Mistura de formas (planos + faixas) ou lista vazia
/// invalidam a linha.
fn faixas_de(e: &Entrada) -> Option<Vec<Faixa>> {
    if let Some(f) = &e.faixas {
        if f.is_empty() || e.input.is_some() || e.output.is_some() || e.cache_read.is_some() || e.cache_write.is_some()
        {
            return None;
        }
        Some(f.clone())
    } else {
        Some(vec![Faixa {
            ate: None,
            input: e.input?,
            output: e.output?,
            cache_read: e.cache_read,
            cache_write: e.cache_write,
        }])
    }
}

/// Desserializa e valida a tabela inteira: preços finitos, não negativos, com
/// até 6 casas decimais; `fonte`/`data` preenchidos; alias só para modelo
/// existente; faixas não vazias com `ate` crescente e ausente só na última.
/// Qualquer falha → `None` (tudo sem preço, nunca pânico).
fn parse(tabela: &str) -> Option<Tabela> {
    let t: Tabela = serde_json::from_str(tabela).ok()?;
    for e in t.modelos.values() {
        let faixas = faixas_de(e)?;
        let mut anterior = 0_i64;
        for (i, f) in faixas.iter().enumerate() {
            micros(f.input)?;
            micros(f.output)?;
            for p in [f.cache_read, f.cache_write].into_iter().flatten() {
                micros(p)?;
            }
            match f.ate {
                // Toda faixa tem teto estritamente crescente; a última pode ser
                // limitada (provedor só publica até certo tamanho) ou aberta.
                Some(ate) if ate > anterior => anterior = ate,
                None if i + 1 == faixas.len() => {}
                _ => return None,
            }
        }
        if !e.fonte.starts_with("https://") || e.data.len() != 10 {
            return None;
        }
    }
    for (alias, destino) in &t.aliases {
        if !t.modelos.contains_key(destino) || t.modelos.contains_key(alias) {
            return None;
        }
    }
    for p in t.politica_cache.values() {
        let valido = |v: &str| matches!(v, "sempre" | "omite" | "sem prova");
        if p.versao.is_empty() || p.fonte.is_empty() || !valido(&p.read) || !valido(&p.write) {
            return None;
        }
    }
    Some(t)
}

fn tabela() -> Option<&'static Tabela> {
    static T: OnceLock<Option<Tabela>> = OnceLock::new();
    T.get_or_init(|| {
        let parsed = parse(TABELA);
        if parsed.is_none() {
            tracing::error!("tabela de preços embutida inválida; todos os modelos ficam sem preço");
        }
        parsed
    })
    .as_ref()
}

/// Política medida de um campo de cache de um adaptador (decisão do
/// coordenador, thread `thr_p2e28pdris`; `fonte` arquivo:linha no JSON).
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum CampoCache {
    /// Emite a contagem sempre, mesmo em zero: ausência é anômala → sem preço.
    Sempre,
    /// Omite o campo quando zero: ausente = zero.
    Omite,
    /// Sem prova sobre omissão (padrão): ausente = sem preço sempre, mesmo
    /// sem preço de cache na linha.
    #[default]
    SemProva,
}

/// Decisão por adaptador, medida na thread `thr_p2e28pdris` (arquivo:linha na
/// `fonte` de cada entrada do JSON). `"omite"` zera a ausência; `"sempre"`,
/// `"sem prova"`, backend desconhecido ou
/// `agentInfo.version` diferente da versão pinada → ausência = `sem preço`.
/// PONTO DE DECISÃO "cache ausente": a regra por motor mora aqui.
#[derive(Clone, Copy, Default)]
pub(crate) struct PoliticaCache {
    pub read: CampoCache,
    pub write: CampoCache,
}
pub(crate) fn politica_cache(backend: &str, versao: Option<&str>) -> PoliticaCache {
    let campo = |v: &str| match v {
        "sempre" => CampoCache::Sempre,
        "omite" => CampoCache::Omite,
        _ => CampoCache::SemProva,
    };
    let Some(p) = tabela().and_then(|t| t.politica_cache.get(backend)) else {
        return PoliticaCache::default();
    };
    if versao != Some(p.versao.as_str()) {
        return PoliticaCache::default();
    }
    PoliticaCache { read: campo(&p.read), write: campo(&p.write) }
}

/// Par (contagem, preço por `MTok` em micro-USD) de uma contagem de cache.
/// `0` explícito vale zero sem exigir preço; contagem positiva exige preço
/// verificado. Ausência só vale zero com prova por adaptador: `Omite` zera;
/// `Sempre`, `SemProva` e qualquer ausência não provada → `sem preço`, mesmo
/// sem preço de cache na faixa (um campo que o adaptador nunca emite não
/// prova que o valor era zero).
fn cache(count: Option<i64>, preco: Option<i64>, pol: CampoCache) -> Option<(i64, i64)> {
    match (count, preco, pol) {
        (Some(0), _, _) | (None, _, CampoCache::Omite) => Some((0, 0)),
        (Some(n), Some(p), _) if n > 0 => Some((n, p)),
        _ => None,
    }
}

/// Estima o custo do turno em USD. Exige `input` e `output` conhecidos; campo
/// de uso presente mas não representável (`tokens.invalid`) → `None` sempre.
/// A faixa é escolhida pelo tamanho AGREGADO do turno (input + cache lido +
/// cache gravado somados — pode cobrir várias chamadas e superestimar, o que
/// é conservador para o teto); sem faixa aplicável com preço verificado, ou
/// prompt acima da última faixa limitada → `None` ("sem preço"). Modelo fora
/// da tabela (nem via alias) → `None`.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn estimar(model: &str, tokens: &Tokens, politica: PoliticaCache) -> Option<Cost> {
    if tokens.invalid {
        return None;
    }
    let t = tabela()?;
    let chave = t.aliases.get(model).map_or(model, String::as_str);
    let e = t.modelos.get(chave)?;
    let faixas = faixas_de(e)?;
    let prompt =
        i128::from(tokens.input?) + i128::from(tokens.read.unwrap_or(0)) + i128::from(tokens.write.unwrap_or(0));
    let faixa = faixas.iter().find(|f| f.ate.is_none_or(|ate| prompt <= i128::from(ate)))?;
    let entrada = micros(faixa.input)?;
    let saida = micros(faixa.output)?;
    let (input, output) = (tokens.input?, tokens.output?);
    if input < 0 || output < 0 {
        return None;
    }
    // Campo de cache ausente na faixa (preço não verificado) → `None`; campo
    // presente mas inválido invalida a linha inteira — nunca subestima.
    let preco = |p: Option<f64>| p.map_or(Some(None), |v| micros(v).map(Some));
    let (n_read, p_read) = cache(tokens.read, preco(faixa.cache_read)?, politica.read)?;
    let (n_write, p_write) = cache(tokens.write, preco(faixa.cache_write)?, politica.write)?;
    let soma: i128 = i128::from(input) * i128::from(entrada)
        + i128::from(output) * i128::from(saida)
        + i128::from(n_read) * i128::from(p_read)
        + i128::from(n_write) * i128::from(p_write);
    // USD = tokens × (micro-USD por MTok) / 1e12; a parte inteira sai em
    // micro-USD exatos e o restante conserva a fração (< 1 micro-USD/turno).
    let inteiro = i64::try_from(soma / 1_000_000).ok()?;
    let resto = i64::try_from(soma % 1_000_000).ok()?;
    (inteiro >= 0)
        .then(|| Cost { amount: inteiro as f64 / MICROS_POR_USD + resto as f64 / 1e12, currency: "USD".into() })
}

#[cfg(test)]
mod tests {
    //! MX-159/P2-5: a tabela embutida desserializa com o MESMO tipo tipado de
    //! produção; texto inválido falha no parse (na prática → tudo sem preço,
    //! sem pânico).
    #![allow(clippy::unwrap_used)]
    #[test]
    fn tabela_embutida_desserializa_e_linhas_sao_validas() {
        let t = super::parse(super::TABELA).expect("precos.json embutido deve desserializar e validar");
        assert!(t.modelos.len() >= 40, "tabela encolheu? {} modelos", t.modelos.len());
    }
    #[test]
    fn tabela_invalida_vira_sem_preco_sem_panico() {
        assert!(super::parse("{lixo").is_none());
        assert!(super::parse("{}").is_none());
        // Preço negativo e alias quebrado também invalidam a tabela inteira.
        assert!(super::parse(
            r#"{"unidade":"u","politica":"p","modelos":{"m":{"input":-1,"output":1,"fonte":"https://x","data":"2026-11-11"}},"aliases":{}}"#
        )
        .is_none());
        assert!(super::parse(
            r#"{"unidade":"u","politica":"p","modelos":{"m":{"input":1,"output":1,"fonte":"https://x","data":"2026-11-11"}},"aliases":{"a":"inexistente"}}"#
        )
        .is_none());
    }
}
