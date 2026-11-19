//! D70: contrato puro, sem bot, rede, banco ou prova de autoridade.
use super::{MAX_BYTES, MAX_VALUE, ValidatedChart, descriptor};
use serde_json::{Value, json};

fn payload(component: &str, count: usize) -> Value {
    let mut value = json!({
        "version":1,"component":component,"title":"Quantidade informada",
        "summary":"Exemplo, não prova de fonte ou audiência.",
        "data":{
            "labels":(0..count).map(|i| format!("Categoria {i}")).collect::<Vec<_>>(),
            "series":[{"id":"volume","label":"Quantidade","values":(0..count).map(|i| i + 1).collect::<Vec<_>>() }],
            "unit":"itens","period":"Novembro de 2026","sourceLabel":"Conversa informada"
        }
    });
    if component == "line_chart" {
        value["data"]["xKind"] = json!("category");
    }
    value
}

fn parse(value: &Value) -> ValidatedChart {
    ValidatedChart::parse(&value.to_string()).unwrap()
}

fn snapshot(chart: &ValidatedChart) -> Value {
    serde_json::from_str(&chart.snapshot().unwrap()).unwrap()
}

fn error(value: &Value) -> String {
    ValidatedChart::parse(&value.to_string()).unwrap_err().to_string()
}

#[test]
fn catalogo_fechado_e_descritor_separado() {
    let schema = descriptor();
    assert!(schema.get("name").is_none());
    let variants = schema["oneOf"].as_array().unwrap();
    assert_eq!(variants.len(), 3);
    for (index, component) in ["line_chart", "bar_chart", "pie_chart"].iter().enumerate() {
        let variant = &variants[index];
        assert_eq!(variant["additionalProperties"], false);
        assert_eq!(variant["properties"]["component"]["const"], *component);
        let data = &variant["properties"]["data"];
        assert_eq!(data["additionalProperties"], false);
        assert_eq!(data["properties"]["series"]["items"]["additionalProperties"], false);
        let value = payload(component, 2);
        assert_eq!(snapshot(&parse(&value))["requested"], *component);
    }
    assert_eq!(
        super::super::contract::descriptor()["inputSchema"]["properties"]["component"]["enum"],
        json!(["choice"])
    );
    for component in ["choice", "table", "steps", "svg", "SENTINELA_PRIVADA"] {
        let value = payload(component, 1);
        assert_eq!(error(&value), "genui_chart_invalid_payload");
    }
    for version in [json!(0), json!(2), json!(256), json!(1.5), json!("1"), Value::Null] {
        let mut value = payload("bar_chart", 1);
        value["version"] = version;
        assert!(error(&value).starts_with("genui_chart_"));
    }
    for key in ["title", "summary", "version", "component", "data"] {
        let mut value = payload("bar_chart", 1);
        value.as_object_mut().unwrap().remove(key);
        assert_eq!(error(&value), "genui_chart_invalid_payload");
    }
}

#[test]
fn campos_extras_inclusive_autoridade_e_recursos_sao_recusados_sem_eco() {
    for key in [
        "html",
        "js",
        "css",
        "svg",
        "url",
        "command",
        "ipc",
        "callback",
        "graphConfig",
        "sourceRefs",
        "verified",
        "spaceId",
        "botId",
        "sessionId",
        "ExecutionRef",
        "requested",
        "effective",
        "payload",
    ] {
        for level in 0..3 {
            let mut value = payload("bar_chart", 1);
            let target = match level {
                0 => &mut value,
                1 => &mut value["data"],
                _ => &mut value["data"]["series"][0],
            };
            target[key] = json!("SENTINELA_PRIVADA");
            assert_eq!(error(&value), "genui_chart_invalid_payload");
        }
    }
    for raw in [
        "SENTINELA_PRIVADA",
        "{\"version\":1,\"version\":1,\"title\":\"SENTINELA_PRIVADA\"}",
        "{\"title\":\"SENTINELA_PRIVADA\"",
    ] {
        let error = ValidatedChart::parse(raw).unwrap_err();
        assert_eq!(format!("{error:#}"), "genui_chart_invalid_payload");
        assert!(!format!("{error:?}").contains("SENTINELA_PRIVADA"));
    }
    let raw = payload("bar_chart", 1).to_string().replacen("\"version\":1", "\"version\":1,\"version\":1", 1);
    assert_eq!(ValidatedChart::parse(&raw).unwrap_err().to_string(), "genui_chart_invalid_payload");
}

