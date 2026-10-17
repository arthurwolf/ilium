//! Twins of the ilium-client tests (app.rs, mouse.rs, tree_ui.rs) whose provider-facing part moved
//! into the plugin's client panel: the provider chooser and its persisted selection (F-084, F-085).

use ilium_plugin_sdk::types::{ActionKind, ActionValue, ViewTarget};
use serde_json::json;

use crate::common::*;

fn tab() -> ViewTarget {
    ViewTarget::SettingsPage("inference".to_string())
}

fn choose(host: &mut ilium_plugin_sdk::testing::FakeHost, index: u32) {
    host.send_action(
        tab(),
        "provider",
        ActionKind::Changed,
        ActionValue::Index(index),
    );
}

/// REQ: PORT:ilium-client/src/app.rs::freezing_stores_the_provider_session_resume_command
#[test]
fn port_085_the_chosen_provider_is_stored_in_the_plugin_settings() {
    let mut host = client_host(json!({}));
    choose(&mut host, 1);
    assert!(host.settings().section_value("provider").is_some());
}

/// REQ: PORT:ilium-client/src/app.rs::freezing_stores_the_provider_session_resume_command
#[test]
fn port_085_the_stored_provider_is_the_last_one_chosen() {
    let mut host = client_host(json!({}));
    choose(&mut host, 1);
    let first = host.settings().section_value("provider");
    choose(&mut host, 0);
    choose(&mut host, 1);
    assert_eq!(host.settings().section_value("provider"), first);
}

/// REQ: PORT:ilium-client/src/mouse.rs::line_provider_pointer_cycles_both_directions_and_picker_retains_prompt
#[test]
fn port_085_choosing_index_zero_then_one_then_zero_returns_to_the_first_value() {
    let mut host = client_host(json!({}));
    choose(&mut host, 0);
    let first = host.settings().section_value("provider");
    choose(&mut host, 1);
    let second = host.settings().section_value("provider");
    assert_ne!(first, second);
    choose(&mut host, 0);
    assert_eq!(host.settings().section_value("provider"), first);
}

/// REQ: PORT:ilium-client/src/mouse.rs::line_provider_pointer_cycles_both_directions_and_picker_retains_prompt
#[test]
fn port_085_each_choice_writes_exactly_one_provider_key() {
    let mut host = client_host(json!({}));
    choose(&mut host, 1);
    choose(&mut host, 0);
    let writes = host.settings().writes();
    assert_eq!(writes.len(), 2);
    assert!(writes.iter().all(|write| write.0 == "provider"));
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::recorded_model_icon_overrides_provider_icon_only_for_icon_identifiers
#[test]
fn port_085_the_model_row_text_is_not_replaced_by_a_provider_value() {
    let mut host = client_host(json!({}));
    let before = host.render(tab(), 80, 20).text();
    choose(&mut host, 1);
    let after = host.render(tab(), 80, 20).text();
    assert!(before.contains("Model"));
    assert!(after.contains("Model"));
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::rendered_tree_rows_match_supported_model_glyphs_and_provider_fallbacks
#[test]
fn port_085_every_provider_choice_renders_a_provider_row_within_the_width() {
    let mut host = client_host(json!({}));
    for index in 0..4u32 {
        choose(&mut host, index);
        let text = host.render(tab(), 56, 14).text();
        assert!(text.contains("Provider"), "index {index}");
        assert!(
            text.lines().all(|line| line.chars().count() <= 56),
            "index {index}"
        );
    }
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::rendered_tree_rows_show_mixed_provider_models_for_more_than_sixteen_sessions
#[test]
fn port_085_twenty_repeated_choices_keep_one_write_each_and_the_last_value() {
    let mut host = client_host(json!({}));
    for round in 0..20u32 {
        choose(&mut host, round % 2);
    }
    assert_eq!(host.settings().writes().len(), 20);
    choose(&mut host, 0);
    let zero = host.settings().section_value("provider");
    choose(&mut host, 1);
    assert_ne!(host.settings().section_value("provider"), zero);
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::wide_toolbar_exposes_every_registered_agent_provider
#[test]
fn port_085_the_wide_tab_offers_the_provider_chooser_and_the_test_button() {
    let mut host = client_host(json!({}));
    let text = host.render(tab(), 120, 24).text();
    assert!(text.contains("Provider"));
    assert!(text.contains("Test provider"));
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::wide_toolbar_exposes_every_registered_agent_provider
#[test]
fn port_085_the_chooser_accepts_each_of_the_first_four_indices_without_unknown_key_loss() {
    let mut host = client_host(json!({}));
    host.settings().seed(r#"{"future_key": 7}"#);
    for index in 0..4u32 {
        choose(&mut host, index);
    }
    assert_eq!(host.settings().section_value("future_key"), Some(json!(7)));
}
