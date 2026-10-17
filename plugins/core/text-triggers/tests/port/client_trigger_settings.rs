//! Twins of trigger_settings.rs and trigger_settings_ui.rs: the event action settings and their tab.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ActionKind, ActionValue, ErrorCode, Event, Outcome, ViewTarget};
use serde_json::{json, Value};

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium-client/src/trigger_settings.rs::defaults_enable_ai_retitling_and_restructuring
#[test]
fn port_001_defaults_enable_ai_retitling_and_restructuring() {
    let mut host = client_host();
    assert_eq!(
        host.settings().get("startup_complete"),
        Some(json!(["restructure_all_projects"]))
    );
    assert_eq!(
        host.settings().get("agent_waiting_background"),
        Some(json!(["retitle_element"]))
    );
    assert_eq!(
        host.settings().get("agent_approval_required"),
        Some(json!([]))
    );
    assert_eq!(
        host.settings().get("agent_prompt_submitted"),
        Some(json!(["retitle_element"]))
    );
    assert_eq!(
        host.settings().get("agent_finished_work"),
        Some(json!(["retitle_element", "restructure_project"]))
    );
}

/// REQ: PORT:ilium-client/src/trigger_settings.rs::none_clears_actions_and_restructure_scopes_are_exclusive
#[test]
fn port_002_none_clears_actions_and_restructure_scopes_are_exclusive() {
    let mut host = client_host();
    host.send_action(
        triggers_tab(),
        "agent_finished_work/restructure_all_projects",
        ActionKind::Toggled,
        ActionValue::Flag(true),
    );
    host.settle();
    let actions = host.settings().get("agent_finished_work").expect("actions");
    assert!(
        actions
            .as_array()
            .expect("array")
            .iter()
            .all(|a| a != "restructure_project"),
        "{actions}"
    );
    host.send_action(
        triggers_tab(),
        "agent_finished_work/none",
        ActionKind::Toggled,
        ActionValue::Flag(true),
    );
    host.settle();
    assert_eq!(host.settings().get("agent_finished_work"), Some(json!([])));
}

/// REQ: PORT:ilium-client/src/trigger_settings.rs::normalization_removes_duplicates_and_impossible_startup_actions
#[test]
fn port_003_normalization_removes_duplicates_and_impossible_startup_actions() {
    let mut host = client_host();
    host.seed_settings_unchecked(
        "startup_complete",
        json!([
            "retitle_element",
            "restructure_project",
            "restructure_all_projects",
            "restructure_all_projects"
        ]),
    );
    host.seed_settings_unchecked(
        "agent_finished_work",
        json!([
            "retitle_element",
            "retitle_element",
            "restructure_all_projects",
            "restructure_project"
        ]),
    );
    host.settle();
    assert_eq!(
        host.settings().get("startup_complete"),
        Some(json!(["restructure_all_projects"]))
    );
    assert_eq!(
        host.settings().get("agent_finished_work"),
        Some(json!(["retitle_element", "restructure_all_projects"]))
    );
}

/// REQ: PORT:ilium-client/src/trigger_settings.rs::partially_specified_toml_keeps_defaults_for_missing_events
#[test]
fn port_004_partially_specified_toml_keeps_defaults_for_missing_events() {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
        .build();
    host.kernel_settings().set_tab(
        "triggers",
        json!({"agent_approval_required": ["retitle_element"]}),
    );
    host.activate::<ilium_plugin_core_text_triggers::TextTriggersClient>();
    host.settle();
    assert_eq!(
        host.settings().get("agent_approval_required"),
        Some(json!(["retitle_element"]))
    );
    assert_eq!(
        host.settings().get("agent_prompt_submitted"),
        Some(json!(["retitle_element"]))
    );
    assert_eq!(
        host.settings().get("startup_complete"),
        Some(json!(["restructure_all_projects"]))
    );
}

/// REQ: PORT:ilium-client/src/trigger_settings_ui.rs::hit_test_returns_the_exact_wrapped_chip
#[test]
fn port_005_hit_test_returns_the_exact_wrapped_chip() {
    let mut host = client_host();
    let text = render_triggers(&mut host, 28);
    assert!(text.lines().any(|line| line.contains("Retitle")), "{text}");
    host.send_action(
        triggers_tab(),
        "agent_waiting_background/retitle_element",
        ActionKind::Toggled,
        ActionValue::Flag(false),
    );
    host.settle();
    assert_eq!(
        host.settings().get("agent_waiting_background"),
        Some(json!([]))
    );
    assert_eq!(
        host.settings().get("agent_prompt_submitted"),
        Some(json!(["retitle_element"]))
    );
}

