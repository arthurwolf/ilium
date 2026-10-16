//! Auto-save (F-090): toggles and choices save at once; text and number inputs save 600 ms after
//! the last edit, or at once on Enter (src/docs/settings.md "Auto-save behaviour").

use ilium_plugin_sdk::types::{ActionKind, ActionValue, ViewTarget};
use serde_json::json;

use crate::common::*;

fn ui_tab() -> ViewTarget {
    ViewTarget::SettingsPage("user-interface".to_string())
}

/// REQ: F-090
#[test]
fn autosave_090_a_toggle_is_saved_at_once() {
    let mut host = client_host();
    host.send_action(ui_tab(), "ui.use_stable_glyphs", ActionKind::Toggled, ActionValue::Flag(true));
    host.settle();
    assert_eq!(host.settings().get("ui.use_stable_glyphs"), Some(json!(true)));
}

/// REQ: F-090
#[test]
fn autosave_090_a_toggle_switched_back_is_saved_at_once() {
    let mut host = client_host();
    host.send_action(ui_tab(), "ui.auto_remove_empty_groups", ActionKind::Toggled, ActionValue::Flag(false));
    host.settle();
    assert_eq!(host.settings().get("ui.auto_remove_empty_groups"), Some(json!(false)));
    host.send_action(ui_tab(), "ui.auto_remove_empty_groups", ActionKind::Toggled, ActionValue::Flag(true));
    host.settle();
    assert_eq!(host.settings().get("ui.auto_remove_empty_groups"), Some(json!(true)));
}

/// REQ: F-090
#[test]
fn autosave_090_a_choice_is_saved_at_once() {
    let mut host = client_host();
    host.send_action(ui_tab(), "ui.motion_level", ActionKind::Changed, ActionValue::Text("off".to_string()));
    host.settle();
    assert_eq!(host.settings().get("ui.motion_level"), Some(json!("off")));
}

/// REQ: F-090
#[test]
fn autosave_090_the_density_choice_of_the_appearance_section_is_saved_at_once() {
    let mut host = client_host();
    host.send_action(ui_tab(), "density", ActionKind::Changed, ActionValue::Text("compact".to_string()));
    host.settle();
    assert_eq!(host.settings().get("density"), Some(json!("compact")));
}

/// REQ: F-090
#[test]
fn autosave_090_a_number_input_is_not_saved_before_600_ms() {
    let mut host = client_host();
    host.send_action(ui_tab(), "ui.last_prompt_max_lines", ActionKind::Changed, ActionValue::Number(7.0));
    host.advance_clock(599);
    host.settle();
    assert_eq!(host.settings().get("ui.last_prompt_max_lines"), Some(json!(4)));
}

/// REQ: F-090
#[test]
fn autosave_090_a_number_input_is_saved_at_600_ms() {
    let mut host = client_host();
    host.send_action(ui_tab(), "ui.last_prompt_max_lines", ActionKind::Changed, ActionValue::Number(7.0));
    host.advance_clock(600);
    host.settle();
    assert_eq!(host.settings().get("ui.last_prompt_max_lines"), Some(json!(7)));
}

/// REQ: F-090
#[test]
fn autosave_090_a_number_input_stays_saved_after_more_time_passes() {
    let mut host = client_host();
    host.send_action(ui_tab(), "ui.last_prompt_max_lines", ActionKind::Changed, ActionValue::Number(7.0));
    host.advance_clock(601);
    host.settle();
    host.advance_clock(10_000);
    host.settle();
    assert_eq!(host.settings().get("ui.last_prompt_max_lines"), Some(json!(7)));
}

/// REQ: F-090
#[test]
fn autosave_090_a_second_edit_restarts_the_600_ms_pause() {
    let mut host = client_host();
    host.send_action(ui_tab(), "ui.last_prompt_max_lines", ActionKind::Changed, ActionValue::Number(5.0));
    host.advance_clock(400);
    host.send_action(ui_tab(), "ui.last_prompt_max_lines", ActionKind::Changed, ActionValue::Number(6.0));
    host.advance_clock(400);
    host.settle();
    assert_eq!(host.settings().get("ui.last_prompt_max_lines"), Some(json!(4)));
    host.advance_clock(200);
    host.settle();
    assert_eq!(host.settings().get("ui.last_prompt_max_lines"), Some(json!(6)));
}

/// REQ: F-090
#[test]
fn autosave_090_enter_commits_a_number_input_without_waiting() {
    let mut host = client_host();
    host.send_action(ui_tab(), "ui.last_prompt_max_lines", ActionKind::Submitted, ActionValue::Number(9.0));
    host.settle();
    assert_eq!(host.settings().get("ui.last_prompt_max_lines"), Some(json!(9)));
}

/// REQ: F-090
#[test]
fn autosave_090_an_out_of_range_number_is_never_saved() {
    let mut host = client_host();
    host.send_action(ui_tab(), "ui.last_prompt_max_lines", ActionKind::Changed, ActionValue::Number(21.0));
    host.advance_clock(600);
    host.settle();
    assert_eq!(host.settings().get("ui.last_prompt_max_lines"), Some(json!(4)));
}

/// REQ: F-090
#[test]
fn autosave_090_a_text_input_is_saved_after_600_ms() {
    let mut host = client_host();
    host.send_action(ui_tab(), "accent", ActionKind::Changed, ActionValue::Text("#ff8800".to_string()));
    host.advance_clock(599);
    host.settle();
    assert_eq!(host.settings().get("accent"), Some(json!("#4a9eff")));
    host.advance_clock(1);
    host.settle();
    assert_eq!(host.settings().get("accent"), Some(json!("#ff8800")));
}

/// REQ: F-090
#[test]
fn autosave_090_the_screen_shows_saved_after_a_change() {
    let mut host = client_host();
    host.send_action(ui_tab(), "ui.use_stable_glyphs", ActionKind::Toggled, ActionValue::Flag(true));
    host.settle();
    let screen = host.render(ui_tab(), 80, 16);
    assert!(screen.text().contains("Saved"), "{}", screen.text());
}

/// REQ: F-090
#[test]
fn autosave_090_a_change_made_by_the_plugin_is_written_without_a_global_save_button() {
    let mut host = client_host();
    host.send_action(ui_tab(), "ui.use_stable_glyphs", ActionKind::Toggled, ActionValue::Flag(true));
    host.settle();
    let screen = host.render(ui_tab(), 80, 16);
    assert!(!screen.text().contains("Save changes"));
    assert_eq!(host.settings().get("ui.use_stable_glyphs"), Some(json!(true)));
}
