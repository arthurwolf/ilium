//! Dedup keys for keyed submissions (rust-sdk.md "Values, submissions and outcomes").

/// Joins the parts into a dedup key of at most 128 bytes that the host de-duplicates for 10 minutes.
pub fn key_for(_parts: &[&str]) -> String {
    unimplemented!("rust-sdk.dedup-key")
}

#[cfg(test)]
mod spec_tests {
    use super::key_for;

    /// REQ: glossary.md "De-duplication key" (`dedup::key_for` joins its parts with `:`)
    #[test]
    fn joins_parts_with_colon() {
        assert_eq!(key_for(&["freeze", "7"]), "freeze:7");
    }

    /// REQ: glossary.md "De-duplication key" (three parts, as in the badge worked example)
    #[test]
    fn joins_three_parts_in_order() {
        assert_eq!(key_for(&["badge", "node-1", "changed"]), "badge:node-1:changed");
    }

    /// REQ: rust-sdk.md "Values, submissions and outcomes" (a dedup key is 1 to 128 characters from `[A-Za-z0-9._:/-]`)
    #[test]
    fn result_uses_only_allowed_characters_and_length() {
        let key = key_for(&["continue", "pane-3", "epoch_9.1/a"]);
        assert!(!key.is_empty() && key.len() <= 128);
        assert!(key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | ':' | '/' | '-')));
    }

    /// REQ: glossary.md "De-duplication key" (a repeat of the same key within 10 minutes is a duplicate, so the same parts must give the same key)
    #[test]
    fn same_parts_give_same_key() {
        assert_eq!(key_for(&["title", "node-2", "Hello"]), key_for(&["title", "node-2", "Hello"]));
    }

    /// REQ: rust-sdk.md "Values, submissions and outcomes" (a single part is the whole key; join rule applies to an empty part as well)
    #[test]
    fn single_part_is_unchanged_and_empty_part_keeps_its_separator() {
        assert_eq!(key_for(&["hello"]), "hello");
        assert_eq!(key_for(&["a", ""]), "a:");
    }

    /// REQ: ilium-plugin-sdk-core/src/dedup.rs doc comment ("a dedup key of at most 128 bytes")
    #[test]
    fn overlong_input_is_limited_to_128_bytes() {
        let long_part = "x".repeat(200);
        let key = key_for(&["prefix", long_part.as_str()]);
        assert!(key.len() <= 128);
    }
}
