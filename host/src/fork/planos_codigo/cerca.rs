//! A cerca da chave: reconhece a chave de um plano numa resposta mesmo quando o fornecedor a devolve disfarçada.
//!
//! Além dos bytes literais, procura as formas que um cliente reconstitui sozinho: escapes de JSON (`\uXXXX`, `\/`,
//! misturados com letras comuns) e base64 (alfabeto padrão e o de URL, nos três alinhamentos possíveis). O limite que sobra:
//! uma chave partida em eventos separados de um mesmo fluxo (o cliente junta os pedaços) ou escrita por outra
//! codificação (hexadecimal, rot13, texto invertido) não é reconhecida. A resposta de um fornecedor honesto nunca traz a
//! chave; isto fecha o eco acidental e o mais comum dos disfarces, não um fornecedor que queira vazá-la.
use base64::{Engine, engine::general_purpose::STANDARD_NO_PAD};
use zeroize::Zeroizing;

pub(super) struct Cerca {
    formas: Vec<Zeroizing<Vec<u8>>>,
}

impl Cerca {
    pub(super) fn nova(chave: &str) -> Self {
        let mut formas = vec![Zeroizing::new(chave.as_bytes().to_vec())];
        let bytes = chave.as_bytes();
        for deslocamento in 0..3usize {
            let mut alinhada = vec![0u8; deslocamento];
            alinhada.extend_from_slice(bytes);
            let codificada = STANDARD_NO_PAD.encode(&alinhada);
            let (inicio, fim) = ((8 * deslocamento).div_ceil(6), (8 * alinhada.len()) / 6);
            if fim <= inicio + 7 {
                continue;
            }
            let padrao = codificada.as_bytes()[inicio..fim].to_vec();
            let url: Vec<u8> = padrao
                .iter()
                .map(|&b| match b {
                    b'+' => b'-',
                    b'/' => b'_',
                    outro => outro,
                })
                .collect();
            formas.push(Zeroizing::new(padrao));
            formas.push(Zeroizing::new(url));
        }
        formas.dedup_by(|a, b| a.as_slice() == b.as_slice());
        Self { formas }
    }

    /// Se a janela traz alguma forma da chave, como veio ou com os escapes de JSON desfeitos.
    pub(super) fn contem(&self, bruto: &[u8]) -> bool {
        let (decodificado, _, _) = decodificar(bruto);
        self.formas.iter().any(|forma| tem(bruto, forma) || tem(&decodificado, forma))
    }

    /// Quantos bytes do fim da janela ainda podem ser o começo de uma forma da chave (ou um escape pela metade): esses
    /// ficam retidos até o próximo pedaço, e o resto segue na hora.
    pub(super) fn retido(&self, bruto: &[u8]) -> usize {
        let (decodificado, origem, incompleto) = decodificar(bruto);
        let mut reter = bruto.len() - incompleto;
        for forma in &self.formas {
            let teto = forma.len().saturating_sub(1);
            for m in (1..=teto.min(bruto.len())).rev() {
                if bruto.ends_with(&forma[..m]) {
                    reter = reter.max(m);
                    break;
                }
            }
            for m in (1..=teto.min(decodificado.len())).rev() {
                if decodificado.ends_with(&forma[..m]) {
                    reter = reter.max(bruto.len() - origem[decodificado.len() - m]);
                    break;
                }
            }
        }
        reter
    }
}

fn tem(janela: &[u8], agulha: &[u8]) -> bool {
    !agulha.is_empty() && janela.windows(agulha.len()).any(|parte| parte == agulha)
}

/// Desfaz os escapes de JSON. Devolve os bytes decodificados, de onde (no texto bruto) cada um veio, e onde começa um
/// escape ainda incompleto no fim (o tamanho do bruto, se não há).
fn decodificar(bruto: &[u8]) -> (Vec<u8>, Vec<usize>, usize) {
    let mut saida = Vec::with_capacity(bruto.len());
    let mut origem = Vec::with_capacity(bruto.len());
    let mut i = 0;
    while i < bruto.len() {
        if bruto[i] != b'\\' {
            saida.push(bruto[i]);
            origem.push(i);
            i += 1;
            continue;
        }
        let Some(&marca) = bruto.get(i + 1) else { return (saida, origem, i) };
        let simples = match marca {
            b'n' => Some(b'\n'),
            b't' => Some(b'\t'),
            b'r' => Some(b'\r'),
            b'b' => Some(8),
            b'f' => Some(12),
            b'"' | b'\\' | b'/' => Some(marca),
            _ => None,
        };
        if let Some(byte) = simples {
            saida.push(byte);
            origem.push(i);
            i += 2;
            continue;
        }
        if marca == b'u' {
            if i + 6 > bruto.len() {
                return (saida, origem, i);
            }
            let digitos = &bruto[i + 2..i + 6];
            if digitos.iter().all(u8::is_ascii_hexdigit) {
                let ponto = u32::from_str_radix(std::str::from_utf8(digitos).unwrap_or("0"), 16).unwrap_or(0xfffd);
                let letra = char::from_u32(ponto).unwrap_or('\u{fffd}');
                let mut buffer = [0u8; 4];
                for &byte in letra.encode_utf8(&mut buffer).as_bytes() {
                    saida.push(byte);
                    origem.push(i);
                }
                i += 6;
                continue;
            }
        }
        saida.push(b'\\');
        origem.push(i);
        i += 1;
    }
    (saida, origem, bruto.len())
}
