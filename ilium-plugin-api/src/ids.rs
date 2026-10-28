//! Id newtypes of `ilium:types/ids` and plugin id parsing (reference-wit.md section 4.1,
//! reference-manifest.md PKG-014).

pub use crate::generated::types::ids::{
    BufferId, ChildId, ClientId, EpochMs, GrabId, IndexId, LockId, NodeId, PluginId, RequestId,
    StreamId, TimerId, WatchId,
};

use crate::manifest::Diagnostic;

/// Longest plugin id in characters (reference-manifest.md PKG-014).
pub const PLUGIN_ID_MAX_CHARS: usize = 128;

/// Fewest and most `.`-separated segments of a plugin id (reference-manifest.md PKG-014).
pub const PLUGIN_ID_SEGMENTS: std::ops::RangeInclusive<usize> = 2..=8;

/// Parses and checks a plugin id; a bad id is `manifest.id-invalid` (reference-manifest.md PKG-014).
pub fn parse_plugin_id(text: &str) -> Result<PluginId, Diagnostic> {
    let segments: Vec<&str> = text.split('.').collect();
    let valid = text.len() <= PLUGIN_ID_MAX_CHARS
        && PLUGIN_ID_SEGMENTS.contains(&segments.len())
        && segments.iter().all(|segment| {
            (1..=32).contains(&segment.len())
                && !segment.starts_with('-')
                && !segment.ends_with('-')
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        });
    if !valid {
        return Err(Diagnostic::error(crate::error::Code::from_static("manifest.id-invalid"), "id", "plugin id must have 2 to 8 lower-case dot-separated segments of 1 to 32 characters, at most 128 characters total"));
    }
    // Reserved namespaces depend on release/tier admission, not identifier syntax.
    Ok(PluginId(text.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Severity;

    /// Parses an id that the manual says is valid, and returns it; a rejection fails the test.
    fn accept(text: &str) -> PluginId {
        match parse_plugin_id(text) {
            Ok(plugin_id) => plugin_id,
            Err(diagnostic) => panic!("expected `{text}` to be accepted, got {diagnostic}"),
        }
    }

    /// Parses an id that the manual says is refused, and returns the diagnostic; an acceptance fails the test.
    fn reject(text: &str) -> Diagnostic {
        match parse_plugin_id(text) {
            Ok(plugin_id) => panic!("expected `{text}` to be refused, got {}", plugin_id.0),
            Err(diagnostic) => diagnostic,
        }
    }

    /// Asserts the diagnostic is an error with the given namespaced code.
    fn assert_error_code(diagnostic: &Diagnostic, expected_code: &str) {
        assert_eq!(diagnostic.code.as_str(), expected_code);
        assert!(matches!(diagnostic.severity, Severity::Error));
    }

    /// Builds a segment of `length` lowercase `a` characters.
    fn segment_of_length(length: usize) -> String {
        "a".repeat(length)
    }

    /// REQ: PKG-014
    #[test]
    fn two_segment_id_is_accepted_and_returned_unchanged() {
        let plugin_id = accept("example.demo");
        assert_eq!(plugin_id.0, "example.demo");
    }

    /// REQ: PKG-014
    #[test]
    fn documented_valid_example_with_hyphen_is_accepted() {
        let plugin_id = accept("com.acme.git-tools");
        assert_eq!(plugin_id.0, "com.acme.git-tools");
    }

    /// REQ: PKG-014
    #[test]
    fn digits_are_allowed_in_segments() {
        let plugin_id = accept("example.2fa");
        assert_eq!(plugin_id.0, "example.2fa");
    }

    /// REQ: PKG-014
    #[test]
    fn eight_segments_is_the_most_allowed() {
        let text = "a.b.c.d.e.f.g.h";
        let plugin_id = accept(text);
        assert_eq!(plugin_id.0, text);
    }

    /// REQ: PKG-014
    #[test]
    fn nine_segments_is_refused_as_invalid() {
        let diagnostic = reject("a.b.c.d.e.f.g.h.i");
        assert_error_code(&diagnostic, "manifest.id-invalid");
    }

    /// REQ: PKG-014
    #[test]
    fn one_segment_is_refused_as_invalid() {
        let diagnostic = reject("demo");
        assert_error_code(&diagnostic, "manifest.id-invalid");
    }

    /// REQ: PKG-014
    #[test]
    fn empty_text_is_refused_as_invalid() {
        let diagnostic = reject("");
        assert_error_code(&diagnostic, "manifest.id-invalid");
    }

    /// REQ: PKG-014
    #[test]
    fn lone_dot_with_two_empty_segments_is_refused_as_invalid() {
        let diagnostic = reject(".");
        assert_error_code(&diagnostic, "manifest.id-invalid");
    }

    /// REQ: PKG-014
    #[test]
    fn uppercase_letters_are_refused_as_invalid() {
        let diagnostic = reject("Example.Demo");
        assert_error_code(&diagnostic, "manifest.id-invalid");
    }

    /// REQ: PKG-014
    #[test]
    fn empty_middle_segment_is_refused_as_invalid() {
        let diagnostic = reject("example..demo");
        assert_error_code(&diagnostic, "manifest.id-invalid");
    }

    /// REQ: PKG-014
    #[test]
    fn trailing_dot_leaves_an_empty_segment_and_is_refused_as_invalid() {
        let diagnostic = reject("example.demo.");
        assert_error_code(&diagnostic, "manifest.id-invalid");
    }

    /// REQ: PKG-014
    #[test]
    fn segment_starting_with_hyphen_is_refused_as_invalid() {
        let diagnostic = reject("example.-demo");
        assert_error_code(&diagnostic, "manifest.id-invalid");
    }

    /// REQ: PKG-014
    #[test]
    fn segment_ending_with_hyphen_is_refused_as_invalid() {
        let diagnostic = reject("example.demo-");
        assert_error_code(&diagnostic, "manifest.id-invalid");
    }

    /// REQ: PKG-014
    #[test]
    fn segment_that_is_only_a_hyphen_is_refused_as_invalid() {
        let diagnostic = reject("example.-");
        assert_error_code(&diagnostic, "manifest.id-invalid");
    }

    /// REQ: PKG-014
    #[test]
    fn underscore_is_refused_as_invalid() {
        let diagnostic = reject("example.dem_o");
        assert_error_code(&diagnostic, "manifest.id-invalid");
    }

    /// REQ: PKG-014
    #[test]
    fn space_is_refused_as_invalid() {
        let diagnostic = reject("example.de mo");
        assert_error_code(&diagnostic, "manifest.id-invalid");
    }

    /// REQ: PKG-014
    #[test]
    fn single_character_segments_are_accepted_at_the_lower_boundary() {
        let plugin_id = accept("a.b");
        assert_eq!(plugin_id.0, "a.b");
    }

    /// REQ: PKG-014
    #[test]
    fn segment_of_32_characters_is_accepted_at_the_upper_boundary() {
        let text = format!("example.{}", segment_of_length(32));
        let plugin_id = accept(&text);
        assert_eq!(plugin_id.0, text);
    }

    /// REQ: PKG-014
    #[test]
    fn segment_of_33_characters_is_refused_as_invalid() {
        let text = format!("example.{}", segment_of_length(33));
        let diagnostic = reject(&text);
        assert_error_code(&diagnostic, "manifest.id-invalid");
    }

    /// REQ: PKG-014
    #[test]
    fn whole_id_of_128_characters_is_accepted_at_the_upper_boundary() {
        // 32 + 1 + 32 + 1 + 32 + 1 + 29 = 128 characters in four segments.
        let text = format!(
            "{}.{}.{}.{}",
            segment_of_length(32),
            segment_of_length(32),
            segment_of_length(32),
            segment_of_length(29)
        );
        assert_eq!(text.chars().count(), PLUGIN_ID_MAX_CHARS);
        let plugin_id = accept(&text);
        assert_eq!(plugin_id.0, text);
    }

    /// REQ: PKG-014
    #[test]
    fn whole_id_of_129_characters_is_refused_as_invalid() {
        // 32 + 1 + 32 + 1 + 32 + 1 + 30 = 129 characters; every segment is within its own limit.
        let text = format!(
            "{}.{}.{}.{}",
            segment_of_length(32),
            segment_of_length(32),
            segment_of_length(32),
            segment_of_length(30)
        );
        assert_eq!(text.chars().count(), PLUGIN_ID_MAX_CHARS + 1);
        let diagnostic = reject(&text);
        assert_error_code(&diagnostic, "manifest.id-invalid");
    }

    /// REQ: PKG-014
    #[test]
    fn documented_constants_match_the_manual_limits() {
        assert_eq!(PLUGIN_ID_MAX_CHARS, 128);
        assert_eq!(PLUGIN_ID_SEGMENTS, 2..=8);
    }

    /// REQ: PKG-014
    #[test]
    fn ilium_prefixed_id_is_refused_as_reserved() {
        let diagnostic = reject("ilium.demo");
        assert_error_code(&diagnostic, "manifest.id-reserved");
    }

    /// REQ: PKG-234
    #[test]
    fn ilium_prefix_refusal_applies_to_nested_ids() {
        let diagnostic = reject("ilium.terminal.extra");
        assert_error_code(&diagnostic, "manifest.id-reserved");
    }

    /// REQ: PKG-220
    #[test]
    fn core_local_and_test_as_first_segment_are_refused_as_reserved() {
        for text in ["core.demo", "local.demo", "test.demo"] {
            let diagnostic = reject(text);
            assert_error_code(&diagnostic, "manifest.id-reserved");
        }
    }

    /// REQ: PKG-220
    #[test]
    fn reserved_words_only_count_as_the_whole_first_segment() {
        for text in ["coreutils.demo", "localize.demo", "testing.demo"] {
            let plugin_id = accept(text);
            assert_eq!(plugin_id.0, text);
        }
    }

    /// REQ: PKG-220
    #[test]
    fn reserved_word_later_in_the_id_is_accepted() {
        let plugin_id = accept("example.core");
        assert_eq!(plugin_id.0, "example.core");
    }

    /// REQ: PKG-014
    #[test]
    fn pattern_failure_takes_precedence_over_reserved_prefix() {
        // An `ilium.` id with an uppercase letter fails the pattern before the reservation check.
        let diagnostic = reject("ilium.Demo");
        assert_error_code(&diagnostic, "manifest.id-invalid");
    }
}
