//! The pure token rules of `ilium-client/src/smart_copy_tokens.rs`, tested through the plugin's
//! `tokens` module.

use ilium_plugin_core_smart_copy::tokens::{
    digit_count, has_source_extension, is_plausible_hash, path_is_plausible, path_kind_label,
    short, trim_trailing, url_end,
};

/// REQ: F-016
#[test]
fn url_tail_drops_sentence_punctuation() {
    let url = "https://example.com/a/b.";
    assert_eq!(&url[..url_end(url)], "https://example.com/a/b");
    let url = "https://example.com/a?!";
    assert_eq!(&url[..url_end(url)], "https://example.com/a");
}

/// REQ: F-016
#[test]
fn url_tail_keeps_balanced_brackets_and_drops_unbalanced_ones() {
    let balanced = "https://en.wikipedia.org/wiki/Rust_(language)";
    assert_eq!(&balanced[..url_end(balanced)], balanced);
    let unbalanced = "https://example.com/a)";
    assert_eq!(&unbalanced[..url_end(unbalanced)], "https://example.com/a");
    let bracket = "https://example.com/a]";
    assert_eq!(&bracket[..url_end(bracket)], "https://example.com/a");
    assert_eq!(url_end(""), 0);
}

/// REQ: F-016
#[test]
fn source_extension_is_judged_on_the_last_segment_only() {
    assert!(has_source_extension("src/main.rs"));
    assert!(has_source_extension("a.b/c.toml"));
    assert!(!has_source_extension("a.rs/readme"));
    assert!(!has_source_extension("noextension"));
    assert!(!has_source_extension("file.unknownext"));
}

/// REQ: F-016
#[test]
fn path_plausibility_follows_the_prefix_segment_and_extension_rules() {
    assert!(path_is_plausible("~/notes"));
    assert!(path_is_plausible("./x"));
    assert!(path_is_plausible("../x"));
    assert!(path_is_plausible("/usr/bin"));
    assert!(!path_is_plausible("/usr"));
    assert!(path_is_plausible("main.rs"));
    assert!(path_is_plausible("src/anything"));
    assert!(!path_is_plausible("random/anything"));
}

/// REQ: F-016
#[test]
fn hash_candidates_need_a_digit_a_letter_and_a_known_length() {
    assert!(is_plausible_hash("a1b2c3d"));
    assert!(is_plausible_hash(&format!("a1{}", "b".repeat(38))));
    assert!(!is_plausible_hash("abcdefg"));
    assert!(!is_plausible_hash("1234567"));
    assert!(!is_plausible_hash("a1b2c3"));
    assert!(!is_plausible_hash("a1b2c3d4e5f6g"));
}

/// REQ: F-016
#[test]
fn labels_collapse_whitespace_and_clip_at_thirty_six_characters() {
    assert_eq!(short("a   b\n c"), "a b c");
    let long = "x".repeat(50);
    let clipped = short(&long);
    assert_eq!(clipped.chars().count(), 36);
    assert!(clipped.ends_with('…'));
    assert_eq!(path_kind_label("src/a.rs"), "path src/a.rs");
}

/// REQ: F-016
#[test]
fn trimming_and_counting_helpers_respect_utf8_boundaries() {
    let text = "ab…,,";
    let end = trim_trailing(text, 0, text.len(), &[',']);
    assert_eq!(&text[..end], "ab…");
    assert_eq!(trim_trailing("..", 0, 2, &['.']), 0);
    assert_eq!(digit_count("a1b22"), 3);
}
