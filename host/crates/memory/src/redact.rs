//! Credentials taken out of text memory keeps or reads: what a bot or dreaming adds to long-form notes, and the chat
//! history dreaming reads. An entry with one is refused instead ([`crate::check`]); a note keeps the rest of its text
//! with [`MARK`] in the credential's place.
//!
//! A neutral mark keeps the label (`password: [removido]`); a word boundary before `sk-`, so "desk-" isn't a key; environment
//! variables in capitals only, and with them header fields, only when the value looks like a secret (eight characters
//! or more, one a digit), so `primary_key: id` stays; no credential-file paths (`~/.ssh`), which are no secret in
//! notes about a person; and Brazilian Portuguese labels, each only with its value.

use regex::{Captures, Regex};
use std::collections::HashSet;
use std::sync::LazyLock;

/// What a credential becomes. No check reads it as a secret, so a note redacted once passes them.
pub const MARK: &str = "[removido]";

/// What the `secret` group of a pattern must look like before it counts.
#[derive(Clone, Copy)]
enum Value {
    /// The shape is enough.
    Any,
    /// Eight characters or more, one a digit: `primary_key: id` isn't one.
    Opaque,
    /// That, or a numeric code of 4 to 8 digits (a PIN, a code sent by SMS): "senha do Netflix: a mesma do e-mail"
    /// isn't one.
    Code,
}

