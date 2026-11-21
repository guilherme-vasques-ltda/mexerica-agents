//! A máscara do código de sessão (`mxs_` e 64 dígitos hexadecimais).
//!
//! O código vale só enquanto o processo do OpenCode dura, mas não deve parar em log, em erro, no histórico nem no app.
//! A máscara passa em todo ponto em que texto do motor sai do processo do Hub, sempre sobre o texto já interpretado e
//! já juntado: a linha de saída, cada texto do JSON depois do parse (escapes já desfeitos), o texto de um segmento de
//! conversa montado de vários pedaços e a saída de uma ferramenta.
use serde_json::Value;
use std::borrow::Cow;

const MARCA: &str = "mxs_";
const DIGITOS: usize = 64;
const OCULTO: &str = "mxs_[oculto]";

/// Troca cada código inteiro por `mxs_[oculto]`.
pub(crate) fn mascarar_codigo(texto: &str) -> Cow<'_, str> {
    if !texto.contains(MARCA) {
        return Cow::Borrowed(texto);
    }
    let mut saida = String::with_capacity(texto.len());
    let mut resto = texto;
    while let Some(posicao) = resto.find(MARCA) {
        saida.push_str(&resto[..posicao]);
        let depois = &resto[posicao + MARCA.len()..];
        if depois.bytes().take_while(u8::is_ascii_hexdigit).count() >= DIGITOS {
            saida.push_str(OCULTO);
            resto = &depois[DIGITOS..];
        } else {
            saida.push_str(MARCA);
            resto = depois;
        }
    }
    saida.push_str(resto);
    Cow::Owned(saida)
}

/// Mascara os textos de um JSON já interpretado, em qualquer profundidade — os valores e também
/// os nomes (chaves) dos campos: um código pode vir como nome, e quem lê o JSON de novo (ou o
/// imprime) não pode vê-lo.
pub(crate) fn mascarar_json(valor: &mut Value) {
    match valor {
        Value::String(texto) => {
            if let Cow::Owned(novo) = mascarar_codigo(texto) {
                *texto = novo;
            }
        }
        Value::Array(itens) => itens.iter_mut().for_each(mascarar_json),
        Value::Object(campos) => {
            let reconstruido: serde_json::Map<String, Value> = std::mem::take(campos)
                .into_iter()
                .map(|(nome, mut valor)| {
                    mascarar_json(&mut valor);
                    let nome = match mascarar_codigo(&nome) {
                        Cow::Borrowed(_) => nome,
                        Cow::Owned(mascarada) => mascarada,
                    };
                    (nome, valor)
                })
                .collect();
            *campos = reconstruido;
        }
        _ => {}
    }
}

/// O texto de um segmento de conversa montado de vários pedaços, pronto para publicar. Com `fim` falso o texto ainda
/// pode crescer: o fim que pode ser o começo de um código (`mxs`, `mxs_ab1`...) fica de fora até o próximo pedaço, para
/// um código partido entre dois pedaços nunca sair pela metade. Com `fim` verdadeiro o que sobra não é código e segue.
pub(crate) fn mascarar_saida(texto: &str, fim: bool) -> Cow<'_, str> {
    let mascarado = mascarar_codigo(texto);
    if fim {
        return mascarado;
    }
    let corte = (mascarado.len().saturating_sub(MARCA.len() + DIGITOS)..mascarado.len())
        .filter(|&i| mascarado.is_char_boundary(i))
        .find(|&i| comeco_de_codigo(&mascarado[i..]));
    match (corte, mascarado) {
        (None, mascarado) => mascarado,
        (Some(corte), Cow::Borrowed(texto)) => Cow::Borrowed(&texto[..corte]),
        (Some(corte), Cow::Owned(mut texto)) => {
            texto.truncate(corte);
            Cow::Owned(texto)
        }
    }
}

/// Se o texto é o começo (ainda incompleto) de um código.
fn comeco_de_codigo(texto: &str) -> bool {
    match texto.strip_prefix(MARCA) {
        None => texto.len() < MARCA.len() && MARCA.starts_with(texto),
        Some(resto) => resto.len() < DIGITOS && resto.bytes().all(|b| b.is_ascii_hexdigit()),
    }
}
