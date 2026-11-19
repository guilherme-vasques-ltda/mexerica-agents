//! Text compared without case or accents: "Ação" and "acao" read the same.

/// Lowercase, without the accents of Latin letters (á, ç, ñ, ü…) or combining marks, so text typed with or without
/// accents, or stored decomposed, compares equal. Other scripts pass through unchanged.
pub fn fold(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).filter_map(base).collect()
}

fn base(ch: char) -> Option<char> {
    Some(match ch {
        // Combining marks: the accent of a letter stored decomposed.
        '\u{300}'..='\u{36F}' => return None,
        'à'..='å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'č' => 'c',
        'è'..='ë' | 'ē' | 'ė' | 'ę' | 'ě' => 'e',
        'ì'..='ï' | 'ī' | 'į' => 'i',
        'ñ' | 'ń' | 'ň' => 'n',
        'ò'..='ö' | 'ø' | 'ō' | 'ő' => 'o',
        'ù'..='ü' | 'ū' | 'ů' | 'ű' => 'u',
        'ý' | 'ÿ' => 'y',
        _ => ch,
    })
}

/// The words of `text`, folded, one space apart and padded with a space at each end, so a phrase matches only as
/// whole words: `words(t).contains(" voce agora e ")`.
pub fn words(text: &str) -> String {
    let folded: String = fold(text).chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    format!(" {} ", folded.split_whitespace().collect::<Vec<_>>().join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accents_and_case_fold_away() {
        assert_eq!(fold("Ação, CORAÇÃO e pão"), "acao, coracao e pao");
        assert_eq!(fold("Você é ótimo"), "voce e otimo");
        assert_eq!(fold("cafe\u{301}"), "cafe", "a decomposed accent");
        assert_eq!(fold("記住 Tainan"), "記住 tainan");
        assert_eq!(words("Você, agora... é!"), " voce agora e ");
    }
}
