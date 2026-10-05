//! Secret redaction for text that leaves the machine inside a summarizer
//! request. A hand-written scanner (no regex dependency) for the common
//! credential shapes; it prefers missing an exotic secret over mangling
//! ordinary text, and counts every replacement.

use serde_json::Value;

pub(crate) const REDACTED: &str = "[REDACTED]";

const PEM_BEGIN: &str = "-----BEGIN ";
const PEM_END_MARKER: &str = "-----END ";

/// Identifier suffixes whose assigned value is treated as a secret.
const SECRET_KEY_SUFFIXES: [&str; 11] = [
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "api-key",
    "secret_key",
    "access_key",
    "private_key",
    "auth_key",
];

/// Token shapes: prefix, minimum count of token characters after it.
const TOKEN_PREFIXES: [(&str, usize); 11] = [
    ("sk-", 16),
    ("ghp_", 20),
    ("gho_", 20),
    ("ghs_", 20),
    ("ghu_", 20),
    ("ghr_", 20),
    ("github_pat_", 20),
    ("xoxb-", 10),
    ("xoxp-", 10),
    ("xoxa-", 10),
    ("xoxs-", 10),
];

fn is_token_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/' | b'+' | b'=')
}

fn is_word_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn run_length(bytes: &[u8], start: usize, accepts: impl Fn(u8) -> bool) -> usize {
    bytes[start..]
        .iter()
        .take_while(|byte| accepts(**byte))
        .count()
}

/// Length of a secret token starting at `at`, when one starts there.
fn token_secret_len(text: &str, at: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if at > 0 && is_word_char(bytes[at - 1]) {
        return None;
    }
    let rest = &text[at..];
    for (prefix, minimum) in TOKEN_PREFIXES {
        if rest.starts_with(prefix) {
            let body = run_length(bytes, at + prefix.len(), is_token_char);
            if body >= minimum {
                return Some(prefix.len() + body);
            }
        }
    }
    // AWS access key ids and Google API keys have a fixed shape.
    for (prefix, body_len, upper_only) in
        [("AKIA", 16, true), ("ASIA", 16, true), ("AIza", 35, false)]
    {
        if rest.starts_with(prefix) {
            let body = run_length(bytes, at + prefix.len(), |byte| {
                byte.is_ascii_uppercase()
                    || byte.is_ascii_digit()
                    || (!upper_only && (byte.is_ascii_lowercase() || matches!(byte, b'_' | b'-')))
            });
            if body >= body_len {
                return Some(prefix.len() + body_len);
            }
        }
    }
    None
}

/// `Bearer <token>`: returns the (offset, length) of the token part.
fn bearer_secret(text: &str, at: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    if at > 0 && is_word_char(bytes[at - 1]) {
        return None;
    }
    let rest = &text[at..];
    let keyword_len = "Bearer ".len();
    if !rest.get(..keyword_len)?.eq_ignore_ascii_case("bearer ") {
        return None;
    }
    let start = at + keyword_len;
    let body = run_length(bytes, start, is_token_char);
    (body >= 16).then_some((keyword_len, body))
}

/// An assignment such as `api_key = "..."`: returns (value offset, value
/// length) relative to `at`, which must be the start of the identifier.
fn assignment_secret(text: &str, at: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    if at > 0 && is_word_char(bytes[at - 1]) {
        return None;
    }
    let identifier_len = run_length(bytes, at, |byte| is_word_char(byte) || byte == b'-');
    if identifier_len == 0 {
        return None;
    }
    let identifier = text[at..at + identifier_len].to_ascii_lowercase();
    if !SECRET_KEY_SUFFIXES
        .iter()
        .any(|suffix| identifier.ends_with(suffix))
    {
        return None;
    }
    let mut cursor = at + identifier_len;
    if matches!(bytes.get(cursor), Some(b'"' | b'\'')) {
        cursor += 1;
    }
    cursor += run_length(bytes, cursor, |byte| byte == b' ' || byte == b'\t');
    if !matches!(bytes.get(cursor), Some(b'=' | b':')) {
        return None;
    }
    cursor += 1;
    // `==` and `:=` style comparisons are not assignments of a literal.
    if matches!(bytes.get(cursor), Some(b'=')) {
        return None;
    }
    cursor += run_length(bytes, cursor, |byte| byte == b' ' || byte == b'\t');
    let quote = match bytes.get(cursor) {
        Some(&quote @ (b'"' | b'\'')) => {
            cursor += 1;
            Some(quote)
        }
        _ => None,
    };
    let value_len = match quote {
        Some(quote) => run_length(bytes, cursor, |byte| byte != quote && byte != b'\n'),
        None => run_length(bytes, cursor, |byte| {
            !byte.is_ascii_whitespace() && !matches!(byte, b',' | b';' | b'"' | b'\'' | b')' | b'}')
        }),
    };
    let value = &text[cursor..cursor + value_len];
    let is_placeholder = value.starts_with(['$', '{', '<', '*'])
        || value.starts_with("[REDACTED")
        || value.eq_ignore_ascii_case("none")
        || value.eq_ignore_ascii_case("null")
        || value.eq_ignore_ascii_case("true")
        || value.eq_ignore_ascii_case("false");
    if value_len < 6 || is_placeholder {
        return None;
    }
    Some((cursor - at, value_len))
}

