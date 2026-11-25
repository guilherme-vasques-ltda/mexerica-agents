//! Interpretação tipada do stream de mensagens. O adaptador aceita apenas
//! texto: bloco de ferramenta, uso ausente, parada anormal, evento desconhecido
//! e eco da credencial são recusas fechadas, não sucesso parcial.
use super::{CorpoBruto, Falha, Resposta, Segredo, Uso};
use serde_json::Value;

/// Teto do texto aceito, alinhado ao limite do trabalho de fundo.
const MAX_SAIDA_BYTES: usize = 256_000;
/// Teto de eventos: entrada patológica não roda sem fim.
const MAX_EVENTOS: usize = 10_000;
const MAX_MODELO: usize = 128;

/// Estado acumulado de um stream. Só vira [`Resposta`] se todas as exigências
/// fecharem.
#[derive(Default)]
struct Acumulado {
    modelo: Option<String>,
    entrada: Option<u64>,
    cache_lido: Option<u64>,
    cache_escrito: Option<u64>,
    texto: String,
    saida: Option<u64>,
    parada: Option<String>,
    terminou: bool,
}

/// Lê o corpo do stream e devolve o resultado tipado, ou a recusa.
pub(crate) fn interpretar(corpo: &CorpoBruto, chave: &Segredo) -> Result<Resposta, Falha> {
    let texto = std::str::from_utf8(corpo.bytes()).map_err(|_| Falha::RespostaInvalida)?;
    let mut acumulado = Acumulado::default();
    let mut dados = String::new();
    let mut nome: Option<String> = None;
    let mut eventos = 0usize;
    for linha in texto.split('\n') {
        let linha = linha.strip_suffix('\r').unwrap_or(linha);
        if linha.is_empty() {
            if !dados.is_empty() {
                eventos += 1;
                if eventos > MAX_EVENTOS {
                    return Err(Falha::Limite);
                }
                let valor: Value = serde_json::from_str(&dados).map_err(|_| Falha::RespostaInvalida)?;
                aplicar(&mut acumulado, &valor, nome.as_deref())?;
                dados.clear();
                nome = None;
            }
            continue;
        }
        if let Some(resto) = linha.strip_prefix("data:") {
            if !dados.is_empty() {
                dados.push('\n');
            }
            dados.push_str(resto.strip_prefix(' ').unwrap_or(resto));
        } else if let Some(resto) = linha.strip_prefix("event:") {
            nome = Some(resto.strip_prefix(' ').unwrap_or(resto).to_owned());
        }
    }
    if !dados.is_empty() {
        let valor: Value = serde_json::from_str(&dados).map_err(|_| Falha::RespostaInvalida)?;
        aplicar(&mut acumulado, &valor, nome.as_deref())?;
    }
    concluir(acumulado, chave)
}

fn aplicar(estado: &mut Acumulado, valor: &Value, nome: Option<&str>) -> Result<(), Falha> {
    let tipo = valor["type"].as_str().ok_or(Falha::RespostaInvalida)?;
    if nome.is_some_and(|declarado| declarado != tipo) {
        return Err(Falha::RespostaInvalida);
    }
    match tipo {
        "message_start" => {
            let mensagem = &valor["message"];
            let modelo = mensagem["model"].as_str().ok_or(Falha::RespostaInvalida)?;
            let valido = modelo.starts_with("claude-")
                && modelo.len() <= MAX_MODELO
                && modelo.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
            if !valido {
                return Err(Falha::RespostaInvalida);
            }
            estado.modelo = Some(modelo.to_owned());
            let uso = &mensagem["usage"];
            estado.entrada = Some(contagem(uso, "input_tokens")?);
            estado.cache_lido = Some(contagem(uso, "cache_read_input_tokens")?);
            estado.cache_escrito = Some(contagem(uso, "cache_creation_input_tokens")?);
        }
        "content_block_start" => {
            if valor["content_block"]["type"].as_str() != Some("text") {
                return Err(Falha::ConteudoNaoTexto);
            }
        }
        "content_block_delta" => {
            let delta = &valor["delta"];
            if delta["type"] != "text_delta" {
                return Err(Falha::ConteudoNaoTexto);
            }
            let pedaco = delta["text"].as_str().ok_or(Falha::RespostaInvalida)?;
            estado.texto.push_str(pedaco);
            if estado.texto.len() > MAX_SAIDA_BYTES {
                return Err(Falha::Limite);
            }
        }
        "content_block_stop" | "ping" => {}
        "message_delta" => {
            estado.parada = valor["delta"]["stop_reason"].as_str().map(str::to_owned);
            estado.saida = Some(contagem(&valor["usage"], "output_tokens")?);
        }
        "message_stop" => estado.terminou = true,
        "error" => return Err(Falha::Provedor { status: None }),
        _ => return Err(Falha::RespostaInvalida),
    }
    Ok(())
}

fn contagem(uso: &Value, campo: &str) -> Result<u64, Falha> {
    uso.get(campo).and_then(Value::as_u64).ok_or(Falha::UsoNaoConfirmado)
}

fn concluir(estado: Acumulado, chave: &Segredo) -> Result<Resposta, Falha> {
    if !estado.terminou {
        return Err(Falha::RespostaInvalida);
    }
    if estado.parada.as_deref() != Some("end_turn") {
        return Err(Falha::ParadaAnormal);
    }
    let (Some(modelo), Some(entrada), Some(cache_lido), Some(cache_escrito), Some(saida)) =
        (estado.modelo, estado.entrada, estado.cache_lido, estado.cache_escrito, estado.saida)
    else {
        return Err(Falha::UsoNaoConfirmado);
    };
    if estado.texto.contains(chave.expor()) {
        return Err(Falha::CredencialRepetida);
    }
    Ok(Resposta { modelo, texto: estado.texto, uso: Uso { entrada, saida, cache_lido, cache_escrito } })
}
