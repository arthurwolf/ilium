//! The Editor settings tab: keys under `[editor]`, defaults, labels and clamping
//! (src/docs/editors-and-boards.md "Editor settings", core-plugin-editor.md "Kernel tabs this plugin owns").

use crate::support::active_host;
use ilium_plugin_sdk::generated::types::ViewTarget;
use ilium_plugin_sdk::testing::host::{FakeHost, Screen};
use proptest::prelude::*;
use serde_json::json;

fn settings_screen(host: &mut FakeHost) -> Screen {
    host.render(ViewTarget::SettingsPage("editor".to_string()), 80, 12)
}

/// The value shown on the row of `label`: the text after the dotted leader, trimmed.
fn row_value(screen: &Screen, label: &str) -> Option<String> {
    for index in 0..screen.rows {
        let row = screen.row(index);
        let text = row.trim_start();
        let Some(rest) = text.strip_prefix(label) else {
            continue;
        };
        let Some(after_gap) = rest.strip_prefix(' ') else {
            continue;
        };
        if !after_gap.starts_with('\u{2026}') {
            continue;
        }
        let value = after_gap.trim_start_matches('\u{2026}').trim();
        return Some(value.to_string());
    }
    None
}

fn value_with(settings: &[(&str, serde_json::Value)], label: &str) -> Option<String> {
    let mut host = active_host(settings);
    let screen = settings_screen(&mut host);
    row_value(&screen, label)
}

/// REQ: F-019
#[test]
fn f_019_defaults_long_lines_clip() {
    assert_eq!(value_with(&[], "Long lines").as_deref(), Some("Clip"));
}

/// REQ: F-019
#[test]
fn f_019_defaults_line_numbers_on() {
    assert_eq!(value_with(&[], "Line numbers").as_deref(), Some("On"));
}

/// REQ: F-019
#[test]
fn f_019_defaults_minimap_on() {
    assert_eq!(value_with(&[], "Minimap").as_deref(), Some("On"));
}

/// REQ: F-019
#[test]
fn f_019_defaults_autosave_on() {
    assert_eq!(value_with(&[], "Autosave").as_deref(), Some("On"));
}

/// REQ: F-019
#[test]
fn f_019_defaults_autosave_delay_1000() {
    assert_eq!(value_with(&[], "Autosave delay").as_deref(), Some("1000"));
}

/// REQ: F-019, F-021
#[test]
fn f_021_defaults_markdown_opens_in_source() {
    assert_eq!(
        value_with(&[], "Markdown default").as_deref(),
        Some("Source")
    );
}

/// REQ: F-019
#[test]
fn f_019_line_display_wrap_is_shown() {
    assert_eq!(
        value_with(&[("line_display", json!("wrap"))], "Long lines").as_deref(),
        Some("Wrap")
    );
}

/// REQ: F-019
#[test]
fn f_019_line_numbers_off_is_shown() {
    assert_eq!(
        value_with(&[("show_line_numbers", json!(false))], "Line numbers").as_deref(),
        Some("Off")
    );
}

/// REQ: F-019
#[test]
fn f_019_minimap_off_is_shown() {
    assert_eq!(
        value_with(&[("show_minimap", json!(false))], "Minimap").as_deref(),
        Some("Off")
    );
}

/// REQ: F-019
#[test]
fn f_019_autosave_off_is_shown() {
    assert_eq!(
        value_with(&[("autosave_enabled", json!(false))], "Autosave").as_deref(),
        Some("Off")
    );
}

/// REQ: F-021
#[test]
fn f_021_markdown_rendered_by_default_is_shown() {
    assert_eq!(
        value_with(
            &[("markdown_rendered_by_default", json!(true))],
            "Markdown default"
        )
        .as_deref(),
        Some("Rendered")
    );
}

/// REQ: F-019
#[test]
fn f_019_autosave_delay_below_the_range_is_clamped_to_250() {
    assert_eq!(
        value_with(&[("autosave_delay_ms", json!(249))], "Autosave delay").as_deref(),
        Some("250")
    );
    assert_eq!(
        value_with(&[("autosave_delay_ms", json!(0))], "Autosave delay").as_deref(),
        Some("250")
    );
}

/// REQ: F-019
#[test]
fn f_019_autosave_delay_at_the_lower_bound_is_kept() {
    assert_eq!(
        value_with(&[("autosave_delay_ms", json!(250))], "Autosave delay").as_deref(),
        Some("250")
    );
}

/// REQ: F-019
#[test]
fn f_019_autosave_delay_at_the_upper_bound_is_kept() {
    assert_eq!(
        value_with(&[("autosave_delay_ms", json!(5000))], "Autosave delay").as_deref(),
        Some("5000")
    );
}

/// REQ: F-019
#[test]
fn f_019_autosave_delay_above_the_range_is_clamped_to_5000() {
    assert_eq!(
        value_with(&[("autosave_delay_ms", json!(5001))], "Autosave delay").as_deref(),
        Some("5000")
    );
    assert_eq!(
        value_with(&[("autosave_delay_ms", json!(60000))], "Autosave delay").as_deref(),
        Some("5000")
    );
}

/// REQ: F-019
#[test]
fn f_019_every_documented_row_is_present_with_a_leader() {
    let mut host = active_host(&[]);
    let screen = settings_screen(&mut host);
    for label in [
        "Long lines",
        "Line numbers",
        "Minimap",
        "Autosave",
        "Autosave delay",
        "Markdown default",
    ] {
        assert!(
            row_value(&screen, label).is_some(),
            "row {label} with a dotted leader"
        );
    }
}

/// REQ: F-019
#[test]
fn f_019_settings_page_keeps_the_stored_values_after_a_reactivation() {
    let mut host = active_host(&[("autosave_delay_ms", json!(2000))]);
    host.deliver(Vec::new());
    let screen = settings_screen(&mut host);
    assert_eq!(
        row_value(&screen, "Autosave delay").as_deref(),
        Some("2000")
    );
}

proptest! {
    // REQ: F-019
    #[test]
    fn f_019_autosave_delay_is_always_shown_inside_250_to_5000(value in 0u32..100_000) {
        let shown = value_with(&[("autosave_delay_ms", json!(value))], "Autosave delay");
        let expected = value.clamp(250, 5000).to_string();
        prop_assert_eq!(shown, Some(expected));
    }
}
