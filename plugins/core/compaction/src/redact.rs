//! Masks secrets in the transcript text before it is sent to a model (setting `remote_compaction.redact_secrets`).

/// What replaces a masked secret.
const MASK: &str = "[REDACTED]";

/// Prefixes of well-known API token formats.
const TOKEN_PREFIXES: [&str; 9] = [
    "sk-", "ghp_", "gho_", "ghs_", "xoxb-", "xoxp-", "AKIA", "AIza", "glpat-",
];

/// Names whose assigned value is a secret (`name=value` or `name: value`).
const SECRET_NAMES: [&str; 6] = ["password", "passwd", "secret", "api_key", "apikey", "token"];

/// Shortest text that counts as a token body.
const MINIMUM_TOKEN_CHARS: usize = 16;

fn is_token_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '+' | '/' | '=')
}

fn token_secret(word: &str) -> bool {
    word.len() >= MINIMUM_TOKEN_CHARS
        && word.chars().all(is_token_char)
        && TOKEN_PREFIXES.iter().any(|prefix| word.starts_with(prefix))
}

fn assigned_secret(word: &str) -> Option<usize> {
    let separator = word.find(['=', ':'])?;
    let name = word[..separator].trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_');
    let value = &word[separator + 1..];
    let lowered = name.to_ascii_lowercase();
    let named = SECRET_NAMES.iter().any(|secret| lowered.ends_with(secret));
    (named && value.len() >= 4).then_some(separator + 1)
}

fn mask_word(word: &str) -> String {
    let core = word.trim_matches(|c: char| !is_token_char(c));
    if token_secret(core) {
        return word.replacen(core, MASK, 1);
    }
    match assigned_secret(word) {
        Some(value_start) => format!("{}{MASK}", &word[..value_start]),
        None => word.to_string(),
    }
}

/// `text` with API tokens, bearer tokens and `password=...` style values masked; whitespace is kept.
pub fn redact(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut after_bearer = false;
    let mut word = String::new();
    let flush = |word: &mut String, output: &mut String, after_bearer: &mut bool| {
        if word.is_empty() {
            return;
        }
        if *after_bearer && word.len() >= 8 {
            output.push_str(MASK);
        } else {
            output.push_str(&mask_word(word));
        }
        *after_bearer = word.eq_ignore_ascii_case("bearer");
        word.clear();
    };
    for character in text.chars() {
        if character.is_whitespace() {
            flush(&mut word, &mut output, &mut after_bearer);
            output.push(character);
        } else {
            word.push(character);
        }
    }
    flush(&mut word, &mut output, &mut after_bearer);
    output
}
