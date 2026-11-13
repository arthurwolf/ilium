//! The pure Smart Copy light rules, tested through the plugin's `light` module. They carry over the
//! unit tests of `ilium-client/src/smart_copy_light.rs`.

use ilium_plugin_core_smart_copy::light::{
    preview_lines, LightState, Preview, PREVIEW_DURATION_MS, RELEASE_IDLE_GRACE_MS,
};

/// REQ: F-016
#[test]
fn short_text_is_shown_whole() {
    assert_eq!(preview_lines("one\ntwo", 6, 20), vec!["one", "two"]);
}

/// REQ: F-016
#[test]
fn long_lines_are_clipped_with_an_ellipsis() {
    assert_eq!(preview_lines("abcdefghij", 6, 5), vec!["abcd…"]);
}

/// REQ: F-016
#[test]
fn extra_lines_collapse_into_a_count() {
    let text = (1..=10)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        preview_lines(&text, 4, 20),
        vec!["1", "2", "3", "… 7 more lines"]
    );
    assert_eq!(preview_lines("a\nb\nc", 2, 20), vec!["a", "… 2 more lines"]);
    assert_eq!(preview_lines("a\nb\nc", 3, 20), vec!["a", "b", "c"]);
    assert_eq!(preview_lines("a\nb", 1, 20), vec!["a", "… 1 more line"]);
}

/// REQ: F-016
#[test]
fn control_characters_never_reach_the_dialog() {
    assert_eq!(preview_lines("a\u{1b}[31mb\tc", 6, 40), vec!["a [31mb c"]);
}

/// REQ: F-016
#[test]
fn preview_counts_down_over_exactly_one_second() {
    let preview = Preview::new("x".into(), 1, true, 0);
    assert!((preview.remaining_fraction(0) - 1.0).abs() < 1e-9);
    assert!((preview.remaining_fraction(500) - 0.5).abs() < 1e-9);
    assert!(!preview.is_expired(PREVIEW_DURATION_MS - 1));
    assert!(preview.is_expired(PREVIEW_DURATION_MS));
    assert_eq!(preview.remaining_fraction(5000), 0.0);
}

/// REQ: F-016
#[test]
fn summary_reports_counts_and_clipboard_failure() {
    assert_eq!(
        Preview::new("abc".into(), 1, true, 0).summary(),
        "1 selection · 3 characters copied"
    );
    assert_eq!(
        Preview::new("abc".into(), 2, false, 0).summary(),
        "2 selections · 3 characters · clipboard unavailable"
    );
}

/// REQ: F-016
#[test]
fn an_idle_modifier_counts_as_released_only_with_a_selection() {
    let mut state = LightState::new("alt", 100);
    assert!(!state.is_idle_released(true, 100 + RELEASE_IDLE_GRACE_MS - 1));
    assert!(state.is_idle_released(true, 100 + RELEASE_IDLE_GRACE_MS));
    assert!(!state.is_idle_released(false, 100 + RELEASE_IDLE_GRACE_MS));
    state.touch(5000);
    assert!(!state.is_idle_released(true, 5000 + RELEASE_IDLE_GRACE_MS - 1));
}
