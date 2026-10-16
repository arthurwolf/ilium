//! Client-side renders (goldens) and the provider chooser write rule.

use ilium_plugin_sdk::testing::assert_golden;
use ilium_plugin_sdk::types::{ActionKind, ActionValue, ViewTarget};
use proptest::prelude::*;

use crate::common::{client_host, golden_path};

fn tab() -> ViewTarget {
    ViewTarget::SettingsPage("inference".to_string())
}

fn dialog() -> ViewTarget {
    ViewTarget::Popup("test-provider".to_string())
}

/// REQ: F-084
#[test]
fn panel_084_the_inference_tab_at_56_columns_matches_the_page_figure() {
    let mut host = client_host();
    let screen = host.render(tab(), 56, 14);
    assert_golden(&golden_path("inference_tab_56.txt"), &screen.text());
}

/// REQ: F-084
#[test]
fn panel_084_the_test_dialog_at_48_columns_matches_the_page_figure() {
    let mut host = client_host();
    let screen = host.render(dialog(), 48, 8);
    assert_golden(&golden_path("test_dialog_48.txt"), &screen.text());
}

/// REQ: F-084
#[test]
fn panel_084_the_last_test_row_starts_as_not_run_yet() {
    let mut host = client_host();
    assert!(host.render(tab(), 80, 20).text().contains("Not run yet"));
}

/// REQ: F-084
#[test]
fn panel_084_a_button_row_offers_test_provider() {
    let mut host = client_host();
    assert!(host.render(tab(), 80, 20).text().contains("Test provider"));
}

/// REQ: COR-033
#[test]
fn panel_033_choosing_a_provider_writes_only_the_provider_key() {
    let mut host = client_host();
    host.send_action(tab(), "provider", ActionKind::Changed, ActionValue::Index(2));
    let writes = host.settings().writes();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].0, "provider");
}

/// REQ: COR-033
#[test]
fn panel_033_unknown_keys_in_the_section_are_kept() {
    let mut host = client_host();
    host.settings().seed(r#"{"future_key": 7}"#);
    host.send_action(tab(), "provider", ActionKind::Changed, ActionValue::Index(1));
    assert_eq!(host.settings().section_value("future_key"), Some(serde_json::json!(7)));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    // REQ: F-084
    #[test]
    fn panel_084_no_render_line_exceeds_the_width(width in 40u16..=120) {
        let mut host = client_host();
        let text = host.render(tab(), width, 20).text();
        for line in text.lines() {
            prop_assert!(line.chars().count() <= width as usize);
        }
    }
}