#[test]
fn limites_de_bytes_strings_e_controles_sem_truncamento() {
    let raw = payload("bar_chart", 1).to_string();
    let exact = format!("{raw}{}", " ".repeat(MAX_BYTES - raw.len()));
    assert!(ValidatedChart::parse(&exact).is_ok());
    assert_eq!(ValidatedChart::parse(&(exact + " ")).unwrap_err().to_string(), "genui_chart_too_large");
    for (pointer, max) in [
        ("/title", 160),
        ("/summary", 2000),
        ("/data/unit", 160),
        ("/data/period", 160),
        ("/data/sourceLabel", 160),
        ("/data/labels/0", 160),
        ("/data/series/0/label", 160),
    ] {
        let mut value = payload("bar_chart", 1);
        *value.pointer_mut(pointer).unwrap() = json!("á".repeat(max));
        assert!(ValidatedChart::parse(&value.to_string()).is_ok(), "{pointer}");
        *value.pointer_mut(pointer).unwrap() = json!("á".repeat(max + 1));
        assert_eq!(error(&value), "genui_chart_invalid_text", "{pointer}");
        for text in ["", " \n\t", "SENTINELA_PRIVADA\u{0}", "texto\r", "texto\u{7f}"] {
            *value.pointer_mut(pointer).unwrap() = json!(text);
            assert_eq!(error(&value), "genui_chart_invalid_text", "{pointer}");
        }
        *value.pointer_mut(pointer).unwrap() = json!("Texto\ncom\ttab");
        assert!(ValidatedChart::parse(&value.to_string()).is_ok());
    }
}

#[test]
fn ids_matriz_irregular_e_limites_de_series_e_pontos() {
    for id in ["", "https://fixture.invalid", "ação", "a b", "a\n", &"a".repeat(161)] {
        let mut value = payload("bar_chart", 1);
        value["data"]["series"][0]["id"] = json!(id);
        assert_eq!(error(&value), "genui_chart_invalid_series_id");
    }
    let mut value = payload("bar_chart", 1);
    for id in [String::from("a_.-09"), "a".repeat(160)] {
        value["data"]["series"][0]["id"] = json!(id);
        assert!(ValidatedChart::parse(&value.to_string()).is_ok());
    }
    let series = value["data"]["series"][0].clone();
    value["data"]["series"].as_array_mut().unwrap().push(series);
    assert_eq!(error(&value), "genui_chart_duplicate_series_id");
    for count in [0, 501] {
        assert_eq!(error(&payload("bar_chart", count)), "genui_chart_invalid_labels");
    }
    for count in [0, 4] {
        let mut value = payload("bar_chart", 1);
        value["data"]["series"] =
            json!((0..count).map(|i| json!({"id":format!("s{i}"),"label":"Série","values":[1]})).collect::<Vec<_>>());
        assert_eq!(error(&value), "genui_chart_invalid_series");
    }
    for values in [json!([]), json!([1, 2])] {
        let mut value = payload("bar_chart", 1);
        value["data"]["series"][0]["values"] = values;
        assert_eq!(error(&value), "genui_chart_irregular_matrix");
    }
    for component in ["line_chart", "bar_chart"] {
        let mut value = payload(component, 500);
        value["data"]["series"] = json!(
            (0..3)
                .map(|i| json!({"id":format!("s{i}"),"label":"Série","values":vec![MAX_VALUE; 500]}))
                .collect::<Vec<_>>()
        );
        // Linha/barra não têm restrição de soma global, nem truncamento em 3×500.
        assert_eq!(snapshot(&parse(&value))["payload"]["data"], value["data"]);
    }
}

