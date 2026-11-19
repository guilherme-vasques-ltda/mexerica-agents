//! Ranking memory entries against the turn's request.
//!
//! Local and deterministic: folded terms, BM25 scoring, no model and no network. Used only when the opening
//! would cut entries, to choose which fit; it never changes the order entries are written in.

use crate::tokens;
use std::collections::BTreeMap;

/// Function words carry no signal for "what is this memory about"; without them every entry scores on "de"/"the".
/// Entries are the folded forms `terms` produces (no accents, no case).
const STOP: &[&str] = &[
    // Português
    "a", "o", "as", "os", "um", "uma", "uns", "umas", "de", "do", "da", "dos", "das", "em", "no", "na", "nos", "nas",
    "por", "para", "com", "sem", "e", "ou", "que", "se", "ao", "aos", "as", "e", "sao", "foi", "ser", "ter", "tem",
    "me", "te", "lhe", "nos", "meu", "minha", "seu", "sua", "este", "esta", "isso", "isto", "aquilo", "como", "quando",
    "onde", "qual", "quais", "quanto", "quanta", "ja", "ainda", "mais", "menos", "muito", "pouco", "tambem", "so",
    "apenas", "sobre", "entre", "ate", "apos", "antes", "depois", "hoje", "ontem", "amanha", "agora", "sempre",
    "nunca", "cada", "todo", "toda", "todos", "todas", "outro", "outra", "mesmo", "mesma", "bem", "mal", "sim", "nao",
    "voce", "voces", "ele", "ela", "eles", "elas", "vos", "meus", "minhas", "seus", "suas", "nosso", "nossa", "dele",
    "dela", "deles", "delas", "lhes", "era", "eram", "somos", "estou", "estamos", "estao", "estava", "estavam",
    "tenho", "temos", "tinha", "tinham", "vai", "vao", "ia", "iam", "fui", "fomos", "sera", "serao", "sido", "sendo",
    // Partículas gramaticais comuns do chinês: contam como termo solto e só geram ruído.
    "的", "了", "在", "是", "和", "与", "吗", "呢", "吧", "啊", "有", "没", "不", "这", "那", // English
    "the", "an", "and", "or", "but", "if", "then", "else", "when", "at", "by", "for", "with", "about", "into", "over",
    "after", "before", "between", "through", "during", "out", "off", "up", "down", "in", "on", "to", "of", "as", "it",
    "its", "this", "that", "these", "those", "i", "you", "he", "she", "we", "they", "him", "her", "us", "them", "my",
    "your", "his", "our", "their", "what", "which", "who", "whom", "not", "no", "yes", "so", "too", "very", "just",
    "only", "also", "than", "now", "here", "there", "all", "any", "both", "each", "few", "more", "most", "other",
    "some", "such", "own", "same", "how", "why", "where", "am", "is", "are", "was", "were", "be", "been", "being",
    "have", "has", "had", "do", "does", "did", "will", "would", "could", "should", "may", "might", "must", "can",
    "shall", // Español
    "el", "la", "los", "las", "un", "una", "unos", "unas", "del", "al", "en", "con", "sin", "y", "o", "es", "son",
    "fue", "estar", "esto", "eso", "cual", "cuales", "quien", "quienes", "cuanto", "cuanta", "ya", "aun", "muy",
    "solo", "hasta", "desde", "despues", "ayer", "manana", "usted", "ella", "ellos", "ellas", "les", "mi", "tu", "su",
    "mis", "tus", "sus", "nuestro", "nuestra",
];

fn stopwords() -> &'static std::collections::BTreeSet<&'static str> {
    static STOPWORDS: std::sync::OnceLock<std::collections::BTreeSet<&'static str>> = std::sync::OnceLock::new();
    STOPWORDS.get_or_init(|| STOP.iter().copied().collect())
}