/// A PEM block from `-----BEGIN ` to the end of its `-----END ...-----` line
/// (or to the end of the text when unterminated). Returns the block length.
fn pem_block_len(text: &str, at: usize) -> Option<usize> {
    let rest = &text[at..];
    if !rest.starts_with(PEM_BEGIN) {
        return None;
    }
    let header_end = rest[PEM_BEGIN.len()..].find("-----")? + PEM_BEGIN.len() + 5;
    if !rest[..header_end].contains("PRIVATE KEY") {
        return None;
    }
    let Some(end_marker) = rest[header_end..].find(PEM_END_MARKER) else {
        return Some(rest.len());
    };
    let after_marker = header_end + end_marker + PEM_END_MARKER.len();
    let closing = rest[after_marker..].find("-----")? + after_marker + 5;
    Some(closing)
}

/// Replaces secrets in `text`; returns the new text and the replacement count.
pub(crate) fn redact_text(text: &str) -> (String, usize) {
    let mut output = String::with_capacity(text.len());
    let mut count = 0;
    let mut index = 0;
    while index < text.len() {
        if let Some(length) = pem_block_len(text, index) {
            output.push_str(REDACTED);
            count += 1;
            index += length;
            continue;
        }
        if let Some(length) = token_secret_len(text, index) {
            output.push_str(REDACTED);
            count += 1;
            index += length;
            continue;
        }
        if let Some((offset, length)) = bearer_secret(text, index) {
            output.push_str(&text[index..index + offset]);
            output.push_str(REDACTED);
            count += 1;
            index += offset + length;
            continue;
        }
        if let Some((offset, length)) = assignment_secret(text, index) {
            output.push_str(&text[index..index + offset]);
            output.push_str(REDACTED);
            count += 1;
            index += offset + length;
            continue;
        }
        let Some(character) = text[index..].chars().next() else {
            break;
        };
        output.push(character);
        index += character.len_utf8();
    }
    (output, count)
}

/// Redacts every string inside a JSON value in place.
pub(crate) fn redact_value(value: &mut Value) -> usize {
    match value {
        Value::String(text) => {
            let (redacted, count) = redact_text(text);
            if count > 0 {
                *text = redacted;
            }
            count
        }
        Value::Array(items) => items.iter_mut().map(redact_value).sum(),
        Value::Object(map) => map.values_mut().map(redact_value).sum(),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redacted(text: &str) -> (String, usize) {
        redact_text(text)
    }

    #[test]
    fn token_shapes_are_replaced_and_counted() {
        let (text, count) = redacted(
            "key sk-abcdefghijklmnop1234 and ghp_abcdefghijklmnopqrstuv and AKIAABCDEFGHIJKLMNOP end",
        );
        assert_eq!(count, 3);
        assert_eq!(text, "key [REDACTED] and [REDACTED] and [REDACTED] end");
    }

    #[test]
    fn slack_and_google_keys_are_replaced() {
        let (text, count) =
            redacted("xoxb-1234567890-abcdef AIzaSyA1234567890abcdefghijklmnopqrstuv");
        assert_eq!(count, 2);
        assert_eq!(text, "[REDACTED] [REDACTED]");
    }

    #[test]
    fn bearer_tokens_keep_the_keyword() {
        let (text, count) = redacted("Authorization: Bearer abcdefghijklmnopqrstuvwxyz0123456789");
        assert_eq!(count, 1);
        assert_eq!(text, "Authorization: Bearer [REDACTED]");
    }

    #[test]
    fn assignments_keep_the_key_and_drop_the_value() {
        let (text, count) = redacted(
            "password=hunter2secret\napi_key: \"abc123def456\"\nexport OPENAI_API_KEY='zzzzzzzz'",
        );
        assert_eq!(count, 3);
        assert_eq!(
            text,
            "password=[REDACTED]\napi_key: \"[REDACTED]\"\nexport OPENAI_API_KEY='[REDACTED]'"
        );
    }

    #[test]
    fn pem_blocks_are_replaced_whole() {
        let (text, count) = redacted(
            "before\n-----BEGIN RSA PRIVATE KEY-----\nMIIBOgIBAAJB\n-----END RSA PRIVATE KEY-----\nafter",
        );
        assert_eq!(count, 1);
        assert_eq!(text, "before\n[REDACTED]\nafter");
    }

    #[test]
    fn public_certificates_and_ordinary_text_survive() {
        let source = "-----BEGIN CERTIFICATE-----\nabc\n-----END CERTIFICATE-----\nrisk-assessment-of-something-long max_tokens: 4096 token_count=12345";
        let (text, count) = redacted(source);
        assert_eq!(count, 0);
        assert_eq!(text, source);
    }

    #[test]
    fn placeholders_and_comparisons_are_not_secrets() {
        for source in [
            "token: ${TOKEN_VALUE}",
            "password == hunter2secret",
            "secret: null",
        ] {
            assert_eq!(redacted(source), (source.to_string(), 0), "{source}");
        }
    }

    #[test]
    fn multibyte_text_is_preserved() {
        let (text, count) = redacted("héllo wörld ✓ sk-abcdefghijklmnop1234 ünï");
        assert_eq!(count, 1);
        assert_eq!(text, "héllo wörld ✓ [REDACTED] ünï");
    }

    #[test]
    fn json_values_are_redacted_recursively() {
        let mut value = serde_json::json!({"a": ["sk-abcdefghijklmnop1234", {"b": "ok"}], "n": 1});
        assert_eq!(redact_value(&mut value), 1);
        assert_eq!(value["a"][0], "[REDACTED]");
    }
}
