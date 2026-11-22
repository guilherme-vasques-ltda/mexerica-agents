use super::{Falha, Pedido};
use reqwest::{Method, Request, Url, header};
use serde::Serialize;
use serde_json::Value;
use std::time::{Duration, Instant};

const DESTINO: &str = "https://api.openai.com/v1/responses";
const MAX_CORPO: usize = 1024 * 1024;
const PRAZO: Duration = Duration::from_secs(60);

pub(super) struct CredencialFicticia(pub(super) &'static str);

impl CredencialFicticia {
    pub(super) fn valida(&self) -> bool {
        !self.0.is_empty() && self.0.bytes().all(|byte| byte.is_ascii_graphic())
    }
}

#[derive(Serialize)]
struct Responses<'a> {
    model: &'a str,
    input: [Mensagem<'a>; 1],
    store: bool,
    stream: bool,
}

#[derive(Serialize)]
struct Mensagem<'a> {
    role: &'static str,
    content: &'a str,
}

pub(super) fn requisicao(pedido: Pedido<'_>, credencial: &CredencialFicticia) -> Result<Request, Falha> {
    if !credencial.valida() {
        return Err(Falha::Recusado);
    }
    let mut requisicao = Request::new(Method::POST, Url::parse(DESTINO).map_err(|_| Falha::RespostaInvalida)?);
    let mut bearer =
        header::HeaderValue::from_str(&format!("Bearer {}", credencial.0)).map_err(|_| Falha::NaoAutorizado)?;
    bearer.set_sensitive(true);
    requisicao.headers_mut().insert(header::AUTHORIZATION, bearer);
    requisicao.headers_mut().insert(header::CONTENT_TYPE, header::HeaderValue::from_static("application/json"));
    *requisicao.timeout_mut() = Some(PRAZO);
    let corpo = Responses {
        model: pedido.modelo,
        input: [Mensagem { role: "user", content: pedido.texto }],
        store: false,
        stream: true,
    };
    *requisicao.body_mut() = Some(serde_json::to_vec(&corpo).map_err(|_| Falha::RespostaInvalida)?.into());
    Ok(requisicao)
}

pub(super) struct Resposta {
    pub status: u16,
    pub sse: bool,
    pub partes: Vec<Vec<u8>>,
}

pub(super) trait Transporte {
    fn enviar(&mut self, requisicao: Request) -> Result<Resposta, Falha>;
}

fn classificar(codigo: Option<&str>) -> Option<Falha> {
    match codigo? {
        "subscription_sharing_usage_limit_exceeded"
        | "subscription_sharing_workplace_individual_usage_limit_exceeded" => Some(Falha::Cota),
        "subscription_sharing_usage_unavailable" | "subscription_sharing_user_unavailable" => Some(Falha::Indisponivel),
        _ => None,
    }
}

