//! D133: a função do bot, um rótulo curto que a barra lateral mostra ao lado do nome.
//! Só texto de exibição: não concede gestão nem muda o papel no time (D119).

use crate::store::BotConfig;
use anyhow::{Result, bail};

pub(crate) const LIMITE: usize = 40;

/// Espaços repetidos viram um só; caracteres invisíveis ou de direção são recusados, para a
/// etiqueta não esconder texto nem se passar por outro rótulo. Grupos não têm função.
pub(crate) fn normalizar(cfg: &mut BotConfig) -> Result<()> {
    if cfg.is_group() {
        cfg.title.clear();
        return Ok(());
    }
    cfg.title = limpa(&cfg.title)?;
    Ok(())
}

/// A mesma regra para a criação, o `updateBot` e o editor do bot.
pub(crate) fn limpa(texto: &str) -> Result<String> {
    let title = texto.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.chars().any(invisivel) {
        bail!("A função do bot tem um caractere invisível. Apague-o e tente de novo.");
    }
    if title.chars().count() > LIMITE {
        bail!("A função do bot pode ter até {LIMITE} caracteres.");
    }
    Ok(title)
}

pub(crate) fn invisivel(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}')
        || matches!(c, '\u{2060}'..='\u{2069}' | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bot(title: &str) -> BotConfig {
        serde_json::from_value(serde_json::json!({"id": "b", "name": "Bot", "title": title})).unwrap()
    }

    #[test]
    fn a_funcao_vira_uma_linha_curta_e_visivel() {
        let mut cfg = bot("  Chefe \t de\n gabinete ");
        normalizar(&mut cfg).unwrap();
        assert_eq!(cfg.title, "Chefe de gabinete");

        for spoof in ["Vendas\u{202E}sotnemagap", "Ven\u{200B}das", "RH\u{0007}"] {
            assert!(normalizar(&mut bot(spoof)).is_err(), "{spoof:?}");
        }
        assert!(normalizar(&mut bot(&"a".repeat(LIMITE))).is_ok());
        assert!(normalizar(&mut bot(&"ã".repeat(LIMITE + 1))).is_err(), "o limite conta caracteres, não bytes");

        let mut group: BotConfig =
            serde_json::from_value(serde_json::json!({"id": "g", "kind": "group", "name": "Sala", "title": "Vendas"}))
                .unwrap();
        normalizar(&mut group).unwrap();
        assert_eq!(group.title, "");
    }
}
