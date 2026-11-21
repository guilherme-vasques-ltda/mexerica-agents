//! Contrato do gráfico de leitura (v1): linha, barras ou pizza a partir de dados estruturados.
//! O parse só valida a forma e os limites; não prova fonte nem audiência e não dá autoridade.
use super::contract::{MAX_BYTES, literal};
use anyhow::{Result, ensure};
use chrono::{DateTime, NaiveDate};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Number, Value, json};
use std::fmt::Write as _;

const MAX_VALUE: f64 = 9_007_199_254_740_991.0;
const MAX_POINTS: usize = 500;
const MAX_SERIES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Component {
    #[serde(rename = "line_chart")]
    Line,
    #[serde(rename = "bar_chart")]
    Bar,
    #[serde(rename = "pie_chart")]
    Pie,
}

impl Component {
    fn name(self) -> &'static str {
        match self {
            Self::Line => "line_chart",
            Self::Bar => "bar_chart",
            Self::Pie => "pie_chart",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum XKind {
    Category,
    Date,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    version: u8,
    component: Component,
    title: String,
    summary: String,
    data: Data,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    // Ausência é distinta de null: null não pertence ao enum em nenhum gráfico.
    #[serde(rename = "xKind", default, deserialize_with = "x_kind", skip_serializing_if = "Option::is_none")]
    x_kind: Option<XKind>,
    labels: Vec<String>,
    series: Vec<Series>,
    unit: String,
    period: String,
    #[serde(rename = "sourceLabel")]
    source_label: String,
}

fn x_kind<'de, D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Option<XKind>, D::Error> {
    XKind::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Series {
    id: String,
    label: String,
    values: Vec<Option<Number>>,
}

/// Fotografia tipada e imutável. Nenhum campo do modelo constitui autoridade.
#[derive(Debug, Clone)]
pub(crate) struct ValidatedChart {
    payload: Payload,
    effective: Component,
}

impl ValidatedChart {
    pub(crate) fn parse(raw: &str) -> Result<Self> {
        ensure!(raw.len() <= MAX_BYTES, "genui_chart_too_large");
        // Serde também aceitaria listas posicionais nas structs; o contrato só admite objetos.
        let shape: Value = serde_json::from_str(raw).map_err(|_| anyhow::anyhow!("genui_chart_invalid_payload"))?;
        let objects = shape.is_object()
            && shape["data"].is_object()
            && shape["data"]["series"].as_array().is_none_or(|all| all.iter().all(Value::is_object));
        ensure!(objects, "genui_chart_invalid_payload");
        let payload: Payload = serde_json::from_str(raw).map_err(|_| anyhow::anyhow!("genui_chart_invalid_payload"))?;
        ensure!(payload.version == 1, "genui_chart_unsupported_version");
        text(&payload.title, 160)?;
        text(&payload.summary, 2_000)?;
        let data = &payload.data;
        for value in [&data.unit, &data.period, &data.source_label] {
            text(value, 160)?;
        }
        ensure!((1..=MAX_POINTS).contains(&data.labels.len()), "genui_chart_invalid_labels");
        ensure!((1..=MAX_SERIES).contains(&data.series.len()), "genui_chart_invalid_series");
        match payload.component {
            Component::Line => ensure!(data.x_kind.is_some(), "genui_chart_invalid_x_kind"),
            Component::Bar | Component::Pie => ensure!(data.x_kind.is_none(), "genui_chart_invalid_x_kind"),
        }
        for label in &data.labels {
            text(label, 160)?;
            if data.x_kind == Some(XKind::Date) {
                ensure!(valid_date(label), "genui_chart_invalid_date");
            }
        }
        let mut ids = std::collections::HashSet::new();
        for series in &data.series {
            ensure!(
                !series.id.is_empty()
                    && series.id.len() <= 160
                    && series.id.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b)),
                "genui_chart_invalid_series_id"
            );
            ensure!(ids.insert(&series.id), "genui_chart_duplicate_series_id");
            text(&series.label, 160)?;
            ensure!(series.values.len() == data.labels.len(), "genui_chart_irregular_matrix");
            for value in series.values.iter().flatten() {
                let number = value.as_f64().ok_or_else(|| anyhow::anyhow!("genui_chart_invalid_number"))?;
                ensure!(number.is_finite() && number.abs() <= MAX_VALUE, "genui_chart_invalid_number");
            }
        }
        let mut effective = payload.component;
        if payload.component == Component::Pie {
            ensure!(data.series.len() == 1, "genui_chart_invalid_pie");
            let mut total = Vec::new();
            for value in &data.series[0].values {
                let value = value.as_ref().ok_or_else(|| anyhow::anyhow!("genui_chart_incomplete_pie"))?;
                let value = value.as_f64().ok_or_else(|| anyhow::anyhow!("genui_chart_invalid_number"))?;
                ensure!(value >= 0.0, "genui_chart_invalid_pie");
                add_exact(&mut total, value);
            }
            // Todos os valores são não negativos: expansão vazia significa total zero.
            ensure!(!total.is_empty(), "genui_chart_invalid_pie_total");
            add_exact(&mut total, -MAX_VALUE);
            // Compara a diferença exata, inclusive seus resíduos, sem arredondá-la.
            ensure!(total.last().is_none_or(|difference| *difference < 0.0), "genui_chart_invalid_pie_total");
            if data.labels.len() > 5 {
                effective = Component::Bar;
            }
        }
        Ok(Self { payload, effective })
    }