pub(super) fn interpretar(resposta: Resposta) -> Result<String, Falha> {
    let inicio = Instant::now();
    if resposta.status != 200 {
        if resposta.status == 429 {
            return Err(Falha::Cota);
        }
        if (300..400).contains(&resposta.status) {
            return Err(Falha::RespostaInvalida);
        }
        let mut corpo = Vec::new();
        for parte in resposta.partes {
            if parte.len() > MAX_CORPO - corpo.len() || inicio.elapsed() >= PRAZO {
                return Err(Falha::RespostaInvalida);
            }
            corpo.extend(parte);
        }
        let codigo = serde_json::from_slice::<Value>(&corpo)
            .ok()
            .and_then(|v| classificar(v.pointer("/error/code").and_then(Value::as_str)));
        return Err(codigo.unwrap_or(match resposta.status {
            401 | 403 => Falha::NaoAutorizado,
            503 => Falha::Indisponivel,
            _ => Falha::RespostaInvalida,
        }));
    }
    if !resposta.sse {
        return Err(Falha::RespostaInvalida);
    }
    let mut linha = Vec::new();
    let mut dados = Vec::new();
    let mut tipo = None;
    let mut texto = String::new();
    let mut total = 0;
    for parte in resposta.partes {
        if parte.len() > MAX_CORPO - total || inicio.elapsed() >= PRAZO {
            return Err(Falha::RespostaInvalida);
        }
        total += parte.len();
        for byte in parte {
            if byte != b'\n' {
                linha.push(byte);
                continue;
            }
            let campo = linha.strip_suffix(b"\r").unwrap_or(&linha);
            if campo.is_empty() {
                if !dados.is_empty() {
                    let valor: Value = serde_json::from_slice(&dados).map_err(|_| Falha::RespostaInvalida)?;
                    let evento = valor["type"].as_str().ok_or(Falha::RespostaInvalida)?;
                    if tipo.as_deref().is_some_and(|t| t != evento.as_bytes()) {
                        return Err(Falha::RespostaInvalida);
                    }
                    match evento {
                        "response.created" | "response.in_progress" => {
                            if valor
                                .pointer("/response/output")
                                .and_then(Value::as_array)
                                .is_none_or(|out| !out.is_empty())
                            {
                                return Err(Falha::RespostaInvalida);
                            }
                        }
                        "response.output_item.added" | "response.output_item.done" if mensagem(&valor["item"]) => {}
                        "response.content_part.added" | "response.content_part.done"
                            if parte_de_texto(&valor["part"]) => {}
                        "response.output_text.delta" => {
                            texto.push_str(valor["delta"].as_str().ok_or(Falha::RespostaInvalida)?);
                        }
                        "response.output_text.done" if valor["text"].is_string() => {}
                        "response.completed" => {
                            let saida = valor
                                .pointer("/response/output")
                                .and_then(Value::as_array)
                                .ok_or(Falha::RespostaInvalida)?;
                            if valor.pointer("/response/status").and_then(Value::as_str) != Some("completed")
                                || saida.is_empty()
                                || !saida.iter().all(mensagem)
                                || texto.is_empty()
                            {
                                return Err(Falha::RespostaInvalida);
                            }
                            let confirmacao: String = saida
                                .iter()
                                .flat_map(|item| item["content"].as_array().into_iter().flatten())
                                .filter_map(|parte| parte["text"].as_str())
                                .collect();
                            if texto != confirmacao {
                                return Err(Falha::RespostaInvalida);
                            }
                            return Ok(texto);
                        }
                        "response.failed" | "error" | "response.incomplete" => {
                            let codigo = ["/response/error/code", "/error/code", "/code"]
                                .into_iter()
                                .filter_map(|path| valor.pointer(path).and_then(Value::as_str))
                                .find_map(|codigo| classificar(Some(codigo)));
                            return Err(codigo.unwrap_or(if evento == "response.incomplete" {
                                Falha::Incompleta
                            } else {
                                Falha::RespostaInvalida
                            }));
                        }
                        _ => return Err(Falha::RespostaInvalida),
                    }
                }
                dados.clear();
                tipo = None;
            } else if let Some(valor) = campo.strip_prefix(b"data:") {
                if !dados.is_empty() {
                    dados.push(b'\n');
                }
                dados.extend_from_slice(valor.strip_prefix(b" ").unwrap_or(valor));
            } else if let Some(valor) = campo.strip_prefix(b"event:") {
                tipo = Some(valor.strip_prefix(b" ").unwrap_or(valor).to_vec());
            } else if !campo.starts_with(b":") {
                return Err(Falha::RespostaInvalida);
            }
            linha.clear();
        }
    }
    Err(Falha::Incompleta)
}

fn parte_de_texto(parte: &Value) -> bool {
    parte["type"] == "output_text" && parte["text"].is_string()
}

fn mensagem(item: &Value) -> bool {
    item["type"] == "message"
        && item["role"] == "assistant"
        && item["content"].as_array().is_some_and(|partes| partes.iter().all(parte_de_texto))
}
