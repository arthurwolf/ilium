//! Sample 3 and the second figure: a `keybinding` row remaps one command, and a `key-capture`
//! field records the next chord and warns when the kernel already binds it.

use ilium_plugin_sdk::prelude::*;
use serde_json::json;

use crate::common::*;

fn capture_view() -> ViewTarget {
    ViewTarget::SettingsPage("keyboard".to_string())
}

fn capture_host() -> ilium_plugin_sdk::testing::FakeHost {
    let mut host = seeded_host();
    seed_sample_labels(&mut host);
    host.keymap().set_label("ctrl+shift+k", "Ctrl+Shift+K");
    host.keymap().set_bindings({
        let mut all = sample_bindings();
        all.push(binding("Terminal", "Paste in pane", "ctrl+shift+k", true));
        all
    });
    host.deliver_one(Event::KeymapChanged);
    host.settle();
    host
}

fn open_capture(host: &mut ilium_plugin_sdk::testing::FakeHost) {
    host.send_action(capture_view(), "capture-split-vertically", ActionKind::Activated, ActionValue::None);
    host.settle();
}

/// REQ: F-007
#[test]
fn capture_007_activating_a_row_opens_the_capture_prompt() {
    let mut host = capture_host();
    open_capture(&mut host);
    let text = host.render(capture_view(), 120, 14).text();
    assert!(text.contains("Press the new key chord."), "{text}");
}

/// REQ: F-007
#[test]
fn capture_007_the_prompt_shows_the_current_binding_of_the_row_above_it() {
    let mut host = capture_host();
    open_capture(&mut host);
    let text = host.render(capture_view(), 120, 14).text();
    let row = text.find("Split vertically").expect("row");
    let prompt = text.find("Press the new key chord.").expect("prompt");
    assert!(row < prompt);
}

/// REQ: F-007
#[test]
fn capture_007_a_captured_chord_is_echoed_in_the_field() {
    let mut host = capture_host();
    open_capture(&mut host);
    host.send_action(capture_view(), "chord", ActionKind::Changed, ActionValue::Text("Ctrl+Shift+J".to_string()));
    host.settle();
    assert!(host.render(capture_view(), 120, 14).text().contains("Ctrl+Shift+J"));
}

/// REQ: F-007
#[test]
fn capture_007_a_chord_the_kernel_already_binds_shows_the_used_by_warning() {
    let mut host = capture_host();
    open_capture(&mut host);
    host.send_action(capture_view(), "chord", ActionKind::Changed, ActionValue::Text("Ctrl+Shift+K".to_string()));
    host.settle();
    let text = host.render(capture_view(), 120, 14).text();
    assert!(text.contains("Ctrl+Shift+K"), "{text}");
    assert!(text.contains("used by Paste in pane"), "{text}");
}

/// REQ: F-007
#[test]
fn capture_007_a_free_chord_shows_no_warning_line() {
    let mut host = capture_host();
    open_capture(&mut host);
    host.send_action(capture_view(), "chord", ActionKind::Changed, ActionValue::Text("Ctrl+Shift+J".to_string()));
    host.settle();
    assert!(!host.render(capture_view(), 120, 14).text().contains("used by"));
}

/// REQ: F-007
#[test]
fn capture_007_the_footer_offers_escape_to_cancel_and_enter_to_apply() {
    let mut host = capture_host();
    open_capture(&mut host);
    assert!(host.render(capture_view(), 120, 14).text().contains("Esc cancel · Enter apply"));
}

/// REQ: F-007
#[test]
fn capture_007_an_empty_captured_chord_clears_the_field() {
    let mut host = capture_host();
    open_capture(&mut host);
    host.send_action(capture_view(), "chord", ActionKind::Changed, ActionValue::Text("Ctrl+Shift+J".to_string()));
    host.send_action(capture_view(), "chord", ActionKind::Changed, ActionValue::Text(String::new()));
    host.settle();
    assert!(!host.render(capture_view(), 120, 14).text().contains("Ctrl+Shift+J"));
}