#[test]
fn numeros_finitos_escala_e_lacuna_diferente_de_zero() {
    for component in ["line_chart", "bar_chart"] {
        let mut value = payload(component, 3);
        value["data"]["series"][0]["values"] = json!([null, 0, -3.5]);
        let chart = parse(&value);
        assert_eq!(snapshot(&chart)["payload"]["data"], value["data"]);
        let fallback = chart.fallback();
        assert!(fallback.contains("Categoria 0: Indisponível\n"));
        assert!(fallback.contains("Categoria 1: 0\n"));
        assert!(fallback.contains("Categoria 2: -3.5\n"));
        value["data"]["series"][0]["values"] = json!([-MAX_VALUE, MAX_VALUE, 0]);
        assert!(ValidatedChart::parse(&value.to_string()).is_ok());
        for out_of_scale in [MAX_VALUE + 1.0, -MAX_VALUE - 1.0] {
            value["data"]["series"][0]["values"] = json!([out_of_scale, 0, 0]);
            assert_eq!(error(&value), "genui_chart_invalid_number");
        }
    }
    for number in ["NaN", "Infinity", "-Infinity", "1e400", "-1e400", "\"SENTINELA_PRIVADA\""] {
        let raw = payload("bar_chart", 1).to_string().replace("\"values\":[1]", &format!("\"values\":[{number}]"));
        assert_eq!(ValidatedChart::parse(&raw).unwrap_err().to_string(), "genui_chart_invalid_payload");
    }
}

#[test]
fn datas_reais_e_fuso_explicito_sem_inferencia() {
    for date in [
        "2024-02-29",
        "2026-11-04",
        "2026-11-04T10:30:00Z",
        "2026-11-04T10:30:00.123-03:00",
        "2026-11-04T10:30:00+00:00",
    ] {
        let mut value = payload("line_chart", 1);
        value["data"]["xKind"] = json!("date");
        value["data"]["labels"][0] = json!(date);
        assert_eq!(snapshot(&parse(&value))["payload"]["data"]["labels"][0], date);
    }
    for date in [
        "2026-02-29",
        "2026-04-31",
        "2026-13-01",
        "2026-1-01",
        "04/11/2026",
        "2026-11-04T10:30:00",
        "2026-11-04T10:30:00-00:00",
        "2026-11-04 10:30:00Z",
        "2026-11-04T25:00:00Z",
        "2026-11-04T10:30:00+24:00",
        "2026-11-04T10:30:00z",
        "2026-11-04T10:30:00 America/Sao_Paulo",
        "áááááááááá",
    ] {
        let mut value = payload("line_chart", 1);
        value["data"]["xKind"] = json!("date");
        value["data"]["labels"][0] = json!(date);
        assert_eq!(error(&value), "genui_chart_invalid_date", "{date}");
        // Categoria não é reinterpretada como data.
        value["data"]["xKind"] = json!("category");
        assert!(ValidatedChart::parse(&value.to_string()).is_ok());
    }
    let mut value = payload("line_chart", 1);
    value["data"].as_object_mut().unwrap().remove("xKind");
    assert_eq!(error(&value), "genui_chart_invalid_x_kind");
    for kind in [Value::Null, json!("time"), json!(1)] {
        value["data"]["xKind"] = kind;
        assert_eq!(error(&value), "genui_chart_invalid_payload");
    }
    for component in ["bar_chart", "pie_chart"] {
        let mut value = payload(component, 1);
        value["data"]["xKind"] = json!("category");
        assert_eq!(error(&value), "genui_chart_invalid_x_kind");
        value["data"]["xKind"] = Value::Null;
        assert_eq!(error(&value), "genui_chart_invalid_payload");
    }
}

#[test]
fn unidade_comum_obrigatoria_sem_normalizar_ou_aceitar_por_serie() {
    let mut value = payload("bar_chart", 2);
    for unit in ["BRL", "%", "ms", "Unidade informada, não verificada"] {
        value["data"]["unit"] = json!(unit);
        assert_eq!(snapshot(&parse(&value))["payload"]["data"]["unit"], unit);
    }
    value["data"]["series"][0]["unit"] = json!("USD");
    assert_eq!(error(&value), "genui_chart_invalid_payload");
    value["data"]["series"][0].as_object_mut().unwrap().remove("unit");
    value["data"].as_object_mut().unwrap().remove("unit");
    assert_eq!(error(&value), "genui_chart_invalid_payload");
}

