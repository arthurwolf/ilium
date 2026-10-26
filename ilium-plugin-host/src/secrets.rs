//! Shared secret redaction and display helpers (SEC-243, SEC-248).
use base64::{Engine as _, engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD}};

/// Replaces recognized representations of secrets of at least six characters.
pub fn redact(text: &str, secrets: &[String]) -> String {
    redact_variants(text, &variants(secrets))
}

pub(crate) fn variants(secrets: &[String]) -> Vec<String> {
    let mut variants = Vec::new();
    for secret in secrets.iter().filter(|secret| secret.chars().count() >= 6) {
        variants.push(secret.clone());
        for engine in [&STANDARD, &STANDARD_NO_PAD, &URL_SAFE, &URL_SAFE_NO_PAD] {
            variants.push(engine.encode(secret.as_bytes()));
        }
        variants.push(hex_lower(secret.as_bytes()));
        variants.push(percent_encode(secret));
        variants.push(secret.bytes().map(|byte| format!("%{byte:02X}")).collect());
        variants.push(secret.bytes().map(|byte| format!("%{byte:02x}")).collect());
    }
    variants.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
    variants.dedup();
    variants
}

// Scan original text only: never interpret the inserted marker as secret input.
pub(crate) fn redact_variants(text: &str, variants: &[String]) -> String {
    let mut remaining = text;
    let mut output = String::new();
    while let Some((position, variant)) = variants.iter().filter(|variant| !variant.is_empty())
        .filter_map(|variant| remaining.find(variant).map(|position| (position, variant)))
        .min_by(|left, right| left.0.cmp(&right.0).then_with(|| right.1.len().cmp(&left.1.len()))) {
        output.push_str(&remaining[..position]);
        output.push_str("<redacted>");
        remaining = &remaining[position + variant.len()..];
    }
    output.push_str(remaining);
    output
}

/// Standard base64 with padding.
pub fn base64_standard(bytes: &[u8]) -> String { STANDARD.encode(bytes) }
/// URL-safe base64 without padding.
pub fn base64_url(bytes: &[u8]) -> String { URL_SAFE_NO_PAD.encode(bytes) }
/// Percent-encoding of bytes outside the URI unreserved set.
pub fn percent_encode(text: &str) -> String {
    text.bytes().map(|byte| if byte.is_ascii_alphanumeric() || matches!(byte,b'-'|b'_'|b'.'|b'~') {
        char::from(byte).to_string()
    } else { format!("%{byte:02X}") }).collect()
}
/// Lower-case hexadecimal.
pub fn hex_lower(bytes: &[u8]) -> String { bytes.iter().map(|byte| format!("{byte:02x}")).collect() }
/// Fixed mask plus at most eight trailing Unicode characters.
pub fn masked_row(secret: &str, visible: u8) -> String {
    let count = secret.chars().count();
    let keep = usize::from(visible.min(8)).min(count);
    format!("••••{}", secret.chars().skip(count - keep).collect::<String>())
}