    pub(crate) fn redacted(&self) -> Result<Self> {
        let mut payload = self.payload.clone();
        let mask = |value: &mut String| {
            *value = crate::fork::planos_codigo::mascarar_codigo(value).into_owned();
        };
        mask(&mut payload.title);
        mask(&mut payload.summary);
        mask(&mut payload.data.unit);
        mask(&mut payload.data.period);
        mask(&mut payload.data.source_label);
        for label in &mut payload.data.labels {
            mask(label);
        }
        for series in &mut payload.data.series {
            mask(&mut series.label);
            let masked_id = crate::fork::planos_codigo::mascarar_codigo(&series.id);
            if masked_id != series.id {
                series.id = masked_id.replace("mxs_[oculto]", "mxs_oculto");
            }
        }
        let raw = serde_json::to_string(&payload).map_err(|_| anyhow::anyhow!("genui_chart_snapshot_failed"))?;
        Self::parse(&raw)
    }

    /// Snapshot canônico (`{requested, effective, payload}`): é o que o Hub guarda e o app lê.
    pub(crate) fn snapshot(&self) -> Result<String> {
        #[derive(Serialize)]
        struct Snapshot<'a> {
            requested: Component,
            effective: Component,
            payload: &'a Payload,
        }
        serde_json::to_string(&Snapshot {
            requested: self.payload.component,
            effective: self.effective,
            payload: &self.payload,
        })
        .map_err(|_| anyhow::anyhow!("genui_chart_snapshot_failed"))
    }

    /// Cada valor aceito aparece literalmente, sem rede, inferência ou proporção inventada.
    pub(crate) fn fallback(&self) -> String {
        let data = &self.payload.data;
        let mut body = format!(
            "{}\n{}\n\nOrigem: Informado pelo bot\nFonte informada: {}\nPeríodo: {}\nUnidade: {}\n",
            self.payload.title, self.payload.summary, data.source_label, data.period, data.unit
        );
        let _ =
            writeln!(body, "Peça solicitada: {}; efetiva: {}", self.payload.component.name(), self.effective.name());
        if let Some(kind) = data.x_kind {
            let _ = writeln!(
                body,
                "Eixo: {}",
                match kind {
                    XKind::Category => "category",
                    XKind::Date => "date",
                }
            );
        }
        for series in &data.series {
            let _ = writeln!(body, "\nSérie: {} (ID: {})", series.label, series.id);
            for (label, value) in data.labels.iter().zip(&series.values) {
                match value {
                    Some(value) => {
                        let _ = writeln!(body, "{label}: {value}");
                    }
                    None => {
                        let _ = writeln!(body, "{label}: Indisponível");
                    }
                }
            }
        }
        literal(&body)
    }
}