/// A credential's shape and what its value must look like. Where the pattern names a `secret` group only that part
/// is replaced; a value never starts with `[`, so the mark isn't matched again. More specific patterns first.
const PATTERNS: &[(&str, Value)] = &[
    // Bearer-style tokens.
    (r"(?i)\bbearer\s+(?P<secret>[A-Za-z0-9._\-+/=]{16,})", Value::Any),
    // Vendor-prefixed API keys (Anthropic, OpenAI, OpenRouter).
    (r"\bsk-[A-Za-z0-9_\-]{16,}", Value::Any),
    // Stripe secret and restricted keys.
    (r"(?:sk|rk)_live_[A-Za-z0-9_\-]{16,}", Value::Any),
    // Every GitHub token prefix, and fine-grained tokens.
    (r"gh[pousr]_[A-Za-z0-9]{20,}", Value::Any),
    (r"github_pat_[A-Za-z0-9_]{20,}", Value::Any),
    // AWS access-key ids, long-lived and temporary, anchored to their exact length.
    (r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b", Value::Any),
    // Google / Gemini API keys, old and new shapes, and OAuth refresh tokens.
    (r"AIza[A-Za-z0-9_\-]{30,}", Value::Any),
    (r"AQ\.Ab[A-Za-z0-9_\-]{30,}", Value::Any),
    (r"1//[0-9A-Za-z_\-]{20,}", Value::Any),
    // Meta / Facebook Graph API access tokens.
    (r"EAA[A-Za-z0-9]{20,}", Value::Any),
    // Telegram bot tokens: <bot-id>:<secret>.
    (r"\b\d{6,10}:(?:AA[A-Za-z0-9_\-]{30,}|[A-Za-z0-9_\-]{34,35})\b", Value::Any),
    // GoHighLevel private integration tokens.
    (r"pit-[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}", Value::Any),
    // Slack tokens.
    (r"xox[abprs]-[A-Za-z0-9\-]{10,}", Value::Any),
    (r"xapp-[A-Za-z0-9\-]{10,}", Value::Any),
    // JWTs: three base64url segments.
    (r"eyJ[A-Za-z0-9_\-]{16,}\.[A-Za-z0-9_\-]{16,}\.[A-Za-z0-9_\-]{16,}", Value::Any),
    // A base64 JSON object with no dots (Cloudflare tunnel tokens and the like).
    (r"eyJ[A-Za-z0-9_\-+/]{40,}={0,2}", Value::Any),
    // PEM private key blocks.
    (r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----", Value::Any),
    // The password in a URL: scheme://user:pass@host.
    (r"[a-zA-Z][a-zA-Z0-9+\-.]*://[^:/\s]+:(?P<secret>[^@\s\[][^@\s]*)@[^\s]+", Value::Any),
    // Auth-bearing headers and fields (`X-Api-Key: …`, `password=…`).
    (
        r#"(?i)\b[A-Za-z0-9_-]*(?:authentication|authorization|credentials?|password|passwd|apikey|accountkey|authtoken|[a-z0-9]*(?:api|auth|access|secret|security|private|session|refresh|client|consumer|subscription|app|bearer)-(?:key|token))"?\s*[=:]\s*(?:(?:basic|bearer|digest|token|apikey)\s+)?"?(?P<secret>[A-Za-z0-9._~+/=-]{8,})"#,
        Value::Opaque,
    ),
    // Provider environment variables, whatever their value looks like.
    (
        r#"\b(?:ANTHROPIC_API_KEY|OPENAI_API_KEY|OPENROUTER_API_KEY|VOYAGE_API_KEY|MISTRAL_API_KEY|GROQ_API_KEY|HF_TOKEN|HUGGINGFACE_TOKEN|AWS_(?:SECRET_)?ACCESS_KEY[A-Z_]*|GITHUB_TOKEN|GH_TOKEN|GITLAB_TOKEN|GOOGLE_API_KEY|GEMINI_API_KEY|OLLAMA_API_KEY)"?\s*[=:]\s*"?(?P<secret>[^\s"\[][^\s"]*)"#,
        Value::Any,
    ),
    // Any *_KEY, *_TOKEN, *_SECRET, *_PASSWORD or *_CREDENTIAL(S) environment variable, and the suffixes one step
    // outside those.
    (
        r#"\b[A-Z][A-Z0-9_]*_(?:KEY|TOKEN|SECRET|PASSWORD|PASSWD|CREDENTIALS?|PRIVATE_KEY|KEY_ID|PASSPHRASE|SIGNING_KEY|PEPPER|SALT)"?\s*[=:]\s*"?(?P<secret>[^\s"\[][^\s"]*)"#,
        Value::Opaque,
    ),
    // Mexerica, Brazilian Portuguese. A password, token or code with its value: "senha do wi-fi: …", "token = …".
    // Up to four words between, but not the conjunction "e" ("a senha e o e-mail do banco: Itaú" names no password).
    (
        r"(?i)\b(?:senha|token|c[oó]digo\s+de\s+(?:verifica[cç][aã]o|seguran[cç]a))(?:\s+(?:[^\s:=]{2,}|[^\s:=e])){0,4}\s*[:=]\s*(?P<secret>[^\s,;\[]+)",
        Value::Code,
    ),
    // "a senha do wi-fi é …", when what follows "é" has a digit ("a senha é forte" isn't one).
    (
        r"(?i)\b(?:senha|token|c[oó]digo\s+de\s+(?:verifica[cç][aã]o|seguran[cç]a))(?:\s+(?:[^\s:=]{2,}|[^\s:=eé])){0,4}?\s+é\s+(?P<secret>[^\s,;\[]*\d[^\s,;]*)",
        Value::Code,
    ),
    // A Pix key: an e-mail, a phone, a CPF or CNPJ, or a random key, after "chave pix".
    (
        r"(?i)\bchave\s+(?:do\s+)?pix\b(?:\s+(?:[^\s:=@]{2,}|[^\s:=@eé])){0,4}?(?:\s*[:=]|\s+é)?\s*(?P<secret>[\w.+-]+@[\w-]+(?:\.[\w-]+)+|\+?\(?\d[\d\s().-]{8,}\d|[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})",
        Value::Any,
    ),
    // A bank account with its check digit, or a long number, right after "conta" ("conta de luz" isn't one).
    (
        r"(?i)\b(?:conta|c/c)(?:\s+(?:corrente|poupan[cç]a))?(?:\s+n[º°o.]?)?\s*:?\s*(?P<secret>\d{3,12}-[\dxX]\b|\d{6,12}\b)",
        Value::Any,
    ),
];

static COMPILED: LazyLock<Vec<(Regex, Value)>> = LazyLock::new(|| {
    PATTERNS
        .iter()
        .map(|(pattern, value)| (Regex::new(pattern).expect("the built-in patterns compile"), *value))
        .collect()
});

impl Value {
    fn fits(self, value: &str) -> bool {
        let opaque = value.chars().count() >= 8 && value.bytes().any(|b| b.is_ascii_digit());
        let code = (4..=8).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_digit());
        match self {
            Self::Any => true,
            Self::Opaque => opaque,
            Self::Code => opaque || code,
        }
    }
}

/// The part of a match to replace, if it is a credential.
fn credential<'t>(caps: &Captures<'t>, value: Value) -> Option<regex::Match<'t>> {
    match caps.name("secret") {
        Some(secret) => value.fits(secret.as_str()).then_some(secret),
        None => caps.get(0),
    }
}

