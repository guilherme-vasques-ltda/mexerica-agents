//! Menção `@Nome` em texto livre — regra única compartilhada por escrita de
//! comentários (Projetos/Tarefas), notificação de avisos e leitura por bots.
//! Mesma semântica do cliente (`mentionsIn`): `@` precedido de borda de palavra
//! e nome seguido de borda; case-insensitive.

/// `true` quando `text` contém `@<name>` como token isolado.
pub(crate) fn text_mentions(text: &str, name: &str) -> bool {
    fn word(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }
    let name = name.trim().to_lowercase();
    if name.is_empty() {
        return false;
    }
    let lower = text.to_lowercase();
    let chars: Vec<(usize, char)> = lower.char_indices().collect();
    for (i, &(pos, c)) in chars.iter().enumerate() {
        if c != '@' || (i > 0 && word(chars[i - 1].1)) {
            continue;
        }
        let rest = &lower[pos + 1..];
        if !rest.starts_with(&name) {
            continue;
        }
        let tail = &rest[name.len()..];
        if tail.chars().next().is_none_or(|c| !word(c)) {
            return true;
        }
    }
    false
}