/// Expansão crescente e não sobreposta: a soma dos termos é exata em f64.
/// `TwoSum` decompõe cada adição em `high + low` sem descartar o resíduo (de qualquer
/// sinal). O último termo não zero domina os anteriores e determina o sinal.
/// Até 500 valores com magnitude <= `MAX_VALUE`, mais `-MAX_VALUE`: nenhum high
/// intermediário chega perto de overflow. A expansão não modifica o payload.
fn add_exact(partials: &mut Vec<f64>, mut value: f64) {
    partials.retain_mut(|partial| {
        let high = value + *partial;
        let recovered = high - value;
        let low = (value - (high - recovered)) + (*partial - recovered);
        value = high;
        *partial = low;
        low != 0.0
    });
    if value != 0.0 {
        partials.push(value);
    }
}

fn text(value: &str, max: usize) -> Result<()> {
    ensure!(
        !value.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}').is_empty()
            && value.chars().count() <= max
            && !value.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\t')),
        "genui_chart_invalid_text"
    );
    Ok(())
}

fn valid_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes[..10].iter().enumerate().all(|(i, b)| matches!(i, 4 | 7) || b.is_ascii_digit())
        || NaiveDate::parse_from_str(&value[..10], "%Y-%m-%d").is_err()
    {
        return false;
    }
    if bytes.len() == 10 {
        return true;
    }
    if bytes.get(10) != Some(&b'T') || value.ends_with("-00:00") {
        return false;
    }
    let explicit_zone = value.ends_with('Z')
        || (bytes.len() >= 25 && matches!(bytes[bytes.len() - 6], b'+' | b'-') && bytes[bytes.len() - 3] == b':');
    explicit_zone && DateTime::parse_from_rfc3339(value).is_ok()
}

/// Esquema do campo `data` de cada gráfico, para o descritor da ferramenta. As regras relacionais, as
/// datas e o total da pizza só o validador confere. Pizza acima de cinco partes vira barras, com todos os dados.
pub(crate) fn data_schemas() -> Vec<Value> {
    let string = |max| json!({"type":"string","minLength":1,"maxLength":max});
    [Component::Line, Component::Bar, Component::Pie]
        .into_iter()
        .map(|component| {
            let pie = component == Component::Pie;
            let mut data = json!({
                "type":"object", "additionalProperties":false,
                "required":["labels","series","unit","period","sourceLabel"],
                "properties":{
                    "labels":{"type":"array","minItems":1,"maxItems":MAX_POINTS,"items":string(160)},
                    "series":{
                        "type":"array","minItems":1,"maxItems":if pie { 1 } else { MAX_SERIES },
                        "items":{
                            "type":"object","additionalProperties":false,"required":["id","label","values"],
                            "properties":{
                                "id":{"type":"string","minLength":1,"maxLength":160,"pattern":"^[A-Za-z0-9_.-]+$"},
                                "label":string(160),
                                "values":{
                                    "type":"array","minItems":1,"maxItems":MAX_POINTS,
                                    "items":if pie {
                                        json!({"type":"number","minimum":0,"maximum":MAX_VALUE})
                                    } else {
                                        json!({"type":["number","null"],"minimum":-MAX_VALUE,"maximum":MAX_VALUE})
                                    }
                                }
                            }
                        }
                    },
                    "unit":string(160),"period":string(160),"sourceLabel":string(160)
                }
            });
            if component == Component::Line {
                data["required"] = json!(["xKind", "labels", "series", "unit", "period", "sourceLabel"]);
                data["properties"]["xKind"] = json!({"type":"string","enum":["category","date"]});
            }
            data
        })
        .collect()
}

pub(crate) const COMPONENTS: [&str; 3] = ["line_chart", "bar_chart", "pie_chart"];

#[cfg(test)]
#[path = "charts_tests.rs"]
mod tests;