/// REQ: F-007
#[test]
fn capture_007_escape_cancels_without_a_write() {
    let mut host = capture_host();
    open_capture(&mut host);
    host.send_action(capture_view(), "chord", ActionKind::Changed, ActionValue::Text("Ctrl+Shift+J".to_string()));
    host.key(capture_view(), "Esc");
    host.settle();
    assert!(host.kernel_settings().writes().is_empty());
    assert!(!host.render(capture_view(), 120, 14).text().contains("Press the new key chord."));
}

/// REQ: F-007
#[test]
fn capture_007_enter_applies_a_free_chord_with_one_write_to_the_keybindings_table() {
    let mut host = capture_host();
    open_capture(&mut host);
    host.send_action(capture_view(), "chord", ActionKind::Changed, ActionValue::Text("z".to_string()));
    host.key(capture_view(), "Enter");
    host.settle();
    let writes = host.kernel_settings().writes();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].0, "keyboard");
}

/// REQ: F-007
#[test]
fn capture_007_enter_on_a_chord_used_by_another_action_is_refused_and_writes_nothing() {
    let mut host = capture_host();
    open_capture(&mut host);
    host.send_action(capture_view(), "chord", ActionKind::Changed, ActionValue::Text("Ctrl+Shift+K".to_string()));
    host.key(capture_view(), "Enter");
    host.settle();
    assert!(host.kernel_settings().writes().is_empty());
    assert!(host.render(capture_view(), 120, 14).text().contains("used by Paste in pane"));
}

/// REQ: F-007
#[test]
fn capture_007_only_bindable_keys_are_accepted() {
    for unbindable in ["F5", "Ctrl+Alt+Del", "Tab"] {
        let mut host = capture_host();
        open_capture(&mut host);
        host.send_action(capture_view(), "chord", ActionKind::Changed, ActionValue::Text(unbindable.to_string()));
        host.key(capture_view(), "Enter");
        host.settle();
        assert!(host.kernel_settings().writes().is_empty(), "{unbindable}");
    }
}

/// REQ: F-007
#[test]
fn capture_007_arrow_and_page_keys_are_accepted() {
    for bindable in ["Left", "PgUp", "PgDn", "Space"] {
        let mut host = capture_host();
        open_capture(&mut host);
        host.send_action(capture_view(), "chord", ActionKind::Changed, ActionValue::Text(bindable.to_string()));
        host.key(capture_view(), "Enter");
        host.settle();
        assert_eq!(host.kernel_settings().writes().len(), 1, "{bindable}");
    }
}

/// REQ: F-007
#[test]
fn capture_007_a_non_text_value_for_the_chord_key_is_ignored() {
    let mut host = capture_host();
    open_capture(&mut host);
    host.send_action(capture_view(), "chord", ActionKind::Changed, ActionValue::Number(3.0));
    host.settle();
    assert!(!host.render(capture_view(), 120, 14).text().contains("used by"));
    assert!(host.failures().is_empty());
}

/// REQ: F-007
#[test]
fn capture_007_the_keybinding_row_of_the_manifest_remaps_show_keys_through_the_settings_store() {
    let mut host = capture_host();
    host.settings().set("show_keys", json!("F9")).expect("a settings write is accepted for the row key");
    host.settle();
    assert_eq!(host.settings().get("show_keys"), Some(json!("F9")));
}

/// REQ: F-007
#[test]
fn capture_007_a_second_row_cannot_take_the_chord_of_show_keys() {
    let mut host = capture_host();
    open_capture(&mut host);
    host.send_action(capture_view(), "chord", ActionKind::Changed, ActionValue::Text("?".to_string()));
    host.key(capture_view(), "Enter");
    host.settle();
    assert!(host.kernel_settings().writes().is_empty());
}
