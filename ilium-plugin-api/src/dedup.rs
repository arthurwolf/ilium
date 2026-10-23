//! Dedup keys of `send-input` and `agent-action` (reference-events-and-intents.md,
//! requests-and-outcomes.md "Duplicates").

use crate::generated::types::PluginError;

/// A checked dedup key: 1 to 128 characters from `[A-Za-z0-9._:/-]` (ABI-010).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DedupKey(String);

impl DedupKey {
    /// ABI-010: `[A-Za-z0-9._:/-]`, 1..=128 characters; anything else is `invalid-argument`.
    pub fn parse(text: &str) -> Result<DedupKey, PluginError> {
        if text.is_empty()
            || text.len() > DEDUP_KEY_MAX_CHARS
            || !text
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._:/-".contains(&byte))
        {
            return Err(PluginError {
                code: crate::ErrorCode::InvalidArgument,
                message: "dedup key must contain 1 to 128 ASCII letters, digits or ._:/-".into(),
                retry_after_ms: None,
            });
        }
        Ok(DedupKey(text.to_owned()))
    }

    /// The key text (ABI-010).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Longest dedup key in characters (ABI-010).
pub const DEDUP_KEY_MAX_CHARS: usize = 128;

/// Characters a dedup key may use besides ASCII letters and digits (ABI-010).
pub const DEDUP_KEY_PUNCTUATION: &str = "._:/-";

/// How long an accepted key stays reserved, on the wall clock, in milliseconds (ABI-034).
pub const DEDUP_WINDOW_MS: u64 = 600_000;

/// ABI-034: a key accepted at `accepted_wall_ms` is still reserved at `now_wall_ms` while
/// `now_wall_ms - accepted_wall_ms < 600,000`. A clock that went backwards keeps it reserved.
pub fn is_reserved(accepted_wall_ms: u64, now_wall_ms: u64) -> bool {
    now_wall_ms.saturating_sub(accepted_wall_ms) < DEDUP_WINDOW_MS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::types::ErrorCode;

    /// REQ: ABI-010
    #[test]
    fn parse_accepts_single_character_key() {
        let key = DedupKey::parse("a").expect("a one-character key is valid");
        assert_eq!(key.as_str(), "a");
    }

    /// REQ: ABI-010
    #[test]
    fn parse_accepts_letters_digits_and_all_punctuation() {
        let text = "Send-Input_01.v2:pane/7";
        let key = DedupKey::parse(text).expect("letters, digits and ._:/- are all valid");
        assert_eq!(key.as_str(), text);
    }

    /// REQ: ABI-010
    #[test]
    fn parse_accepts_every_documented_punctuation_character() {
        let key = DedupKey::parse(DEDUP_KEY_PUNCTUATION).expect("each of ._:/- is allowed");
        assert_eq!(key.as_str(), DEDUP_KEY_PUNCTUATION);
    }

    /// REQ: ABI-010
    #[test]
    fn parse_accepts_exactly_128_characters() {
        let text = "k".repeat(DEDUP_KEY_MAX_CHARS);
        let key = DedupKey::parse(&text).expect("128 characters is the documented maximum");
        assert_eq!(key.as_str().len(), 128);
    }

    /// REQ: ABI-010
    #[test]
    fn parse_rejects_empty_text_as_invalid_argument() {
        let error = DedupKey::parse("").expect_err("an empty key is below the 1-character minimum");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(error.retry_after_ms, None);
    }

    /// REQ: ABI-010
    #[test]
    fn parse_rejects_129_characters_as_invalid_argument() {
        let text = "k".repeat(DEDUP_KEY_MAX_CHARS + 1);
        let error = DedupKey::parse(&text).expect_err("129 characters exceeds the maximum");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    /// REQ: ABI-010
    #[test]
    fn parse_rejects_disallowed_ascii_punctuation_as_invalid_argument() {
        for text in [
            "has space",
            "at@sign",
            "hash#",
            "plus+",
            "equal=",
            "comma,",
            "star*",
        ] {
            let error = DedupKey::parse(text).expect_err("this character is outside the set");
            assert_eq!(error.code, ErrorCode::InvalidArgument, "input {text:?}");
        }
    }

    /// REQ: ABI-010
    #[test]
    fn parse_rejects_non_ascii_letters_as_invalid_argument() {
        let error =
            DedupKey::parse("caf\u{e9}").expect_err("accented letters are not ASCII letters");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    /// REQ: ABI-010
    #[test]
    fn parse_rejects_newline_as_invalid_argument() {
        let error =
            DedupKey::parse("line\nbreak").expect_err("control characters are outside the set");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    /// REQ: ABI-010
    #[test]
    fn constant_max_chars_is_128() {
        assert_eq!(DEDUP_KEY_MAX_CHARS, 128);
    }

    /// REQ: ABI-010
    #[test]
    fn constant_punctuation_is_exactly_the_documented_set() {
        assert_eq!(DEDUP_KEY_PUNCTUATION, "._:/-");
    }

    /// REQ: ABI-034
    #[test]
    fn window_constant_is_600_000_milliseconds() {
        assert_eq!(DEDUP_WINDOW_MS, 600_000);
    }

    /// REQ: ABI-034
    #[test]
    fn key_is_reserved_immediately_after_acceptance() {
        assert!(is_reserved(1_000, 1_000));
    }

    /// REQ: ABI-034
    #[test]
    fn key_is_reserved_one_millisecond_before_the_window_ends() {
        assert!(is_reserved(1_000, 1_000 + 599_999));
    }

    /// REQ: ABI-034
    #[test]
    fn key_is_free_at_exactly_600_000_milliseconds() {
        assert!(!is_reserved(1_000, 1_000 + 600_000));
    }

    /// REQ: ABI-034
    #[test]
    fn key_is_free_well_after_the_window() {
        assert!(!is_reserved(0, 600_001));
        assert!(!is_reserved(0, 3_600_000));
    }

    /// REQ: ABI-034
    #[test]
    fn backwards_clock_keeps_the_key_reserved() {
        assert!(is_reserved(700_000, 699_999));
        assert!(is_reserved(1_000_000, 0));
    }

    /// REQ: ABI-034
    #[test]
    fn zero_timestamps_reserve_for_the_window() {
        assert!(is_reserved(0, 0));
        assert!(is_reserved(0, 599_999));
        assert!(!is_reserved(0, 600_000));
    }

    /// REQ: ABI-034
    #[test]
    fn extreme_timestamps_do_not_overflow() {
        assert!(is_reserved(u64::MAX, u64::MAX));
        assert!(is_reserved(u64::MAX - 1, u64::MAX));
        assert!(!is_reserved(0, u64::MAX));
    }
}