/// The terms of `text` for scoring: folded (case and accents out), alphanumeric runs as words and each CJK
/// character on its own. Function words, runs of one character and CJK aside, carry no signal and are dropped.
pub(crate) fn terms(text: &str) -> Vec<String> {
    let folded = crate::text::fold(text);
    let mut terms = Vec::new();
    let mut word = String::new();
    for ch in folded.chars() {
        if tokens::is_cjk(ch) {
            if !word.is_empty() {
                terms.push(std::mem::take(&mut word));
            }
            terms.push(ch.to_string());
        } else if ch.is_alphanumeric() {
            word.push(ch);
        } else if !word.is_empty() {
            terms.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        terms.push(word);
    }
    let stop = stopwords();
    terms.retain(|term| {
        let len = term.chars().count();
        // Um dígito sozinho ainda diz algo ("sprint 5"); uma letra solta, não.
        let keep =
            len >= 2 || term.chars().all(tokens::is_cjk) || (len == 1 && term.bytes().all(|b| b.is_ascii_digit()));
        keep && !stop.contains(term.as_str())
    });
    terms
}

/// BM25 score of each text in `texts` against `query`, aligned with `texts`. All scores are zero when the
/// query has no usable term or every document is empty; ties are the caller's problem (stable sort).
// Counts are memory-entry sizes, far below where f64 loses integer precision.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn scores(query: &str, texts: &[&str]) -> Vec<f64> {
    const K1: f64 = 1.2;
    const B: f64 = 0.75;
    // Um pedido gigante não pode virar uma lista de termos sem fim: os primeiros bastam para orientar o corte.
    const MAX_TERMS: usize = 64;
    let wanted: std::collections::BTreeSet<String> = terms(query).into_iter().take(MAX_TERMS).collect();
    if wanted.is_empty() || texts.is_empty() {
        return vec![0.0; texts.len()];
    }
    let docs: Vec<Vec<String>> = texts.iter().map(|text| terms(text)).collect();
    let lens: Vec<usize> = docs.iter().map(Vec::len).collect();
    let n = docs.len() as f64;
    let avgdl = lens.iter().sum::<usize>() as f64 / n;
    if avgdl <= 0.0 {
        return vec![0.0; texts.len()];
    }
    // Term frequency per document, limited to query terms, and document frequency per term.
    let mut tfs: Vec<BTreeMap<String, usize>> = Vec::with_capacity(docs.len());
    let mut dfs: BTreeMap<String, usize> = BTreeMap::new();
    for doc in &docs {
        let mut tf: BTreeMap<String, usize> = BTreeMap::new();
        for term in doc {
            if wanted.contains(term) {
                *tf.entry(term.clone()).or_default() += 1;
            }
        }
        for term in tf.keys() {
            *dfs.entry(term.clone()).or_default() += 1;
        }
        tfs.push(tf);
    }
    let mut out = Vec::with_capacity(docs.len());
    for (tf, &dl) in tfs.iter().zip(&lens) {
        let dl = dl as f64;
        let mut score = 0.0;
        for (term, &freq) in tf {
            let df = dfs[term] as f64;
            let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
            let freq = freq as f64;
            score += idf * (freq * (K1 + 1.0)) / (freq + K1 * (1.0 - B + B * dl / avgdl));
        }
        out.push(score);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terms_fold_accents_and_case_and_drop_function_words() {
        assert_eq!(terms("Açougue do João, PORTO-ALEGRE!"), ["acougue", "joao", "porto", "alegre"]);
        assert_eq!(terms("São Paulo, 2024"), ["paulo", "2024"]);
    }

    #[test]
    fn terms_drop_single_letters_but_keep_digits_and_cjk() {
        assert_eq!(terms("a casa é 1"), ["casa", "1"]);
        assert_eq!(terms("你好世界"), ["你", "好", "世", "界"]);
        assert_eq!(terms("的了吗"), Vec::<String>::new(), "common particles carry no signal");
    }

    #[test]
    fn scores_reward_overlap_and_stay_deterministic() {
        let texts =
            ["o fornecedor entrega às sextas", "o cliente prefere café pela manhã", "o fornecedor atrasou de novo"];
        let got = scores("fornecedor", &texts);
        assert!(got[0] > got[1] && got[2] > got[1]);
        assert!(got[0] > 0.0 && got[1] == 0.0);
        assert_eq!(got, scores("fornecedor", &texts));
    }

    #[test]
    fn pt_br_terms_match_across_accents_and_plural_is_not_stemmed() {
        let scores = scores("entregas do fornecedor", &["o fornecedor entrega às sextas", "o cliente prefere café"]);
        assert!(scores[0] > 0.0 && scores[1] == 0.0);
    }
}
