//! Dedup keys for keyed submissions (rust-sdk.md "Values, submissions and outcomes").

/// Joins the parts into a dedup key of at most 128 bytes that the host de-duplicates for 10 minutes.
pub fn key_for(_parts: &[&str]) -> String {
    unimplemented!("rust-sdk.dedup-key")
}

#[cfg(test)]
mod spec_tests {
    use super::key_for;

    /// Manual: `dedup::key_for` joins its parts with `:`
    #[test]
    fn joins_parts_with_colon() {
        assert_eq!(key_for(&["freeze", "7"]), "freeze:7");
    }

    #[test]
    fn joins_three_parts_in_order() {
        assert_eq!(
            key_for(&["badge", "node-1", "changed"]),
            "badge:node-1:changed"
        );
    }

    /// Manual: a dedup key is 1 to 128 characters from `[A-Za-z0-9._:/-]`
    #[test]
    fn result_uses_only_allowed_characters_and_length() {
        let key = key_for(&["continue", "pane-3", "epoch_9.1/a"]);
        assert!(!key.is_empty() && key.len() <= 128);
        assert!(key
            .chars()
            .all(|character| character.is_ascii_alphanumeric()
                || matches!(character, '.' | '_' | ':' | '/' | '-')));
    }

    #[test]
    fn same_parts_give_same_key() {
        assert_eq!(
            key_for(&["title", "node-2", "Hello"]),
            key_for(&["title", "node-2", "Hello"])
        );
    }

    #[test]
    fn single_part_is_unchanged_and_empty_part_keeps_its_separator() {
        assert_eq!(key_for(&["hello"]), "hello");
        assert_eq!(key_for(&["a", ""]), "a:");
    }

    /// Manual: "a dedup key of at most 128 bytes"
    #[test]
    fn overlong_input_is_limited_to_128_bytes() {
        let long_part = "x".repeat(200);
        let key = key_for(&["prefix", long_part.as_str()]);
        assert!(key.len() <= 128);
    }
}