#[test]
fn pizza_recusa_negativos_zero_lacunas_e_total_fora_da_escala() {
    for count in [2, 6] {
        for (values, expected) in [
            (vec![json!(-1); count], "genui_chart_invalid_pie"),
            (vec![json!(0); count], "genui_chart_invalid_pie_total"),
            (vec![Value::Null; count], "genui_chart_incomplete_pie"),
            (vec![json!(MAX_VALUE); count], "genui_chart_invalid_pie_total"),
        ] {
            let mut value = payload("pie_chart", count);
            value["data"]["series"][0]["values"] = json!(values);
            assert_eq!(error(&value), expected);
        }
    }
    let mut value = payload("pie_chart", 2);
    value["data"]["series"][0]["values"] = json!([MAX_VALUE, 0.1]);
    assert_eq!(error(&value), "genui_chart_invalid_pie_total");
    value["data"]["series"][0]["values"] = json!([0, 0.25]);
    assert!(ValidatedChart::parse(&value.to_string()).is_ok());
    let mut second = value["data"]["series"][0].clone();
    second["id"] = json!("outra");
    value["data"]["series"].as_array_mut().unwrap().push(second);
    assert_eq!(error(&value), "genui_chart_invalid_pie");
}

#[test]
fn pizza_cinco_seis_e_quinhentos_conserva_todos_os_dados_de_b() {
    for count in [1, 5, 6, 500] {
        let mut value = payload("pie_chart", count);
        value["data"]["series"][0]["values"][0] = json!(0.125);
        let chart = parse(&value);
        let snapshot = snapshot(&chart);
        assert_eq!(snapshot["requested"], "pie_chart");
        assert_eq!(snapshot["effective"], if count <= 5 { "pie_chart" } else { "bar_chart" });
        assert_eq!(snapshot["payload"], value);
        let fallback = chart.fallback();
        for (label, value) in value["data"]["labels"]
            .as_array()
            .unwrap()
            .iter()
            .zip(value["data"]["series"][0]["values"].as_array().unwrap())
        {
            assert!(fallback.contains(&format!("{}: {value}\n", label.as_str().unwrap())));
        }
        assert!(!fallback.contains("Outros"));
        for pointer in ["/title", "/summary", "/data/unit", "/data/period", "/data/sourceLabel"] {
            assert!(fallback.contains(value.pointer(pointer).unwrap().as_str().unwrap()));
        }
    }
}

#[test]
fn fallback_malicioso_literal_e_snapshot_parse_estaveis_sem_mutacao() {
    let malicious = "![imagem](https://fixture.invalid/img)\n``````\n<script>ipc('apagar')</script>\n[ação](https://fixture.invalid)";
    let mut value = payload("bar_chart", 1);
    for pointer in [
        "/title",
        "/summary",
        "/data/unit",
        "/data/period",
        "/data/sourceLabel",
        "/data/labels/0",
        "/data/series/0/label",
    ] {
        *value.pointer_mut(pointer).unwrap() = json!(malicious);
    }
    let chart = parse(&value);
    let before = chart.snapshot().unwrap();
    let fallback = chart.fallback();
    assert!(fallback.starts_with("```````\n"));
    assert!(fallback.ends_with("\n```````"));
    assert_eq!(fallback.matches(malicious).count(), 7);
    assert!(fallback.contains("Origem: Informado pelo bot\n"));
    assert!(fallback.contains("ID: volume"));
    assert_eq!(chart.snapshot().unwrap(), before);
    let typed = snapshot(&chart)["payload"].clone();
    assert_eq!(parse(&typed).snapshot().unwrap(), before);
    let mut reversed = serde_json::Map::new();
    for (key, value) in value.as_object().unwrap().iter().rev() {
        reversed.insert(key.clone(), value.clone());
    }
    let reordered = serde_json::to_string_pretty(&reversed).unwrap();
    assert_eq!(ValidatedChart::parse(&reordered).unwrap().snapshot().unwrap(), before);
    // Também preserva requested/effective ao revalidar o payload de pizza normalizada.
    let pie = parse(&payload("pie_chart", 6));
    assert_eq!(parse(&snapshot(&pie)["payload"]).snapshot().unwrap(), pie.snapshot().unwrap());
}

