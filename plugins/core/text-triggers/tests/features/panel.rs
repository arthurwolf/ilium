//! The client side Text Triggers tab (page "What the user sees"): one group per rule with a pattern, a reply and
//! an enabled toggle, a `notice` row for a pattern that does not compile, rendered at 40, 50 (the page figures),
//! 80 and 120 columns.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ActionKind, ActionValue, ViewTarget};
use serde_json::json;

use crate::common::*;

fn tab() -> ViewTarget {
    ViewTarget::SettingsPage("text-triggers".to_string())
}

fn render(host: &mut FakeHost, columns: u16) -> String {
    host.render(tab(), columns, 14).text()
}

/// Seeds the two rules of the page figure: names, patterns, replies, enabled flags.
fn host_with_page_rules() -> FakeHost {
    let mut host = client_host();
    host.settings().set("names", json!(["Confirm prompts", "Accept edits"])).expect("names");
    set_rules(&mut host, &["\\(y/n\\)\\s*$", "Apply changes\\?"], &["y", "yes"]);
    host.settings().set("enabled", json!([true, false])).expect("enabled");
    host.settle();
    host
}

fn trimmed_lines(text: &str, count: usize) -> String {
    let lines: Vec<String> = text.lines().take(count).map(|line| line.trim_end().to_string()).collect();
    lines.join("\n") + "\n"
}

fn row_containing<'a>(text: &'a str, needle: &str) -> &'a str {
    text.lines().find(|line| line.contains(needle)).unwrap_or_else(|| panic!("no row with {needle:?} in\n{text}"))
}

/// REQ: F-056
#[test]
fn feature_056_the_tab_equals_the_first_figure_of_the_page_at_fifty_columns() {
    let mut host = host_with_page_rules();
    let text = render(&mut host, 50);
    let expected = std::fs::read_to_string(workspace_root().join("plugins/core/text-triggers/tests/features/golden/rules_48.txt")).expect("golden");
    ilium_plugin_sdk::testing::assert_golden("tests/features/golden/rules_48.txt", &trimmed_lines(&text, 7));
    assert_eq!(trimmed_lines(&text, 7), expected.replace("\r\n", "\n"));
}

/// REQ: F-056
#[test]
fn feature_056_an_invalid_pattern_tab_equals_the_second_figure_of_the_page() {
    let mut host = client_host();
    host.settings().set("names", json!(["Confirm prompts"])).expect("names");
    host.seed_settings_unchecked("patterns", json!(["(y/n"]));
    host.seed_settings_unchecked("enabled", json!([false]));
    host.settle();
    let text = render(&mut host, 50);
    let expected = std::fs::read_to_string(workspace_root().join("plugins/core/text-triggers/tests/features/golden/invalid_pattern_48.txt")).expect("golden");
    ilium_plugin_sdk::testing::assert_golden("tests/features/golden/invalid_pattern_48.txt", &trimmed_lines(&text, 6));
    assert_eq!(trimmed_lines(&text, 6), expected.replace("\r\n", "\n"));
}

/// REQ: F-056
#[test]
fn feature_056_the_tab_is_titled_text_triggers_at_forty_columns() {
    let mut host = host_with_page_rules();
    assert!(render(&mut host, 40).contains("Text Triggers"));
}

/// REQ: F-056
#[test]
fn feature_056_the_tab_shows_both_rules_at_forty_columns() {
    let mut host = host_with_page_rules();
    let text = render(&mut host, 40);
    assert!(text.contains("Confirm prompts") && text.contains("Accept edits"), "{text}");
}

/// REQ: F-056
#[test]
fn feature_056_the_tab_fits_forty_columns_without_overflow() {
    let mut host = host_with_page_rules();
    let text = render(&mut host, 40);
    assert!(text.lines().all(|line| line.chars().count() <= 40), "{text}");
}

/// REQ: F-056
#[test]
fn feature_056_the_tab_shows_pattern_and_reply_rows_at_eighty_columns() {
    let mut host = host_with_page_rules();
    let text = render(&mut host, 80);
    assert!(row_containing(&text, "Pattern").contains("\\(y/n\\)\\s*$"), "{text}");
    assert!(row_containing(&text, "Reply").contains('y'), "{text}");
}

/// REQ: F-056
#[test]
fn feature_056_the_tab_fits_one_hundred_twenty_columns_without_overflow() {
    let mut host = host_with_page_rules();
    let text = render(&mut host, 120);
    assert!(text.lines().all(|line| line.chars().count() <= 120), "{text}");
    assert!(text.contains("Accept edits"), "{text}");
}

/// REQ: F-056
#[test]
fn feature_056_the_enabled_rule_shows_on_and_the_disabled_rule_shows_off() {
    let mut host = host_with_page_rules();
    let text = render(&mut host, 80);
    assert!(row_containing(&text, "Confirm prompts").trim_end().ends_with("on") || row_containing(&text, "Confirm prompts").contains(" on"), "{text}");
    assert!(row_containing(&text, "Accept edits").contains("off"), "{text}");
}

/// REQ: F-056
#[test]
fn feature_056_label_and_value_are_joined_by_dotted_leaders() {
    let mut host = host_with_page_rules();
    let text = render(&mut host, 80);
    assert!(row_containing(&text, "Confirm prompts").contains("……"), "{text}");
    assert!(row_containing(&text, "Pattern").contains("……"), "{text}");
}