/// `text` with every credential replaced by [`MARK`].
pub fn redact(text: &str) -> String {
    let mut out = text.to_owned();
    for (pattern, value) in COMPILED.iter() {
        if !pattern.is_match(&out) {
            continue;
        }
        out = pattern
            .replace_all(&out, |caps: &Captures<'_>| {
                let whole = caps.get(0).expect("a match");
                match credential(caps, *value) {
                    Some(secret) => format!(
                        "{}{MARK}{}",
                        &whole.as_str()[..secret.start() - whole.start()],
                        &whole.as_str()[secret.end() - whole.start()..]
                    ),
                    None => whole.as_str().to_owned(),
                }
            })
            .into_owned();
    }
    out
}

/// Whether `text` holds a credential [`redact`] would take out.
pub fn finds(text: &str) -> bool {
    COMPILED.iter().any(|(pattern, value)| pattern.captures_iter(text).any(|caps| credential(&caps, *value).is_some()))
}

/// `after` with credentials taken out only of what a writer added: lines not in `before`, a run of them at a time
/// (so a key over several lines is found). Lines that were already there stay exactly as they were.
pub fn added(before: Option<&str>, after: &str) -> String {
    let kept: HashSet<&str> = before.map(|text| text.lines().collect()).unwrap_or_default();
    let mut out = String::with_capacity(after.len());
    let mut run = String::new();
    for line in after.split_inclusive('\n') {
        let bare = line.strip_suffix('\n').map_or(line, |l| l.strip_suffix('\r').unwrap_or(l));
        if kept.contains(bare) {
            out.push_str(&redact(&run));
            run.clear();
            out.push_str(line);
        } else {
            run.push_str(line);
        }
    }
    out.push_str(&redact(&run));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixtures are fake shapes, not real credentials.
    #[test]
    fn takes_out_credentials_and_keeps_the_rest() {
        for (input, output) in [
            ("chave sk-proj-abcdefghijklmnop1234", "chave [removido]"),
            ("Authorization: Bearer abcdefghijklmnop.qrstuv", "Authorization: Bearer [removido]"),
            ("banco em postgres://app:s3nh4F0rte@db.local/app", "banco em postgres://app:[removido]@db.local/app"),
            ("OPENAI_API_KEY=qualquer-coisa", "OPENAI_API_KEY=[removido]"),
            ("MY_SERVICE_TOKEN=abc123def456", "MY_SERVICE_TOKEN=[removido]"),
            ("password: hunter22x", "password: [removido]"),
            ("-----BEGIN RSA PRIVATE KEY-----\nMIIabc\n-----END RSA PRIVATE KEY-----", "[removido]"),
            ("senha do wifi: hunter22", "senha do wifi: [removido]"),
            ("A senha do wi-fi é hunter22, na sala", "A senha do wi-fi é [removido], na sala"),
            ("Chave Pix: maria@example.com", "Chave Pix: [removido]"),
            ("Banco Itaú, agência 1234, conta 56789-0", "Banco Itaú, agência 1234, conta [removido]"),
            ("Token do banco: 482913", "Token do banco: [removido]"),
        ] {
            assert_eq!(redact(input), output, "{input}");
            assert!(finds(input), "{input}");
            assert!(!finds(output), "redacting again changes nothing: {output}");
        }
    }

    #[test]
    fn leaves_what_only_looks_like_one() {
        for text in [
            "Projeto desk-organizer-premium-2026, prazo 10/11",
            "primary_key: id, partition_key = tenant_id",
            "PARTITION_KEY = tenant_id",
            "authorization: required",
            "Preciso trocar a senha e o e-mail do banco: Itaú",
            "A senha do banco é longa",
            "Prefere pagar por chave pix",
            "Paga a diarista por pix; telefone dela (11) 98765-4321",
            "Conta de luz em nome da esposa, CEP 01310-100",
            "Cartão Nubank: central 0800 591 2117",
            "Senha do Netflix: a mesma do e-mail",
            "Lembrete: trocar senha do banco: até sexta",
            "Token do banco: chega por SMS",
        ] {
            assert!(!finds(text), "{text}");
            assert_eq!(redact(text), text);
        }
    }

    #[test]
    fn only_what_a_writer_added_is_redacted() {
        let before = "---\ndescription: casa\n---\nsenha antiga: hunter11\n";
        let after = "---\ndescription: casa\n---\nsenha antiga: hunter11\nsenha nova: hunter22\n";
        assert_eq!(added(Some(before), after), format!("{before}senha nova: [removido]\n"));
        assert_eq!(added(None, "token: abc12345\n"), "token: [removido]\n");
    }
}