#[test]
fn total_exato_da_pizza_conserva_residuos_positivos_e_negativos_nas_duas_ordens() {
    let smallest = f64::from_bits(1);
    for values in [
        vec![MAX_VALUE],
        vec![MAX_VALUE - 1.0, 0.75],
        vec![MAX_VALUE - 1.0, 0.75, 0.25],
        vec![MAX_VALUE - 1.0, 0.5, 0.5],
        // Soma ingênua high chega a 2^53; resíduo negativo legítimo mantém total válido.
        vec![MAX_VALUE - 3.0, 0.75, 0.75, 0.75, 0.75],
        vec![MAX_VALUE - 3.0, 0.75, 0.75, 0.75, 0.5],
        vec![0.1, 0.2, 0.3],
        vec![smallest],
    ] {
        for reverse in [false, true] {
            let mut ordered = values.clone();
            if reverse {
                ordered.reverse();
            }
            let mut value = payload("pie_chart", ordered.len());
            value["data"]["series"][0]["values"] = json!(ordered);
            let chart = parse(&value);
            assert_eq!(snapshot(&chart)["payload"], value);
        }
    }
    for values in [
        vec![MAX_VALUE, 0.1],
        vec![MAX_VALUE, smallest],
        vec![MAX_VALUE - 1.0, 0.75, 0.5],
        vec![MAX_VALUE - 1.0, 0.5, 0.5, smallest],
        vec![MAX_VALUE - 3.0, 0.75, 0.75, 0.75, 1.0],
    ] {
        for reverse in [false, true] {
            let mut ordered = values.clone();
            if reverse {
                ordered.reverse();
            }
            let mut value = payload("pie_chart", ordered.len());
            value["data"]["series"][0]["values"] = json!(ordered);
            assert_eq!(error(&value), "genui_chart_invalid_pie_total");
        }
    }
}

#[test]
fn floats_snapshot_e_fallback_preservados_em_multiplos_round_trips() {
    for component in ["line_chart", "bar_chart", "pie_chart"] {
        let values = if component == "pie_chart" {
            // Seis categorias: valida soma no limite e conversão B sem mudar os floats.
            json!([MAX_VALUE - 3.0, 0.75, 0.75, 0.75, 0.75, 0.0])
        } else {
            json!([MAX_VALUE, -MAX_VALUE, 0.1, 0.2, -0.0, f64::from_bits(1), null, 0])
        };
        let mut original = payload(component, values.as_array().unwrap().len());
        original["data"]["series"][0]["values"] = values;
        let first = parse(&original);
        let expected_snapshot = first.snapshot().unwrap();
        let expected_fallback = first.fallback();
        let mut raw = original.to_string();
        for _ in 0..4 {
            let chart = ValidatedChart::parse(&raw).unwrap();
            let saved = snapshot(&chart);
            assert_eq!(saved["payload"], original);
            assert_eq!(saved["requested"], component);
            assert_eq!(saved["effective"], if component == "pie_chart" { "bar_chart" } else { component });
            assert_eq!(chart.snapshot().unwrap(), expected_snapshot);
            assert_eq!(chart.fallback(), expected_fallback);
            for (label, value) in original["data"]["labels"]
                .as_array()
                .unwrap()
                .iter()
                .zip(original["data"]["series"][0]["values"].as_array().unwrap())
            {
                let expected = if value.is_null() { "Indisponível".to_owned() } else { value.to_string() };
                assert!(chart.fallback().contains(&format!("{}: {expected}\n", label.as_str().unwrap())));
            }
            raw = saved["payload"].to_string();
        }
    }
}