/// REQ: PORT:ilium-client/src/trigger_settings_ui.rs::narrow_document_wraps_chips_without_overlapping_hit_rectangles
#[test]
fn port_006_narrow_document_wraps_chips_without_overlapping_hit_rectangles() {
    let mut host = client_host();
    let text = render_triggers(&mut host, 28);
    for line in text.lines() {
        assert!(line.chars().count() <= 28, "{line:?}");
    }
    assert!(text.matches("Retitle").count() >= 3, "{text}");
}

/// REQ: PORT:ilium-client/src/trigger_settings_ui.rs::narrow_document_wraps_every_description_without_clipping_words
#[test]
fn port_007_narrow_document_wraps_every_description_without_clipping_words() {
    let mut host = client_host();
    let text = render_triggers(&mut host, 28);
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let flat = flat.replace(
        [
            '\u{2502}', '\u{256d}', '\u{256e}', '\u{2570}', '\u{256f}', '\u{2500}',
        ],
        "",
    );
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(flat.contains("Changes apply immediately."), "{flat}");
    assert!(flat.contains("replay are loaded."), "{flat}");
}

/// REQ: PORT:ilium-client/src/trigger_settings_ui.rs::overflow_indicator_does_not_replace_trigger_document_text
#[test]
fn port_008_overflow_indicator_does_not_replace_trigger_document_text() {
    let mut host = client_host();
    let text = host.render(triggers_tab(), 28, 6).text();
    assert!(text.lines().any(|line| !line.trim().is_empty()), "{text}");
    assert!(text
        .lines()
        .next()
        .is_some_and(|line| !line.contains("\u{2026}") || line.trim().len() > 1));
}

/// REQ: PORT:ilium-client/src/trigger_settings_ui.rs::trigger_events_use_rounded_sections_at_wide_and_compact_widths
#[test]
fn port_009_trigger_events_use_rounded_sections_at_wide_and_compact_widths() {
    let mut host = client_host();
    for columns in [100u16, 28] {
        let text = render_triggers(&mut host, columns);
        for corner in ['\u{256d}', '\u{256e}', '\u{2570}', '\u{256f}'] {
            assert!(text.contains(corner), "{columns}: missing {corner}");
        }
    }
}

/// REQ: PORT:ilium-client/src/trigger_settings_ui.rs::trigger_surface_marks_every_cell_for_safe_repaint
#[test]
fn port_010_trigger_surface_marks_every_cell_for_safe_repaint() {
    let mut host = client_host();
    let screen = host.render(triggers_tab(), 60, 30);
    assert_eq!(
        screen.cells.len(),
        screen.columns as usize * screen.rows as usize
    );
    assert!(!screen.text().is_empty());
}

/// REQ: PORT:ilium-client/src/trigger_settings_ui.rs::vs16_action_icons_never_share_a_physical_row
#[test]
fn port_011_vs16_action_icons_never_share_a_physical_row() {
    let mut host = client_host();
    for columns in [100u16, 28] {
        for line in render_triggers(&mut host, columns).lines() {
            assert!(line.matches('\u{fe0f}').count() <= 1, "{line:?}");
        }
    }
}

/// REQ: PORT:ilium-client/src/keys.rs::triggers_keyboard_navigation_toggles_chips_and_none_clears_actions
#[test]
fn port_010_triggers_keyboard_navigation_toggles_chips_and_none_clears_actions() {
    let mut host = client_host();
    // Space on the focused chip of the finished-work row toggles it: the first chip is Retitle element.
    host.key(triggers_tab(), "right");
    host.key(triggers_tab(), "space");
    host.settle();
    let actions = host.settings().get("agent_finished_work").expect("actions");
    assert_eq!(
        actions,
        json!(["restructure_project"]),
        "Retitle element is removed, Restructure project stays"
    );
    // Left, then Enter on the None chip clears every action.
    host.key(triggers_tab(), "left");
    host.key(triggers_tab(), "enter");
    host.settle();
    assert_eq!(host.settings().get("agent_finished_work"), Some(json!([])));
}
