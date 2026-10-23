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

#[cfg(test)]
mod tests {
    use super::{base64_standard, base64_url, hex_lower, masked_row, percent_encode, redact};

    /// Secret whose standard base64 contains neither `+` nor `/` (padded: `czNjcjN0LXRva2VuIQ==`).
    const SECRET_PLAIN: &str = "s3cr3t-token!";
    /// Secret whose base64 contains `/` (standard) and `_` (URL-safe) and whose percent form contains `%3E`.
    const SECRET_SYMBOLS: &str = "~>?~>?x";

    fn secrets_of(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    /// REQ: SEC-243
    #[test]
    fn base64_standard_encodes_empty_input_as_empty_string() {
        assert_eq!(base64_standard(&[]), "");
    }

    /// REQ: SEC-243
    #[test]
    fn base64_standard_pads_to_a_multiple_of_four() {
        assert_eq!(base64_standard(b"f"), "Zg==");
        assert_eq!(base64_standard(b"fo"), "Zm8=");
        assert_eq!(base64_standard(b"foo"), "Zm9v");
        assert_eq!(base64_standard(b"foob"), "Zm9vYg==");
    }

    /// REQ: SEC-243
    #[test]
    fn base64_standard_uses_plus_and_slash_alphabet() {
        assert_eq!(base64_standard(&[0xfb, 0xff]), "+/8=");
    }

    /// REQ: SEC-243
    #[test]
    fn base64_url_encodes_empty_input_as_empty_string() {
        assert_eq!(base64_url(&[]), "");
    }

    /// REQ: SEC-243
    #[test]
    fn base64_url_omits_padding() {
        assert_eq!(base64_url(b"f"), "Zg");
        assert_eq!(base64_url(b"foob"), "Zm9vYg");
        assert_eq!(base64_url(&[0xfb, 0xff]), "-_8");
    }

    /// REQ: SEC-243
    #[test]
    fn base64_url_differs_from_standard_on_symbol_characters() {
        assert_ne!(base64_url(&[0xfb, 0xff]), base64_standard(&[0xfb, 0xff]));
    }

    /// REQ: SEC-243
    #[test]
    fn percent_encode_empty_input_is_empty() {
        assert_eq!(percent_encode(""), "");
    }

    /// REQ: SEC-243
    #[test]
    fn percent_encode_keeps_unreserved_characters() {
        assert_eq!(
            percent_encode("AZaz09-._~"),
            "AZaz09-._~",
            "unreserved set must pass through unchanged"
        );
    }

    /// REQ: SEC-243
    #[test]
    fn percent_encode_escapes_space_slash_and_bang() {
        assert_eq!(percent_encode("a b/c!"), "a%20b%2Fc%21");
    }

    /// REQ: SEC-243
    #[test]
    fn percent_encode_escapes_every_utf8_byte_of_non_ascii() {
        assert_eq!(percent_encode("\u{e9}"), "%C3%A9");
    }

    /// REQ: SEC-243
    #[test]
    fn percent_encode_matches_reference_for_plain_secret() {
        assert_eq!(percent_encode(SECRET_PLAIN), "s3cr3t-token%21");
        assert_eq!(percent_encode(SECRET_SYMBOLS), "~%3E%3F~%3E%3Fx");
    }

    /// REQ: SEC-243
    #[test]
    fn hex_lower_empty_input_is_empty() {
        assert_eq!(hex_lower(&[]), "");
    }

    /// REQ: SEC-243
    #[test]
    fn hex_lower_uses_lower_case_digits() {
        assert_eq!(hex_lower(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
    }

    /// REQ: SEC-243
    #[test]
    fn hex_lower_keeps_leading_zero_nibbles() {
        assert_eq!(hex_lower(&[0x00, 0x0f, 0xa0, 0xff]), "000fa0ff");
    }

    /// REQ: SEC-243
    #[test]
    fn hex_lower_matches_reference_for_plain_secret() {
        assert_eq!(
            hex_lower(SECRET_PLAIN.as_bytes()),
            "7333637233742d746f6b656e21"
        );
    }

    /// REQ: SEC-243
    #[test]
    fn redact_empty_text_with_no_secrets_is_empty() {
        assert_eq!(redact("", &[]), "");
    }

    /// REQ: SEC-243
    #[test]
    fn redact_text_without_secret_is_unchanged() {
        assert_eq!(
            redact("nothing sensitive here", &secrets_of(&[SECRET_PLAIN])),
            "nothing sensitive here"
        );
    }

    /// REQ: SEC-243
    #[test]
    fn redact_replaces_raw_secret_with_marker() {
        assert_eq!(
            redact("token s3cr3t-token! end", &secrets_of(&[SECRET_PLAIN])),
            "token <redacted> end"
        );
    }

    /// REQ: SEC-243
    #[test]
    fn redact_replaces_every_occurrence() {
        assert_eq!(
            redact(
                "s3cr3t-token! and s3cr3t-token!",
                &secrets_of(&[SECRET_PLAIN])
            ),
            "<redacted> and <redacted>"
        );
    }

    /// REQ: SEC-243
    #[test]
    fn redact_replaces_standard_base64_with_padding() {
        assert_eq!(
            redact(
                "Authorization: czNjcjN0LXRva2VuIQ==",
                &secrets_of(&[SECRET_PLAIN])
            ),
            "Authorization: <redacted>"
        );
    }

    /// REQ: SEC-243
    #[test]
    fn redact_replaces_standard_base64_without_padding() {
        assert_eq!(
            redact("x czNjcjN0LXRva2VuIQ y", &secrets_of(&[SECRET_PLAIN])),
            "x <redacted> y"
        );
    }

    /// REQ: SEC-243
    #[test]
    fn redact_replaces_url_safe_base64_with_padding() {
        assert_eq!(
            redact(
                "https://host/?t=fj4_fj4_eA==",
                &secrets_of(&[SECRET_SYMBOLS])
            ),
            "https://host/?t=<redacted>"
        );
    }

    /// REQ: SEC-243
    #[test]
    fn redact_replaces_url_safe_base64_without_padding() {
        assert_eq!(
            redact("https://host/?t=fj4_fj4_eA", &secrets_of(&[SECRET_SYMBOLS])),
            "https://host/?t=<redacted>"
        );
    }

    /// REQ: SEC-243
    #[test]
    fn redact_replaces_standard_base64_with_slash_symbols() {
        assert_eq!(
            redact("a fj4/fj4/eA== b", &secrets_of(&[SECRET_SYMBOLS])),
            "a <redacted> b"
        );
    }

    /// REQ: SEC-243
    #[test]
    fn redact_replaces_percent_encoded_secret() {
        assert_eq!(
            redact("q=s3cr3t-token%21&x", &secrets_of(&[SECRET_PLAIN])),
            "q=<redacted>&x"
        );
        assert_eq!(
            redact("u=~%3E%3F~%3E%3Fx", &secrets_of(&[SECRET_SYMBOLS])),
            "u=<redacted>"
        );
    }

    /// REQ: SEC-243
    #[test]
    fn redact_replaces_lower_case_hex_secret() {
        assert_eq!(
            redact("h=7333637233742d746f6b656e21", &secrets_of(&[SECRET_PLAIN])),
            "h=<redacted>"
        );
    }

    /// REQ: SEC-243
    #[test]
    fn redact_matching_is_case_sensitive() {
        assert_eq!(
            redact("h=7333637233742D746F6B656E21", &secrets_of(&[SECRET_PLAIN])),
            "h=7333637233742D746F6B656E21",
            "upper-case hex is not a listed variant and must not be replaced"
        );
    }

    /// REQ: SEC-243
    #[test]
    fn redact_skips_secrets_shorter_than_six_characters() {
        assert_eq!(redact("abc xyz", &secrets_of(&["abc"])), "abc xyz");
        assert_eq!(redact("abcde", &secrets_of(&["abcde"])), "abcde");
    }

    /// REQ: SEC-243
    #[test]
    fn redact_empty_secret_does_not_insert_markers() {
        assert_eq!(redact("abc", &secrets_of(&[""])), "abc");
    }

    /// REQ: SEC-243
    #[test]
    fn redact_accepts_secret_of_exactly_six_characters() {
        assert_eq!(
            redact("pw=abcdef.", &secrets_of(&["abcdef"])),
            "pw=<redacted>."
        );
    }

    /// REQ: SEC-243
    #[test]
    fn redact_replaces_longer_value_first_regardless_of_order() {
        let text = "x abcdefghijkl y";
        assert_eq!(
            redact(text, &secrets_of(&["abcdef", "abcdefghijkl"])),
            "x <redacted> y"
        );
        assert_eq!(
            redact(text, &secrets_of(&["abcdefghijkl", "abcdef"])),
            "x <redacted> y"
        );
    }

    /// REQ: SEC-243
    #[test]
    fn redact_replaces_each_of_several_secrets() {
        assert_eq!(
            redact(
                "one s3cr3t-token! two abcdef",
                &secrets_of(&[SECRET_PLAIN, "abcdef"])
            ),
            "one <redacted> two <redacted>"
        );
    }

    /// REQ: SEC-248
    #[test]
    fn masked_row_reveals_requested_trailing_characters() {
        assert_eq!(
            masked_row("token-xyz-a1b2", 4),
            "\u{2022}\u{2022}\u{2022}\u{2022}a1b2"
        );
    }

    /// REQ: SEC-248
    #[test]
    fn masked_row_with_zero_visible_shows_dots_only() {
        assert_eq!(
            masked_row("token-xyz-a1b2", 0),
            "\u{2022}\u{2022}\u{2022}\u{2022}"
        );
    }

    /// REQ: SEC-248
    #[test]
    fn masked_row_reveals_up_to_eight_trailing_characters() {
        assert_eq!(
            masked_row("abcdefghijklmnop", 8),
            "\u{2022}\u{2022}\u{2022}\u{2022}ijklmnop"
        );
    }

    /// REQ: SEC-248
    #[test]
    fn masked_row_caps_reveal_at_eight_characters() {
        assert_eq!(
            masked_row("abcdefghijklmnop", 12),
            "\u{2022}\u{2022}\u{2022}\u{2022}ijklmnop"
        );
        assert_eq!(
            masked_row("abcdefghijklmnop", 255),
            "\u{2022}\u{2022}\u{2022}\u{2022}ijklmnop"
        );
    }

    /// REQ: SEC-248
    #[test]
    fn masked_row_short_secret_reveals_no_more_than_it_has() {
        assert_eq!(masked_row("ab", 8), "\u{2022}\u{2022}\u{2022}\u{2022}ab");
    }

    /// REQ: SEC-248
    #[test]
    fn masked_row_empty_secret_shows_dots_only() {
        assert_eq!(masked_row("", 4), "\u{2022}\u{2022}\u{2022}\u{2022}");
    }

    /// REQ: SEC-248
    #[test]
    fn masked_row_counts_characters_not_bytes() {
        assert_eq!(
            masked_row("p\u{e4}ssw\u{f6}rd", 3),
            "\u{2022}\u{2022}\u{2022}\u{2022}\u{f6}rd"
        );
    }
}