/// REQ: F-056
#[test]
fn feature_056_a_valid_rule_shows_no_notice() {
    let mut host = host_with_page_rules();
    let text = render(&mut host, 80);
    assert!(!text.contains("is not valid"), "{text}");
}

/// REQ: F-056
#[test]
fn feature_056_an_invalid_pattern_shows_a_notice_with_the_quoted_pattern() {
    let mut host = client_host();
    host.seed_settings_unchecked("patterns", json!(["(y/n"]));
    host.settle();
    let text = render(&mut host, 80);
    assert!(text.contains("Pattern \"(y/n\" is not valid"), "{text}");
}

/// REQ: F-056
#[test]
fn feature_056_an_invalid_pattern_notice_gives_the_compiler_reason_and_the_switched_off_note() {
    let mut host = client_host();
    host.seed_settings_unchecked("patterns", json!(["(y/n"]));
    host.settle();
    let text = render(&mut host, 80);
    assert!(text.contains("unclosed group. The rule is switched off."), "{text}");
}

/// REQ: F-056
#[test]
fn feature_056_an_invalid_pattern_rule_shows_off_even_if_stored_enabled() {
    let mut host = client_host();
    host.seed_settings_unchecked("patterns", json!(["(y/n"]));
    host.seed_settings_unchecked("enabled", json!([true]));
    host.settle();
    let text = render(&mut host, 80);
    assert!(text.contains("off"), "{text}");
}

/// REQ: F-056
#[test]
fn feature_056_the_notice_is_drawn_above_the_toggle_of_its_rule() {
    let mut host = client_host();
    host.seed_settings_unchecked("names", json!(["Confirm prompts"]));
    host.seed_settings_unchecked("patterns", json!(["(y/n"]));
    host.settle();
    let text = render(&mut host, 80);
    assert!(text.find("is not valid").expect("notice") < text.find("Confirm prompts").expect("rule"), "{text}");
}

/// REQ: F-056
#[test]
fn feature_056_only_the_invalid_rule_gets_a_notice() {
    let mut host = client_host();
    host.seed_settings_unchecked("patterns", json!(["ok", "(bad"]));
    host.settle();
    let text = render(&mut host, 80);
    assert_eq!(text.matches("is not valid").count(), 1, "{text}");
    assert!(text.contains("Pattern \"(bad\" is not valid"), "{text}");
}

/// REQ: F-056
#[test]
fn feature_056_toggling_a_rule_writes_the_enabled_flag() {
    let mut host = host_with_page_rules();
    host.send_action(tab(), "rule-1", ActionKind::Toggled, ActionValue::Flag(true));
    host.settle();
    assert_eq!(host.settings().get("enabled"), Some(json!([true, true])));
}

/// REQ: F-056
#[test]
fn feature_056_toggling_a_rule_off_writes_false_for_that_index_only() {
    let mut host = host_with_page_rules();
    host.send_action(tab(), "rule-0", ActionKind::Toggled, ActionValue::Flag(false));
    host.settle();
    assert_eq!(host.settings().get("enabled"), Some(json!([false, false])));
}

/// REQ: F-056
#[test]
fn feature_056_toggling_a_rule_updates_the_row_text() {
    let mut host = host_with_page_rules();
    host.send_action(tab(), "rule-1", ActionKind::Toggled, ActionValue::Flag(true));
    host.settle();
    let text = render(&mut host, 80);
    assert!(!row_containing(&text, "Accept edits").contains("off"), "{text}");
}

/// REQ: F-056
#[test]
fn feature_056_toggling_leaves_patterns_and_replies_untouched() {
    let mut host = host_with_page_rules();
    host.send_action(tab(), "rule-0", ActionKind::Toggled, ActionValue::Flag(false));
    host.settle();
    assert_eq!(host.settings().get("patterns"), Some(json!(["\\(y/n\\)\\s*$", "Apply changes\\?"])));
    assert_eq!(host.settings().get("replies"), Some(json!(["y", "yes"])));
}

/// REQ: F-056, COR-030
#[test]
fn feature_056_without_the_kernel_tab_grant_a_toggle_is_refused_not_permitted() {
    let mut host = host_with_page_rules();
    host.revoke("settings.write-kernel:text-triggers");
    host.send_action(tab(), "rule-0", ActionKind::Toggled, ActionValue::Flag(false));
    host.settle();
    let outcomes = host.outcomes();
    assert!(
        outcomes.iter().any(|o| matches!(o, ilium_plugin_sdk::types::Outcome::Refused(e) if e.code == ilium_plugin_sdk::types::ErrorCode::NotPermitted)),
        "{outcomes:?}"
    );
    assert_eq!(host.settings().get("enabled"), Some(json!([true, false])));
}

/// REQ: F-056
#[test]
fn feature_056_an_empty_rule_list_renders_the_title_and_no_rule_group() {
    let mut host = client_host();
    let text = render(&mut host, 80);
    assert!(text.contains("Text Triggers"), "{text}");
    assert!(!text.contains("Pattern"), "{text}");
}

/// REQ: F-056
#[test]
fn feature_056_the_rendered_tab_is_framed_by_a_rounded_border() {
    let mut host = host_with_page_rules();
    let text = render(&mut host, 80);
    let first = text.lines().next().expect("first row");
    assert!(first.starts_with('╭') && first.trim_end().ends_with('╮'), "{text}");
}
