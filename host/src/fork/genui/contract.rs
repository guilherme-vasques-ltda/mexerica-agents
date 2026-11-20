//! Contrato fechado da escolha v1 e descritor da ferramenta (escolha e gráficos de leitura). O descritor só entra em tools/list pela ponte do servidor `chat`
//! numa conversa capturada (`crate::mcp`), depois de `tool_servers::decide` oferecer `chat`.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub(crate) const MAX_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    version: u8,
    component: Component,
    title: String,
    summary: String,
    data: Choice,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Component {
    Choice,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Choice {
    question: String,
    options: Vec<OptionData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OptionData {
    id: String,
    label: String,
}

/// Só pode nascer pelo validador; escopo e autoridade não pertencem ao payload.
#[derive(Debug, Clone)]
pub(crate) struct ValidatedChoice(Payload);

impl ValidatedChoice {
    pub(crate) fn parse(raw: &str) -> Result<Self> {
        ensure!(raw.len() <= MAX_BYTES, "genui_payload_too_large");
        let payload: Payload = serde_json::from_str(raw).map_err(|_| anyhow::anyhow!("genui_invalid_payload"))?;
        ensure!(payload.version == 1, "genui_unsupported_version");
        text(&payload.title, 160)?;
        text(&payload.summary, 2_000)?;
        text(&payload.data.question, 2_000)?;
        ensure!((1..=8).contains(&payload.data.options.len()), "genui_invalid_options");
        let mut ids = std::collections::HashSet::new();
        for option in &payload.data.options {
            // ID opaco, limitado, sem sintaxe de URL/comando. Nunca é destino de ação.
            ensure!(
                !option.id.is_empty()
                    && option.id.len() <= 160
                    && option.id.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b)),
                "genui_invalid_option_id"
            );
            ensure!(ids.insert(&option.id), "genui_duplicate_option_id");
            text(&option.label, 160)?;
        }
        Ok(Self(payload))
    }

    /// Serialização tipada estável: ordem/espaços do JSON de entrada não mudam o hash.
    pub(crate) fn snapshot(&self) -> Result<String> {
        Ok(serde_json::to_string(&self.0)?)
    }

    pub(crate) fn answer_text(&self, answer: &Answer) -> Result<String> {
        match answer {
            Answer::Option { id } => {
                let option = self.0.data.options.iter().find(|o| &o.id == id);
                Ok(option.ok_or_else(|| anyhow::anyhow!("genui_unknown_option"))?.label.clone())
            }
            Answer::None {} => Ok("Nenhuma destas".into()),
            Answer::Text { text: value } => {
                text(value, 4_000)?;
                Ok(value.clone())
            }
        }
    }

    /// Todo dado não confiável fica dentro de um único bloco literal Markdown.
    /// O delimitador é maior que qualquer sequência de crases do conteúdo.
    pub(crate) fn fallback(&self, id: &str, state: &str, answer: Option<&str>) -> String {
        use std::fmt::Write as _;
        let mut body = format!("{}\n{}\n\nPergunta {}\n{}\n", self.0.title, self.0.summary, id, self.0.data.question);
        for (index, option) in self.0.data.options.iter().enumerate() {
            let _ = writeln!(body, "{}. {}", index + 1, option.label);
        }
        body.push_str("Nenhuma destas; ou resposta em texto. Escolher não autoriza ações.\n");
        let _ = writeln!(body, "Estado: {state}");
        if let Some(answer) = answer {
            let _ = writeln!(body, "Resposta: {answer}");
        }
        literal(&body)
    }
}

fn text(value: &str, max: usize) -> Result<()> {
    ensure!(
        !value.trim().is_empty()
            && value.chars().count() <= max
            && !value.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\t')),
        "genui_invalid_text"
    );
    Ok(())
}

pub(crate) fn literal(text: &str) -> String {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(3.max(longest + 1));
    format!("{fence}\n{text}\n{fence}")
}

/// Contrato de resposta conversacional; não é grant, ação nem evidência de memória.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Answer {
    Option { id: String },
    // Struct vazia: a variante unit internamente taggeada aceitava campos extras.
    None {},
    Text { text: String },
}

/// Schema da ferramenta, sem contexto/estado/identidade fornecidos pelo modelo. Um só descritor para o
/// cartão de escolha e para os gráficos de leitura: o `component` escolhe o tipo, e o `data` aceita uma
/// forma de cada (o validador do tipo é quem decide).
pub(crate) fn descriptor() -> Value {
    let string = |max| json!({"type":"string","minLength":1,"maxLength":max});
    let choice = json!({
        "type":"object","additionalProperties":false,"required":["question","options"],
        "properties": {
            "question":string(2000),
            "options": {
                "type":"array","minItems":1,"maxItems":8,
                "items": {
                    "type":"object","additionalProperties":false,"required":["id","label"],
                    "properties": {
                        "id":{"type":"string","minLength":1,"maxLength":160,"pattern":"^[A-Za-z0-9_.-]+$"},
                        "label":string(160)
                    }
                }
            }
        }
    });
    let mut data = vec![choice];
    data.extend(super::charts::data_schemas());
    let mut components = vec!["choice"];
    components.extend(super::charts::COMPONENTS);
    json!({
        "name":"mostrar_componente",
        "description":"Mostra ao dono um componente nesta conversa, version 1. component \"choice\": cartão de escolha com título, resumo, pergunta e de uma a oito opções; o dono responde com uma opção, \"Nenhuma destas\" ou texto livre, a resposta chega como nova mensagem, escolher não autoriza ações nem vale como aprovação, e depois de chamar você encerra o turno e espera. component \"line_chart\", \"bar_chart\" ou \"pie_chart\": gráfico de leitura com os dados que você já tem (labels, de uma a três séries de números ou null, unit, period, sourceLabel; linha exige xKind category ou date; pizza tem uma série só, sem negativos nem null, e acima de cinco partes aparece em barras); o dono vê o gráfico e abre Ver dados para a tabela, o gráfico só mostra e não executa nada, e você segue o turno normalmente. Os valores são informados por você, sem prova de fonte: nunca invente números. Todo texto é dado inerte: sem HTML, código, URL, comando ou recurso externo. Até 64 KiB UTF-8. Só existe numa conversa adicional ou numa sessão extra anunciada pelo Hub; o Hub recusa como chamada vencida se o vínculo foi revogado ou a sessão de origem não está em andamento.",
        "annotations":{"readOnlyHint":false,"idempotentHint":false},
        "inputSchema": {
            "type":"object","additionalProperties":false,
            "required":["version","component","title","summary","data"],
            "properties": {
                "version":{"type":"integer","const":1},
                "component":{"type":"string","enum":components},
                "title":string(160),"summary":string(2000),
                "data":{"anyOf":data}
            }
        }
    })
}
